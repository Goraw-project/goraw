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
    intrinsics: HashSet<String>,  // declare-строки использованных LLVM-интринзиков
    // Пока генерируется IR-ШАБЛОН jit-блока: захваченные имена -> (индекс, тип).
    // Их чтение выдаёт плейсхолдер `$CAPi$`, который рантайм заменит на константу.
    captures: Option<HashMap<String, (usize, Ty)>>,
    in_jit_template: bool,
    jittmpl_count: u32,
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
            intrinsics: HashSet::new(),
            captures: None,
            in_jit_template: false,
            jittmpl_count: 0,
        }
    }

    // ---------- сборка модуля ----------

    pub fn emit_module(mut self, prog: &Program) -> String {
        // Сначала генерируем тела функций — попутно собираются использованные
        // строковые константы и LLVM-интринзики, нужные для заголовка.
        for f in &prog.fns {
            if !f.is_extern {
                self.gen_fn(f);
            }
        }

        let mut header = String::new();
        header.push_str("; Goraw -> LLVM IR\n");
        header.push_str("target triple = \"x86_64-w64-windows-gnu\"\n\n");

        // Тип среза (fat-pointer): указатель на элементы + длина.
        header.push_str("%slice = type { ptr, i64 }\n\n");

        // Определения структур в порядке объявления.
        for name in &self.ctx.struct_order {
            let info = &self.ctx.structs[name];
            let fields: Vec<String> = info.fields.iter().map(|(_, t)| t.llvm()).collect();
            header.push_str(&format!("%struct.{name} = type {{ {} }}\n", fields.join(", ")));
        }
        if !self.ctx.struct_order.is_empty() {
            header.push('\n');
        }

        // extern-объявления.
        for f in &prog.fns {
            if f.is_extern {
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
                header.push_str(&format!("declare {} @{}({})\n", sig.ret.llvm(), f.name, plist));
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
        out.push_str(&self.strings);
        if !self.strings.is_empty() {
            out.push('\n');
        }
        out.push_str(&self.body);
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
        self.unsafe_depth = 0;
        self.scopes.clear();
        self.loops.clear();
        self.terminated = false;
        self.scopes.push(HashMap::new());

        // Сигнатура.
        let mut params_sig = Vec::new();
        for (p, pty) in f.params.iter().zip(param_tys.iter()) {
            params_sig.push(format!("{} %arg.{}", pty.llvm(), p.name));
        }
        self.body.push_str(&format!(
            "define {} @{}({}) {{\n",
            ret_ty.llvm(),
            f.name,
            params_sig.join(", ")
        ));

        // Пролог: слоты под параметры.
        for (p, pty) in f.params.iter().zip(param_tys.iter()) {
            let slot = self.fresh_slot(&p.name);
            self.alloca(&slot, pty);
            self.emit(format!("store {ty} %arg.{name}, ptr {slot}", ty = pty.llvm(), name = p.name));
            self.scopes
                .last_mut()
                .unwrap()
                .insert(p.name.clone(), Local { slot, ty: pty.clone(), mutable: false });
        }

        if let Some(body) = &f.body {
            self.gen_block(body);
        }

        // Финальный терминатор, если провалились в конец.
        if !self.terminated {
            match &self.cur_ret {
                Ty::Void => self.emit("ret void".into()),
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
                    self.emit(format!("ret {} {}", other.llvm(), z));
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
                self.scopes.last_mut().unwrap().insert(
                    name.clone(),
                    Local { slot, ty: var_ty, mutable: *mutable },
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
                        self.emit("ret void".into());
                    }
                    (None, Ty::Void) => self.emit("ret void".into()),
                    (None, ret) => {
                        self.err(
                            "E0042",
                            *span,
                            format!("`return` без значения, а функция возвращает `{}`", ret.name()),
                            None,
                        );
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
                            // структура по значению: грузим агрегат из адреса.
                            let t = self.fresh_tmp();
                            self.emit(format!("{t} = load {ty}, ptr {val}", ty = ret.llvm()));
                            self.emit(format!("ret {} {}", ret.llvm(), t));
                        } else {
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

            Stmt::Asm(a) => self.gen_asm(a),
        }
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

        // Тип результата inline asm: один выход -> его тип, иначе void и
        // выходы через "=*m" не поддержаны в этой простой модели.
        let result_ty = if out_slots.len() == 1 { out_slots[0].1.clone() } else { Ty::Void };
        let cons = constraints.join(",");
        let dialect = "inteldialect"; // и masm, и nasm у нас Intel-синтаксис

        if result_ty == Ty::Void {
            self.emit(format!(
                "call void asm sideeffect {dialect} \"{asm}\", \"{cons}\"({args})",
                asm = asm_text,
                args = call_args.join(", ")
            ));
        } else {
            let t = self.fresh_tmp();
            self.emit(format!(
                "{t} = call {rty} asm sideeffect {dialect} \"{asm}\", \"{cons}\"({args})",
                rty = result_ty.llvm(),
                asm = asm_text,
                args = call_args.join(", ")
            ));
            // Записываем единственный выход обратно в переменную.
            let (slot, ty) = out_slots[0].clone();
            self.emit(format!("store {rty} {t}, ptr {slot}", rty = ty.llvm()));
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
                    // Индексация среза — безопасная операция (unsafe не нужен).
                    Ty::Slice(elem) => {
                        let dp = self.fresh_tmp();
                        self.emit(format!("{dp} = getelementptr %slice, ptr {bv}, i32 0, i32 0"));
                        let data = self.fresh_tmp();
                        self.emit(format!("{data} = load ptr, ptr {dp}"));
                        let (iv, _) = self.gen_expr(index, Some(&Ty::I64));
                        let t = self.fresh_tmp();
                        self.emit(format!("{t} = getelementptr {ety}, ptr {data}, i64 {iv}", ety = elem.llvm()));
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
                        self.err("E0053", base.span(), format!("индексировать можно только указатель или срез, а тут `{}`", other.name()), None);
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
                self.require_unsafe(base.span(), "доступ к полю через сырой указатель");
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
                self.require_unsafe(base.span(), "запись поля через сырой указатель");
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
            Expr::Str(s, _) => {
                let g = self.intern_string(s);
                (g, Ty::Ptr(Box::new(Ty::U8), false))
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
                    // Захваченная в jit-шаблоне переменная -> плейсхолдер-константа.
                    if let Some(caps) = &self.captures {
                        if let Some((idx, ty)) = caps.get(name) {
                            return (format!("$CAP{idx}$"), ty.clone());
                        }
                    }
                    if self.ctx.fns.contains_key(name) {
                        self.err("E0057", *span, format!("функцию `{name}` нельзя использовать как значение"), Some("её можно только вызывать: `{name}(...)`"));
                    } else {
                        self.err("E0032", *span, format!("неизвестное имя `{name}`"), Some("объявите переменную через `let`"));
                    }
                    ("0".into(), Ty::Err)
                }
            },
            Expr::Unary { op, expr, span } => self.gen_unary(*op, expr, *span, expected),
            Expr::Binary { op, lhs, rhs, span } => self.gen_binary(*op, lhs, rhs, *span, expected),
            Expr::Cast { expr, ty, span } => self.gen_cast(expr, ty, *span),
            Expr::Call { callee, args, span } => self.gen_call(callee, args, *span),
            Expr::Field { base, field, span } => {
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
                let t = self.fresh_tmp();
                self.emit(format!("{t} = load {lty}, ptr {ptr}", lty = ty.llvm()));
                (t, ty)
            }
            Expr::StructLit { name, fields, span } => self.gen_struct_lit(name, fields, *span),
            Expr::Jit { captures, inner, span } => self.gen_jit(captures, inner, *span),
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
        // Указатели: требуют unsafe.
        if src.is_ptr() && dst.is_ptr() {
            return (v, dst); // непрозрачные ptr — без инструкции
        }
        if (src.is_ptr() && dst.is_int()) || (src.is_int() && dst.is_ptr()) {
            self.require_unsafe(span, "приведение между указателем и числом");
            let t = self.fresh_tmp();
            let instr = if src.is_ptr() { "ptrtoint" } else { "inttoptr" };
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

    fn gen_call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> (String, Ty) {
        let name = match callee {
            Expr::Ident(n, _) => n.clone(),
            _ => {
                self.err("E0070", callee.span(), "вызывать можно только функцию по имени".into(), None);
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
                if let Some(r) = self.try_builtin(&name, args, span) {
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
            if sig.ret == Ty::Void {
                self.emit(format!("call void ({plist}) @{name}({})", argvals.join(", ")));
                ("".into(), Ty::Void)
            } else {
                let t = self.fresh_tmp();
                self.emit(format!("{t} = call {rty} ({plist}) @{name}({})", argvals.join(", "), rty = sig.ret.llvm()));
                self.spill_if_aggregate(t, sig.ret)
            }
        } else if sig.ret == Ty::Void {
            self.emit(format!("call void @{name}({})", argvals.join(", ")));
            ("".into(), Ty::Void)
        } else {
            let t = self.fresh_tmp();
            self.emit(format!("{t} = call {rty} @{name}({})", argvals.join(", "), rty = sig.ret.llvm()));
            self.spill_if_aggregate(t, sig.ret)
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
    fn try_builtin(&mut self, name: &str, args: &[Expr], span: Span) -> Option<(String, Ty)> {
        // Куча и работа с памятью — разблокируют динамические структуры
        // (Vec/Bytes/String можно писать на самом Goraw поверх этого).
        match name {
            "alloc" | "free" | "realloc" | "mem_copy" | "mem_set" => {
                return Some(self.heap_builtin(name, args, span));
            }
            "make_slice" => return Some(self.bi_make_slice(args, span)),
            "f32_bits" | "f64_bits" => return Some(self.bi_bits(name, args, span)),
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

    /// Непрямой вызов через функцию-указатель (например, хендл jit-блока).
    fn gen_indirect_call(&mut self, name: &str, slot: &str, params: &[Ty], ret: &Ty, args: &[Expr], span: Span) -> (String, Ty) {
        let fp = self.fresh_tmp();
        self.emit(format!("{fp} = load ptr, ptr {slot}"));

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
            other => (v, other),
        }
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
            Expr::Str(..) => Ty::Ptr(Box::new(Ty::U8), false),
            Expr::Ident(n, _) => self.lookup(n).map(|l| l.ty.clone()).unwrap_or(Ty::Err),
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
                    if let Some(l) = self.lookup(n) {
                        if let Ty::FnPtr(_, ret) = &l.ty {
                            return (**ret).clone();
                        }
                    }
                    self.ctx.fns.get(n).map(|s| s.ret.clone()).unwrap_or(Ty::Err)
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
            Expr::Field { base, field, .. } => {
                let bt = self.type_of(base);
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
                _ => Ty::Err,
            },
            Expr::StructLit { name, .. } => Ty::Struct(name.clone()),
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
        format!("%{base}.{}", self.slotcount)
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
    matches!(ty, Ty::Struct(_) | Ty::Slice(_))
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

/// Форматирует float как точную hex-константу LLVM (без потери точности).
fn fmt_float(v: f64, ty: &Ty) -> String {
    let bits = if *ty == Ty::F32 {
        (v as f32 as f64).to_bits()
    } else {
        v.to_bits()
    };
    format!("0x{:016X}", bits)
}
