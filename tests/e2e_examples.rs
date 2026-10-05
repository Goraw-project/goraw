//! E2E интеграционные тесты компилятора Goraw.
//! Тестируют сквозную компиляцию всех примеров (examples/),
//! нативный ассемблер и PE-линковщик без внешних зависимостей,
//! двустороннюю трансляцию (gw -> ll -> gw и gw -> cpp) и компиляцию proto.

use gorawc::{asm, cpp_transpiler, diag, lexer, linker, llvm_to_goraw, parser, proto};
use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_e2e_compile_all_examples_with_goraw_cli() {
    let goraw_bin = env!("CARGO_BIN_EXE_goraw");
    let examples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    let out_dir = std::env::temp_dir().join("goraw_e2e_out");
    let _ = std::fs::create_dir_all(&out_dir);

    let test_files = [
        "arrays.gw",
        "bytes.gw",
        "consts.gw",
        "enums.gw",
        "fnptr.gw",
        "match.gw",
        "math.gw",
        "methods.gw",
        "slices.gw",
        "statics.gw",
        "strings.gw",
        "strings_v2.gw",
        "test_generics_raii.gw",
        "test_sync.gw",
        "tour.gw",
    ];

    for file in &test_files {
        let p = examples_dir.join(file);
        assert!(p.exists(), "файл примера не найден: {}", p.display());
        let out_ll = out_dir.join(format!("{file}.ll"));

        // Компилируем файл в LLVM IR полностью своими силами
        let status = Command::new(goraw_bin)
            .arg(&p)
            .arg("--emit-llvm")
            .arg("-o")
            .arg(&out_ll)
            .status()
            .unwrap_or_else(|e| panic!("не удалось запустить {goraw_bin}: {e}"));

        assert!(status.success(), "компиляция примера {file} завершилась с ошибкой");
        assert!(out_ll.exists(), "LLVM IR файл не создан для {file}");

        let ll_content = std::fs::read_to_string(&out_ll).expect("чтение сгенерированного .ll");
        assert!(!ll_content.is_empty(), "файл .ll пустой для {file}");
        assert!(ll_content.contains("define "), "в .ll нет функций для {file}");
        let _ = std::fs::remove_file(&out_ll);
    }
}

#[test]
fn test_e2e_native_asm_and_pe_linker() {
    let examples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    let asm_path = examples_dir.join("native_hello.asm");
    assert!(asm_path.exists(), "native_hello.asm не найден");

    let src = std::fs::read_to_string(&asm_path).expect("чтение native_hello.asm");
    let (obj_bytes, diags) = asm::assemble(&asm_path.display().to_string(), &src);
    assert!(!diags.has_errors(), "ошибки ассемблера: {}", diags.render_human());
    let obj = obj_bytes.expect("байты COFF объекта");
    assert!(!obj.is_empty());

    // Линкуем через нативный PE/COFF компоновщик
    let pe_result = linker::link_coff_to_pe(&[&obj], Some("main"));
    assert!(pe_result.is_ok(), "ошибка линковщика PE: {:?}", pe_result.err());
    let pe = pe_result.unwrap();

    // Проверяем валидность сигнатур PE (MZ + PE)
    assert!(pe.len() > 512);
    assert_eq!(&pe[0..2], b"MZ");
    let pe_offset = u32::from_le_bytes([pe[0x3c], pe[0x3d], pe[0x3e], pe[0x3f]]) as usize;
    assert_eq!(&pe[pe_offset..pe_offset + 4], b"PE\0\0");

    #[cfg(target_os = "windows")]
    {
        let out_exe = std::env::temp_dir().join("goraw_e2e_native_hello.exe");
        std::fs::write(&out_exe, &pe).expect("запись тестового exe");
        let status = Command::new(&out_exe).status();
        let _ = std::fs::remove_file(&out_exe);
        assert!(status.is_ok(), "запуск нативного exe завершился с ошибкой");
        assert_eq!(status.unwrap().code(), Some(0));
    }
}

#[test]
fn test_e2e_roundtrip_gw_to_cpp() {
    let examples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    let files = ["math.gw", "consts.gw", "enums.gw", "methods.gw"];

    for file in &files {
        let p = examples_dir.join(file);
        let src = std::fs::read_to_string(&p).expect("чтение gw");
        let mut diags = diag::Diags::new(p.display().to_string(), src.clone());
        let mut lx = lexer::Lexer::new(&src);
        let toks = lx.tokenize(&mut diags);
        assert!(!diags.has_errors());
        let mut parser = parser::Parser::new(toks, &src, &mut diags);
        let prog = parser.parse_program();
        assert!(!diags.has_errors());

        let cpp_code = cpp_transpiler::transpile(&prog, false, &[])
            .expect("трансляция в C++23");
        assert!(!cpp_code.is_empty(), "C++ вывод пустой для {}", file);
        assert!(cpp_code.contains("#include <cstdint>"), "нет заголовков в C++ для {}", file);
        assert!(cpp_code.contains("GorawStr"), "нет GorawStr в C++ для {}", file);
    }
}

#[test]
fn test_e2e_roundtrip_gw_to_ll_to_gw() {
    let goraw_bin = env!("CARGO_BIN_EXE_goraw");
    let examples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    let math_path = examples_dir.join("math.gw");
    let out_ll = std::env::temp_dir().join("goraw_e2e_math.ll");

    let status = Command::new(goraw_bin)
        .arg(&math_path)
        .arg("--emit-llvm")
        .arg("-o")
        .arg(&out_ll)
        .status()
        .expect("вызов goraw");
    assert!(status.success());

    let ir = std::fs::read_to_string(&out_ll).expect("чтение ll");
    let _ = std::fs::remove_file(&out_ll);

    let llvm_opts = llvm_to_goraw::LlvmToGorawOptions::default();
    let decompiled_gw = llvm_to_goraw::transpile_llvm_ir(&ir, &llvm_opts)
        .expect("декомпиляция LLVM IR в Goraw");

    assert!(!decompiled_gw.is_empty());
    assert!(decompiled_gw.contains("fn hypot("), "декомпилированный код не содержит hypot");
    assert!(decompiled_gw.contains("fn main("), "декомпилированный код не содержит main");

    // Проверяем, что декомпилированный код парсится компилятором Goraw
    let mut diags = diag::Diags::new("decompiled_math.gw".to_string(), decompiled_gw.clone());
    let mut lx = lexer::Lexer::new(&decompiled_gw);
    let toks = lx.tokenize(&mut diags);
    assert!(!diags.has_errors(), "ошибки лексера при повторном разборе: {}", diags.render_human());
    let mut parser = parser::Parser::new(toks, &decompiled_gw, &mut diags);
    let prog = parser.parse_program();
    assert!(!diags.has_errors(), "ошибки парсера при повторном разборе: {}", diags.render_human());
    assert!(!prog.fns.is_empty(), "нет распарсенных функций в декомпилированном коде");
}

#[test]
fn test_e2e_proto_compilation() {
    let proto_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples").join("proto");
    let proto_file = proto_dir.join("service_demo.proto");
    assert!(proto_file.exists(), "service_demo.proto не найден");

    let proto_src = std::fs::read_to_string(&proto_file).expect("чтение proto");
    let (gw_code, diags) = proto::compile(&proto_file.display().to_string(), &proto_src);
    assert!(!diags.has_errors(), "ошибки трансляции proto: {:?}", diags.items);
    let gw_code = gw_code.expect("сгенерированный gw код");

    assert!(!gw_code.is_empty());
    assert!(gw_code.contains("struct UserRequest"), "не содержит структуру UserRequest");
    assert!(gw_code.contains("struct UserResponse"), "не содержит структуру UserResponse");

    // Парсим сгенерированный код
    let mut diags = diag::Diags::new("generated_proto.gw".to_string(), gw_code.clone());
    let mut lx = lexer::Lexer::new(&gw_code);
    let toks = lx.tokenize(&mut diags);
    assert!(!diags.has_errors());
    let mut parser = parser::Parser::new(toks, &gw_code, &mut diags);
    let prog = parser.parse_program();
    assert!(!diags.has_errors());
    assert!(!prog.structs.is_empty());
}
