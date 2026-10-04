//! Встроенный нативный LLVM / Clang / Third-party стек на языке Goraw (.gw).
//!
//! Все компоненты транслированы в чистый .gw код и хранятся локально:
//! - LLVM SelectionDAG (FastISel, Legalize, ScheduleDAG)
//! - LLVM Target X86-64 (генерация инструкций и опкодов x86_64)
//! - LLVM Target AArch64 (ARM64 генерация)
//! - LLD / LLVM Linker (компоновка COFF / PE / ELF)
//! - LLVM InstCombine (комбинация и свертка инструкций)
//! - Clang Lexer & Clang Parser (транслированный C++ фронтенд)
//! - Third-party std/alloc (Microsoft mimalloc аллокатор)
//! - Third-party std/crypto/blake3 (криптографический хэшер BLAKE3)

use std::path::PathBuf;

/// Манифест встроенного бэкенда
pub const BACKEND_ENTRY: &str = include_str!("../backend/mod.gw");

/// Находит корневую директорию проекта Goraw (где лежат backend/ и std/)
pub fn find_root_dir() -> Option<PathBuf> {
    // 1. Через переменную окружения GORAW_ROOT / GORAW_HOME
    if let Ok(root) = std::env::var("GORAW_ROOT").or_else(|_| std::env::var("GORAW_HOME")) {
        let p = PathBuf::from(root);
        if p.exists() {
            return Some(p);
        }
    }

    // 2. Через найденную директорию backend/
    if let Some(b) = find_backend_dir() {
        if let Some(parent) = b.parent() {
            return Some(parent.to_path_buf());
        }
    }

    // 3. Через текущую директорию и ее предков
    if let Ok(cwd) = std::env::current_dir() {
        let mut curr = cwd;
        for _ in 0..5 {
            if curr.join("backend").join("mod.gw").exists() || curr.join("std").exists() {
                return Some(curr);
            }
            if let Some(p) = curr.parent() {
                curr = p.to_path_buf();
            } else {
                break;
            }
        }
    }

    None
}

/// Находит корневую директорию backend/ с модулями .gw
pub fn find_backend_dir() -> Option<PathBuf> {
    // 1. Через переменную окружения GORAW_BACKEND
    if let Ok(b) = std::env::var("GORAW_BACKEND") {
        let p = PathBuf::from(b);
        if p.join("mod.gw").exists() {
            return Some(p);
        }
    }

    // 2. Относительно текущей директории и предков
    if let Ok(cwd) = std::env::current_dir() {
        let mut curr = cwd;
        for _ in 0..5 {
            let candidate = curr.join("backend");
            if candidate.join("mod.gw").exists() {
                return Some(candidate);
            }
            if let Some(p) = curr.parent() {
                curr = p.to_path_buf();
            } else {
                break;
            }
        }
    }

    // 3. Относительно директории исполняемого файла goraw.exe
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let mut curr = parent.to_path_buf();
            for _ in 0..6 {
                let candidate = curr.join("backend");
                if candidate.join("mod.gw").exists() {
                    return Some(candidate);
                }
                if let Some(p) = curr.parent() {
                    curr = p.to_path_buf();
                } else {
                    break;
                }
            }
        }
    }

    // 4. Домашний каталог пользователя ~/.goraw/backend
    if let Some(home) = dirs_home() {
        let user_backend = home.join(".goraw").join("backend");
        if user_backend.join("mod.gw").exists() {
            return Some(user_backend);
        }
    }

    None
}

/// Находит корневую директорию std/ с модулями .gw
pub fn find_std_dir() -> Option<PathBuf> {
    if let Some(root) = find_root_dir() {
        let s = root.join("std");
        if s.exists() {
            return Some(s);
        }
    }

    if let Some(home) = dirs_home() {
        let user_std = home.join(".goraw").join("std");
        if user_std.exists() {
            return Some(user_std);
        }
    }

    None
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .map(PathBuf::from)
}

/// Проверяет доступность .gw LLVM бэкенда
pub fn is_backend_available() -> bool {
    find_backend_dir().is_some()
}

/// Возвращает путь к точке входа backend/mod.gw
pub fn backend_mod_path() -> Option<PathBuf> {
    find_backend_dir().map(|d| d.join("mod.gw"))
}

/// Описание подключенного стека модулей на GW
pub fn get_stack_summary() -> Vec<(&'static str, &'static str, bool)> {
    let root = find_root_dir();
    let check = |sub: &str| -> bool {
        root.as_ref().map(|r| r.join(sub).exists()).unwrap_or(false)
    };

    vec![
        ("LLVM SelectionDAG", "backend/selectiondag", check("backend/selectiondag")),
        ("LLVM X86-64 Target", "backend/x86", check("backend/x86")),
        ("LLVM AArch64 Target", "backend/aarch64", check("backend/aarch64")),
        ("LLVM InstCombine", "backend/opt/instcombine", check("backend/opt/instcombine")),
        ("LLD Linker", "backend/linker", check("backend/linker")),
        ("Clang Lexer", "backend/clang_lex", check("backend/clang_lex")),
        ("Clang Parser", "backend/clang_parse", check("backend/clang_parse")),
        ("mimalloc (std/alloc)", "std/alloc", check("std/alloc")),
        ("BLAKE3 (std/crypto/blake3)", "std/crypto/blake3", check("std/crypto/blake3")),
    ]
}
