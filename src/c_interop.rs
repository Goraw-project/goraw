//! Модуль интеграции с C/C++:
//! - Автообнаружение MSVC и Windows SDK (include / lib)
//! - Компиляция инлайн-блоков `c { ... }` и `cpp { ... }` в LLVM Bitcode (.bc)
//! - Извлечение сигнатур функций и типов через Clang AST JSON dump
//! - Генератор биндингов из C-заголовков (`--bind-c`)
//! - Транслятор C в Goraw (`--port-c`)

use crate::ast::{FnDef, Param, TypeExpr};
use crate::diag::Span;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

#[derive(Clone, Debug, Default)]
pub struct SdkPaths {
    pub includes: Vec<PathBuf>,
    pub libs: Vec<PathBuf>,
}

static SDK_CACHE: OnceLock<SdkPaths> = OnceLock::new();

/// Автоматическое обнаружение установленных Visual Studio и Windows SDK.
pub fn get_sdk_paths() -> &'static SdkPaths {
    SDK_CACHE.get_or_init(|| {
        let mut paths = SdkPaths::default();

        // 1. Поиск MSVC через стандартные каталоги Visual Studio
        let vs_roots = [
            r"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC",
            r"C:\Program Files\Microsoft Visual Studio\2022\Professional\VC\Tools\MSVC",
            r"C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Tools\MSVC",
            r"C:\Program Files (x86)\Microsoft Visual Studio\2019\Community\VC\Tools\MSVC",
            r"C:\Program Files (x86)\Microsoft Visual Studio\2019\Professional\VC\Tools\MSVC",
            r"C:\Program Files (x86)\Microsoft Visual Studio\2019\Enterprise\VC\Tools\MSVC",
        ];

        for root in &vs_roots {
            let p = Path::new(root);
            if p.exists() {
                if let Ok(entries) = std::fs::read_dir(p) {
                    let mut versions: Vec<PathBuf> = entries
                        .filter_map(|e| e.ok().map(|e| e.path()))
                        .filter(|p| p.is_dir())
                        .collect();
                    versions.sort();
                    if let Some(latest) = versions.last() {
                        let inc = latest.join("include");
                        if inc.exists() {
                            paths.includes.push(inc);
                        }
                        let lib = latest.join("lib").join("x64");
                        if lib.exists() {
                            paths.libs.push(lib);
                        }
                        break;
                    }
                }
            }
        }

        // 2. Поиск Windows SDK (Include и Lib)
        let sdk_root = Path::new(r"C:\Program Files (x86)\Windows Kits\10");
        let inc_root = sdk_root.join("Include");
        let lib_root = sdk_root.join("Lib");

        if inc_root.exists() {
            if let Ok(entries) = std::fs::read_dir(&inc_root) {
                let mut versions: Vec<String> = entries
                    .filter_map(|e| {
                        e.ok().and_then(|e| {
                            let name = e.file_name().to_string_lossy().to_string();
                            if name.starts_with("10.") {
                                Some(name)
                            } else {
                                None
                            }
                        })
                    })
                    .collect();
                versions.sort();
                for ver in versions.iter().rev() {
                    let ver_inc = inc_root.join(ver);
                    if ver_inc.join("ucrt").exists() {
                        for sub in &["ucrt", "shared", "um"] {
                            let p = ver_inc.join(sub);
                            if p.exists() {
                                paths.includes.push(p);
                            }
                        }

                        let ver_lib = lib_root.join(ver);
                        for sub in &["ucrt", "um"] {
                            let p = ver_lib.join(sub).join("x64");
                            if p.exists() {
                                paths.libs.push(p);
                            }
                        }
                        break;
                    }
                }
            }
        }

        paths
    })
}

/// Компиляция инлайн C/C++ блока в LLVM Bitcode (.bc).
pub fn compile_inline_snippet(
    code: &str,
    is_cpp: bool,
    clang_path: &str,
    output_bc: &Path,
) -> Result<(), String> {
    let compiler = if is_cpp {
        if clang_path.ends_with("clang.exe") || clang_path == "clang" {
            clang_path.replace("clang.exe", "clang++.exe").replace("clang", "clang++")
        } else {
            clang_path.to_string()
        }
    } else {
        clang_path.to_string()
    };

    let mut cmd = Command::new(&compiler);
    cmd.arg("--target=x86_64-w64-windows-gnu");
    cmd.arg("-c")
        .arg("-x")
        .arg(if is_cpp { "c++" } else { "c" })
        .arg("-emit-llvm")
        .arg("-O2");

    if is_cpp {
        cmd.arg("-fno-exceptions");
    }

    cmd.arg("-o").arg(output_bc);
    cmd.arg("-"); // чтение из stdin

    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("не удалось запустить `{compiler}`: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(code.as_bytes())
            .map_err(|e| format!("ошибка записи в `{compiler}`: {e}"))?;
    }

    let out = child
        .wait_with_output()
        .map_err(|e| format!("ошибка ожидания `{compiler}`: {e}"))?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let lang = if is_cpp { "C++" } else { "C" };
        return Err(format!("ошибка компиляции инлайн {lang} кода:\n{err}"));
    }

    Ok(())
}

/// Извлечение функций из C/C++ кода через Clang AST JSON dump.
pub fn extract_functions_from_code(
    code: &str,
    is_cpp: bool,
    clang_path: &str,
) -> Result<Vec<FnDef>, String> {
    let compiler = if is_cpp {
        if clang_path.ends_with("clang.exe") || clang_path == "clang" {
            clang_path.replace("clang.exe", "clang++.exe").replace("clang", "clang++")
        } else {
            clang_path.to_string()
        }
    } else {
        clang_path.to_string()
    };

    let mut cmd = Command::new(&compiler);
    cmd.arg("--target=x86_64-w64-windows-gnu");
    cmd.arg("-x")
        .arg(if is_cpp { "c++" } else { "c" })
        .arg("-Xclang")
        .arg("-ast-dump=json")
        .arg("-fsyntax-only");

    cmd.arg("-");

    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("не удалось запустить `{compiler}`: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(code.as_bytes());
    }

    let out = child
        .wait_with_output()
        .map_err(|e| format!("ошибка ожидания `{compiler}`: {e}"))?;

    let json_str = String::from_utf8_lossy(&out.stdout);
    if json_str.trim().is_empty() {
        return Ok(Vec::new());
    }

    let v: Value = serde_json::from_str(&json_str)
        .map_err(|e| format!("ошибка парсинга AST JSON от Clang: {e}"))?;

    let mut fns = Vec::new();
    if let Some(inner) = v.get("inner").and_then(|i| i.as_array()) {
        for node in inner {
            collect_functions_recursive(node, &mut fns);
        }
    }

    Ok(fns)
}

fn collect_functions_recursive(node: &Value, fns: &mut Vec<FnDef>) {
    let kind = node.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    if kind == "FunctionDecl" {
        if let Some(f) = parse_ast_function_node(node) {
            fns.push(f);
        }
    } else if kind == "LinkageSpecDecl" || kind == "NamespaceDecl" {
        if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
            for child in inner {
                collect_functions_recursive(child, fns);
            }
        }
    }
}

fn parse_ast_function_node(node: &Value) -> Option<FnDef> {
    let kind = node.get("kind")?.as_str()?;
    if kind != "FunctionDecl" {
        return None;
    }

    let name = node.get("name")?.as_str()?.to_string();
    if name.starts_with('_') {
        return None; // пропускаем внутренние CRT и компиляторные интринзики
    }

    // Проверяем: если узел пришёл из внешнего файла (include / SDK), пропускаем его
    if let Some(loc) = node.get("loc") {
        if let Some(file) = loc.get("file").and_then(|f| f.as_str()) {
            if file.contains("include") || file.contains("Include") || file.contains("MSVC") || file.contains("Windows Kits") {
                return None;
            }
        }
        if loc.get("includedFrom").is_some() {
            return None;
        }
    }

    // Только функции с телом (определённые пользователем в блоке)
    let has_body = node
        .get("inner")
        .and_then(|i| i.as_array())
        .map(|arr| {
            arr.iter().any(|n| {
                matches!(
                    n.get("kind").and_then(|k| k.as_str()),
                    Some("CompoundStmt")
                )
            })
        })
        .unwrap_or(false);

    if !has_body {
        return None;
    }

    let qual_type = node.get("type")?.get("qualType")?.as_str()?;
    let ret_type = parse_c_return_type(qual_type);

    let mut params = Vec::new();
    if let Some(inner) = node.get("inner").and_then(|i| i.as_array()) {
        for param_node in inner {
            if param_node.get("kind").and_then(|k| k.as_str()) == Some("ParmVarDecl") {
                let pname = param_node
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("arg")
                    .to_string();
                let ptype_str = param_node
                    .get("type")
                    .and_then(|t| t.get("qualType"))
                    .and_then(|q| q.as_str())
                    .unwrap_or("void");
                params.push(Param {
                    name: pname,
                    ty: map_c_type(ptype_str),
                    span: Span::dummy(),
                });
            }
        }
    }

    Some(FnDef {
        name,
        type_params: Vec::new(),
        params,
        variadic: qual_type.contains("..."),
        ret: if ret_type.is_void() { None } else { Some(ret_type) },
        body: None,
        is_unsafe: false,
        is_extern: true,
        is_test: false,
        span: Span::dummy(),
    })
}

fn parse_c_return_type(sig: &str) -> TypeExpr {
    // Пример сигнатуры: "int (int, int)" или "char *(const char *, int)"
    if let Some(idx) = sig.find('(') {
        let ret_str = sig[..idx].trim();
        map_c_type(ret_str)
    } else {
        TypeExpr::Named("void".into(), Span::dummy())
    }
}

pub fn map_c_type(c_ty: &str) -> TypeExpr {
    let s = c_ty.trim();
    if s.ends_with('*') {
        let base = s[..s.len() - 1].trim();
        let is_const = base.starts_with("const ");
        let clean_base = base.replace("const ", "").replace("volatile ", "");
        let inner = map_c_type(clean_base.trim());
        if is_const {
            TypeExpr::Ptr(Box::new(inner), Span::dummy())
        } else {
            TypeExpr::PtrMut(Box::new(inner), Span::dummy())
        }
    } else {
        let clean = s.replace("const ", "").replace("volatile ", "");
        let clean = clean.trim();
        match clean {
            "int" | "signed int" | "int32_t" | "long" | "signed long" => {
                TypeExpr::Named("i32".into(), Span::dummy())
            }
            "unsigned int" | "uint32_t" | "unsigned long" => {
                TypeExpr::Named("u32".into(), Span::dummy())
            }
            "long long" | "signed long long" | "int64_t" | "ssize_t" | "intptr_t" | "long int" => {
                TypeExpr::Named("i64".into(), Span::dummy())
            }
            "unsigned long long" | "uint64_t" | "size_t" | "uintptr_t" => {
                TypeExpr::Named("u64".into(), Span::dummy())
            }
            "short" | "signed short" | "int16_t" => TypeExpr::Named("i16".into(), Span::dummy()),
            "unsigned short" | "uint16_t" => TypeExpr::Named("u16".into(), Span::dummy()),
            "char" | "signed char" | "int8_t" => TypeExpr::Named("i8".into(), Span::dummy()),
            "unsigned char" | "uint8_t" | "byte" => TypeExpr::Named("u8".into(), Span::dummy()),
            "float" => TypeExpr::Named("f32".into(), Span::dummy()),
            "double" => TypeExpr::Named("f64".into(), Span::dummy()),
            "bool" | "_Bool" => TypeExpr::Named("bool".into(), Span::dummy()),
            "void" => TypeExpr::Named("void".into(), Span::dummy()),
            other => TypeExpr::Named(other.replace(' ', "_"), Span::dummy()),
        }
    }
}

impl TypeExpr {
    fn is_void(&self) -> bool {
        matches!(self, TypeExpr::Named(n, _) if n == "void")
    }
}

/// Генерация файла биндингов `.gw` из C-заголовка.
pub fn generate_bindings_from_header(header_path: &Path, clang_path: &str) -> Result<String, String> {
    let header_src = std::fs::read_to_string(header_path)
        .map_err(|e| format!("не удалось прочитать `{}`: {e}", header_path.display()))?;

    let fns = extract_functions_from_code(&header_src, false, clang_path)?;
    let mut out = String::new();
    out.push_str(&format!("// Автоматические биндинги Goraw для `{}`\n\n", header_path.display()));

    for f in fns {
        out.push_str("extern fn ");
        out.push_str(&f.name);
        out.push('(');
        for (i, p) in f.params.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&p.name);
            out.push_str(": ");
            out.push_str(&format_type_expr(&p.ty));
        }
        if f.variadic {
            if !f.params.is_empty() {
                out.push_str(", ");
            }
            out.push_str("...");
        }
        out.push(')');
        if let Some(ret) = &f.ret {
            out.push_str(" -> ");
            out.push_str(&format_type_expr(ret));
        }
        out.push_str(";\n");
    }

    Ok(out)
}

fn format_type_expr(t: &TypeExpr) -> String {
    match t {
        TypeExpr::Named(n, _) => n.clone(),
        TypeExpr::Generic(n, args, _) => {
            let inner: Vec<String> = args.iter().map(format_type_expr).collect();
            format!("{n}<{}>", inner.join(", "))
        }
        TypeExpr::Ptr(inner, _) => format!("*{}", format_type_expr(inner)),
        TypeExpr::PtrMut(inner, _) => format!("*mut {}", format_type_expr(inner)),
        TypeExpr::Slice(inner, _) => format!("[]{}", format_type_expr(inner)),
        TypeExpr::Array(inner, n, _) => format!("[{}]{}", n, format_type_expr(inner)),
        TypeExpr::Fn(params, ret, _) => {
            let plist: Vec<String> = params.iter().map(format_type_expr).collect();
            let r = match ret {
                Some(rt) => format!(" -> {}", format_type_expr(rt)),
                None => String::new(),
            };
            format!("fn({}){}", plist.join(", "), r)
        }
    }
}
