//! Генератор LLVM IR для Goraw + семантические проверки в один проход.
//! Сигнатуры и структуры уже собраны в `TyCtx` (первый проход), поэтому
//! здесь поддержаны forward-ссылки. При любой ошибке в Diags модуль
//! продолжает работу с «отравленным» типом `Ty::Err`, гася каскады, а
//! итоговый IR вызывающая сторона просто выбрасывает, если были ошибки.

use crate::ast::*;
use crate::diag::{Diagnostic, Diags, Span};
use crate::types::{Ty, TyCtx};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct Local {
    slot: String, // имя alloca-слота, напр. "%sum.0"
    ty: Ty,
    mutable: bool,
    /// Безопасная ссылка (приёмник метода `self`): доступ к полям через неё
    /// не требует `unsafe`, хотя тип — сырой указатель.
    safe: bool,
    order: usize, // Порядок объявления для детерминированного LIFO-уничтожения (RAII)
}

pub struct Codegen<'a> {
    ctx: &'a TyCtx,
    diags: &'a mut Diags,

    strings: String,     // глобальные строковые константы
    body: String,        // определения функций
    allocas: String,     // alloca текущей функции (буфер входного блока)
    code: String,        // тело текущей функции после allocas

    tmp: u32,
    label: u32,
    strcount: u32,
    slotcount: u32,

    cur_ret: Ty,
    cur_unsafe: bool,   // вся функция помечена unsafe
    unsafe_depth: u32,  // вложенность unsafe { }

    scopes: Vec<HashMap<String, Local>>,
    loops: Vec<(String, String)>, // (continue-label, break-label)
    terminated: bool,             // текущий блок уже завершён терминатором
    order_counter: usize,         // счётчик для LIFO порядка деструкторов
    intrinsics: HashSet<String>,  // declare-строки использованных LLVM-интринзиков
    // Пока генерируется IR-ШАБЛОН jit-блока: захваченные имена -> (индекс, тип).
    // Их чтение выдаёт плейсхолдер `$CAPi$`, который рантайм заменит на константу.
    captures: Option<HashMap<String, (usize, Ty)>>,
    in_jit_template: bool,
    jittmpl_count: u32,
    in_test: bool, // тело текущей функции — тест (разрешён assert)
    /// Свёрнутые глобальные константы: имя -> значение.
    consts: HashMap<String, CVal>,
    /// Глобальные static-переменные: имя -> (LLVM-символ, тип).
    statics: HashMap<String, (String, Ty)>,
    /// Определения глобалов для заголовка.
    globals: String,
    /// Включена ли встроенная обфускация строк для всех строковых литералов.
    obfuscate_strings: bool,
    /// Счётчик обфусцированных строк.
    obf_count: u32,
    /// Сгенерирован ли рантайм расшифровки строк.
    has_obf_decrypt_runtime: bool,
    /// Целевой triple для заголовка LLVM IR.
    target_triple: String,
}

/// Значение константы, свёрнутое в компайл-тайме.
#[derive(Clone)]
enum CVal {
    Int(i64, Ty),
    Float(f64, Ty),
    Bool(bool),
}

impl CVal {
    fn ty(&self) -> Ty {
        match self {
            CVal::Int(_, t) => t.clone(),
            CVal::Float(_, t) => t.clone(),
            CVal::Bool(_) => Ty::Bool,
        }
    }
    fn render(&self) -> String {
        match self {
            CVal::Int(n, _) => n.to_string(),
            CVal::Float(f, t) => fmt_float(*f, t),
            CVal::Bool(b) => if *b { "true".into() } else { "false".into() },
        }
    }
}

impl<'a> Codegen<'a> {
    pub fn new(ctx: &'a TyCtx, diags: &'a mut Diags) -> Codegen<'a> {
        Codegen {
            ctx,
            diags,
            strings: String::new(),
            body: String::new(),
            allocas: String::new(),
            code: String::new(),
            tmp: 0,
            label: 0,
            strcount: 0,
            slotcount: 0,
            cur_ret: Ty::Void,
            cur_unsafe: false,
            unsafe_depth: 0,
            scopes: Vec::new(),
            loops: Vec::new(),
            terminated: false,
            order_counter: 0,
            intrinsics: HashSet::new(),
            captures: None,
            in_jit_template: false,
            jittmpl_count: 0,
            in_test: false,
            consts: HashMap::new(),
            statics: HashMap::new(),
            globals: String::new(),
            obfuscate_strings: false,
            obf_count: 0,
            has_obf_decrypt_runtime: false,
            target_triple: "x86_64-w64-windows-gnu".to_string(),
        }
    }

    pub fn with_target_triple(mut self, target: String) -> Self {
        self.target_triple = target;
        self
    }

    pub fn with_obfuscate_strings(mut self, obf: bool) -> Self {
        self.obfuscate_strings = obf;
        self
    }

    pub fn set_obfuscate_strings(&mut self, obf: bool) {
        self.obfuscate_strings = obf;
    }

    #[inline]
    fn next_order(&mut self) -> usize {
        self.order_counter += 1;
        self.order_counter
    }

    // ---------- сборка модуля ----------

    pub fn emit_module(mut self, prog: &Program) -> String {
        // Свёртка глобальных констант (до тел — их подставляют по имени).
        for c in &prog.consts {
            let expected = c.ty.as_ref().map(|t| self.resolve(t));
            match self.eval_const(&c.value, expected.as_ref()) {
                Some(cv) => {
                    let ty = cv.ty();
                    if let Some(want) = &expected {
                        if *want != ty && ty != Ty::Err && *want != Ty::Err {
                            self.err("E0083", c.value.span(), format!("тип константы `{}` не совпадает с объявленным `{}`", ty.name(), want.name()), None);
                        }
                    }
                    self.consts.insert(c.name.clone(), cv);
                }
                None => {
                    self.err("E0084", c.value.span(), format!("`const {}` должна быть константным выражением", c.name), Some("допустимы литералы, арифметика над ними, enum-варианты и `as`"));
                    self.consts.insert(c.name.clone(), CVal::Int(0, Ty::Err));
                }
            }
        }

        // Глобальные static-переменные с константной инициализацией.
        for s in &prog.statics {
            let ty = self.resolve(&s.ty);
            let sym = llvm_global(&format!("g.{}", s.name));
            let init = match self.eval_const(&s.value, Some(&ty)) {
                Some(cv) => {
                    if !compat(&ty, &cv.ty()) && cv.ty() != Ty::Err && ty != Ty::Err {
                        self.err("E0088", s.value.span(), format!("инициализатор static типа `{}`, а объявлен `{}`", cv.ty().name(), ty.name()), None);
                    }
                    cv.render()
                }
                None => {
                    self.err("E0089", s.value.span(), format!("`static {}` требует константный инициализатор", s.name), None);
                    self.zero_of(&ty)
                }
            };
            self.globals.push_str(&format!("{sym} = global {} {init}\n", ty.llvm()));
            self.statics.insert(s.name.clone(), (sym, ty));
        }

        // Сначала генерируем тела функций — попутно собираются использованные
        // строковые константы и LLVM-интринзики, нужные для заголовка.
        for f in &prog.fns {
            if !f.is_extern {
                self.gen_fn(f);
            }
        }

        let mut header = String::new();
        header.push_str("; Goraw -> LLVM IR\n");
        header.push_str(&format!("target triple = \"{}\"\n\n", self.target_triple));

        // Тип среза (fat-pointer): указатель на элементы + длина.
        header.push_str("%slice = type { ptr, i64 }\n\n");

        // Определения структур в порядке объявления.
        for name in &self.ctx.struct_order {
            let info = &self.ctx.structs[name];
            let fields: Vec<String> = info.fields.iter().map(|(_, t)| t.llvm()).collect();
            let st_ty = Ty::Struct(name.clone()).llvm();
            header.push_str(&format!("{st_ty} = type {{ {} }}\n", fields.join(", ")));
        }
        if !self.ctx.struct_order.is_empty() {
            header.push('\n');
        }

        // extern-объявления.
        let mut emitted_externs = HashSet::new();
        for f in &prog.fns {
            if f.is_extern {
                if !emitted_externs.insert(&f.name) {
                    continue;
                }
                let sig = &self.ctx.fns[&f.name];
                let params: Vec<String> = sig.params.iter().map(|t| t.llvm()).collect();
                let mut plist = params.join(", ");
                if sig.variadic {
                    if plist.is_empty() {
                        plist.push_str("...");
                    } else {
                        plist.push_str(", ...");
                    }
                }
                let fn_sym = llvm_global(&f.name);
                header.push_str(&format!("declare {} {fn_sym}({})\n", sig.ret.llvm(), plist));
            }
        }


        // Объявления использованных интринзиков (в стабильном порядке).
        let mut intr: Vec<&String> = self.intrinsics.iter().collect();
        intr.sort();
        for d in intr {
            header.push_str(d);
            header.push('\n');
        }
        header.push('\n');

        let mut out = header;
        if !self.globals.is_empty() {
            out.push_str(&self.globals);
            out.push('\n');
        }
        out.push_str(&self.strings);
        if !self.strings.is_empty() {
            out.push('\n');
        }
        out.push_str(&self.body);

        if self.has_obf_decrypt_runtime {
            out.push_str("\n; Goraw String Decryption Runtime\n");
            out.push_str(OBF_DECRYPT_IR);
            out.push('\n');
        }

        out
    }

    // ---------- функции ----------

    fn gen_fn(&mut self, f: &FnDef) {
        // Сигнатуру строим прямо из объявления — это работает и для top-level
        // функций, и для внутренней функции jit-блока (её нет в ctx.fns).
        // Типы уже проверены на первом проходе, поэтому ошибки resolve глушим.
        let param_tys: Vec<Ty> = f
            .params
            .iter()
            .map(|p| {
                let mut junk = Vec::new();
                self.ctx.resolve(&p.ty, &mut junk)
            })
            .collect();
        let ret_ty = match &f.ret {
            Some(t) => {
                let mut junk = Vec::new();
                self.ctx.resolve(t, &mut junk)
            }
            None => Ty::Void,
        };

        self.tmp = 0;
        self.label = 0;
        self.slotcount = 0;
        self.allocas.clear();
        self.code.clear();
        self.cur_ret = ret_ty.clone();
        self.cur_unsafe = f.is_unsafe;
        self.in_test = f.is_test;
        self.unsafe_depth = 0;
        self.scopes.clear();
        self.loops.clear();
        self.terminated = false;
        self.order_counter = 0;
        self.scopes.push(HashMap::new());

        // Сигнатура.
        let mut params_sig = Vec::new();
        for (p, pty) in f.params.iter().zip(param_tys.iter()) {
            let arg_ident = llvm_local(&format!("arg.{}", p.name));
            params_sig.push(format!("{} {arg_ident}", pty.llvm()));
        }
        let fn_sym = llvm_global(&f.name);
        let inline_attr = if f.name != "main" && !f.is_test {
            if let Some(body) = &f.body {
                if body.stmts.len() <= 6 {
                    " alwaysinline"
                } else {
                    " inlinehint"
                }
            } else {
                ""
            }
        } else {
            ""
        };
        self.body.push_str(&format!(
            "define {} {fn_sym}({}){inline_attr} {{\n",
            ret_ty.llvm(),
            params_sig.join(", ")
        ));

        // Пролог: слоты под параметры.
        for (p, pty) in f.params.iter().zip(param_tys.iter()) {
            let slot = self.fresh_slot(&p.name);
            self.alloca(&slot, pty);
            let arg_ident = llvm_local(&format!("arg.{}", p.name));
            self.emit(format!("store {ty} {arg_ident}, ptr {slot}", ty = pty.llvm()));
            let is_self = p.name == "self" && matches!(pty, Ty::Ptr(..));
            let order = self.next_order();
            self.scopes
                .last_mut()
                .unwrap()
                .insert(p.name.clone(), Local { slot, ty: pty.clone(), mutable: false, safe: is_self, order });
        }

        if let Some(body) = &f.body {
            self.gen_block(body);
        }

        // Финальный терминатор, если провалились в конец.
        if !self.terminated {
            if f.is_test {
                // Дошли до конца теста — все assert прошли: возвращаем 0 (ок).
                self.emit("ret i64 0".into());
            } else {
            let cur_ret = self.cur_ret.clone();
            match &cur_ret {
                Ty::Void => {
                    self.emit_drops_for_all_scopes(None);
                    self.emit("ret void".into());
                }
                other => {
                    if f.body.is_some() {
                        self.diags.push(
                            Diagnostic::error(
                                "E0040",
                                f.span,
                                format!("функция `{}` должна вернуть значение типа `{}` на всех путях", f.name, other.name()),
                            )
                            .with_hint("добавьте `return <значение>;` в конце"),
                        );
                    }
                    // Заполнитель, чтобы IR оставался валидным (не используется —
                    // при ошибках модуль не компилируется).
                    let z = self.zero_of(other);
                    self.emit_drops_for_all_scopes(None);
                    self.emit(format!("ret {} {}", other.llvm(), z));
                }
            }
            }
        }

        // Склейка: entry + allocas + code.
        self.body.push_str("entry:\n");
        self.body.push_str(&self.allocas);
        self.body.push_str(&self.code);
        self.body.push_str("}\n\n");
    }

    // ---------- инструкции ----------

    fn gen_block(&mut self, b: &Block) {
        self.scopes.push(HashMap::new());
        for s in &b.stmts {
            if self.terminated {
                // Код после return/break — недостижим; тихо пропускаем.
                break;
            }
            self.gen_stmt(s);
        }
        if !self.terminated {
            self.emit_drops_for_current_scope();
        }
        self.scopes.pop();
    }

    fn gen_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let { name, mutable, ty, value, span } => {
                let expected = ty.as_ref().map(|t| self.resolve(t));
                let (val, vty) = self.gen_expr(value, expected.as_ref());
                let var_ty = match &expected {
                    Some(t) => {
                        if !compat(t, &vty) && vty != Ty::Err && *t != Ty::Err {
                            self.err(
                                "E0033",
                                value.span(),
                                format!(
                                    "тип значения `{}` не совпадает с объявленным `{}`",
                                    vty.name(),
                                    t.name()
                                ),
                                Some("приведите значение через `as` или измените аннотацию"),
                            );
                        }
                        t.clone()
                    }
                    None => vty.clone(),
                };
                if var_ty == Ty::Void {
                    self.err("E0035", *span, "нельзя объявить переменную типа void".into(), None);
                }
                let slot = self.fresh_slot(name);
                self.alloca(&slot, &var_ty);
                self.store_value(&var_ty, &val, &vty, &slot);
                let order = self.next_order();
                self.scopes.last_mut().unwrap().insert(
                    name.clone(),
                    Local { slot, ty: var_ty, mutable: *mutable, safe: false, order },
                );
            }

            Stmt::Assign { target, value, .. } => {
                let (ptr, ty, mutable) = self.gen_lvalue(target);
                if !mutable && ty != Ty::Err {
                    self.err(
                        "E0034",
                        target.span(),
                        "присваивание в неизменяемое место".into(),
                        Some("объявите переменную как `let mut` или используйте `*mut` указатель"),
                    );
                }
                let (val, vty) = self.gen_expr(value, Some(&ty));
                if !compat(&ty, &vty) && ty != Ty::Err && vty != Ty::Err {
                    self.err(
                        "E0036",
                        value.span(),
                        format!("нельзя присвоить `{}` в место типа `{}`", vty.name(), ty.name()),
                        None,
                    );
                }
                self.store_value(&ty, &val, &vty, &ptr);
            }

            Stmt::Expr(e) => {
                let _ = self.gen_expr(e, None);
            }

            Stmt::Return(value, span) => {
                match (value, &self.cur_ret.clone()) {
                    (Some(e), Ty::Void) => {
                        self.err("E0041", e.span(), "функция ничего не возвращает, а `return` со значением".into(), None);
                        self.emit_drops_for_all_scopes(None);
                        self.emit("ret void".into());
                    }
                    (None, Ty::Void) => {
                        self.emit_drops_for_all_scopes(None);
                        self.emit("ret void".into());
                    }
                    (None, ret) => {
                        self.err(
                            "E0042",
                            *span,
                            format!("`return` без значения, а функция возвращает `{}`", ret.name()),
                            None,
                        );
                        self.emit_drops_for_all_scopes(None);
                        let z = self.zero_of(ret);
                        self.emit(format!("ret {} {}", ret.llvm(), z));
                    }
                    (Some(e), ret) => {
                        let (val, vty) = self.gen_expr(e, Some(ret));
                        if !compat(ret, &vty) && vty != Ty::Err {
                            self.err(
                                "E0043",
                                e.span(),
                                format!("возвращается `{}`, а ожидается `{}`", vty.name(), ret.name()),
                                None,
                            );
                        }
                        if is_aggregate(ret) {
                            // структура по значению: грузим агрегат из адреса до вызова деструкторов!
                            let t = self.fresh_tmp();
                            self.emit(format!("{t} = load {ty}, ptr {val}", ty = ret.llvm()));
                            self.emit_drops_for_all_scopes(Some(&val));
                            self.emit(format!("ret {} {}", ret.llvm(), t));
                        } else {
                            self.emit_drops_for_all_scopes(None);
                            self.emit(format!("ret {} {}", ret.llvm(), val));
                        }
                    }
                }
                self.terminated = true;
            }

            Stmt::If { cond, then, els, .. } => {
                let (c, cty) = self.gen_expr(cond, Some(&Ty::Bool));
                self.expect_bool(&cty, cond.span());
                let then_l = self.fresh_label("then");
                let end_l = self.fresh_label("endif");
                let else_l = if els.is_some() { self.fresh_label("else") } else { end_l.clone() };
                self.emit(format!("br i1 {c}, label %{then_l}, label %{else_l}"));

                self.emit_label(&then_l);
                self.gen_block(then);
                if !self.terminated {
                    self.emit(format!("br label %{end_l}"));
                }

                if let Some(eb) = els {
                    self.emit_label(&else_l);
                    self.gen_block(eb);
                    if !self.terminated {
                        self.emit(format!("br label %{end_l}"));
                    }
                }

                self.emit_label(&end_l);
            }

            Stmt::While { cond, body, .. } => {
                let cond_l = self.fresh_label("wcond");
                let body_l = self.fresh_label("wbody");
                let end_l = self.fresh_label("wend");
                self.emit(format!("br label %{cond_l}"));
                self.emit_label(&cond_l);
                let (c, cty) = self.gen_expr(cond, Some(&Ty::Bool));
                self.expect_bool(&cty, cond.span());
                self.emit(format!("br i1 {c}, label %{body_l}, label %{end_l}"));
                self.emit_label(&body_l);
                self.loops.push((cond_l.clone(), end_l.clone()));
                self.gen_block(body);
                self.loops.pop();
                if !self.terminated {
                    self.emit(format!("br label %{cond_l}"));
                }
                self.emit_label(&end_l);
            }

            Stmt::For { init, cond, post, body, .. } => {
                // init получает собственную область видимости.
                self.scopes.push(HashMap::new());
                if let Some(i) = init {
                    self.gen_stmt(i);
                }
                let cond_l = self.fresh_label("fcond");
                let body_l = self.fresh_label("fbody");
                let post_l = self.fresh_label("fpost");
                let end_l = self.fresh_label("fend");
                self.emit(format!("br label %{cond_l}"));
                self.emit_label(&cond_l);
                match cond {
                    Some(c) => {
                        let (cv, cty) = self.gen_expr(c, Some(&Ty::Bool));
                        self.expect_bool(&cty, c.span());
                        self.emit(format!("br i1 {cv}, label %{body_l}, label %{end_l}"));
                    }
                    None => self.emit(format!("br label %{body_l}")),
                }
                self.emit_label(&body_l);
                self.loops.push((post_l.clone(), end_l.clone()));
                self.gen_block(body);
                self.loops.pop();
                if !self.terminated {
                    self.emit(format!("br label %{post_l}"));
                }
                self.emit_label(&post_l);
                if let Some(p) = post {
                    self.gen_stmt(p);
                }
                if !self.terminated {
                    self.emit(format!("br label %{cond_l}"));
                }
                self.emit_label(&end_l);
                self.scopes.pop();
            }

            Stmt::ForIn { var, iter, body, span } => {
                self.scopes.push(HashMap::new());
                let (saddr, sty) = self.gen_expr(iter, None);
                let elem = match sty {
                    Ty::Slice(e) => *e,
                    Ty::Err => {
                        self.scopes.pop();
                        return;
                    }
                    other => {
                        self.err("E0081", *span, format!("`for .. in` работает по срезу `[]T`, а тут `{}`", other.name()), None);
                        self.scopes.pop();
                        return;
                    }
                };
                // длина и указатель на данные (снимок на входе в цикл).
                let lenp = self.fresh_tmp();
                self.emit(format!("{lenp} = getelementptr %slice, ptr {saddr}, i32 0, i32 1"));
                let slen = self.fresh_tmp();
                self.emit(format!("{slen} = load i64, ptr {lenp}"));
                let datap = self.fresh_tmp();
                self.emit(format!("{datap} = getelementptr %slice, ptr {saddr}, i32 0, i32 0"));
                let data = self.fresh_tmp();
                self.emit(format!("{data} = load ptr, ptr {datap}"));

                let islot = self.fresh_slot("foridx");
                self.alloca(&islot, &Ty::I64);
                self.emit(format!("store i64 0, ptr {islot}"));
                let vslot = self.fresh_slot(var);
                self.alloca(&vslot, &elem);
                let order = self.next_order();
                self.scopes.last_mut().unwrap().insert(
                    var.clone(),
                    Local { slot: vslot.clone(), ty: elem.clone(), mutable: false, safe: false, order },
                );

                let cond_l = self.fresh_label("ficond");
                let body_l = self.fresh_label("fibody");
                let post_l = self.fresh_label("fipost");
                let end_l = self.fresh_label("fiend");
                self.emit(format!("br label %{cond_l}"));
                self.emit_label(&cond_l);
                let iv = self.fresh_tmp();
                self.emit(format!("{iv} = load i64, ptr {islot}"));
                let c = self.fresh_tmp();
                self.emit(format!("{c} = icmp slt i64 {iv}, {slen}"));
                self.emit(format!("br i1 {c}, label %{body_l}, label %{end_l}"));
                self.emit_label(&body_l);
                // var = data[i]
                let ep = self.fresh_tmp();
                self.emit(format!("{ep} = getelementptr {ety}, ptr {data}, i64 {iv}", ety = elem.llvm()));
                if is_aggregate(&elem) {
                    self.store_value(&elem, &ep, &elem, &vslot);
                } else {
                    let ev = self.fresh_tmp();
                    self.emit(format!("{ev} = load {ety}, ptr {ep}", ety = elem.llvm()));
                    self.emit(format!("store {ety} {ev}, ptr {vslot}", ety = elem.llvm()));
                }
                self.loops.push((post_l.clone(), end_l.clone()));
                self.gen_block(body);
                self.loops.pop();
                if !self.terminated {
                    self.emit(format!("br label %{post_l}"));
                }
                self.emit_label(&post_l);
                let iv2 = self.fresh_tmp();
                self.emit(format!("{iv2} = load i64, ptr {islot}"));
                let iv3 = self.fresh_tmp();
                self.emit(format!("{iv3} = add i64 {iv2}, 1"));
                self.emit(format!("store i64 {iv3}, ptr {islot}"));
                self.emit(format!("br label %{cond_l}"));
                self.emit_label(&end_l);
                self.scopes.pop();
            }

            Stmt::Break(span) => match self.loops.last() {
                Some((_, brk)) => {
                    let brk = brk.clone();
                    self.emit(format!("br label %{brk}"));
                    self.terminated = true;
                }
                None => self.err("E0044", *span, "`break` вне цикла".into(), None),
            },

            Stmt::Continue(span) => match self.loops.last() {
                Some((cont, _)) => {
                    let cont = cont.clone();
                    self.emit(format!("br label %{cont}"));
                    self.terminated = true;
                }
                None => self.err("E0045", *span, "`continue` вне цикла".into(), None),
            },

            Stmt::Unsafe(block, _) => {
                self.unsafe_depth += 1;
                self.gen_block(block);
                self.unsafe_depth -= 1;
            }

            Stmt::Match { scrut, arms, span } => {
                let (v, vty) = self.gen_expr(scrut, None);
                if !vty.is_int() && vty != Ty::Err {
                    self.err("E0100", *span, format!("`match` работает по целым/enum, а тут `{}`", vty.name()), None);
                    return;
                }
                let end_l = self.fresh_label("mend");
                let mut default_l = end_l.clone();
                let mut arm_infos: Vec<(String, &Block)> = Vec::new();
                let mut cases: Vec<(i64, String)> = Vec::new();
                let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();

                for (pat, body) in arms {
                    let lbl = self.fresh_label("marm");
                    match pat {
                        None => default_l = lbl.clone(),
                        Some(e) => match self.eval_const(e, Some(&vty)) {
                            Some(CVal::Int(n, _)) => {
                                if seen.insert(n) {
                                    cases.push((n, lbl.clone()));
                                }
                            }
                            _ => self.err("E0101", e.span(), "паттерн match должен быть целочисленной константой или enum-вариантом".into(), None),
                        },
                    }
                    arm_infos.push((lbl, body));
                }

                // Инструкция switch (многострочная — пишем напрямую).
                let mut sw = format!("  switch {ty} {v}, label %{default_l} [\n", ty = vty.llvm());
                for (n, lbl) in &cases {
                    sw.push_str(&format!("    {ty} {n}, label %{lbl}\n", ty = vty.llvm()));
                }
                sw.push_str("  ]\n");
                self.code.push_str(&sw);
                self.terminated = true;

                // Конец достижим, если switch-default идёт в end (нет `_`)
                // или хотя бы одна ветвь проваливается в end.
                let mut reaches_end = default_l == end_l;
                for (lbl, body) in &arm_infos {
                    self.emit_label(lbl);
                    self.gen_block(body);
                    if !self.terminated {
                        self.emit(format!("br label %{end_l}"));
                        reaches_end = true;
                    }
                }
                self.emit_label(&end_l);
                if !reaches_end {
                    // Все ветви завершились — end недостижим.
                    self.emit("unreachable".into());
                    self.terminated = true;
                }
            }

            Stmt::Assert(expr, span) => {
                if !self.in_test {
                    self.err("E0082", *span, "`assert` допустим только внутри test-блока".into(), Some("оберните проверку в `test \"имя\" { ... }`"));
                    return;
                }
                let (c, cty) = self.gen_expr(expr, Some(&Ty::Bool));
                self.expect_bool(&cty, expr.span());
                // при ложности тест возвращает номер строки упавшего assert (!=0).
                let okl = self.fresh_label("asok");
                let faill = self.fresh_label("asfail");
                self.emit(format!("br i1 {c}, label %{okl}, label %{faill}"));
                self.emit_label(&faill);
                self.emit(format!("ret i64 {}", expr.span().lo.line));
                self.emit_label(&okl);
            }

            Stmt::Asm(a) => self.gen_asm(a),
            Stmt::InlineC { is_cpp, inputs, outputs, body, span } => {
                self.gen_inline_c(*is_cpp, inputs, outputs, body, *span);
            }
        }
    }

    // ---------- RAII / Деструкторы ----------

    fn emit_drop_for_local(&mut self, local: &Local) {
        if let Ty::Struct(sname) = &local.ty {
            let drop_fn = format!("{sname}__drop");
            if self.ctx.fns.contains_key(&drop_fn) {
                let sym = llvm_global(&drop_fn);
                self.emit(format!("call void {sym}(ptr {})", local.slot));
            }
        }
    }

    fn emit_drops_for_current_scope(&mut self) {
        let mut locals: Vec<Local> = self.scopes.last().map(|s| s.values().cloned().collect()).unwrap_or_default();
        // LIFO порядок вызова деструкторов (обратный объявлению)
        locals.sort_by_key(|l| std::cmp::Reverse(l.order));
        for local in &locals {
            self.emit_drop_for_local(local);
        }
    }

    fn emit_drops_for_all_scopes(&mut self, skip_slot: Option<&str>) {
        let mut locals: Vec<Local> = self.scopes.iter().rev().flat_map(|s| s.values().cloned()).collect();
        // LIFO порядок вызова деструкторов (обратный объявлению)
        locals.sort_by_key(|l| std::cmp::Reverse(l.order));
        for local in &locals {
            if let Some(skip) = skip_slot {
                if local.slot == skip {
                    continue;
                }
            }
            self.emit_drop_for_local(local);
        }
    }

    // ---------- инлайн C / C++ ----------

    fn gen_inline_c(
        &mut self,
        is_cpp: bool,
        inputs: &[String],
        outputs: &[String],
        _body: &str,
        span: Span,
    ) {
        let mut arg_vals = Vec::new();
        let mut arg_types = Vec::new();

        for inp in inputs {
            let (v, ty) = self.gen_expr(&Expr::Ident(inp.clone(), span), None);
            arg_vals.push(format!("{} {v}", ty.llvm()));
            arg_types.push(ty.llvm());
        }

        for out in outputs {
            let (ptr, _ty, _) = self.gen_lvalue(&Expr::Ident(out.clone(), span));
            arg_vals.push(format!("ptr {ptr}"));
            arg_types.push("ptr".into());
        }

        self.tmp += 1;
        let fn_name = format!("__gw_inline_{}_{}", if is_cpp { "cpp" } else { "c" }, self.tmp);
        let proto = format!("declare void @{fn_name}({})", arg_types.join(", "));
        self.intrinsics.insert(proto);

        self.emit(format!("call void @{fn_name}({})", arg_vals.join(", ")));
    }

    // ---------- инлайн-ассемблер (Intel/MASM/NASM -> LLVM inline asm) ----------

    fn gen_asm(&mut self, a: &AsmBlock) {
        self.require_unsafe(a.span, "инлайн-ассемблер");

        // Собираем ограничения (constraints) и операнды. Модель простая:
        // каждый выход — "=r" (регистр), каждый вход — "r". Внутри тела на
        // операнды ссылаются по имени переменной ($name), которое мы заменяем
        // на позиционные $0,$1,... как того требует LLVM inline asm.
        let mut constraints: Vec<String> = Vec::new();
        let mut call_args: Vec<String> = Vec::new();
        let mut name_to_index: HashMap<String, usize> = HashMap::new();
        let mut out_slots: Vec<(String, Ty)> = Vec::new();
        let mut idx = 0usize;

        for out in &a.outputs {
            if let Some(l) = self.lookup(out).cloned() {
                constraints.push("=r".into());
                name_to_index.insert(out.clone(), idx);
                out_slots.push((l.slot.clone(), l.ty.clone()));
                idx += 1;
            } else {
                self.err("E0050", a.span, format!("неизвестная выходная переменная `{out}`"), None);
            }
        }
        for inp in &a.inputs {
            if let Some(l) = self.lookup(inp).cloned() {
                let t = self.fresh_tmp();
                self.emit(format!("{t} = load {ty}, ptr {slot}", ty = l.ty.llvm(), slot = l.slot));
                constraints.push("r".into());
                call_args.push(format!("{} {}", l.ty.llvm(), t));
                name_to_index.insert(inp.clone(), idx);
                idx += 1;
            } else {
                self.err("E0051", a.span, format!("неизвестная входная переменная `{inp}`"), None);
            }
        }

        // Текст ассемблера: заменяем $name на $0.. и приводим MASM-комментарии
        // (`;`) к пустым строкам, экранируем спецсимволы для LLVM.
        let mut clobbers: Vec<String> = Vec::new();
        let asm_text = self.rewrite_asm_body(&a.body, &name_to_index, &mut clobbers);
        // Явно названные регистры-скретчи помечаем как затираемые, плюс флаги.
        for c in &clobbers {
            constraints.push(format!("~{{{c}}}"));
        }
        constraints.push("~{dirflag}".into());
        constraints.push("~{fpsr}".into());
        constraints.push("~{flags}".into());

        let cons = constraints.join(",");
        let dialect = "inteldialect"; // и masm, и nasm у нас Intel-синтаксис

        if out_slots.is_empty() {
            self.emit(format!(
                "call void asm sideeffect {dialect} \"{asm}\", \"{cons}\"({args})",
                asm = asm_text,
                args = call_args.join(", ")
            ));
        } else if out_slots.len() == 1 {
            let (slot, ty) = out_slots[0].clone();
            let t = self.fresh_tmp();
            self.emit(format!(
                "{t} = call {rty} asm sideeffect {dialect} \"{asm}\", \"{cons}\"({args})",
                rty = ty.llvm(),
                asm = asm_text,
                args = call_args.join(", ")
            ));
            self.emit(format!("store {rty} {t}, ptr {slot}", rty = ty.llvm()));
        } else {
            let elem_types: Vec<String> = out_slots.iter().map(|(_, ty)| ty.llvm()).collect();
            let struct_ty = format!("{{ {} }}", elem_types.join(", "));
            let t = self.fresh_tmp();
            self.emit(format!(
                "{t} = call {struct_ty} asm sideeffect {dialect} \"{asm}\", \"{cons}\"({args})",
                asm = asm_text,
                args = call_args.join(", ")
            ));
            for (i, (slot, ty)) in out_slots.iter().enumerate() {
                let ev = self.fresh_tmp();
                self.emit(format!("{ev} = extractvalue {struct_ty} {t}, {i}"));
                self.emit(format!("store {rty} {ev}, ptr {slot}", rty = ty.llvm()));
            }
        }
    }


    fn rewrite_asm_body(&self, body: &str, names: &HashMap<String, usize>, clobbers: &mut Vec<String>) -> String {
        let mut out = String::new();
        for (li, raw_line) in body.lines().enumerate() {
            // Отрезаем MASM/NASM комментарий, начинающийся с ';'.
            let line = match raw_line.find(';') {
                Some(p) => &raw_line[..p],
                None => raw_line,
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if li > 0 && !out.is_empty() {
                out.push_str("\\0A"); // перевод строки внутри asm-строки LLVM
            }
            // Подстановка имён переменных на позиционные операнды $N.
            let mut tokenbuf = String::new();
            let mut chars = line.chars().peekable();
            while let Some(&c) = chars.peek() {
                if c.is_alphabetic() || c == '_' {
                    let mut ident = String::new();
                    while let Some(&d) = chars.peek() {
                        if d.is_alphanumeric() || d == '_' {
                            ident.push(d);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    if let Some(i) = names.get(&ident) {
                        tokenbuf.push('$');
                        tokenbuf.push_str(&i.to_string());
                    } else {
                        if is_x86_reg(&ident) && !clobbers.contains(&ident) {
                            clobbers.push(ident.clone());
                        }
                        tokenbuf.push_str(&ident);
                    }
                } else {
                    // Экранируем кавычки и обратный слэш для строки LLVM.
                    match c {
                        '"' => tokenbuf.push_str("\\22"),
                        '\\' => tokenbuf.push_str("\\5C"),
                        _ => tokenbuf.push(c),
                    }
                    chars.next();
                }
            }
            out.push_str(&tokenbuf);
        }
        out
    }

    // ---------- lvalue ----------

    /// Возвращает (указатель, тип-элемента, изменяемо ли место).
    fn gen_lvalue(&mut self, e: &Expr) -> (String, Ty, bool) {
        match e {
            Expr::Ident(name, span) => match self.lookup(name) {
                Some(l) => (l.slot.clone(), l.ty.clone(), l.mutable),
                None => {
                    // static — изменяемое место (адрес глобала).
                    if let Some((sym, ty)) = self.statics.get(name) {
                        return (sym.clone(), ty.clone(), true);
                    }
                    self.err("E0032", *span, format!("неизвестное имя `{name}`"), Some("объявите его через `let`"));
                    ("%poison".into(), Ty::Err, true)
                }
            },
            Expr::Unary { op: UnOp::Deref, expr, span } => {
                let (pv, pty) = self.gen_expr(expr, None);
                self.require_unsafe(*span, "разыменование сырого указателя");
                match pty {
                    Ty::Ptr(inner, m) => (pv, *inner, m),
                    Ty::Err => ("%poison".into(), Ty::Err, true),
                    other => {
                        self.err("E0052", expr.span(), format!("разыменовывать можно только указатель, а тут `{}`", other.name()), None);
                        ("%poison".into(), Ty::Err, true)
                    }
                }
            }
            Expr::Index { base, index, span } => {
                let (bv, bty) = self.gen_expr(base, None);
                match bty {
                    // Индексация среза — безопасная операция (unsafe не нужен),
                    // с проверкой границ: 0 <= i < len, иначе abort.
                    Ty::Slice(elem) => {
                        let lenp = self.fresh_tmp();
                        self.emit(format!("{lenp} = getelementptr %slice, ptr {bv}, i32 0, i32 1"));
                        let len = self.fresh_tmp();
                        self.emit(format!("{len} = load i64, ptr {lenp}"));
                        let dp = self.fresh_tmp();
                        self.emit(format!("{dp} = getelementptr %slice, ptr {bv}, i32 0, i32 0"));
                        let data = self.fresh_tmp();
                        self.emit(format!("{data} = load ptr, ptr {dp}"));
                        let (iv, _) = self.gen_expr(index, Some(&Ty::I64));
                        self.emit_bounds_check(&iv, &len);
                        let t = self.fresh_tmp();
                        self.emit(format!("{t} = getelementptr {ety}, ptr {data}, i64 {iv}", ety = elem.llvm()));
                        (t, *elem, true)
                    }
                    // Массив — bounds-check по константной длине; bv это адрес.
                    Ty::Array(elem, n) => {
                        let (iv, _) = self.gen_expr(index, Some(&Ty::I64));
                        self.emit_bounds_check(&iv, &n.to_string());
                        let t = self.fresh_tmp();
                        self.emit(format!(
                            "{t} = getelementptr [{n} x {ety}], ptr {bv}, i64 0, i64 {iv}",
                            ety = elem.llvm()
                        ));
                        (t, *elem, true)
                    }
                    Ty::Ptr(inner, m) => {
                        self.require_unsafe(*span, "индексация сырого указателя");
                        let (iv, _) = self.gen_expr(index, Some(&Ty::I64));
                        let t = self.fresh_tmp();
                        self.emit(format!(
                            "{t} = getelementptr {ety}, ptr {bv}, i64 {iv}",
                            ety = inner.llvm()
                        ));
                        (t, *inner, m)
                    }
                    Ty::Err => ("%poison".into(), Ty::Err, true),
                    other => {
                        self.err("E0053", base.span(), format!("индексировать можно только указатель, срез или массив, а тут `{}`", other.name()), None);
                        ("%poison".into(), Ty::Err, true)
                    }
                }
            }
            Expr::Field { base, field, span } => {
                // Поля среза (.ptr / .len).
                if let Ty::Slice(elem) = self.type_of(base) {
                    let (addr, _, m) = self.gen_lvalue(base);
                    let (ptr, fty) = self.slice_field(&addr, &elem, field, *span);
                    return (ptr, fty, m);
                }
                let (addr, sname, mutable) = self.struct_addr_lvalue(base);
                self.field_gep(&addr, &sname, field, *span, mutable)
            }
            other => {
                self.err(
                    "E0054",
                    other.span(),
                    "это выражение нельзя использовать как место для записи".into(),
                    Some("присваивать можно переменной, полю, `*p` или `p[i]`"),
                );
                ("%poison".into(), Ty::Err, false)
            }
        }
    }

    /// Проверка границ индекса среза: при `i < 0 || i >= len` вызывает abort.
    fn emit_bounds_check(&mut self, idx: &str, len: &str) {
        let lo = self.fresh_tmp();
        self.emit(format!("{lo} = icmp slt i64 {idx}, 0"));
        let hi = self.fresh_tmp();
        self.emit(format!("{hi} = icmp sge i64 {idx}, {len}"));
        let oob = self.fresh_tmp();
        self.emit(format!("{oob} = or i1 {lo}, {hi}"));
        let fail = self.fresh_label("oob");
        let ok = self.fresh_label("inb");
        self.emit(format!("br i1 {oob}, label %{fail}, label %{ok}"));
        self.emit_label(&fail);
        if !self.ctx.fns.contains_key("abort") {
            self.use_intrinsic("declare void @abort()".into());
        }
        self.emit("call void @abort()".into());
        self.emit("unreachable".into());
        self.emit_label(&ok);
    }

    fn field_gep(&mut self, addr: &str, sname: &str, field: &str, span: Span, mutable: bool) -> (String, Ty, bool) {
        let info = match self.ctx.structs.get(sname) {
            Some(i) => i.clone(),
            None => return ("%poison".into(), Ty::Err, mutable),
        };
        match info.field_index(field) {
            Some(idx) => {
                let fty = info.fields[idx].1.clone();
                let t = self.fresh_tmp();
                self.emit(format!(
                    "{t} = getelementptr %struct.{sname}, ptr {addr}, i32 0, i32 {idx}"
                ));
                (t, fty, mutable)
            }
            None => {
                self.err("E0055", span, format!("у структуры `{sname}` нет поля `{field}`"), None);
                ("%poison".into(), Ty::Err, mutable)
            }
        }
    }

    /// GEP к синтетическому полю среза: `.ptr` (индекс 0) или `.len` (индекс 1).
    /// Возвращает (указатель-на-поле, тип поля).
    fn slice_field(&mut self, addr: &str, elem: &Ty, field: &str, span: Span) -> (String, Ty) {
        match field {
            "ptr" => {
                let t = self.fresh_tmp();
                self.emit(format!("{t} = getelementptr %slice, ptr {addr}, i32 0, i32 0"));
                (t, Ty::Ptr(Box::new(elem.clone()), true))
            }
            "len" => {
                let t = self.fresh_tmp();
                self.emit(format!("{t} = getelementptr %slice, ptr {addr}, i32 0, i32 1"));
                (t, Ty::I64)
            }
            _ => {
                self.err("E0055", span, format!("у среза есть только поля `ptr` и `len`, а не `{field}`"), None);
                ("%poison".into(), Ty::Err)
            }
        }
    }

    /// Адрес структуры для чтения (годится любое выражение-структура).
    fn struct_addr_rvalue(&mut self, base: &Expr) -> (String, String) {
        let bty = self.type_of(base);
        match bty {
            Ty::Struct(name) => {
                let (v, _) = self.gen_expr(base, None); // для структур это адрес
                (v, name)
            }
            Ty::Ptr(inner, _) if matches!(*inner, Ty::Struct(_)) => {
                if !self.is_safe_ref(base) {
                    self.require_unsafe(base.span(), "доступ к полю через сырой указатель");
                }
                let (v, _) = self.gen_expr(base, None); // загруженный указатель
                if let Ty::Struct(name) = *inner {
                    (v, name)
                } else {
                    unreachable!()
                }
            }
            Ty::Err => ("%poison".into(), String::new()),
            other => {
                self.err("E0056", base.span(), format!("доступ к полю у не-структуры `{}`", other.name()), None);
                ("%poison".into(), String::new())
            }
        }
    }

    /// Адрес структуры как места записи.
    fn struct_addr_lvalue(&mut self, base: &Expr) -> (String, String, bool) {
        let bty = self.type_of(base);
        match bty {
            Ty::Struct(name) => {
                let (addr, _, m) = self.gen_lvalue(base);
                (addr, name, m)
            }
            Ty::Ptr(inner, m) if matches!(*inner, Ty::Struct(_)) => {
                if !self.is_safe_ref(base) {
                    self.require_unsafe(base.span(), "запись поля через сырой указатель");
                }
                let (v, _) = self.gen_expr(base, None);
                if let Ty::Struct(name) = *inner {
                    (v, name, m)
                } else {
                    unreachable!()
                }
            }
            Ty::Err => ("%poison".into(), String::new(), false),
            other => {
                self.err("E0056", base.span(), format!("доступ к полю у не-структуры `{}`", other.name()), None);
                ("%poison".into(), String::new(), false)
            }
        }
    }

    // ---------- выражения (rvalue) ----------

    fn gen_expr(&mut self, e: &Expr, expected: Option<&Ty>) -> (String, Ty) {
        let (v, ty) = self.gen_expr_inner(e, expected);
        // Коэрция `[N]T` -> `[]T`: строим срез из адреса массива и длины N.
        if let (Some(Ty::Slice(want)), Ty::Array(got, n)) = (expected, &ty) {
            if want == got {
                let elem = (**got).clone();
                let slot = self.fresh_slot("a2s");
                self.alloca(&slot, &Ty::Slice(Box::new(elem.clone())));
                let pf = self.fresh_tmp();
                self.emit(format!("{pf} = getelementptr %slice, ptr {slot}, i32 0, i32 0"));
                self.emit(format!("store ptr {v}, ptr {pf}"));
                let lf = self.fresh_tmp();
                self.emit(format!("{lf} = getelementptr %slice, ptr {slot}, i32 0, i32 1"));
                self.emit(format!("store i64 {n}, ptr {lf}"));
                return (slot, Ty::Slice(Box::new(elem)));
            }
        }
        (v, ty)
    }

    fn gen_expr_inner(&mut self, e: &Expr, expected: Option<&Ty>) -> (String, Ty) {
        match e {
            Expr::Int(n, _) => match expected {
                Some(Ty::F32) => (fmt_float(*n as f64, &Ty::F32), Ty::F32),
                Some(Ty::F64) => (fmt_float(*n as f64, &Ty::F64), Ty::F64),
                Some(t) if t.is_int() => (n.to_string(), t.clone()),
                _ => (n.to_string(), Ty::I64),
            },
            Expr::Float(f, _) => match expected {
                Some(Ty::F32) => (fmt_float(*f, &Ty::F32), Ty::F32),
                _ => (fmt_float(*f, &Ty::F64), Ty::F64),
            },
            Expr::Bool(b, _) => (if *b { "true".into() } else { "false".into() }, Ty::Bool),
            Expr::Path(en, variant, span) => {
                match self.ctx.enums.get(en).and_then(|m| m.get(variant)) {
                    Some(v) => ((*v as i32).to_string(), Ty::I32),
                    None => {
                        if self.ctx.enums.contains_key(en) {
                            self.err("E0058", *span, format!("у перечисления `{en}` нет варианта `{variant}`"), None);
                        } else {
                            self.err("E0059", *span, format!("неизвестное перечисление `{en}`"), None);
                        }
                        ("0".into(), Ty::Err)
                    }
                }
            }
            Expr::Null(_) => {
                // Тип берём из ожидания (если это указатель), иначе *mut u8.
                let ty = match expected {
                    Some(t @ Ty::Ptr(..)) => t.clone(),
                    Some(t @ Ty::FnPtr(..)) => t.clone(),
                    _ => Ty::Ptr(Box::new(Ty::U8), true),
                };
                ("null".into(), ty)
            }
            Expr::Str(s, _) => {
                if self.obfuscate_strings {
                    return self.gen_obfuscated_string(s, expected);
                }
                let g = self.intern_string(s);
                // Если ожидается сырой указатель (*u8 / *void / etc.), отдаём ptr (C-строка).
                // Иначе по умолчанию литерал "..." — первоклассная строка str ([]u8).
                let want_raw_ptr = matches!(expected, Some(Ty::Ptr(..)));
                if !want_raw_ptr {
                    let len = s.as_bytes().len();
                    let slot = self.fresh_slot("str");
                    self.alloca(&slot, &Ty::Slice(Box::new(Ty::U8)));
                    let pf = self.fresh_tmp();
                    self.emit(format!("{pf} = getelementptr %slice, ptr {slot}, i32 0, i32 0"));
                    self.emit(format!("store ptr {g}, ptr {pf}"));
                    let lf = self.fresh_tmp();
                    self.emit(format!("{lf} = getelementptr %slice, ptr {slot}, i32 0, i32 1"));
                    self.emit(format!("store i64 {len}, ptr {lf}"));
                    (slot, Ty::Slice(Box::new(Ty::U8)))
                } else {
                    (g, Ty::Ptr(Box::new(Ty::U8), false))
                }
            }
            Expr::Ident(name, span) => match self.lookup(name) {
                Some(l) => {
                    let l = l.clone();
                    if is_aggregate(&l.ty) {
                        // агрегат (структура/срез) представляется адресом слота
                        (l.slot.clone(), l.ty)
                    } else {
                        let t = self.fresh_tmp();
                        self.emit(format!("{t} = load {ty}, ptr {slot}", ty = l.ty.llvm(), slot = l.slot));
                        (t, l.ty)
                    }
                }
                None => {
                    // Глобальная static-переменная — грузим из глобала.
                    if let Some((sym, ty)) = self.statics.get(name) {
                        let (sym, ty) = (sym.clone(), ty.clone());
                        if is_aggregate(&ty) {
                            return (sym, ty);
                        }
                        let t = self.fresh_tmp();
                        self.emit(format!("{t} = load {lty}, ptr {sym}", lty = ty.llvm()));
                        return (t, ty);
                    }
                    // Глобальная константа (свёрнута заранее).
                    if let Some(cv) = self.consts.get(name) {
                        return (cv.render(), cv.ty());
                    }
                    // Захваченная в jit-шаблоне переменная -> плейсхолдер-константа.
                    if let Some(caps) = &self.captures {
                        if let Some((idx, ty)) = caps.get(name) {
                            return (format!("$CAP{idx}$"), ty.clone());
                        }
                    }
                    // Имя функции как значение -> указатель на функцию.
                    if let Some(sig) = self.ctx.fns.get(name) {
                        if sig.variadic {
                            self.err("E0057", *span, format!("нельзя взять указатель на вариадическую функцию `{name}`"), None);
                            return ("0".into(), Ty::Err);
                        }
                        let ty = Ty::FnPtr(sig.params.clone(), Box::new(sig.ret.clone()));
                        return (llvm_global(name), ty);
                    }

                    self.err("E0032", *span, format!("неизвестное имя `{name}`"), Some("объявите переменную через `let`"));
                    ("0".into(), Ty::Err)
                }
            },
            Expr::Unary { op, expr, span } => self.gen_unary(*op, expr, *span, expected),
            Expr::Binary { op, lhs, rhs, span } => self.gen_binary(*op, lhs, rhs, *span, expected),
            Expr::Cast { expr, ty, span } => self.gen_cast(expr, ty, *span),
            Expr::Call { callee, args, span } => self.gen_call(callee, args, *span, expected),
            Expr::Field { base, field, span } => {
                // Длина массива — константа.
                if let Ty::Array(_, n) = self.type_of(base) {
                    if field == "len" {
                        return (n.to_string(), Ty::I64);
                    }
                    self.err("E0055", *span, format!("у массива есть только поле `len`, а не `{field}`"), None);
                    return ("0".into(), Ty::Err);
                }
                // Поля среза (.ptr / .len) — синтетические.
                if let Ty::Slice(elem) = self.type_of(base) {
                    let (addr, _) = self.gen_expr(base, None); // адрес агрегата
                    let (ptr, fty) = self.slice_field(&addr, &elem, field, *span);
                    if fty == Ty::Err {
                        return ("0".into(), Ty::Err);
                    }
                    let t = self.fresh_tmp();
                    self.emit(format!("{t} = load {ty}, ptr {ptr}", ty = fty.llvm()));
                    return (t, fty);
                }
                let (addr, sname) = self.struct_addr_rvalue(base);
                if sname.is_empty() {
                    return ("0".into(), Ty::Err);
                }
                let (ptr, fty, _) = self.field_gep(&addr, &sname, field, *span, false);
                if is_aggregate(&fty) {
                    (ptr, fty)
                } else if fty == Ty::Err {
                    ("0".into(), Ty::Err)
                } else {
                    let t = self.fresh_tmp();
                    self.emit(format!("{t} = load {ty}, ptr {ptr}", ty = fty.llvm()));
                    (t, fty)
                }
            }
            Expr::Index { base, index, span } => {
                let (ptr, ty, _) = self.gen_lvalue(&Expr::Index {
                    base: base.clone(),
                    index: index.clone(),
                    span: *span,
                });
                if ty == Ty::Err {
                    return ("0".into(), Ty::Err);
                }
                // Агрегатный элемент представляется адресом (без загрузки).
                if is_aggregate(&ty) {
                    return (ptr, ty);
                }
                let t = self.fresh_tmp();
                self.emit(format!("{t} = load {lty}, ptr {ptr}", lty = ty.llvm()));
                (t, ty)
            }
            Expr::Slice { base, start, end, span } => {
                self.gen_slice(base, start.as_deref(), end.as_deref(), *span)
            }
            Expr::StructLit { name, fields, span } => self.gen_struct_lit(name, fields, *span),
            Expr::ArrayLit(elems, span) => self.gen_array_lit(elems, expected, *span),
            Expr::IfExpr { cond, then, els, span } => self.gen_if_expr(cond, then, els, expected, *span),
            Expr::Jit { captures, inner, span } => self.gen_jit(captures, inner, *span),
            Expr::Try(inner, span) => self.gen_try(inner, *span),
        }
    }

    /// Кодоген jit-блока: строит IR-шаблон внутренней функции с плейсхолдерами
    /// захватов, встраивает его строкой, и в рантайме зовёт goraw_jit_compile,
    /// который подставляет константы и JIT-компилирует специализацию.
    fn gen_jit(&mut self, captures: &[(String, Span)], inner: &FnDef, span: Span) -> (String, Ty) {
        // Типы параметров и результата внутренней функции.
        let param_tys: Vec<Ty> = inner.params.iter().map(|p| self.resolve(&p.ty)).collect();
        let ret_ty = match &inner.ret {
            Some(t) => self.resolve(t),
            None => Ty::Void,
        };
        let fnptr_ty = Ty::FnPtr(param_tys.clone(), Box::new(ret_ty.clone()));

        // Разрешаем захваты в текущей области: имя -> (индекс, слот, тип).
        let mut cap_map: HashMap<String, (usize, Ty)> = HashMap::new();
        let mut cap_slots: Vec<(String, Ty)> = Vec::new();
        for (i, (name, csp)) in captures.iter().enumerate() {
            match self.lookup(name) {
                Some(l) => {
                    cap_map.insert(name.clone(), (i, l.ty.clone()));
                    cap_slots.push((l.slot.clone(), l.ty.clone()));
                }
                None => {
                    self.err("E0096", *csp, format!("неизвестная захватываемая переменная `{name}`"), None);
                    cap_map.insert(name.clone(), (i, Ty::Err));
                    cap_slots.push(("%poison".into(), Ty::Err));
                }
            }
        }

        // 1. Строим IR-шаблон внутренней функции в изолированных буферах.
        let template = self.build_jit_template(inner, cap_map, &ret_ty);

        // 2. Встраиваем шаблон и имя функции как строковые константы.
        let tmpl_ptr = self.intern_string(&template);
        let name_ptr = self.intern_string("__goraw_jit");

        // 3. Гарантируем extern-объявление рантайма.
        self.use_intrinsic("declare ptr @goraw_jit_compile(ptr, ptr, i32, ptr, ptr)".into());

        // 4. В рантайме готовим массивы bits[] и kinds[] по захватам.
        let n = cap_slots.len();
        let bits_arr = self.fresh_slot("jit_bits");
        let kinds_arr = self.fresh_slot("jit_kinds");
        self.allocas.push_str(&format!("  {bits_arr} = alloca [{n} x i64]\n", n = n.max(1)));
        self.allocas.push_str(&format!("  {kinds_arr} = alloca [{n} x i32]\n", n = n.max(1)));

        for (i, (slot, ty)) in cap_slots.iter().enumerate() {
            let (bits, kind) = self.capture_to_bits(slot, ty);
            let bp = self.fresh_tmp();
            self.emit(format!("{bp} = getelementptr [{n} x i64], ptr {bits_arr}, i64 0, i64 {i}", n = n.max(1)));
            self.emit(format!("store i64 {bits}, ptr {bp}"));
            let kp = self.fresh_tmp();
            self.emit(format!("{kp} = getelementptr [{n} x i32], ptr {kinds_arr}, i64 0, i64 {i}", n = n.max(1)));
            self.emit(format!("store i32 {kind}, ptr {kp}"));
        }

        // 5. Зовём рантайм: goraw_jit_compile(tmpl, name, n, bits, kinds) -> ptr.
        let _ = span;
        let res = self.fresh_tmp();
        self.emit(format!(
            "{res} = call ptr @goraw_jit_compile(ptr {tmpl_ptr}, ptr {name_ptr}, i32 {n}, ptr {bits_arr}, ptr {kinds_arr})"
        ));
        (res, fnptr_ty)
    }

    /// Реализация оператора `?`: проверка статуса и ранний выход при ошибке.
    fn gen_try(&mut self, inner: &Expr, span: Span) -> (String, Ty) {
        let (val, ty) = self.gen_expr(inner, None);
        match ty.clone() {
            Ty::Struct(sname) => {
                let info = match self.ctx.structs.get(&sname).cloned() {
                    Some(i) => i,
                    None => {
                        self.err("E0101", span, format!("неизвестная структура `{sname}` в `?`"), None);
                        return ("0".into(), Ty::Err);
                    }
                };

                let status_field = if info.field_index("is_ok").is_some() {
                    "is_ok"
                } else if info.field_index("ok").is_some() {
                    "ok"
                } else if info.field_index("is_some").is_some() {
                    "is_some"
                } else {
                    self.err("E0102", span, format!("структура `{sname}` не имеет поля `is_ok` или `is_some` для оператора `?`"), None);
                    return ("0".into(), Ty::Err);
                };

                let val_idx = match info.field_index("value") {
                    Some(idx) => idx,
                    None => {
                        self.err("E0103", span, format!("структура `{sname}` не имеет поля `value` для оператора `?`"), None);
                        return ("0".into(), Ty::Err);
                    }
                };

                let val_ty = info.fields[val_idx].1.clone();
                let status_idx = info.field_index(status_field).unwrap();

                // Загружаем флаг статуса (i1)
                let status_ptr = self.fresh_tmp();
                self.emit(format!("{status_ptr} = getelementptr %struct.{sname}, ptr {val}, i32 0, i32 {status_idx}"));
                let status_val = self.fresh_tmp();
                self.emit(format!("{status_val} = load i1, ptr {status_ptr}"));

                let ok_l = self.fresh_label("try_ok");
                let err_l = self.fresh_label("try_err");
                self.emit(format!("br i1 {status_val}, label %{ok_l}, label %{err_l}"));

                // Ветвь ошибки (err_l): ранний возврат из текущей функции
                self.emit_label(&err_l);
                let current_ret = self.cur_ret.clone();
                if let Ty::Struct(ret_sname) = &current_ret {
                    if let Some(ret_info) = self.ctx.structs.get(ret_sname).cloned() {
                        let ret_slot = self.fresh_slot("try_ret");
                        self.alloca(&ret_slot, &current_ret);
                        self.emit(format!("store %struct.{ret_sname} zeroinitializer, ptr {ret_slot}"));
                        if let Some(err_idx) = info.field_index("error") {
                            if let Some(ret_err_idx) = ret_info.field_index("error") {
                                let err_src_ptr = self.fresh_tmp();
                                self.emit(format!("{err_src_ptr} = getelementptr %struct.{sname}, ptr {val}, i32 0, i32 {err_idx}"));
                                let err_ty = info.fields[err_idx].1.clone();
                                let err_val = self.fresh_tmp();
                                self.emit(format!("{err_val} = load {ety}, ptr {err_src_ptr}", ety = err_ty.llvm()));
                                let err_dst_ptr = self.fresh_tmp();
                                self.emit(format!("{err_dst_ptr} = getelementptr %struct.{ret_sname}, ptr {ret_slot}, i32 0, i32 {ret_err_idx}"));
                                self.emit(format!("store {ety} {err_val}, ptr {err_dst_ptr}", ety = err_ty.llvm()));
                            }
                        }
                        let ret_val = self.fresh_tmp();
                        self.emit(format!("{ret_val} = load %struct.{ret_sname}, ptr {ret_slot}"));
                        self.emit_drops_for_all_scopes(Some(&ret_slot));
                        self.emit(format!("ret %struct.{ret_sname} {ret_val}"));
                    } else {
                        self.emit_drops_for_all_scopes(None);
                        self.emit(format!("ret {} {}", current_ret.llvm(), self.zero_of(&current_ret)));
                    }
                } else if current_ret.is_int() {
                    let err_code = if let Some(err_idx) = info.field_index("error") {
                        let err_ptr = self.fresh_tmp();
                        self.emit(format!("{err_ptr} = getelementptr %struct.{sname}, ptr {val}, i32 0, i32 {err_idx}"));
                        let err_v = self.fresh_tmp();
                        self.emit(format!("{err_v} = load i64, ptr {err_ptr}"));
                        if current_ret != Ty::I64 {
                            let tr = self.fresh_tmp();
                            self.emit(format!("{tr} = trunc i64 {err_v} to {}", current_ret.llvm()));
                            tr
                        } else {
                            err_v
                        }
                    } else {
                        "1".to_string()
                    };
                    self.emit_drops_for_all_scopes(None);
                    self.emit(format!("ret {} {err_code}", current_ret.llvm()));
                } else if current_ret == Ty::Void {
                    self.emit_drops_for_all_scopes(None);
                    self.emit("ret void".into());
                } else {
                    self.emit_drops_for_all_scopes(None);
                    self.emit(format!("ret {} {}", current_ret.llvm(), self.zero_of(&current_ret)));
                }

                // Ветвь успеха (ok_l): извлекаем value
                self.emit_label(&ok_l);
                let val_ptr = self.fresh_tmp();
                self.emit(format!("{val_ptr} = getelementptr %struct.{sname}, ptr {val}, i32 0, i32 {val_idx}"));
                if is_aggregate(&val_ty) {
                    (val_ptr, val_ty)
                } else {
                    let res = self.fresh_tmp();
                    self.emit(format!("{res} = load {vty}, ptr {val_ptr}", vty = val_ty.llvm()));
                    (res, val_ty)
                }
            }
            Ty::Ptr(inner, is_mut) => {
                let is_null = self.fresh_tmp();
                self.emit(format!("{is_null} = icmp eq ptr {val}, null"));
                let ok_l = self.fresh_label("try_ok");
                let err_l = self.fresh_label("try_err");
                self.emit(format!("br i1 {is_null}, label %{err_l}, label %{ok_l}"));

                self.emit_label(&err_l);
                self.emit_drops_for_all_scopes(None);
                let current_ret = self.cur_ret.clone();
                if current_ret == Ty::Void {
                    self.emit("ret void".into());
                } else {
                    self.emit(format!("ret {} {}", current_ret.llvm(), self.zero_of(&current_ret)));
                }

                self.emit_label(&ok_l);
                (val, Ty::Ptr(inner, is_mut))
            }
            other if other.is_int() => {
                let is_err = self.fresh_tmp();
                self.emit(format!("{is_err} = icmp slt {ty_s} {val}, 0", ty_s = other.llvm()));
                let ok_l = self.fresh_label("try_ok");
                let err_l = self.fresh_label("try_err");
                self.emit(format!("br i1 {is_err}, label %{err_l}, label %{ok_l}"));

                self.emit_label(&err_l);
                self.emit_drops_for_all_scopes(None);
                let current_ret = self.cur_ret.clone();
                if current_ret == Ty::Void {
                    self.emit("ret void".into());
                } else {
                    self.emit(format!("ret {} {val}", current_ret.llvm()));
                }

                self.emit_label(&ok_l);
                (val, other)
            }
            other => {
                self.err("E0104", span, format!("оператор `?` не применим к типу `{}`", other.name()), None);
                ("0".into(), Ty::Err)
            }
        }
    }

    /// Загружает захват из слота и приводит к паре (i64-биты, код-типа) для рантайма.
    fn capture_to_bits(&mut self, slot: &str, ty: &Ty) -> (String, i32) {
        let kind = match ty {
            Ty::I8 => 0,
            Ty::I16 => 1,
            Ty::I32 => 2,
            Ty::I64 => 3,
            Ty::U8 => 4,
            Ty::U16 => 5,
            Ty::U32 => 6,
            Ty::U64 => 7,
            Ty::F32 => 8,
            Ty::F64 => 9,
            _ => 3,
        };
        let v = self.fresh_tmp();
        self.emit(format!("{v} = load {lty}, ptr {slot}", lty = ty.llvm()));
        let bits = match ty {
            Ty::I64 | Ty::U64 => v,
            Ty::F64 => {
                let t = self.fresh_tmp();
                self.emit(format!("{t} = bitcast double {v} to i64"));
                t
            }
            Ty::F32 => {
                let bc = self.fresh_tmp();
                self.emit(format!("{bc} = bitcast float {v} to i32"));
                let ze = self.fresh_tmp();
                self.emit(format!("{ze} = zext i32 {bc} to i64"));
                ze
            }
            t if t.is_signed() => {
                let t2 = self.fresh_tmp();
                self.emit(format!("{t2} = sext {lty} {v} to i64", lty = ty.llvm()));
                t2
            }
            _ => {
                // беззнаковые целые уже < 64 бит
                let t2 = self.fresh_tmp();
                self.emit(format!("{t2} = zext {lty} {v} to i64", lty = ty.llvm()));
                t2
            }
        };
        (bits, kind)
    }

    /// Собирает самостоятельный IR-модуль-шаблон для внутренней функции jit.
    /// Захваты внутри становятся плейсхолдерами `$CAPi$`. Модуль без triple/
    /// datalayout — их проставит LLJIT под хост.
    fn build_jit_template(
        &mut self,
        inner: &FnDef,
        cap_map: HashMap<String, (usize, Ty)>,
        _ret_ty: &Ty,
    ) -> String {
        // Сохраняем состояние текущего (главного) модуля и функции.
        let saved_body = std::mem::take(&mut self.body);
        let saved_strings = std::mem::take(&mut self.strings);
        let saved_intr = std::mem::take(&mut self.intrinsics);
        let saved_allocas = std::mem::take(&mut self.allocas);
        let saved_code = std::mem::take(&mut self.code);
        let saved_scopes = std::mem::take(&mut self.scopes);
        let saved_loops = std::mem::take(&mut self.loops);
        let saved_tmp = self.tmp;
        let saved_label = self.label;
        let saved_slot = self.slotcount;
        let saved_ret = self.cur_ret.clone();
        let saved_unsafe = self.cur_unsafe;
        let saved_udepth = self.unsafe_depth;
        let saved_term = self.terminated;
        let saved_strcount = self.strcount;

        // Переключаемся в режим шаблона.
        self.captures = Some(cap_map);
        self.in_jit_template = true;

        // Внутренняя функция всегда компилируется под именем __goraw_jit.
        let mut renamed = inner.clone();
        renamed.name = "__goraw_jit".to_string();
        self.gen_fn(&renamed);

        let tmpl_body = std::mem::take(&mut self.body);
        let tmpl_strings = std::mem::take(&mut self.strings);
        let tmpl_intr = std::mem::take(&mut self.intrinsics);

        // Собираем модуль-шаблон: declare внешних C-функций (их резолвит
        // генератор символов процесса) + declare интринзиков + строки + тело.
        let mut module = String::new();
        let mut extern_decls: Vec<String> = Vec::new();
        for (fname, sig) in &self.ctx.fns {
            if sig.is_extern {
                let params: Vec<String> = sig.params.iter().map(|t| t.llvm()).collect();
                let mut plist = params.join(", ");
                if sig.variadic {
                    if plist.is_empty() {
                        plist.push_str("...");
                    } else {
                        plist.push_str(", ...");
                    }
                }
                extern_decls.push(format!("declare {} @{}({})", sig.ret.llvm(), fname, plist));
            }
        }
        extern_decls.sort();
        for d in extern_decls {
            module.push_str(&d);
            module.push('\n');
        }
        let mut intr: Vec<&String> = tmpl_intr.iter().collect();
        intr.sort();
        for d in intr {
            module.push_str(d);
            module.push('\n');
        }
        module.push_str(&tmpl_strings);
        module.push_str(&tmpl_body);

        // Восстанавливаем состояние главного модуля/функции.
        self.body = saved_body;
        self.strings = saved_strings;
        self.intrinsics = saved_intr;
        self.allocas = saved_allocas;
        self.code = saved_code;
        self.scopes = saved_scopes;
        self.loops = saved_loops;
        self.tmp = saved_tmp;
        self.label = saved_label;
        self.slotcount = saved_slot;
        self.cur_ret = saved_ret;
        self.cur_unsafe = saved_unsafe;
        self.unsafe_depth = saved_udepth;
        self.terminated = saved_term;
        self.strcount = saved_strcount;
        self.captures = None;
        self.in_jit_template = false;
        self.jittmpl_count += 1;

        module
    }

    fn gen_unary(&mut self, op: UnOp, expr: &Expr, span: Span, expected: Option<&Ty>) -> (String, Ty) {
        match op {
            UnOp::Neg => {
                let (v, ty) = self.gen_expr(expr, expected.filter(|t| t.is_numeric()));
                if ty.is_int() {
                    let t = self.fresh_tmp();
                    self.emit(format!("{t} = sub {lty} 0, {v}", lty = ty.llvm()));
                    (t, ty)
                } else if ty.is_float() {
                    let t = self.fresh_tmp();
                    self.emit(format!("{t} = fneg {lty} {v}", lty = ty.llvm()));
                    (t, ty)
                } else if ty == Ty::Err {
                    ("0".into(), Ty::Err)
                } else {
                    self.err("E0060", span, format!("унарный минус неприменим к `{}`", ty.name()), None);
                    ("0".into(), Ty::Err)
                }
            }
            UnOp::Not => {
                let (v, ty) = self.gen_expr(expr, Some(&Ty::Bool));
                if ty == Ty::Bool {
                    let t = self.fresh_tmp();
                    self.emit(format!("{t} = xor i1 {v}, true"));
                    (t, Ty::Bool)
                } else if ty == Ty::Err {
                    ("0".into(), Ty::Err)
                } else {
                    self.err("E0061", span, format!("`!` применяется к bool, а не к `{}`", ty.name()), None);
                    ("0".into(), Ty::Err)
                }
            }
            UnOp::BitNot => {
                let (v, ty) = self.gen_expr(expr, expected.filter(|t| t.is_int()));
                if ty.is_int() {
                    let t = self.fresh_tmp();
                    self.emit(format!("{t} = xor {lty} {v}, -1", lty = ty.llvm()));
                    (t, ty)
                } else if ty == Ty::Err {
                    ("0".into(), Ty::Err)
                } else {
                    self.err("E0062", span, format!("`~` применяется к целому, а не к `{}`", ty.name()), None);
                    ("0".into(), Ty::Err)
                }
            }
            UnOp::Deref => {
                let (ptr, ty, _) = self.gen_lvalue(&Expr::Unary { op: UnOp::Deref, expr: Box::new(expr.clone()), span });
                if ty == Ty::Err {
                    return ("0".into(), Ty::Err);
                }
                if is_aggregate(&ty) {
                    (ptr, ty)
                } else {
                    let t = self.fresh_tmp();
                    self.emit(format!("{t} = load {lty}, ptr {ptr}", lty = ty.llvm()));
                    (t, ty)
                }
            }
            UnOp::Ref | UnOp::RefMut => {
                let want_mut = op == UnOp::RefMut;
                let (ptr, ty, mutable) = self.gen_lvalue(expr);
                if want_mut && !mutable && ty != Ty::Err {
                    self.err("E0062", span, "нельзя взять `&mut` на неизменяемое место".into(), Some("объявите переменную через `let mut`"));
                }
                (ptr, Ty::Ptr(Box::new(ty), want_mut))
            }
        }
    }

    fn gen_binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr, span: Span, expected: Option<&Ty>) -> (String, Ty) {
        // Логические && и || — с коротким замыканием через bool-слот.
        if op == BinOp::And || op == BinOp::Or {
            return self.gen_logical(op, lhs, rhs, span);
        }

        let (lv, lty) = self.gen_expr(lhs, expected.filter(|t| t.is_numeric()));
        let rexp = if lty.is_numeric() { Some(lty.clone()) } else { None };
        let (rv, rty) = self.gen_expr(rhs, rexp.as_ref());

        if lty == Ty::Err || rty == Ty::Err {
            return ("0".into(), Ty::Err);
        }

        let is_cmp = matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge);

        // Операции над первоклассными строками str ([]u8).
        if is_str(&lty) && is_str(&rty) {
            if op == BinOp::Add {
                return self.gen_str_concat(&lv, &rv);
            }
            if op == BinOp::Eq || op == BinOp::Ne {
                return self.gen_str_cmp(op, &lv, &rv);
            }
        }

        // Сравнение указателей допустимо для == и !=.
        if lty.is_ptr() && rty.is_ptr() && matches!(op, BinOp::Eq | BinOp::Ne) {
            let t = self.fresh_tmp();
            let pred = if op == BinOp::Eq { "eq" } else { "ne" };
            self.emit(format!("{t} = icmp {pred} ptr {lv}, {rv}"));
            return (t, Ty::Bool);
        }

        if lty != rty {
            self.err(
                "E0031",
                span,
                format!("несовместимые типы в операции: `{}` и `{}`", lty.name(), rty.name()),
                Some("приведите один из операндов через `as`"),
            );
            return ("0".into(), Ty::Err);
        }

        if is_cmp {
            let t = self.fresh_tmp();
            if lty.is_float() {
                let pred = match op {
                    BinOp::Eq => "oeq",
                    BinOp::Ne => "one",
                    BinOp::Lt => "olt",
                    BinOp::Le => "ole",
                    BinOp::Gt => "ogt",
                    BinOp::Ge => "oge",
                    _ => unreachable!(),
                };
                self.emit(format!("{t} = fcmp {pred} {lty} {lv}, {rv}", lty = lty.llvm()));
            } else {
                let signed = lty.is_signed();
                let pred = match op {
                    BinOp::Eq => "eq",
                    BinOp::Ne => "ne",
                    BinOp::Lt => if signed { "slt" } else { "ult" },
                    BinOp::Le => if signed { "sle" } else { "ule" },
                    BinOp::Gt => if signed { "sgt" } else { "ugt" },
                    BinOp::Ge => if signed { "sge" } else { "uge" },
                    _ => unreachable!(),
                };
                self.emit(format!("{t} = icmp {pred} {lty} {lv}, {rv}", lty = lty.llvm()));
            }
            return (t, Ty::Bool);
        }

        // Арифметика/битовые операции.
        let float = lty.is_float();
        if float && matches!(op, BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr | BinOp::Rem) {
            if op == BinOp::Rem {
                // frem допустим
            } else {
                self.err("E0063", span, "битовые операции и сдвиги неприменимы к float".into(), None);
                return ("0".into(), Ty::Err);
            }
        }
        let signed = lty.is_signed();
        let instr = match op {
            BinOp::Add => if float { "fadd" } else { "add" },
            BinOp::Sub => if float { "fsub" } else { "sub" },
            BinOp::Mul => if float { "fmul" } else { "mul" },
            BinOp::Div => if float { "fdiv" } else if signed { "sdiv" } else { "udiv" },
            BinOp::Rem => if float { "frem" } else if signed { "srem" } else { "urem" },
            BinOp::BitAnd => "and",
            BinOp::BitOr => "or",
            BinOp::BitXor => "xor",
            BinOp::Shl => "shl",
            BinOp::Shr => if signed { "ashr" } else { "lshr" },
            _ => unreachable!(),
        };
        let t = self.fresh_tmp();
        self.emit(format!("{t} = {instr} {lty} {lv}, {rv}", lty = lty.llvm()));
        (t, lty)
    }

    fn gen_logical(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr, _span: Span) -> (String, Ty) {
        let slot = self.fresh_slot("logic");
        self.alloca(&slot, &Ty::Bool);
        let (lv, lty) = self.gen_expr(lhs, Some(&Ty::Bool));
        self.expect_bool(&lty, lhs.span());
        self.emit(format!("store i1 {lv}, ptr {slot}"));
        let rhs_l = self.fresh_label("logrhs");
        let done_l = self.fresh_label("logdone");
        if op == BinOp::And {
            // если lhs ложно — пропускаем rhs (в слоте уже false)
            self.emit(format!("br i1 {lv}, label %{rhs_l}, label %{done_l}"));
        } else {
            // OR: если lhs истинно — пропускаем rhs (в слоте уже true)
            self.emit(format!("br i1 {lv}, label %{done_l}, label %{rhs_l}"));
        }
        self.emit_label(&rhs_l);
        let (rv, rty) = self.gen_expr(rhs, Some(&Ty::Bool));
        self.expect_bool(&rty, rhs.span());
        self.emit(format!("store i1 {rv}, ptr {slot}"));
        self.emit(format!("br label %{done_l}"));
        self.emit_label(&done_l);
        let t = self.fresh_tmp();
        self.emit(format!("{t} = load i1, ptr {slot}"));
        (t, Ty::Bool)
    }

    fn gen_cast(&mut self, expr: &Expr, ty: &TypeExpr, span: Span) -> (String, Ty) {
        let dst = self.resolve(ty);
        let (v, src) = self.gen_expr(expr, None);
        if src == Ty::Err || dst == Ty::Err {
            return ("0".into(), dst);
        }
        if src == dst {
            return (v, dst);
        }
        // Коэрция среза str -> сырой указатель *u8.
        if let Ty::Slice(ref elem) = src {
            if let Ty::Ptr(ref delem, _) = dst {
                if **delem == **elem {
                    let pf = self.fresh_tmp();
                    self.emit(format!("{pf} = getelementptr %slice, ptr {v}, i32 0, i32 0"));
                    let p = self.fresh_tmp();
                    self.emit(format!("{p} = load ptr, ptr {pf}"));
                    return (p, dst);
                }
            }
        }
        let src_is_ptr = src.is_ptr() || matches!(src, Ty::FnPtr(..));
        let dst_is_ptr = dst.is_ptr() || matches!(dst, Ty::FnPtr(..));
        if src_is_ptr && dst_is_ptr {
            return (v, dst); // непрозрачные ptr — без инструкции
        }
        if (src_is_ptr && dst.is_int()) || (src.is_int() && dst_is_ptr) {
            self.require_unsafe(span, "приведение между указателем и числом");
            let t = self.fresh_tmp();
            let instr = if src_is_ptr { "ptrtoint" } else { "inttoptr" };
            self.emit(format!("{t} = {instr} {s} {v} to {d}", s = src.llvm(), d = dst.llvm()));
            return (t, dst);
        }
        let t = self.fresh_tmp();
        let instr: &str;
        if src.is_int() && dst.is_int() {
            let sb = src.int_bits();
            let db = dst.int_bits();
            if db > sb {
                instr = if src.is_signed() { "sext" } else { "zext" };
            } else if db < sb {
                instr = "trunc";
            } else {
                return (v, dst); // одинаковая ширина, только смена знаковости
            }
        } else if src == Ty::Bool && dst.is_int() {
            instr = "zext";
        } else if src.is_int() && dst.is_float() {
            instr = if src.is_signed() { "sitofp" } else { "uitofp" };
        } else if src.is_float() && dst.is_int() {
            instr = if dst.is_signed() { "fptosi" } else { "fptoui" };
        } else if src.is_float() && dst.is_float() {
            instr = if dst == Ty::F64 { "fpext" } else { "fptrunc" };
        } else {
            self.err("E0064", span, format!("нельзя привести `{}` к `{}`", src.name(), dst.name()), None);
            return ("0".into(), Ty::Err);
        }
        self.emit(format!("{t} = {instr} {s} {v} to {d}", s = src.llvm(), d = dst.llvm()));
        (t, dst)
    }

    fn gen_call(&mut self, callee: &Expr, args: &[Expr], span: Span, expected: Option<&Ty>) -> (String, Ty) {
        // Вызов метода: expr.method(args) -> Type__method(self, args) или встроенный метод str.
        if let Expr::Field { base, field, .. } = callee {
            let bt = self.type_of(base);
            if is_str(&bt) {
                match field.as_str() {
                    "starts_with" => return self.gen_str_starts_with(base, args, span),
                    "ends_with" => return self.gen_str_ends_with(base, args, span),
                    "clone" => return self.gen_str_clone(base, args, span),
                    "is_empty" => return self.gen_str_is_empty(base, args, span),
                    _ => {}
                }
            }
            let tname = match &bt {
                Ty::Struct(n) => Some(n.clone()),
                Ty::Ptr(inner, _) => match &**inner {
                    Ty::Struct(n) => Some(n.clone()),
                    _ => None,
                },
                _ => None,
            };
            if let Some(tn) = tname {
                let mangled = format!("{tn}__{field}");
                if self.ctx.fns.contains_key(&mangled) {
                    return self.gen_method_call(&mangled, base, &bt, args, span);
                }
            }
        }

        let name = match callee {
            Expr::Ident(n, _) => n.clone(),
            other => {
                // Непрямой вызов через любое FnPtr-выражение (напр. ops[i](x)).
                if let Ty::FnPtr(params, ret) = self.type_of(other) {
                    let (fp, _) = self.gen_expr(other, None);
                    return self.gen_indirect_call_ptr(&fp, "функция", &params, &ret, args, span);
                }
                self.err("E0070", callee.span(), "вызывать можно только функцию по имени, метод `x.m(...)` или значение-указатель на функцию".into(), None);
                return ("0".into(), Ty::Err);
            }
        };

        // Локальная переменная-функция (в т.ч. хендл jit) — непрямой вызов.
        if let Some(local) = self.lookup(&name).cloned() {
            if let Ty::FnPtr(params, ret) = local.ty {
                return self.gen_indirect_call(&name, &local.slot, &params, &ret, args, span);
            }
        }

        let sig = match self.ctx.fns.get(&name) {
            Some(s) => s.clone(),
            None => {
                // Не пользовательская функция — возможно, встроенная математика.
                if let Some(r) = self.try_builtin(&name, args, span, expected) {
                    return r;
                }
                self.err("E0071", span, format!("вызов неизвестной функции `{name}`"), Some("объявите её, добавьте `extern fn`, либо это не встроенная math-функция"));
                return ("0".into(), Ty::Err);
            }
        };

        // Внутри jit-шаблона можно звать только extern C и math-builtin —
        // пользовательские функции не экспортируются и не резолвятся в рантайме.
        if self.in_jit_template && !sig.is_extern {
            self.err(
                "E0095",
                span,
                format!("вызов пользовательской функции `{name}` из jit-блока пока не поддержан"),
                Some("в jit-блоке доступны extern C-функции и встроенная математика"),
            );
            return ("0".into(), Ty::Err);
        }

        if sig.is_unsafe {
            self.require_unsafe(span, format!("вызов unsafe-функции `{name}`"));
        }

        // Проверка арности.
        if sig.variadic {
            if args.len() < sig.params.len() {
                self.err("E0072", span, format!("функции `{name}` нужно минимум {} аргумент(ов), передано {}", sig.params.len(), args.len()), None);
            }
        } else if args.len() != sig.params.len() {
            self.err("E0072", span, format!("функция `{name}` ждёт {} аргумент(ов), передано {}", sig.params.len(), args.len()), None);
        }

        let mut argvals: Vec<String> = Vec::new();
        for (i, a) in args.iter().enumerate() {
            let expected = sig.params.get(i).cloned();
            let (mut v, mut vty) = self.gen_expr(a, expected.as_ref());
            // Проверка типа фиксированных параметров.
            if let Some(pt) = &expected {
                if !compat(pt, &vty) && vty != Ty::Err && *pt != Ty::Err {
                    self.err(
                        "E0073",
                        a.span(),
                        format!("аргумент {} функции `{name}`: ожидался `{}`, передан `{}`", i + 1, pt.name(), vty.name()),
                        Some("приведите значение через `as`"),
                    );
                }
                // агрегат по значению -> грузим из адреса
                if is_aggregate(pt) {
                    let t = self.fresh_tmp();
                    self.emit(format!("{t} = load {ty}, ptr {v}", ty = pt.llvm()));
                    v = t;
                    vty = pt.clone();
                }
            } else {
                // вариадический хвост: приведение по правилам C
                (v, vty) = self.promote_variadic(v, vty);
            }
            argvals.push(format!("{} {}", vty.llvm(), v));
        }

        if sig.variadic {
            let param_tys: Vec<String> = sig.params.iter().map(|t| t.llvm()).collect();
            let mut plist = param_tys.join(", ");
            if plist.is_empty() {
                plist.push_str("...");
            } else {
                plist.push_str(", ...");
            }
            let fn_sym = llvm_global(&name);
            if sig.ret == Ty::Void {
                self.emit(format!("call void ({plist}) {fn_sym}({})", argvals.join(", ")));
                ("".into(), Ty::Void)
            } else {
                let t = self.fresh_tmp();
                self.emit(format!("{t} = call {rty} ({plist}) {fn_sym}({})", argvals.join(", "), rty = sig.ret.llvm()));
                self.spill_if_aggregate(t, sig.ret)
            }
        } else {
            let fn_sym = llvm_global(&name);
            if sig.ret == Ty::Void {
                self.emit(format!("call void {fn_sym}({})", argvals.join(", ")));
                ("".into(), Ty::Void)
            } else {
                let t = self.fresh_tmp();
                self.emit(format!("{t} = call {rty} {fn_sym}({})", argvals.join(", "), rty = sig.ret.llvm()));
                self.spill_if_aggregate(t, sig.ret)
            }
        }
    }


    /// Функция, вернувшая агрегат (структуру/срез), отдаёт его ЗНАЧЕНИЕМ, а по
    /// нашему соглашению агрегаты представляются АДРЕСОМ. Спиллим во временный
    /// слот и возвращаем адрес.
    fn spill_if_aggregate(&mut self, val: String, ty: Ty) -> (String, Ty) {
        if is_aggregate(&ty) {
            let slot = self.fresh_slot("ret");
            self.alloca(&slot, &ty);
            self.emit(format!("store {t} {val}, ptr {slot}", t = ty.llvm()));
            (slot, ty)
        } else {
            (val, ty)
        }
    }

    /// Встроенная функциональная математика, ложащаяся на LLVM-интринзики.
    /// Возвращает None, если имя не является builtin.
    fn try_builtin(&mut self, name: &str, args: &[Expr], span: Span, expected: Option<&Ty>) -> Option<(String, Ty)> {
        // Куча и работа с памятью — разблокируют динамические структуры
        // (Vec/Bytes/String можно писать на самом Goraw поверх этого).
        match name {
            "alloc" | "free" | "realloc" | "mem_copy" | "mem_set" => {
                return Some(self.heap_builtin(name, args, span));
            }
            "make_slice" => return Some(self.bi_make_slice(args, span)),
            "panic" => return Some(self.bi_panic(args, span)),
            "f32_bits" | "f64_bits" => return Some(self.bi_bits(name, args, span)),
            "f32_from_bits" | "f64_from_bits" => return Some(self.bi_from_bits(name, args, span)),
            "sizeof" => return Some(self.bi_sizeof(args, span)),
            "zeroed" => return Some(self.bi_zeroed(expected, span)),
            "print" | "println" => return Some(self.bi_print(name == "println", args, span)),
            "str_from_cstr" => return Some(self.bi_str_from_cstr(args, span)),
            "obf" | "obf_str" => return Some(self.bi_obf(args, span, expected)),
            _ => {}
        }

        // Классификация builtin по имени.
        enum Kind {
            FUnary,             // f(x): float -> float
            FBinary,            // f(x,y): (float,float) -> float
            NMinMax(bool),      // min/max: float или int; bool = это max
            Abs,                // abs: float или int
            Clamp,              // clamp(x, lo, hi)
            Fma,                // fma(a,b,c)
        }
        let (kind, intr_base): (Kind, &str) = match name {
            "sqrt" => (Kind::FUnary, "sqrt"),
            "sin" => (Kind::FUnary, "sin"),
            "cos" => (Kind::FUnary, "cos"),
            "exp" => (Kind::FUnary, "exp"),
            "exp2" => (Kind::FUnary, "exp2"),
            "log" => (Kind::FUnary, "log"),
            "log2" => (Kind::FUnary, "log2"),
            "log10" => (Kind::FUnary, "log10"),
            "floor" => (Kind::FUnary, "floor"),
            "ceil" => (Kind::FUnary, "ceil"),
            "round" => (Kind::FUnary, "round"),
            "trunc" => (Kind::FUnary, "trunc"),
            "fabs" => (Kind::FUnary, "fabs"),
            "pow" => (Kind::FBinary, "pow"),
            "fma" => (Kind::Fma, "fma"),
            "min" => (Kind::NMinMax(false), ""),
            "max" => (Kind::NMinMax(true), ""),
            "abs" => (Kind::Abs, ""),
            "clamp" => (Kind::Clamp, ""),
            _ => return None,
        };

        let want = |n: usize| -> bool { args.len() == n };

        match kind {
            Kind::FUnary => {
                if !want(1) {
                    self.err("E0090", span, format!("`{name}` ждёт 1 аргумент, передано {}", args.len()), None);
                    return Some(("0".into(), Ty::Err));
                }
                let (v, ty) = self.gen_expr(&args[0], Some(&Ty::F64));
                if !ty.is_float() {
                    self.err("E0091", args[0].span(), format!("`{name}` применяется к f32/f64, а не к `{}`", ty.name()), Some("приведите через `as f64`"));
                    return Some(("0".into(), Ty::Err));
                }
                let suffix = if ty == Ty::F32 { "f32" } else { "f64" };
                let lty = ty.llvm();
                self.use_intrinsic(format!("declare {lty} @llvm.{intr_base}.{suffix}({lty})"));
                let t = self.fresh_tmp();
                self.emit(format!("{t} = call {lty} @llvm.{intr_base}.{suffix}({lty} {v})"));
                Some((t, ty))
            }
            Kind::FBinary => {
                if !want(2) {
                    self.err("E0090", span, format!("`{name}` ждёт 2 аргумента, передано {}", args.len()), None);
                    return Some(("0".into(), Ty::Err));
                }
                let (a, aty) = self.gen_expr(&args[0], Some(&Ty::F64));
                let (b, _bty) = self.gen_expr(&args[1], Some(&aty));
                if !aty.is_float() {
                    self.err("E0091", args[0].span(), format!("`{name}` применяется к float"), None);
                    return Some(("0".into(), Ty::Err));
                }
                let suffix = if aty == Ty::F32 { "f32" } else { "f64" };
                let lty = aty.llvm();
                self.use_intrinsic(format!("declare {lty} @llvm.{intr_base}.{suffix}({lty}, {lty})"));
                let t = self.fresh_tmp();
                self.emit(format!("{t} = call {lty} @llvm.{intr_base}.{suffix}({lty} {a}, {lty} {b})"));
                Some((t, aty))
            }
            Kind::Fma => {
                if !want(3) {
                    self.err("E0090", span, format!("`fma` ждёт 3 аргумента, передано {}", args.len()), None);
                    return Some(("0".into(), Ty::Err));
                }
                let (a, aty) = self.gen_expr(&args[0], Some(&Ty::F64));
                let (b, _) = self.gen_expr(&args[1], Some(&aty));
                let (c, _) = self.gen_expr(&args[2], Some(&aty));
                if !aty.is_float() {
                    self.err("E0091", span, "`fma` применяется к float".into(), None);
                    return Some(("0".into(), Ty::Err));
                }
                let suffix = if aty == Ty::F32 { "f32" } else { "f64" };
                let lty = aty.llvm();
                self.use_intrinsic(format!("declare {lty} @llvm.fma.{suffix}({lty}, {lty}, {lty})"));
                let t = self.fresh_tmp();
                self.emit(format!("{t} = call {lty} @llvm.fma.{suffix}({lty} {a}, {lty} {b}, {lty} {c})"));
                Some((t, aty))
            }
            Kind::NMinMax(is_max) => {
                if !want(2) {
                    self.err("E0090", span, format!("`{name}` ждёт 2 аргумента, передано {}", args.len()), None);
                    return Some(("0".into(), Ty::Err));
                }
                let (a, aty) = self.gen_expr(&args[0], None);
                let (b, _) = self.gen_expr(&args[1], Some(&aty));
                let (intr, lty) = if aty.is_float() {
                    (format!("{}num", if is_max { "max" } else { "min" }), aty.llvm())
                } else if aty.is_int() {
                    let s = if aty.is_signed() { if is_max { "smax" } else { "smin" } } else if is_max { "umax" } else { "umin" };
                    (s.to_string(), aty.llvm())
                } else {
                    self.err("E0091", span, format!("`{name}` применяется к числам, а не к `{}`", aty.name()), None);
                    return Some(("0".into(), Ty::Err));
                };
                let suffix = self.type_suffix(&aty);
                self.use_intrinsic(format!("declare {lty} @llvm.{intr}.{suffix}({lty}, {lty})"));
                let t = self.fresh_tmp();
                self.emit(format!("{t} = call {lty} @llvm.{intr}.{suffix}({lty} {a}, {lty} {b})"));
                Some((t, aty))
            }
            Kind::Abs => {
                if !want(1) {
                    self.err("E0090", span, format!("`abs` ждёт 1 аргумент, передано {}", args.len()), None);
                    return Some(("0".into(), Ty::Err));
                }
                let (v, ty) = self.gen_expr(&args[0], None);
                if ty.is_float() {
                    let suffix = if ty == Ty::F32 { "f32" } else { "f64" };
                    let lty = ty.llvm();
                    self.use_intrinsic(format!("declare {lty} @llvm.fabs.{suffix}({lty})"));
                    let t = self.fresh_tmp();
                    self.emit(format!("{t} = call {lty} @llvm.fabs.{suffix}({lty} {v})"));
                    Some((t, ty))
                } else if ty.is_int() {
                    let suffix = self.type_suffix(&ty);
                    let lty = ty.llvm();
                    self.use_intrinsic(format!("declare {lty} @llvm.abs.{suffix}({lty}, i1)"));
                    let t = self.fresh_tmp();
                    self.emit(format!("{t} = call {lty} @llvm.abs.{suffix}({lty} {v}, i1 false)"));
                    Some((t, ty))
                } else {
                    self.err("E0091", span, format!("`abs` применяется к числам, а не к `{}`", ty.name()), None);
                    Some(("0".into(), Ty::Err))
                }
            }
            Kind::Clamp => {
                if !want(3) {
                    self.err("E0090", span, format!("`clamp` ждёт 3 аргумента (x, lo, hi), передано {}", args.len()), None);
                    return Some(("0".into(), Ty::Err));
                }
                // clamp(x, lo, hi) = min(max(x, lo), hi)
                let inner = Expr::Call {
                    callee: Box::new(Expr::Ident("max".into(), span)),
                    args: vec![args[0].clone(), args[1].clone()],
                    span,
                };
                let outer = Expr::Call {
                    callee: Box::new(Expr::Ident("min".into(), span)),
                    args: vec![inner, args[2].clone()],
                    span,
                };
                Some(self.gen_expr(&outer, None))
            }
        }
    }

    /// Builtin'ы кучи и памяти: alloc/free/realloc/mem_copy/mem_set.
    /// Возвращают сырые указатели — работа через них требует `unsafe`, что и
    /// держит safe/unsafe-границу.
    fn heap_builtin(&mut self, name: &str, args: &[Expr], span: Span) -> (String, Ty) {
        let ptr_u8_mut = Ty::Ptr(Box::new(Ty::U8), true);
        match name {
            "alloc" => {
                if args.len() != 1 {
                    self.err("E0092", span, format!("`alloc` ждёт 1 аргумент (размер), передано {}", args.len()), None);
                    return ("null".into(), ptr_u8_mut);
                }
                let (n, _) = self.gen_expr(&args[0], Some(&Ty::I64));
                self.use_intrinsic("declare ptr @malloc(i64)".into());
                let t = self.fresh_tmp();
                self.emit(format!("{t} = call ptr @malloc(i64 {n})"));
                (t, ptr_u8_mut)
            }
            "realloc" => {
                if args.len() != 2 {
                    self.err("E0092", span, format!("`realloc` ждёт 2 аргумента (ptr, размер), передано {}", args.len()), None);
                    return ("null".into(), ptr_u8_mut);
                }
                let (p, pty) = self.gen_expr(&args[0], None);
                self.expect_ptr(&pty, args[0].span(), "realloc");
                let (n, _) = self.gen_expr(&args[1], Some(&Ty::I64));
                self.use_intrinsic("declare ptr @realloc(ptr, i64)".into());
                let t = self.fresh_tmp();
                self.emit(format!("{t} = call ptr @realloc(ptr {p}, i64 {n})"));
                (t, ptr_u8_mut)
            }
            "free" => {
                if args.len() != 1 {
                    self.err("E0092", span, format!("`free` ждёт 1 аргумент (ptr), передано {}", args.len()), None);
                    return ("".into(), Ty::Void);
                }
                let (p, pty) = self.gen_expr(&args[0], None);
                self.expect_ptr(&pty, args[0].span(), "free");
                self.use_intrinsic("declare void @free(ptr)".into());
                self.emit(format!("call void @free(ptr {p})"));
                ("".into(), Ty::Void)
            }
            "mem_copy" => {
                if args.len() != 3 {
                    self.err("E0092", span, format!("`mem_copy` ждёт 3 аргумента (dst, src, n), передано {}", args.len()), None);
                    return ("".into(), Ty::Void);
                }
                self.require_unsafe(span, "копирование памяти");
                let (dst, dty) = self.gen_expr(&args[0], None);
                self.expect_ptr(&dty, args[0].span(), "mem_copy");
                let (src, sty) = self.gen_expr(&args[1], None);
                self.expect_ptr(&sty, args[1].span(), "mem_copy");
                let (n, _) = self.gen_expr(&args[2], Some(&Ty::I64));
                self.use_intrinsic("declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)".into());
                self.emit(format!("call void @llvm.memcpy.p0.p0.i64(ptr {dst}, ptr {src}, i64 {n}, i1 false)"));
                ("".into(), Ty::Void)
            }
            "mem_set" => {
                if args.len() != 3 {
                    self.err("E0092", span, format!("`mem_set` ждёт 3 аргумента (dst, byte, n), передано {}", args.len()), None);
                    return ("".into(), Ty::Void);
                }
                self.require_unsafe(span, "заполнение памяти");
                let (dst, dty) = self.gen_expr(&args[0], None);
                self.expect_ptr(&dty, args[0].span(), "mem_set");
                let (val, _) = self.gen_expr(&args[1], Some(&Ty::I32));
                let (n, _) = self.gen_expr(&args[2], Some(&Ty::I64));
                let v8 = self.fresh_tmp();
                self.emit(format!("{v8} = trunc i32 {val} to i8"));
                self.use_intrinsic("declare void @llvm.memset.p0.i64(ptr, i8, i64, i1)".into());
                self.emit(format!("call void @llvm.memset.p0.i64(ptr {dst}, i8 {v8}, i64 {n}, i1 false)"));
                ("".into(), Ty::Void)
            }
            _ => unreachable!(),
        }
    }

    /// `make_slice(ptr, len)` — собирает срез `[]T` из указателя `*T`/`*mut T`
    /// и длины. Тип элемента берётся из указателя.
    fn bi_make_slice(&mut self, args: &[Expr], span: Span) -> (String, Ty) {
        if args.len() != 2 {
            self.err("E0094", span, format!("`make_slice` ждёт 2 аргумента (ptr, len), передано {}", args.len()), None);
            return ("null".into(), Ty::Err);
        }
        let (p, pty) = self.gen_expr(&args[0], None);
        let elem = match &pty {
            Ty::Ptr(inner, _) => (**inner).clone(),
            Ty::Err => return ("null".into(), Ty::Err),
            other => {
                self.err("E0094", args[0].span(), format!("`make_slice` ждёт указатель, а тут `{}`", other.name()), None);
                return ("null".into(), Ty::Err);
            }
        };
        let (n, _) = self.gen_expr(&args[1], Some(&Ty::I64));
        let slot = self.fresh_slot("slice");
        self.alloca(&slot, &Ty::Slice(Box::new(elem.clone())));
        let pf = self.fresh_tmp();
        self.emit(format!("{pf} = getelementptr %slice, ptr {slot}, i32 0, i32 0"));
        self.emit(format!("store ptr {p}, ptr {pf}"));
        let lf = self.fresh_tmp();
        self.emit(format!("{lf} = getelementptr %slice, ptr {slot}, i32 0, i32 1"));
        self.emit(format!("store i64 {n}, ptr {lf}"));
        (slot, Ty::Slice(Box::new(elem)))
    }

    /// Срез: `base[start..end]`, `base[start..]`, `base[..end]`, `base[..]`.
    fn gen_slice(
        &mut self,
        base: &Expr,
        start_expr: Option<&Expr>,
        end_expr: Option<&Expr>,
        span: Span,
    ) -> (String, Ty) {
        let (bv, bty) = self.gen_expr(base, None);
        let (elem, data_ptr, cur_len) = match bty {
            Ty::Slice(elem) => {
                let dp = self.fresh_tmp();
                self.emit(format!("{dp} = getelementptr %slice, ptr {bv}, i32 0, i32 0"));
                let data = self.fresh_tmp();
                self.emit(format!("{data} = load ptr, ptr {dp}"));
                let lp = self.fresh_tmp();
                self.emit(format!("{lp} = getelementptr %slice, ptr {bv}, i32 0, i32 1"));
                let len = self.fresh_tmp();
                self.emit(format!("{len} = load i64, ptr {lp}"));
                (*elem, data, len)
            }
            Ty::Array(elem, n) => {
                let len = n.to_string();
                let data = self.fresh_tmp();
                self.emit(format!("{data} = getelementptr [{n} x {ety}], ptr {bv}, i64 0, i64 0", ety = elem.llvm()));
                (*elem, data, len)
            }
            Ty::Ptr(elem, _) => {
                if end_expr.is_none() {
                    self.err(
                        "E0096",
                        span,
                        "для взятия среза от сырого указателя необходимо указать верхнюю границу: `ptr[start..end]`".into(),
                        None,
                    );
                    return ("null".into(), Ty::Err);
                }
                (*elem, bv, "".to_string())
            }
            Ty::Err => return ("null".into(), Ty::Err),
            other => {
                self.err(
                    "E0097",
                    base.span(),
                    format!("срез можно брать только от среза, массива или указателя, а тут `{}`", other.name()),
                    None,
                );
                return ("null".into(), Ty::Err);
            }
        };

        let start_val = if let Some(se) = start_expr {
            let (sv, _) = self.gen_expr(se, Some(&Ty::I64));
            sv
        } else {
            "0".to_string()
        };

        let end_val = if let Some(ee) = end_expr {
            let (ev, _) = self.gen_expr(ee, Some(&Ty::I64));
            ev
        } else {
            cur_len.clone()
        };

        if !cur_len.is_empty() {
            self.emit_slice_bounds_check(&start_val, &end_val, &cur_len);
        } else {
            let s_lt_0 = self.fresh_tmp();
            self.emit(format!("{s_lt_0} = icmp slt i64 {start_val}, 0"));
            let e_lt_s = self.fresh_tmp();
            self.emit(format!("{e_lt_s} = icmp slt i64 {end_val}, {start_val}"));
            let bad = self.fresh_tmp();
            self.emit(format!("{bad} = or i1 {s_lt_0}, {e_lt_s}"));
            let fail = self.fresh_label("ptr_slice_oob");
            let ok = self.fresh_label("ptr_slice_inb");
            self.emit(format!("br i1 {bad}, label %{fail}, label %{ok}"));
            self.emit_label(&fail);
            if !self.ctx.fns.contains_key("abort") {
                self.use_intrinsic("declare void @abort()".into());
            }
            self.emit("call void @abort()".into());
            self.emit("unreachable".into());
            self.emit_label(&ok);
        }

        let new_len = self.fresh_tmp();
        self.emit(format!("{new_len} = sub i64 {end_val}, {start_val}"));
        let new_data = self.fresh_tmp();
        self.emit(format!("{new_data} = getelementptr {ety}, ptr {data_ptr}, i64 {start_val}", ety = elem.llvm()));

        let slot = self.fresh_slot("slice");
        self.alloca(&slot, &Ty::Slice(Box::new(elem.clone())));
        let pf = self.fresh_tmp();
        self.emit(format!("{pf} = getelementptr %slice, ptr {slot}, i32 0, i32 0"));
        self.emit(format!("store ptr {new_data}, ptr {pf}"));
        let lf = self.fresh_tmp();
        self.emit(format!("{lf} = getelementptr %slice, ptr {slot}, i32 0, i32 1"));
        self.emit(format!("store i64 {new_len}, ptr {lf}"));
        (slot, Ty::Slice(Box::new(elem)))
    }

    /// Проверка границ среза: 0 <= start <= end <= len.
    fn emit_slice_bounds_check(&mut self, start: &str, end: &str, len: &str) {
        let s_lt_0 = self.fresh_tmp();
        self.emit(format!("{s_lt_0} = icmp slt i64 {start}, 0"));
        let e_lt_s = self.fresh_tmp();
        self.emit(format!("{e_lt_s} = icmp slt i64 {end}, {start}"));
        let e_gt_l = self.fresh_tmp();
        self.emit(format!("{e_gt_l} = icmp sgt i64 {end}, {len}"));

        let bad1 = self.fresh_tmp();
        self.emit(format!("{bad1} = or i1 {s_lt_0}, {e_lt_s}"));
        let bad2 = self.fresh_tmp();
        self.emit(format!("{bad2} = or i1 {bad1}, {e_gt_l}"));

        let fail = self.fresh_label("slice_oob");
        let ok = self.fresh_label("slice_inb");
        self.emit(format!("br i1 {bad2}, label %{fail}, label %{ok}"));
        self.emit_label(&fail);
        if !self.ctx.fns.contains_key("abort") {
            self.use_intrinsic("declare void @abort()".into());
        }
        self.emit("call void @abort()".into());
        self.emit("unreachable".into());
        self.emit_label(&ok);
    }

    /// Конкатенация строк `+`: выделяет память через malloc, копирует байты обеих строк,
    /// ставит завершающий NUL-байт для C-совместимости и возвращает срез `str`.
    fn gen_str_concat(&mut self, lv: &str, rv: &str) -> (String, Ty) {
        let l_dp = self.fresh_tmp();
        self.emit(format!("{l_dp} = getelementptr %slice, ptr {lv}, i32 0, i32 0"));
        let l_data = self.fresh_tmp();
        self.emit(format!("{l_data} = load ptr, ptr {l_dp}"));
        let l_lp = self.fresh_tmp();
        self.emit(format!("{l_lp} = getelementptr %slice, ptr {lv}, i32 0, i32 1"));
        let l_len = self.fresh_tmp();
        self.emit(format!("{l_len} = load i64, ptr {l_lp}"));

        let r_dp = self.fresh_tmp();
        self.emit(format!("{r_dp} = getelementptr %slice, ptr {rv}, i32 0, i32 0"));
        let r_data = self.fresh_tmp();
        self.emit(format!("{r_data} = load ptr, ptr {r_dp}"));
        let r_lp = self.fresh_tmp();
        self.emit(format!("{r_lp} = getelementptr %slice, ptr {rv}, i32 0, i32 1"));
        let r_len = self.fresh_tmp();
        self.emit(format!("{r_len} = load i64, ptr {r_lp}"));

        let total_len = self.fresh_tmp();
        self.emit(format!("{total_len} = add i64 {l_len}, {r_len}"));
        let alloc_len = self.fresh_tmp();
        self.emit(format!("{alloc_len} = add i64 {total_len}, 1"));

        if !self.ctx.fns.contains_key("malloc") {
            self.use_intrinsic("declare ptr @malloc(i64)".into());
        }
        let buf = self.fresh_tmp();
        self.emit(format!("{buf} = call ptr @malloc(i64 {alloc_len})"));

        self.use_intrinsic("declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)".into());
        self.emit(format!("call void @llvm.memcpy.p0.p0.i64(ptr {buf}, ptr {l_data}, i64 {l_len}, i1 false)"));

        let dest_r = self.fresh_tmp();
        self.emit(format!("{dest_r} = getelementptr i8, ptr {buf}, i64 {l_len}"));
        self.emit(format!("call void @llvm.memcpy.p0.p0.i64(ptr {dest_r}, ptr {r_data}, i64 {r_len}, i1 false)"));

        let nul_p = self.fresh_tmp();
        self.emit(format!("{nul_p} = getelementptr i8, ptr {buf}, i64 {total_len}"));
        self.emit(format!("store i8 0, ptr {nul_p}"));

        let slot = self.fresh_slot("strconcat");
        self.alloca(&slot, &Ty::Slice(Box::new(Ty::U8)));
        let pf = self.fresh_tmp();
        self.emit(format!("{pf} = getelementptr %slice, ptr {slot}, i32 0, i32 0"));
        self.emit(format!("store ptr {buf}, ptr {pf}"));
        let lf = self.fresh_tmp();
        self.emit(format!("{lf} = getelementptr %slice, ptr {slot}, i32 0, i32 1"));
        self.emit(format!("store i64 {total_len}, ptr {lf}"));
        (slot, Ty::Slice(Box::new(Ty::U8)))
    }

    /// Сравнение строк `==` и `!=`: сначала сравнивает длины, при совпадении — `memcmp`.
    fn gen_str_cmp(&mut self, op: BinOp, lv: &str, rv: &str) -> (String, Ty) {
        let l_dp = self.fresh_tmp();
        self.emit(format!("{l_dp} = getelementptr %slice, ptr {lv}, i32 0, i32 0"));
        let l_data = self.fresh_tmp();
        self.emit(format!("{l_data} = load ptr, ptr {l_dp}"));
        let l_lp = self.fresh_tmp();
        self.emit(format!("{l_lp} = getelementptr %slice, ptr {lv}, i32 0, i32 1"));
        let l_len = self.fresh_tmp();
        self.emit(format!("{l_len} = load i64, ptr {l_lp}"));

        let r_dp = self.fresh_tmp();
        self.emit(format!("{r_dp} = getelementptr %slice, ptr {rv}, i32 0, i32 0"));
        let r_data = self.fresh_tmp();
        self.emit(format!("{r_data} = load ptr, ptr {r_dp}"));
        let r_lp = self.fresh_tmp();
        self.emit(format!("{r_lp} = getelementptr %slice, ptr {rv}, i32 0, i32 1"));
        let r_len = self.fresh_tmp();
        self.emit(format!("{r_len} = load i64, ptr {r_lp}"));

        let len_eq = self.fresh_tmp();
        self.emit(format!("{len_eq} = icmp eq i64 {l_len}, {r_len}"));

        let cmp_bb = self.fresh_label("streq_cmp");
        let end_bb = self.fresh_label("streq_end");
        let res_slot = self.fresh_slot("streq_res");
        self.alloca(&res_slot, &Ty::Bool);
        self.emit(format!("store i1 {}, ptr {res_slot}", if op == BinOp::Eq { "false" } else { "true" }));
        self.emit(format!("br i1 {len_eq}, label %{cmp_bb}, label %{end_bb}"));

        self.emit_label(&cmp_bb);
        if !self.ctx.fns.contains_key("memcmp") {
            self.use_intrinsic("declare i32 @memcmp(ptr, ptr, i64)".into());
        }
        let cmp = self.fresh_tmp();
        self.emit(format!("{cmp} = call i32 @memcmp(ptr {l_data}, ptr {r_data}, i64 {l_len})"));
        let is_zero = self.fresh_tmp();
        let pred = if op == BinOp::Eq { "eq" } else { "ne" };
        self.emit(format!("{is_zero} = icmp {pred} i32 {cmp}, 0"));
        self.emit(format!("store i1 {is_zero}, ptr {res_slot}"));
        self.emit(format!("br label %{end_bb}"));

        self.emit_label(&end_bb);
        let final_res = self.fresh_tmp();
        self.emit(format!("{final_res} = load i1, ptr {res_slot}"));
        (final_res, Ty::Bool)
    }

    fn gen_str_starts_with(&mut self, base: &Expr, args: &[Expr], span: Span) -> (String, Ty) {
        if args.len() != 1 {
            self.err("E0098", span, format!("`starts_with` ждёт 1 аргумент (prefix: str), передано {}", args.len()), None);
            return ("0".into(), Ty::Bool);
        }
        let (sv, sty) = self.gen_expr(base, None);
        let (pv, pty) = self.gen_expr(&args[0], Some(&Ty::Slice(Box::new(Ty::U8))));
        if !is_str(&sty) || !is_str(&pty) {
            self.err("E0098", span, "`starts_with` вызывается на str с аргументом str".into(), None);
            return ("0".into(), Ty::Bool);
        }

        let slen_p = self.fresh_tmp();
        self.emit(format!("{slen_p} = getelementptr %slice, ptr {sv}, i32 0, i32 1"));
        let slen = self.fresh_tmp();
        self.emit(format!("{slen} = load i64, ptr {slen_p}"));

        let plen_p = self.fresh_tmp();
        self.emit(format!("{plen_p} = getelementptr %slice, ptr {pv}, i32 0, i32 1"));
        let plen = self.fresh_tmp();
        self.emit(format!("{plen} = load i64, ptr {plen_p}"));

        let can_fit = self.fresh_tmp();
        self.emit(format!("{can_fit} = icmp sge i64 {slen}, {plen}"));

        let cmp_bb = self.fresh_label("sw_cmp");
        let end_bb = self.fresh_label("sw_end");
        let res_slot = self.fresh_slot("sw_res");
        self.alloca(&res_slot, &Ty::Bool);
        self.emit(format!("store i1 false, ptr {res_slot}"));
        self.emit(format!("br i1 {can_fit}, label %{cmp_bb}, label %{end_bb}"));

        self.emit_label(&cmp_bb);
        let sdata_p = self.fresh_tmp();
        self.emit(format!("{sdata_p} = getelementptr %slice, ptr {sv}, i32 0, i32 0"));
        let sdata = self.fresh_tmp();
        self.emit(format!("{sdata} = load ptr, ptr {sdata_p}"));

        let pdata_p = self.fresh_tmp();
        self.emit(format!("{pdata_p} = getelementptr %slice, ptr {pv}, i32 0, i32 0"));
        let pdata = self.fresh_tmp();
        self.emit(format!("{pdata} = load ptr, ptr {pdata_p}"));

        if !self.ctx.fns.contains_key("memcmp") {
            self.use_intrinsic("declare i32 @memcmp(ptr, ptr, i64)".into());
        }
        let cmp = self.fresh_tmp();
        self.emit(format!("{cmp} = call i32 @memcmp(ptr {sdata}, ptr {pdata}, i64 {plen})"));
        let is_eq = self.fresh_tmp();
        self.emit(format!("{is_eq} = icmp eq i32 {cmp}, 0"));
        self.emit(format!("store i1 {is_eq}, ptr {res_slot}"));
        self.emit(format!("br label %{end_bb}"));

        self.emit_label(&end_bb);
        let res = self.fresh_tmp();
        self.emit(format!("{res} = load i1, ptr {res_slot}"));
        (res, Ty::Bool)
    }

    fn gen_str_ends_with(&mut self, base: &Expr, args: &[Expr], span: Span) -> (String, Ty) {
        if args.len() != 1 {
            self.err("E0098", span, format!("`ends_with` ждёт 1 аргумент (suffix: str), передано {}", args.len()), None);
            return ("0".into(), Ty::Bool);
        }
        let (sv, sty) = self.gen_expr(base, None);
        let (pv, pty) = self.gen_expr(&args[0], Some(&Ty::Slice(Box::new(Ty::U8))));
        if !is_str(&sty) || !is_str(&pty) {
            self.err("E0098", span, "`ends_with` вызывается на str с аргументом str".into(), None);
            return ("0".into(), Ty::Bool);
        }

        let slen_p = self.fresh_tmp();
        self.emit(format!("{slen_p} = getelementptr %slice, ptr {sv}, i32 0, i32 1"));
        let slen = self.fresh_tmp();
        self.emit(format!("{slen} = load i64, ptr {slen_p}"));

        let plen_p = self.fresh_tmp();
        self.emit(format!("{plen_p} = getelementptr %slice, ptr {pv}, i32 0, i32 1"));
        let plen = self.fresh_tmp();
        self.emit(format!("{plen} = load i64, ptr {plen_p}"));

        let can_fit = self.fresh_tmp();
        self.emit(format!("{can_fit} = icmp sge i64 {slen}, {plen}"));

        let cmp_bb = self.fresh_label("ew_cmp");
        let end_bb = self.fresh_label("ew_end");
        let res_slot = self.fresh_slot("ew_res");
        self.alloca(&res_slot, &Ty::Bool);
        self.emit(format!("store i1 false, ptr {res_slot}"));
        self.emit(format!("br i1 {can_fit}, label %{cmp_bb}, label %{end_bb}"));

        self.emit_label(&cmp_bb);
        let sdata_p = self.fresh_tmp();
        self.emit(format!("{sdata_p} = getelementptr %slice, ptr {sv}, i32 0, i32 0"));
        let sdata = self.fresh_tmp();
        self.emit(format!("{sdata} = load ptr, ptr {sdata_p}"));

        let pdata_p = self.fresh_tmp();
        self.emit(format!("{pdata_p} = getelementptr %slice, ptr {pv}, i32 0, i32 0"));
        let pdata = self.fresh_tmp();
        self.emit(format!("{pdata} = load ptr, ptr {pdata_p}"));

        let offset = self.fresh_tmp();
        self.emit(format!("{offset} = sub i64 {slen}, {plen}"));
        let sub_ptr = self.fresh_tmp();
        self.emit(format!("{sub_ptr} = getelementptr i8, ptr {sdata}, i64 {offset}"));

        if !self.ctx.fns.contains_key("memcmp") {
            self.use_intrinsic("declare i32 @memcmp(ptr, ptr, i64)".into());
        }
        let cmp = self.fresh_tmp();
        self.emit(format!("{cmp} = call i32 @memcmp(ptr {sub_ptr}, ptr {pdata}, i64 {plen})"));
        let is_eq = self.fresh_tmp();
        self.emit(format!("{is_eq} = icmp eq i32 {cmp}, 0"));
        self.emit(format!("store i1 {is_eq}, ptr {res_slot}"));
        self.emit(format!("br label %{end_bb}"));

        self.emit_label(&end_bb);
        let res = self.fresh_tmp();
        self.emit(format!("{res} = load i1, ptr {res_slot}"));
        (res, Ty::Bool)
    }

    fn gen_str_clone(&mut self, base: &Expr, args: &[Expr], span: Span) -> (String, Ty) {
        if !args.is_empty() {
            self.err("E0098", span, format!("`clone` не принимает аргументов, передано {}", args.len()), None);
        }
        let (sv, sty) = self.gen_expr(base, None);
        if !is_str(&sty) {
            self.err("E0098", span, "`clone` вызывается на str".into(), None);
            return ("null".into(), Ty::Err);
        }

        let sdata_p = self.fresh_tmp();
        self.emit(format!("{sdata_p} = getelementptr %slice, ptr {sv}, i32 0, i32 0"));
        let sdata = self.fresh_tmp();
        self.emit(format!("{sdata} = load ptr, ptr {sdata_p}"));

        let slen_p = self.fresh_tmp();
        self.emit(format!("{slen_p} = getelementptr %slice, ptr {sv}, i32 0, i32 1"));
        let slen = self.fresh_tmp();
        self.emit(format!("{slen} = load i64, ptr {slen_p}"));

        let alloc_len = self.fresh_tmp();
        self.emit(format!("{alloc_len} = add i64 {slen}, 1"));

        if !self.ctx.fns.contains_key("malloc") {
            self.use_intrinsic("declare ptr @malloc(i64)".into());
        }
        let buf = self.fresh_tmp();
        self.emit(format!("{buf} = call ptr @malloc(i64 {alloc_len})"));

        self.use_intrinsic("declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)".into());
        self.emit(format!("call void @llvm.memcpy.p0.p0.i64(ptr {buf}, ptr {sdata}, i64 {slen}, i1 false)"));

        let nul_p = self.fresh_tmp();
        self.emit(format!("{nul_p} = getelementptr i8, ptr {buf}, i64 {slen}"));
        self.emit(format!("store i8 0, ptr {nul_p}"));

        let slot = self.fresh_slot("strclone");
        self.alloca(&slot, &Ty::Slice(Box::new(Ty::U8)));
        let pf = self.fresh_tmp();
        self.emit(format!("{pf} = getelementptr %slice, ptr {slot}, i32 0, i32 0"));
        self.emit(format!("store ptr {buf}, ptr {pf}"));
        let lf = self.fresh_tmp();
        self.emit(format!("{lf} = getelementptr %slice, ptr {slot}, i32 0, i32 1"));
        self.emit(format!("store i64 {slen}, ptr {lf}"));
        (slot, Ty::Slice(Box::new(Ty::U8)))
    }

    fn gen_str_is_empty(&mut self, base: &Expr, args: &[Expr], span: Span) -> (String, Ty) {
        if !args.is_empty() {
            self.err("E0098", span, format!("`is_empty` не принимает аргументов, передано {}", args.len()), None);
        }
        let (sv, sty) = self.gen_expr(base, None);
        if !is_str(&sty) {
            self.err("E0098", span, "`is_empty` вызывается на str".into(), None);
            return ("0".into(), Ty::Bool);
        }
        let slen_p = self.fresh_tmp();
        self.emit(format!("{slen_p} = getelementptr %slice, ptr {sv}, i32 0, i32 1"));
        let slen = self.fresh_tmp();
        self.emit(format!("{slen} = load i64, ptr {slen_p}"));
        let is_z = self.fresh_tmp();
        self.emit(format!("{is_z} = icmp eq i64 {slen}, 0"));
        (is_z, Ty::Bool)
    }

    /// `str_from_cstr(ptr)` — срез `str` из C-строки (*u8) через `strlen`.
    fn bi_str_from_cstr(&mut self, args: &[Expr], span: Span) -> (String, Ty) {
        if args.len() != 1 {
            self.err("E0094", span, format!("`str_from_cstr` ждёт 1 аргумент (*u8), передано {}", args.len()), None);
            return ("null".into(), Ty::Err);
        }
        let (p, _) = self.gen_expr(&args[0], Some(&Ty::Ptr(Box::new(Ty::U8), false)));
        if !self.ctx.fns.contains_key("strlen") {
            self.use_intrinsic("declare i64 @strlen(ptr)".into());
        }
        let len = self.fresh_tmp();
        self.emit(format!("{len} = call i64 @strlen(ptr {p})"));

        let slot = self.fresh_slot("from_cstr");
        self.alloca(&slot, &Ty::Slice(Box::new(Ty::U8)));
        let pf = self.fresh_tmp();
        self.emit(format!("{pf} = getelementptr %slice, ptr {slot}, i32 0, i32 0"));
        self.emit(format!("store ptr {p}, ptr {pf}"));
        let lf = self.fresh_tmp();
        self.emit(format!("{lf} = getelementptr %slice, ptr {slot}, i32 0, i32 1"));
        self.emit(format!("store i64 {len}, ptr {lf}"));
        (slot, Ty::Slice(Box::new(Ty::U8)))
    }

    /// `obf("...")` / `obf_str("...")` — маркер встроенной обфускации строк.
    fn bi_obf(&mut self, args: &[Expr], span: Span, expected: Option<&Ty>) -> (String, Ty) {
        if args.len() != 1 {
            self.err("E1300", span, format!("`obf` ожидает ровно 1 аргумент (строковый литерал), передано {}", args.len()), Some("пример: obf(\"secret_data\")"));
            return ("null".into(), Ty::Slice(Box::new(Ty::U8)));
        }
        match &args[0] {
            Expr::Str(s, _) => self.gen_obfuscated_string(s, expected),
            _ => {
                self.err("E1301", span, "аргумент `obf(...)` должен быть строковым литералом на этапе компиляции".into(), Some("передайте строку прямо в кавычках: obf(\"...\")"));
                ("null".into(), Ty::Slice(Box::new(Ty::U8)))
            }
        }
    }

    /// Шифрование строки (циклический сдвиг + аддитивный XOR шифр).
    /// data[i] = raw[i] ^ ((rotl32(key, i % 32) + i) as u8)
    pub fn obf_encrypt_str(s: &str, key: u32) -> Vec<u8> {
        let raw = s.as_bytes();
        let mut data = Vec::with_capacity(raw.len() + 1);
        for i in 0..=raw.len() {
            let b = if i < raw.len() { raw[i] } else { 0 };
            let shift = (i % 32) as u32;
            let rot = key.rotate_left(shift);
            let mask = rot.wrapping_add(i as u32) as u8;
            data.push(b ^ mask);
        }
        data
    }

    /// Генерация 32-битного ключа для обфускации строки.
    fn next_obf_key(&mut self, s: &str) -> u32 {
        let mut h = 0x811c9dc5u32.wrapping_add((self.obf_count + 1).wrapping_mul(0x5bd1e995));
        for &b in s.as_bytes() {
            h = (h ^ (b as u32)).wrapping_mul(0x01000193);
        }
        let mix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0x1337c0de);
        let mut key = h ^ mix;
        if key == 0 {
            key = 0x5a17e0b1;
        }
        key
    }

    /// Гарантирует наличие рантайм-функции расшифровки `@__goraw_decrypt_str` и интринзика `@llvm.fshl.i32`.
    fn ensure_obf_decrypt_runtime(&mut self) {
        if self.has_obf_decrypt_runtime {
            return;
        }
        self.has_obf_decrypt_runtime = true;
        self.intrinsics.insert("declare i32 @llvm.fshl.i32(i32, i32, i32)".to_string());
    }

    /// Генерация зашифрованной строковой константы и вызова расшифровки.
    fn gen_obfuscated_string(&mut self, s: &str, expected: Option<&Ty>) -> (String, Ty) {
        self.ensure_obf_decrypt_runtime();

        let idx = self.obf_count;
        self.obf_count += 1;

        let key = self.next_obf_key(s);
        let enc_bytes = Self::obf_encrypt_str(s, key);
        let total_bytes = enc_bytes.len();

        let enc_label = format!("@.obf.enc.{}", idx);
        let buf_label = format!("@.obf.buf.{}", idx);
        let init_label = format!("@.obf.init.{}", idx);

        let mut enc_str = String::new();
        for &b in &enc_bytes {
            enc_str.push_str(&format!("\\{:02X}", b));
        }

        self.strings.push_str(&format!(
            "{enc_label} = private unnamed_addr constant [{total_bytes} x i8] c\"{enc_str}\"\n"
        ));
        self.strings.push_str(&format!(
            "{buf_label} = internal global [{total_bytes} x i8] zeroinitializer\n"
        ));
        self.strings.push_str(&format!(
            "{init_label} = internal global i1 false\n"
        ));

        let dec_ptr = self.fresh_tmp();
        self.emit(format!(
            "{dec_ptr} = call ptr @__goraw_decrypt_str(ptr {enc_label}, ptr {buf_label}, i64 {total_bytes}, i32 {key}, ptr {init_label})"
        ));

        let want_raw_ptr = matches!(expected, Some(Ty::Ptr(..)));
        if !want_raw_ptr {
            let str_len = s.as_bytes().len();
            let slot = self.fresh_slot("obfstr");
            self.alloca(&slot, &Ty::Slice(Box::new(Ty::U8)));
            let pf = self.fresh_tmp();
            self.emit(format!("{pf} = getelementptr %slice, ptr {slot}, i32 0, i32 0"));
            self.emit(format!("store ptr {dec_ptr}, ptr {pf}"));
            let lf = self.fresh_tmp();
            self.emit(format!("{lf} = getelementptr %slice, ptr {slot}, i32 0, i32 1"));
            self.emit(format!("store i64 {str_len}, ptr {lf}"));
            (slot, Ty::Slice(Box::new(Ty::U8)))
        } else {
            (dec_ptr, Ty::Ptr(Box::new(Ty::U8), false))
        }
    }

    /// `print(...)` и `println(...)` — полиморфный вывод строк, чисел, bool, указателей.
    fn bi_print(&mut self, newline: bool, args: &[Expr], _span: Span) -> (String, Ty) {
        if !self.ctx.fns.contains_key("printf") {
            self.use_intrinsic("declare i32 @printf(ptr, ...)".into());
        }
        for arg in args {
            let (v, ty) = self.gen_expr(arg, None);
            if is_str(&ty) {
                let dp = self.fresh_tmp();
                self.emit(format!("{dp} = getelementptr %slice, ptr {v}, i32 0, i32 0"));
                let data = self.fresh_tmp();
                self.emit(format!("{data} = load ptr, ptr {dp}"));
                let lp = self.fresh_tmp();
                self.emit(format!("{lp} = getelementptr %slice, ptr {v}, i32 0, i32 1"));
                let len = self.fresh_tmp();
                self.emit(format!("{len} = load i64, ptr {lp}"));
                let len32 = self.fresh_tmp();
                self.emit(format!("{len32} = trunc i64 {len} to i32"));
                let fmt = self.intern_string("%.*s");
                self.emit(format!("call i32 (ptr, ...) @printf(ptr {fmt}, i32 {len32}, ptr {data})"));
            } else if ty.is_int() {
                if ty.is_signed() {
                    let v64 = if ty == Ty::I64 {
                        v
                    } else {
                        let t = self.fresh_tmp();
                        self.emit(format!("{t} = sext {} {v} to i64", ty.llvm()));
                        t
                    };
                    let fmt = self.intern_string("%lld");
                    self.emit(format!("call i32 (ptr, ...) @printf(ptr {fmt}, i64 {v64})"));
                } else {
                    let v64 = if ty == Ty::U64 {
                        v
                    } else {
                        let t = self.fresh_tmp();
                        self.emit(format!("{t} = zext {} {v} to i64", ty.llvm()));
                        t
                    };
                    let fmt = self.intern_string("%llu");
                    self.emit(format!("call i32 (ptr, ...) @printf(ptr {fmt}, i64 {v64})"));
                }
            } else if ty.is_float() {
                let v64 = if ty == Ty::F64 {
                    v
                } else {
                    let t = self.fresh_tmp();
                    self.emit(format!("{t} = fpext float {v} to double"));
                    t
                };
                let fmt = self.intern_string("%g");
                self.emit(format!("call i32 (ptr, ...) @printf(ptr {fmt}, double {v64})"));
            } else if ty == Ty::Bool {
                let t_str = self.intern_string("true");
                let f_str = self.intern_string("false");
                let b_str = self.fresh_tmp();
                self.emit(format!("{b_str} = select i1 {v}, ptr {t_str}, ptr {f_str}"));
                let fmt = self.intern_string("%s");
                self.emit(format!("call i32 (ptr, ...) @printf(ptr {fmt}, ptr {b_str})"));
            } else if let Ty::Ptr(inner, _) = &ty {
                if **inner == Ty::U8 {
                    let fmt = self.intern_string("%s");
                    self.emit(format!("call i32 (ptr, ...) @printf(ptr {fmt}, ptr {v})"));
                } else {
                    let fmt = self.intern_string("%p");
                    self.emit(format!("call i32 (ptr, ...) @printf(ptr {fmt}, ptr {v})"));
                }
            }
        }
        if newline {
            let nl = self.intern_string("\n");
            self.emit(format!("call i32 (ptr, ...) @printf(ptr {nl})"));
        }
        ("".into(), Ty::Void)
    }

    /// Побитовое представление float как целого (bitcast, не преобразование
    /// значения): `f32_bits(x) -> u32`, `f64_bits(x) -> u64`. Нужно для
    /// fixed32/fixed64/float/double protobuf.
    fn bi_bits(&mut self, name: &str, args: &[Expr], span: Span) -> (String, Ty) {
        if args.len() != 1 {
            self.err("E0094", span, format!("`{name}` ждёт 1 аргумент, передано {}", args.len()), None);
            return ("0".into(), Ty::Err);
        }
        let (src_ty, dst_ty, ll_src) = if name == "f32_bits" {
            (Ty::F32, Ty::U32, "float")
        } else {
            (Ty::F64, Ty::U64, "double")
        };
        let (v, vty) = self.gen_expr(&args[0], Some(&src_ty));
        if vty != src_ty && vty != Ty::Err {
            self.err("E0094", args[0].span(), format!("`{name}` ждёт `{}`, а тут `{}`", src_ty.name(), vty.name()), None);
        }
        let t = self.fresh_tmp();
        self.emit(format!("{t} = bitcast {ll_src} {v} to {}", dst_ty.llvm()));
        (t, dst_ty)
    }

    /// Обратный bitcast: `f32_from_bits(u32) -> f32`, `f64_from_bits(u64) -> f64`.
    fn bi_from_bits(&mut self, name: &str, args: &[Expr], span: Span) -> (String, Ty) {
        if args.len() != 1 {
            self.err("E0094", span, format!("`{name}` ждёт 1 аргумент, передано {}", args.len()), None);
            return ("0".into(), Ty::Err);
        }
        let (src_ty, dst_ty, ll_dst) = if name == "f32_from_bits" {
            (Ty::U32, Ty::F32, "float")
        } else {
            (Ty::U64, Ty::F64, "double")
        };
        let (v, _) = self.gen_expr(&args[0], Some(&src_ty));
        let t = self.fresh_tmp();
        self.emit(format!("{t} = bitcast {} {v} to {ll_dst}", src_ty.llvm()));
        (t, dst_ty)
    }

    /// `sizeof(T)` — размер типа в байтах (с учётом выравнивания), считается
    /// LLVM через идиому getelementptr null. Аргумент — имя типа.
    fn bi_sizeof(&mut self, args: &[Expr], span: Span) -> (String, Ty) {
        if args.len() != 1 {
            self.err("E0094", span, "`sizeof` ждёт 1 аргумент — имя типа".into(), None);
            return ("0".into(), Ty::I64);
        }
        let ty = match &args[0] {
            Expr::Ident(name, sp) => self.resolve(&crate::ast::TypeExpr::Named(name.clone(), *sp)),
            other => {
                self.err("E0094", other.span(), "`sizeof` ждёт имя типа, напр. `sizeof(Point)`".into(), None);
                return ("0".into(), Ty::I64);
            }
        };
        if ty == Ty::Err {
            return ("0".into(), Ty::I64);
        }
        let g = self.fresh_tmp();
        self.emit(format!("{g} = getelementptr {ll}, ptr null, i64 1", ll = ty.llvm()));
        let s = self.fresh_tmp();
        self.emit(format!("{s} = ptrtoint ptr {g} to i64"));
        (s, Ty::I64)
    }

    /// `zeroed()` — нулевое значение ожидаемого типа (для инициализации).
    fn bi_zeroed(&mut self, expected: Option<&Ty>, span: Span) -> (String, Ty) {
        let ty = match expected {
            Some(t) if *t != Ty::Void => t.clone(),
            _ => {
                self.err("E0094", span, "`zeroed()` требует известный тип — укажите его в аннотации".into(), None);
                return ("0".into(), Ty::Err);
            }
        };
        if is_aggregate(&ty) {
            let slot = self.fresh_slot("zero");
            self.alloca(&slot, &ty);
            self.emit(format!("store {t} zeroinitializer, ptr {slot}", t = ty.llvm()));
            (slot, ty)
        } else {
            let z = self.zero_of(&ty);
            (z, ty)
        }
    }

    /// `panic(msg: *u8)` — печатает сообщение и аварийно завершает процесс.
    fn bi_panic(&mut self, args: &[Expr], span: Span) -> (String, Ty) {
        if args.len() != 1 {
            self.err("E0094", span, format!("`panic` ждёт 1 аргумент (сообщение *u8), передано {}", args.len()), None);
            return ("".into(), Ty::Void);
        }
        let (msg, _) = self.gen_expr(&args[0], Some(&Ty::Ptr(Box::new(Ty::U8), false)));
        let fmt = self.intern_string("panic: %s\n");
        if !self.ctx.fns.contains_key("printf") {
            self.use_intrinsic("declare i32 @printf(ptr, ...)".into());
        }
        if !self.ctx.fns.contains_key("abort") {
            self.use_intrinsic("declare void @abort()".into());
        }
        let t = self.fresh_tmp();
        self.emit(format!("{t} = call i32 (ptr, ...) @printf(ptr {fmt}, ptr {msg})"));
        self.emit("call void @abort()".into());
        self.emit("unreachable".into());
        self.terminated = true;
        ("".into(), Ty::Void)
    }

    fn expect_ptr(&mut self, ty: &Ty, span: Span, what: &str) {
        if !ty.is_ptr() && *ty != Ty::Err {
            self.err("E0093", span, format!("`{what}` ожидает указатель, а тут `{}`", ty.name()), None);
        }
    }

    fn type_suffix(&self, ty: &Ty) -> String {
        match ty {
            Ty::F32 => "f32".into(),
            Ty::F64 => "f64".into(),
            _ => format!("i{}", ty.int_bits()),
        }
    }

    fn use_intrinsic(&mut self, decl: String) {
        self.intrinsics.insert(decl);
    }

    /// Вызов метода `base.m(args)` -> `Type__m(self, args)`. `self` — адрес
    /// приёмника (для значения-структуры) или сам указатель (для `*T`).
    fn gen_method_call(&mut self, mangled: &str, base: &Expr, bt: &Ty, args: &[Expr], span: Span) -> (String, Ty) {
        let sig = self.ctx.fns[mangled].clone();
        // self: для Struct — адрес хранилища, для Ptr(Struct) — сам указатель.
        let (self_val, _) = self.gen_expr(base, None);
        let _ = bt;

        let want = sig.params.len().saturating_sub(1); // без self
        if args.len() != want {
            self.err("E0072", span, format!("метод `{mangled}` ждёт {want} аргумент(ов), передано {}", args.len()), None);
        }
        let mut argvals: Vec<String> = vec![format!("ptr {self_val}")];
        for (i, a) in args.iter().enumerate() {
            let expected = sig.params.get(i + 1).cloned();
            let (v, vty) = self.gen_expr(a, expected.as_ref());
            match &expected {
                Some(pt) => {
                    if !compat(pt, &vty) && vty != Ty::Err && *pt != Ty::Err {
                        self.err("E0073", a.span(), format!("аргумент {}: ожидался `{}`, передан `{}`", i + 1, pt.name(), vty.name()), None);
                    }
                    if is_aggregate(pt) {
                        let t = self.fresh_tmp();
                        self.emit(format!("{t} = load {ty}, ptr {v}", ty = pt.llvm()));
                        argvals.push(format!("{} {}", pt.llvm(), t));
                    } else {
                        argvals.push(format!("{} {}", pt.llvm(), v));
                    }
                }
                None => argvals.push(format!("{} {}", vty.llvm(), v)),
            }
        }
        let argstr = argvals.join(", ");
        if sig.ret == Ty::Void {
            self.emit(format!("call void @{mangled}({argstr})"));
            ("".into(), Ty::Void)
        } else {
            let t = self.fresh_tmp();
            self.emit(format!("{t} = call {rty} @{mangled}({argstr})", rty = sig.ret.llvm()));
            self.spill_if_aggregate(t, sig.ret)
        }
    }

    /// Непрямой вызов через функцию-указатель (например, хендл jit-блока).
    fn gen_indirect_call(&mut self, name: &str, slot: &str, params: &[Ty], ret: &Ty, args: &[Expr], span: Span) -> (String, Ty) {
        let fp = self.fresh_tmp();
        self.emit(format!("{fp} = load ptr, ptr {slot}"));
        self.gen_indirect_call_ptr(&fp, name, params, ret, args, span)
    }

    /// Непрямой вызов, где `fp` — уже готовый ptr-значение функции.
    fn gen_indirect_call_ptr(&mut self, fp: &str, name: &str, params: &[Ty], ret: &Ty, args: &[Expr], span: Span) -> (String, Ty) {
        if args.len() != params.len() {
            self.err("E0072", span, format!("`{name}` ждёт {} аргумент(ов), передано {}", params.len(), args.len()), None);
        }
        let mut argvals: Vec<String> = Vec::new();
        for (i, a) in args.iter().enumerate() {
            let expected = params.get(i).cloned();
            let (v, vty) = self.gen_expr(a, expected.as_ref());
            if let Some(pt) = &expected {
                if !compat(pt, &vty) && vty != Ty::Err && *pt != Ty::Err {
                    self.err("E0073", a.span(), format!("аргумент {}: ожидался `{}`, передан `{}`", i + 1, pt.name(), vty.name()), None);
                }
                argvals.push(format!("{} {}", pt.llvm(), v));
            } else {
                argvals.push(format!("{} {}", vty.llvm(), v));
            }
        }
        let argstr = argvals.join(", ");
        if *ret == Ty::Void {
            self.emit(format!("call void {fp}({argstr})"));
            ("".into(), Ty::Void)
        } else {
            let t = self.fresh_tmp();
            self.emit(format!("{t} = call {rty} {fp}({argstr})", rty = ret.llvm()));
            self.spill_if_aggregate(t, ret.clone())
        }
    }

    /// Приведение вариадических аргументов по правилам C (default argument promotions).
    fn promote_variadic(&mut self, v: String, ty: Ty) -> (String, Ty) {
        match ty {
            Ty::F32 => {
                let t = self.fresh_tmp();
                self.emit(format!("{t} = fpext float {v} to double"));
                (t, Ty::F64)
            }
            Ty::Bool | Ty::I8 | Ty::I16 => {
                let t = self.fresh_tmp();
                self.emit(format!("{t} = sext {s} {v} to i32", s = ty.llvm()));
                (t, Ty::I32)
            }
            Ty::U8 | Ty::U16 => {
                let t = self.fresh_tmp();
                self.emit(format!("{t} = zext {s} {v} to i32", s = ty.llvm()));
                (t, Ty::I32)
            }
            Ty::Slice(ref elem) if **elem == Ty::U8 => {
                let pf = self.fresh_tmp();
                self.emit(format!("{pf} = getelementptr %slice, ptr {v}, i32 0, i32 0"));
                let p = self.fresh_tmp();
                self.emit(format!("{p} = load ptr, ptr {pf}"));
                (p, Ty::Ptr(elem.clone(), false))
            }
            other => (v, other),
        }
    }

    fn gen_if_expr(&mut self, cond: &Expr, then: &Expr, els: &Expr, expected: Option<&Ty>, _span: Span) -> (String, Ty) {
        let (c, cty) = self.gen_expr(cond, Some(&Ty::Bool));
        self.expect_bool(&cty, cond.span());
        // Тип результата — из then-ветви (else обязана совпасть).
        let then_l = self.fresh_label("ifethen");
        let else_l = self.fresh_label("ifeelse");
        let end_l = self.fresh_label("ifeend");
        self.emit(format!("br i1 {c}, label %{then_l}, label %{else_l}"));

        self.emit_label(&then_l);
        let (tv, tty) = self.gen_expr(then, expected);
        if tty == Ty::Err {
            return ("0".into(), Ty::Err);
        }
        let slot = self.fresh_slot("ifeval");
        self.alloca(&slot, &tty);
        self.store_value(&tty, &tv, &tty, &slot);
        self.emit(format!("br label %{end_l}"));

        self.emit_label(&else_l);
        let (ev, ety) = self.gen_expr(els, Some(&tty));
        if !compat(&tty, &ety) && ety != Ty::Err {
            self.err("E0087", els.span(), format!("ветви if-выражения разных типов: `{}` и `{}`", tty.name(), ety.name()), None);
        }
        self.store_value(&tty, &ev, &ety, &slot);
        self.emit(format!("br label %{end_l}"));

        self.emit_label(&end_l);
        if is_aggregate(&tty) {
            (slot, tty)
        } else {
            let r = self.fresh_tmp();
            self.emit(format!("{r} = load {ty}, ptr {slot}", ty = tty.llvm()));
            (r, tty)
        }
    }

    fn gen_array_lit(&mut self, elems: &[Expr], expected: Option<&Ty>, span: Span) -> (String, Ty) {
        // Тип элемента: из ожидания [N]T либо из первого элемента.
        let elem_ty = match expected {
            Some(Ty::Array(e, _)) => Some((**e).clone()),
            _ => None,
        };
        let elem_ty = match elem_ty {
            Some(t) => t,
            None => {
                if elems.is_empty() {
                    self.err("E0085", span, "нельзя вывести тип пустого массива — укажите аннотацию".into(), None);
                    return ("0".into(), Ty::Err);
                }
                self.type_of(&elems[0])
            }
        };
        let n = elems.len() as u64;
        let arr_ty = Ty::Array(Box::new(elem_ty.clone()), n);
        let slot = self.fresh_slot("arr");
        self.alloca(&slot, &arr_ty);
        for (i, e) in elems.iter().enumerate() {
            let (v, vty) = self.gen_expr(e, Some(&elem_ty));
            if !compat(&elem_ty, &vty) && vty != Ty::Err && elem_ty != Ty::Err {
                self.err("E0086", e.span(), format!("элемент массива типа `{}`, а массив из `{}`", vty.name(), elem_ty.name()), None);
            }
            let ep = self.fresh_tmp();
            self.emit(format!("{ep} = getelementptr {aty}, ptr {slot}, i64 0, i64 {i}", aty = arr_ty.llvm()));
            self.store_value(&elem_ty, &v, &vty, &ep);
        }
        (slot, arr_ty)
    }

    fn gen_struct_lit(&mut self, name: &str, fields: &[(String, Expr, Span)], span: Span) -> (String, Ty) {
        let info = match self.ctx.structs.get(name) {
            Some(i) => i.clone(),
            None => {
                self.err("E0074", span, format!("неизвестная структура `{name}`"), None);
                return ("0".into(), Ty::Err);
            }
        };
        let slot = self.fresh_slot(&format!("lit_{name}"));
        self.alloca(&slot, &Ty::Struct(name.to_string()));

        // Заполняем поля; проверяем, что все поля заданы ровно один раз.
        let mut seen = vec![false; info.fields.len()];
        for (fname, fexpr, fsp) in fields {
            match info.field_index(fname) {
                Some(idx) => {
                    if seen[idx] {
                        self.err("E0075", *fsp, format!("поле `{fname}` задано дважды"), None);
                    }
                    seen[idx] = true;
                    let fty = info.fields[idx].1.clone();
                    let (v, vty) = self.gen_expr(fexpr, Some(&fty));
                    if !compat(&fty, &vty) && vty != Ty::Err && fty != Ty::Err {
                        self.err("E0076", fexpr.span(), format!("поле `{fname}`: ожидался `{}`, передан `{}`", fty.name(), vty.name()), None);
                    }
                    let fptr = self.fresh_tmp();
                    self.emit(format!("{fptr} = getelementptr %struct.{name}, ptr {slot}, i32 0, i32 {idx}"));
                    self.store_value(&fty, &v, &vty, &fptr);
                }
                None => self.err("E0077", *fsp, format!("у структуры `{name}` нет поля `{fname}`"), None),
            }
        }
        for (i, ok) in seen.iter().enumerate() {
            if !ok {
                self.err("E0078", span, format!("не задано поле `{}` структуры `{name}`", info.fields[i].0), None);
            }
        }
        (slot, Ty::Struct(name.to_string()))
    }

    // ---------- вспомогательное ----------

    /// Лёгкий вывод типа выражения без генерации кода (для решения,
    /// как трактовать доступ к полю). Ошибок не порождает.
    fn type_of(&self, e: &Expr) -> Ty {
        match e {
            Expr::Int(..) => Ty::I64,
            Expr::Float(..) => Ty::F64,
            Expr::Bool(..) => Ty::Bool,
            Expr::Null(..) => Ty::Ptr(Box::new(Ty::U8), true),
            Expr::Path(..) => Ty::I32,
            Expr::Str(..) => Ty::Slice(Box::new(Ty::U8)),
            Expr::Ident(n, _) => self
                .lookup(n)
                .map(|l| l.ty.clone())
                .or_else(|| self.statics.get(n).map(|(_, t)| t.clone()))
                .or_else(|| self.consts.get(n).map(|cv| cv.ty()))
                .or_else(|| {
                    self.ctx
                        .fns
                        .get(n)
                        .filter(|s| !s.variadic)
                        .map(|s| Ty::FnPtr(s.params.clone(), Box::new(s.ret.clone())))
                })
                .unwrap_or(Ty::Err),
            Expr::Unary { op: UnOp::Deref, expr, .. } => match self.type_of(expr) {
                Ty::Ptr(inner, _) => *inner,
                _ => Ty::Err,
            },
            Expr::Unary { op: UnOp::Ref, expr, .. } => Ty::Ptr(Box::new(self.type_of(expr)), false),
            Expr::Unary { op: UnOp::RefMut, expr, .. } => Ty::Ptr(Box::new(self.type_of(expr)), true),
            Expr::Unary { expr, .. } => self.type_of(expr),
            Expr::Binary { op, lhs, .. } => {
                if matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::And | BinOp::Or) {
                    Ty::Bool
                } else {
                    self.type_of(lhs)
                }
            }
            Expr::Cast { ty, .. } => {
                let mut junk = Vec::new();
                self.ctx.resolve(ty, &mut junk)
            }
            Expr::Call { callee, .. } => match &**callee {
                Expr::Ident(n, _) => {
                    match n.as_str() {
                        "print" | "println" => return Ty::Void,
                        "str_from_cstr" | "obf" | "obf_str" => return Ty::Slice(Box::new(Ty::U8)),
                        _ => {}
                    }
                    if let Some(l) = self.lookup(n) {
                        if let Ty::FnPtr(_, ret) = &l.ty {
                            return (**ret).clone();
                        }
                    }
                    self.ctx.fns.get(n).map(|s| s.ret.clone()).unwrap_or(Ty::Err)
                }
                // метод base.m(...)
                Expr::Field { base, field, .. } => {
                    let bt = self.type_of(base);
                    if is_str(&bt) {
                        return match field.as_str() {
                            "starts_with" | "ends_with" | "is_empty" => Ty::Bool,
                            "clone" => Ty::Slice(Box::new(Ty::U8)),
                            _ => Ty::Err,
                        };
                    }
                    let tn = match bt {
                        Ty::Struct(n) => Some(n),
                        Ty::Ptr(inner, _) => match *inner {
                            Ty::Struct(n) => Some(n),
                            _ => None,
                        },
                        _ => None,
                    };
                    tn.and_then(|t| self.ctx.fns.get(&format!("{t}__{field}")).map(|s| s.ret.clone()))
                        .unwrap_or(Ty::Err)
                }
                _ => Ty::Err,
            },
            Expr::Jit { inner, .. } => {
                let mut junk = Vec::new();
                let ps = inner.params.iter().map(|p| self.ctx.resolve(&p.ty, &mut junk)).collect();
                let r = match &inner.ret {
                    Some(t) => self.ctx.resolve(t, &mut junk),
                    None => Ty::Void,
                };
                Ty::FnPtr(ps, Box::new(r))
            }
            Expr::ArrayLit(elems, _) => {
                if elems.is_empty() {
                    Ty::Err
                } else {
                    Ty::Array(Box::new(self.type_of(&elems[0])), elems.len() as u64)
                }
            }
            Expr::IfExpr { then, .. } => self.type_of(then),
            Expr::Field { base, field, .. } => {
                let bt = self.type_of(base);
                if let Ty::Array(_, _) = bt {
                    return if field == "len" { Ty::I64 } else { Ty::Err };
                }
                if let Ty::Slice(elem) = bt {
                    return match field.as_str() {
                        "ptr" => Ty::Ptr(elem, true),
                        "len" => Ty::I64,
                        _ => Ty::Err,
                    };
                }
                let sname = match bt {
                    Ty::Struct(n) => n,
                    Ty::Ptr(inner, _) => match *inner {
                        Ty::Struct(n) => n,
                        _ => return Ty::Err,
                    },
                    _ => return Ty::Err,
                };
                self.ctx
                    .structs
                    .get(&sname)
                    .and_then(|i| i.field_index(field).map(|idx| i.fields[idx].1.clone()))
                    .unwrap_or(Ty::Err)
            }
            Expr::Index { base, .. } => match self.type_of(base) {
                Ty::Ptr(inner, _) => *inner,
                Ty::Slice(elem) => *elem,
                Ty::Array(elem, _) => *elem,
                _ => Ty::Err,
            },
            Expr::Slice { base, .. } => match self.type_of(base) {
                Ty::Slice(elem) => Ty::Slice(elem),
                Ty::Array(elem, _) => Ty::Slice(elem),
                Ty::Ptr(elem, _) => Ty::Slice(elem),
                _ => Ty::Err,
            },
            Expr::StructLit { name, .. } => Ty::Struct(name.clone()),
            Expr::Try(inner, _) => {
                let ty = self.type_of(inner);
                match ty {
                    Ty::Struct(name) => {
                        if let Some(i) = self.ctx.structs.get(&name) {
                            if let Some(idx) = i.field_index("value") {
                                i.fields[idx].1.clone()
                            } else {
                                Ty::Err
                            }
                        } else {
                            Ty::Err
                        }
                    }
                    Ty::Ptr(..) => ty,
                    other if other.is_int() => other,
                    _ => Ty::Err,
                }
            }
        }
    }

    fn resolve(&mut self, te: &TypeExpr) -> Ty {
        let mut out = Vec::new();
        let ty = self.ctx.resolve(te, &mut out);
        for d in out {
            self.diags.push(d);
        }
        ty
    }

    /// Свёртка константного выражения. None — не константа.
    fn eval_const(&self, e: &Expr, expected: Option<&Ty>) -> Option<CVal> {
        match e {
            Expr::Int(n, _) => Some(match expected {
                Some(Ty::F32) => CVal::Float(*n as f64, Ty::F32),
                Some(Ty::F64) => CVal::Float(*n as f64, Ty::F64),
                Some(t) if t.is_int() => CVal::Int(*n, t.clone()),
                _ => CVal::Int(*n, Ty::I64),
            }),
            Expr::Float(f, _) => Some(match expected {
                Some(Ty::F32) => CVal::Float(*f, Ty::F32),
                _ => CVal::Float(*f, Ty::F64),
            }),
            Expr::Bool(b, _) => Some(CVal::Bool(*b)),
            Expr::Path(en, var, _) => {
                self.ctx.enums.get(en).and_then(|m| m.get(var)).map(|v| CVal::Int(*v, Ty::I32))
            }
            Expr::Ident(name, _) => self.consts.get(name).cloned(),
            Expr::Unary { op, expr, .. } => {
                let v = self.eval_const(expr, expected)?;
                match op {
                    UnOp::Neg => match v {
                        CVal::Int(n, t) => Some(CVal::Int(n.wrapping_neg(), t)),
                        CVal::Float(f, t) => Some(CVal::Float(-f, t)),
                        _ => None,
                    },
                    UnOp::Not => match v {
                        CVal::Bool(b) => Some(CVal::Bool(!b)),
                        _ => None,
                    },
                    UnOp::BitNot => match v {
                        CVal::Int(n, t) => Some(CVal::Int(!n, t)),
                        _ => None,
                    },
                    _ => None,
                }
            }
            Expr::Cast { expr, ty, .. } => {
                let mut junk = Vec::new();
                let dst = self.ctx.resolve(ty, &mut junk);
                let v = self.eval_const(expr, None)?;
                match v {
                    CVal::Int(n, _) => {
                        if dst.is_float() {
                            Some(CVal::Float(n as f64, dst))
                        } else if dst.is_int() {
                            Some(CVal::Int(n, dst))
                        } else {
                            None
                        }
                    }
                    CVal::Float(f, _) => {
                        if dst.is_float() {
                            Some(CVal::Float(f, dst))
                        } else if dst.is_int() {
                            Some(CVal::Int(f as i64, dst))
                        } else {
                            None
                        }
                    }
                    CVal::Bool(b) => {
                        if dst.is_int() {
                            Some(CVal::Int(b as i64, dst))
                        } else {
                            None
                        }
                    }
                }
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                let l = self.eval_const(lhs, expected)?;
                let lty = l.ty();
                let r = self.eval_const(rhs, if lty.is_numeric() { Some(&lty) } else { None })?;
                eval_bin(*op, l, r)
            }
            _ => None,
        }
    }

    fn lookup(&self, name: &str) -> Option<&Local> {
        for scope in self.scopes.iter().rev() {
            if let Some(l) = scope.get(name) {
                return Some(l);
            }
        }
        None
    }

    /// Копирует значение в слот: скаляр — store; агрегат — load+store агрегата.
    fn store_value(&mut self, dst_ty: &Ty, val: &str, val_ty: &Ty, slot: &str) {
        if is_aggregate(dst_ty) {
            if *val_ty == Ty::Err {
                return;
            }
            let t = self.fresh_tmp();
            self.emit(format!("{t} = load {ty}, ptr {val}", ty = dst_ty.llvm()));
            self.emit(format!("store {ty} {t}, ptr {slot}", ty = dst_ty.llvm()));
        } else if *dst_ty != Ty::Err {
            self.emit(format!("store {ty} {val}, ptr {slot}", ty = dst_ty.llvm()));
        }
    }

    fn intern_string(&mut self, s: &str) -> String {
        let label = format!("@.str.{}", self.strcount);
        self.strcount += 1;
        let bytes = s.as_bytes();
        let mut enc = String::new();
        for &b in bytes {
            if b == b'"' || b == b'\\' || b < 0x20 || b >= 0x7f {
                enc.push_str(&format!("\\{:02X}", b));
            } else {
                enc.push(b as char);
            }
        }
        enc.push_str("\\00");
        let len = bytes.len() + 1;
        self.strings.push_str(&format!(
            "{label} = private unnamed_addr constant [{len} x i8] c\"{enc}\"\n"
        ));
        label
    }

    /// Является ли выражение безопасной ссылкой (приёмником `self`)?
    fn is_safe_ref(&self, e: &Expr) -> bool {
        matches!(e, Expr::Ident(n, _) if self.lookup(n).map(|l| l.safe).unwrap_or(false))
    }

    fn require_unsafe(&mut self, span: Span, what: impl Into<String>) {
        if self.unsafe_depth == 0 && !self.cur_unsafe {
            self.diags.push(
                Diagnostic::error(
                    "E0030",
                    span,
                    format!("{} требует `unsafe`", what.into()),
                )
                .with_hint("оберните код в `unsafe { ... }` или пометьте функцию `unsafe fn`"),
            );
        }
    }

    fn expect_bool(&mut self, ty: &Ty, span: Span) {
        if *ty != Ty::Bool && *ty != Ty::Err {
            self.err("E0080", span, format!("ожидалось условие типа bool, а тут `{}`", ty.name()), None);
        }
    }

    fn err(&mut self, code: &'static str, span: Span, msg: String, hint: Option<&str>) {
        let mut d = Diagnostic::error(code, span, msg);
        if let Some(h) = hint {
            d = d.with_hint(h);
        }
        self.diags.push(d);
    }

    fn zero_of(&self, ty: &Ty) -> String {
        if ty.is_float() {
            "0.0".into()
        } else if is_aggregate(ty) {
            "zeroinitializer".into()
        } else if ty.is_ptr() || matches!(ty, Ty::FnPtr(..)) {
            "null".into()
        } else {
            "0".into()
        }
    }

    fn fresh_tmp(&mut self) -> String {
        self.tmp += 1;
        format!("%t{}", self.tmp)
    }
    fn fresh_label(&mut self, prefix: &str) -> String {
        self.label += 1;
        format!("{prefix}{}", self.label)
    }
    fn fresh_slot(&mut self, base: &str) -> String {
        self.slotcount += 1;
        llvm_local(&format!("{base}.{}", self.slotcount))
    }

    fn alloca(&mut self, slot: &str, ty: &Ty) {
        self.allocas.push_str(&format!("  {slot} = alloca {}\n", ty.llvm()));
    }
    fn emit(&mut self, line: String) {
        self.code.push_str("  ");
        self.code.push_str(&line);
        self.code.push('\n');
    }
    fn emit_label(&mut self, label: &str) {
        self.code.push_str(label);
        self.code.push_str(":\n");
        self.terminated = false;
    }
}

/// Является ли идентификатор именем регистра x86-64 (для авто-клобберов).
fn is_x86_reg(s: &str) -> bool {
    const REGS: &[&str] = &[
        "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "rsp",
        "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15",
        "eax", "ebx", "ecx", "edx", "esi", "edi", "ebp", "esp",
        "r8d", "r9d", "r10d", "r11d", "r12d", "r13d", "r14d", "r15d",
        "ax", "bx", "cx", "dx", "al", "bl", "cl", "dl", "ah", "bh", "ch", "dh",
    ];
    REGS.contains(&s)
}

/// Агрегатные типы (структуры/срезы) представляются в кодогене АДРЕСОМ, а не
/// скалярным значением: их читают/пишут через load/store агрегата.
fn is_aggregate(ty: &Ty) -> bool {
    matches!(ty, Ty::Struct(_) | Ty::Slice(_) | Ty::Array(..))
}

/// Является ли тип первоклассной строкой Goraw (`str` == `[]u8`).
fn is_str(ty: &Ty) -> bool {
    matches!(ty, Ty::Slice(elem) if **elem == Ty::U8)
}

/// Совместимы ли типы при передаче/присваивании. Точное равенство, плюс
/// коэрция указателей `*mut T` -> `*T` (потеря права записи — безопасна;
/// обратно — нет). На уровне LLVM указатели непрозрачны, конверсия не нужна.
fn compat(to: &Ty, from: &Ty) -> bool {
    if to == from {
        return true;
    }
    matches!((to, from), (Ty::Ptr(a, false), Ty::Ptr(b, _)) if a == b)
}

/// Свёртка бинарной операции над константами.
fn eval_bin(op: BinOp, l: CVal, r: CVal) -> Option<CVal> {
    use BinOp::*;
    match (l, r) {
        (CVal::Int(a, t), CVal::Int(b, _)) => {
            let iarith = |v: i64| Some(CVal::Int(v, t.clone()));
            match op {
                Add => iarith(a.wrapping_add(b)),
                Sub => iarith(a.wrapping_sub(b)),
                Mul => iarith(a.wrapping_mul(b)),
                Div => if b == 0 { None } else { iarith(a.wrapping_div(b)) },
                Rem => if b == 0 { None } else { iarith(a.wrapping_rem(b)) },
                BitAnd => iarith(a & b),
                BitOr => iarith(a | b),
                BitXor => iarith(a ^ b),
                Shl => iarith(a.wrapping_shl(b as u32)),
                Shr => iarith(a.wrapping_shr(b as u32)),
                Eq => Some(CVal::Bool(a == b)),
                Ne => Some(CVal::Bool(a != b)),
                Lt => Some(CVal::Bool(a < b)),
                Le => Some(CVal::Bool(a <= b)),
                Gt => Some(CVal::Bool(a > b)),
                Ge => Some(CVal::Bool(a >= b)),
                _ => None,
            }
        }
        (CVal::Float(a, t), CVal::Float(b, _)) => match op {
            Add => Some(CVal::Float(a + b, t)),
            Sub => Some(CVal::Float(a - b, t)),
            Mul => Some(CVal::Float(a * b, t)),
            Div => Some(CVal::Float(a / b, t)),
            Rem => Some(CVal::Float(a % b, t)),
            Eq => Some(CVal::Bool(a == b)),
            Ne => Some(CVal::Bool(a != b)),
            Lt => Some(CVal::Bool(a < b)),
            Le => Some(CVal::Bool(a <= b)),
            Gt => Some(CVal::Bool(a > b)),
            Ge => Some(CVal::Bool(a >= b)),
            _ => None,
        },
        (CVal::Bool(a), CVal::Bool(b)) => match op {
            And => Some(CVal::Bool(a && b)),
            Or => Some(CVal::Bool(a || b)),
            Eq => Some(CVal::Bool(a == b)),
            Ne => Some(CVal::Bool(a != b)),
            _ => None,
        },
        _ => None,
    }
}

/// Форматирует float как точную hex-константу LLVM (без потери точности).
fn fmt_float(v: f64, ty: &Ty) -> String {
    let bits = if *ty == Ty::F32 {
        (v as f32 as f64).to_bits()
    } else {
        v.to_bits()
    };
    format!("0x{:016X}", bits)
}

pub fn llvm_local(name: &str) -> String {
    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '$') {
        format!("%{name}")
    } else {
        format!("%\"{name}\"")
    }
}

pub fn llvm_global(name: &str) -> String {
    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '$') {
        format!("@{name}")
    } else {
        format!("@\"{name}\"")
    }
}

const OBF_DECRYPT_IR: &str = r#"define internal ptr @__goraw_decrypt_str(ptr %enc, ptr %buf, i64 %len, i32 %key, ptr %init_ptr) {
entry:
  %is_init = load i1, ptr %init_ptr
  br i1 %is_init, label %exit, label %do_init

do_init:
  store i1 true, ptr %init_ptr
  %is_zero = icmp eq i64 %len, 0
  br i1 %is_zero, label %exit, label %loop

loop:
  %idx = phi i64 [ 0, %do_init ], [ %next_idx, %loop ]
  %enc_gep = getelementptr inbounds i8, ptr %enc, i64 %idx
  %b_enc = load i8, ptr %enc_gep
  %i32 = trunc i64 %idx to i32
  %shift = and i32 %i32, 31
  %rot = call i32 @llvm.fshl.i32(i32 %key, i32 %key, i32 %shift)
  %mask_i32 = add i32 %rot, %i32
  %mask = trunc i32 %mask_i32 to i8
  %b_dec = xor i8 %b_enc, %mask
  %buf_gep = getelementptr inbounds i8, ptr %buf, i64 %idx
  store i8 %b_dec, ptr %buf_gep
  %next_idx = add i64 %idx, 1
  %done = icmp eq i64 %next_idx, %len
  br i1 %done, label %exit, label %loop

exit:
  ret ptr %buf
}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_obf_string_encryption_roundtrip() {
        let original = "Hello, string obfuscation in Goraw!";
        let key = 0xdeadbeef;
        let enc = Codegen::obf_encrypt_str(original, key);
        assert_eq!(enc.len(), original.len() + 1);

        // Decrypt using the same formula as the runtime LLVM IR
        let mut dec = Vec::with_capacity(enc.len());
        for i in 0..enc.len() {
            let shift = (i % 32) as u32;
            let rot = key.rotate_left(shift);
            let mask = rot.wrapping_add(i as u32) as u8;
            dec.push(enc[i] ^ mask);
        }

        assert_eq!(&dec[..original.len()], original.as_bytes());
        assert_eq!(dec[original.len()], 0); // null terminator
    }
}

