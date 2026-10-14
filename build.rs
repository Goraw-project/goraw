// Сборочный скрипт gorawc: статически линкует предсобранные LLVM+clang либы
// (X86-only, MSVC ABI), mimalloc и blake3, плюс C++-шим runtime/goraw_cc.cpp.
// Так gorawc перестаёт зависеть от системного LLVM/Clang.
//
// Пути НИЧЕГО не хардкодят. Порядок поиска каждого каталога:
//   1) env-переменная (см. ниже) — явное переопределение;
//   2) относительно корня крейта (CARGO_MANIFEST_DIR) — раскладка по умолчанию;
//   3) если не нашлось — внятный panic с подсказкой, что собрать и какой env задать.
//
// Env-переопределения:
//   GORAW_LLVM_BUILD   — каталог сборки LLVM (…/lib/*.lib, …/include)
//   GORAW_LLVM_SRC     — каталог исходников llvm-project
//   GORAW_MIMALLOC_LIB — каталог с mimalloc.lib
//   GORAW_BLAKE3_LIB   — каталог с blake3.lib

use std::path::{Path, PathBuf};

/// Каталог: сперва env, иначе первый существующий из кандидатов (относительно
/// корня крейта). Возвращает None, если ничего не найдено.
fn locate(env_key: &str, manifest: &Path, candidates: &[&str]) -> Option<PathBuf> {
    println!("cargo:rerun-if-env-changed={env_key}");
    if let Ok(v) = std::env::var(env_key) {
        return Some(PathBuf::from(v));
    }
    for rel in candidates {
        let p = manifest.join(rel);
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

fn main() {
    // cfg(goraw_static_llvm) включается ТОЛЬКО когда предсобранные LLVM+clang либы
    // реально найдены. Иначе статическая линковка отключается (gorawc работает через
    // системный clang / нативный backend) — важно для CI без предсобранных либ.
    println!("cargo:rustc-check-cfg=cfg(goraw_static_llvm)");

    // На не-Windows пока не поддерживаем статическую линковку LLVM — выходим тихо.
    if !cfg!(target_os = "windows") {
        return;
    }

    let manifest = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"),
    );

    // Раскладка по умолчанию: llvm-build — сосед корня крейта (../llvm-build) или
    // вложен (./llvm-build); llvm-project/mimalloc/blake3 — внутри крейта.
    let found = (|| {
        Some((
            locate("GORAW_LLVM_BUILD", &manifest, &["../llvm-build", "llvm-build"])?,
            locate("GORAW_LLVM_SRC", &manifest, &["llvm-project", "../llvm-project"])?,
            locate("GORAW_MIMALLOC_LIB", &manifest, &["mimalloc/build", "../mimalloc/build"])?,
            locate("GORAW_BLAKE3_LIB", &manifest, &["blake3_repo/c/build", "../blake3_repo/c/build"])?,
        ))
    })();

    let Some((llvm_build, llvm_src, mimalloc_lib, blake3_lib)) = found else {
        println!(
            "cargo:warning=Goraw: предсобранные LLVM+clang либы не найдены — \
             статическая линковка отключена (сборка идёт в режиме внешнего clang). \
             Чтобы включить, собери либы (см. memory/goraw-static-llvm) и задай \
             GORAW_LLVM_BUILD/GORAW_LLVM_SRC/GORAW_MIMALLOC_LIB/GORAW_BLAKE3_LIB."
        );
        return;
    };

    let llvm_build = llvm_build.to_string_lossy().replace('\\', "/");
    let llvm_src = llvm_src.to_string_lossy().replace('\\', "/");
    let llvm_lib_dir = format!("{llvm_build}/lib");
    if !Path::new(&llvm_lib_dir).is_dir() {
        println!(
            "cargo:warning=Goraw: в {llvm_build} нет подкаталога lib/ — \
             статическая линковка отключена."
        );
        return;
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

    // --- 2. Resource-dir clang (встроенные заголовки) — находим и запекаем ---
    // Путь кладём в компайл-тайм env, чтобы в бинаре не было хардкода; рантайм
    // всё равно может переопределить его через GORAW_CLANG_RESOURCE_DIR.
    if let Some(rd) = find_resource_dir(&llvm_lib_dir) {
        println!("cargo:rustc-env=GORAW_CLANG_RESOURCE_DIR_BUILTIN={rd}");
    }

    // --- 3. Пути поиска либ ---
    println!("cargo:rustc-link-search=native={llvm_lib_dir}");
    println!(
        "cargo:rustc-link-search=native={}",
        mimalloc_lib.to_string_lossy().replace('\\', "/")
    );
    println!(
        "cargo:rustc-link-search=native={}",
        blake3_lib.to_string_lossy().replace('\\', "/")
    );

    // --- 4. Линкуем все LLVM/clang статиклибы (MSVC link тянет только нужное) ---
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

    // --- 5. mimalloc + blake3 ---
    println!("cargo:rustc-link-lib=static=mimalloc");
    println!("cargo:rustc-link-lib=static=blake3");

    // --- 6. Системные либы, нужные LLVM на Windows ---
    for sys in [
        "ole32", "uuid", "psapi", "shell32", "advapi32", "user32", "kernel32",
        "ws2_32", "version", "ntdll",
    ] {
        println!("cargo:rustc-link-lib=dylib={sys}");
    }

    // --- 7. Статическая линковка активна — включаем cfg для кода ---
    println!("cargo:rustc-cfg=goraw_static_llvm");
}

/// Ищет каталог встроенных заголовков clang: <llvm_lib_dir>/clang/<ver>/include,
/// в котором есть stddef.h.
fn find_resource_dir(llvm_lib_dir: &str) -> Option<String> {
    let base = Path::new(llvm_lib_dir).join("clang");
    let rd = std::fs::read_dir(&base).ok()?;
    for e in rd.flatten() {
        let inc = e.path().join("include");
        if inc.join("stddef.h").exists() {
            return Some(inc.to_string_lossy().replace('\\', "/"));
        }
    }
    None
}
