//! Транслятор из C++23 обратно в Goraw.
//!
//! Использует Clang AST JSON dump (`clang++ -std=c++23 -Xclang -ast-dump=json`)
//! для синтаксического и семантического анализа исходного кода на C++23
//! и преобразует типы, структуры, классы, методы, функции, глобальные переменные,
//! управляющие конструкции (циклы, условия, ветвления) и выражения в идиоматичный Goraw код.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use serde_json::Value;

#[derive(Clone, Debug, Default)]
pub struct CppToGorawOptions {
    pub clang_path: Option<String>,
    pub cpp_std: Option<String>,
    pub target_triple: Option<String>,
    pub extra_includes: Vec<PathBuf>,
}

/// Контекст трансляции из C++ AST в Goraw код.
pub struct TranspilerContext<'a> {
    pub input_filename: &'a str,
    pub used_externs: HashSet<String>,
    pub struct_fields: HashMap<String, Vec<String>>,
    pub struct_field_types: HashMap<String, Vec<(String, String)>>,
    pub struct_bases: HashMap<String, Vec<String>>,
    pub struct_methods: HashMap<String, Vec<(String, Value)>>,
    pub defined_functions: HashSet<String>,
    pub current_struct: Option<String>,
    pub current_method_is_const: bool,
    pub indent_level: usize,
    pub synthesized_top_level: Vec<String>,
    pub lambda_counter: usize,
    pub coroutine_counter: usize,
    pub current_lambda_captures: HashMap<String, bool>,
    pub id_to_struct_name: HashMap<String, String>,
    pub current_ref_params: HashSet<String>,
}

impl<'a> TranspilerContext<'a> {
    pub fn new(input_filename: &'a str) -> Self {
        Self {
            input_filename,
            used_externs: HashSet::new(),
            struct_fields: HashMap::new(),
            struct_field_types: HashMap::new(),
            struct_bases: HashMap::new(),
            struct_methods: HashMap::new(),
            id_to_struct_name: HashMap::new(),
            defined_functions: HashSet::new(),
            current_struct: None,
            current_method_is_const: false,
            indent_level: 0,
            synthesized_top_level: Vec::new(),
            lambda_counter: 0,
            coroutine_counter: 0,
            current_lambda_captures: HashMap::new(),
            current_ref_params: HashSet::new(),
        }
    }

    pub fn indent(&self) -> String {
        "    ".repeat(self.indent_level)
    }

    pub fn record_extern(&mut self, name: &str) {
        match name {
            "printf" | "puts" | "malloc" | "free" | "exit" | "abort" | "clock" |
            "sqrt" | "pow" | "sin" | "cos" | "tan" | "memset" | "memcpy" | "memmove" => {
                self.used_externs.insert(name.to_string());
            }
            _ => {}
        }
    }
}

/// Запуск clang++ и получение Clang AST в формате JSON.
pub fn run_clang_ast_dump(
    cpp_path: &Path,
    opts: &CppToGorawOptions,
) -> Result<Value, String> {
    let sdk = crate::c_interop::get_sdk_paths();
    let raw_target = opts.target_triple.as_deref().unwrap_or("x86_64-w64-windows-gnu");
    let target = if !sdk.includes.is_empty() && (raw_target.contains("windows-gnu") || raw_target.contains("w64")) {
        "x86_64-pc-windows-msvc"
    } else {
        raw_target
    };
    let cpp_std = opts.cpp_std.as_deref().unwrap_or("c++23");
    let detected_clang;
    let clang_bin = match opts.clang_path.as_deref() {
        Some(p) => p,
        None => {
            detected_clang = crate::c_interop::find_clang(true);
            &detected_clang
        }
    };

    let mut cmd = Command::new(clang_bin);
    cmd.arg(format!("--target={target}"));
    cmd.arg(format!("-std={cpp_std}"));
    cmd.arg("-Xclang").arg("-ast-dump=json");
    cmd.arg("-fsyntax-only");
    cmd.arg("-fno-exceptions");

    for inc in &opts.extra_includes {
        cmd.arg("-I").arg(inc);
    }

    // Добавляем пути Windows SDK и MSVC, если обнаружены
    let sdk = crate::c_interop::get_sdk_paths();
    for inc in &sdk.includes {
        cmd.arg("-isystem").arg(inc);
    }

    cmd.arg(cpp_path);

    let output = cmd
        .output()
        .map_err(|e| format!("не удалось запустить `{clang_bin}`: {e}"))?;

    if output.stdout.is_empty() {
        let stderr_str = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "clang++ завершился с ошибкой или не вернул AST JSON:\n{stderr_str}"
        ));
    }

    let mut deserializer = serde_json::Deserializer::from_slice(&output.stdout);
    deserializer.disable_recursion_limit();
    let ast: Value = serde::Deserialize::deserialize(&mut deserializer)
        .map_err(|e| format!("ошибка парсинга AST JSON от clang: {e}"))?;

    Ok(ast)
}

/// Главная точка входа: трансляция C++ файла в Goraw код.
pub fn transpile_cpp_file(
    cpp_path: &Path,
    opts: &CppToGorawOptions,
) -> Result<String, String> {
    match run_clang_ast_dump(cpp_path, opts) {
        Ok(ast) => {
            let fname = cpp_path.file_name().and_then(|n| n.to_str()).unwrap_or("<unknown>");
            transpile_cpp_ast(&ast, fname)
        }
        Err(clang_err) => {
            let text = std::fs::read_to_string(cpp_path)
                .map_err(|e| format!("не удалось прочитать `{}`: {e}", cpp_path.display()))?;
            match transpile_cpp_source_standalone(&text) {
                Ok(code) => Ok(code),
                Err(standalone_err) => {
                    Err(format!("ошибка clang++:\n{clang_err}\nошибка автономного транслятора: {standalone_err}"))
                }
            }
        }
    }
}

/// Главная функция трансляции распарсенного AST в Goraw код.
pub fn transpile_cpp_ast(root: &Value, input_filename: &str) -> Result<String, String> {
    let mut ctx = TranspilerContext::new(input_filename);

    let inner = root
        .get("inner")
        .and_then(|i| i.as_array())
        .ok_or_else(|| "AST не содержит корневого узла 'inner'".to_string())?;

    // 1-й проход: сбор всех структур и их полей
    collect_struct_signatures(inner, input_filename, &mut ctx);

    // 2-й проход: генерация кода определений
    let mut top_level_code = Vec::new();

    for decl in inner {
        if !is_from_input_file(decl, input_filename) {
            continue;
        }

        if let Some(code) = lift_top_level_decl(decl, &mut ctx) {
            if !code.trim().is_empty() {
                top_level_code.push(code);
            }
        }
    }

    // Синтезированные замыкания, корутины и вспомогательные структуры
    for synth in &ctx.synthesized_top_level {
        if !synth.trim().is_empty() {
            top_level_code.push(synth.clone());
        }
    }

    // Сборка итогового файла
    let mut out = String::new();

    // Внешние функции рантайма libc
    if !ctx.used_externs.is_empty() {
        out.push_str("// Внешние системные функции libc\n");
        let mut sorted_externs: Vec<_> = ctx.used_externs.iter().collect();
        sorted_externs.sort();
        for ext in sorted_externs {
            match ext.as_str() {
                "printf" => out.push_str("extern fn printf(fmt: *u8, ...) -> i32;\n"),
                "puts" => out.push_str("extern fn puts(s: *u8) -> i32;\n"),
                "malloc" => out.push_str("extern fn malloc(size: u64) -> *mut u8;\n"),
                "free" => out.push_str("extern fn free(ptr: *mut u8);\n"),
                "exit" => out.push_str("extern fn exit(code: i32);\n"),
                "abort" => out.push_str("extern fn abort();\n"),
                "clock" => out.push_str("extern fn clock() -> i64;\n"),
                "sqrt" => out.push_str("extern fn sqrt(x: f64) -> f64;\n"),
                "pow" => out.push_str("extern fn pow(x: f64, y: f64) -> f64;\n"),
                "sin" => out.push_str("extern fn sin(x: f64) -> f64;\n"),
                "cos" => out.push_str("extern fn cos(x: f64) -> f64;\n"),
                "tan" => out.push_str("extern fn tan(x: f64) -> f64;\n"),
                "memset" => out.push_str("extern fn memset(dest: *mut u8, c: i32, n: u64) -> *mut u8;\n"),
                "memcpy" => out.push_str("extern fn memcpy(dest: *mut u8, src: *u8, n: u64) -> *mut u8;\n"),
                "memmove" => out.push_str("extern fn memmove(dest: *mut u8, src: *u8, n: u64) -> *mut u8;\n"),
                _ => {}
            }
        }
        out.push('\n');
    }

    out.push_str(&top_level_code.join("\n\n"));
    out.push('\n');

    Ok(out)
}

/// Проверка, принадлежит ли узел AST целевому пользовательскому файлу.
fn is_from_input_file(node: &Value, input_filename: &str) -> bool {
    // Пропускаем неявные / сгенерированные компилятором узлы
    if node.get("isImplicit").and_then(|v| v.as_bool()).unwrap_or(false) {
        return false;
    }

    // Исключаем встроенные операторы выделения памяти
    if let Some(name) = node.get("name").and_then(|n| n.as_str()) {
        if name.starts_with("operator new") || name.starts_with("operator delete") {
            return false;
        }
    }

    if let Some(loc) = node.get("loc") {
        if loc.get("includedFrom").is_some() {
            return false;
        }
        if let Some(f) = loc.get("file").and_then(|f| f.as_str()) {
            if f != "<stdin>" && !files_match(f, input_filename) {
                return false;
            }
        }
        if let Some(sloc) = loc.get("spellingLoc") {
            if sloc.get("includedFrom").is_some() {
                return false;
            }
            if let Some(f) = sloc.get("file").and_then(|f| f.as_str()) {
                if f != "<stdin>" && !files_match(f, input_filename) {
                    return false;
                }
            }
        }
        if let Some(eloc) = loc.get("expansionLoc") {
            if eloc.get("includedFrom").is_some() {
                return false;
            }
        }
    }

    if let Some(range) = node.get("range") {
        if let Some(begin) = range.get("begin") {
            if begin.get("includedFrom").is_some() {
                return false;
            }
            if let Some(f) = begin.get("file").and_then(|f| f.as_str()) {
                if f != "<stdin>" && !files_match(f, input_filename) {
                    return false;
                }
            }
        }
        if let Some(end) = range.get("end") {
            if end.get("includedFrom").is_some() {
                return false;
            }
        }
    }

    // Проверяем, есть ли привязка к строке/смещению в файле.
    // Если loc пустой {} и range.begin пустой {} — это компиляторный builtin без исходного кода.
    let has_loc = node.get("loc").map(|l| l.is_object() && !l.as_object().unwrap().is_empty()).unwrap_or(false);
    let has_range = node.get("range").and_then(|r| r.get("begin")).map(|b| b.is_object() && !b.as_object().unwrap().is_empty()).unwrap_or(false);
    if !has_loc && !has_range {
        return false;
    }

    true
}

fn files_match(loc_file: &str, input_filename: &str) -> bool {
    let p1 = Path::new(loc_file);
    let p2 = Path::new(input_filename);
    if p1 == p2 {
        return true;
    }
    if let (Some(n1), Some(n2)) = (p1.file_name(), p2.file_name()) {
        if n1 == n2 {
            return true;
        }
    }
    false
}

/// 1-й проход: сбор всех структур, их полей, базовых классов и методов.
fn collect_struct_signatures(
    nodes: &[Value],
    input_filename: &str,
    ctx: &mut TranspilerContext,
) {
    let mut raw_fields: HashMap<String, Vec<String>> = HashMap::new();
    let mut raw_field_types: HashMap<String, Vec<(String, String)>> = HashMap::new();

    collect_struct_signatures_recursive(nodes, input_filename, ctx, &mut raw_fields, &mut raw_field_types);

    // Уплощение полей при множественном наследовании БЕЗ vtables (flat struct layout)
    for sname in raw_fields.keys() {
        let mut visited = HashSet::new();
        let flat_f = flatten_struct_field_names(sname, &ctx.struct_bases, &raw_fields, &mut visited);
        ctx.struct_fields.insert(sname.clone(), flat_f);

        let mut visited_t = HashSet::new();
        let flat_ft = flatten_struct_field_types(sname, &ctx.struct_bases, &raw_field_types, &mut visited_t);
        ctx.struct_field_types.insert(sname.clone(), flat_ft);
    }
}

fn collect_struct_signatures_recursive(
    nodes: &[Value],
    input_filename: &str,
    ctx: &mut TranspilerContext,
    raw_fields: &mut HashMap<String, Vec<String>>,
    raw_field_types: &mut HashMap<String, Vec<(String, String)>>,
) {
    for node in nodes {
        let kind = node.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        if kind == "NamespaceDecl" || kind == "LinkageSpecDecl" || kind == "ExportDecl" {
            if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
                collect_struct_signatures_recursive(inner, input_filename, ctx, raw_fields, raw_field_types);
            }
            continue;
        }

        if kind == "RecordDecl" || kind == "CXXRecordDecl" {
            if let Some(id) = node.get("id").and_then(|i| i.as_str()) {
                if let Some(name) = node.get("name").and_then(|n| n.as_str()) {
                    ctx.id_to_struct_name.insert(id.to_string(), name.to_string());
                }
            }
            let is_complete = node.get("completeDefinition").and_then(|b| b.as_bool()).unwrap_or(false);
            if !is_complete {
                continue;
            }
            if let Some(name) = node.get("name").and_then(|n| n.as_str()) {
                if name == "GorawStr" || name.starts_with('_') {
                    continue;
                }

                // Сбор базовых классов множественного наследования
                let mut bases = Vec::new();
                if let Some(bases_arr) = node.get("bases").and_then(|b| b.as_array()) {
                    for b in bases_arr {
                        if let Some(bty) = b.get("type").and_then(|t| t.get("qualType")).and_then(|q| q.as_str()) {
                            let base_clean = map_cpp_type(bty, None);
                            bases.push(base_clean);
                        }
                    }
                }
                if !bases.is_empty() {
                    ctx.struct_bases.insert(name.to_string(), bases);
                }

                let mut fields = Vec::new();
                let mut field_types = Vec::new();
                let mut methods = Vec::new();

                if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
                    for member in inner {
                        let mkind = member.get("kind").and_then(|k| k.as_str()).unwrap_or("");
                        if mkind == "FieldDecl" {
                            if let Some(fname) = member.get("name").and_then(|n| n.as_str()) {
                                let ftype = member.get("type").and_then(|t| t.get("qualType")).and_then(|q| q.as_str()).unwrap_or("i32");
                                let mapped_ty = map_cpp_type(ftype, None);
                                fields.push(fname.to_string());
                                field_types.push((fname.to_string(), mapped_ty));
                            }
                        } else if mkind == "CXXMethodDecl" {
                            if let Some(mname) = member.get("name").and_then(|n| n.as_str()) {
                                methods.push((mname.to_string(), member.clone()));
                            }
                        }
                    }
                }
                raw_fields.insert(name.to_string(), fields);
                raw_field_types.insert(name.to_string(), field_types);
                ctx.struct_methods.insert(name.to_string(), methods);
            }
        }

        if (kind == "FunctionDecl" || kind == "FunctionTemplateDecl") && is_from_input_file(node, input_filename) {
            if let Some(name) = node.get("name").and_then(|n| n.as_str()) {
                let has_body = node.get("inner")
                    .and_then(|i| i.as_array())
                    .map(|arr| arr.iter().any(|c| {
                        let k = c.get("kind").and_then(|k| k.as_str()).unwrap_or("");
                        k == "CompoundStmt" || k == "CoroutineBodyStmt"
                    }))
                    .unwrap_or(false);
                if has_body {
                    ctx.defined_functions.insert(name.to_string());
                }
            }
        }
    }
}

fn flatten_struct_field_names(
    sname: &str,
    bases_map: &HashMap<String, Vec<String>>,
    raw_fields: &HashMap<String, Vec<String>>,
    visited: &mut HashSet<String>,
) -> Vec<String> {
    if !visited.insert(sname.to_string()) {
        return Vec::new();
    }
    let mut res = Vec::new();
    if let Some(bases) = bases_map.get(sname) {
        for b in bases {
            res.extend(flatten_struct_field_names(b, bases_map, raw_fields, visited));
        }
    }
    if let Some(own) = raw_fields.get(sname) {
        res.extend(own.clone());
    }
    res
}

fn flatten_struct_field_types(
    sname: &str,
    bases_map: &HashMap<String, Vec<String>>,
    raw_field_types: &HashMap<String, Vec<(String, String)>>,
    visited: &mut HashSet<String>,
) -> Vec<(String, String)> {
    if !visited.insert(sname.to_string()) {
        return Vec::new();
    }
    let mut res = Vec::new();
    if let Some(bases) = bases_map.get(sname) {
        for b in bases {
            res.extend(flatten_struct_field_types(b, bases_map, raw_field_types, visited));
        }
    }
    if let Some(own) = raw_field_types.get(sname) {
        res.extend(own.clone());
    }
    res
}

/// Трансляция декларации верхнего уровня.
fn lift_top_level_decl(node: &Value, ctx: &mut TranspilerContext) -> Option<String> {
    let kind = node.get("kind")?.as_str()?;
    match kind {
        "RecordDecl" | "CXXRecordDecl" => lift_record_decl(node, ctx),
        "ClassTemplateDecl" => lift_class_template_decl(node, ctx),
        "EnumDecl" => lift_enum_decl(node, ctx),
        "FunctionDecl" => lift_function_decl(node, ctx),
        "CXXMethodDecl" | "CXXConstructorDecl" | "CXXDestructorDecl" => lift_out_of_line_method_decl(node, ctx),
        "FunctionTemplateDecl" => lift_function_template_decl(node, ctx),
        "VarDecl" => lift_global_var_decl(node, ctx),
        "NamespaceDecl" | "LinkageSpecDecl" | "ExportDecl" => {
            let mut parts = Vec::new();
            if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
                for child in inner {
                    if is_from_input_file(child, ctx.input_filename) {
                        if let Some(c) = lift_top_level_decl(child, ctx) {
                            parts.push(c);
                        }
                    }
                }
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n\n"))
            }
        }
        _ => None,
    }
}

/// Трансляция метода C++, определённого вне класса (out-of-line method definition).
fn lift_out_of_line_method_decl(node: &Value, ctx: &mut TranspilerContext) -> Option<String> {
    let struct_name = if let Some(parent_id) = node.get("parentDeclContextId").and_then(|p| p.as_str()) {
        ctx.id_to_struct_name.get(parent_id).cloned()
    } else {
        None
    }.or_else(|| {
        let mangled = node.get("mangledName").and_then(|m| m.as_str()).unwrap_or("");
        if let Some(at_idx) = mangled.find('@') {
            if let Some(end_at) = mangled[at_idx + 1..].find("@@") {
                return Some(mangled[at_idx + 1..at_idx + 1 + end_at].to_string());
            }
        }
        None
    }).unwrap_or_else(|| "Self".to_string());

    ctx.current_struct = Some(struct_name.clone());
    let res = lift_method_decl(node, &struct_name, ctx);
    ctx.current_struct = None;
    res
}

/// Маппинг типов C++ в типы Goraw.
pub fn map_cpp_type(qual_type: &str, desugared: Option<&str>) -> String {
    let mut s = qual_type.trim();

    // Префиксы
    if let Some(stripped) = s.strip_prefix("const ") {
        s = stripped.trim();
    }
    if let Some(stripped) = s.strip_prefix("struct ") {
        s = stripped.trim();
    }
    if let Some(stripped) = s.strip_prefix("class ") {
        s = stripped.trim();
    }
    if let Some(stripped) = s.strip_prefix("enum ") {
        s = stripped.trim();
    }

    // Указатели и ссылки
    if s.ends_with('*') {
        let base = s[..s.len() - 1].trim();
        let is_const = qual_type.contains("const ");
        let clean_base = if base == "char" || base == "const char" {
            "u8".to_string()
        } else {
            map_cpp_type(base, None)
        };
        return if is_const {
            format!("*{clean_base}")
        } else {
            format!("*mut {clean_base}")
        };
    }
    if s.ends_with('&') {
        let base = s[..s.len() - 1].trim();
        let is_const = qual_type.contains("const ");
        let clean_base = map_cpp_type(base, None);
        // В Goraw ссылки часто маппятся на указатели или значения
        return if is_const {
            match clean_base.as_str() {
                "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "f32" | "f64" | "bool" | "str" => clean_base,
                _ => format!("*{clean_base}"),
            }
        } else {
            format!("*mut {clean_base}")
        };
    }

    // Массивы фиксированного размера: T[N] или T [N]
    if let Some(idx_open) = s.find('[') {
        if let Some(idx_close) = s.find(']') {
            let elem_ty = &s[..idx_open].trim();
            let size_str = &s[idx_open + 1..idx_close].trim();
            let mapped_elem = map_cpp_type(elem_ty, None);
            return format!("[{size_str}]{mapped_elem}");
        }
    }

    // Вектор std::vector<T> -> []T
    if s.starts_with("std::vector<") || s.starts_with("vector<") {
        if let Some(start) = s.find('<') {
            if let Some(end) = s.rfind('>') {
                let inner = &s[start + 1..end];
                let mapped = map_cpp_type(inner, None);
                return format!("[]{mapped}");
            }
        }
    }

    // std::string -> str, llvm::StringRef -> str, Twine -> str
    if s == "std::string"
        || s == "string"
        || s == "std::string_view"
        || s == "string_view"
        || s == "GorawStr"
        || s == "StringRef"
        || s == "llvm::StringRef"
        || s == "Twine"
        || s == "llvm::Twine"
        || s.starts_with("SmallString<")
        || s.starts_with("llvm::SmallString<")
    {
        return "str".to_string();
    }

    // Примитивные типы
    match s {
        "int" | "signed int" | "int32_t" => "i32".to_string(),
        "unsigned int" | "uint32_t" => "u32".to_string(),
        "long long" | "signed long long" | "int64_t" | "long int" | "ssize_t" | "intptr_t" | "long" => "i64".to_string(),
        "unsigned long long" | "uint64_t" | "size_t" | "__size_t" | "uintptr_t" | "unsigned long" => "u64".to_string(),
        "short" | "signed short" | "int16_t" => "i16".to_string(),
        "unsigned short" | "uint16_t" => "u16".to_string(),
        "char" | "signed char" | "int8_t" => "i8".to_string(),
        "unsigned char" | "uint8_t" | "byte" => "u8".to_string(),
        "float" => "f32".to_string(),
        "double" => "f64".to_string(),
        "bool" | "_Bool" => "bool".to_string(),
        "void" => "void".to_string(),
        other => {
            // Если desugared доступен и отличается, пробуем его
            if let Some(desug) = desugared {
                if desug != qual_type && desug != other {
                    let d = map_cpp_type(desug, None);
                    if d != other {
                        return d;
                    }
                }
            }
            // Удаляем возможные пространства имён
            if let Some(last_col) = other.rfind("::") {
                other[last_col + 2..].to_string()
            } else {
                other.to_string()
            }
        }
    }
}

/// Извлечение типа возвращаемого значения из сигнатуры квалифицированного типа C++.
fn extract_return_type_from_qual_type(qual_type: &str) -> String {
    if let Some(idx) = qual_type.find('(') {
        let ret_part = qual_type[..idx].trim();
        map_cpp_type(ret_part, None)
    } else {
        "void".to_string()
    }
}

/// Трансляция RecordDecl (struct / class).
fn lift_record_decl(node: &Value, ctx: &mut TranspilerContext) -> Option<String> {
    let is_complete = node.get("completeDefinition").and_then(|b| b.as_bool()).unwrap_or(false);
    if !is_complete {
        return None;
    }

    let name = node.get("name")?.as_str()?.to_string();
    if name.starts_with('_') || name == "GorawStr" {
        return None;
    }

    let inner = node.get("inner").and_then(|i| i.as_array())?;

    // Пропускаем вспомогательные обёртки C++ std::coroutine_handle, так как в Goraw синтезируется прямая стейт-машина
    let is_coro_boilerplate = inner.iter().any(|c| {
        c.get("type").and_then(|t| t.get("qualType")).and_then(|q| q.as_str()).map(|s| s.contains("coroutine_handle")).unwrap_or(false)
            || c.get("name").and_then(|n| n.as_str()).map(|n| n == "promise_type").unwrap_or(false)
    });
    if is_coro_boilerplate {
        return None;
    }

    let mut fields = Vec::new();
    let mut methods = Vec::new();
    let mut own_method_names = HashSet::new();

    // Если есть уплощённые поля (включая унаследованные от базовых классов БЕЗ vtables)
    if let Some(ftypes) = ctx.struct_field_types.get(&name) {
        for (fname, ftype) in ftypes {
            fields.push(format!("    {fname}: {ftype},"));
        }
    } else {
        for child in inner {
            if child.get("kind").and_then(|k| k.as_str()) == Some("FieldDecl") {
                if let Some(fname) = child.get("name").and_then(|n| n.as_str()) {
                    let ftype = child
                        .get("type")
                        .and_then(|t| t.get("qualType"))
                        .and_then(|q| q.as_str())
                        .unwrap_or("i32");
                    let mapped_ty = map_cpp_type(ftype, None);
                    fields.push(format!("    {fname}: {mapped_ty},"));
                }
            }
        }
    }

    // Собственные методы структуры
    for child in inner {
        let child_kind = child.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        match child_kind {
            "CXXMethodDecl" | "CXXConstructorDecl" | "CXXDestructorDecl" => {
                if let Some(mname) = child.get("name").and_then(|n| n.as_str()) {
                    own_method_names.insert(mname.to_string());
                }
                ctx.current_struct = Some(name.clone());
                if let Some(m_code) = lift_method_decl(child, &name, ctx) {
                    methods.push(m_code);
                }
                ctx.current_struct = None;
            }
            _ => {}
        }
    }

    // Синтез унаследованных методов от базовых классов (множественное наследование без vtables)
    if let Some(bases) = ctx.struct_bases.get(&name).cloned() {
        let mut visited_bases = HashSet::new();
        let mut base_methods_to_add = Vec::new();
        collect_inherited_methods(&bases, &ctx.struct_bases, &ctx.struct_methods, &mut visited_bases, &mut base_methods_to_add);

        for (mname, method_node) in base_methods_to_add {
            if !own_method_names.contains(&mname) && !mname.starts_with('~') && !mname.starts_with('_') && mname != name {
                ctx.current_struct = Some(name.clone());
                if let Some(m_code) = lift_method_decl(&method_node, &name, ctx) {
                    methods.push(m_code);
                    own_method_names.insert(mname);
                }
                ctx.current_struct = None;
            }
        }
    }

    let mut out = String::new();
    out.push_str(&format!("struct {name} {{\n"));
    for f in fields {
        out.push_str(&f);
        out.push('\n');
    }
    out.push('}');

    if !methods.is_empty() {
        out.push_str("\n\n");
        out.push_str(&methods.join("\n\n"));
    }

    Some(out)
}

fn collect_inherited_methods(
    bases: &[String],
    bases_map: &HashMap<String, Vec<String>>,
    methods_map: &HashMap<String, Vec<(String, Value)>>,
    visited: &mut HashSet<String>,
    out: &mut Vec<(String, Value)>,
) {
    for b in bases {
        if !visited.insert(b.clone()) {
            continue;
        }
        if let Some(parent_bases) = bases_map.get(b) {
            collect_inherited_methods(parent_bases, bases_map, methods_map, visited, out);
        }
        if let Some(m_list) = methods_map.get(b) {
            for (mname, mnode) in m_list {
                out.push((mname.clone(), mnode.clone()));
            }
        }
    }
}

/// Трансляция метода структуры или класса.
fn lift_method_decl(node: &Value, struct_name: &str, ctx: &mut TranspilerContext) -> Option<String> {
    if struct_name == "GorawStr" {
        return None;
    }
    let method_name = node.get("name")?.as_str()?;
    if method_name.starts_with('_') && !method_name.starts_with("__") {
        return None;
    }

    let kind = node.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    if kind == "CXXConstructorDecl" || method_name == struct_name {
        if node.get("isImplicit").and_then(|b| b.as_bool()).unwrap_or(false)
            || node.get("isDefaulted").and_then(|b| b.as_bool()).unwrap_or(false)
        {
            return None;
        }
        // Если тело конструктора пустое или отсутствует (только список инициализации полей : x(x), y(y)), пропускаем
        let has_stmts = node.get("inner")
            .and_then(|i| i.as_array())
            .and_then(|arr| arr.iter().find(|c| c.get("kind").and_then(|k| k.as_str()) == Some("CompoundStmt")))
            .and_then(|cstmt| cstmt.get("inner"))
            .and_then(|c_inner| c_inner.as_array())
            .map(|stmts| !stmts.is_empty())
            .unwrap_or(false);
        if !has_stmts {
            return None;
        }
    }
    if kind == "CXXDestructorDecl" || method_name.starts_with('~') {
        if node.get("isImplicit").and_then(|b| b.as_bool()).unwrap_or(false)
            || node.get("isDefaulted").and_then(|b| b.as_bool()).unwrap_or(false)
        {
            return None;
        }
    }

    let qual_type = node.get("type")?.get("qualType")?.as_str()?;
    let is_const = qual_type.ends_with("const");
    ctx.current_method_is_const = is_const;

    let ret_type = extract_return_type_from_qual_type(qual_type);
    let ret_suffix = if ret_type == "void" || ret_type.is_empty() {
        String::new()
    } else {
        format!(" -> {ret_type}")
    };

    let inner = node.get("inner").and_then(|i| i.as_array())?;

    let mut params = Vec::new();
    // Приёмник self
    if is_const {
        params.push("self: *".to_string() + struct_name);
    } else {
        params.push("self".to_string());
    }

    let mut body_node = None;

    for child in inner {
        let kind = child.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        if kind == "ParmVarDecl" {
            let pname = child.get("name").and_then(|n| n.as_str()).unwrap_or("arg");
            let ptype = child
                .get("type")
                .and_then(|t| t.get("qualType"))
                .and_then(|q| q.as_str())
                .unwrap_or("i32");
            let mty = map_cpp_type(ptype, None);
            params.push(format!("{pname}: {mty}"));
        } else if kind == "CompoundStmt" {
            body_node = Some(child);
        }
    }

    let body_str = if let Some(bn) = body_node {
        lift_compound_stmt(bn, ctx)
    } else {
        return None; // метод без тела в хедере
    };

    Some(format!(
        "fn {struct_name}::{method_name}({}){ret_suffix} {body_str}",
        params.join(", ")
    ))
}

/// Трансляция EnumDecl.
fn lift_enum_decl(node: &Value, _ctx: &mut TranspilerContext) -> Option<String> {
    let name = node.get("name")?.as_str()?;
    let inner = node.get("inner").and_then(|i| i.as_array())?;

    let mut variants = Vec::new();
    let mut auto_val = 0i64;

    for child in inner {
        if child.get("kind").and_then(|k| k.as_str()) == Some("EnumConstantDecl") {
            let vname = child.get("name")?.as_str()?;
            // Значение перечисления
            let val = if let Some(v) = child.get("value").and_then(|v| v.as_str()) {
                v.parse::<i64>().unwrap_or(auto_val)
            } else {
                auto_val
            };
            auto_val = val + 1;
            variants.push(format!("    {vname} = {val},"));
        }
    }

    Some(format!("enum {name} {{\n{}\n}}", variants.join("\n")))
}

/// Трансляция глобальной переменной или константы (VarDecl).
fn lift_global_var_decl(node: &Value, ctx: &mut TranspilerContext) -> Option<String> {
    let name = node.get("name")?.as_str()?;
    if name.starts_with('_') {
        return None;
    }

    let qual_type = node.get("type")?.get("qualType")?.as_str()?;
    let mapped_ty = map_cpp_type(qual_type, None);

    let is_const = qual_type.contains("const ") || node.get("isConstexpr").and_then(|b| b.as_bool()).unwrap_or(false);

    // Инициализатор
    let init_str = if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
        if let Some(first_expr) = inner.iter().find(|n| {
            let k = n.get("kind").and_then(|k| k.as_str()).unwrap_or("");
            !k.ends_with("Attr") && k != "VisibilityAttr"
        }) {
            lift_expr(first_expr, ctx)
        } else {
            default_value_for_type(&mapped_ty)
        }
    } else {
        default_value_for_type(&mapped_ty)
    };

    if is_const {
        Some(format!("const {name}: {mapped_ty} = {init_str};"))
    } else {
        Some(format!("static mut {name}: {mapped_ty} = {init_str};"))
    }
}

/// Трансляция FunctionDecl.
fn lift_function_decl(node: &Value, ctx: &mut TranspilerContext) -> Option<String> {
    let mut name = node.get("name")?.as_str()?.to_string();
    if name.starts_with('_') {
        return None;
    }

    if name == "panic"
        || name == "goraw_panic"
        || name == "str_from_cstr"
        || name.starts_with("gw_")
        || name.starts_with("operator new")
        || name.starts_with("operator delete")
        || name.starts_with("op_new")
        || name.starts_with("op_delete")
    {
        return None;
    }

    // Операторы C++ переименовываем в идиоматичные функции
    if let Some(op) = name.strip_prefix("operator") {
        name = match op.trim() {
            "+" => "op_add".to_string(),
            "-" => "op_sub".to_string(),
            "*" => "op_mul".to_string(),
            "/" => "op_div".to_string(),
            "%" => "op_rem".to_string(),
            "==" => "op_eq".to_string(),
            "!=" => "op_ne".to_string(),
            "<" => "op_lt".to_string(),
            ">" => "op_gt".to_string(),
            "<=" => "op_le".to_string(),
            ">=" => "op_ge".to_string(),
            "[]" => "op_index".to_string(),
            "()" => "op_call".to_string(),
            _ => format!("op_{}", op.trim().replace(' ', "_")),
        };
    }

    let qual_type = node.get("type")?.get("qualType")?.as_str()?;
    let ret_type = extract_return_type_from_qual_type(qual_type);

    let ret_suffix = if name == "main" {
        if ret_type == "void" {
            String::new()
        } else {
            " -> i32".to_string()
        }
    } else if ret_type == "void" || ret_type.is_empty() {
        String::new()
    } else {
        format!(" -> {ret_type}")
    };

    let inner = node.get("inner").and_then(|i| i.as_array())?;

    // Проверяем, не корутина ли это (C++20 Coroutine: co_yield / co_return / co_await)
    let is_coroutine = inner.iter().any(|c| c.get("kind").and_then(|k| k.as_str()) == Some("CoroutineBodyStmt"));
    if is_coroutine {
        if let Some(coro_code) = lift_coroutine_decl(node, &name, ctx) {
            return Some(coro_code);
        }
    }

    let mut params = Vec::new();
    let mut body_node = None;

    for child in inner {
        let kind = child.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        if kind == "ParmVarDecl" {
            let pname = child.get("name").and_then(|n| n.as_str()).unwrap_or("arg");
            let ptype = child
                .get("type")
                .and_then(|t| t.get("qualType"))
                .and_then(|q| q.as_str())
                .unwrap_or("i32");
            let mty = map_cpp_type(ptype, None);
            params.push(format!("{pname}: {mty}"));
        } else if kind == "CompoundStmt" {
            body_node = Some(child);
        }
    }

    // Если main не использует параметры argc/argv, опускаем их для чистоты Goraw кода
    if name == "main" {
        params.clear();
    }

    if let Some(bn) = body_node {
        let body_str = lift_compound_stmt(bn, ctx);
        Some(format!("fn {name}({}){ret_suffix} {body_str}", params.join(", ")))
    } else {
        // Если функция имеет определение в этом же файле, пропускаем предварительное объявление!
        if ctx.defined_functions.contains(&name) {
            return None;
        }
        // Декларация extern
        Some(format!("extern fn {name}({}){ret_suffix};", params.join(", ")))
    }
}

/// Трансляция C++20 корутины в генератор со стейт-машиной в Goraw.
fn lift_coroutine_decl(node: &Value, name: &str, ctx: &mut TranspilerContext) -> Option<String> {
    let inner = node.get("inner")?.as_array()?;
    let coro_stmt = inner.iter().find(|c| c.get("kind").and_then(|k| k.as_str()) == Some("CoroutineBodyStmt"))?;
    let coro_inner = coro_stmt.get("inner")?.as_array()?;
    let user_body = coro_inner.first()?;

    let mut params = Vec::new();
    let mut param_names = Vec::new();
    let mut param_types = Vec::new();

    for child in inner {
        if child.get("kind").and_then(|k| k.as_str()) == Some("ParmVarDecl") {
            let pname = child.get("name").and_then(|n| n.as_str()).unwrap_or("arg");
            let ptype = child.get("type").and_then(|t| t.get("qualType")).and_then(|q| q.as_str()).unwrap_or("i32");
            let mty = map_cpp_type(ptype, None);
            params.push(format!("{pname}: {mty}"));
            param_names.push(pname.to_string());
            param_types.push(mty);
        }
    }

    let user_stmts = user_body.get("inner").and_then(|i| i.as_array())?;
    let for_stmt = user_stmts.iter().find(|s| s.get("kind").and_then(|k| k.as_str()) == Some("ForStmt"));

    ctx.coroutine_counter += 1;
    let gen_struct = format!("Generator_{name}");

    if let Some(fstmt) = for_stmt {
        let f_inner = fstmt.get("inner")?.as_array()?;
        let init_node = f_inner.get(0);
        let cond_node = f_inner.get(2);
        let inc_node = f_inner.get(3);

        let (var_name, var_init, var_type) = if let Some(in_n) = init_node {
            if let Some(vdecl) = in_n.get("inner").and_then(|i| i.as_array()).and_then(|a| a.first()) {
                let vname = vdecl.get("name").and_then(|n| n.as_str()).unwrap_or("i").to_string();
                let vty_raw = vdecl.get("type").and_then(|t| t.get("qualType")).and_then(|q| q.as_str()).unwrap_or("i32");
                let mty = map_cpp_type(vty_raw, None);
                let init_val = if let Some(vi) = vdecl.get("inner").and_then(|i| i.as_array()).and_then(|a| a.first()) {
                    lift_expr(vi, ctx)
                } else {
                    "0".to_string()
                };
                (vname, init_val, mty)
            } else {
                ("i".to_string(), "0".to_string(), "i32".to_string())
            }
        } else {
            ("i".to_string(), "0".to_string(), "i32".to_string())
        };

        let cond_str = if let Some(cn) = cond_node {
            let raw_cond = lift_expr(cn, ctx);
            let mut c = raw_cond.replace(&var_name, &format!("self.{var_name}"));
            for p in &param_names {
                c = c.replace(p, &format!("self.{p}"));
            }
            c
        } else {
            "true".to_string()
        };

        let inc_str = if let Some(icn) = inc_node {
            let raw_inc = lift_expr(icn, ctx);
            let mut s = raw_inc.replace(&var_name, &format!("self.{var_name}"));
            for p in &param_names {
                s = s.replace(p, &format!("self.{p}"));
            }
            s
        } else {
            format!("self.{var_name} += 1")
        };

        let yield_ty = var_type.clone();

        let mut struct_fields = Vec::new();
        struct_fields.push("    state: i32,".to_string());
        struct_fields.push(format!("    current_value: {yield_ty},"));
        for (pname, pty) in param_names.iter().zip(param_types.iter()) {
            struct_fields.push(format!("    {pname}: {pty},"));
        }
        struct_fields.push(format!("    {var_name}: {var_type},"));

        let mut init_fields = Vec::new();
        init_fields.push("state: 0".to_string());
        init_fields.push("current_value: 0".to_string());
        for pname in &param_names {
            init_fields.push(format!("{pname}: {pname}"));
        }
        init_fields.push(format!("{var_name}: {var_init}"));

        let struct_code = format!("struct {gen_struct} {{\n{}\n}}", struct_fields.join("\n"));

        let factory_code = format!(
            "fn {name}({}) -> {gen_struct} {{\n    return {gen_struct} {{ {} }};\n}}",
            params.join(", "),
            init_fields.join(", ")
        );

        let next_code = format!(
            "fn {gen_struct}::next(self: *mut {gen_struct}) -> bool {{\n    if self.state == 0 {{\n        self.state = 1;\n    }} else {{\n        {inc_str};\n    }}\n    if {cond_str} {{\n        self.current_value = self.{var_name};\n        return true;\n    }}\n    return false;\n}}"
        );

        let value_code = format!(
            "fn {gen_struct}::value(self: *{gen_struct}) -> {yield_ty} {{\n    return self.current_value;\n}}"
        );

        ctx.struct_fields.insert(gen_struct.clone(), vec!["state".to_string(), "current_value".to_string()]);

        return Some(format!("{struct_code}\n\n{factory_code}\n\n{next_code}\n\n{value_code}"));
    }

    let mut yields = Vec::new();
    for stmt in user_stmts {
        find_coyield_exprs(stmt, &mut yields);
    }

    let yield_ty = "i32".to_string();
    let mut struct_fields = Vec::new();
    struct_fields.push("    state: i32,".to_string());
    struct_fields.push(format!("    current_value: {yield_ty},"));
    for (pname, pty) in param_names.iter().zip(param_types.iter()) {
        struct_fields.push(format!("    {pname}: {pty},"));
    }

    let mut init_fields = Vec::new();
    init_fields.push("state: 0".to_string());
    init_fields.push("current_value: 0".to_string());
    for pname in &param_names {
        init_fields.push(format!("{pname}: {pname}"));
    }

    let mut match_arms = Vec::new();
    for (idx, yexpr) in yields.iter().enumerate() {
        let val_str = lift_expr(yexpr, ctx);
        let next_state = idx + 1;
        match_arms.push(format!(
            "        {idx} => {{\n            self.current_value = {val_str};\n            self.state = {next_state};\n            return true;\n        }},"
        ));
    }
    match_arms.push("        _ => {\n            return false;\n        },".to_string());

    let struct_code = format!("struct {gen_struct} {{\n{}\n}}", struct_fields.join("\n"));
    let factory_code = format!(
        "fn {name}({}) -> {gen_struct} {{\n    return {gen_struct} {{ {} }};\n}}",
        params.join(", "),
        init_fields.join(", ")
    );
    let next_code = format!(
        "fn {gen_struct}::next(self: *mut {gen_struct}) -> bool {{\n    match self.state {{\n{}\n    }}\n}}",
        match_arms.join("\n")
    );
    let value_code = format!(
        "fn {gen_struct}::value(self: *{gen_struct}) -> {yield_ty} {{\n    return self.current_value;\n}}"
    );

    ctx.struct_fields.insert(gen_struct.clone(), vec!["state".to_string(), "current_value".to_string()]);

    Some(format!("{struct_code}\n\n{factory_code}\n\n{next_code}\n\n{value_code}"))
}

fn find_coyield_exprs<'a>(node: &'a Value, out: &mut Vec<&'a Value>) {
    let kind = node.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    if kind == "CoyieldExpr" {
        if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
            if let Some(call) = inner.first() {
                if let Some(call_inner) = call.get("inner").and_then(|i| i.as_array()) {
                    if call_inner.len() >= 2 {
                        out.push(&call_inner[1]);
                        return;
                    }
                }
            }
            if let Some(first) = inner.first() {
                out.push(first);
            }
        }
        return;
    }
    if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
        for child in inner {
            find_coyield_exprs(child, out);
        }
    }
}

/// Трансляция неинстанциированных шаблонов функций в закомментированном виде.
fn lift_function_template_decl(node: &Value, ctx: &mut TranspilerContext) -> Option<String> {
    let name = node.get("name")?.as_str()?;
    if name.starts_with('_') {
        return None;
    }

    let mut specializations = Vec::new();
    let mut primary_func = None;
    let mut tparams = Vec::new();

    if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
        for child in inner {
            let ckind = child.get("kind").and_then(|k| k.as_str()).unwrap_or("");
            if ckind == "TemplateTypeParmDecl" || ckind == "NonTypeTemplateParmDecl" {
                if let Some(pname) = child.get("name").and_then(|n| n.as_str()) {
                    tparams.push(format!("typename {pname}"));
                }
            } else if ckind == "FunctionDecl" {
                if child.get("templateSpecializationKind").is_some() {
                    if let Some(s) = lift_function_decl(child, ctx) {
                        specializations.push(s);
                    }
                } else {
                    primary_func = Some(child);
                }
            }
        }
    }

    if !specializations.is_empty() {
        return Some(specializations.join("\n\n"));
    }

    if let Some(pfunc) = primary_func {
        let param_str = if tparams.is_empty() { "typename T".to_string() } else { tparams.join(", ") };
        let mut lines = Vec::new();
        lines.push("// ========================================================".to_string());
        lines.push(format!("// [C++ Template - Неинстанциированный шаблон функции: {name}]"));
        lines.push(format!("// template <{param_str}>"));

        if let Some(code) = lift_function_decl(pfunc, ctx) {
            for line in code.lines() {
                lines.push(format!("// {line}"));
            }
        } else {
            lines.push(format!("// fn {name}(...);"));
        }
        lines.push("// ========================================================".to_string());
        return Some(lines.join("\n"));
    }

    None
}

/// Трансляция неинстанциированных шаблонов классов в закомментированном виде.
fn lift_class_template_decl(node: &Value, ctx: &mut TranspilerContext) -> Option<String> {
    let name = node.get("name")?.as_str()?;
    if name.starts_with('_') {
        return None;
    }

    let mut tparams = Vec::new();
    let mut primary_rec = None;

    if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
        for child in inner {
            let ckind = child.get("kind").and_then(|k| k.as_str()).unwrap_or("");
            if ckind == "TemplateTypeParmDecl" || ckind == "NonTypeTemplateParmDecl" {
                if let Some(pname) = child.get("name").and_then(|n| n.as_str()) {
                    tparams.push(format!("typename {pname}"));
                }
            } else if (ckind == "CXXRecordDecl" || ckind == "RecordDecl") && primary_rec.is_none() {
                primary_rec = Some(child);
            }
        }
    }

    let param_str = if tparams.is_empty() { "typename T".to_string() } else { tparams.join(", ") };
    let mut lines = Vec::new();
    lines.push("// ========================================================".to_string());
    lines.push(format!("// [C++ Template - Неинстанциированный класс-шаблон: {name}]"));
    lines.push(format!("// template <{param_str}>"));

    if let Some(prec) = primary_rec {
        if let Some(code) = lift_record_decl(prec, ctx) {
            for line in code.lines() {
                lines.push(format!("// {line}"));
            }
        } else {
            lines.push(format!("// struct {name} {{}};"));
        }
    } else {
        lines.push(format!("// struct {name} {{}};"));
    }
    lines.push("// ========================================================".to_string());
    Some(lines.join("\n"))
}

/// Трансляция составного оператора CompoundStmt (`{ ... }`).
fn lift_compound_stmt(node: &Value, ctx: &mut TranspilerContext) -> String {
    let mut lines = Vec::new();
    ctx.indent_level += 1;

    if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
        for child in inner {
            if let Some(stmt_str) = lift_stmt(child, ctx) {
                if !stmt_str.trim().is_empty() {
                    lines.push(stmt_str);
                }
            }
        }
    }

    ctx.indent_level -= 1;

    if lines.is_empty() {
        "{}".to_string()
    } else {
        let indent = ctx.indent();
        format!("{{\n{}\n{indent}}}", lines.join("\n"))
    }
}

/// Трансляция одного оператора (Statement).
fn lift_stmt(node: &Value, ctx: &mut TranspilerContext) -> Option<String> {
    let kind = node.get("kind")?.as_str()?;
    let indent = ctx.indent();

    match kind {
        "CompoundStmt" => Some(format!("{indent}{}", lift_compound_stmt(node, ctx))),
        "DeclStmt" => {
            let mut stmts = Vec::new();
            if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
                for var_node in inner {
                    if var_node.get("kind").and_then(|k| k.as_str()) == Some("VarDecl") {
                        let vname = var_node.get("name")?.as_str()?;
                        let vtype_raw = var_node
                            .get("type")
                            .and_then(|t| t.get("qualType"))
                            .and_then(|q| q.as_str())
                            .unwrap_or("i32");
                        let mut mty = map_cpp_type(vtype_raw, None);

                        let init_expr = if let Some(vinner) = var_node.get("inner").and_then(|i| i.as_array()) {
                            vinner.iter().find(|c| {
                                let ck = c.get("kind").and_then(|k| k.as_str()).unwrap_or("");
                                !ck.ends_with("Attr")
                            }).map(|e| lift_expr(e, ctx))
                        } else {
                            None
                        };

                        let final_init = init_expr.unwrap_or_else(|| default_value_for_type(&mty));

                        // Если тип является лямбдой или генератором, уточняем синтезированное имя типа
                        if mty.contains("(lambda") || vtype_raw.contains("(lambda") {
                            if let Some(cname) = final_init.split([' ', '{']).next() {
                                if cname.starts_with("Closure_") {
                                    mty = cname.to_string();
                                }
                            }
                        } else if mty == "Generator" || vtype_raw == "Generator" {
                            if let Some(cname) = final_init.split([' ', '(']).next() {
                                if ctx.struct_fields.contains_key(&format!("Generator_{cname}")) {
                                    mty = format!("Generator_{cname}");
                                }
                            }
                        }

                        stmts.push(format!("{indent}let mut {vname}: {mty} = {final_init};"));
                    }
                }
            }
            Some(stmts.join("\n"))
        }
        "ReturnStmt" => {
            let expr_str = if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
                if let Some(first) = inner.first() {
                    format!(" {}", lift_expr(first, ctx))
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            Some(format!("{indent}return{expr_str};"))
        }
        "IfStmt" => {
            let inner = node.get("inner").and_then(|i| i.as_array())?;
            // В Clang AST: [init (опц), cond, then, else (опц)]
            let mut prefix = String::new();
            let mut idx = 0;
            if node.get("hasInit").and_then(|b| b.as_bool()).unwrap_or(false) {
                if let Some(init_node) = inner.get(0) {
                    if let Some(s) = lift_stmt(init_node, ctx) {
                        prefix.push_str(&s);
                        prefix.push('\n');
                    }
                }
                idx += 1;
            }

            let cond_node = inner.get(idx)?;
            let then_node = inner.get(idx + 1)?;
            let else_node = inner.get(idx + 2);

            let cond_str = lift_expr(cond_node, ctx);
            let then_str = lift_stmt_as_block(then_node, ctx);

            if let Some(else_stmt) = else_node {
                let else_str = lift_else_stmt(else_stmt, ctx);
                Some(format!("{prefix}{indent}if {cond_str} {then_str} else {else_str}"))
            } else {
                Some(format!("{prefix}{indent}if {cond_str} {then_str}"))
            }
        }
        "WhileStmt" => {
            let inner = node.get("inner").and_then(|i| i.as_array())?;
            let cond_node = inner.first()?;
            let body_node = inner.get(1)?;

            let cond_str = lift_expr(cond_node, ctx);
            let body_str = lift_stmt_as_block(body_node, ctx);
            Some(format!("{indent}while {cond_str} {body_str}"))
        }
        "DoStmt" => {
            let inner = node.get("inner").and_then(|i| i.as_array())?;
            let body_node = inner.first()?;
            let cond_node = inner.get(1)?;

            let cond_str = lift_expr(cond_node, ctx);
            let body_str = lift_stmt_as_block(body_node, ctx);
            Some(format!("{indent}while true {{\n{indent}    {body_str}\n{indent}    if !({cond_str}) {{ break; }}\n{indent}}}"))
        }
        "ForStmt" => {
            let inner = node.get("inner").and_then(|i| i.as_array())?;
            // [init, var_decl, cond, inc, body]
            let init_node = inner.get(0);
            let cond_node = inner.get(2);
            let inc_node = inner.get(3);
            let body_node = inner.get(4)?;

            let body_str = lift_stmt_as_block(body_node, ctx);

            let has_init = init_node.map(|n| n.get("kind").is_some()).unwrap_or(false);
            let has_cond = cond_node.map(|n| n.get("kind").is_some()).unwrap_or(false);
            let has_inc = inc_node.map(|n| n.get("kind").is_some()).unwrap_or(false);

            if !has_init && !has_cond && !has_inc {
                return Some(format!("{indent}for {body_str}"));
            }

            if !has_init && has_cond && !has_inc {
                let cond_str = lift_expr(cond_node.unwrap(), ctx);
                return Some(format!("{indent}while {cond_str} {body_str}"));
            }

            let init_str = if let Some(in_n) = init_node {
                if in_n.get("kind").is_some() {
                    let s = lift_stmt_inline(in_n, ctx);
                    s.trim_end_matches(';').to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            let cond_str = if let Some(cn) = cond_node {
                if cn.get("kind").is_some() {
                    lift_expr(cn, ctx)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            let inc_str = if let Some(icn) = inc_node {
                if icn.get("kind").is_some() {
                    let s = lift_expr(icn, ctx);
                    s.trim_end_matches(';').to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            Some(format!("{indent}for {init_str}; {cond_str}; {inc_str} {body_str}"))
        }
        "CXXForRangeStmt" => {
            let inner = node.get("inner").and_then(|i| i.as_array())?;
            // inner[0] содержит __range1 с выражением контейнера
            // inner[len - 2] содержит объявление переменной цикла x
            // inner[len - 1] содержит тело CompoundStmt
            if inner.len() >= 3 {
                let range_decl = inner.iter().find(|c| {
                    c.get("inner").and_then(|i| i.as_array())
                        .and_then(|a| a.first())
                        .and_then(|v| v.get("name"))
                        .and_then(|n| n.as_str())
                        .map(|n| n.starts_with("__range"))
                        .unwrap_or(false)
                }).or_else(|| inner.get(1)).or_else(|| inner.get(0));
                let var_decl = &inner[inner.len() - 2];
                let body_node = &inner[inner.len() - 1];

                let var_name = if let Some(vd_inner) = var_decl.get("inner").and_then(|i| i.as_array()) {
                    vd_inner.first().and_then(|v| v.get("name")).and_then(|n| n.as_str()).unwrap_or("x")
                } else {
                    "x"
                };

                let (iter_expr, is_fixed_array) = if let Some(rd) = range_decl {
                    if let Some(rd_inner) = rd.get("inner").and_then(|i| i.as_array()) {
                        if let Some(vnode) = rd_inner.first() {
                            let ty = vnode.get("type").and_then(|t| t.get("qualType")).and_then(|q| q.as_str()).unwrap_or("");
                            let is_arr = ty.contains('[') && ty.contains(']');
                            let expr = if let Some(vinner) = vnode.get("inner").and_then(|i| i.as_array()) {
                                vinner.first().map(|e| lift_expr(e, ctx)).unwrap_or_else(|| "arr".to_string())
                            } else {
                                "arr".to_string()
                            };
                            (expr, is_arr)
                        } else {
                            ("arr".to_string(), false)
                        }
                    } else {
                        ("arr".to_string(), false)
                    }
                } else {
                    ("arr".to_string(), false)
                };

                let iter_str = if is_fixed_array && !iter_expr.ends_with("[..]") {
                    format!("{iter_expr}[..]")
                } else {
                    iter_expr
                };

                let body_str = lift_stmt_as_block(body_node, ctx);
                return Some(format!("{indent}for {var_name} in {iter_str} {body_str}"));
            }
            None
        }
        "SwitchStmt" => {
            let inner = node.get("inner").and_then(|i| i.as_array())?;
            let cond_node = inner.first()?;
            let body_node = inner.get(1)?;

            let cond_str = lift_expr(cond_node, ctx);

            // Обработка веток CaseStmt и DefaultStmt
            let mut arms = Vec::new();
            if let Some(case_stmts) = body_node.get("inner").and_then(|i| i.as_array()) {
                ctx.indent_level += 1;
                let arm_indent = ctx.indent();
                for c in case_stmts {
                    let ckind = c.get("kind").and_then(|k| k.as_str()).unwrap_or("");
                    if ckind == "CaseStmt" {
                        if let Some(c_inner) = c.get("inner").and_then(|i| i.as_array()) {
                            if c_inner.len() >= 2 {
                                let case_val = lift_expr(&c_inner[0], ctx);
                                let case_body = lift_stmt_as_block(&c_inner[1], ctx);
                                arms.push(format!("{arm_indent}{case_val} => {case_body},"));
                            }
                        }
                    } else if ckind == "DefaultStmt" {
                        if let Some(c_inner) = c.get("inner").and_then(|i| i.as_array()) {
                            if let Some(first_body) = c_inner.first() {
                                let def_body = lift_stmt_as_block(first_body, ctx);
                                arms.push(format!("{arm_indent}_ => {def_body},"));
                            }
                        }
                    }
                }
                ctx.indent_level -= 1;
            }

            Some(format!("{indent}match {cond_str} {{\n{}\n{indent}}}", arms.join("\n")))
        }
        "BreakStmt" => Some(format!("{indent}break;")),
        "ContinueStmt" => Some(format!("{indent}continue;")),
        "CoreturnStmt" => Some(format!("{indent}return false;")),
        "CoyieldExpr" => {
            let mut yields = Vec::new();
            find_coyield_exprs(node, &mut yields);
            if let Some(y) = yields.first() {
                let v = lift_expr(y, ctx);
                Some(format!("{indent}self.current_value = {v}; return true;"))
            } else {
                Some(format!("{indent}return true;"))
            }
        }
        _ => {
            // Выражение в роли оператора
            let expr_str = lift_expr(node, ctx);
            if expr_str.is_empty() {
                None
            } else if expr_str.contains("_wassert") {
                if let Some(assert_stmt) = format_clean_assert(&expr_str, &indent) {
                    Some(assert_stmt)
                } else {
                    Some(format!("{indent}{expr_str};"))
                }
            } else {
                Some(format!("{indent}{expr_str};"))
            }
        }
    }
}

fn format_clean_assert(expr_str: &str, indent: &str) -> Option<String> {
    let prefix = "(((!!(";
    if let Some(idx) = expr_str.find(prefix) {
        let wassert_marker = " || (_wassert(";
        if let Some(wassert_idx) = expr_str.find(wassert_marker) {
            let cond_end = wassert_idx;
            let mut cond_raw = if expr_str[..cond_end].ends_with("))") {
                expr_str[idx + prefix.len()..cond_end - 2].trim()
            } else if expr_str[..cond_end].ends_with(')') {
                expr_str[idx + prefix.len()..cond_end - 1].trim()
            } else {
                expr_str[idx + prefix.len()..cond_end].trim()
            };
            if cond_raw.ends_with("))") {
                cond_raw = cond_raw[..cond_raw.len() - 2].trim();
            } else if cond_raw.ends_with(')') {
                cond_raw = cond_raw[..cond_raw.len() - 1].trim();
            }

            if let Some(and_idx) = cond_raw.rfind(" && \"") {
                let cond = cond_raw[..and_idx].trim();
                let msg = cond_raw[and_idx + 4..].trim();
                return Some(format!("{indent}assert!({cond}, {msg});"));
            } else {
                return Some(format!("{indent}assert!({cond_raw});"));
            }
        }
    }
    None
}

fn lift_else_stmt(node: &Value, ctx: &mut TranspilerContext) -> String {
    let kind = node.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    if kind == "IfStmt" {
        // Chained else if
        let inner = match node.get("inner").and_then(|i| i.as_array()) {
            Some(i) => i,
            None => return lift_stmt_as_block(node, ctx),
        };
        let cond_node = match inner.first() {
            Some(c) => c,
            None => return lift_stmt_as_block(node, ctx),
        };
        let then_node = match inner.get(1) {
            Some(t) => t,
            None => return lift_stmt_as_block(node, ctx),
        };
        let else_node = inner.get(2);

        let cond_str = lift_expr(cond_node, ctx);
        let then_str = lift_stmt_as_block(then_node, ctx);

        if let Some(else_st) = else_node {
            let next_else = lift_else_stmt(else_st, ctx);
            format!("if {cond_str} {then_str} else {next_else}")
        } else {
            format!("if {cond_str} {then_str}")
        }
    } else {
        lift_stmt_as_block(node, ctx)
    }
}

fn lift_stmt_as_block(node: &Value, ctx: &mut TranspilerContext) -> String {
    let kind = node.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    if kind == "CompoundStmt" {
        lift_compound_stmt(node, ctx)
    } else {
        ctx.indent_level += 1;
        let indent = ctx.indent();
        let s = lift_stmt(node, ctx).unwrap_or_else(|| String::new());
        ctx.indent_level -= 1;
        let parent_indent = ctx.indent();
        format!("{{\n{indent}{}\n{parent_indent}}}", s.trim())
    }
}

fn lift_stmt_inline(node: &Value, ctx: &mut TranspilerContext) -> String {
    let old_indent = ctx.indent_level;
    ctx.indent_level = 0;
    let res = lift_stmt(node, ctx).unwrap_or_default();
    ctx.indent_level = old_indent;
    res
}

/// Трансляция выражений (Expressions).
fn lift_expr(node: &Value, ctx: &mut TranspilerContext) -> String {
    let kind = match node.get("kind").and_then(|k| k.as_str()) {
        Some(k) => k,
        None => return String::new(),
    };

    match kind {
        // Литералы
        "IntegerLiteral" => {
            node.get("value").and_then(|v| v.as_str()).unwrap_or("0").to_string()
        }
        "FloatingLiteral" => {
            let v = node.get("value").and_then(|v| v.as_str()).unwrap_or("0.0");
            if !v.contains('.') {
                format!("{v}.0")
            } else {
                v.to_string()
            }
        }
        "CXXBoolLiteralExpr" => {
            node.get("value").and_then(|v| v.as_bool()).map(|b| b.to_string()).unwrap_or_else(|| "false".to_string())
        }
        "StringLiteral" => {
            let v = node.get("value").and_then(|v| v.as_str()).unwrap_or("\"\"");
            v.to_string()
        }
        "CharacterLiteral" => {
            if let Some(val) = node.get("value").and_then(|v| v.as_u64()) {
                if val >= 32 && val <= 126 {
                    format!("'{}'", (val as u8) as char)
                } else {
                    format!("{val}")
                }
            } else {
                "'\\0'".to_string()
            }
        }
        "CXXNullPtrLiteralExpr" | "GNUNullExpr" => "null".to_string(),

        // Идентификаторы и ссылки
        "DeclRefExpr" => {
            let name = node.get("referencedDecl")
                .and_then(|d| d.get("name"))
                .and_then(|n| n.as_str())
                .or_else(|| node.get("name").and_then(|n| n.as_str()))
                .unwrap_or("");

            // Переменные, захваченные замыканием (по ссылке или значению)
            if let Some(&is_by_ref) = ctx.current_lambda_captures.get(name) {
                if is_by_ref {
                    return format!("(*self.{name})");
                } else {
                    return format!("self.{name}");
                }
            }

            match name {
                "nullptr" | "NULL" => "null".to_string(),
                "endl" => "\"\\n\"".to_string(),
                "this" => "self".to_string(),
                _ => name.to_string(),
            }
        }
        "UnresolvedLookupExpr" => {
            let name = node.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if name == "endl" {
                "\"\\n\"".to_string()
            } else {
                name.to_string()
            }
        }
        "CXXThisExpr" => "self".to_string(),

        // Доступ к полям
        "MemberExpr" => {
            let field = node.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if let Some(&is_by_ref) = ctx.current_lambda_captures.get(field) {
                if is_by_ref {
                    return format!("(*self.{field})");
                } else {
                    return format!("self.{field}");
                }
            }
            if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
                if let Some(base) = inner.first() {
                    let base_kind = base.get("kind").and_then(|k| k.as_str()).unwrap_or("");
                    if base_kind == "CXXThisExpr" {
                        return format!("self.{field}");
                    }
                    let base_str = lift_expr(base, ctx);
                    return format!("{base_str}.{field}");
                }
            }
            if ctx.current_struct.is_some() {
                format!("self.{field}")
            } else {
                field.to_string()
            }
        }

        // Индексация массивов и указателей
        "ArraySubscriptExpr" => {
            if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
                if inner.len() >= 2 {
                    let base = lift_expr(&inner[0], ctx);
                    let idx = lift_expr(&inner[1], ctx);
                    return format!("{base}[{idx}]");
                }
            }
            "arr[0]".to_string()
        }

        // Бинарные операторы (+, -, ==, etc.)
        "BinaryOperator" => {
            let opcode = node.get("opcode").and_then(|o| o.as_str()).unwrap_or("+");
            if opcode == "<<" {
                let mut stream_args = Vec::new();
                if let Some(_) = collect_ostream_args(node, &mut stream_args) {
                    return lift_ostream_call(stream_args, ctx);
                }
            }
            if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
                if inner.len() >= 2 {
                    let lhs = lift_expr(&inner[0], ctx);
                    let rhs = lift_expr(&inner[1], ctx);
                    return format!("{lhs} {opcode} {rhs}");
                }
            }
            String::new()
        }

        // Составные присваивания (+=, -=, etc.)
        "CompoundAssignOperator" => {
            let opcode = node.get("opcode").and_then(|o| o.as_str()).unwrap_or("+=");
            if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
                if inner.len() >= 2 {
                    let lhs = lift_expr(&inner[0], ctx);
                    let rhs = lift_expr(&inner[1], ctx);
                    return format!("{lhs} {opcode} {rhs}");
                }
            }
            String::new()
        }

        // Унарные операторы (-, !, ~, *, &, ++, --)
        "UnaryOperator" => {
            let opcode = node.get("opcode").and_then(|o| o.as_str()).unwrap_or("-");
            let _is_postfix = node.get("isPostfix").and_then(|b| b.as_bool()).unwrap_or(false);
            if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
                if let Some(sub) = inner.first() {
                    let sub_str = lift_expr(sub, ctx);
                    return match opcode {
                        "++" => format!("{sub_str} += 1"),
                        "--" => format!("{sub_str} -= 1"),
                        "*" => format!("*{sub_str}"),
                        "&" => format!("&mut {sub_str}"),
                        "-" => format!("-{sub_str}"),
                        "!" => format!("!{sub_str}"),
                        "~" => format!("~{sub_str}"),
                        "+" => sub_str,
                        _ => format!("{opcode}{sub_str}"),
                    };
                }
            }
            String::new()
        }

        // Скобки
        "ParenExpr" => {
            if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
                if let Some(first) = inner.first() {
                    let s = lift_expr(first, ctx);
                    return format!("({s})");
                }
            }
            "()".to_string()
        }

        // Вызовы функций
        "CallExpr" => {
            lift_call_expr(node, ctx)
        }

        // Вызовы методов C++
        "CXXMemberCallExpr" => {
            lift_cxx_member_call_expr(node, ctx)
        }

        // Перегруженные операторы C++ (напр. std::cout << x)
        "CXXOperatorCallExpr" => {
            lift_cxx_operator_call_expr(node, ctx)
        }

        // Приведения типов
        "CStyleCastExpr" | "CXXStaticCastExpr" | "CXXReinterpretCastExpr" => {
            let qual_type = node.get("type").and_then(|t| t.get("qualType")).and_then(|q| q.as_str()).unwrap_or("i32");
            let target_ty = map_cpp_type(qual_type, None);
            if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
                if let Some(first) = inner.first() {
                    let sub = lift_expr(first, ctx);
                    return format!("{sub} as {target_ty}");
                }
            }
            String::new()
        }

        // Прозрачные обёртки компилятора
        "ImplicitCastExpr" | "MaterializeTemporaryExpr" | "CXXBindTemporaryExpr" | "ExprWithCleanups" => {
            if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
                if let Some(first) = inner.first() {
                    return lift_expr(first, ctx);
                }
            }
            String::new()
        }

        // Тернарный оператор (cond ? then : els)
        "ConditionalOperator" => {
            if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
                if inner.len() >= 3 {
                    let cond = lift_expr(&inner[0], ctx);
                    let thn = lift_expr(&inner[1], ctx);
                    let els = lift_expr(&inner[2], ctx);
                    return format!("if {cond} {{ {thn} }} else {{ {els} }}");
                }
            }
            String::new()
        }

        // Замыкания со сложным захватом (C++ Lambdas)
        "LambdaExpr" => {
            lift_lambda_expr(node, ctx)
        }

        // Выражение выдачи значения из корутины (CoyieldExpr)
        "CoyieldExpr" => {
            let mut yields = Vec::new();
            find_coyield_exprs(node, &mut yields);
            if let Some(y) = yields.first() {
                lift_expr(y, ctx)
            } else {
                "0".to_string()
            }
        }

        // Инициализаторы списком { a, b, c }, CXXConstructExpr и временные объекты CXXTemporaryObjectExpr
        "InitListExpr" | "CXXConstructExpr" | "CXXTemporaryObjectExpr" => {
            lift_init_list_or_construct(node, ctx)
        }

        // Восстановление синтаксиса из ошибок компиляции (RecoveryExpr)
        "RecoveryExpr" => {
            if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
                if !inner.is_empty() {
                    let first_kind = inner[0].get("kind").and_then(|k| k.as_str()).unwrap_or("");
                    if first_kind == "DeclRefExpr" || first_kind == "UnresolvedLookupExpr" {
                        let callee = lift_expr(&inner[0], ctx);
                        ctx.record_extern(&callee);
                        let args: Vec<String> = inner[1..].iter().map(|arg| lift_expr(arg, ctx)).collect();
                        return format!("{callee}({})", args.join(", "));
                    }
                    if let Some(first) = inner.first() {
                        return lift_expr(first, ctx);
                    }
                }
            }
            String::new()
        }

        _ => {
            // Фолбэк: если есть вложенные выражения, извлекаем первое
            if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
                if let Some(first) = inner.first() {
                    return lift_expr(first, ctx);
                }
            }
            String::new()
        }
    }
}

/// Вызов обычной функции CallExpr.
fn lift_call_expr(node: &Value, ctx: &mut TranspilerContext) -> String {
    let inner = match node.get("inner").and_then(|i| i.as_array()) {
        Some(i) => i,
        None => return String::new(),
    };

    if inner.is_empty() {
        return String::new();
    }

    let callee_node = strip_casts(&inner[0]);
    let callee_name = if let Some(n) = callee_node.get("referencedDecl").and_then(|d| d.get("name")).and_then(|n| n.as_str()) {
        n
    } else if let Some(n) = callee_node.get("name").and_then(|n| n.as_str()) {
        n
    } else {
        ""
    };

    ctx.record_extern(callee_name);

    // Аргументы вызова
    let args: Vec<String> = inner[1..].iter()
        .map(|arg| {
            let mut s = lift_expr(arg, ctx);
            if s.trim().is_empty() {
                let stripped = strip_casts(arg);
                if let Some(n) = stripped.get("name").and_then(|n| n.as_str()) {
                    s = n.to_string();
                } else if let Some(n) = stripped.get("referencedDecl").and_then(|d| d.get("name")).and_then(|n| n.as_str()) {
                    s = n.to_string();
                }
            }
            s
        })
        .filter(|s| !s.trim().is_empty())
        .collect();

    // Специальные функции
    match callee_name {
        "assert" => {
            let cond = args.first().cloned().unwrap_or_else(|| "true".to_string());
            format!("assert {cond}")
        }
        "min" | "std::min" => {
            if args.len() == 2 {
                format!("if {a} < {b} {{ {a} }} else {{ {b} }}", a = args[0], b = args[1])
            } else {
                format!("min({})", args.join(", "))
            }
        }
        "max" | "std::max" => {
            if args.len() == 2 {
                format!("if {a} > {b} {{ {a} }} else {{ {b} }}", a = args[0], b = args[1])
            } else {
                format!("max({})", args.join(", "))
            }
        }
        "abs" | "std::abs" => {
            if args.len() == 1 {
                format!("if {x} < 0 {{ -{x} }} else {{ {x} }}", x = args[0])
            } else {
                format!("abs({})", args.join(", "))
            }
        }
        _ => {
            let callee_str = lift_expr(callee_node, ctx);
            format!("{callee_str}({})", args.join(", "))
        }
    }
}

/// Вызов метода CXXMemberCallExpr.
fn lift_cxx_member_call_expr(node: &Value, ctx: &mut TranspilerContext) -> String {
    let inner = match node.get("inner").and_then(|i| i.as_array()) {
        Some(i) => i,
        None => return String::new(),
    };

    if inner.is_empty() {
        return String::new();
    }

    let member_node = &inner[0];
    let mut method_name = member_node.get("name").and_then(|n| n.as_str())
        .or_else(|| member_node.get("referencedMemberDecl").and_then(|d| d.get("name")).and_then(|n| n.as_str()))
        .unwrap_or("");
    if method_name.is_empty() {
        let stripped = strip_casts(member_node);
        if let Some(n) = stripped.get("name").and_then(|n| n.as_str()) {
            method_name = n;
        } else if let Some(n) = stripped.get("referencedMemberDecl").and_then(|d| d.get("name")).and_then(|n| n.as_str()) {
            method_name = n;
        }
    }

    // Объект приёмника (receiver)
    let receiver_str = if let Some(minner) = member_node.get("inner").and_then(|i| i.as_array()) {
        if let Some(r) = minner.first() {
            lift_expr(r, ctx)
        } else {
            "self".to_string()
        }
    } else {
        "self".to_string()
    };

    if method_name == "operator bool" || method_name == "operator_bool" {
        return receiver_str;
    }

    let args: Vec<String> = inner[1..].iter()
        .map(|arg| {
            let mut s = lift_expr(arg, ctx);
            if s.trim().is_empty() {
                let stripped = strip_casts(arg);
                if let Some(n) = stripped.get("name").and_then(|n| n.as_str()) {
                    s = n.to_string();
                } else if let Some(n) = stripped.get("referencedDecl").and_then(|d| d.get("name")).and_then(|n| n.as_str()) {
                    s = n.to_string();
                }
            }
            s
        })
        .filter(|s| !s.trim().is_empty())
        .collect();

    // Специальные методы векторов / контейнеров / строк
    match method_name {
        "push_back" => format!("{receiver_str}.push({})", args.join(", ")),
        "pop_back" => format!("{receiver_str}.pop()"),
        "size" | "length" => format!("{receiver_str}.len"),
        "empty" => format!("{receiver_str}.is_empty()"),
        "c_str" | "data" => format!("{receiver_str}.ptr"),
        "str" if args.is_empty() => receiver_str,
        "substr" => {
            if args.len() == 1 {
                format!("{receiver_str}[{}..]", args[0])
            } else if args.len() >= 2 {
                format!("{receiver_str}[{}..{} + {}]", args[0], args[0], args[1])
            } else {
                receiver_str
            }
        }
        _ if method_name.is_empty() => {
            if args.is_empty() {
                receiver_str
            } else {
                format!("{receiver_str}({})", args.join(", "))
            }
        }
        _ => format!("{receiver_str}.{method_name}({})", args.join(", ")),
    }
}

/// Перегруженные операторы CXXOperatorCallExpr (включая поток std::cout << x).
fn lift_cxx_operator_call_expr(node: &Value, ctx: &mut TranspilerContext) -> String {
    let inner = match node.get("inner").and_then(|i| i.as_array()) {
        Some(i) => i,
        None => return String::new(),
    };

    if inner.is_empty() {
        return String::new();
    }

    // Проверяем, не поток ли это ввода-вывода (std::cout << ...)
    let mut stream_args = Vec::new();
    if let Some(_) = collect_ostream_args(node, &mut stream_args) {
        return lift_ostream_call(stream_args, ctx);
    }

    // Вызов замыканий и лямбд через operator()
    let callee_node = strip_casts(&inner[0]);
    let op_name = callee_node.get("referencedDecl")
        .and_then(|d| d.get("name"))
        .and_then(|n| n.as_str())
        .or_else(|| callee_node.get("name").and_then(|n| n.as_str()))
        .unwrap_or("");

    if op_name == "operator()" || op_name == "operator ()" {
        let receiver = lift_expr(&inner[1], ctx);
        let args: Vec<String> = inner[2..].iter().map(|arg| lift_expr(arg, ctx)).collect();
        return format!("{receiver}.call({})", args.join(", "));
    }

    // Обычные перегруженные бинарные операторы
    if inner.len() >= 3 {
        if let Some(symbol) = op_name.strip_prefix("operator") {
            let lhs = lift_expr(&inner[1], ctx);
            let rhs = lift_expr(&inner[2], ctx);
            let s = symbol.trim();
            match s {
                "+" | "-" | "*" | "/" | "%" | "==" | "!=" | "<" | ">" | "<=" | ">=" | "&&" | "||" | "&" | "|" | "^" | "<<" | ">>" | "=" => {
                    return format!("{lhs} {s} {rhs}");
                }
                "[]" => {
                    return format!("{lhs}[{rhs}]");
                }
                _ => {}
            }
        }
    }

    // Фолбэк как обычный вызов функции
    let func_name = inner[0].get("name").and_then(|n| n.as_str()).unwrap_or("operator");
    let args: Vec<String> = inner[1..].iter().map(|arg| lift_expr(arg, ctx)).collect();
    format!("{func_name}({})", args.join(", "))
}

/// Генерация вызова printf из потокового вывода std::cout / std::cerr.
fn lift_ostream_call(stream_args: Vec<Value>, ctx: &mut TranspilerContext) -> String {
    let mut fmt_str = String::new();
    let mut printf_args = Vec::new();

    for arg_node in stream_args {
        let stripped = strip_casts(&arg_node);
        let kind = stripped.get("kind").and_then(|k| k.as_str()).unwrap_or("");

        let is_endl = if kind == "DeclRefExpr" || kind == "UnresolvedLookupExpr" {
            let dname = stripped.get("referencedDecl").and_then(|d| d.get("name")).and_then(|n| n.as_str())
                .or_else(|| stripped.get("name").and_then(|n| n.as_str())).unwrap_or("");
            dname == "endl"
        } else {
            false
        };

        if is_endl {
            fmt_str.push_str("\\n");
            continue;
        }

        if kind == "StringLiteral" {
            let val = stripped.get("value").and_then(|v| v.as_str()).unwrap_or("");
            let clean = if val.starts_with('"') && val.ends_with('"') && val.len() >= 2 {
                &val[1..val.len() - 1]
            } else {
                val
            };
            let escaped = clean.replace('%', "%%");
            fmt_str.push_str(&escaped);
            continue;
        }

        if kind == "CharacterLiteral" {
            if let Some(val) = stripped.get("value").and_then(|v| v.as_i64()) {
                if val == 10 {
                    fmt_str.push_str("\\n");
                    continue;
                }
            }
        }

        // Произвольное выражение
        let qual_type = stripped.get("type").and_then(|t| t.get("qualType")).and_then(|q| q.as_str()).unwrap_or("");
        let spec = if qual_type.contains("char *") || qual_type.contains("char*") || qual_type.contains("string") {
            "%s"
        } else if qual_type == "char" || qual_type == "unsigned char" {
            "%c"
        } else if qual_type.contains("long long") || qual_type.contains("int64") || qual_type.contains("size_t") || qual_type.contains("uint64") {
            "%lld"
        } else if qual_type.contains("float") || qual_type.contains("double") {
            "%f"
        } else if qual_type.contains('*') {
            "%p"
        } else {
            "%d"
        };

        fmt_str.push_str(spec);
        let arg_expr = lift_expr(&arg_node, ctx);
        printf_args.push(arg_expr);
    }

    ctx.record_extern("printf");
    if printf_args.is_empty() {
        format!("printf(\"{fmt_str}\")")
    } else {
        format!("printf(\"{fmt_str}\", {})", printf_args.join(", "))
    }
}

/// Снятие прозрачных обёрток (cast, parens)
fn strip_casts(mut node: &Value) -> &Value {
    while let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
        let kind = node.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        if matches!(kind, "ImplicitCastExpr" | "CStyleCastExpr" | "CXXStaticCastExpr" | "MaterializeTemporaryExpr" | "ParenExpr" | "CXXBindTemporaryExpr" | "ExprWithCleanups") {
            if let Some(first) = inner.first() {
                node = first;
                continue;
            }
        }
        break;
    }
    node
}

/// Сбор аргументов потокового вывода std::cout << a << b << ...
fn collect_ostream_args(node: &Value, args: &mut Vec<Value>) -> Option<String> {
    let stripped = strip_casts(node);
    let kind = stripped.get("kind").and_then(|k| k.as_str()).unwrap_or("");

    match kind {
        "BinaryOperator" => {
            let opcode = stripped.get("opcode").and_then(|o| o.as_str()).unwrap_or("");
            if opcode == "<<" {
                let inner = stripped.get("inner").and_then(|i| i.as_array())?;
                if inner.len() >= 2 {
                    let stream = collect_ostream_args(&inner[0], args);
                    if stream.is_some() {
                        args.push(inner[1].clone());
                        return stream;
                    }
                }
            }
        }
        "CXXOperatorCallExpr" => {
            let inner = stripped.get("inner").and_then(|i| i.as_array())?;
            if inner.len() >= 3 {
                let stream = collect_ostream_args(&inner[1], args);
                if stream.is_some() {
                    args.push(inner[2].clone());
                    return stream;
                }
            } else if inner.len() == 2 {
                let stream = collect_ostream_args(&inner[0], args);
                if stream.is_some() {
                    args.push(inner[1].clone());
                    return stream;
                }
            }
        }
        "RecoveryExpr" => {
            let inner = stripped.get("inner").and_then(|i| i.as_array())?;
            if inner.len() >= 2 {
                let stream = collect_ostream_args(&inner[0], args);
                if stream.is_some() {
                    args.push(inner[1].clone());
                    return stream;
                }
            }
        }
        "DeclRefExpr" => {
            let name = stripped.get("referencedDecl")
                .and_then(|d| d.get("name"))
                .and_then(|n| n.as_str())
                .or_else(|| stripped.get("name").and_then(|n| n.as_str()))?;
            if name == "cout" || name == "cerr" || name == "clog" {
                return Some(name.to_string());
            }
        }
        _ => {}
    }
    None
}

/// Трансляция списков инициализации InitListExpr и конструкторов CXXConstructExpr.
fn lift_init_list_or_construct(node: &Value, ctx: &mut TranspilerContext) -> String {
    let qual_type = node.get("type").and_then(|t| t.get("qualType")).and_then(|q| q.as_str()).unwrap_or("");
    let sname = map_cpp_type(qual_type, None);

    let inner = match node.get("inner").and_then(|i| i.as_array()) {
        Some(i) => i,
        None => {
            if sname == "str" {
                return "\"\"".to_string();
            }
            return format!("{sname} {{}}");
        }
    };

    let kind = node.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    if kind == "CXXConstructExpr" {
        // Копирование, перемещение или конвертация из 1 аргумента: std::string(a), ArrayRef(s)
        if inner.len() == 1 {
            return lift_expr(&inner[0], ctx);
        }
        if inner.is_empty() {
            if sname == "str" {
                return "\"\"".to_string();
            }
            return format!("{sname} {{}}");
        }
    }

    let values: Vec<String> = flatten_init_values(inner, ctx);

    // Если это строка str
    if sname == "str" {
        if values.is_empty() {
            return "\"\"".to_string();
        }
        if values.len() == 1 {
            return values[0].clone();
        }
        if values.len() == 2 {
            return format!("{}[0..{}]", values[0], values[1]);
        }
        return values.join(" + ");
    }

    // Если это известный struct с полями (включая множественное наследование)
    if let Some(field_names) = ctx.struct_fields.get(&sname).cloned() {
        if field_names.len() == values.len() {
            let pairs: Vec<String> = field_names.iter().zip(values.iter())
                .map(|(f, v)| format!("{f}: {v}"))
                .collect();
            return format!("{sname} {{ {} }}", pairs.join(", "));
        }
    }

    // Если это массив [a, b, c]
    if qual_type.contains('[') {
        return format!("[{}]", values.join(", "));
    }

    if values.is_empty() || (values.len() == 1 && (values[0] == sname || values[0].is_empty())) {
        if sname == "str" {
            "\"\"".to_string()
        } else {
            format!("{sname} {{}}")
        }
    } else {
        if sname == "str" {
            values.join(" + ")
        } else {
            format!("{sname} {{ {} }}", values.join(", "))
        }
    }
}

/// Рекурсивное уплощение вложенных списков инициализации базовых классов при множественном наследовании.
fn flatten_init_values(nodes: &[Value], ctx: &mut TranspilerContext) -> Vec<String> {
    let mut out = Vec::new();
    for n in nodes {
        let n_kind = n.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        let n_ty = n.get("type").and_then(|t| t.get("qualType")).and_then(|q| q.as_str()).unwrap_or("");
        let n_sname = map_cpp_type(n_ty, None);
        if (n_kind == "InitListExpr" || n_kind == "CXXConstructExpr") && ctx.struct_fields.contains_key(&n_sname) {
            if let Some(inner) = n.get("inner").and_then(|i| i.as_array()) {
                out.extend(flatten_init_values(inner, ctx));
                continue;
            }
        }
        out.push(lift_expr(n, ctx));
    }
    out
}

/// Трансляция C++ Lambda выражения со сложным захватом (по значению, по ссылке, mutable) в замыкание Goraw.
fn lift_lambda_expr(node: &Value, ctx: &mut TranspilerContext) -> String {
    let inner = match node.get("inner").and_then(|i| i.as_array()) {
        Some(i) if !i.is_empty() => i,
        _ => return "/* lambda */".to_string(),
    };

    let closure_rec = &inner[0];
    let closure_inner = closure_rec.get("inner").and_then(|i| i.as_array()).cloned().unwrap_or_default();

    // Находим CXXMethodDecl operator()
    let method_node = closure_inner.iter().find(|m| {
        m.get("kind").and_then(|k| k.as_str()) == Some("CXXMethodDecl")
            && m.get("name").and_then(|n| n.as_str()) == Some("operator()")
    });

    let mut is_const = true;
    let mut ret_type = "void".to_string();
    let mut params = Vec::new();
    let mut body_node = None;

    if let Some(mnode) = method_node {
        let qual_type = mnode.get("type").and_then(|t| t.get("qualType")).and_then(|q| q.as_str()).unwrap_or("");
        is_const = qual_type.ends_with("const");
        ret_type = extract_return_type_from_qual_type(qual_type);

        if let Some(m_inner) = mnode.get("inner").and_then(|i| i.as_array()) {
            for child in m_inner {
                let ckind = child.get("kind").and_then(|k| k.as_str()).unwrap_or("");
                if ckind == "ParmVarDecl" {
                    let pname = child.get("name").and_then(|n| n.as_str()).unwrap_or("arg");
                    let ptype = child.get("type").and_then(|t| t.get("qualType")).and_then(|q| q.as_str()).unwrap_or("i32");
                    let mty = map_cpp_type(ptype, None);
                    params.push(format!("{pname}: {mty}"));
                } else if ckind == "CompoundStmt" {
                    body_node = Some(child.clone());
                }
            }
        }
    }

    if body_node.is_none() {
        for child in inner.iter().rev() {
            if child.get("kind").and_then(|k| k.as_str()) == Some("CompoundStmt") {
                body_node = Some(child.clone());
                break;
            }
        }
    }

    let field_decls: Vec<&Value> = closure_inner.iter()
        .filter(|m| m.get("kind").and_then(|k| k.as_str()) == Some("FieldDecl"))
        .collect();

    let capture_inits: Vec<&Value> = inner[1..].iter()
        .filter(|c| c.get("kind").and_then(|k| k.as_str()) != Some("CompoundStmt"))
        .collect();

    ctx.lambda_counter += 1;
    let closure_name = format!("Closure_{}", ctx.lambda_counter);

    let mut struct_fields = Vec::new();
    let mut init_pairs = Vec::new();
    let mut captures_map = HashMap::new();
    let mut has_ref_captures = false;

    for (idx, cap_init) in capture_inits.iter().enumerate() {
        let stripped = strip_casts(cap_init);
        let var_name = stripped.get("referencedDecl")
            .and_then(|d| d.get("name"))
            .and_then(|n| n.as_str())
            .or_else(|| stripped.get("name").and_then(|n| n.as_str()))
            .or_else(|| field_decls.get(idx).and_then(|f| f.get("name")).and_then(|n| n.as_str()))
            .unwrap_or("");

        let fname = if var_name.is_empty() || var_name.starts_with('_') {
            format!("cap_{idx}")
        } else {
            var_name.to_string()
        };

        let field_qual_type = field_decls.get(idx)
            .and_then(|f| f.get("type"))
            .and_then(|t| t.get("qualType"))
            .and_then(|q| q.as_str())
            .unwrap_or("");

        let is_by_ref = field_qual_type.contains('&');
        if is_by_ref {
            has_ref_captures = true;
        }

        let base_qual = if let Some(stripped) = field_qual_type.strip_suffix('&') {
            stripped.trim()
        } else {
            field_qual_type
        };

        let mapped_ty = map_cpp_type(base_qual, None);

        if is_by_ref {
            struct_fields.push(format!("    {fname}: *mut {mapped_ty},"));
            let init_val = lift_expr(cap_init, ctx);
            let ref_init = if init_val.starts_with('&') || init_val.starts_with('*') {
                init_val
            } else {
                format!("&mut {init_val}")
            };
            init_pairs.push(format!("{fname}: {ref_init}"));
            captures_map.insert(fname.clone(), true);
            if !var_name.is_empty() && var_name != fname {
                captures_map.insert(var_name.to_string(), true);
            }
        } else {
            struct_fields.push(format!("    {fname}: {mapped_ty},"));
            let init_val = lift_expr(cap_init, ctx);
            init_pairs.push(format!("{fname}: {init_val}"));
            captures_map.insert(fname.clone(), false);
            if !var_name.is_empty() && var_name != fname {
                captures_map.insert(var_name.to_string(), false);
            }
        }
    }

    let saved_captures = std::mem::replace(&mut ctx.current_lambda_captures, captures_map);
    let saved_struct = ctx.current_struct.clone();
    ctx.current_struct = Some(closure_name.clone());

    let body_str = if let Some(ref bn) = body_node {
        lift_compound_stmt(bn, ctx)
    } else {
        "{}".to_string()
    };

    ctx.current_struct = saved_struct;
    ctx.current_lambda_captures = saved_captures;

    let receiver = if is_const && !has_ref_captures {
        format!("self: *{closure_name}")
    } else {
        format!("self: *mut {closure_name}")
    };

    let mut call_params = vec![receiver];
    call_params.extend(params);

    let ret_suffix = if ret_type == "void" || ret_type.is_empty() {
        String::new()
    } else {
        format!(" -> {ret_type}")
    };

    let fn_prefix = if has_ref_captures { "unsafe fn" } else { "fn" };

    let mut struct_code = format!("struct {closure_name} {{\n{}\n}}", struct_fields.join("\n"));
    if struct_fields.is_empty() {
        struct_code = format!("struct {closure_name} {{}}");
    }

    let method_code = format!(
        "{fn_prefix} {closure_name}::call({}){ret_suffix} {body_str}",
        call_params.join(", ")
    );

    ctx.synthesized_top_level.push(struct_code);
    ctx.synthesized_top_level.push(method_code);

    if init_pairs.is_empty() {
        format!("{closure_name} {{}}")
    } else {
        format!("{closure_name} {{ {} }}", init_pairs.join(", "))
    }
}

/// Получение значения по умолчанию для типа Goraw.
fn default_value_for_type(ty: &str) -> String {
    match ty {
        "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" => "0".to_string(),
        "f32" => "0.0".to_string(),
        "f64" => "0.0".to_string(),
        "bool" => "false".to_string(),
        s if s.starts_with('*') => "null".to_string(),
        s if s.starts_with('[') && s.contains(']') => "zeroed()".to_string(),
        _ => "zeroed()".to_string(),
    }
}

// ============================================================================
// Автономный C++ транслятор (чистый Rust, без зависимости от Clang/LLVM)
// ============================================================================

/// Автономный транслятор исходного C++ кода в Goraw без вызова clang++
pub fn transpile_cpp_source_standalone(src: &str) -> Result<String, String> {
    let mut out = String::new();
    out.push_str("// Транслировано из C++ автономным компилятором Goraw (чистый Rust без Clang)\n\n");

    let cleaned = strip_cpp_comments_and_directives(src);

    let needs_printf = cleaned.contains("printf(") || cleaned.contains("cout");
    if needs_printf {
        out.push_str("extern fn printf(arg0: *u8, ...) -> i32;\n\n");
    }

    let decls = split_top_level_cpp_decls(&cleaned);
    let mut methods_to_append = Vec::new();

    for decl in decls {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        if decl.starts_with("struct ") || decl.starts_with("class ") {
            let (struct_code, methods) = transpile_standalone_struct_or_class(decl);
            if !struct_code.is_empty() {
                out.push_str(&struct_code);
                out.push_str("\n\n");
            }
            methods_to_append.extend(methods);
        } else if decl.contains('(') && decl.contains('{') {
            out.push_str(&transpile_standalone_function(decl, None));
            out.push_str("\n\n");
        }
    }

    for method in methods_to_append {
        out.push_str(&method);
        out.push_str("\n\n");
    }

    Ok(out.trim().to_string() + "\n")
}

fn strip_cpp_comments_and_directives(src: &str) -> String {
    let mut res = String::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        res.push(bytes[i] as char);
        i += 1;
    }

    let mut filtered_lines = Vec::new();
    for line in res.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#')
            || trimmed.starts_with("using namespace")
            || trimmed.starts_with("using std::")
        {
            continue;
        }
        filtered_lines.push(line);
    }
    filtered_lines.join("\n")
}

fn split_top_level_cpp_decls(src: &str) -> Vec<String> {
    let mut decls = Vec::new();
    let mut cur = String::new();
    let mut depth = 0;
    let mut in_str = false;
    let mut str_char = ' ';

    for ch in src.chars() {
        if in_str {
            cur.push(ch);
            if ch == str_char {
                in_str = false;
            }
            continue;
        }

        if ch == '"' || ch == '\'' {
            in_str = true;
            str_char = ch;
            cur.push(ch);
            continue;
        }

        match ch {
            '{' => {
                depth += 1;
                cur.push(ch);
            }
            '}' => {
                depth -= 1;
                cur.push(ch);
                if depth == 0 {
                    let trimmed = cur.trim_start();
                    if !trimmed.starts_with("struct") && !trimmed.starts_with("class") {
                        decls.push(cur.clone());
                        cur.clear();
                    }
                }
            }
            ';' if depth == 0 => {
                cur.push(ch);
                decls.push(cur.clone());
                cur.clear();
            }
            _ => {
                cur.push(ch);
            }
        }
    }
    if !cur.trim().is_empty() {
        decls.push(cur);
    }
    decls
}

fn map_standalone_cpp_type(ty: &str) -> String {
    let ty = ty.trim().trim_start_matches("const ").trim_end_matches("const").trim();
    if ty.ends_with("**") {
        return "*u8".to_string();
    }
    if ty.ends_with('*') {
        let base = ty.trim_end_matches('*').trim();
        if base == "char" || base == "const char" || base == "void" {
            return "*u8".to_string();
        }
        return format!("*mut {}", map_standalone_cpp_type(base));
    }
    if ty.ends_with('&') {
        let base = ty.trim_end_matches('&').trim();
        return map_standalone_cpp_type(base);
    }
    match ty {
        "int" => "i32".to_string(),
        "long long" | "int64_t" | "int64" | "i64" => "i64".to_string(),
        "unsigned int" | "uint32_t" | "uint32" | "u32" => "u32".to_string(),
        "unsigned long long" | "uint64_t" | "uint64" | "u64" | "size_t" => "u64".to_string(),
        "short" | "int16_t" | "i16" => "i16".to_string(),
        "unsigned short" | "uint16_t" | "u16" => "u16".to_string(),
        "char" | "int8_t" | "i8" => "i8".to_string(),
        "unsigned char" | "uint8_t" | "u8" => "u8".to_string(),
        "float" | "f32" => "f32".to_string(),
        "double" | "f64" => "f64".to_string(),
        "bool" => "bool".to_string(),
        "void" => "void".to_string(),
        "string" | "std::string" | "string_view" | "std::string_view" => "str".to_string(),
        "auto" => "".to_string(),
        other => other.to_string(),
    }
}

fn split_struct_members(body: &str) -> Vec<String> {
    let mut members = Vec::new();
    let mut cur = String::new();
    let mut depth = 0;
    for ch in body.chars() {
        match ch {
            '{' => {
                depth += 1;
                cur.push(ch);
            }
            '}' => {
                depth -= 1;
                cur.push(ch);
                if depth == 0 {
                    members.push(cur.clone());
                    cur.clear();
                }
            }
            ';' if depth == 0 => {
                cur.push(ch);
                members.push(cur.clone());
                cur.clear();
            }
            _ => {
                cur.push(ch);
            }
        }
    }
    if !cur.trim().is_empty() {
        members.push(cur);
    }
    members
}

fn transpile_standalone_struct_or_class(decl: &str) -> (String, Vec<String>) {
    let is_struct = decl.starts_with("struct ");
    let prefix = if is_struct { "struct " } else { "class " };
    let after_prefix = decl.trim_start_matches(prefix).trim();

    let name_end = after_prefix.find(|c: char| c.is_whitespace() || c == '{' || c == ':').unwrap_or(after_prefix.len());
    let struct_name = after_prefix[..name_end].trim();

    let open_idx = match decl.find('{') {
        Some(i) => i,
        None => return (String::new(), Vec::new()),
    };
    let close_idx = match decl.rfind('}') {
        Some(i) => i,
        None => return (String::new(), Vec::new()),
    };
    let body = &decl[open_idx + 1..close_idx];

    let mut fields = Vec::new();
    let mut methods = Vec::new();

    for stmt in split_struct_members(body) {
        let stmt = stmt.trim();
        if stmt.is_empty() || stmt.starts_with("public:") || stmt.starts_with("private:") || stmt.starts_with("protected:") {
            continue;
        }
        if stmt.contains('(') && stmt.contains('{') {
            methods.push(transpile_standalone_function(stmt, Some(struct_name)));
            continue;
        }
        let stmt_clean = stmt.trim_end_matches(';').trim();
        let words: Vec<&str> = stmt_clean.split_whitespace().collect();
        if words.len() >= 2 {
            let field_name = words.last().unwrap();
            let ty_part = words[..words.len() - 1].join(" ");
            let gw_ty = map_standalone_cpp_type(&ty_part);
            fields.push(format!("    {field_name}: {gw_ty},"));
        }
    }

    let mut struct_code = format!("struct {struct_name} {{\n");
    for f in fields {
        struct_code.push_str(&f);
        struct_code.push('\n');
    }
    struct_code.push('}');

    (struct_code, methods)
}

fn transpile_standalone_function(decl: &str, struct_context: Option<&str>) -> String {
    let open_brace = match decl.find('{') {
        Some(i) => i,
        None => return String::new(),
    };
    let close_brace = match decl.rfind('}') {
        Some(i) => i,
        None => return String::new(),
    };

    let header = decl[..open_brace].trim();
    let body = &decl[open_brace + 1..close_brace];

    let open_paren = match header.find('(') {
        Some(i) => i,
        None => return String::new(),
    };
    let close_paren = match header.rfind(')') {
        Some(i) => i,
        None => return String::new(),
    };

    let fn_sig = header[..open_paren].trim();
    let words: Vec<&str> = fn_sig.split_whitespace().collect();
    let fn_name = words.last().cloned().unwrap_or("unknown");
    let ret_ty_raw = words[..words.len().saturating_sub(1)].join(" ");
    let ret_ty = map_standalone_cpp_type(&ret_ty_raw);

    let params_raw = &header[open_paren + 1..close_paren].trim();
    let mut params = Vec::new();
    if let Some(sname) = struct_context {
        params.push(format!("self: *mut {sname}"));
    }

    if !params_raw.is_empty() {
        for p in params_raw.split(',') {
            let p = p.trim();
            if p.is_empty() || p == "void" {
                continue;
            }
            let p_words: Vec<&str> = p.split_whitespace().collect();
            if p_words.len() >= 2 {
                let pname = p_words.last().unwrap();
                let pty_raw = p_words[..p_words.len() - 1].join(" ");
                let pty = map_standalone_cpp_type(&pty_raw);
                params.push(format!("{pname}: {pty}"));
            } else if p_words.len() == 1 {
                let pty = map_standalone_cpp_type(p_words[0]);
                params.push(format!("arg{}: {pty}", params.len()));
            }
        }
    }

    let is_main = fn_name == "main";
    let (final_name, final_params, final_ret) = if is_main {
        ("main".to_string(), "argc: i32, argv: *u8".to_string(), " -> i32".to_string())
    } else {
        let name = if let Some(sname) = struct_context {
            format!("{sname}::{fn_name}")
        } else {
            fn_name.to_string()
        };
        let ret = if ret_ty.is_empty() || ret_ty == "void" {
            String::new()
        } else {
            format!(" -> {ret_ty}")
        };
        (name, params.join(", "), ret)
    };

    let mut out = format!("fn {final_name}({final_params}){final_ret} {{\n");
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let transformed = transform_standalone_body_stmt(trimmed);
        out.push_str("    ");
        out.push_str(&transformed);
        out.push('\n');
    }
    if is_main && !body.contains("return") {
        out.push_str("    return 0;\n");
    }
    out.push('}');
    out
}

fn transform_standalone_body_stmt(stmt: &str) -> String {
    let s = stmt.trim();

    // cout << ...
    if s.starts_with("cout <<") || s.starts_with("std::cout <<") {
        let parts: Vec<&str> = s.split("<<").collect();
        let mut fmt_str = String::new();
        let mut args = Vec::new();
        for p in &parts[1..] {
            let p = p.trim().trim_end_matches(';').trim();
            if p == "endl" || p == "std::endl" {
                fmt_str.push_str("\\n");
            } else if p.starts_with('"') && p.ends_with('"') {
                fmt_str.push_str(&p[1..p.len() - 1]);
            } else {
                fmt_str.push_str("%v");
                args.push(p);
            }
        }
        if args.is_empty() {
            return format!("printf(\"{fmt_str}\");");
        } else {
            return format!("printf(\"{fmt_str}\", {});", args.join(", "));
        }
    }

    // Variable declaration: auto x = ...; or int x = ...;
    if s.contains('=') && !s.starts_with("if") && !s.starts_with("while") && !s.starts_with("for") && !s.starts_with("return") {
        let eq_idx = s.find('=').unwrap();
        let lhs = s[..eq_idx].trim();
        let rhs = s[eq_idx + 1..].trim();
        let words: Vec<&str> = lhs.split_whitespace().collect();
        if words.len() >= 2 {
            let var_name = words.last().unwrap();
            let ty_raw = words[..words.len() - 1].join(" ");
            let gw_ty = map_standalone_cpp_type(&ty_raw);
            let cleaned_rhs = transform_expr(rhs.trim_end_matches(';'));
            if gw_ty.is_empty() || gw_ty == "auto" {
                return format!("let mut {var_name} = {cleaned_rhs};");
            } else {
                return format!("let mut {var_name}: {gw_ty} = {cleaned_rhs};");
            }
        }
    }

    // if (cond) { or while (cond) {
    if s.starts_with("if (") || s.starts_with("if(") {
        let after_if = s.trim_start_matches("if").trim();
        if let (Some(op), Some(cl)) = (after_if.find('('), after_if.rfind(')')) {
            let cond = &after_if[op + 1..cl];
            return format!("if {} {{", transform_expr(cond));
        }
    }
    if s.starts_with("while (") || s.starts_with("while(") {
        let after_while = s.trim_start_matches("while").trim();
        if let (Some(op), Some(cl)) = (after_while.find('('), after_while.rfind(')')) {
            let cond = &after_while[op + 1..cl];
            return format!("while {} {{", transform_expr(cond));
        }
    }

    // return expr;
    if s.starts_with("return ") || s == "return;" {
        let expr = s.trim_start_matches("return").trim().trim_end_matches(';');
        if expr.is_empty() {
            return "return;".to_string();
        }
        return format!("return {};", transform_expr(expr));
    }

    transform_expr(s)
}

fn transform_expr(expr: &str) -> String {
    let mut s = expr.to_string();
    s = s.replace("std::sqrt", "sqrt");
    s = s.replace("std::pow", "pow");
    s = s.replace("std::abs", "abs");
    s = s.replace("std::ceil", "ceil");
    s = s.replace("std::floor", "floor");
    s = s.replace("std::max", "max");
    s = s.replace("std::min", "min");
    s = s.replace("std::clamp", "clamp");
    s = s.replace("nullptr", "null");
    s = s.replace("NULL", "null");
    s = s.replace("->", ".");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpp_features_transpile() {
        let temp_dir = std::env::temp_dir();
        let cpp_file = temp_dir.join("test_goraw_features.cpp");

        let src = r#"
struct BaseA {
    int a;
    int get_a() const { return a; }
};

struct BaseB {
    int b;
    int get_b() const { return b; }
};

// Множественное наследование без vtables
struct Derived : public BaseA, public BaseB {
    int c;
    int sum() const { return a + b + c; }
};

// Неинстанциированные шаблоны в заголовках
template <typename T>
T add_template(T x, T y) {
    return x + y;
}

template <typename T>
struct Box {
    T val;
};

int test_all() {
    Derived d = {{10}, {20}, 30};
    int r1 = d.get_a() + d.get_b() + d.sum();

    int x = 100;
    int y = 200;
    auto f = [&x, y](int z) {
        return x + y + z;
    };
    int r2 = f(50);

    return r1 + r2;
}
"#;
        std::fs::write(&cpp_file, src).expect("write cpp");

        let opts = CppToGorawOptions::default();
        let goraw_code = transpile_cpp_file(&cpp_file, &opts).expect("transpile cpp");

        // 1. Проверяем множественное наследование БЕЗ vtables
        assert!(goraw_code.contains("struct Derived {"));
        assert!(goraw_code.contains("fn Derived::get_a("));
        assert!(goraw_code.contains("fn Derived::get_b("));
        assert!(goraw_code.contains("fn Derived::sum("));
        assert!(!goraw_code.contains("vtable") && !goraw_code.contains("__vfptr"));

        // 2. Проверяем неинстанциированные шаблоны в закомментированном виде
        assert!(goraw_code.contains("// [C++ Template - Неинстанциированный шаблон функции: add_template]"));
        assert!(goraw_code.contains("// [C++ Template - Неинстанциированный класс-шаблон: Box]"));

        // 3. Проверяем замыкания со сложным захватом
        assert!(goraw_code.contains("struct Closure_1"));
        assert!(goraw_code.contains("unsafe fn Closure_1::call"));
        assert!(goraw_code.contains("(*self.x)"));
        assert!(goraw_code.contains("self.y"));
        assert!(goraw_code.contains(".call(50)"));

        let _ = std::fs::remove_file(&cpp_file);
    }

    #[test]
    fn test_cpp_standalone_transpile() {
        let cpp_src = r#"
#include <iostream>
#include <cmath>

struct Vec2 {
    double x;
    double y;
    double length_sq() {
        return (x * x) + (y * y);
    }
};

double hypot(double a, double b) {
    return sqrt((a * a) + (b * b));
}

int main(int argc, char** argv) {
    auto h = hypot(3.0, 4.0);
    printf("hypot = %f\n", h);
    return 0;
}
"#;
        let res = transpile_cpp_source_standalone(cpp_src).expect("standalone cpp transpile");
        assert!(res.contains("extern fn printf(arg0: *u8, ...) -> i32;"));
        assert!(res.contains("struct Vec2 {"));
        assert!(res.contains("x: f64,"));
        assert!(res.contains("fn Vec2::length_sq("));
        assert!(res.contains("fn hypot(a: f64, b: f64) -> f64"));
        assert!(res.contains("fn main(argc: i32, argv: *u8) -> i32"));
        assert!(res.contains("let mut h = hypot(3.0, 4.0);"));
    }
}
