//! Генератор LLVM IR для Goraw + семантические проверки в один проход.
//! Сигнатуры и структуры уже собраны в `TyCtx` (первый проход), поэтому
//! здесь поддержаны forward-ссылки. При любой ошибке в Diags модуль
//! продолжает работу с «отравленным» типом `Ty::Err`, гася каскады, а
//! итоговый IR вызывающая сторона просто выбрасывает, если были ошибки.

use crate::ast::*;
use crate::diag::{Diagnostic, Diags, Span};
use crate::types::{Ty, TyCtx};
use std::collections::HashMap;

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
        }
    }

    // ---------- сборка модуля ----------

    pub fn emit_module(mut self, prog: &Program) -> String {
        let mut header = String::new();
        header.push_str("; Goraw -> LLVM IR\n");
        header.push_str("target triple = \"x86_64-w64-windows-gnu\"\n\n");

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
        header.push('\n');

        // Тела функций.
        for f in &prog.fns {
            if !f.is_extern {
                self.gen_fn(f);
            }
        }

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
        let sig = self.ctx.fns[&f.name].clone();
        self.tmp = 0;
        self.label = 0;
        self.slotcount = 0;
        self.allocas.clear();
        self.code.clear();
        self.cur_ret = sig.ret.clone();
        self.cur_unsafe = f.is_unsafe;
        self.unsafe_depth = 0;
        self.scopes.clear();
        self.loops.clear();
        self.terminated = false;
        self.scopes.push(HashMap::new());

        // Сигнатура.
        let mut params_sig = Vec::new();
        for (p, pty) in f.params.iter().zip(sig.params.iter()) {
            params_sig.push(format!("{} %arg.{}", pty.llvm(), p.name));
        }
        self.body.push_str(&format!(
            "define {} @{}({}) {{\n",
            sig.ret.llvm(),
            f.name,
            params_sig.join(", ")
        ));

        // Пролог: слоты под параметры.
        for (p, pty) in f.params.iter().zip(sig.params.iter()) {
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
                        if *t != vty && vty != Ty::Err && *t != Ty::Err {
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
                if ty != vty && ty != Ty::Err && vty != Ty::Err {
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
                        if *ret != vty && vty != Ty::Err {
                            self.err(
                                "E0043",
                                e.span(),
                                format!("возвращается `{}`, а ожидается `{}`", vty.name(), ret.name()),
                                None,
                            );
                        }
                        if let Ty::Struct(_) = ret {
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
                self.require_unsafe(*span, "индексация сырого указателя");
                let (iv, _) = self.gen_expr(index, Some(&Ty::I64));
                match bty {
                    Ty::Ptr(inner, m) => {
                        let t = self.fresh_tmp();
                        self.emit(format!(
                            "{t} = getelementptr {ety}, ptr {bv}, i64 {iv}",
                            ety = inner.llvm()
                        ));
                        (t, *inner, m)
                    }
                    Ty::Err => ("%poison".into(), Ty::Err, true),
                    other => {
                        self.err("E0053", base.span(), format!("индексировать можно только указатель, а тут `{}`", other.name()), None);
                        ("%poison".into(), Ty::Err, true)
                    }
                }
            }
            Expr::Field { base, field, span } => {
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
                    if let Ty::Struct(_) = l.ty {
                        // структура-значение представляется адресом слота
                        (l.slot.clone(), l.ty)
                    } else {
                        let t = self.fresh_tmp();
                        self.emit(format!("{t} = load {ty}, ptr {slot}", ty = l.ty.llvm(), slot = l.slot));
                        (t, l.ty)
                    }
                }
                None => {
                    if self.ctx.fns.contains_key(name) {
                        self.err("E0057", *span, format!("функцию `{name}` нельзя использовать как значение"), Some("её можно только вызывать: `{name}(...)`"));
                    } else {
                        self.err("E0032", *span, format!("неизвестное имя `{name}`"), Some("объявите переменную через `let`"));
                    }
                    ("0".into(), Ty::Err)
                }
            },
            Expr::Unary { op, expr, span } => self.gen_unary(*op, expr, *span),
            Expr::Binary { op, lhs, rhs, span } => self.gen_binary(*op, lhs, rhs, *span, expected),
            Expr::Cast { expr, ty, span } => self.gen_cast(expr, ty, *span),
            Expr::Call { callee, args, span } => self.gen_call(callee, args, *span),
            Expr::Field { base, field, span } => {
                let (addr, sname) = self.struct_addr_rvalue(base);
                if sname.is_empty() {
                    return ("0".into(), Ty::Err);
                }
                let (ptr, fty, _) = self.field_gep(&addr, &sname, field, *span, false);
                if let Ty::Struct(_) = fty {
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
        }
    }

    fn gen_unary(&mut self, op: UnOp, expr: &Expr, span: Span) -> (String, Ty) {
        match op {
            UnOp::Neg => {
                let (v, ty) = self.gen_expr(expr, None);
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
                if let Ty::Struct(_) = ty {
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
        let sig = match self.ctx.fns.get(&name) {
            Some(s) => s.clone(),
            None => {
                self.err("E0071", span, format!("вызов неизвестной функции `{name}`"), Some("объявите её или добавьте `extern fn`"));
                return ("0".into(), Ty::Err);
            }
        };

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
                if *pt != vty && vty != Ty::Err && *pt != Ty::Err {
                    self.err(
                        "E0073",
                        a.span(),
                        format!("аргумент {} функции `{name}`: ожидался `{}`, передан `{}`", i + 1, pt.name(), vty.name()),
                        Some("приведите значение через `as`"),
                    );
                }
                // структура по значению -> грузим агрегат
                if let Ty::Struct(_) = pt {
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
                (t, sig.ret)
            }
        } else if sig.ret == Ty::Void {
            self.emit(format!("call void @{name}({})", argvals.join(", ")));
            ("".into(), Ty::Void)
        } else {
            let t = self.fresh_tmp();
            self.emit(format!("{t} = call {rty} @{name}({})", argvals.join(", "), rty = sig.ret.llvm()));
            (t, sig.ret)
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
                    if fty != vty && vty != Ty::Err && fty != Ty::Err {
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
                Expr::Ident(n, _) => self.ctx.fns.get(n).map(|s| s.ret.clone()).unwrap_or(Ty::Err),
                _ => Ty::Err,
            },
            Expr::Field { base, field, .. } => {
                let bt = self.type_of(base);
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

    /// Копирует значение в слот: скаляр — store; структура — load+store агрегата.
    fn store_value(&mut self, dst_ty: &Ty, val: &str, val_ty: &Ty, slot: &str) {
        if let Ty::Struct(_) = dst_ty {
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
        } else if let Ty::Struct(_) = ty {
            "zeroinitializer".into()
        } else if ty.is_ptr() {
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

/// Форматирует float как точную hex-константу LLVM (без потери точности).
fn fmt_float(v: f64, ty: &Ty) -> String {
    let bits = if *ty == Ty::F32 {
        (v as f32 as f64).to_bits()
    } else {
        v.to_bits()
    };
    format!("0x{:016X}", bits)
}
