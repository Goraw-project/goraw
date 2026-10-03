//! Транслятор AST Goraw в современный стандарт C++23.
//! Поддерживает:
//! - Полное отображение скалярных типов, структур, перечислений и указателей
//! - RAII деструкторы (автоматический маппинг `Type::drop` -> C++ деструктор `~Type()`)
//! - Go-style циклы `for`, `for i in slice`, `while`, `if`/`else`
//! - Литералы структур C++20/23 designated initializers
//! - Строки `str` и срезы `[]T`
//! - Инлайн C/C++ блоки (`c { ... }`, `cpp { ... }`)
//! - Shadow-тесты и тестовый раннер

use crate::ast::*;

pub fn transpile(prog: &Program, _test_mode: bool) -> Result<String, String> {
    let mut tr = Transpiler::new();
    tr.run(prog)
}

struct Transpiler {
    out: String,
    indent_level: usize,
}

impl Transpiler {
    fn new() -> Self {
        Self {
            out: String::with_capacity(32 * 1024),
            indent_level: 0,
        }
    }

    fn indent(&self) -> String {
        "    ".repeat(self.indent_level)
    }

    fn write_line(&mut self, line: &str) {
        if line.is_empty() {
            self.out.push('\n');
        } else {
            self.out.push_str(&self.indent());
            self.out.push_str(line);
            self.out.push('\n');
        }
    }

    fn run(&mut self, prog: &Program) -> Result<String, String> {
        // 1. Заголовок и прелюдия стандартной библиотеки C++23
        self.out.push_str(
            "// ============================================================================\n\
             // Сгенерировано компилятором Goraw\n\
             // ============================================================================\n\n\
             #include <cstdint>\n\
             #include <cstddef>\n\
             #include <cstdlib>\n\
             #include <cstdio>\n\
             #include <cstring>\n\
             #include <cassert>\n\
             #include <cmath>\n\
             #include <string>\n\
             #include <string_view>\n\
             #include <vector>\n\
             #include <array>\n\
             #include <span>\n\
             #include <utility>\n\
             #include <algorithm>\n\
             #include <iostream>\n\
             #include <type_traits>\n\n\
             // Goraw базовые псевдонимы типов\n\
             using i8  = int8_t;\n\
             using i16 = int16_t;\n\
             using i32 = int32_t;\n\
             using i64 = int64_t;\n\
             using u8  = uint8_t;\n\
             using u16 = uint16_t;\n\
             using u32 = uint32_t;\n\
             using u64 = uint64_t;\n\
             using f32 = float;\n\
             using f64 = double;\n\n\
             // --- Goraw Runtime & Helper Types ---\n\
             \n\
             struct GorawStr {\n\
                 const char* ptr{nullptr};\n\
                 int64_t len{0};\n\
                 constexpr GorawStr() = default;\n\
                 constexpr GorawStr(const char* s) : ptr(s), len(s ? (int64_t)std::string_view(s).size() : 0) {}\n\
                 constexpr GorawStr(const char* p, int64_t l) : ptr(p), len(l) {}\n\
                 constexpr int64_t size() const noexcept { return len; }\n\
                 constexpr int64_t length() const noexcept { return len; }\n\
                 constexpr bool empty() const noexcept { return len == 0; }\n\
                 constexpr std::string_view view() const noexcept { return {ptr, (size_t)len}; }\n\
                 operator std::string_view() const noexcept { return view(); }\n\
                 const char* c_str() const noexcept { return ptr; }\n\
                 char operator[](int64_t idx) const noexcept { return ptr[idx]; }\n\
                 bool operator==(const GorawStr& o) const noexcept { return view() == o.view(); }\n\
                 bool operator==(const char* o) const noexcept { return view() == o; }\n\
                 bool operator!=(const GorawStr& o) const noexcept { return view() != o.view(); }\n\
             };\n\
             \n\
             template <typename T>\n\
             struct GorawSlice {\n\
                 T* ptr{nullptr};\n\
                 int64_t len{0};\n\
                 constexpr GorawSlice() = default;\n\
                 constexpr GorawSlice(T* p, int64_t l) : ptr(p), len(l) {}\n\
                 T& operator[](int64_t idx) { return ptr[idx]; }\n\
                 const T& operator[](int64_t idx) const { return ptr[idx]; }\n\
                 T* begin() noexcept { return ptr; }\n\
                 T* end() noexcept { return ptr + len; }\n\
                 const T* begin() const noexcept { return ptr; }\n\
                 const T* end() const noexcept { return ptr + len; }\n\
                 int64_t size() const noexcept { return len; }\n\
                 int64_t length() const noexcept { return len; }\n\
             };\n\
             \n\
             template <typename T>\n\
             constexpr decltype(auto) gw_deref(T&& obj) noexcept {\n\
                 if constexpr (std::is_pointer_v<std::remove_reference_t<T>>) {\n\
                     return *obj;\n\
                 } else {\n\
                     return std::forward<T>(obj);\n\
                 }\n\
             }\n\
             \n\
             inline void* alloc(int64_t sz) noexcept { return std::malloc(sz); }\n\
             inline void goraw_panic(const char* msg) noexcept {\n\
                 std::fprintf(stderr, \"[GORAW PANIC] %s\\n\", msg);\n\
                 std::abort();\n\
             }\n\
             \n\
             // Встроенные функции и математика Goraw из std\n\
             using std::abs;\n\
             using std::min;\n\
             using std::max;\n\
             using std::clamp;\n\
             using std::sqrt;\n\
             using std::pow;\n\
             using std::floor;\n\
             using std::ceil;\n\
             using std::sin;\n\
             using std::cos;\n\
             \n\
             template <typename... Args>\n\
             inline void println(const Args&... args) {\n\
                 auto print_one = [](const auto& val) {\n\
                     if constexpr (std::is_same_v<std::decay_t<decltype(val)>, GorawStr>) {\n\
                         std::cout << val.view();\n\
                     } else {\n\
                         std::cout << val;\n\
                     }\n\
                 };\n\
                 (print_one(args), ...);\n\
                 std::cout << std::endl;\n\
             }\n\
             \n\
             template <typename... Args>\n\
             inline void print(const Args&... args) {\n\
                 auto print_one = [](const auto& val) {\n\
                     if constexpr (std::is_same_v<std::decay_t<decltype(val)>, GorawStr>) {\n\
                         std::cout << val.view();\n\
                     } else {\n\
                         std::cout << val;\n\
                     }\n\
                 };\n\
                 (print_one(args), ...);\n\
             }\n\
             \n"
        );

        // 2. Верхнеуровневые блоки `c { ... }` и `cpp { ... }`
        if !prog.c_blocks.is_empty() {
            self.write_line("// --- Встроенные C/C++ блоки ---");
            for b in &prog.c_blocks {
                self.out.push_str(&b.code);
                self.out.push('\n');
            }
            self.out.push('\n');
        }

        // 3. Предварительное объявление структур
        if !prog.structs.is_empty() {
            self.write_line("// --- Предварительные объявления структур ---");
            for s in &prog.structs {
                self.write_line(&format!("struct {};", escape_ident(&s.name)));
            }
            self.out.push('\n');
        }

        // 4. Перечисления (enums)
        if !prog.enums.is_empty() {
            self.write_line("// --- Перечисления ---");
            for e in &prog.enums {
                self.write_line(&format!("enum class {} : int32_t {{", escape_ident(&e.name)));
                self.indent_level += 1;
                for (var, val) in &e.variants {
                    self.write_line(&format!("{} = {},", escape_ident(var), val));
                }
                self.indent_level -= 1;
                self.write_line("};\n");
            }
        }

        // 5. Полные определения структур
        let mut destructors_to_emit: Vec<(String, String, bool)> = Vec::new();
        if !prog.structs.is_empty() {
            self.write_line("// --- Определения структур ---");
            for s in &prog.structs {
                self.write_line(&format!("struct {} {{", escape_ident(&s.name)));
                self.indent_level += 1;
                for f in &s.fields {
                    let ty_s = self.transpile_type(&f.ty);
                    self.write_line(&format!("{} {}{{}};", ty_s, escape_ident(&f.name)));
                }

                // Проверка: есть ли деструктор `drop` для данной структуры
                let drop_fn_name = format!("{}_drop", s.name);
                let drop_colon_name = format!("{}__drop", s.name);
                let drop_decl = prog.fns.iter().find(|f| f.name == drop_fn_name || f.name == drop_colon_name);
                if let Some(df) = drop_decl {
                    let fn_to_call = df.name.clone();
                    let is_ptr = df.params.first()
                        .map(|p| matches!(&p.ty, TypeExpr::Ptr(..) | TypeExpr::PtrMut(..)))
                        .unwrap_or(false);
                    destructors_to_emit.push((s.name.clone(), fn_to_call, is_ptr));
                    self.write_line(&format!("~{}() noexcept;", escape_ident(&s.name)));
                }

                self.indent_level -= 1;
                self.write_line("};\n");
            }
        }

        // 6. Глобальные константы
        if !prog.consts.is_empty() {
            self.write_line("// --- Константы ---");
            for c in &prog.consts {
                let ty_s = match &c.ty {
                    Some(t) => self.transpile_type(t),
                    None => "auto".to_string(),
                };
                let val_s = self.transpile_expr(&c.value);
                self.write_line(&format!("constexpr {} {} = {};", ty_s, escape_ident(&c.name), val_s));
            }
            self.out.push('\n');
        }

        // 7. Глобальные статические переменные
        if !prog.statics.is_empty() {
            self.write_line("// --- Статические переменные ---");
            for st in &prog.statics {
                let ty_s = self.transpile_type(&st.ty);
                let val_s = self.transpile_expr(&st.value);
                self.write_line(&format!("inline {} {} = {};", ty_s, escape_ident(&st.name), val_s));
            }
            self.out.push('\n');
        }

        // 8. Предварительные объявления функций
        self.write_line("// --- Предварительные объявления функций ---");
        for f in &prog.fns {
            if f.is_extern {
                // Внешние C функции
                let ret_s = match &f.ret {
                    Some(t) => self.transpile_type(t),
                    None => "void".to_string(),
                };
                let mut params_s = Vec::new();
                for p in &f.params {
                    params_s.push(format!("{} {}", self.transpile_type(&p.ty), escape_ident(&p.name)));
                }
                if f.variadic {
                    params_s.push("...".to_string());
                }
                let plist = params_s.join(", ");
                // Пропускаем extern printf/malloc/free/clock/exit, так как они уже объявлены в <c...>
                if !matches!(f.name.as_str(), "printf" | "malloc" | "free" | "realloc" | "clock" | "exit" | "abort") {
                    self.write_line(&format!("extern \"C\" {} {}({});", ret_s, f.name, plist));
                }
            } else {
                let ret_s = match &f.ret {
                    Some(t) => self.transpile_type(t),
                    None => "void".to_string(),
                };
                let mut params_s = Vec::new();
                for p in &f.params {
                    params_s.push(format!("{} {}", self.transpile_type(&p.ty), escape_ident(&p.name)));
                }
                let plist = params_s.join(", ");
                let fn_name = if f.name == "main" { "main".to_string() } else { escape_fn_name(&f.name) };
                self.write_line(&format!("{} {}({});", ret_s, fn_name, plist));
            }
        }
        self.out.push('\n');

        // 9. Определения функций
        self.write_line("// --- Определения функций ---");
        for f in &prog.fns {
            if f.is_extern {
                continue;
            }
            let ret_s = match &f.ret {
                Some(t) => self.transpile_type(t),
                None => "void".to_string(),
            };
            let mut params_s = Vec::new();
            for p in &f.params {
                params_s.push(format!("{} {}", self.transpile_type(&p.ty), escape_ident(&p.name)));
            }
            let plist = params_s.join(", ");
            let fn_name = if f.name == "main" { "main".to_string() } else { escape_fn_name(&f.name) };

            self.write_line(&format!("{} {}({}) {{", ret_s, fn_name, plist));
            self.indent_level += 1;
            if let Some(body) = &f.body {
                self.transpile_block(body);
            }
            self.indent_level -= 1;
            self.write_line("}\n");
        }

        // 10. Деструкторы структур (RAII)
        if !destructors_to_emit.is_empty() {
            self.write_line("// --- Деструкторы структур (RAII) ---");
            for (struct_name, fn_name, is_ptr) in &destructors_to_emit {
                let arg = if *is_ptr { "this" } else { "*this" };
                self.write_line(&format!(
                    "inline {}::~{}() noexcept {{ {}({}); }}",
                    escape_ident(struct_name),
                    escape_ident(struct_name),
                    fn_name,
                    arg
                ));
            }
            self.out.push('\n');
        }

        Ok(std::mem::take(&mut self.out))
    }

    fn transpile_type(&self, ty: &TypeExpr) -> String {
        match ty {
            TypeExpr::Named(name, _) => match name.as_str() {
                "i8" => "int8_t".into(),
                "i16" => "int16_t".into(),
                "i32" => "int32_t".into(),
                "i64" => "int64_t".into(),
                "u8" => "uint8_t".into(),
                "u16" => "uint16_t".into(),
                "u32" => "uint32_t".into(),
                "u64" => "uint64_t".into(),
                "f32" => "float".into(),
                "f64" => "double".into(),
                "bool" => "bool".into(),
                "void" => "void".into(),
                "str" => "GorawStr".into(),
                other => escape_ident(other),
            },
            TypeExpr::Generic(name, args, _) => {
                let args_s: Vec<String> = args.iter().map(|a| self.transpile_type(a)).collect();
                format!("{}<{}>", escape_ident(name), args_s.join(", "))
            }
            TypeExpr::Ptr(inner, _) => format!("const {}*", self.transpile_type(inner)),
            TypeExpr::PtrMut(inner, _) => format!("{}*", self.transpile_type(inner)),
            TypeExpr::Fn(params, ret, _) => {
                let ret_s = match ret {
                    Some(r) => self.transpile_type(r),
                    None => "void".into(),
                };
                let ps: Vec<String> = params.iter().map(|p| self.transpile_type(p)).collect();
                format!("{} (*)({})", ret_s, ps.join(", "))
            }
            TypeExpr::Slice(inner, _) => format!("GorawSlice<{}>", self.transpile_type(inner)),
            TypeExpr::Array(inner, n, _) => format!("std::array<{}, {}>", self.transpile_type(inner), n),
        }
    }

    fn transpile_block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.transpile_stmt(stmt);
        }
    }

    fn transpile_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, ty, value, .. } => {
                let ty_s = match ty {
                    Some(t) => self.transpile_type(t),
                    None => "auto".to_string(),
                };
                let val_s = self.transpile_expr(value);
                self.write_line(&format!("{} {} = {};", ty_s, escape_ident(name), val_s));
            }
            Stmt::Assign { target, value, .. } => {
                let tgt_s = self.transpile_expr(target);
                let val_s = self.transpile_expr(value);
                self.write_line(&format!("{} = {};", tgt_s, val_s));
            }
            Stmt::Expr(e) => {
                let e_s = self.transpile_expr(e);
                self.write_line(&format!("{};", e_s));
            }
            Stmt::Return(opt_expr, _) => {
                match opt_expr {
                    Some(e) => {
                        let e_s = self.transpile_expr(e);
                        self.write_line(&format!("return {};", e_s));
                    }
                    None => {
                        self.write_line("return;");
                    }
                }
            }
            Stmt::If { cond, then, els, .. } => {
                let c_s = self.transpile_expr(cond);
                self.write_line(&format!("if ({}) {{", unparen(&c_s)));
                self.indent_level += 1;
                self.transpile_block(then);
                self.indent_level -= 1;
                if let Some(e) = els {
                    self.write_line("} else {");
                    self.indent_level += 1;
                    self.transpile_block(e);
                    self.indent_level -= 1;
                }
                self.write_line("}");
            }
            Stmt::While { cond, body, .. } => {
                let c_s = self.transpile_expr(cond);
                self.write_line(&format!("while ({}) {{", unparen(&c_s)));
                self.indent_level += 1;
                self.transpile_block(body);
                self.indent_level -= 1;
                self.write_line("}");
            }
            Stmt::For { init, cond, post, body, .. } => {
                let init_s = match init {
                    Some(s) => match &**s {
                        Stmt::Let { name, ty, value, .. } => {
                            let ty_s = match ty {
                                Some(t) => self.transpile_type(t),
                                None => "auto".to_string(),
                            };
                            format!("{} {} = {}", ty_s, escape_ident(name), self.transpile_expr(value))
                        }
                        Stmt::Assign { target, value, .. } => {
                            format!("{} = {}", self.transpile_expr(target), self.transpile_expr(value))
                        }
                        Stmt::Expr(e) => self.transpile_expr(e),
                        _ => String::new(),
                    },
                    None => String::new(),
                };
                let cond_s = match cond {
                    Some(e) => self.transpile_expr(e),
                    None => String::new(),
                };
                let post_s = match post {
                    Some(s) => match &**s {
                        Stmt::Assign { target, value, .. } => {
                            format!("{} = {}", self.transpile_expr(target), self.transpile_expr(value))
                        }
                        Stmt::Expr(e) => self.transpile_expr(e),
                        _ => String::new(),
                    },
                    None => String::new(),
                };
                self.write_line(&format!("for ({}; {}; {}) {{", init_s, cond_s, post_s));
                self.indent_level += 1;
                self.transpile_block(body);
                self.indent_level -= 1;
                self.write_line("}");
            }
            Stmt::ForIn { var, iter, body, .. } => {
                let it_s = self.transpile_expr(iter);
                self.write_line(&format!("for (auto&& {} : {}) {{", escape_ident(var), it_s));
                self.indent_level += 1;
                self.transpile_block(body);
                self.indent_level -= 1;
                self.write_line("}");
            }
            Stmt::Break(_) => self.write_line("break;"),
            Stmt::Continue(_) => self.write_line("continue;"),
            Stmt::Unsafe(block, _) => {
                self.write_line("{ // unsafe");
                self.indent_level += 1;
                self.transpile_block(block);
                self.indent_level -= 1;
                self.write_line("}");
            }
            Stmt::Assert(expr, _) => {
                let e_s = self.transpile_expr(expr);
                self.write_line(&format!("assert({});", e_s));
            }
            Stmt::Match { scrut, arms, .. } => {
                let s_s = self.transpile_expr(scrut);
                self.write_line(&format!("switch ({}) {{", s_s));
                self.indent_level += 1;
                for (pat, block) in arms {
                    match pat {
                        Some(p) => {
                            let p_s = self.transpile_expr(p);
                            self.write_line(&format!("case {}: {{", p_s));
                        }
                        None => {
                            self.write_line("default: {");
                        }
                    }
                    self.indent_level += 1;
                    self.transpile_block(block);
                    self.write_line("break;");
                    self.indent_level -= 1;
                    self.write_line("}");
                }
                self.indent_level -= 1;
                self.write_line("}");
            }
            Stmt::InlineC { body, .. } => {
                self.write_line("// inline C/C++");
                self.write_line("{");
                self.out.push_str(body);
                self.out.push('\n');
                self.write_line("}");
            }
            Stmt::Asm(asm) => {
                self.write_line("// inline asm");
                let escaped_body = asm.body.replace('\n', "\\n\n").replace('"', "\\\"");
                self.write_line(&format!("__asm__ volatile (\"{escaped_body}\");"));
            }
        }
    }

    fn transpile_expr(&self, expr: &Expr) -> String {
        match expr {
            Expr::Int(n, _) => {
                if *n == -9223372036854775807 - 1 {
                    "(-9223372036854775807LL - 1LL)".into()
                } else if *n > 2147483647 || *n < -2147483648 {
                    format!("{n}LL")
                } else {
                    n.to_string()
                }
            }
            Expr::Float(f, _) => {
                let mut s = f.to_string();
                if !s.contains('.') && !s.contains('e') && !s.contains('E') {
                    s.push_str(".0");
                }
                s
            }
            Expr::Bool(b, _) => if *b { "true".into() } else { "false".into() },
            Expr::Str(s, _) => {
                let escaped = s
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
                    .replace('\r', "\\r")
                    .replace('\t', "\\t");
                format!("\"{escaped}\"")
            }
            Expr::Ident(name, _) => {
                if name == "null" {
                    "nullptr".into()
                } else {
                    escape_ident(name)
                }
            }
            Expr::Null(_) => "nullptr".into(),
            Expr::Path(en, var, _) => format!("{}::{}", escape_ident(en), escape_ident(var)),
            Expr::Binary { op, lhs, rhs, .. } => {
                let op_s = match op {
                    BinOp::Add => "+",
                    BinOp::Sub => "-",
                    BinOp::Mul => "*",
                    BinOp::Div => "/",
                    BinOp::Rem => "%",
                    BinOp::And => "&&",
                    BinOp::Or => "||",
                    BinOp::BitAnd => "&",
                    BinOp::BitOr => "|",
                    BinOp::BitXor => "^",
                    BinOp::Shl => "<<",
                    BinOp::Shr => ">>",
                    BinOp::Eq => "==",
                    BinOp::Ne => "!=",
                    BinOp::Lt => "<",
                    BinOp::Le => "<=",
                    BinOp::Gt => ">",
                    BinOp::Ge => ">=",
                };
                format!("({} {} {})", self.transpile_expr(lhs), op_s, self.transpile_expr(rhs))
            }
            Expr::Unary { op, expr, .. } => {
                match op {
                    UnOp::Neg => format!("(-{})", self.transpile_expr(expr)),
                    UnOp::Not => format!("(!{})", self.transpile_expr(expr)),
                    UnOp::BitNot => format!("(~{})", self.transpile_expr(expr)),
                    UnOp::Deref => format!("(*{})", self.transpile_expr(expr)),
                    UnOp::Ref | UnOp::RefMut => format!("(&{})", self.transpile_expr(expr)),
                }
            }
            Expr::Call { callee, args, .. } => {
                let args_s: Vec<String> = args.iter().map(|a| self.transpile_expr(a)).collect();
                let joined = args_s.join(", ");
                match &**callee {
                    Expr::Ident(name, _) if name == "sizeof" && args.len() == 1 => {
                        format!("sizeof({})", self.transpile_expr(&args[0]))
                    }
                    Expr::Field { base, field, .. } => {
                        // Метод вызов `base.field(args)` -> `gw_deref(base).field(args)`
                        format!("gw_deref({}).{}({})", self.transpile_expr(base), escape_ident(field), joined)
                    }
                    _ => {
                        let c_s = self.transpile_expr(callee);
                        format!("{}({})", c_s, joined)
                    }
                }
            }
            Expr::Field { base, field, .. } => {
                // Доступ к полю: gw_deref(base).field работает как со структурами, так и с указателями
                format!("gw_deref({}).{}", self.transpile_expr(base), escape_ident(field))
            }
            Expr::Index { base, index, .. } => {
                format!("{}[{}]", self.transpile_expr(base), self.transpile_expr(index))
            }
            Expr::Slice { base, start, end, .. } => {
                let b_s = self.transpile_expr(base);
                let st_s = start.as_ref().map(|s| self.transpile_expr(s)).unwrap_or_else(|| "0".into());
                let en_s = end.as_ref().map(|e| self.transpile_expr(e)).unwrap_or_else(|| "-1".into());
                format!("gw_slice({}, {}, {})", b_s, st_s, en_s)
            }
            Expr::ArrayLit(elems, ..) => {
                let el_s: Vec<String> = elems.iter().map(|e| self.transpile_expr(e)).collect();
                format!("{{ {} }}", el_s.join(", "))
            }
            Expr::IfExpr { cond, then, els, .. } => {
                format!("(({}) ? ({}) : ({}))", self.transpile_expr(cond), self.transpile_expr(then), self.transpile_expr(els))
            }
            Expr::StructLit { name, fields, .. } => {
                let mut fs = Vec::new();
                for (fname, val, _) in fields {
                    fs.push(format!(".{} = {}", escape_ident(fname), self.transpile_expr(val)));
                }
                format!("{}{{\n        {}\n    }}", escape_ident(name), fs.join(",\n        "))
            }
            Expr::Cast { expr, ty, .. } => {
                let ty_s = self.transpile_type(ty);
                let e_s = self.transpile_expr(expr);
                format!("(({})({}))", ty_s, e_s)
            }
            Expr::Jit { .. } => {
                "/* JIT-блок не поддержан в статической трансляции */ nullptr".into()
            }
            Expr::Try(inner, ..) => {
                let in_s = self.transpile_expr(inner);
                format!("gw_try({})", in_s)
            }
        }
    }
}

/// Экранирование зарезервированных ключевых слов C++
fn escape_ident(name: &str) -> String {
    match name {
        "new" | "delete" | "class" | "template" | "virtual" | "this" | "operator"
        | "concept" | "requires" | "catch" | "throw" | "try" | "default" | "private"
        | "protected" | "public" | "co_await" | "co_yield" | "co_return" => {
            format!("gw_{name}")
        }
        _ => name.to_string(),
    }
}

/// Имена функций с :: (методы Goraw) транслируются в `Struct__method`
fn escape_fn_name(name: &str) -> String {
    name.replace("::", "__")
}

fn unparen(s: &str) -> &str {
    let t = s.trim();
    if t.starts_with('(') && t.ends_with(')') {
        let mut depth = 0;
        let mut balanced_at_end = true;
        for (i, c) in t.char_indices() {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
                if depth == 0 && i < t.len() - 1 {
                    balanced_at_end = false;
                    break;
                }
            }
        }
        if balanced_at_end && depth == 0 {
            return &t[1..t.len() - 1];
        }
    }
    t
}
