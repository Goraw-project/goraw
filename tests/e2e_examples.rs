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

#[test]
fn test_e2e_native_loop_and_gw_execution() {
    let goraw_bin = env!("CARGO_BIN_EXE_goraw");
    let examples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    let gw_path = examples_dir.join("native_loop_main.gw");
    let asm_path = examples_dir.join("native_loop.asm");
    assert!(gw_path.exists());
    assert!(asm_path.exists());

    let out_exe = std::env::temp_dir().join("goraw_e2e_loop_native.exe");

    let output = Command::new(goraw_bin)
        .arg(&gw_path)
        .arg(&asm_path)
        .arg("-o")
        .arg(&out_exe)
        .arg("--run")
        .output()
        .expect("вызов goraw");

    if out_exe.exists() {
        let _ = std::fs::remove_file(&out_exe);
    }

    assert!(output.status.success(), "компиляция/запуск native_loop завершились с ошибкой");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("goraw_sum_to_n(10) = 55"), "неверный вывод: {stdout}");
    assert!(stdout.contains("goraw_sum_to_n(100) = 5050"), "неверный вывод: {stdout}");
}

#[test]
fn test_e2e_native_mem_and_gw_execution() {
    let goraw_bin = env!("CARGO_BIN_EXE_goraw");
    let examples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    let gw_path = examples_dir.join("native_mem_main.gw");
    let asm_path = examples_dir.join("native_mem.asm");
    assert!(gw_path.exists());
    assert!(asm_path.exists());

    let out_exe = std::env::temp_dir().join("goraw_e2e_mem_native.exe");

    let output = Command::new(goraw_bin)
        .arg(&gw_path)
        .arg(&asm_path)
        .arg("-o")
        .arg(&out_exe)
        .arg("--run")
        .output()
        .expect("вызов goraw");

    if out_exe.exists() {
        let _ = std::fs::remove_file(&out_exe);
    }

    assert!(output.status.success(), "компиляция/запуск native_mem завершились с ошибкой");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("SUCCESS: native memory operands work end-to-end!"), "неверный вывод: {stdout}");
}

#[test]
fn test_e2e_direct_ll_compilation_and_run() {
    let goraw_bin = env!("CARGO_BIN_EXE_goraw");
    let examples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    let math_gw = examples_dir.join("math.gw");
    let temp_ll = std::env::temp_dir().join("goraw_e2e_direct.ll");
    let out_exe = std::env::temp_dir().join("goraw_e2e_direct_ll.exe");

    let status_gen = Command::new(goraw_bin)
        .arg(&math_gw)
        .arg("--emit-llvm")
        .arg("-o")
        .arg(&temp_ll)
        .status()
        .expect("генерация ll");
    assert!(status_gen.success());

    let output = Command::new(goraw_bin)
        .arg(&temp_ll)
        .arg("-o")
        .arg(&out_exe)
        .arg("--run")
        .output()
        .expect("компиляция ll через goraw");

    let _ = std::fs::remove_file(&temp_ll);
    if out_exe.exists() {
        let _ = std::fs::remove_file(&out_exe);
    }

    assert!(output.status.success(), "прямая компиляция .ll завершилась ошибкой");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hypot(3,4)      = 5.000000"), "неверный вывод: {stdout}");
    assert!(stdout.contains("pow(2, 10)      = 1024.000000"), "неверный вывод: {stdout}");
}

#[test]
fn test_e2e_direct_cpp_compilation_and_run() {
    let goraw_bin = env!("CARGO_BIN_EXE_goraw");
    let examples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    let math_cpp = examples_dir.join("math.cpp");
    assert!(math_cpp.exists());

    let out_exe = std::env::temp_dir().join("goraw_e2e_direct_cpp.exe");

    let output = Command::new(goraw_bin)
        .arg(&math_cpp)
        .arg("-o")
        .arg(&out_exe)
        .arg("--run")
        .output()
        .expect("компиляция cpp через goraw");

    if out_exe.exists() {
        let _ = std::fs::remove_file(&out_exe);
    }

    assert!(output.status.success(), "прямая компиляция .cpp завершилась ошибкой");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hypot(3,4)      = 5.000000"), "неверный вывод: {stdout}");
    assert!(stdout.contains("pow(2, 10)      = 1024.000000"), "неверный вывод: {stdout}");
}

#[test]
fn test_e2e_inline_c_and_cpp_blocks() {
    let goraw_bin = env!("CARGO_BIN_EXE_goraw");
    let examples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    let inline_gw = examples_dir.join("test_inline_c_cpp.gw");
    assert!(inline_gw.exists());

    let temp_ll = std::env::temp_dir().join("goraw_e2e_inline.ll");

    let status = Command::new(goraw_bin)
        .arg(&inline_gw)
        .arg("--emit-llvm")
        .arg("-o")
        .arg(&temp_ll)
        .status()
        .expect("компиляция inline c/cpp");

    assert!(status.success());
    assert!(temp_ll.exists());
    let ir = std::fs::read_to_string(&temp_ll).expect("чтение ll");
    let _ = std::fs::remove_file(&temp_ll);

    assert!(ir.contains("define "), "IR не содержит функций");
    assert!(ir.contains("main"), "IR не содержит main");
}

#[test]
fn test_e2e_mixed_sources_compilation_and_execution() {
    let goraw_bin = env!("CARGO_BIN_EXE_goraw");
    let temp_dir = std::env::temp_dir();

    let gw_file = temp_dir.join("mixed_main.gw");
    let cpp_file = temp_dir.join("mixed_helper.cpp");
    let cxx_file = temp_dir.join("mixed_extra.cxx");
    let ll_file = temp_dir.join("mixed_helper.ll");
    let hpp_file = temp_dir.join("mixed_header.hpp");
    let asm_file = temp_dir.join("mixed_asm.asm");
    let exe_file = temp_dir.join("mixed_mega_test.exe");

    std::fs::write(&cpp_file, r#"
extern "C" long long add_nums(long long a, long long b) {
    return a + b;
}
"#).unwrap();

    std::fs::write(&cxx_file, r#"
extern "C" long long scale_nums(long long x) {
    return x * 2;
}
"#).unwrap();

    std::fs::write(&hpp_file, r#"
#pragma once
inline long long sub_nums(long long a, long long b) {
    return a - b;
}
"#).unwrap();

    std::fs::write(&ll_file, r#"
define i64 @mul_nums(i64 %a, i64 %b) {
    %res = mul i64 %a, %b
    ret i64 %res
}
"#).unwrap();

    std::fs::write(&asm_file, r#"
section .text
global bitwise_inv

bitwise_inv:
    mov rax, rcx
    not rax
    ret
"#).unwrap();

    std::fs::write(&gw_file, r#"
extern fn printf(fmt: *u8, ...) -> i32;
extern fn bitwise_inv(x: i64) -> i64;

fn main() -> i32 {
    let s: i64 = add_nums(15, 27);
    let m: i64 = mul_nums(6, 7);
    let sub: i64 = sub_nums(100, 58);
    let sc: i64 = scale_nums(21);
    let bw: i64 = bitwise_inv(-43);
    printf("s=%lld, m=%lld, sub=%lld, sc=%lld, bw=%lld\n", s, m, sub, sc, bw);
    return 0;
}
"#).unwrap();

    let output = Command::new(goraw_bin)
        .arg(&gw_file)
        .arg(&cpp_file)
        .arg(&cxx_file)
        .arg(&ll_file)
        .arg(&hpp_file)
        .arg(&asm_file)
        .arg("-o")
        .arg(&exe_file)
        .arg("--run")
        .output()
        .expect("сборка и запуск смешанного проекта");

    let _ = std::fs::remove_file(&gw_file);
    let _ = std::fs::remove_file(&cpp_file);
    let _ = std::fs::remove_file(&cxx_file);
    let _ = std::fs::remove_file(&ll_file);
    let _ = std::fs::remove_file(&hpp_file);
    let _ = std::fs::remove_file(&asm_file);
    if exe_file.exists() {
        let _ = std::fs::remove_file(&exe_file);
    }

    assert!(output.status.success(), "ошибка сборки/запуска: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("s=42, m=42, sub=42, sc=42, bw=42"), "неверный вывод смешанного проекта: {stdout}");
}

#[test]
fn test_e2e_full_cycle_transpilation_matrix() {
    let goraw_bin = env!("CARGO_BIN_EXE_goraw");
    let examples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    let math_gw = examples_dir.join("math.gw");
    let temp_dir = std::env::temp_dir();

    let stage1_ll = temp_dir.join("matrix_s1.ll");
    let stage2_cpp = temp_dir.join("matrix_s2.cpp");
    let stage3_gw = temp_dir.join("matrix_s3.gw");
    let stage4_cpp = temp_dir.join("matrix_s4.cpp");
    let stage5_ll = temp_dir.join("matrix_s5.ll");
    let stage6_gw = temp_dir.join("matrix_s6.gw");
    let final_exe = temp_dir.join("matrix_final.exe");

    // 1. gw -> ll
    let s1 = Command::new(goraw_bin)
        .arg(&math_gw).arg("--emit-llvm").arg("-o").arg(&stage1_ll).status().expect("s1");
    assert!(s1.success());

    // 2. ll -> cpp
    let s2 = Command::new(goraw_bin)
        .arg(&stage1_ll).arg("--emit-cpp").arg("-o").arg(&stage2_cpp).status().expect("s2");
    assert!(s2.success());

    // 3. cpp -> gw
    let s3 = Command::new(goraw_bin)
        .arg(&stage2_cpp).arg("--emit-gw").arg("-o").arg(&stage3_gw).status().expect("s3");
    assert!(s3.success());

    // 4. gw -> cpp
    let s4 = Command::new(goraw_bin)
        .arg(&stage3_gw).arg("--emit-cpp").arg("-o").arg(&stage4_cpp).status().expect("s4");
    assert!(s4.success());

    // 5. cpp -> ll
    let s5 = Command::new(goraw_bin)
        .arg(&stage4_cpp).arg("--emit-llvm").arg("-o").arg(&stage5_ll).status().expect("s5");
    assert!(s5.success());

    // 6. ll -> gw
    let s6 = Command::new(goraw_bin)
        .arg(&stage5_ll).arg("--emit-gw").arg("-o").arg(&stage6_gw).status().expect("s6");
    assert!(s6.success());

    // 7. Сборка и запуск финального результата после 6-шагового полного цикла транспиляции
    let run_out = Command::new(goraw_bin)
        .arg(&stage6_gw).arg("-o").arg(&final_exe).arg("--run").output().expect("final run");

    // Очистка временных файлов
    let _ = std::fs::remove_file(&stage1_ll);
    let _ = std::fs::remove_file(&stage2_cpp);
    let _ = std::fs::remove_file(&stage3_gw);
    let _ = std::fs::remove_file(&stage4_cpp);
    let _ = std::fs::remove_file(&stage5_ll);
    let _ = std::fs::remove_file(&stage6_gw);
    if final_exe.exists() {
        let _ = std::fs::remove_file(&final_exe);
    }

    assert!(run_out.status.success(), "финальный запуск завершился с ошибкой: {}", String::from_utf8_lossy(&run_out.stderr));
    let stdout = String::from_utf8_lossy(&run_out.stdout);
    assert!(stdout.contains("hypot(3,4)      = 5.000000"), "неверный вывод hypot: {stdout}");
    assert!(stdout.contains("pow(2, 10)      = 1024.000000"), "неверный вывод pow: {stdout}");
}
