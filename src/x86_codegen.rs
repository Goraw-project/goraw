//! Нативный x86-64 кодогенератор компилятора Goraw.
//!
//! Генерирует машинный ассемблер Win64 x86-64 напрямую из AST (ast::Program),
//! который затем собирается встроенным ассемблером `gorawas` (iced-x86)
//! в COFF-объектник (.obj) и линкуется встроенным `linker.rs` в .exe.
//!
//! Полный цикл компиляции без вызова внешнего Clang / LLVM.

use crate::ast::*;
use std::collections::{HashMap, HashSet};

pub fn compile_program_to_obj(prog: &Program, filename: &str) -> Result<Vec<u8>, String> {
    let asm_code = compile_program_to_asm(prog)?;
    let (obj_bytes, diags) = crate::asm::assemble(filename, &asm_code);
    if diags.has_errors() {
        return Err(format!("ошибка ассемблирования нативного x86-64 кода:\n{}", diags.render_human()));
    }
    obj_bytes.ok_or_else(|| "ассемблер не вернул объектный файл".to_string())
}

pub fn compile_program_to_asm(prog: &Program) -> Result<String, String> {
    let mut emitter = X86Emitter::new();
    emitter.compile_program(prog)
}

struct X86Emitter {
    text: Vec<String>,
    data: Vec<String>,
    externs: HashSet<String>,
    globals: HashSet<String>,
    strings: Vec<(String, Vec<u8>)>,
    label_counter: usize,
    locals: HashMap<String, i32>,
    next_local_offset: i32,
    loop_stack: Vec<(String, String)>,
    current_fn_ret_label: String,
    known_fns: HashSet<String>,
}

impl X86Emitter {
    fn new() -> Self {
        Self {
            text: Vec::new(),
            data: Vec::new(),
            externs: HashSet::new(),
            globals: HashSet::new(),
            strings: Vec::new(),
            label_counter: 0,
            locals: HashMap::new(),
            next_local_offset: 8,
            loop_stack: Vec::new(),
            current_fn_ret_label: String::new(),
            known_fns: HashSet::new(),
        }
    }

    fn new_label(&mut self, prefix: &str) -> String {
        self.label_counter += 1;
        format!(".L{}_{}", prefix, self.label_counter)
    }

    fn emit_text(&mut self, s: &str) {
        self.text.push(format!("    {s}"));
    }

    fn emit_label(&mut self, label: &str) {
        self.text.push(format!("{label}:"));
    }

    fn intern_string(&mut self, bytes: &[u8]) -> String {
        for (lbl, existing) in &self.strings {
            if existing.as_slice() == bytes {
                return lbl.clone();
            }
        }
        let lbl = format!("_gw_str_{}", self.strings.len());
        self.strings.push((lbl.clone(), bytes.to_vec()));
        lbl
    }

    fn alloc_local(&mut self, name: &str) -> i32 {
        if let Some(&off) = self.locals.get(name) {
            return off;
        }
        let off = self.next_local_offset;
        self.next_local_offset += 8;
        self.locals.insert(name.to_string(), off);
        off
    }

    fn compile_program(&mut self, prog: &Program) -> Result<String, String> {
        for f in &prog.fns {
            self.known_fns.insert(f.name.clone());
            if f.body.is_none() || f.is_extern {
                self.externs.insert(f.name.clone());
            } else {
                self.globals.insert(f.name.clone());
            }
        }

        // 1. Компилируем функции
        for f in &prog.fns {
            if f.body.is_some() && !f.is_extern {
                self.compile_fn(f)?;
            }
        }

        // 2. Статические переменные
        for s in &prog.statics {
            self.globals.insert(s.name.clone());
            let val = match &s.value {
                Expr::Int(n, _) => *n,
                Expr::Bool(b, _) => if *b { 1 } else { 0 },
                _ => 0,
            };
            self.data.push(format!("global {}", s.name));
            self.data.push(format!("{}: dq {}", s.name, val));
        }

        // 3. Формируем итоговый листинг
        let mut asm = String::new();
        asm.push_str("; Goraw Native x86-64 Backend Code Generator\n");
        asm.push_str("; Target: x86_64-pc-windows-msvc (Win64 ABI)\n\n");

        // Секция .rdata
        asm.push_str("section .rdata\n");
        for (lbl, bytes) in &self.strings {
            let mut byte_strs: Vec<String> = bytes.iter().map(|b| b.to_string()).collect();
            byte_strs.push("0".to_string()); // null terminator
            asm.push_str(&format!("{lbl}: db {}\n", byte_strs.join(", ")));
        }
        asm.push('\n');

        // Секция .data
        asm.push_str("section .data\n");
        // Стандартная переменная MSVC CRT для операций с плавающей точкой
        asm.push_str("global _fltused\n");
        asm.push_str("_fltused: dd 0x9876\n");
        for line in &self.data {
            asm.push_str(line);
            asm.push('\n');
        }
        asm.push('\n');

        // Секция .text
        asm.push_str("section .text\n");
        for g in &self.globals {
            asm.push_str(&format!("global {g}\n"));
        }
        for e in &self.externs {
            if !self.globals.contains(e) {
                asm.push_str(&format!("extern {e}\n"));
            }
        }
        asm.push('\n');

        for line in &self.text {
            asm.push_str(line);
            asm.push('\n');
        }

        Ok(asm)
    }

    fn compile_fn(&mut self, f: &FnDef) -> Result<(), String> {
        self.locals.clear();
        self.next_local_offset = 8;
        self.current_fn_ret_label = self.new_label(&format!("ret_{}", f.name));

        // Вычисляем размер стека под параметры и локальные переменные
        for p in &f.params {
            self.alloc_local(&p.name);
        }

        // Предварительное сканирование блока для резервирования локальных переменных
        if let Some(body) = &f.body {
            self.scan_locals(&body.stmts);
        }

        let raw_size = self.next_local_offset + 32; // +32 байта под shadow space вызовов
        let aligned_stack_size = if raw_size % 16 == 0 { raw_size } else { raw_size + (16 - (raw_size % 16)) };

        self.emit_label(&f.name);
        self.emit_text("push rbp");
        self.emit_text("mov rbp, rsp");
        self.emit_text(&format!("sub rsp, {aligned_stack_size}"));

        // Сохраняем первые 4 параметра Win64 ABI из регистров в локальные слоты стека
        let win64_param_regs = ["rcx", "rdx", "r8", "r9"];
        for (i, p) in f.params.iter().enumerate() {
            let off = self.locals.get(&p.name).copied().unwrap_or(8);
            if i < 4 {
                self.emit_text(&format!("mov [rbp - {off}], {}", win64_param_regs[i]));
            } else {
                // Параметры 4+ передаются вызывающей стороной на стеке выше shadow space: [rbp + 16 + (i * 8)]
                let caller_stack_off = 16 + (i as i32 * 8);
                self.emit_text(&format!("mov rax, [rbp + {caller_stack_off}]"));
                self.emit_text(&format!("mov [rbp - {off}], rax"));
            }
        }

        if let Some(body) = &f.body {
            for stmt in &body.stmts {
                self.compile_stmt(stmt)?;
            }
        }

        // Если функция не завершилась return, выставляем по умолчанию 0 в rax
        self.emit_text("xor rax, rax");

        let ret_lbl = self.current_fn_ret_label.clone();
        self.emit_label(&ret_lbl);
        self.emit_text("mov rsp, rbp");
        self.emit_text("pop rbp");
        self.emit_text("ret");
        self.text.push(String::new());

        Ok(())
    }

    fn scan_locals(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            match s {
                Stmt::Let { name, .. } => {
                    self.alloc_local(name);
                }
                Stmt::If { then, els, .. } => {
                    self.scan_locals(&then.stmts);
                    if let Some(e) = els {
                        self.scan_locals(&e.stmts);
                    }
                }
                Stmt::While { body, .. } => {
                    self.scan_locals(&body.stmts);
                }
                Stmt::For { init, body, .. } => {
                    if let Some(i) = init {
                        self.scan_locals(std::slice::from_ref(i));
                    }
                    self.scan_locals(&body.stmts);
                }
                Stmt::Unsafe(b, _) => {
                    self.scan_locals(&b.stmts);
                }
                Stmt::Match { arms, .. } => {
                    for (_, b) in arms {
                        self.scan_locals(&b.stmts);
                    }
                }
                _ => {}
            }
        }
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::Let { name, value, .. } => {
                let off = self.alloc_local(name);
                self.compile_expr(value)?;
                self.emit_text(&format!("mov [rbp - {off}], rax"));
            }
            Stmt::Assign { target, value, .. } => {
                match target {
                    Expr::Ident(name, _) => {
                        let off = self.alloc_local(name);
                        self.compile_expr(value)?;
                        self.emit_text(&format!("mov [rbp - {off}], rax"));
                    }
                    Expr::Unary { op: UnOp::Deref, expr, .. } => {
                        self.compile_expr(value)?;
                        self.emit_text("push rax");
                        self.compile_expr(expr)?;
                        self.emit_text("pop rbx");
                        self.emit_text("mov [rax], rbx");
                    }
                    Expr::Index { base, index, .. } => {
                        self.compile_expr(value)?;
                        self.emit_text("push rax");
                        self.compile_expr(base)?;
                        self.emit_text("push rax");
                        self.compile_expr(index)?;
                        self.emit_text("mov rbx, rax");
                        self.emit_text("pop rcx");
                        self.emit_text("pop rax");
                        self.emit_text("mov [rcx + rbx*8], rax");
                    }
                    _ => {
                        self.compile_expr(value)?;
                    }
                }
            }
            Stmt::Return(maybe_expr, _) => {
                if let Some(expr) = maybe_expr {
                    self.compile_expr(expr)?;
                } else {
                    self.emit_text("xor rax, rax");
                }
                let ret_lbl = self.current_fn_ret_label.clone();
                self.emit_text(&format!("jmp {ret_lbl}"));
            }
            Stmt::If { cond, then, els, .. } => {
                if let Some(e) = els {
                    let else_lbl = self.new_label("else");
                    let end_lbl = self.new_label("end_if");

                    self.compile_expr(cond)?;
                    self.emit_text("cmp rax, 0");
                    self.emit_text(&format!("je {else_lbl}"));

                    for s in &then.stmts {
                        self.compile_stmt(s)?;
                    }
                    self.emit_text(&format!("jmp {end_lbl}"));

                    self.emit_label(&else_lbl);
                    for s in &e.stmts {
                        self.compile_stmt(s)?;
                    }
                    self.emit_label(&end_lbl);
                    self.emit_text("nop");
                } else {
                    let end_lbl = self.new_label("end_if");
                    self.compile_expr(cond)?;
                    self.emit_text("cmp rax, 0");
                    self.emit_text(&format!("je {end_lbl}"));

                    for s in &then.stmts {
                        self.compile_stmt(s)?;
                    }
                    self.emit_label(&end_lbl);
                    self.emit_text("nop");
                }
            }
            Stmt::While { cond, body, .. } => {
                let start_lbl = self.new_label("while_start");
                let end_lbl = self.new_label("while_end");

                self.loop_stack.push((end_lbl.clone(), start_lbl.clone()));
                self.emit_label(&start_lbl);

                self.compile_expr(cond)?;
                self.emit_text("cmp rax, 0");
                self.emit_text(&format!("je {end_lbl}"));

                for s in &body.stmts {
                    self.compile_stmt(s)?;
                }
                self.emit_text(&format!("jmp {start_lbl}"));

                self.emit_label(&end_lbl);
                self.emit_text("nop");
                self.loop_stack.pop();
            }
            Stmt::For { init, cond, post, body, .. } => {
                if let Some(i) = init {
                    self.compile_stmt(i)?;
                }
                let start_lbl = self.new_label("for_start");
                let cont_lbl = self.new_label("for_cont");
                let end_lbl = self.new_label("for_end");

                self.loop_stack.push((end_lbl.clone(), cont_lbl.clone()));
                self.emit_label(&start_lbl);

                if let Some(c) = cond {
                    self.compile_expr(c)?;
                    self.emit_text("cmp rax, 0");
                    self.emit_text(&format!("je {end_lbl}"));
                }

                for s in &body.stmts {
                    self.compile_stmt(s)?;
                }

                self.emit_label(&cont_lbl);
                if let Some(p) = post {
                    self.compile_stmt(p)?;
                }
                self.emit_text(&format!("jmp {start_lbl}"));

                self.emit_label(&end_lbl);
                self.emit_text("nop");
                self.loop_stack.pop();
            }
            Stmt::Break(_) => {
                if let Some((brk, _)) = self.loop_stack.last() {
                    let brk = brk.clone();
                    self.emit_text(&format!("jmp {brk}"));
                }
            }
            Stmt::Continue(_) => {
                if let Some((_, cont)) = self.loop_stack.last() {
                    let cont = cont.clone();
                    self.emit_text(&format!("jmp {cont}"));
                }
            }
            Stmt::Expr(expr) => {
                self.compile_expr(expr)?;
            }
            Stmt::Asm(asm_blk) => {
                for line in asm_blk.body.lines() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        self.emit_text(trimmed);
                    }
                }
            }
            Stmt::Assert(expr, _) => {
                let ok_lbl = self.new_label("assert_ok");
                self.compile_expr(expr)?;
                self.emit_text("cmp rax, 0");
                self.emit_text(&format!("jne {ok_lbl}"));
                self.externs.insert("ExitProcess".to_string());
                self.emit_text("mov rcx, 1");
                self.emit_text("sub rsp, 32");
                self.emit_text("call ExitProcess");
                self.emit_text("add rsp, 32");
                self.emit_label(&ok_lbl);
            }
            Stmt::Unsafe(block, _) => {
                for s in &block.stmts {
                    self.compile_stmt(s)?;
                }
            }
            Stmt::Match { scrut, arms, .. } => {
                self.compile_expr(scrut)?;
                let end_match = self.new_label("end_match");
                for (pat, block) in arms {
                    let next_arm = self.new_label("next_arm");
                    if let Some(p) = pat {
                        self.emit_text("push rax");
                        self.compile_expr(p)?;
                        self.emit_text("mov rbx, rax");
                        self.emit_text("pop rax");
                        self.emit_text("cmp rax, rbx");
                        self.emit_text(&format!("jne {next_arm}"));
                    }
                    for s in &block.stmts {
                        self.compile_stmt(s)?;
                    }
                    self.emit_text(&format!("jmp {end_match}"));
                    self.emit_label(&next_arm);
                }
                self.emit_label(&end_match);
            }
            Stmt::ForIn { var, iter, body, .. } => {
                // Итерация по слайсу [ptr, len]
                let var_off = self.alloc_local(var);
                let idx_off = self.alloc_local(&format!("__idx_{var}"));
                let len_off = self.alloc_local(&format!("__len_{var}"));
                let ptr_off = self.alloc_local(&format!("__ptr_{var}"));

                self.compile_expr(iter)?;
                // rax хранит указатель на данные
                self.emit_text(&format!("mov [rbp - {ptr_off}], rax"));
                self.emit_text(&format!("mov [rbp - {len_off}], 1024")); // дефолтная граница
                self.emit_text(&format!("mov [rbp - {idx_off}], 0"));

                let start_lbl = self.new_label("forin_start");
                let end_lbl = self.new_label("forin_end");

                self.emit_label(&start_lbl);
                self.emit_text(&format!("mov rax, [rbp - {idx_off}]"));
                self.emit_text(&format!("cmp rax, [rbp - {len_off}]"));
                self.emit_text(&format!("jge {end_lbl}"));

                self.emit_text(&format!("mov rcx, [rbp - {ptr_off}]"));
                self.emit_text("mov rdx, [rcx + rax*8]");
                self.emit_text(&format!("mov [rbp - {var_off}], rdx"));

                for s in &body.stmts {
                    self.compile_stmt(s)?;
                }

                self.emit_text(&format!("mov rax, [rbp - {idx_off}]"));
                self.emit_text("inc rax");
                self.emit_text(&format!("mov [rbp - {idx_off}], rax"));
                self.emit_text(&format!("jmp {start_lbl}"));
                self.emit_label(&end_lbl);
            }
            Stmt::InlineC { .. } => {}
        }
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), String> {
        match expr {
            Expr::Int(n, _) => {
                self.emit_text(&format!("mov rax, {n}"));
            }
            Expr::Float(f, _) => {
                let bits = f.to_bits();
                self.emit_text(&format!("mov rax, {bits}"));
            }
            Expr::Bool(b, _) => {
                let v = if *b { 1 } else { 0 };
                self.emit_text(&format!("mov rax, {v}"));
            }
            Expr::Str(s, _) => {
                let lbl = self.intern_string(s.as_bytes());
                self.emit_text(&format!("lea rax, [rel {lbl}]"));
            }
            Expr::Ident(name, _) => {
                if let Some(&off) = self.locals.get(name) {
                    self.emit_text(&format!("mov rax, [rbp - {off}]"));
                } else if self.known_fns.contains(name) || self.externs.contains(name) {
                    self.emit_text(&format!("lea rax, [rel {name}]"));
                } else {
                    self.emit_text(&format!("mov rax, [rel {name}]"));
                }
            }
            Expr::Null(_) => {
                self.emit_text("xor rax, rax");
            }
            Expr::Path(_module, variant, _) => {
                if let Some(&off) = self.locals.get(variant) {
                    self.emit_text(&format!("mov rax, [rbp - {off}]"));
                } else {
                    self.emit_text("xor rax, rax");
                }
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                self.compile_expr(lhs)?;
                self.emit_text("push rax");
                self.compile_expr(rhs)?;
                self.emit_text("mov rbx, rax");
                self.emit_text("pop rax");

                match op {
                    BinOp::Add => self.emit_text("add rax, rbx"),
                    BinOp::Sub => self.emit_text("sub rax, rbx"),
                    BinOp::Mul => self.emit_text("imul rax, rbx"),
                    BinOp::Div => {
                        self.emit_text("cqo");
                        self.emit_text("idiv rbx");
                    }
                    BinOp::Rem => {
                        self.emit_text("cqo");
                        self.emit_text("idiv rbx");
                        self.emit_text("mov rax, rdx");
                    }
                    BinOp::BitAnd => self.emit_text("and rax, rbx"),
                    BinOp::BitOr => self.emit_text("or rax, rbx"),
                    BinOp::BitXor => self.emit_text("xor rax, rbx"),
                    BinOp::Shl => {
                        self.emit_text("mov rcx, rbx");
                        self.emit_text("shl rax, cl");
                    }
                    BinOp::Shr => {
                        self.emit_text("mov rcx, rbx");
                        self.emit_text("sar rax, cl");
                    }
                    BinOp::Eq => {
                        let l_true = self.new_label("eq_true");
                        let l_end = self.new_label("eq_end");
                        self.emit_text("cmp rax, rbx");
                        self.emit_text(&format!("je {l_true}"));
                        self.emit_text("xor rax, rax");
                        self.emit_text(&format!("jmp {l_end}"));
                        self.emit_label(&l_true);
                        self.emit_text("mov rax, 1");
                        self.emit_label(&l_end);
                        self.emit_text("nop");
                    }
                    BinOp::Ne => {
                        let l_true = self.new_label("ne_true");
                        let l_end = self.new_label("ne_end");
                        self.emit_text("cmp rax, rbx");
                        self.emit_text(&format!("jne {l_true}"));
                        self.emit_text("xor rax, rax");
                        self.emit_text(&format!("jmp {l_end}"));
                        self.emit_label(&l_true);
                        self.emit_text("mov rax, 1");
                        self.emit_label(&l_end);
                        self.emit_text("nop");
                    }
                    BinOp::Lt => {
                        let l_true = self.new_label("lt_true");
                        let l_end = self.new_label("lt_end");
                        self.emit_text("cmp rax, rbx");
                        self.emit_text(&format!("jl {l_true}"));
                        self.emit_text("xor rax, rax");
                        self.emit_text(&format!("jmp {l_end}"));
                        self.emit_label(&l_true);
                        self.emit_text("mov rax, 1");
                        self.emit_label(&l_end);
                        self.emit_text("nop");
                    }
                    BinOp::Le => {
                        let l_true = self.new_label("le_true");
                        let l_end = self.new_label("le_end");
                        self.emit_text("cmp rax, rbx");
                        self.emit_text(&format!("jle {l_true}"));
                        self.emit_text("xor rax, rax");
                        self.emit_text(&format!("jmp {l_end}"));
                        self.emit_label(&l_true);
                        self.emit_text("mov rax, 1");
                        self.emit_label(&l_end);
                        self.emit_text("nop");
                    }
                    BinOp::Gt => {
                        let l_true = self.new_label("gt_true");
                        let l_end = self.new_label("gt_end");
                        self.emit_text("cmp rax, rbx");
                        self.emit_text(&format!("jg {l_true}"));
                        self.emit_text("xor rax, rax");
                        self.emit_text(&format!("jmp {l_end}"));
                        self.emit_label(&l_true);
                        self.emit_text("mov rax, 1");
                        self.emit_label(&l_end);
                        self.emit_text("nop");
                    }
                    BinOp::Ge => {
                        let l_true = self.new_label("ge_true");
                        let l_end = self.new_label("ge_end");
                        self.emit_text("cmp rax, rbx");
                        self.emit_text(&format!("jge {l_true}"));
                        self.emit_text("xor rax, rax");
                        self.emit_text(&format!("jmp {l_end}"));
                        self.emit_label(&l_true);
                        self.emit_text("mov rax, 1");
                        self.emit_label(&l_end);
                        self.emit_text("nop");
                    }
                    BinOp::And => {
                        let l_false = self.new_label("and_false");
                        let l_end = self.new_label("and_end");
                        self.emit_text("cmp rax, 0");
                        self.emit_text(&format!("je {l_false}"));
                        self.emit_text("cmp rbx, 0");
                        self.emit_text(&format!("je {l_false}"));
                        self.emit_text("mov rax, 1");
                        self.emit_text(&format!("jmp {l_end}"));
                        self.emit_label(&l_false);
                        self.emit_text("xor rax, rax");
                        self.emit_label(&l_end);
                        self.emit_text("nop");
                    }
                    BinOp::Or => {
                        let l_true = self.new_label("or_true");
                        let l_end = self.new_label("or_end");
                        self.emit_text("cmp rax, 0");
                        self.emit_text(&format!("jne {l_true}"));
                        self.emit_text("cmp rbx, 0");
                        self.emit_text(&format!("jne {l_true}"));
                        self.emit_text("xor rax, rax");
                        self.emit_text(&format!("jmp {l_end}"));
                        self.emit_label(&l_true);
                        self.emit_text("mov rax, 1");
                        self.emit_label(&l_end);
                        self.emit_text("nop");
                    }
                }
            }
            Expr::Unary { op, expr, .. } => {
                match op {
                    UnOp::Neg => {
                        self.compile_expr(expr)?;
                        self.emit_text("neg rax");
                    }
                    UnOp::Not => {
                        let l_zero = self.new_label("not_zero");
                        let l_end = self.new_label("not_end");
                        self.compile_expr(expr)?;
                        self.emit_text("cmp rax, 0");
                        self.emit_text(&format!("je {l_zero}"));
                        self.emit_text("xor rax, rax");
                        self.emit_text(&format!("jmp {l_end}"));
                        self.emit_label(&l_zero);
                        self.emit_text("mov rax, 1");
                        self.emit_label(&l_end);
                        self.emit_text("nop");
                    }
                    UnOp::BitNot => {
                        self.compile_expr(expr)?;
                        self.emit_text("not rax");
                    }
                    UnOp::Deref => {
                        self.compile_expr(expr)?;
                        self.emit_text("mov rax, [rax]");
                    }
                    UnOp::Ref | UnOp::RefMut => {
                        if let Expr::Ident(name, _) = expr.as_ref() {
                            let off = self.alloc_local(name);
                            self.emit_text(&format!("lea rax, [rbp - {off}]"));
                        } else {
                            self.compile_expr(expr)?;
                        }
                    }
                }
            }
            Expr::Call { callee, args, .. } => {
                let callee_name = match callee.as_ref() {
                    Expr::Ident(name, _) => name.clone(),
                    _ => "callee".to_string(),
                };

                if !self.known_fns.contains(&callee_name) {
                    self.externs.insert(callee_name.clone());
                }

                // Вычисляем аргументы и сохраняем на стеке
                for arg in args {
                    self.compile_expr(arg)?;
                    self.emit_text("push rax");
                }

                // Загружаем аргументы в регистры Win64 ABI (обратный порядок со стека)
                let num_args = args.len();
                let win64_regs = ["rcx", "rdx", "r8", "r9"];
                let reg_args = num_args.min(4);

                // Если аргументов > 4, оставшиеся лежат на стеке выше shadow space
                for i in (0..reg_args).rev() {
                    self.emit_text(&format!("pop {}", win64_regs[i]));
                }

                // 32 байта shadow space для Win64 ABI вызова
                self.emit_text("sub rsp, 32");
                self.emit_text(&format!("call {callee_name}"));
                self.emit_text("add rsp, 32");

                // Если были аргументы на стеке (5+), освобождаем их
                if num_args > 4 {
                    let extra_stack = (num_args - 4) * 8;
                    self.emit_text(&format!("add rsp, {extra_stack}"));
                }
            }
            Expr::Field { base, .. } => {
                self.compile_expr(base)?;
                self.emit_text("mov rax, [rax]");
            }
            Expr::Index { base, index, .. } => {
                self.compile_expr(base)?;
                self.emit_text("push rax");
                self.compile_expr(index)?;
                self.emit_text("mov rbx, rax");
                self.emit_text("pop rcx");
                self.emit_text("mov rax, [rcx + rbx*8]");
            }
            Expr::IfExpr { cond, then, els, .. } => {
                let else_lbl = self.new_label("if_expr_else");
                let end_lbl = self.new_label("if_expr_end");

                self.compile_expr(cond)?;
                self.emit_text("cmp rax, 0");
                self.emit_text(&format!("je {else_lbl}"));

                self.compile_expr(then)?;
                self.emit_text(&format!("jmp {end_lbl}"));

                self.emit_label(&else_lbl);
                self.compile_expr(els)?;
                self.emit_label(&end_lbl);
                self.emit_text("nop");
            }
            Expr::Cast { expr, .. } => {
                self.compile_expr(expr)?;
            }
            Expr::Slice { base, .. } => {
                self.compile_expr(base)?;
            }
            Expr::ArrayLit(items, _) => {
                if let Some(first) = items.first() {
                    self.compile_expr(first)?;
                } else {
                    self.emit_text("xor rax, rax");
                }
            }
            Expr::StructLit { fields, .. } => {
                if let Some((_, first_expr, _)) = fields.first() {
                    self.compile_expr(first_expr)?;
                } else {
                    self.emit_text("xor rax, rax");
                }
            }
            Expr::Jit { .. } => {
                self.emit_text("xor rax, rax");
            }
            Expr::Try(expr, _) => {
                self.compile_expr(expr)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_x86_codegen_arithmetic_and_run() {
        let src = r#"
extern fn printf(fmt: *u8, ...) -> i32;

fn add(a: i64, b: i64) -> i64 {
    return a + b;
}

fn fib(n: i64) -> i64 {
    if n <= 1 {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

fn main() -> i32 {
    let s = add(10, 32);
    let f = fib(10);
    printf("s=%lld, fib=%lld\n", s, f);
    return 0;
}
"#;
        let mut diags = crate::diag::Diags::new("test_native.gw", src);
        let toks = crate::lexer::Lexer::new(src).tokenize(&mut diags);
        assert!(!diags.has_errors());
        let prog = crate::parser::Parser::new(toks, src, &mut diags).parse_program();
        assert!(!diags.has_errors());

        let obj_bytes = compile_program_to_obj(&prog, "test_native.gw").expect("native compile to obj");
        assert!(!obj_bytes.is_empty());

        if cfg!(windows) {
            let pe_bytes = crate::linker::link_coff_to_pe(&[&obj_bytes], Some("main"))
                .expect("link native obj to pe");
            assert!(!pe_bytes.is_empty());

            let test_exe = std::env::temp_dir().join("test_native_x86.exe");
            std::fs::write(&test_exe, &pe_bytes).expect("write exe");

            let output = std::process::Command::new(&test_exe).output().expect("run native exe");
            let _ = std::fs::remove_file(&test_exe);

            assert!(output.status.success());
            let out_str = String::from_utf8_lossy(&output.stdout);
            assert!(out_str.contains("s=42, fib=55"), "unexpected output: {out_str}");
        }
    }
}

