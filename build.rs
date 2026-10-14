// Сборочный скрипт gorawc: статически линкует предсобранные LLVM+clang либы
// (X86-only, MSVC ABI), mimalloc и blake3, плюс C++-шим runtime/goraw_cc.cpp.
// Так gorawc перестаёт зависеть от системного LLVM/Clang.
//
// Пути к предсобранным либам задаются через env (со значениями по умолчанию
// под текущую машину):
//   GORAW_LLVM_BUILD   — каталог сборки LLVM (…/lib/*.lib, …/include)
//   GORAW_LLVM_SRC     — каталог исходников llvm-project
//   GORAW_MIMALLOC_LIB — каталог с mimalloc.lib
//   GORAW_BLAKE3_LIB   — каталог с blake3.lib

use std::path::{Path, PathBuf};

fn env_or(key: &str, default: &str) -> String {
    println!("cargo:rerun-if-env-changed={key}");
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn main() {
    // На не-Windows пока не поддерживаем статическую линковку LLVM — выходим тихо.
    if !cfg!(target_os = "windows") {
        return;
    }

    let llvm_build = env_or("GORAW_LLVM_BUILD", "P:/llvm-build");
    let llvm_src = env_or("GORAW_LLVM_SRC", "P:/Goraw/llvm-project");
    let mimalloc_lib = env_or("GORAW_MIMALLOC_LIB", "P:/Goraw/mimalloc/build");
    let blake3_lib = env_or("GORAW_BLAKE3_LIB", "P:/Goraw/blake3_repo/c/build");

    let llvm_lib_dir = format!("{llvm_build}/lib");
    if !Path::new(&llvm_lib_dir).is_dir() {
        panic!(
            "GORAW_LLVM_BUILD={llvm_build}: нет каталога либ {llvm_lib_dir}. \
             Сначала собери статические LLVM+clang либы (см. memory/goraw-static-llvm)."
        );
    }

    // --- 1. Компилируем C++-шим в статическую либу goraw_cc.lib ---
    // В статической либе (а не объектнике) линкер подтянет шим только в те
    // бинари, что реально его зовут — прочие бинари не тащат LLVM.
    let mut cc = cc::Build::new();
    cc.cpp(true)
        .compiler("clang-cl")
        .file("runtime/goraw_cc.cpp")
        .flag("/std:c++17")
        .flag("/EHs-c-")
        .flag("/GR-")
        .flag("/W0")
        .flag("/DNDEBUG")
        .include(format!("{llvm_build}/include"))
        .include(format!("{llvm_src}/llvm/include"))
        .include(format!("{llvm_build}/tools/clang/include"))
        .include(format!("{llvm_src}/clang/include"));
    cc.compile("goraw_cc");
    println!("cargo:rerun-if-changed=runtime/goraw_cc.cpp");

    // --- 2. Пути поиска либ ---
    println!("cargo:rustc-link-search=native={llvm_lib_dir}");
    println!("cargo:rustc-link-search=native={mimalloc_lib}");
    println!("cargo:rustc-link-search=native={blake3_lib}");

    // --- 3. Линкуем все LLVM/clang статиклибы (MSVC link тянет только нужное) ---
    let mut names: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&llvm_lib_dir).expect("read llvm lib dir") {
        let p: PathBuf = entry.unwrap().path();
        if p.extension().and_then(|e| e.to_str()) == Some("lib") {
            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_string());
            }
        }
    }
    names.sort();
    for n in &names {
        println!("cargo:rustc-link-lib=static={n}");
    }

    // --- 4. mimalloc + blake3 ---
    println!("cargo:rustc-link-lib=static=mimalloc");
    println!("cargo:rustc-link-lib=static=blake3");

    // --- 5. Системные либы, нужные LLVM на Windows ---
    for sys in [
        "ole32", "uuid", "psapi", "shell32", "advapi32", "user32", "kernel32",
        "ws2_32", "version", "ntdll",
    ] {
        println!("cargo:rustc-link-lib=dylib={sys}");
    }
}
