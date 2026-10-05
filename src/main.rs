//! Драйвер компилятора Goraw.
//!
//! Пайплайн: исходник -> лексер -> парсер -> сбор типов -> кодоген (LLVM IR)
//! -> clang -> .exe. Диагностики умеет печатать по-человечески или в
//! LLM-дружественном JSON (`--json`).

use gorawc::{ast, codegen, diag, lexer, parser, types};

use std::path::{Path, PathBuf};
use std::io::Write;
use std::process::{exit, Command};

/// Рантайм JIT-специализации встроен в компилятор и разворачивается рядом с
/// .ll только когда программа реально использует jit-блоки.
const JIT_RUNTIME_C: &str = include_str!("../runtime/goraw_jit.c");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Debug,
    Release,
}

struct Options {
    input: Option<PathBuf>,
    bind_c: Option<PathBuf>,
    extra_sources: Vec<PathBuf>,
    extra_objects: Vec<PathBuf>,
    extra_asms: Vec<PathBuf>,
    extra_protos: Vec<PathBuf>,
    output: Option<PathBuf>,
    emit_llvm: bool,   // остановиться на .ll
    emit_cpp: bool,    // транслировать в C++23 (.cpp)
    emit_gw: bool,     // транслировать в Goraw (.gw)
    emit_opcodes: bool, // извлечь массив опкодов x86-64
    opcode_format: gorawc::opcodes::OpcodeFormat, // формат опкодов
    json: bool,        // диагностика в JSON
    run: bool,         // запустить после сборки
    opt: Option<String>, // уровень оптимизации, напр. "2"
    clang: String,
    keep_ll: bool,
    test: bool, // собрать и прогнать shadow-тесты
    shadow_strict: bool, // строгий режим обязательных shadow-тестов
    obfuscate_strings: bool, // встроенная обфускация строковых литералов
    cpp_std: String, // стандарт C++ для инлайн-вставок (по умолчанию c++23)
    c_std: String,   // стандарт C для инлайн-вставок (по умолчанию c23)
    silent: bool,    // авто-байпас предупреждений безопасности песочницы
    profile: Profile, // профиль сборки (Debug / Release)
    target: String,  // целевой triple платформы (напр. x86_64-w64-windows-gnu)
    from_cpp: Option<PathBuf>, // транслировать C++23 код (.cpp) обратно в Goraw (.gw)
    from_llvm: Option<PathBuf>, // транслировать LLVM IR (.ll) в Goraw (.gw)
    native_linker: bool, // автономный встроенный PE/COFF компоновщик
    native_backend: bool, // автономный встроенный x86-64 кодогенератор
    emit_asm: bool,      // сгенерировать x86-64 ассемблер (.asm, -S)
    emit_obj: bool,      // скомпилировать только в объектный файл (.obj, -c)
}

pub fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "transpile" | "xlat" => exit(handle_transpile_subcommand(&args[2..])),
            "from-cpp" | "c++" | "port-cpp" | "cpp-to-gw" => exit(handle_from_cpp_subcommand(&args[2..])),
            "from-llvm" | "llvm" | "port-llvm" | "llvm-to-gw" => exit(handle_from_llvm_subcommand(&args[2..])),
            "opcodes" | "to-opcodes" | "emit-opcodes" => exit(handle_opcodes_subcommand(&args[2..])),
            "proto" | "pb" => exit(handle_proto_subcommand(&args[2..])),
            "lsp" | "--lsp" => {
                if let Err(e) = gorawc::lsp::run_lsp_server() {
                    eprintln!("ошибка LSP сервера: {e}");
                    exit(1);
                }
                exit(0);
            }
            "init" => {
                let name = args.get(2).map(|s| s.as_str());
                let cwd = match std::env::current_dir() {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("не удалось определить текущую директорию: {e}");
                        exit(1);
                    }
                };
                if let Err(e) = gorawc::pkg::init_project(name, &cwd) {
                    eprintln!("ошибка инициализации проекта: {e}");
                    exit(1);
                }
                exit(0);
            }
            "get" => {
                let mut dep_spec = None;
                let mut is_global = false;
                for a in &args[2..] {
                    if a == "-g" || a == "--global" {
                        is_global = true;
                    } else if !a.starts_with('-') && dep_spec.is_none() {
                        dep_spec = Some(a.as_str());
                    }
                }
                if let Err(e) = gorawc::pkg::get(dep_spec, is_global) {
                    eprintln!("ошибка goraw get: {e}");
                    exit(1);
                }
                exit(0);
            }
            "vendor" => {
                if let Err(e) = gorawc::pkg::get(None, false) {
                    eprintln!("ошибка вендоринга: {e}");
                    exit(1);
                }
                exit(0);
            }
            "build" => {
                args.remove(1);
                check_manifest_entry(&mut args);
            }
            "run" => {
                args.remove(1);
                if !args.iter().any(|a| a == "--run") {
                    args.push("--run".to_string());
                }
                check_manifest_entry(&mut args);
            }
            "test" => {
                args.remove(1);
                if !args.iter().any(|a| a == "--test" || a == "test") {
                    args.push("--test".to_string());
                }
                check_manifest_entry(&mut args);
            }
            "to-cpp" | "cpp" => {
                args.remove(1);
                if !args.iter().any(|a| a == "--emit-cpp") {
                    args.push("--emit-cpp".to_string());
                }
                check_manifest_entry(&mut args);
            }
            "to-all" | "emit-all" => {
                args.remove(1);
                if !args.iter().any(|a| a == "--emit-cpp") {
                    args.push("--emit-cpp".to_string());
                }
                if !args.iter().any(|a| a == "--emit-llvm") {
                    args.push("--emit-llvm".to_string());
                }
                check_manifest_entry(&mut args);
            }
            _ => {}
        }
    } else {
        check_manifest_entry(&mut args);
    }

    let opts = match parse_args(&args) {
        Ok(o) => o,
        Err(msg) => {
            eprintln!("{msg}");
            exit(2);
        }
    };

    if let Some(header) = &opts.bind_c {
        match gorawc::c_interop::generate_bindings_from_header(header, &opts.clang) {
            Ok(bindings) => {
                if let Some(out) = &opts.output {
                    if let Err(e) = std::fs::write(out, &bindings) {
                        eprintln!("не удалось записать `{}`: {e}", out.display());
                        exit(2);
                    }
                    eprintln!("биндинги сохранены в `{}`", out.display());
                } else {
                    print!("{bindings}");
                }
                exit(0);
            }
            Err(e) => {
                eprintln!("{e}");
                exit(1);
            }
        }
    }

    if let Some(cpp_file) = &opts.from_cpp {
        let cpp_opts = gorawc::cpp_to_goraw::CppToGorawOptions {
            clang_path: Some(opts.clang.clone()),
            cpp_std: Some(opts.cpp_std.clone()),
            target_triple: Some(opts.target.clone()),
            extra_includes: Vec::new(),
        };
        match gorawc::cpp_to_goraw::transpile_cpp_file(cpp_file, &cpp_opts) {
            Ok(code) => {
                if let Some(out) = &opts.output {
                    if out.to_str() == Some("-") {
                        print!("{code}");
                        exit(0);
                    }
                }
                let out = opts.output.clone().unwrap_or_else(|| cpp_file.with_extension("gw"));
                if let Err(e) = std::fs::write(&out, &code) {
                    eprintln!("не удалось записать `{}`: {e}", out.display());
                    exit(2);
                }
                eprintln!("сгенерирован исходный код Goraw: `{}`", out.display());
                exit(0);
            }
            Err(e) => {
                eprintln!("ошибка трансляции C++ в Goraw:\n{e}");
                exit(1);
            }
        }
    }

    if let Some(llvm_file) = &opts.from_llvm {
        let llvm_opts = gorawc::llvm_to_goraw::LlvmToGorawOptions::default();
        match gorawc::llvm_to_goraw::transpile_llvm_file(llvm_file, &llvm_opts) {
            Ok(code) => {
                if let Some(out) = &opts.output {
                    if out.to_str() == Some("-") {
                        print!("{code}");
                        exit(0);
                    }
                }
                let out = opts.output.clone().unwrap_or_else(|| llvm_file.with_extension("gw"));
                if let Err(e) = std::fs::write(&out, &code) {
                    eprintln!("не удалось записать `{}`: {e}", out.display());
                    exit(2);
                }
                eprintln!("сгенерирован исходный код Goraw: `{}`", out.display());
                exit(0);
            }
            Err(e) => {
                eprintln!("ошибка трансляции LLVM IR в Goraw:\n{e}");
                exit(1);
            }
        }
    }

    exit(run(opts));
}

fn handle_transpile_subcommand(args: &[String]) -> i32 {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut clang: Option<String> = None;
    let mut cpp_std: Option<String> = None;
    let mut target: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!(
                    "goraw transpile — универсальный транспилятор между C++, Goraw и LLVM IR (.cpp <-> .gw <-> .ll)\n\n\
                     ИСПОЛЬЗОВАНИЕ:\n    goraw transpile <входной_файл> [-o выходной_файл]\n\n\
                     ПОДДЕРЖИВАЕМЫЕ НАПРАВЛЕНИЯ:\n\
                         .cpp -> .gw (C++23 в Goraw)\n\
                         .cpp -> .ll (C++23 в LLVM IR)\n\
                         .gw  -> .cpp (Goraw в C++23)\n\
                         .gw  -> .ll (Goraw в LLVM IR)\n\
                         .ll  -> .gw (LLVM IR в Goraw)\n\
                         .ll  -> .cpp (LLVM IR в C++23)\n\n\
                     ОПЦИИ:\n    -o <путь>      имя выходного файла ('-' для stdout)\n    -h, --help     показать справку"
                );
                return 0;
            }
            "-o" => {
                i += 1;
                output = Some(PathBuf::from(match args.get(i) {
                    Some(o) => o,
                    None => {
                        eprintln!("-o требует аргумент");
                        return 2;
                    }
                }));
            }
            "--clang" => {
                i += 1;
                clang = args.get(i).cloned();
            }
            "--std" => {
                i += 1;
                cpp_std = args.get(i).cloned();
            }
            "--target" => {
                i += 1;
                target = args.get(i).cloned();
            }
            s if s.starts_with('-') => {
                eprintln!("неизвестная опция `{s}` (см. goraw transpile --help)");
                return 2;
            }
            s => {
                if input.is_none() {
                    input = Some(PathBuf::from(s));
                } else {
                    eprintln!("лишний аргумент `{s}`");
                    return 2;
                }
            }
        }
        i += 1;
    }

    let input = match input {
        Some(p) => p,
        None => {
            eprintln!("не указан входной файл (см. goraw transpile --help)");
            return 2;
        }
    };

    let in_ext = input.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let out_ext = output.as_ref().and_then(|o| o.extension()).and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    match in_ext.as_str() {
        "cpp" | "cc" | "cxx" => {
            let cpp_opts = gorawc::cpp_to_goraw::CppToGorawOptions {
                clang_path: clang,
                cpp_std: cpp_std.or_else(|| Some("c++23".into())),
                target_triple: target,
                extra_includes: Vec::new(),
            };
            let gw_code = match gorawc::cpp_to_goraw::transpile_cpp_file(&input, &cpp_opts) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("ошибка трансляции C++:\n{e}");
                    return 1;
                }
            };
            if out_ext == "ll" {
                let mut diags = gorawc::diag::Diags::new(input.display().to_string(), gw_code.clone());
                let mut lx = gorawc::lexer::Lexer::new(&gw_code);
                let toks = lx.tokenize(&mut diags);
                let mut p = gorawc::parser::Parser::new(toks, &gw_code, &mut diags);
                let mut prog = p.parse_program();
                gorawc::mono::monomorphize(&mut prog);
                let mut collected = Vec::new();
                let ctx = gorawc::types::collect(&prog.structs, &prog.enums, &prog.fns, &mut collected);
                diags.items.extend(collected);
                let cg = gorawc::codegen::Codegen::new(&ctx, &mut diags);
                let ir = cg.emit_module(&prog);
                return write_or_print(output.as_ref(), &ir, &input.with_extension("ll"));
            } else {
                return write_or_print(output.as_ref(), &gw_code, &input.with_extension("gw"));
            }
        }
        "ll" => {
            let llvm_opts = gorawc::llvm_to_goraw::LlvmToGorawOptions::default();
            let gw_code = match gorawc::llvm_to_goraw::transpile_llvm_file(&input, &llvm_opts) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("ошибка трансляции LLVM IR:\n{e}");
                    return 1;
                }
            };
            if out_ext == "cpp" {
                let mut diags = gorawc::diag::Diags::new(input.display().to_string(), gw_code.clone());
                let mut lx = gorawc::lexer::Lexer::new(&gw_code);
                let toks = lx.tokenize(&mut diags);
                let mut p = gorawc::parser::Parser::new(toks, &gw_code, &mut diags);
                let mut prog = p.parse_program();
                gorawc::mono::monomorphize(&mut prog);
                let cpp_code = match gorawc::cpp_transpiler::transpile(&prog, false, &[]) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("ошибка трансляции в C++: {e}");
                        return 1;
                    }
                };
                return write_or_print(output.as_ref(), &cpp_code, &input.with_extension("cpp"));
            } else {
                return write_or_print(output.as_ref(), &gw_code, &input.with_extension("gw"));
            }
        }
        "gw" => {
            let src = match std::fs::read_to_string(&input) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("не удалось прочитать `{}`: {e}", input.display());
                    return 2;
                }
            };
            let mut diags = gorawc::diag::Diags::new(input.display().to_string(), src.clone());
            let mut lx = gorawc::lexer::Lexer::new(&src);
            let toks = lx.tokenize(&mut diags);
            let mut p = gorawc::parser::Parser::new(toks, &src, &mut diags);
            let mut prog = p.parse_program();
            gorawc::mono::monomorphize(&mut prog);

            if out_ext == "cpp" {
                let cpp_code = match gorawc::cpp_transpiler::transpile(&prog, false, &[]) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("ошибка трансляции в C++: {e}");
                        return 1;
                    }
                };
                return write_or_print(output.as_ref(), &cpp_code, &input.with_extension("cpp"));
            } else {
                let mut collected = Vec::new();
                let ctx = gorawc::types::collect(&prog.structs, &prog.enums, &prog.fns, &mut collected);
                diags.items.extend(collected);
                let cg = gorawc::codegen::Codegen::new(&ctx, &mut diags);
                let ir = cg.emit_module(&prog);
                return write_or_print(output.as_ref(), &ir, &input.with_extension("ll"));
            }
        }
        other => {
            eprintln!("нераспознанное расширение файла `.{other}`. Ожидается .cpp, .gw или .ll");
            return 2;
        }
    }
}

fn write_or_print(output: Option<&PathBuf>, content: &str, default_path: &Path) -> i32 {
    if let Some(out) = output {
        if out.to_str() == Some("-") {
            print!("{content}");
            return 0;
        }
    }
    let target = output.cloned().unwrap_or_else(|| default_path.to_path_buf());
    if let Err(e) = std::fs::write(&target, content) {
        eprintln!("не удалось записать `{}`: {e}", target.display());
        return 2;
    }
    eprintln!("результат сохранен в `{}`", target.display());
    0
}

fn handle_from_cpp_subcommand(args: &[String]) -> i32 {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut clang: Option<String> = None;
    let mut cpp_std: Option<String> = None;
    let mut target: Option<String> = None;

    let mut extra_includes: Vec<PathBuf> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!(
                    "goraw from-cpp — транслятор C++23 исходного кода обратно в Goraw (.gw)\n\n\
                     ИСПОЛЬЗОВАНИЕ:\n    goraw from-cpp <файл.cpp> [-o out.gw] [--std c++23] [-I <include_path>] [--clang <путь>]\n\n\
                     ОПЦИИ:\n    -o <путь>      имя выходного файла (по умолчанию <файл>.gw, '-' для stdout)\n    -I <путь>      дополнительный путь поиска заголовков\n    --std <версия> стандарт C++ (по умолчанию c++23)\n    --clang <путь> путь к компилятору clang++\n    --target <trp> целевой target triple\n    -h, --help     показать справку"
                );
                return 0;
            }
            "-o" => {
                i += 1;
                match args.get(i) {
                    Some(o) => output = Some(PathBuf::from(o)),
                    None => {
                        eprintln!("-o требует аргумент");
                        return 2;
                    }
                }
            }
            "-I" => {
                i += 1;
                match args.get(i) {
                    Some(inc) => extra_includes.push(PathBuf::from(inc)),
                    None => {
                        eprintln!("-I требует аргумент");
                        return 2;
                    }
                }
            }
            "--std" | "--cpp-std" => {
                i += 1;
                match args.get(i) {
                    Some(s) => cpp_std = Some(s.clone()),
                    None => {
                        eprintln!("--std требует аргумент (напр. c++23)");
                        return 2;
                    }
                }
            }
            "--clang" => {
                i += 1;
                match args.get(i) {
                    Some(c) => clang = Some(c.clone()),
                    None => {
                        eprintln!("--clang требует аргумент");
                        return 2;
                    }
                }
            }
            "--target" => {
                i += 1;
                match args.get(i) {
                    Some(t) => target = Some(t.clone()),
                    None => {
                        eprintln!("--target требует аргумент");
                        return 2;
                    }
                }
            }
            s if s.starts_with("-I") => {
                extra_includes.push(PathBuf::from(&s[2..]));
            }
            s if s.starts_with('-') => {
                eprintln!("неизвестная опция `{s}` (см. goraw from-cpp --help)");
                return 2;
            }
            s => {
                if input.is_none() {
                    input = Some(PathBuf::from(s));
                } else {
                    eprintln!("лишний аргумент `{s}`");
                    return 2;
                }
            }
        }
        i += 1;
    }

    let input = match input {
        Some(p) => p,
        None => {
            eprintln!("не указан входной файл .cpp (см. goraw from-cpp --help)");
            return 2;
        }
    };

    let opts = gorawc::cpp_to_goraw::CppToGorawOptions {
        clang_path: clang.or_else(|| Some(gorawc::c_interop::find_clang(true))),
        cpp_std,
        target_triple: target,
        extra_includes,
    };

    match gorawc::cpp_to_goraw::transpile_cpp_file(&input, &opts) {
        Ok(code) => {
            if let Some(out) = &output {
                if out.to_str() == Some("-") {
                    print!("{code}");
                    return 0;
                }
            }
            let out = output.unwrap_or_else(|| input.with_extension("gw"));
            if let Err(e) = std::fs::write(&out, &code) {
                eprintln!("не удалось записать `{}`: {e}", out.display());
                return 2;
            }
            eprintln!("сгенерирован исходный код Goraw: `{}`", out.display());
            0
        }
        Err(e) => {
            eprintln!("ошибка трансляции C++ в Goraw:\n{e}");
            1
        }
    }
}

fn handle_from_llvm_subcommand(args: &[String]) -> i32 {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut verbose = false;
    let mut structured = true;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!(
                    "goraw from-llvm — транслятор LLVM IR (.ll) обратно в Goraw (.gw)\n\n\
                     ИСПОЛЬЗОВАНИЕ:\n    goraw from-llvm <файл.ll> [-o out.gw] [--raw-cfg] [-v, --verbose]\n\n\
                     ОПЦИИ:\n    -o <путь>      имя выходного файла (по умолчанию <файл>.gw, '-' для stdout)\n    --raw-cfg      отключить структурный лифтинг (только basic block dispatch loop)\n    -v, --verbose  подробный вывод транслятора\n    -h, --help     показать справку"
                );
                return 0;
            }
            "-o" => {
                i += 1;
                match args.get(i) {
                    Some(o) => output = Some(PathBuf::from(o)),
                    None => {
                        eprintln!("-o требует аргумент");
                        return 2;
                    }
                }
            }
            "--raw-cfg" => {
                structured = false;
            }
            "-v" | "--verbose" => {
                verbose = true;
            }
            s if s.starts_with('-') => {
                eprintln!("неизвестная опция `{s}` (см. goraw from-llvm --help)");
                return 2;
            }
            s => {
                if input.is_none() {
                    input = Some(PathBuf::from(s));
                } else {
                    eprintln!("лишний аргумент `{s}`");
                    return 2;
                }
            }
        }
        i += 1;
    }

    let input = match input {
        Some(p) => p,
        None => {
            eprintln!("не указан входной файл .ll (см. goraw from-llvm --help)");
            return 2;
        }
    };

    let opts = gorawc::llvm_to_goraw::LlvmToGorawOptions {
        verbose,
        emit_comments: true,
        structured_cfg: structured,
    };

    match gorawc::llvm_to_goraw::transpile_llvm_file(&input, &opts) {
        Ok(code) => {
            if let Some(out) = &output {
                if out.to_str() == Some("-") {
                    print!("{code}");
                    return 0;
                }
            }
            let out = output.unwrap_or_else(|| input.with_extension("gw"));
            if let Err(e) = std::fs::write(&out, &code) {
                eprintln!("не удалось записать `{}`: {e}", out.display());
                return 2;
            }
            eprintln!("сгенерирован исходный код Goraw: `{}`", out.display());
            0
        }
        Err(e) => {
            eprintln!("ошибка трансляции LLVM IR в Goraw:\n{e}");
            1
        }
    }
}

fn handle_proto_subcommand(args: &[String]) -> i32 {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut json = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!(
                    "goraw proto — компилятор Protobuf (Editions / proto2 / proto3) в Goraw\n\n\
                     ИСПОЛЬЗОВАНИЕ:\n    goraw proto <схема.proto> [-o out.gw] [--json]\n    goraw pb    <схема.proto> [-o out.gw] [--json]\n\n\
                     ОПЦИИ:\n    -o <путь>   имя выходного файла (по умолчанию <схема>.gw, '-' для stdout)\n    --json      вывод диагностик в JSON\n    -h, --help  показать справку"
                );
                return 0;
            }
            "-o" => {
                i += 1;
                match args.get(i) {
                    Some(o) => output = Some(PathBuf::from(o)),
                    None => {
                        eprintln!("-o требует аргумент");
                        return 2;
                    }
                }
            }
            "--json" => json = true,
            s if s.starts_with('-') => {
                eprintln!("неизвестная опция `{s}` (см. goraw proto --help)");
                return 2;
            }
            s => {
                if input.is_none() {
                    input = Some(PathBuf::from(s));
                } else {
                    eprintln!("лишний аргумент `{s}`");
                    return 2;
                }
            }
        }
        i += 1;
    }

    let input = match input {
        Some(p) => p,
        None => {
            eprintln!("не указан входной файл .proto (см. goraw proto --help)");
            return 2;
        }
    };
    let src = match std::fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("не удалось прочитать `{}`: {e}", input.display());
            return 2;
        }
    };

    let file = input.display().to_string();
    let (code, diags) = gorawc::proto::compile(&file, &src);

    if !diags.items.is_empty() {
        if json {
            print!("{}", diags.render_llm_json());
        } else {
            eprint!("{}", diags.render_human());
        }
    }

    let code = match code {
        Some(c) => c,
        None => return 1,
    };

    if let Some(out) = &output {
        if out.to_str() == Some("-") {
            print!("{code}");
            return 0;
        }
    }

    let out = output.unwrap_or_else(|| input.with_extension("gw"));
    if let Err(e) = std::fs::write(&out, &code) {
        eprintln!("не удалось записать `{}`: {e}", out.display());
        return 2;
    }
    eprintln!("сгенерировано: `{}`", out.display());
    0
}

fn handle_opcodes_subcommand(args: &[String]) -> i32 {
    use std::io::Write;
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut format = gorawc::opcodes::OpcodeFormat::Goraw;
    let mut func_filter: Option<String> = None;
    let mut opt_level = "2".to_string();
    let mut target = if cfg!(windows) { "x86_64-pc-windows-msvc".to_string() } else { "x86_64-unknown-linux-gnu".to_string() };
    let mut clang: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!(
                    "goraw opcodes — извлечение массива опкодов (x86-64 machine code) из .gw, .ll или .asm\n\n\
                     ИСПОЛЬЗОВАНИЕ:\n    goraw opcodes <файл.gw|файл.ll|файл.asm> [опции]\n\n\
                     ОПЦИИ:\n    -o <путь>         имя выходного файла ('-' для вывода в stdout, по умолчанию stdout)\n    --format <формат> формат представления опкодов:\n                      goraw (массив const NAME_OPCODES: [u8; N] = [0x...], по умолчанию)\n                      c     (массив C/C++ const unsigned char name_opcodes[] = {{ ... }})\n                      rust  (массив Rust pub const NAME_OPCODES: &[u8] = &[ ... ])\n                      hex   (шеллакод-строка \\x48\\x89...)\n                      asm   (дизассемблер с опкодами через iced-x86)\n                      bin   (сырой бинарный файл опкодов)\n    --func <имя>      извлечь опкоды только для указанной функции\n    -O<n>             уровень оптимизации (0, 1, 2, 3; по умолчанию 2)\n    --target <triple> целевой target triple\n    --clang <путь>    путь к компилятору clang\n    -h, --help        показать эту справку"
                );
                return 0;
            }
            "-o" => {
                i += 1;
                match args.get(i) {
                    Some(o) => output = Some(PathBuf::from(o)),
                    None => {
                        eprintln!("-o требует аргумент");
                        return 2;
                    }
                }
            }
            "--format" => {
                i += 1;
                match args.get(i) {
                    Some(f) => {
                        match gorawc::opcodes::OpcodeFormat::from_str(f) {
                            Some(fmt) => format = fmt,
                            None => {
                                eprintln!("неизвестный формат `{f}` (доступны: goraw, c, rust, hex, asm, bin)");
                                return 2;
                            }
                        }
                    }
                    None => {
                        eprintln!("--format требует аргумент");
                        return 2;
                    }
                }
            }
            "--func" => {
                i += 1;
                match args.get(i) {
                    Some(f) => func_filter = Some(f.clone()),
                    None => {
                        eprintln!("--func требует аргумент");
                        return 2;
                    }
                }
            }
            "-r" | "--release" => {
                opt_level = "3".to_string();
            }
            "--debug" => {
                opt_level = "0".to_string();
            }
            s if s.starts_with("-O") => {
                opt_level = s.trim_start_matches("-O").to_string();
            }
            "--target" => {
                i += 1;
                match args.get(i) {
                    Some(t) => target = t.clone(),
                    None => {
                        eprintln!("--target требует аргумент");
                        return 2;
                    }
                }
            }
            "--clang" => {
                i += 1;
                match args.get(i) {
                    Some(c) => clang = Some(c.clone()),
                    None => {
                        eprintln!("--clang требует аргумент");
                        return 2;
                    }
                }
            }
            s if s.starts_with('-') => {
                eprintln!("неизвестная опция `{s}` (см. goraw opcodes --help)");
                return 2;
            }
            s => {
                if input.is_none() {
                    input = Some(PathBuf::from(s));
                } else {
                    eprintln!("лишний аргумент `{s}`");
                    return 2;
                }
            }
        }
        i += 1;
    }

    let input = match input {
        Some(p) => p,
        None => {
            eprintln!("не указан входной файл (см. goraw opcodes --help)");
            return 2;
        }
    };

    let clang = clang.unwrap_or_else(|| gorawc::c_interop::find_clang(false));

    let report_res = match input.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "gw" => gorawc::opcodes::extract_opcodes_from_gw(&input, &clang, &opt_level, &target),
        "ll" => gorawc::opcodes::extract_opcodes_from_ll(&input, &clang, &opt_level, &target),
        "asm" => gorawc::opcodes::extract_opcodes_from_asm(&input),
        "obj" | "o" => {
            let bytes = std::fs::read(&input).map_err(|e| format!("ошибка чтения `{}`: {e}", input.display()));
            bytes.and_then(|b| gorawc::opcodes::extract_opcodes_from_obj(&b))
        }
        ext => {
            eprintln!("неподдерживаемое расширение файла `.{ext}` (ожидалось .gw, .ll, .asm или .obj)");
            return 2;
        }
    };

    let report = match report_res {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };

    let rendered = match report.render(format, func_filter.as_deref()) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("ошибка форматирования опкодов: {e}");
            return 1;
        }
    };

    if let Some(out) = &output {
        if out.to_str() == Some("-") {
            let _ = std::io::stdout().write_all(&rendered);
            return 0;
        }
        if let Err(e) = std::fs::write(out, &rendered) {
            eprintln!("не удалось записать `{}`: {e}", out.display());
            return 2;
        }
        eprintln!("опкоды успешно сохранены в `{}`", out.display());
    } else {
        let _ = std::io::stdout().write_all(&rendered);
    }

    0
}

fn check_manifest_entry(args: &mut Vec<String>) {
    let has_file = args.iter().skip(1).any(|a| {
        !a.starts_with('-') && (a.ends_with(".gw") || a.ends_with(".proto") || a.ends_with(".obj") || a.ends_with(".asm") || a.ends_with(".cpp") || a.ends_with(".cc") || a.ends_with(".cxx"))
    });
    if !has_file {
        if let Ok(cwd) = std::env::current_dir() {
            if let Some(manifest) = gorawc::pkg::load_manifest(&cwd) {
                if let Some(pkg) = manifest.package {
                    let entry_str = pkg.entry.unwrap_or_else(|| "src/main.gw".to_string());
                    let entry_path = cwd.join(&entry_str);
                    if entry_path.exists() {
                        args.insert(1, entry_str);
                        return;
                    }
                }
            }
        }
        if args.len() <= 1 {
            print_help();
            exit(0);
        }
    }
}

fn print_help() {
    println!(
        "goraw — компилятор и пакетный менеджер языка Goraw (LLVM backend)\n\
\n\
ИСПОЛЬЗОВАНИЕ:\n\
    goraw [подкоманда] [опции]\n\
    goraw <файл.gw> [schema.proto ...] [helper.asm ...] [lib.obj ...] [опции]\n\
\n\
ПОДКОМАНДЫ:\n\
    init [имя]       инициализировать новый проект (goraw.toml, src/main.gw)\n\
    get [url] [-g]   скачать зависимость в ./vendor/ (или -g в ~/.goraw/pkg/)\n\
    vendor           синхронизировать все зависимости из goraw.toml в ./vendor/\n\
    build            собрать проект из goraw.toml\n\
    run              собрать и запустить проект из goraw.toml\n\
    test             собрать и запустить shadow-тесты проекта\n\
    to-cpp, cpp      транслировать Goraw проект/файл в C++23 код (.cpp)\n\
    from-cpp, c++    транслировать C++23 код (.cpp) обратно в Goraw (.gw)\n\
    from-llvm, llvm  транслировать LLVM IR (.ll) обратно в Goraw (.gw)\n\
    transpile, xlat  универсальный кросс-транспилятор (.cpp <-> .gw <-> .ll)\n\
    opcodes, to-opcodes извлечь массив опкодов (x86-64 machine code) из .gw, .ll или .asm\n\
    proto, pb        скомпилировать .proto схему в Goraw-код\n\
    lsp              запустить Goraw Language Server Protocol (LSP) сервер для IDE\n\
\n\
ОПЦИИ:\n\
    -o <путь>        имя выходного файла (.exe, .ll или .cpp)\n\
    --release, -r    собрать в релизном профиле (-O3, удаление мёртвого кода, стриппинг)\n\
    --debug          собрать в отладочном профиле (-O0, -g отладочные символы, по умолчанию)\n\
    --emit-llvm      остановиться на LLVM IR (.ll), не звать clang\n\
    --emit-cpp       транслировать исходный код Goraw в C++23 (.cpp)\n\
    --emit-opcodes   извлечь массив опкодов x86-64 machine code\n\
    --opcode-format <fmt> формат опкодов (goraw, c, rust, hex, asm, bin)\n\
    --from-cpp <f.cpp> транслировать C++23 код (.cpp) обратно в Goraw (.gw)\n\
    --from-llvm <f.ll> транслировать LLVM IR (.ll) обратно в Goraw (.gw)\n\
    --json           печатать диагностику в LLM-формате (JSON + XML-нотки)\n\
    --run            запустить программу после успешной сборки\n\
    --test           собрать и прогнать shadow-тесты (test-блоки)\n\
    --silent, -s     автопропуск предупреждений безопасности при запуске тестов вне песочницы\n\
    --shadow=strict  строгий режим: ошибка E1200 при отсутствии shadow-теста для функции\n\
    --obfuscate-strings обфускация всех строковых литералов\n\
    --bind-c <header.h> сгенерировать Goraw-биндинги из C-заголовка\n\
    --cpp-std <std>  стандарт C++ для инлайн-вставок (по умолчанию `c++23`, также `c++26`, `c++20`)\n\
    --c-std <std>    стандарт C для инлайн-вставок (по умолчанию `c23`, также `c17`, `c11`)\n\
    --target <triple> целевая платформа clang/LLVM (по умолчанию `x86_64-w64-windows-gnu`)\n\
    -O<n>            уровень оптимизации clang (напр. -O2, переопределяет профиль)\n\
    --keep-ll        не удалять промежуточный .ll при сборке .exe\n\
    -S, --emit-asm   сгенерировать чистый x86-64 ассемблер (.asm)\n\
    -c, --emit-obj   скомпилировать в COFF объектный файл (.obj)\n\
    --native-backend, --self-hosted автономная сборка AST -> x86-64 machine code -> PE (без clang/LLVM)\n\
    --native-linker, --native-pe нативный встроенный PE/COFF компоновщик Goraw (без внешнего clang)\n\
    --clang <путь>   путь к clang (по умолчанию `clang` из PATH)\n\
    -h, --help       показать эту справку\n"
    );
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut input: Option<PathBuf> = None;
    let mut from_cpp: Option<PathBuf> = None;
    let mut from_llvm: Option<PathBuf> = None;
    let mut bind_c: Option<PathBuf> = None;
    let mut extra_objects: Vec<PathBuf> = Vec::new();
    let mut extra_asms: Vec<PathBuf> = Vec::new();
    let mut extra_protos: Vec<PathBuf> = Vec::new();
    let mut extra_sources: Vec<PathBuf> = Vec::new();
    let mut output = None;
    let mut emit_llvm = false;
    let mut emit_cpp = false;
    let mut emit_gw = false;
    let mut emit_asm = false;
    let mut emit_obj = false;
    let mut native_backend = false;
    let mut emit_opcodes = false;
    let mut opcode_format = gorawc::opcodes::OpcodeFormat::Goraw;
    let mut json = false;
    let mut run = false;
    let mut opt = None;
    let mut clang: Option<String> = None;
    let mut keep_ll = false;
    let mut test = false;
    let mut shadow_strict = false;
    let mut obfuscate_strings = false;
    let mut cpp_std = "c++23".to_string();
    let mut c_std = "c23".to_string();
    let mut profile = Profile::Debug;
    let mut target = if cfg!(windows) { "x86_64-pc-windows-msvc".to_string() } else { "x86_64-unknown-linux-gnu".to_string() };
    let mut native_linker = cfg!(windows);
    let mut silent = std::env::var("GORAW_SILENT").map(|v| v == "1").unwrap_or(false)
        || std::env::var("GORAW_BOX_SILENT").map(|v| v == "1").unwrap_or(false)
        || std::env::var("SILENT").map(|v| v == "1").unwrap_or(false);

    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "-h" | "--help" => {
                print_help();
                exit(0);
            }
            "-o" => {
                i += 1;
                output = Some(PathBuf::from(args.get(i).ok_or("-o требует аргумент")?));
            }
            "--bind-c" => {
                i += 1;
                bind_c = Some(PathBuf::from(args.get(i).ok_or("--bind-c требует аргумент")?));
            }
            "--from-cpp" => {
                i += 1;
                from_cpp = Some(PathBuf::from(args.get(i).ok_or("--from-cpp требует аргумент")?));
            }
            "--from-llvm" => {
                i += 1;
                from_llvm = Some(PathBuf::from(args.get(i).ok_or("--from-llvm требует аргумент")?));
            }
            "--release" | "-r" => profile = Profile::Release,
            "--debug" => profile = Profile::Debug,
            "--target" => {
                i += 1;
                target = args.get(i).ok_or("--target требует аргумент")?.clone();
            }
            "--emit-llvm" => emit_llvm = true,
            "--emit-cpp" => emit_cpp = true,
            "--emit-gw" | "--emit-goraw" => emit_gw = true,
            "--json" => json = true,
            "--run" => run = true,
            "--test" | "-t" | "test" => test = true,
            "lsp" | "--lsp" => {
                if let Err(e) = gorawc::lsp::run_lsp_server() {
                    eprintln!("ошибка LSP сервера: {e}");
                    exit(1);
                }
                exit(0);
            }
            "--silent" | "-s" => silent = true,
            "--shadow=strict" => shadow_strict = true,
            "--obfuscate-strings" | "--obf-strings" => obfuscate_strings = true,
            "--keep-ll" => keep_ll = true,
            "-S" | "--emit-asm" => emit_asm = true,
            "-c" | "--emit-obj" => emit_obj = true,
            "--native-backend" | "--native-codegen" | "--self-hosted" => {
                native_backend = true;
                native_linker = true;
            }
            "--native-linker" | "--native-pe" | "--native" => native_linker = true,
            "--no-native-linker" | "--clang-linker" => native_linker = false,
            "--clang" => {
                i += 1;
                clang = Some(args.get(i).ok_or("--clang требует аргумент")?.clone());
            }
            "--cpp-std" => {
                i += 1;
                cpp_std = args.get(i).ok_or("--cpp-std требует аргумент")?.clone();
            }
            "--c-std" => {
                i += 1;
                c_std = args.get(i).ok_or("--c-std требует аргумент")?.clone();
            }
            "--emit-opcodes" | "--opcodes" => emit_opcodes = true,
            "--opcode-format" => {
                i += 1;
                let f = args.get(i).ok_or("--opcode-format требует аргумент")?;
                opcode_format = gorawc::opcodes::OpcodeFormat::from_str(f)
                    .ok_or_else(|| format!("неизвестный формат опкодов `{f}` (доступны: goraw, c, rust, hex, asm, bin)"))?;
            }
            s if s.starts_with("-O") => opt = Some(s[2..].to_string()),
            s if s.starts_with('-') => return Err(format!("неизвестная опция `{s}` (см. --help)")),
            s => {
                let p = PathBuf::from(s);
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext == "asm" {
                    extra_asms.push(p);
                } else if ext == "proto" {
                    extra_protos.push(p);
                } else if ext == "obj" || ext == "o" || ext == "lib" || ext == "a" || ext == "bc" {
                    extra_objects.push(p);
                } else if input.is_none() {
                    input = Some(p);
                } else {
                    extra_sources.push(p);
                }
            }
        }
        i += 1;
    }

    if input.is_none() && !extra_asms.is_empty() {
        input = Some(extra_asms.remove(0));
    }
    if input.is_none() && !extra_sources.is_empty() {
        input = Some(extra_sources.remove(0));
    }

    if input.is_none() && bind_c.is_none() && from_cpp.is_none() && from_llvm.is_none() {
        if !extra_protos.is_empty() && !run && !test {
            // Прямой запуск компилятора: goraw schema.proto -> компиляция proto в .gw
            let proto_file = extra_protos.remove(0);
            let mut proto_args = vec![proto_file.to_string_lossy().to_string()];
            if let Some(o) = output {
                proto_args.push("-o".into());
                proto_args.push(o.to_string_lossy().to_string());
            }
            if json {
                proto_args.push("--json".into());
            }
            exit(handle_proto_subcommand(&proto_args));
        }
        return Err("не указан входной файл (см. --help)".into());
    }

    Ok(Options {
        input,
        bind_c,
        extra_sources,
        extra_objects,
        extra_asms,
        extra_protos,
        output,
        emit_llvm,
        emit_cpp,
        emit_gw,
        emit_opcodes,
        opcode_format,
        json,
        run,
        opt,
        clang: clang.unwrap_or_else(|| gorawc::c_interop::find_clang(false)),
        keep_ll,
        test,
        shadow_strict,
        obfuscate_strings,
        cpp_std,
        c_std,
        silent,
        profile,
        target,
        from_cpp,
        from_llvm,
        native_linker,
        native_backend,
        emit_asm,
        emit_obj,
    })
}

/// Проверяет, запущены ли тесты внутри подписанной песочницы.
/// Если подпись песочницы отсутствует (например, запуск на голой ОС), выводит WARN
/// и запрашивает Y/N у пользователя (если не указан флаг --silent / -s).
fn check_sandbox_security(silent: bool) -> bool {
    let in_sandbox = std::env::var("GORAW_SANDBOX_ACTIVE").map(|v| v == "1").unwrap_or(false);
    let is_signed = std::env::var("GORAW_SANDBOX_SIGNED").map(|v| v == "1").unwrap_or(false);

    if in_sandbox && is_signed {
        return true;
    }

    if silent {
        eprintln!("⚠️  [WARN] Запуск тестов вне подписанной песочницы (Silent bypass активирован).");
        return true;
    }

    let reason = if !in_sandbox {
        "Обнаружен прямой запуск тестов на голой ОС без активной песочницы!"
    } else {
        "Обнаружен запуск тестов в песочнице, но цифровая подпись компонентов не найдена!"
    };

    eprintln!(
        "================================================================================\n\
         ⚠️  [WARN] ПРЕДУПРЕЖДЕНИЕ БЕЗОПАСНОСТИ: ПОДПИСЬ ПЕСОЧНИЦЫ НЕ НАЙДЕНА\n\
         ================================================================================\n\
         {reason}\n\
         Тесты выполняют машинный код без подтверждённых гарантий изоляции и без лимита 16 ГБ.\n\
         \n\
         Рекомендуется запускать через подписанную песочницу:\n\
             .\\box.cmd goraw <файл.gw> --test\n\
         или подписать компоненты песочницы:\n\
             .\\box.cmd --sign\n\
         ================================================================================"
    );

    eprint!("Продолжить выполнение тестов без подписанной песочницы? [Y/N] (по умолчанию: N): ");
    let _ = std::io::stderr().flush();

    let mut input = String::new();
    match std::io::stdin().read_line(&mut input) {
        Ok(n) if n > 0 => {
            let choice = input.trim().to_lowercase();
            if choice == "y" || choice == "yes" || choice == "да" || choice == "д" {
                eprintln!("[INFO] Выполнение тестов разрешено пользователем.\n");
                true
            } else {
                eprintln!("[ABORT] Выполнение тестов прервано пользователем.");
                false
            }
        }
        _ => {
            eprintln!("\n[ABORT] Неинтерактивный ввод (EOF). Для автоматического пропуска используйте флаг --silent (-s).");
            false
        }
    }
}

fn run(opts: Options) -> i32 {
    if opts.test && !check_sandbox_security(opts.silent) {
        return 1;
    }

    let input_path = opts.input.as_ref().expect("входной файл");

    // Прямая компиляция и компоновка ассемблерных файлов (.asm)
    if input_path.extension().map_or(false, |e| e == "asm") {
        let asm_src = match std::fs::read_to_string(input_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("не удалось прочитать `{}`: {e}", input_path.display());
                return 2;
            }
        };
        let (obj, diags) = gorawc::asm::assemble(&input_path.display().to_string(), &asm_src);
        if diags.has_errors() {
            emit_diags(&diags, opts.json);
            return 1;
        }
        if !diags.items.is_empty() {
            emit_diags(&diags, opts.json);
        }
        let obj_bytes = match obj {
            Some(b) => b,
            None => return 1,
        };
        let (_ll_path, exe_path) = output_paths(&opts);
        let mut all_objs = vec![obj_bytes];
        for extra in &opts.extra_objects {
            match std::fs::read(extra) {
                Ok(b) => all_objs.push(b),
                Err(e) => {
                    eprintln!("не удалось прочитать `{}`: {e}", extra.display());
                    return 2;
                }
            }
        }
        let slices: Vec<&[u8]> = all_objs.iter().map(|v| v.as_slice()).collect();
        match gorawc::linker::link_coff_to_pe(&slices, Some("main")) {
            Ok(pe_bytes) => {
                if let Err(e) = std::fs::write(&exe_path, &pe_bytes) {
                    eprintln!("не удалось записать `{}`: {e}", exe_path.display());
                    return 2;
                }
                eprintln!("собрано (нативный PE): `{}`", exe_path.display());
                if opts.run || opts.test {
                    let status = Command::new(ensure_runnable_path(&exe_path)).status();
                    match status {
                        Ok(s) => return s.code().unwrap_or(0),
                        Err(e) => {
                            eprintln!("не удалось запустить `{}`: {e}", exe_path.display());
                            return 2;
                        }
                    }
                }
                return 0;
            }
            Err(e) => {
                eprintln!("ошибка нативного компоновщика PE: {e}");
                return 1;
            }
        }
    }

    let (src, line_map) = if input_path.extension().map_or(false, |e| e == "ll") && opts.extra_sources.is_empty() && opts.extra_protos.is_empty() {
        let llvm_opts = gorawc::llvm_to_goraw::LlvmToGorawOptions::default();
        match gorawc::llvm_to_goraw::transpile_llvm_file(input_path, &llvm_opts) {
            Ok(code) => (code, Vec::new()),
            Err(e) => {
                eprintln!("ошибка обработки LLVM IR `{}`: {e}", input_path.display());
                return 1;
            }
        }
    } else if input_path.extension().map_or(false, |e| e == "cpp" || e == "cc" || e == "cxx" || e == "hpp" || e == "h") && opts.extra_sources.is_empty() && opts.extra_protos.is_empty() {
        let cpp_opts = gorawc::cpp_to_goraw::CppToGorawOptions {
            clang_path: Some(opts.clang.clone()),
            cpp_std: Some(opts.cpp_std.clone()),
            target_triple: Some(opts.target.clone()),
            extra_includes: Vec::new(),
        };
        match gorawc::cpp_to_goraw::transpile_cpp_file(input_path, &cpp_opts) {
            Ok(code) => (code, Vec::new()),
            Err(e) => {
                eprintln!("ошибка обработки C++ `{}`: {e}", input_path.display());
                return 1;
            }
        }
    } else {
        match gather_sources(input_path, &opts.extra_sources, &opts.extra_protos, &opts.clang, &opts.target, &opts.cpp_std) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("{e}");
                return 2;
            }
        }
    };
    let file = input_path.display().to_string();
    if opts.emit_gw {
        if let Some(out) = &opts.output {
            if let Err(e) = std::fs::write(out, &src) {
                eprintln!("не удалось записать `{}`: {e}", out.display());
                return 2;
            }
            eprintln!("Goraw код записан в `{}`", out.display());
        } else if !opts.run && !opts.test {
            print!("{src}");
        }
        if !opts.run && !opts.test && !opts.emit_cpp && !opts.emit_llvm {
            return 0;
        }
    }
    let mut diags = diag::Diags::new(file.clone(), src.clone());
    diags.set_line_map(line_map.clone());

    // Лексер.
    let mut lx = lexer::Lexer::new(&src);
    let toks = lx.tokenize(&mut diags);

    // Парсер.
    let mut prog = {
        let mut p = parser::Parser::new(toks, &src, &mut diags);
        p.parse_program()
    };

    // 1. Мономорфизация дженериков (дженерик-структуры и методы -> конкретные специализации)
    gorawc::mono::monomorphize(&mut prog);

    // 2. Десахаризация statement-level инлайн C блоков в синтезированные функции
    let mut inline_c_code = String::new();
    let mut inline_cpp_code = String::new();
    let mut inline_counter = 0usize;
    let mut new_inline_fns = Vec::new();
    for f in &mut prog.fns {
        if let Some(b) = &mut f.body {
            desugar_inline_c_stmts(b, &mut inline_counter, &mut inline_c_code, &mut inline_cpp_code, &mut new_inline_fns);
        }
    }
    prog.fns.extend(new_inline_fns);

    // 3. Сборка top-level инлайн C / C++ блоков (`c { ... }` и `cpp { ... }`)
    let mut top_level_c = String::new();
    let mut top_level_cpp = String::new();
    for b in &prog.c_blocks {
        if b.is_cpp {
            top_level_cpp.push_str(&b.code);
            top_level_cpp.push('\n');
            inline_cpp_code.push_str(&b.code);
            inline_cpp_code.push('\n');
        } else {
            top_level_c.push_str(&b.code);
            top_level_c.push('\n');
            inline_c_code.push_str(&b.code);
            inline_c_code.push('\n');
        }
    }

    // 4. Извлечение функций из top-level C / C++ кода (чтобы тайпчекер и кодоген знали сигнатуры)
    if !top_level_c.trim().is_empty() {
        if let Ok(c_fns) = gorawc::c_interop::extract_functions_from_code(&top_level_c, false, Some(&opts.c_std), &opts.clang) {
            for f in c_fns {
                if !prog.fns.iter().any(|existing| existing.name == f.name) {
                    prog.fns.push(f);
                }
            }
        }
    }

    if !top_level_cpp.trim().is_empty() {
        if let Ok(cpp_fns) = gorawc::c_interop::extract_functions_from_code(&top_level_cpp, true, Some(&opts.cpp_std), &opts.clang) {
            for f in cpp_fns {
                if !prog.fns.iter().any(|existing| existing.name == f.name) {
                    prog.fns.push(f);
                }
            }
        }
    }

    // Проверка строгого режима shadow-тестов (--shadow=strict)
    if opts.shadow_strict {
        for f in &prog.fns {
            if !f.is_extern && f.name != "main" && !f.is_test {
                let has_test = prog.tests.iter().any(|t| {
                    t.name == f.name
                        || t.name == format!("shadow {}", f.name)
                        || t.name.starts_with(&format!("{} ", f.name))
                        || t.name.contains(&f.name)
                });
                if !has_test {
                    diags.push(
                        diag::Diagnostic::error(
                            "E1200",
                            f.span,
                            format!("функция `{}` не имеет обязательного shadow-теста (--shadow=strict)", f.name),
                        )
                        .with_hint(format!("добавьте `shadow {} {{ ... }}` или `test \"{}\" {{ ... }}`", f.name, f.name)),
                    );
                }
            }
        }
    }

    if opts.emit_cpp {
        let cpp_code = match gorawc::cpp_transpiler::transpile(&prog, opts.test, &line_map) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("ошибка C++ трансляции: {e}");
                return 1;
            }
        };

        let cpp_path = match &opts.output {
            Some(o) => {
                if o.extension().map_or(false, |ext| ext == "cpp") {
                    o.clone()
                } else {
                    o.with_extension("cpp")
                }
            }
            None => {
                let stem = input_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "out".into());
                let dir = input_path.parent().unwrap_or(Path::new("."));
                dir.join(format!("{stem}.cpp"))
            }
        };

        if let Some(parent) = cpp_path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                let _ = std::fs::create_dir_all(parent);
            }
        }

        if let Err(e) = std::fs::write(&cpp_path, &cpp_code) {
            eprintln!("не удалось записать `{}`: {e}", cpp_path.display());
            return 2;
        }

        eprintln!("C++23 код записан в `{}`", cpp_path.display());

        if !opts.emit_llvm && (opts.run || opts.test) {
            let exe_path = cpp_path.with_extension("exe");
            let mut cmd = Command::new(&opts.clang);
            cmd.arg(&format!("--target={}", opts.target))
                .arg(&format!("-std={}", opts.cpp_std))
                .arg(match opts.profile {
                    Profile::Release => "-O3",
                    Profile::Debug => "-O0",
                })
                .arg(&cpp_path)
                .arg("-o")
                .arg(&exe_path);
            cmd.arg("-lstdc++");
            if opts.target.contains("windows") {
                cmd.arg("-lws2_32");
            }

            let status = match cmd.status() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("ошибка вызова `{}`: {e}", opts.clang);
                    return 1;
                }
            };

            if !status.success() {
                eprintln!("ошибка сборки сгенерированного C++23 кода");
                return status.code().unwrap_or(1);
            }

            let mut run_cmd = Command::new(ensure_runnable_path(&exe_path));
            let run_status = match run_cmd.status() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("ошибка запуска `{}`: {e}", exe_path.display());
                    return 1;
                }
            };
            return run_status.code().unwrap_or(0);
        }

        if !opts.emit_llvm {
            return 0;
        }
    }

    // Интегрируем test/shadow-блоки в функции (@contract_*, @test_*)
    // и генерируем @run_all_goraw_tests() -> i32
    integrate_tests_into_program(&mut prog);

    // Сбор типов (первый проход).
    let mut collected = Vec::new();
    let ctx = types::collect(&prog.structs, &prog.enums, &prog.fns, &mut collected);
    for d in collected {
        diags.push(d);
    }

    // Кодоген + семантика (второй проход).
    let ir = {
        let cg = codegen::Codegen::new(&ctx, &mut diags)
            .with_target_triple(opts.target.clone())
            .with_obfuscate_strings(opts.obfuscate_strings)
            .with_test_mode(opts.test);
        cg.emit_module(&prog)
    };

    // Есть ошибки — печатаем диагностику и выходим.
    if diags.has_errors() {
        emit_diags(&diags, opts.json);
        return 1;
    }
    // Предупреждения печатаем, но продолжаем.
    if !diags.items.is_empty() {
        emit_diags(&diags, opts.json);
    }

    // Пути вывода.
    let (ll_path, exe_path) = output_paths(&opts);

    if let Some(parent) = ll_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    if let Err(e) = std::fs::write(&ll_path, &ir) {
        eprintln!("не удалось записать `{}`: {e}", ll_path.display());
        return 2;
    }

    if opts.emit_asm {
        match gorawc::x86_codegen::compile_program_to_asm(&prog) {
            Ok(asm_text) => {
                let asm_path = opts.output.clone().unwrap_or_else(|| ll_path.with_extension("asm"));
                if asm_path.to_str() == Some("-") {
                    print!("{asm_text}");
                } else {
                    if let Err(e) = std::fs::write(&asm_path, &asm_text) {
                        eprintln!("не удалось записать `{}`: {e}", asm_path.display());
                        return 2;
                    }
                    eprintln!("x86-64 ассемблер записан в `{}`", asm_path.display());
                }
                if !opts.keep_ll && !opts.emit_llvm {
                    let _ = std::fs::remove_file(&ll_path);
                }
                return 0;
            }
            Err(e) => {
                eprintln!("ошибка генерации x86-64 ассемблера: {e}");
                return 1;
            }
        }
    }

    if opts.emit_obj {
        let input_name = opts.input.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "main.gw".to_string());
        match gorawc::x86_codegen::compile_program_to_obj(&prog, &input_name) {
            Ok(obj_bytes) => {
                let obj_path = opts.output.clone().unwrap_or_else(|| ll_path.with_extension("obj"));
                if let Err(e) = std::fs::write(&obj_path, &obj_bytes) {
                    eprintln!("не удалось записать `{}`: {e}", obj_path.display());
                    return 2;
                }
                eprintln!("объектный файл COFF x86-64 записан в `{}`", obj_path.display());
                if !opts.keep_ll && !opts.emit_llvm {
                    let _ = std::fs::remove_file(&ll_path);
                }
                return 0;
            }
            Err(e) => {
                eprintln!("ошибка компиляции объектного файла: {e}");
                return 1;
            }
        }
    }

    if opts.emit_opcodes {
        use std::io::Write;
        let opt_lvl = opts.opt.clone().unwrap_or_else(|| match opts.profile {
            Profile::Debug => "0".to_string(),
            Profile::Release => "3".to_string(),
        });
        match gorawc::opcodes::extract_opcodes_from_ll(&ll_path, &opts.clang, &opt_lvl, &opts.target) {
            Ok(report) => {
                match report.render(opts.opcode_format, None) {
                    Ok(rendered) => {
                        if let Some(out) = &opts.output {
                            if out.to_str() == Some("-") {
                                let _ = std::io::stdout().write_all(&rendered);
                            } else {
                                if let Err(e) = std::fs::write(out, &rendered) {
                                    eprintln!("не удалось записать `{}`: {e}", out.display());
                                    return 2;
                                }
                                eprintln!("опкоды успешно сохранены в `{}`", out.display());
                            }
                        } else {
                            let _ = std::io::stdout().write_all(&rendered);
                        }
                        if !opts.keep_ll {
                            let _ = std::fs::remove_file(&ll_path);
                        }
                        return 0;
                    }
                    Err(e) => {
                        eprintln!("ошибка форматирования опкодов: {e}");
                        return 1;
                    }
                }
            }
            Err(e) => {
                eprintln!("{e}");
                return 1;
            }
        }
    }

    if opts.emit_llvm && !opts.run && !opts.test {
        eprintln!("LLVM IR записан в `{}`", ll_path.display());
        return 0;
    }
    if opts.emit_llvm {
        eprintln!("LLVM IR записан в `{}`", ll_path.display());
    }

    // Сборка внешних .asm файлов через нативный gorawas
    let mut temp_objs = Vec::new();
    for asm_path in &opts.extra_asms {
        let asm_src = match std::fs::read_to_string(asm_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("не удалось прочитать `{}`: {e}", asm_path.display());
                return 2;
            }
        };
        let (obj_bytes, asm_diags) = gorawc::asm::assemble(&asm_path.display().to_string(), &asm_src);
        if asm_diags.has_errors() {
            emit_diags(&asm_diags, opts.json);
            return 1;
        }
        let out_obj = asm_path.with_extension("obj");
        if let Some(bytes) = obj_bytes {
            if let Err(e) = std::fs::write(&out_obj, bytes) {
                eprintln!("не удалось записать `{}`: {e}", out_obj.display());
                return 2;
            }
            temp_objs.push(out_obj);
        }
    }

    // Если программа использует jit-блоки — рядом кладём C-рантайм и линкуем его.
    let uses_jit = ir.contains("@goraw_jit_compile(");
    let mut jit_rt_path: Option<PathBuf> = None;
    if uses_jit {
        let rt = ll_path.with_extension("jitrt.c");
        if let Err(e) = std::fs::write(&rt, JIT_RUNTIME_C) {
            eprintln!("не удалось записать JIT-рантайм `{}`: {e}", rt.display());
            return 2;
        }
        jit_rt_path = Some(rt);
    }

    let effective_opt = opts.opt.clone().unwrap_or_else(|| {
        match opts.profile {
            Profile::Release => "3".to_string(),
            Profile::Debug => "0".to_string(),
        }
    });

    // Компиляция инлайн C / C++ блоков в .bc файлы
    let mut temp_c_objs = Vec::new();
    if !inline_c_code.trim().is_empty() {
        let c_bc = ll_path.with_extension("c.bc");
        if let Err(e) = gorawc::c_interop::compile_inline_snippet(&inline_c_code, false, Some(&opts.c_std), &opts.clang, &c_bc, Some(&effective_opt), Some(&opts.target)) {
            eprintln!("{e}");
            return 1;
        }
        temp_c_objs.push(c_bc);
    }
    if !inline_cpp_code.trim().is_empty() {
        let cpp_bc = ll_path.with_extension("cpp.bc");
        if let Err(e) = gorawc::c_interop::compile_inline_snippet(&inline_cpp_code, true, Some(&opts.cpp_std), &opts.clang, &cpp_bc, Some(&effective_opt), Some(&opts.target)) {
            eprintln!("{e}");
            return 1;
        }
        temp_c_objs.push(cpp_bc);
    }

    // Если явно включен --native-backend, --native-linker или clang недоступен на Windows:
    if (opts.native_backend || opts.native_linker) && opts.target.contains("windows") {
        let input_name = opts.input.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "main.gw".to_string());
        let mut obj_bytes_list: Vec<Vec<u8>> = Vec::new();
        let mut compiled_obj_ok = false;

        if opts.native_backend {
            // Прямая генерация COFF .obj через встроенный x86-64 backend без clang
            match gorawc::x86_codegen::compile_program_to_obj(&prog, &input_name) {
                Ok(bytes) => {
                    obj_bytes_list.push(bytes);
                    compiled_obj_ok = true;
                }
                Err(e) => {
                    eprintln!("ошибка автономного x86-64 кодогенератора Goraw: {e}");
                    return 1;
                }
            }
        } else {
            // Компилируем LLVM IR в объектный файл через clang -c, если clang есть
            let ll_obj = ll_path.with_extension("obj");
            let mut compile_cmd = Command::new(&opts.clang);
            compile_cmd.arg(format!("--target={}", opts.target));
            compile_cmd.arg(format!("-O{effective_opt}"));
            compile_cmd.arg("-c").arg(&ll_path).arg("-o").arg(&ll_obj);
            compile_cmd.arg("-Wno-override-module");

            if matches!(compile_cmd.status(), Ok(s) if s.success()) {
                if let Ok(b) = std::fs::read(&ll_obj) {
                    obj_bytes_list.push(b);
                    compiled_obj_ok = true;
                }
                let _ = std::fs::remove_file(&ll_obj);
            } else {
                // Если clang не установлен на машине, автоматически переключаемся
                // на встроенный автономный x86-64 backend Goraw!
                match gorawc::x86_codegen::compile_program_to_obj(&prog, &input_name) {
                    Ok(bytes) => {
                        obj_bytes_list.push(bytes);
                        compiled_obj_ok = true;
                    }
                    Err(e) => {
                        eprintln!("ошибка сборки: Clang недоступен, а автономный x86-64 генератор сообщил: {e}");
                        return 1;
                    }
                }
            }
        }

        if compiled_obj_ok {
            for obj in &opts.extra_objects {
                if let Ok(b) = std::fs::read(obj) {
                    obj_bytes_list.push(b);
                }
            }
            for obj in &temp_objs {
                if let Ok(b) = std::fs::read(obj) {
                    obj_bytes_list.push(b);
                }
            }
            for obj in &temp_c_objs {
                if let Ok(b) = std::fs::read(obj) {
                    obj_bytes_list.push(b);
                }
            }

            let slices: Vec<&[u8]> = obj_bytes_list.iter().map(|b| b.as_slice()).collect();
            match gorawc::linker::link_coff_to_pe(&slices, Some("main")) {
                Ok(pe_bytes) => {
                    if let Err(e) = std::fs::write(&exe_path, &pe_bytes) {
                        eprintln!("не удалось записать `{}`: {e}", exe_path.display());
                        return 2;
                    }
                    if !opts.keep_ll && !opts.emit_llvm {
                        let _ = std::fs::remove_file(&ll_path);
                        if let Some(rt) = &jit_rt_path {
                            let _ = std::fs::remove_file(rt);
                        }
                        for obj in &temp_c_objs {
                            let _ = std::fs::remove_file(obj);
                        }
                    }
                    let backend_desc = if opts.native_backend {
                        "native x86-64 backend + native PE linker"
                    } else {
                        "native PE linker"
                    };
                    let profile_desc = match opts.profile {
                        Profile::Release => format!("release [optimized + {backend_desc}]"),
                        Profile::Debug => format!("debug [{backend_desc}]"),
                    };
                    eprintln!("собрано ({profile_desc}): `{}`", exe_path.display());

                    if opts.run || opts.test {
                        let status = Command::new(ensure_runnable_path(&exe_path)).status();
                        match status {
                            Ok(s) => return s.code().unwrap_or(0),
                            Err(e) => {
                                eprintln!("не удалось запустить `{}`: {e}", exe_path.display());
                                return 2;
                            }
                        }
                    }
                    return 0;
                }
                Err(e) => {
                    eprintln!("ошибка встроенного PE компоновщика: {e}");
                    return 1;
                }
            }
        }
    }

    // Линковка через clang.
    let mut cmd = Command::new(&opts.clang);
    cmd.arg(format!("--target={}", opts.target));
    cmd.arg(format!("-O{effective_opt}"));

    if opts.profile == Profile::Debug {
        cmd.arg("-g");
    } else {
        cmd.arg("-Wl,--gc-sections");
        cmd.arg("-Wl,-s");
    }

    cmd.arg(&ll_path);
    if let Some(rt) = &jit_rt_path {
        cmd.arg(rt);
    }
    for obj in &opts.extra_objects {
        cmd.arg(obj);
    }
    for obj in &temp_objs {
        cmd.arg(obj);
    }
    for obj in &temp_c_objs {
        cmd.arg(obj);
    }
    // Если в программе есть C++ блоки или вызовы, подключаем рантайм C++
    if !inline_cpp_code.trim().is_empty() {
        cmd.arg("-lstdc++");
    }
    // На Windows линкуем базовые сокеты WinSock2 ws2_32
    if opts.target.contains("windows") {
        cmd.arg("-lws2_32");
    }

    let sdk = gorawc::c_interop::get_sdk_paths();
    for lib_dir in &sdk.libs {
        cmd.arg(format!("-L{}", lib_dir.display()));
    }

    cmd.arg("-o").arg(&exe_path);
    // Подавляем предупреждение о переопределении triple (у нас он корректный).
    cmd.arg("-Wno-override-module");

    let status = match cmd.status() {
        Ok(s) => s,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                // Если есть готовые .obj на Windows, пробуем автономно связать
                if opts.target.contains("windows") && (!temp_objs.is_empty() || !opts.extra_objects.is_empty()) {
                    let mut obj_bytes_list = Vec::new();
                    for obj in temp_objs.iter().chain(opts.extra_objects.iter()) {
                        if let Ok(b) = std::fs::read(obj) {
                            obj_bytes_list.push(b);
                        }
                    }
                    let slices: Vec<&[u8]> = obj_bytes_list.iter().map(|b| b.as_slice()).collect();
                    if let Ok(pe_bytes) = gorawc::linker::link_coff_to_pe(&slices, Some("main")) {
                        let _ = std::fs::write(&exe_path, &pe_bytes);
                        eprintln!("clang не найден, успешно собрано через встроенный PE компоновщик: `{}`", exe_path.display());
                        return 0;
                    }
                }
                eprintln!(
                    "не удалось запустить clang (`{}`): {e}\n\
                     Подсказка: установите LLVM или используйте встроенный компоновщик (--native-linker / .asm)",
                    opts.clang
                );
            } else {
                eprintln!("не удалось запустить clang (`{}`): {e}\nУстановите LLVM или укажите путь через --clang", opts.clang);
            }
            return 2;
        }
    };
    if !status.success() {
        eprintln!("clang завершился с ошибкой при линковке `{}`", ll_path.display());
        return 1;
    }

    if !opts.keep_ll && !opts.emit_llvm {
        let _ = std::fs::remove_file(&ll_path);
        if let Some(rt) = &jit_rt_path {
            let _ = std::fs::remove_file(rt);
        }
        for obj in &temp_c_objs {
            let _ = std::fs::remove_file(obj);
        }
    }

    let profile_desc = match opts.profile {
        Profile::Release => "release [optimized + stripped]",
        Profile::Debug => "debug [unoptimized + debuginfo]",
    };
    eprintln!("собрано ({profile_desc}): `{}`", exe_path.display());

    if opts.run || opts.test {
        let status = Command::new(ensure_runnable_path(&exe_path)).status();
        match status {
            Ok(s) => return s.code().unwrap_or(0),
            Err(e) => {
                eprintln!("не удалось запустить `{}`: {e}", exe_path.display());
                return 2;
            }
        }
    }

    0
}

/// Интегрирует test/shadow-блоки в программу: каждый блок преобразуется в функцию
/// `contract_<name>() -> i64` или `test_<name>() -> i64` (0 = ок, иначе номер строки упавшего assert),
/// плюс генерируется функция `run_all_goraw_tests() -> i32`, которая прогоняет тесты и печатает отчёт.
/// Пользовательский `main` сохраняется нетронутым!
fn integrate_tests_into_program(prog: &mut ast::Program) {
    if prog.tests.is_empty() {
        return;
    }

    use ast::*;
    let dummy = diag::Span::dummy();

    let user_has_printf = prog.fns.iter().any(|f| f.name == "printf");
    let tests = &prog.tests;
    let mut test_meta = Vec::new();
    let mut used_names = std::collections::HashSet::new();

    for t in tests {
        let prefix = if t.is_shadow { "contract" } else { "test" };
        let clean = gorawc::cpp_transpiler::sanitize_test_name(&t.name);
        let base_name = format!("{prefix}_{clean}");
        let mut fn_name = base_name.clone();
        let mut idx = 1;
        while !used_names.insert(fn_name.clone()) {
            idx += 1;
            fn_name = format!("{base_name}_{idx}");
        }
        test_meta.push((fn_name.clone(), t.name.clone(), t.is_shadow));

        prog.fns.push(FnDef {
            name: fn_name,
            type_params: Vec::new(),
            params: Vec::new(),
            variadic: false,
            ret: Some(TypeExpr::Named("i64".into(), dummy)),
            body: Some(t.body.clone()),
            is_unsafe: false,
            is_extern: false,
            is_test: true,
            span: t.span,
        });
    }

    // Runner function: fn run_all_goraw_tests() -> i32
    let mut hs = String::new();
    if !user_has_printf {
        hs.push_str("extern fn printf(fmt: *u8, ...) -> i32;\n");
    }
    hs.push_str("fn run_all_goraw_tests() -> i32 {\n");
    hs.push_str("    let mut __p: i64 = 0;\n    let mut __f: i64 = 0;\n");
    hs.push_str("    printf(\"============================================================\\n\");\n");
    hs.push_str("    printf(\"  Running Goraw Contracts & Tests in LLVM IR                \\n\");\n");
    hs.push_str("    printf(\"============================================================\\n\");\n");

    let contracts: Vec<_> = test_meta.iter().filter(|(_, _, is_s)| *is_s).collect();
    let unit_tests: Vec<_> = test_meta.iter().filter(|(_, _, is_s)| !*is_s).collect();

    if !contracts.is_empty() {
        hs.push_str("    printf(\"\\n--- Shadow Contracts ---\\n\");\n");
        for (fn_name, name, _) in &contracts {
            let esc_name = name.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%");
            hs.push_str(&format!("    let r_{fn_name} = {fn_name}();\n"));
            hs.push_str(&format!(
                "    if r_{fn_name} != 0 {{\n        printf(\"[CONTRACT FAIL] {esc_name} (line %lld)\\n\", r_{fn_name});\n        __f += 1;\n    }} else {{\n        printf(\"[CONTRACT ok] {esc_name}\\n\");\n        __p += 1;\n    }}\n"
            ));
        }
    }

    if !unit_tests.is_empty() {
        hs.push_str("    printf(\"\\n--- Integration Tests ---\\n\");\n");
        for (fn_name, name, _) in &unit_tests {
            let esc_name = name.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%");
            hs.push_str(&format!("    let r_{fn_name} = {fn_name}();\n"));
            hs.push_str(&format!(
                "    if r_{fn_name} != 0 {{\n        printf(\"[TEST FAIL] {esc_name} (line %lld)\\n\", r_{fn_name});\n        __f += 1;\n    }} else {{\n        printf(\"[TEST ok] {esc_name}\\n\");\n        __p += 1;\n    }}\n"
            ));
        }
    }

    let total = contracts.len() + unit_tests.len();
    hs.push_str(&format!(
        "    printf(\"\\n[LLVM IR] All {total} tests finished: %lld passed, %lld failed\\n\\n\", __p, __f);\n"
    ));
    hs.push_str("    return __f as i32;\n}\n");

    let mut hdiags = diag::Diags::new("<test-harness>", hs.clone());
    let htoks = lexer::Lexer::new(&hs).tokenize(&mut hdiags);
    let hprog = parser::Parser::new(htoks, &hs, &mut hdiags).parse_program();
    for f in hprog.fns {
        if !prog.fns.iter().any(|existing| existing.name == f.name) {
            prog.fns.push(f);
        }
    }
}

/// Рекурсивно собирает главный файл и все импортируемые (`import "path";`) в
/// один объединённый источник + карту строк (для диагностик по файлам).
/// Порядок: зависимости раньше импортёра; дубли включаются один раз.
/// Рекурсивно собирает главный файл, переданные .proto схемы и все импортируемые
/// (`import "path";`, в т.ч. .gw, .proto, .h) в один объединённый источник + карту строк.
/// Порядок: зависимости раньше импортёра; дубли включаются один раз.
fn gather_sources(
    main: &Path,
    extra_sources: &[PathBuf],
    extra_protos: &[PathBuf],
    clang_path: &str,
    target_triple: &str,
    cpp_std: &str,
) -> Result<(String, Vec<(u32, String)>), String> {
    use std::collections::HashSet;
    let mut order: Vec<(String, String)> = Vec::new(); // (отображаемый путь, src)
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut proto_prelude_emitted = false;

    // 1. Сначала компилируем все явно переданные .proto файлы (extra_protos)
    for proto_path in extra_protos {
        let canon = std::fs::canonicalize(proto_path).unwrap_or_else(|_| proto_path.to_path_buf());
        if !seen.insert(canon.clone()) {
            continue;
        }
        let proto_src = std::fs::read_to_string(proto_path)
            .map_err(|e| format!("не удалось прочитать `{}`: {e}", proto_path.display()))?;
        let (code, diags) = gorawc::proto::compile_ext(
            &proto_path.display().to_string(),
            &proto_src,
            !proto_prelude_emitted,
        );
        if diags.has_errors() {
            return Err(format!(
                "ошибка компиляции Protobuf из `{}`:\n{}",
                proto_path.display(),
                diags.render_human()
            ));
        }
        proto_prelude_emitted = true;
        order.push((proto_path.display().to_string(), code.unwrap_or_default()));
    }

    fn visit(
        path: &Path,
        seen: &mut HashSet<PathBuf>,
        order: &mut Vec<(String, String)>,
        proto_prelude_emitted: &mut bool,
        clang_path: &str,
    ) -> Result<(), String> {
        let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !seen.insert(canon.clone()) {
            return Ok(());
        }
        let src = std::fs::read_to_string(path)
            .map_err(|e| format!("не удалось прочитать `{}`: {e}", path.display()))?;
        let dir = path.parent().unwrap_or(Path::new("."));
        for imp in scan_imports(&src) {
            let resolved = match gorawc::pkg::resolve_import(dir, &imp) {
                Some(p) => p,
                None => return Err(format!("не удалось найти импортируемый модуль `{imp}` (импорт из `{}`)", path.display())),
            };
            let resolved_str = resolved.to_string_lossy().to_string();
            if resolved_str.ends_with(".h") || resolved_str.ends_with(".hpp") {
                let bindings = gorawc::c_interop::generate_bindings_from_header(&resolved, clang_path)
                    .map_err(|e| format!("ошибка генерации биндингов из `{}`: {e}", resolved.display()))?;
                order.push((resolved.display().to_string(), bindings));
            } else if resolved_str.ends_with(".proto") {
                let r_canon = std::fs::canonicalize(&resolved).unwrap_or_else(|_| resolved.clone());
                if seen.insert(r_canon) {
                    let proto_src = std::fs::read_to_string(&resolved)
                        .map_err(|e| format!("не удалось прочитать `{}`: {e}", resolved.display()))?;
                    let (code, diags) = gorawc::proto::compile_ext(
                        &resolved.display().to_string(),
                        &proto_src,
                        !*proto_prelude_emitted,
                    );
                    if diags.has_errors() {
                        return Err(format!(
                            "ошибка компиляции Protobuf из `{}`:\n{}",
                            resolved.display(),
                            diags.render_human()
                        ));
                    }
                    *proto_prelude_emitted = true;
                    order.push((resolved.display().to_string(), code.unwrap_or_default()));
                }
            } else {
                visit(&resolved, seen, order, proto_prelude_emitted, clang_path)?;
            }
        }
        order.push((path.display().to_string(), src));
        Ok(())
    }

    let ingest_file = |path: &Path, seen: &mut HashSet<PathBuf>, order: &mut Vec<(String, String)>, proto_prelude_emitted: &mut bool| -> Result<(), String> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext == "ll" {
            let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            if !seen.insert(canon) {
                return Ok(());
            }
            let llvm_opts = gorawc::llvm_to_goraw::LlvmToGorawOptions::default();
            let code = gorawc::llvm_to_goraw::transpile_llvm_file(path, &llvm_opts)
                .map_err(|e| format!("ошибка обработки LLVM IR `{}`: {e}", path.display()))?;
            order.push((path.display().to_string(), code));
        } else if ext == "cpp" || ext == "cc" || ext == "cxx" || ext == "hpp" || ext == "h" {
            let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            if !seen.insert(canon) {
                return Ok(());
            }
            let cpp_opts = gorawc::cpp_to_goraw::CppToGorawOptions {
                clang_path: Some(clang_path.to_string()),
                cpp_std: Some(cpp_std.to_string()),
                target_triple: Some(target_triple.to_string()),
                extra_includes: Vec::new(),
            };
            let code = gorawc::cpp_to_goraw::transpile_cpp_file(path, &cpp_opts)
                .map_err(|e| format!("ошибка обработки C/C++ `{}`: {e}", path.display()))?;
            order.push((path.display().to_string(), code));
        } else {
            visit(path, seen, order, proto_prelude_emitted, clang_path)?;
        }
        Ok(())
    };

    // 2. Обрабатываем extra_sources (дополнительные модули любого типа)
    for extra in extra_sources {
        ingest_file(extra, &mut seen, &mut order, &mut proto_prelude_emitted)?;
    }

    // 3. Обрабатываем основной входной файл
    ingest_file(main, &mut seen, &mut order, &mut proto_prelude_emitted)?;

    let mut combined = String::new();
    let mut map = Vec::new();
    for (path, src) in &order {
        let start_line = combined.matches('\n').count() as u32 + 1;
        map.push((start_line, path.clone()));
        combined.push_str(src);
        if !src.ends_with('\n') {
            combined.push('\n');
        }
    }
    Ok((combined, map))
}

/// Лёгкое сканирование строк на `import "path";` (без полного парсинга).
fn scan_imports(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in src.lines() {
        let line = match raw.find("//") {
            Some(p) => &raw[..p],
            None => raw,
        };
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("import") {
            let rest = rest.trim_start();
            if let Some(start) = rest.find('"') {
                if let Some(end) = rest[start + 1..].find('"') {
                    out.push(rest[start + 1..start + 1 + end].to_string());
                }
            }
        }
    }
    out
}

fn ensure_runnable_path(p: &Path) -> PathBuf {
    if p.is_relative() && p.parent().map_or(true, |parent| parent.as_os_str().is_empty()) {
        Path::new(".").join(p)
    } else {
        p.to_path_buf()
    }
}

fn output_paths(opts: &Options) -> (PathBuf, PathBuf) {
    let inp = opts.input.as_ref().expect("входной файл");
    let stem = inp.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "out".into());
    let dir = inp.parent().unwrap_or(Path::new("."));
    match &opts.output {
        Some(o) => {
            if opts.emit_llvm && !opts.emit_cpp && !opts.run && !opts.test {
                (o.clone(), o.clone())
            } else {
                let ll = o.with_extension("ll");
                (ll, o.clone())
            }
        }
        None => {
            let ll_name = if inp.extension().map_or(false, |e| e == "ll") {
                format!("{stem}.goraw_gen.ll")
            } else {
                format!("{stem}.ll")
            };
            if opts.emit_llvm && !opts.run && !opts.test {
                (dir.join(&ll_name), dir.join(&ll_name))
            } else {
                (dir.join(&ll_name), dir.join(format!("{stem}.exe")))
            }
        }
    }
}

fn emit_diags(diags: &diag::Diags, json: bool) {
    if json {
        // JSON идёт в stdout (чтобы удобно ловить пайпом), человекочитаемое — в stderr.
        print!("{}", diags.render_llm_json());
    } else {
        eprint!("{}", diags.render_human());
    }
}

fn desugar_inline_c_stmts(
    b: &mut ast::Block,
    counter: &mut usize,
    c_out: &mut String,
    cpp_out: &mut String,
    fns_out: &mut Vec<ast::FnDef>,
) {
    for s in &mut b.stmts {
        match s {
            ast::Stmt::InlineC { is_cpp, inputs, outputs, body, span } => {
                *counter += 1;
                let fn_name = format!("__gw_inline_{}_{}", if *is_cpp { "cpp" } else { "c" }, *counter);

                let mut c_func = String::new();
                if *is_cpp {
                    c_func.push_str("extern \"C\" ");
                }
                c_func.push_str(&format!("void {fn_name}("));
                let mut params = Vec::new();
                let mut fn_params = Vec::new();
                let mut call_args = Vec::new();

                for (idx, inp) in inputs.iter().enumerate() {
                    let pname = format!("__in_{idx}");
                    params.push(format!("long long {pname}"));
                    fn_params.push(ast::Param {
                        name: pname.clone(),
                        ty: ast::TypeExpr::Named("i64".into(), *span),
                        span: *span,
                    });
                    call_args.push(ast::Expr::Cast {
                        expr: Box::new(ast::Expr::Ident(inp.clone(), *span)),
                        ty: ast::TypeExpr::Named("i64".into(), *span),
                        span: *span,
                    });
                }

                for (idx, out) in outputs.iter().enumerate() {
                    let pname = format!("__out_{idx}");
                    params.push(format!("long long* {pname}"));
                    fn_params.push(ast::Param {
                        name: pname.clone(),
                        ty: ast::TypeExpr::PtrMut(Box::new(ast::TypeExpr::Named("i64".into(), *span)), *span),
                        span: *span,
                    });
                    call_args.push(ast::Expr::Unary {
                        op: ast::UnOp::RefMut,
                        expr: Box::new(ast::Expr::Ident(out.clone(), *span)),
                        span: *span,
                    });
                }

                c_func.push_str(&params.join(", "));
                c_func.push_str(") {\n");

                for (idx, inp) in inputs.iter().enumerate() {
                    c_func.push_str(&format!("    #define {inp} __in_{idx}\n"));
                }
                for (idx, out) in outputs.iter().enumerate() {
                    c_func.push_str(&format!("    #define {out} __out_{idx}\n"));
                }

                c_func.push_str("    ");
                c_func.push_str(body);
                c_func.push('\n');

                for inp in inputs {
                    c_func.push_str(&format!("    #undef {inp}\n"));
                }
                for out in outputs {
                    c_func.push_str(&format!("    #undef {out}\n"));
                }
                c_func.push_str("}\n\n");

                if *is_cpp {
                    cpp_out.push_str(&c_func);
                } else {
                    c_out.push_str(&c_func);
                }

                fns_out.push(ast::FnDef {
                    name: fn_name.clone(),
                    type_params: Vec::new(),
                    params: fn_params,
                    variadic: false,
                    ret: None,
                    body: None,
                    is_unsafe: false,
                    is_extern: true,
                    is_test: false,
                    span: *span,
                });

                *s = ast::Stmt::Expr(ast::Expr::Call {
                    callee: Box::new(ast::Expr::Ident(fn_name, *span)),
                    args: call_args,
                    span: *span,
                });
            }
            ast::Stmt::If { then, els, .. } => {
                desugar_inline_c_stmts(then, counter, c_out, cpp_out, fns_out);
                if let Some(el) = els {
                    desugar_inline_c_stmts(el, counter, c_out, cpp_out, fns_out);
                }
            }
            ast::Stmt::While { body, .. } => {
                desugar_inline_c_stmts(body, counter, c_out, cpp_out, fns_out);
            }
            ast::Stmt::For { body, .. } | ast::Stmt::ForIn { body, .. } | ast::Stmt::Unsafe(body, _) => {
                desugar_inline_c_stmts(body, counter, c_out, cpp_out, fns_out);
            }
            ast::Stmt::Match { arms, .. } => {
                for (_, arm_b) in arms {
                    desugar_inline_c_stmts(arm_b, counter, c_out, cpp_out, fns_out);
                }
            }
            _ => {}
        }
    }
}
