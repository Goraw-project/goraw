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

pub fn transpile(prog: &Program, test_mode: bool, line_map: &[(u32, String)]) -> Result<String, String> {
    let mut tr = Transpiler::new(test_mode, line_map);
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
    field_struct: HashMap<String, String>,
}

#[derive(Clone, Debug)]
struct FnMeta {
    is_ret_pointer: bool,
    ret_struct_name: Option<String>,
    first_param_is_ptr: bool,
}

struct Transpiler {
    out: String,
    indent_level: usize,
    test_mode: bool,
    in_test: bool,
    line_map: Vec<(u32, String)>,
    scopes: Vec<HashMap<String, VarMeta>>,
    struct_defs: HashMap<String, StructMeta>,
    fns: HashMap<String, FnMeta>,
    used_math: HashSet<String>,
    needs_str: bool,
    needs_slice: bool,
}

impl Transpiler {
    fn new(test_mode: bool, line_map: &[(u32, String)]) -> Self {
        Self {
            out: String::with_capacity(32 * 1024),
            indent_level: 0,
            test_mode,
            in_test: false,
            line_map: line_map.to_vec(),
            scopes: Vec::new(),
            struct_defs: HashMap::new(),
            fns: HashMap::new(),
            used_math: HashSet::new(),
            needs_str: false,
            needs_slice: false,
        }
    }

    fn locate_line(&self, line: u32) -> u32 {
        if self.line_map.is_empty() {
            return line;
        }
        let mut best = &self.line_map[0];
        for e in &self.line_map {
            if e.0 <= line {
                best = e;
            } else {
                break;
            }
        }
        line.saturating_sub(best.0) + 1
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
                if field == "ptr" {
                    return true;
                }
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

    fn extract_struct_name(ty: &TypeExpr) -> Option<String> {
        match ty {
            TypeExpr::Named(name, ..) => {
                if matches!(
                    name.as_str(),
                    "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "f32" | "f64" | "bool" | "void" | "str"
                ) {
                    None
                } else {
                    Some(name.clone())
                }
            }
            TypeExpr::Ptr(inner, ..) | TypeExpr::PtrMut(inner, ..) => Self::extract_struct_name(inner),
            TypeExpr::Slice(inner, ..) | TypeExpr::Array(inner, ..) => Self::extract_struct_name(inner),
            _ => None,
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
            Expr::Unary { op, expr, .. } => match op {
                UnOp::Deref | UnOp::Ref | UnOp::RefMut => self.infer_struct_name(expr),
                _ => None,
            },
            Expr::Field { base, field, .. } => {
                if let Some(sname) = self.infer_struct_name(base) {
                    if let Some(sm) = self.struct_defs.get(&sname) {
                        return sm.field_struct.get(field).cloned();
                    }
                }
                None
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
            let mut field_struct = HashMap::new();
            for f in &s.fields {
                field_names.push(f.name.clone());
                let is_p = matches!(&f.ty, TypeExpr::Ptr(..) | TypeExpr::PtrMut(..));
                field_is_ptr.insert(f.name.clone(), is_p);
                if let Some(st) = Self::extract_struct_name(&f.ty) {
                    field_struct.insert(f.name.clone(), st);
                }
            }
            self.struct_defs.insert(s.name.clone(), StructMeta { field_names, field_is_ptr, field_struct });
        }

        // Сбор метаданных функций
        for f in &prog.fns {
            let is_p = f.ret.as_ref().map_or(false, |r| matches!(r, TypeExpr::Ptr(..) | TypeExpr::PtrMut(..)));
            let ret_s = f.ret.as_ref().and_then(|r| Self::extract_struct_name(r));
            let first_ptr = f.params.first().map_or(false, |p| {
                matches!(&p.ty, TypeExpr::Ptr(..) | TypeExpr::PtrMut(..))
            });
            let meta = FnMeta { is_ret_pointer: is_p, ret_struct_name: ret_s, first_param_is_ptr: first_ptr };
            self.fns.insert(f.name.clone(), meta.clone());
            self.fns.insert(escape_fn_name(&f.name), meta);
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
             #include <chrono>\n\
             #include <array>\n\
             #include <span>\n\
             #include <string_view>\n\
             #include <string>\n\
             #include <iostream>\n\
             #include <type_traits>\n\
             #include <cmath>\n\n\
             // Переносимый замер времени (строго в миллисекундах на всех ОС)\n\
             inline int64_t gw_clock_ms() noexcept {\n\
                 return std::chrono::duration_cast<std::chrono::milliseconds>(\n\
                     std::chrono::steady_clock::now().time_since_epoch()\n\
                 ).count();\n\
             }\n\n\
             inline void* alloc(int64_t sz) noexcept { return std::malloc(sz); }\n\
             inline void* realloc(void* p, int64_t sz) noexcept { return std::realloc(p, sz); }\n\n\
             struct GwZeroInit {\n\
                 template <typename U>\n\
                 constexpr operator U() const noexcept { return U{}; }\n\
             };\n\
             constexpr GwZeroInit zeroed() noexcept { return {}; }\n\n\
             struct GorawStr {\n\
                 const char* ptr{\"\"};\n\
                 int64_t len{0};\n\
                 constexpr GorawStr() = default;\n\
                 constexpr GorawStr(const char* s) : ptr(s ? s : \"\"), len(s ? (int64_t)std::string_view(s).size() : 0) {}\n\
                 constexpr GorawStr(const char* p, int64_t l) : ptr(p ? p : \"\"), len(l) {}\n\
                 constexpr int64_t size() const noexcept { return len; }\n\
                 constexpr int64_t length() const noexcept { return len; }\n\
                 constexpr bool empty() const noexcept { return len == 0; }\n\
                 constexpr bool is_empty() const noexcept { return len == 0; }\n\
                 constexpr std::string_view view() const noexcept { return {ptr, (size_t)len}; }\n\
                 operator std::string_view() const noexcept { return view(); }\n\
                 const char* c_str() const noexcept { return ptr; }\n\
                 bool starts_with(std::string_view prefix) const noexcept { return view().starts_with(prefix); }\n\
                 bool ends_with(std::string_view suffix) const noexcept { return view().ends_with(suffix); }\n\
                 uint8_t operator[](int64_t idx) const noexcept { return static_cast<uint8_t>(ptr[idx]); }\n\
                 GorawStr clone() const {\n\
                     if (len <= 0) return GorawStr();\n\
                     char* buf = static_cast<char*>(std::malloc(len + 1));\n\
                     std::memcpy(buf, ptr, len);\n\
                     buf[len] = '\\0';\n\
                     return GorawStr(buf, len);\n\
                 }\n\
                 friend bool operator==(const GorawStr& a, const GorawStr& b) noexcept { return a.view() == b.view(); }\n\
                 friend bool operator!=(const GorawStr& a, const GorawStr& b) noexcept { return a.view() != b.view(); }\n\
                 friend bool operator<(const GorawStr& a, const GorawStr& b) noexcept { return a.view() < b.view(); }\n\
                 friend std::ostream& operator<<(std::ostream& os, const GorawStr& s) { return os.write(s.ptr, s.len); }\n\
                 friend GorawStr operator+(const GorawStr& a, const GorawStr& b) {\n\
                     int64_t total = a.len + b.len;\n\
                     char* buf = static_cast<char*>(std::malloc(total + 1));\n\
                     if (a.len > 0) std::memcpy(buf, a.ptr, a.len);\n\
                     if (b.len > 0) std::memcpy(buf + a.len, b.ptr, b.len);\n\
                     buf[total] = '\\0';\n\
                     return GorawStr(buf, total);\n\
                 }\n\
             };\n\n\
             inline GorawStr str_from_cstr(const char* s) noexcept { return GorawStr(s); }\n\
             inline GorawStr str_from_cstr(const uint8_t* s) noexcept { return GorawStr(reinterpret_cast<const char*>(s)); }\n\n\
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
                 bool empty() const noexcept { return len == 0; }\n\
             };\n\n\
             template <typename T>\n\
             inline GorawSlice<T> make_slice(T* ptr, int64_t len) noexcept { return GorawSlice<T>(ptr, len); }\n\n\
             template <typename T>\n\
             inline GorawSlice<T> gw_slice(GorawSlice<T> s, int64_t start, int64_t end) noexcept {\n\
                 if (start < 0) start = 0;\n\
                 if (end < 0 || end > s.len) end = s.len;\n\
                 if (start > end) start = end;\n\
                 return GorawSlice<T>(s.ptr + start, end - start);\n\
             }\n\
             template <typename T, size_t N>\n\
             inline GorawSlice<T> gw_slice(std::array<T, N>& arr, int64_t start, int64_t end) noexcept {\n\
                 int64_t len = (int64_t)N;\n\
                 if (start < 0) start = 0;\n\
                 if (end < 0 || end > len) end = len;\n\
                 if (start > end) start = end;\n\
                 return GorawSlice<T>(arr.data() + start, end - start);\n\
             }\n\
             template <typename T, size_t N>\n\
             inline GorawSlice<const T> gw_slice(const std::array<T, N>& arr, int64_t start, int64_t end) noexcept {\n\
                 int64_t len = (int64_t)N;\n\
                 if (start < 0) start = 0;\n\
                 if (end < 0 || end > len) end = len;\n\
                 if (start > end) start = end;\n\
                 return GorawSlice<const T>(arr.data() + start, end - start);\n\
             }\n\
             template <typename T>\n\
             inline GorawSlice<T> gw_slice(T* ptr, int64_t start, int64_t end) noexcept {\n\
                 if (start < 0) start = 0;\n\
                 int64_t len = (end >= start) ? (end - start) : 0;\n\
                 return GorawSlice<T>(ptr + start, len);\n\
             }\n\
             inline GorawStr gw_slice(const GorawStr& s, int64_t start, int64_t end) noexcept {\n\
                 if (start < 0) start = 0;\n\
                 if (end < 0 || end > s.len) end = s.len;\n\
                 if (start > end) start = end;\n\
                 return GorawStr(s.ptr + start, end - start);\n\
             }\n\n\
             template <typename T>\n\
             constexpr auto gw_len(const T& x) noexcept {\n\
                 if constexpr (requires { x.len; }) {\n\
                     return x.len;\n\
                 } else if constexpr (requires { x.size(); }) {\n\
                     return static_cast<int64_t>(x.size());\n\
                 } else {\n\
                     return x.len;\n\
                 }\n\
             }\n\n\
             template <typename T>\n\
             constexpr auto gw_ptr(T& x) noexcept {\n\
                 if constexpr (requires { x.ptr; }) {\n\
                     return x.ptr;\n\
                 } else if constexpr (requires { x.data(); }) {\n\
                     return x.data();\n\
                 } else {\n\
                     return x.ptr;\n\
                 }\n\
             }\n\n\
             template <typename T, size_t N>\n\
             struct GwArray : std::array<T, N> {\n\
                 template <typename U>\n\
                 constexpr operator std::array<U, N>() const {\n\
                     return [&]<size_t... Is>(std::index_sequence<Is...>) {\n\
                         return std::array<U, N>{ static_cast<U>((*this)[Is])... };\n\
                     }(std::make_index_sequence<N>{});\n\
                 }\n\
             };\n\n\
             template <typename... Ts>\n\
             constexpr auto gw_make_array(Ts&&... args) {\n\
                 if constexpr (sizeof...(Ts) == 0) {\n\
                     return GwArray<int64_t, 0>{};\n\
                 } else {\n\
                     using CommonType = std::common_type_t<std::decay_t<Ts>...>;\n\
                     return GwArray<CommonType, sizeof...(Ts)>{ { static_cast<CommonType>(std::forward<Ts>(args))... } };\n\
                 }\n\
             };\n\n\
             template <typename A, typename B>\n\
             constexpr auto gw_add(A&& a, B&& b) {\n\
                 if constexpr (std::is_convertible_v<A, std::string_view> && std::is_convertible_v<B, std::string_view>) {\n\
                     return GorawStr(a) + GorawStr(b);\n\
                 } else {\n\
                     return std::forward<A>(a) + std::forward<B>(b);\n\
                 }\n\
             }\n\n\
             template <typename A, typename B>\n\
             constexpr bool gw_eq(const A& a, const B& b) {\n\
                 if constexpr (std::is_convertible_v<A, std::string_view> && std::is_convertible_v<B, std::string_view>) {\n\
                     return std::string_view(a) == std::string_view(b);\n\
                 } else {\n\
                     return a == b;\n\
                 }\n\
             }\n\n\
             template <typename A, typename B>\n\
             constexpr bool gw_ne(const A& a, const B& b) {\n\
                 if constexpr (std::is_convertible_v<A, std::string_view> && std::is_convertible_v<B, std::string_view>) {\n\
                     return std::string_view(a) != std::string_view(b);\n\
                 } else {\n\
                     return a != b;\n\
                 }\n\
             }\n\n\
             template <typename T>\n\
             inline void gw_print_val(std::ostream& os, const T& val) {\n\
                 if constexpr (std::is_same_v<std::decay_t<T>, bool>) {\n\
                     os << (val ? \"true\" : \"false\");\n\
                 } else {\n\
                     os << val;\n\
                 }\n\
             }\n\
             template <typename... Args>\n\
             inline void print(Args&&... args) {\n\
                 ((gw_print_val(std::cout, std::forward<Args>(args))), ...);\n\
             }\n\
             template <typename... Args>\n\
             inline void println(Args&&... args) {\n\
                 ((gw_print_val(std::cout, std::forward<Args>(args))), ...);\n\
                 std::cout << '\\n';\n\
             }\n\
             inline void panic(const char* msg = \"panic\") {\n\
                 std::cout << std::flush;\n\
                 std::fprintf(stderr, \"[GORAW PANIC] %s\\n\", msg);\n\
                 std::abort();\n\
             }\n\
             inline void panic(GorawStr msg) {\n\
                 std::cout << std::flush;\n\
                 std::fprintf(stderr, \"[GORAW PANIC] %.*s\\n\", (int)msg.len, msg.ptr);\n\
                 std::abort();\n\
             }\n\
             inline void goraw_panic(const char* msg) noexcept { panic(msg); }\n\n\
             template <typename T>\n\
             inline auto gw_try(T&& val) { return std::forward<T>(val); }\n"
        );

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
                self.write_line(&format!("enum {} : int32_t {{", escape_ident(&e.name)));
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
            self.write_line("namespace contracts { void run_all_contracts(int64_t& __p, int64_t& __f); }");
        }
        if has_integration_tests {
            self.write_line("namespace integration_tests { void run_all_tests(int64_t& __p, int64_t& __f); }");
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
            let ret_s = if f.name == "main" {
                "int".to_string()
            } else {
                match &f.ret {
                    Some(t) => self.transpile_type(t),
                    None => "void".to_string(),
                }
            };
            let mut params_s = Vec::new();
            self.scopes.clear();
            self.scopes.push(HashMap::new());

            for p in &f.params {
                let is_p = matches!(&p.ty, TypeExpr::Ptr(..) | TypeExpr::PtrMut(..));
                let sname = Self::extract_struct_name(&p.ty);
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
            let has_trailing_return = f.body.as_ref().map_or(false, |b| {
                b.stmts.last().map_or(false, |s| matches!(s, Stmt::Return(..)))
            });
            if f.name == "main" && !has_trailing_return {
                self.write_line("return 0;");
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
                let mut used_names = HashSet::new();
                let mut contracts_meta = Vec::new();
                for t in prog.tests.iter().filter(|t| t.is_shadow) {
                    let clean = sanitize_test_name(&t.name);
                    let mut fn_name = format!("contract_{clean}");
                    let mut idx = 1;
                    while !used_names.insert(fn_name.clone()) {
                        idx += 1;
                        fn_name = format!("contract_{clean}_{idx}");
                    }
                    contracts_meta.push((fn_name.clone(), t.name.clone()));
                    self.write_line(&format!("inline int64_t {}() {{", fn_name));
                    self.indent_level += 1;
                    self.scopes.clear();
                    self.scopes.push(HashMap::new());
                    self.in_test = true;
                    self.transpile_block(&t.body);
                    self.in_test = false;
                    self.write_line("return 0;");
                    self.indent_level -= 1;
                    self.write_line("}\n");
                }

                self.write_line("inline void run_all_contracts(int64_t& __p, int64_t& __f) {");
                self.indent_level += 1;
                self.write_line("std::printf(\"\\n--- Shadow Contracts ---\\n\");");
                for (fn_name, orig_name) in contracts_meta {
                    let escaped_name = orig_name.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%");
                    self.write_line(&format!("int64_t r_{fn_name} = {fn_name}();"));
                    self.write_line(&format!("if (r_{fn_name} != 0) {{"));
                    self.indent_level += 1;
                    self.write_line(&format!("std::printf(\"[CONTRACT FAIL] {escaped_name} (line %lld)\\n\", (long long)r_{fn_name});"));
                    self.write_line("__f += 1;");
                    self.indent_level -= 1;
                    self.write_line("} else {");
                    self.indent_level += 1;
                    self.write_line(&format!("std::printf(\"[CONTRACT ok] {escaped_name}\\n\");"));
                    self.write_line("__p += 1;");
                    self.indent_level -= 1;
                    self.write_line("}");
                }
                self.indent_level -= 1;
                self.write_line("}\n");
                self.indent_level -= 1;
                self.write_line("} // namespace contracts\n");
            }

            if has_integration_tests {
                self.write_line("namespace integration_tests {");
                self.indent_level += 1;
                let mut used_names = HashSet::new();
                let mut tests_meta = Vec::new();
                for t in prog.tests.iter().filter(|t| !t.is_shadow) {
                    let clean = sanitize_test_name(&t.name);
                    let mut fn_name = format!("test_{clean}");
                    let mut idx = 1;
                    while !used_names.insert(fn_name.clone()) {
                        idx += 1;
                        fn_name = format!("test_{clean}_{idx}");
                    }
                    tests_meta.push((fn_name.clone(), t.name.clone()));
                    self.write_line(&format!("inline int64_t {}() {{", fn_name));
                    self.indent_level += 1;
                    self.scopes.clear();
                    self.scopes.push(HashMap::new());
                    self.in_test = true;
                    self.transpile_block(&t.body);
                    self.in_test = false;
                    self.write_line("return 0;");
                    self.indent_level -= 1;
                    self.write_line("}\n");
                }

                self.write_line("inline void run_all_tests(int64_t& __p, int64_t& __f) {");
                self.indent_level += 1;
                self.write_line("std::printf(\"\\n--- Integration Tests ---\\n\");");
                for (fn_name, orig_name) in tests_meta {
                    let escaped_name = orig_name.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%");
                    self.write_line(&format!("int64_t r_{fn_name} = {fn_name}();"));
                    self.write_line(&format!("if (r_{fn_name} != 0) {{"));
                    self.indent_level += 1;
                    self.write_line(&format!("std::printf(\"[TEST FAIL] {escaped_name} (line %lld)\\n\", (long long)r_{fn_name});"));
                    self.write_line("__f += 1;");
                    self.indent_level -= 1;
                    self.write_line("} else {");
                    self.indent_level += 1;
                    self.write_line(&format!("std::printf(\"[TEST ok] {escaped_name}\\n\");"));
                    self.write_line("__p += 1;");
                    self.indent_level -= 1;
                    self.write_line("}");
                }
                self.indent_level -= 1;
                self.write_line("}\n");
                self.indent_level -= 1;
                self.write_line("} // namespace integration_tests\n");
            }

            self.write_line("inline int32_t run_all_goraw_tests() {");
            self.indent_level += 1;
            self.write_line("int64_t __p = 0;");
            self.write_line("int64_t __f = 0;");
            self.write_line("std::printf(\"============================================================\\n\");");
            self.write_line("std::printf(\"  Running Goraw Contracts & Tests in C++23                 \\n\");");
            self.write_line("std::printf(\"============================================================\\n\");");
            if has_contracts {
                self.write_line("contracts::run_all_contracts(__p, __f);");
            }
            if has_integration_tests {
                self.write_line("integration_tests::run_all_tests(__p, __f);");
            }
            let total = prog.tests.len();
            self.write_line(&format!(
                "std::printf(\"\\n[C++23] All {total} tests finished: %lld passed, %lld failed\\n\\n\", (long long)__p, (long long)__f);"
            ));
            self.write_line("return static_cast<int32_t>(__f);");
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
            TypeExpr::Ptr(inner, _) => {
                if let TypeExpr::Named(name, _) = &**inner {
                    if name == "u8" || name == "i8" {
                        return "const char*".into();
                    }
                }
                format!("const {}*", self.transpile_type(inner))
            }
            TypeExpr::PtrMut(inner, _) => {
                if let TypeExpr::Named(name, _) = &**inner {
                    if name == "u8" || name == "i8" {
                        return "char*".into();
                    }
                }
                format!("{}*", self.transpile_type(inner))
            }
            TypeExpr::Fn(params, ret, _) => {
                let ret_s = match ret {
                    Some(r) => self.transpile_type(r),
                    None => "void".into(),
                };
                let ps: Vec<String> = params.iter().map(|p| self.transpile_type(p)).collect();
                format!("std::add_pointer_t<{}({})>", ret_s, ps.join(", "))
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
                let sname = ty.as_ref().and_then(|t| Self::extract_struct_name(t)).or_else(|| self.infer_struct_name(value));
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
                        if self.in_test {
                            self.write_line("return 0;");
                        } else {
                            self.write_line("return;");
                        }
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
            Stmt::Assert(expr, span) => {
                let e_s = self.transpile_expr(expr);
                let local_line = self.locate_line(span.lo.line);
                self.write_line(&format!("if (!({e_s})) return {local_line};"));
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
                let l_s = self.transpile_expr(lhs);
                let r_s = self.transpile_expr(rhs);
                let is_ptr_cmp = self.is_expr_pointer(lhs) || self.is_expr_pointer(rhs);
                match op {
                    BinOp::Add if !is_ptr_cmp => format!("gw_add({l_s}, {r_s})"),
                    BinOp::Eq if !is_ptr_cmp => format!("gw_eq({l_s}, {r_s})"),
                    BinOp::Ne if !is_ptr_cmp => format!("gw_ne({l_s}, {r_s})"),
                    _ => {
                        let op_s = match op {
                            BinOp::Add => "+",
                            BinOp::Eq => "==",
                            BinOp::Ne => "!=",
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
                            BinOp::Lt => "<",
                            BinOp::Le => "<=",
                            BinOp::Gt => ">",
                            BinOp::Ge => ">=",
                        };
                        format!("({l_s} {op_s} {r_s})")
                    }
                }
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
                    Expr::Ident(name, _) if (name == "clock" || name == "gw_clock_ms") && args.is_empty() => {
                        "gw_clock_ms()".to_string()
                    }
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
                                "bool" => "bool".to_string(),
                                other => escape_ident(other),
                            },
                            other => self.transpile_expr(other),
                        };
                        format!("sizeof({})", arg_s)
                    }
                    Expr::Field { base, field, .. } => {
                        if matches!(field.as_str(), "starts_with" | "ends_with" | "is_empty" | "clone") {
                            let b_s = match &**base {
                                Expr::Str(..) => format!("GorawStr({})", self.transpile_expr(base)),
                                _ => self.transpile_expr(base),
                            };
                            format!("{b_s}.{}({})", escape_ident(field), joined)
                        } else if let Some(sname) = self.infer_struct_name(base) {
                            let mangled = format!("{sname}__{field}");
                            let alt = format!("{sname}::{field}");
                            if let Some(fm) = self.fns.get(&mangled).or_else(|| self.fns.get(&alt)) {
                                let first_arg = if fm.first_param_is_ptr {
                                    if self.is_expr_pointer(base) {
                                        self.transpile_expr(base)
                                    } else {
                                        format!("&({})", self.transpile_expr(base))
                                    }
                                } else {
                                    if self.is_expr_pointer(base) {
                                        format!("*({})", self.transpile_expr(base))
                                    } else {
                                        self.transpile_expr(base)
                                    }
                                };
                                let all_args = if joined.is_empty() {
                                    first_arg
                                } else {
                                    format!("{first_arg}, {joined}")
                                };
                                format!("{mangled}({all_args})")
                            } else {
                                let op = if self.is_expr_pointer(base) { "->" } else { "." };
                                format!("{}{}{}({})", self.transpile_expr(base), op, escape_ident(field), joined)
                            }
                        } else {
                            let op = if self.is_expr_pointer(base) { "->" } else { "." };
                            format!("{}{}{}({})", self.transpile_expr(base), op, escape_ident(field), joined)
                        }
                    }
                    _ => {
                        let c_s = self.transpile_expr(callee);
                        format!("{}({})", c_s, joined)
                    }
                }
            }
            Expr::Field { base, field, .. } => {
                let op = if self.is_expr_pointer(base) { "->" } else { "." };
                if field == "len" && op == "." {
                    format!("gw_len({})", self.transpile_expr(base))
                } else if field == "ptr" && op == "." {
                    format!("gw_ptr({})", self.transpile_expr(base))
                } else {
                    format!("{}{}{}", self.transpile_expr(base), op, escape_ident(field))
                }
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
                if elems.is_empty() {
                    "gw_make_array()".to_string()
                } else {
                    let el_s: Vec<String> = elems.iter().map(|e| self.transpile_expr(e)).collect();
                    format!("gw_make_array({})", el_s.join(", "))
                }
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
        | "protected" | "public" | "co_await" | "co_yield" | "co_return" | "auto"
        | "int" | "long" | "short" | "char" | "double" | "float" | "register"
        | "volatile" | "friend" | "namespace" | "using" | "typedef" | "inline"
        | "explicit" | "export" | "mutable" | "static_assert" | "thread_local"
        | "alignas" | "alignof" | "decltype" | "constexpr" | "consteval" | "constinit"
        | "char8_t" | "char16_t" | "char32_t" | "wchar_t" | "bool" | "void"
        | "goto" | "case" | "switch" => {
            format!("gw_{name}")
        }
        _ => name.to_string(),
    }
}

/// Имена функций с :: (методы Goraw) транслируются в `Struct__method`
fn escape_fn_name(name: &str) -> String {
    let parts: Vec<String> = name.split("::").map(escape_ident).collect();
    parts.join("__")
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

