//! Транслятор AST Goraw в современный стандарт C++23.
//! Поддерживает:
//! - Полное отображение скалярных типов, структур, перечислений и указателей
//! - RAII деструкторы (автоматический маппинг `Type::drop` -> C++ деструктор `~Type()`) с соблюдением "Правила пяти"
//! - Чистый C++ синтаксис доступа к полям `.` и `->` без оверхеда `gw_deref`
//! - Конструкторы типов вместо designated initializers для валидности по C++20 (P1008R1)
//! - Прямой вызов libc `clock()` без подмены FFI сигнатур
//! - Полное сохранение и прогон shadow-тестов и контрактов через флаг `--test` / `#ifdef GORAW_TEST`
//! - Минимальные заголовки без замусоривания неиспользуемыми библиотеками

use crate::ast::*;
use std::collections::{HashMap, HashSet};

pub fn transpile(prog: &Program, test_mode: bool) -> Result<String, String> {
    let mut tr = Transpiler::new(test_mode);
    tr.run(prog)
}

#[derive(Clone, Debug)]
struct VarMeta {
    is_pointer: bool,
    struct_name: Option<String>,
}

#[derive(Clone, Debug)]
struct StructMeta {
    field_names: Vec<String>,
    field_is_ptr: HashMap<String, bool>,
}

#[derive(Clone, Debug)]
struct FnMeta {
    is_ret_pointer: bool,
    ret_struct_name: Option<String>,
}

struct Transpiler {
    out: String,
    indent_level: usize,
    test_mode: bool,
    scopes: Vec<HashMap<String, VarMeta>>,
    struct_defs: HashMap<String, StructMeta>,
    fns: HashMap<String, FnMeta>,
    used_math: HashSet<String>,
    needs_str: bool,
    needs_slice: bool,
}

impl Transpiler {
    fn new(test_mode: bool) -> Self {
        Self {
            out: String::with_capacity(32 * 1024),
            indent_level: 0,
            test_mode,
            scopes: Vec::new(),
            struct_defs: HashMap::new(),
            fns: HashMap::new(),
            used_math: HashSet::new(),
            needs_str: false,
            needs_slice: false,
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

    fn insert_var(&mut self, name: &str, meta: VarMeta) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), meta);
        }
    }

    fn lookup_var(&self, name: &str) -> Option<&VarMeta> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(v);
            }
        }
        None
    }

    fn is_expr_pointer(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Index { .. } => false, // pts[i] возвращает значение/ссылку на структуру, не указатель
            Expr::Ident(name, _) => {
                self.lookup_var(name).map(|v| v.is_pointer).unwrap_or(false)
            }
            Expr::Unary { op, expr, .. } => match op {
                UnOp::Ref | UnOp::RefMut => true,
                UnOp::Deref => false,
                _ => self.is_expr_pointer(expr),
            },
            Expr::Call { callee, .. } => {
                if let Expr::Ident(name, _) = &**callee {
                    if name == "alloc" || name == "malloc" || name == "realloc" {
                        return true;
                    }
                    if let Some(fm) = self.fns.get(name) {
                        return fm.is_ret_pointer;
                    }
                }
                false
            }
            Expr::Field { base, field, .. } => {
                if let Some(sname) = self.infer_struct_name(base) {
                    if let Some(sm) = self.struct_defs.get(&sname) {
                        return sm.field_is_ptr.get(field).copied().unwrap_or(false);
                    }
                }
                false
            }
            Expr::Cast { ty, .. } => {
                matches!(ty, TypeExpr::Ptr(..) | TypeExpr::PtrMut(..))
            }
            _ => false,
        }
    }

    fn infer_struct_name(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::StructLit { name, .. } => Some(name.clone()),
            Expr::Call { callee, .. } => {
                if let Expr::Ident(name, _) = &**callee {
                    if let Some(fm) = self.fns.get(name) {
                        return fm.ret_struct_name.clone();
                    }
                }
                None
            }
            Expr::Index { base, .. } => {
                if let Expr::Ident(name, _) = &**base {
                    if let Some(v) = self.lookup_var(name) {
                        return v.struct_name.clone();
                    }
                }
                None
            }
            Expr::Ident(name, _) => {
                self.lookup_var(name).and_then(|v| v.struct_name.clone())
            }
            _ => None,
        }
    }

    fn scan_usage(&mut self, prog: &Program) {
        for f in &prog.fns {
            if let Some(body) = &f.body {
                self.scan_block(body);
            }
        }
        for t in &prog.tests {
            self.scan_block(&t.body);
        }
    }

    fn scan_block(&mut self, block: &Block) {
        for s in &block.stmts {
            self.scan_stmt(s);
        }
    }

    fn scan_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { ty, value, .. } => {
                if let Some(t) = ty {
                    self.scan_type(t);
                }
                self.scan_expr(value);
            }
            Stmt::Assign { target, value, .. } => {
                self.scan_expr(target);
                self.scan_expr(value);
            }
            Stmt::Expr(e) | Stmt::Assert(e, _) => self.scan_expr(e),
            Stmt::Return(opt_e, _) => {
                if let Some(e) = opt_e {
                    self.scan_expr(e);
                }
            }
            Stmt::If { cond, then, els, .. } => {
                self.scan_expr(cond);
                self.scan_block(then);
                if let Some(b) = els {
                    self.scan_block(b);
                }
            }
            Stmt::While { cond, body, .. } => {
                self.scan_expr(cond);
                self.scan_block(body);
            }
            Stmt::For { init, cond, post, body, .. } => {
                if let Some(i) = init {
                    self.scan_stmt(i);
                }
                if let Some(c) = cond {
                    self.scan_expr(c);
                }
                if let Some(p) = post {
                    self.scan_stmt(p);
                }
                self.scan_block(body);
            }
            Stmt::ForIn { iter, body, .. } => {
                self.needs_slice = true;
                self.scan_expr(iter);
                self.scan_block(body);
            }
            Stmt::Unsafe(b, _) => self.scan_block(b),
            Stmt::Match { scrut, arms, .. } => {
                self.scan_expr(scrut);
                for (p, b) in arms {
                    if let Some(e) = p {
                        self.scan_expr(e);
                    }
                    self.scan_block(b);
                }
            }
            _ => {}
        }
    }

    fn scan_type(&mut self, ty: &TypeExpr) {
        match ty {
            TypeExpr::Named(name, _) if name == "str" => self.needs_str = true,
            TypeExpr::Slice(inner, _) => {
                self.needs_slice = true;
                self.scan_type(inner);
            }
            TypeExpr::Ptr(inner, _) | TypeExpr::PtrMut(inner, _) => self.scan_type(inner),
            _ => {}
        }
    }

    fn scan_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Call { callee, args, .. } => {
                if let Expr::Ident(name, _) = &**callee {
                    match name.as_str() {
                        "abs" | "min" | "max" | "clamp" | "sqrt" | "pow" | "floor" | "ceil" | "sin" | "cos" => {
                            self.used_math.insert(name.clone());
                        }
                        _ => {}
                    }
                }
                self.scan_expr(callee);
                for a in args {
                    self.scan_expr(a);
                }
            }
            Expr::Binary { lhs, rhs, .. } => {
                self.scan_expr(lhs);
                self.scan_expr(rhs);
            }
            Expr::Unary { expr, .. } => self.scan_expr(expr),
            Expr::Field { base, .. } => self.scan_expr(base),
            Expr::Index { base, index, .. } => {
                self.scan_expr(base);
                self.scan_expr(index);
            }
            Expr::Slice { base, start, end, .. } => {
                self.needs_slice = true;
                self.scan_expr(base);
                if let Some(s) = start { self.scan_expr(s); }
                if let Some(e) = end { self.scan_expr(e); }
            }
            Expr::ArrayLit(elems, ..) => {
                for e in elems {
                    self.scan_expr(e);
                }
            }
            Expr::IfExpr { cond, then, els, .. } => {
                self.scan_expr(cond);
                self.scan_expr(then);
                self.scan_expr(els);
            }
            Expr::StructLit { fields, .. } => {
                for (_, v, _) in fields {
                    self.scan_expr(v);
                }
            }
            Expr::Cast { expr, ty, .. } => {
                self.scan_type(ty);
                self.scan_expr(expr);
            }
            _ => {}
        }
    }

    fn run(&mut self, prog: &Program) -> Result<String, String> {
        // Сбор метаданных структур
        for s in &prog.structs {
            let mut field_names = Vec::new();
            let mut field_is_ptr = HashMap::new();
            for f in &s.fields {
                field_names.push(f.name.clone());
                let is_p = matches!(&f.ty, TypeExpr::Ptr(..) | TypeExpr::PtrMut(..));
                field_is_ptr.insert(f.name.clone(), is_p);
            }
            self.struct_defs.insert(s.name.clone(), StructMeta { field_names, field_is_ptr });
        }

        // Сбор метаданных функций
        for f in &prog.fns {
            let is_p = f.ret.as_ref().map_or(false, |r| matches!(r, TypeExpr::Ptr(..) | TypeExpr::PtrMut(..)));
            let ret_s = f.ret.as_ref().and_then(|r| match r {
                TypeExpr::Named(name, ..) => Some(name.clone()),
                _ => None,
            });
            self.fns.insert(f.name.clone(), FnMeta { is_ret_pointer: is_p, ret_struct_name: ret_s });
        }

        self.scan_usage(prog);

        // 1. Преамбула
        self.out.push_str(
            "// ============================================================================\n\
             // Сгенерировано компилятором Goraw (C++23 Backend)\n\
             // ============================================================================\n\n\
             #include <cstdint>\n\
             #include <cstddef>\n\
             #include <cstdlib>\n\
             #include <cstdio>\n\
             #include <cstring>\n\
             #include <cassert>\n\
             #include <ctime>\n"
        );

        if !self.used_math.is_empty() {
            self.out.push_str("#include <cmath>\n");
        }
        if self.needs_str {
            self.out.push_str("#include <string_view>\n");
        }
        if self.needs_slice {
            self.out.push_str("#include <span>\n");
        }

        self.out.push_str(
            "\ninline void* alloc(int64_t sz) noexcept { return std::malloc(sz); }\n\
             inline void goraw_panic(const char* msg) noexcept {\n\
                 std::fprintf(stderr, \"[GORAW PANIC] %s\\n\", msg);\n\
                 std::abort();\n\
             }\n"
        );

        if self.needs_str {
            self.out.push_str(
                "\nstruct GorawStr {\n\
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
                 };\n"
            );
        }

        if self.needs_slice {
            self.out.push_str(
                "\ntemplate <typename T>\n\
                 struct GorawSlice {\n\
                     T* ptr{nullptr};\n\
                     int64_t len{0};\n\
                     constexpr GorawSlice() = default;\n\
                     constexpr GorawSlice(T* p, int64_t l) : ptr(p), len(l) {}\n\
                     T& operator[](int64_t idx) { return ptr[idx]; }\n\
                     const T& operator[](int64_t idx) const { return ptr[idx]; }\n\
                     T* begin() noexcept { return ptr; }\n\
                     T* end() noexcept { return ptr + len; }\n\
                     int64_t size() const noexcept { return len; }\n\
                 };\n"
            );
        }

        if !self.used_math.is_empty() {
            self.out.push('\n');
            let mut sorted_math: Vec<_> = self.used_math.iter().collect();
            sorted_math.sort();
            for m in sorted_math {
                self.out.push_str(&format!("using std::{m};\n"));
            }
        }
        self.out.push('\n');

        // 2. Встроенные блоки c { } / cpp { }
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
        let mut destructors_to_emit: Vec<(String, String, bool, Vec<Param>)> = Vec::new();
        if !prog.structs.is_empty() {
            self.write_line("// --- Определения структур ---");
            for s in &prog.structs {
                self.write_line(&format!("struct {} {{", escape_ident(&s.name)));
                self.indent_level += 1;
                for f in &s.fields {
                    let ty_s = self.transpile_type(&f.ty);
                    self.write_line(&format!("{} {}{{}};", ty_s, escape_ident(&f.name)));
                }
                self.write_line("");
                self.write_line(&format!("constexpr {}() = default;", escape_ident(&s.name)));
                if !s.fields.is_empty() {
                    let params: Vec<String> = s.fields.iter().map(|f| format!("{} {}", self.transpile_type(&f.ty), escape_ident(&f.name))).collect();
                    let inits: Vec<String> = s.fields.iter().map(|f| format!("{}({})", escape_ident(&f.name), escape_ident(&f.name))).collect();
                    self.write_line(&format!("constexpr {}({}) : {} {{}}", escape_ident(&s.name), params.join(", "), inits.join(", ")));
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
                    destructors_to_emit.push((s.name.clone(), fn_to_call, is_ptr, s.fields.clone()));
                    self.write_line(&format!("~{}() noexcept;", escape_ident(&s.name)));

                    // Правило пяти (Rule of 5)
                    self.write_line(&format!("{}(const {}&) = delete;", escape_ident(&s.name), escape_ident(&s.name)));
                    self.write_line(&format!("{}& operator=(const {}&) = delete;", escape_ident(&s.name), escape_ident(&s.name)));

                    let move_inits: Vec<String> = s.fields.iter().map(|f| format!("{}(other.{})", escape_ident(&f.name), escape_ident(&f.name))).collect();
                    self.write_line(&format!("constexpr {}({}&& other) noexcept : {} {{", escape_ident(&s.name), escape_ident(&s.name), move_inits.join(", ")));
                    self.indent_level += 1;
                    for f in &s.fields {
                        if matches!(&f.ty, TypeExpr::Ptr(..) | TypeExpr::PtrMut(..)) {
                            self.write_line(&format!("other.{} = nullptr;", escape_ident(&f.name)));
                        }
                    }
                    self.indent_level -= 1;
                    self.write_line("}");

                    self.write_line(&format!("{}& operator=({}&& other) noexcept;", escape_ident(&s.name), escape_ident(&s.name)));
                }

                self.indent_level -= 1;
                self.write_line("};\n");
            }
        }

        // 6. Константы
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

        // 7. Статические переменные
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
                if !matches!(f.name.as_str(), "printf" | "malloc" | "free" | "realloc" | "clock" | "exit" | "abort") {
                    self.write_line(&format!("extern \"C\" {} {}({});", ret_s, f.name, plist));
                }
            } else {
                if f.name == "main" {
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
                let fn_name = escape_fn_name(&f.name);
                self.write_line(&format!("{} {}({});", ret_s, fn_name, plist));
            }
        }
        let has_contracts = prog.tests.iter().any(|t| t.is_shadow);
        let has_integration_tests = prog.tests.iter().any(|t| !t.is_shadow);
        if has_contracts {
            self.write_line("namespace contracts { void run_all_contracts(); }");
        }
        if has_integration_tests {
            self.write_line("namespace integration_tests { void run_all_tests(); }");
        }
        if !prog.tests.is_empty() {
            self.write_line("int32_t run_all_goraw_tests();");
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
            self.scopes.clear();
            self.scopes.push(HashMap::new());

            for p in &f.params {
                let is_p = matches!(&p.ty, TypeExpr::Ptr(..) | TypeExpr::PtrMut(..));
                let sname = match &p.ty {
                    TypeExpr::Named(n, ..) => Some(n.clone()),
                    _ => None,
                };
                self.insert_var(&p.name, VarMeta { is_pointer: is_p, struct_name: sname });
                params_s.push(format!("{} {}", self.transpile_type(&p.ty), escape_ident(&p.name)));
            }
            let plist = if f.name == "main" && f.params.is_empty() {
                "int argc, char** argv".to_string()
            } else {
                params_s.join(", ")
            };
            let fn_name = if f.name == "main" { "main".to_string() } else { escape_fn_name(&f.name) };

            self.write_line(&format!("{} {}({}) {{", ret_s, fn_name, plist));
            self.indent_level += 1;

            if f.name == "main" && !prog.tests.is_empty() {
                if self.test_mode {
                    self.write_line("return run_all_goraw_tests();");
                } else {
                    self.write_line("#ifdef GORAW_TEST");
                    self.write_line("return run_all_goraw_tests();");
                    self.write_line("#else");
                    self.write_line("if (argc > 1 && (std::strcmp(argv[1], \"--test\") == 0 || std::strcmp(argv[1], \"-t\") == 0)) {");
                    self.write_line("    return run_all_goraw_tests();");
                    self.write_line("}");
                    self.write_line("#endif");
                }
            }

            if let Some(body) = &f.body {
                self.transpile_block(body);
            }
            self.indent_level -= 1;
            self.write_line("}\n");
        }

        // 10. Деструкторы и операторы перемещения структур (RAII)
        if !destructors_to_emit.is_empty() {
            self.write_line("// --- Деструкторы и операторы перемещения структур (RAII) ---");
            for (struct_name, fn_name, is_ptr, fields) in &destructors_to_emit {
                let s_id = escape_ident(struct_name);
                let drop_arg = if *is_ptr { "this" } else { "*this" };
                self.write_line(&format!(
                    "inline {}::~{}() noexcept {{\n    {}({});\n}}",
                    s_id,
                    s_id,
                    escape_fn_name(fn_name),
                    drop_arg
                ));

                self.write_line(&format!("inline {}& {}::operator=({}&& other) noexcept {{", s_id, s_id, s_id));
                self.indent_level += 1;
                self.write_line("if (this != &other) {");
                self.indent_level += 1;
                self.write_line(&format!("{}({});", escape_fn_name(fn_name), drop_arg));
                for f in fields {
                    self.write_line(&format!("{} = other.{};", escape_ident(&f.name), escape_ident(&f.name)));
                    if matches!(&f.ty, TypeExpr::Ptr(..) | TypeExpr::PtrMut(..)) {
                        self.write_line(&format!("other.{} = nullptr;", escape_ident(&f.name)));
                    }
                }
                self.indent_level -= 1;
                self.write_line("}");
                self.write_line("return *this;");
                self.indent_level -= 1;
                self.write_line("}\n");
            }
            self.out.push('\n');
        }

        // 11. Shadow-контракты и интеграционные тесты
        if !prog.tests.is_empty() {
            self.write_line("// --- Shadow-контракты и интеграционные тесты ---");
            if has_contracts {
                self.write_line("namespace contracts {");
                self.indent_level += 1;
                for t in prog.tests.iter().filter(|t| t.is_shadow) {
                    let clean_name = sanitize_test_name(&t.name);
                    self.write_line(&format!("inline void contract_{}() {{", clean_name));
                    self.indent_level += 1;
                    self.scopes.clear();
                    self.scopes.push(HashMap::new());
                    self.transpile_block(&t.body);
                    self.indent_level -= 1;
                    self.write_line("}\n");
                }

                self.write_line("inline void run_all_contracts() {");
                self.indent_level += 1;
                self.write_line("std::printf(\"\\n--- Shadow Contracts ---\\n\");");
                for t in prog.tests.iter().filter(|t| t.is_shadow) {
                    let clean_name = sanitize_test_name(&t.name);
                    let escaped_name = t.name.replace('\\', "\\\\").replace('"', "\\\"");
                    self.write_line(&format!("contract_{}();", clean_name));
                    self.write_line(&format!("std::printf(\"[CONTRACT ok] {}\\n\");", escaped_name));
                }
                self.indent_level -= 1;
                self.write_line("}\n");
                self.indent_level -= 1;
                self.write_line("} // namespace contracts\n");
            }

            if has_integration_tests {
                self.write_line("namespace integration_tests {");
                self.indent_level += 1;
                for t in prog.tests.iter().filter(|t| !t.is_shadow) {
                    let clean_name = sanitize_test_name(&t.name);
                    self.write_line(&format!("inline void test_{}() {{", clean_name));
                    self.indent_level += 1;
                    self.scopes.clear();
                    self.scopes.push(HashMap::new());
                    self.transpile_block(&t.body);
                    self.indent_level -= 1;
                    self.write_line("}\n");
                }

                self.write_line("inline void run_all_tests() {");
                self.indent_level += 1;
                self.write_line("std::printf(\"\\n--- Integration Tests ---\\n\");");
                for t in prog.tests.iter().filter(|t| !t.is_shadow) {
                    let clean_name = sanitize_test_name(&t.name);
                    let escaped_name = t.name.replace('\\', "\\\\").replace('"', "\\\"");
                    self.write_line(&format!("test_{}();", clean_name));
                    self.write_line(&format!("std::printf(\"[TEST ok] {}\\n\");", escaped_name));
                }
                self.indent_level -= 1;
                self.write_line("}\n");
                self.indent_level -= 1;
                self.write_line("} // namespace integration_tests\n");
            }

            self.write_line("inline int32_t run_all_goraw_tests() {");
            self.indent_level += 1;
            self.write_line("std::printf(\"============================================================\\n\");");
            self.write_line("std::printf(\"  Running Goraw Contracts & Tests in C++23                 \\n\");");
            self.write_line("std::printf(\"============================================================\\n\");");
            if has_contracts {
                self.write_line("contracts::run_all_contracts();");
            }
            if has_integration_tests {
                self.write_line("integration_tests::run_all_tests();");
            }
            self.write_line(&format!(
                "std::printf(\"\\n[C++23] All {} tests PASSED successfully!\\n\\n\");",
                prog.tests.len()
            ));
            self.write_line("return 0;");
            self.indent_level -= 1;
            self.write_line("}\n");
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
        self.scopes.push(HashMap::new());
        for stmt in &block.stmts {
            self.transpile_stmt(stmt);
        }
        self.scopes.pop();
    }

    fn transpile_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, ty, value, .. } => {
                let is_ptr = match ty {
                    Some(t) => matches!(t, TypeExpr::Ptr(..) | TypeExpr::PtrMut(..)),
                    None => self.is_expr_pointer(value),
                };
                let sname = match ty {
                    Some(TypeExpr::Named(n, ..)) => Some(n.clone()),
                    _ => self.infer_struct_name(value),
                };
                self.insert_var(name, VarMeta { is_pointer: is_ptr, struct_name: sname });

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
                self.scopes.push(HashMap::new());
                let init_s = match init {
                    Some(s) => match &**s {
                        Stmt::Let { name, ty, value, .. } => {
                            let is_ptr = match ty {
                                Some(t) => matches!(t, TypeExpr::Ptr(..) | TypeExpr::PtrMut(..)),
                                None => self.is_expr_pointer(value),
                            };
                            let sname = match ty {
                                Some(TypeExpr::Named(n, ..)) => Some(n.clone()),
                                _ => self.infer_struct_name(value),
                            };
                            self.insert_var(name, VarMeta { is_pointer: is_ptr, struct_name: sname });
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
                for b_stmt in &body.stmts {
                    self.transpile_stmt(b_stmt);
                }
                self.indent_level -= 1;
                self.write_line("}");
                self.scopes.pop();
            }
            Stmt::ForIn { var, iter, body, .. } => {
                self.scopes.push(HashMap::new());
                self.insert_var(var, VarMeta { is_pointer: false, struct_name: None });
                let it_s = self.transpile_expr(iter);
                self.write_line(&format!("for (auto&& {} : {}) {{", escape_ident(var), it_s));
                self.indent_level += 1;
                for b_stmt in &body.stmts {
                    self.transpile_stmt(b_stmt);
                }
                self.indent_level -= 1;
                self.write_line("}");
                self.scopes.pop();
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
                        let arg_s = match &args[0] {
                            Expr::Ident(tname, _) => match tname.as_str() {
                                "i8" => "int8_t".to_string(),
                                "i16" => "int16_t".to_string(),
                                "i32" => "int32_t".to_string(),
                                "i64" => "int64_t".to_string(),
                                "u8" => "uint8_t".to_string(),
                                "u16" => "uint16_t".to_string(),
                                "u32" => "uint32_t".to_string(),
                                "u64" => "uint64_t".to_string(),
                                "f32" => "float".to_string(),
                                "f64" => "double".to_string(),
                                other => escape_ident(other),
                            },
                            other => self.transpile_expr(other),
                        };
                        format!("sizeof({})", arg_s)
                    }
                    Expr::Field { base, field, .. } => {
                        let op = if self.is_expr_pointer(base) { "->" } else { "." };
                        format!("{}{}{}({})", self.transpile_expr(base), op, escape_ident(field), joined)
                    }
                    _ => {
                        let c_s = self.transpile_expr(callee);
                        format!("{}({})", c_s, joined)
                    }
                }
            }
            Expr::Field { base, field, .. } => {
                let op = if self.is_expr_pointer(base) { "->" } else { "." };
                format!("{}{}{}", self.transpile_expr(base), op, escape_ident(field))
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
                // Вызов параметризованного конструктора StructName(val1, val2, ...) вместо designated initializers
                if let Some(sm) = self.struct_defs.get(name) {
                    let mut ordered_vals = Vec::new();
                    for fname in &sm.field_names {
                        if let Some((_, val, _)) = fields.iter().find(|(fn_name, ..)| fn_name == fname) {
                            ordered_vals.push(self.transpile_expr(val));
                        } else {
                            ordered_vals.push("{}".to_string());
                        }
                    }
                    format!("{}({})", escape_ident(name), ordered_vals.join(", "))
                } else {
                    let mut vals = Vec::new();
                    for (_, val, _) in fields {
                        vals.push(self.transpile_expr(val));
                    }
                    format!("{}({})", escape_ident(name), vals.join(", "))
                }
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

/// Санитизация имени теста в валидный идентификатор C++ и LLVM IR
pub fn sanitize_test_name(name: &str) -> String {
    let mut res = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            res.push(c);
        } else {
            res.push('_');
        }
    }
    while res.contains("__") {
        res = res.replace("__", "_");
    }
    let res = res.trim_matches('_');
    if res.is_empty() {
        "unnamed".to_string()
    } else if res.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        format!("t_{res}")
    } else {
        res.to_string()
    }
}

