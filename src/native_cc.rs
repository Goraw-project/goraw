//! Безопасные обёртки над статически слинкованным C++-шимом `runtime/goraw_cc.cpp`.
//!
//! Шим содержит встроенные LLVM (X86 codegen) и clang (frontend/tooling), так что
//! gorawc выполняет IR→obj и операции с C/C++ исходниками БЕЗ системного LLVM/Clang.
//!
//! На не-Windows целях шим не линкуется (см. build.rs) — здесь функции вернут
//! ошибку, вызывающий код должен иметь фолбэк (внешний clang / нативный backend).

#![allow(dead_code)]

#[cfg(windows)]
mod ffi {
    use std::os::raw::{c_char, c_int};
    unsafe extern "C" {
        pub fn goraw_llc(
            ir_text: *const c_char,
            out_path: *const c_char,
            emit_obj: c_int,
            opt_level: c_int,
            triple: *const c_char,
            err_out: *mut *mut c_char,
        ) -> c_int;

        pub fn goraw_cc_emit_bc(
            source: *const c_char,
            is_cpp: c_int,
            argv: *const *const c_char,
            argc: c_int,
            err_out: *mut *mut c_char,
        ) -> c_int;

        pub fn goraw_cc_emit_obj(
            source: *const c_char,
            is_cpp: c_int,
            argv: *const *const c_char,
            argc: c_int,
            err_out: *mut *mut c_char,
        ) -> c_int;

        pub fn goraw_cc_ast_json(
            source: *const c_char,
            is_cpp: c_int,
            argv: *const *const c_char,
            argc: c_int,
            filter: *const c_char,
        ) -> *mut c_char;

        pub fn goraw_cc_free(p: *mut c_char);
    }
}

/// Доступен ли встроенный шим на этой сборке/платформе.
pub const AVAILABLE: bool = cfg!(windows);

/// Каталог встроенных заголовков clang (resource-dir, где stddef.h и пр.).
/// Нужен встроенному clang, т.к. у него нет инсталляции на диске.
///
/// Порядок поиска (без хардкода путей):
///   1) env GORAW_CLANG_RESOURCE_DIR — явное переопределение в рантайме;
///   2) путь, запечённый build.rs (GORAW_CLANG_RESOURCE_DIR_BUILTIN);
///   3) рядом с исполняемым файлом: <exe_dir>/clang-rt/include и
///      <exe_dir>/lib/clang/<ver>/include (на случай переносимой раскладки).
pub fn clang_resource_dir() -> Option<std::path::PathBuf> {
    let has_stddef = |p: &std::path::Path| p.join("stddef.h").exists();

    if let Ok(p) = std::env::var("GORAW_CLANG_RESOURCE_DIR") {
        let pb = std::path::PathBuf::from(p);
        if has_stddef(&pb) {
            return Some(pb);
        }
    }

    if let Some(p) = option_env!("GORAW_CLANG_RESOURCE_DIR_BUILTIN") {
        let pb = std::path::PathBuf::from(p);
        if has_stddef(&pb) {
            return Some(pb);
        }
    }

    // Переносимая раскладка рядом с бинарём.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let direct = dir.join("clang-rt").join("include");
            if has_stddef(&direct) {
                return Some(direct);
            }
            if let Ok(rd) = std::fs::read_dir(dir.join("lib").join("clang")) {
                for e in rd.flatten() {
                    let inc = e.path().join("include");
                    if has_stddef(&inc) {
                        return Some(inc);
                    }
                }
            }
        }
    }

    None
}

#[cfg(windows)]
mod imp {
    use super::ffi;
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;
    use std::path::Path;

    fn cstr(s: &str) -> CString {
        // Нуль-байты в путях/исходниках недопустимы; заменяем на пробел.
        CString::new(s.replace('\0', " ")).unwrap()
    }

    fn take_err(err: *mut c_char) -> String {
        if err.is_null() {
            return "(без текста)".to_string();
        }
        unsafe {
            let msg = CStr::from_ptr(err).to_string_lossy().into_owned();
            ffi::goraw_cc_free(err);
            msg
        }
    }

    /// Текстовый LLVM IR -> объектный файл (.obj) или ассемблер (.s).
    pub fn llc_to_obj(
        ir_text: &str,
        out_path: &Path,
        emit_obj: bool,
        opt_level: u8,
        triple: &str,
    ) -> Result<(), String> {
        let ir_c = cstr(ir_text);
        let out_c = cstr(&out_path.to_string_lossy());
        let triple_c = cstr(triple);
        let mut err: *mut c_char = std::ptr::null_mut();
        let rc = unsafe {
            ffi::goraw_llc(
                ir_c.as_ptr(),
                out_c.as_ptr(),
                if emit_obj { 1 } else { 0 },
                opt_level as i32,
                triple_c.as_ptr(),
                &mut err,
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(format!("goraw_llc(rc={rc}): {}", take_err(err)))
        }
    }

    /// C/C++ исходник -> LLVM bitcode (.bc). `args` — driver-аргументы; функция
    /// сама добавит `-o <out_path>` и `-emit-llvm -c`.
    pub fn cc_emit_bc(
        source: &str,
        is_cpp: bool,
        args: &[String],
        out_path: &Path,
    ) -> Result<(), String> {
        let mut full: Vec<String> = Vec::with_capacity(args.len() + 4);
        full.extend_from_slice(args);
        full.push("-o".to_string());
        full.push(out_path.to_string_lossy().into_owned());

        let c_args: Vec<CString> = full.iter().map(|a| cstr(a)).collect();
        let ptrs: Vec<*const c_char> = c_args.iter().map(|c| c.as_ptr()).collect();
        let src_c = cstr(source);
        let mut err: *mut c_char = std::ptr::null_mut();
        let rc = unsafe {
            ffi::goraw_cc_emit_bc(
                src_c.as_ptr(),
                if is_cpp { 1 } else { 0 },
                ptrs.as_ptr(),
                ptrs.len() as i32,
                &mut err,
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(format!("goraw_cc_emit_bc(rc={rc}): {}", take_err(err)))
        }
    }

    /// C/C++ исходник -> COFF объектный файл (.obj). `args` — driver-аргументы;
    /// функция сама добавит `-c -o <out_path>`.
    pub fn cc_emit_obj(
        source: &str,
        is_cpp: bool,
        args: &[String],
        out_path: &Path,
    ) -> Result<(), String> {
        let mut full: Vec<String> = Vec::with_capacity(args.len() + 4);
        full.extend_from_slice(args);
        full.push("-o".to_string());
        full.push(out_path.to_string_lossy().into_owned());

        let c_args: Vec<CString> = full.iter().map(|a| cstr(a)).collect();
        let ptrs: Vec<*const c_char> = c_args.iter().map(|c| c.as_ptr()).collect();
        let src_c = cstr(source);
        let mut err: *mut c_char = std::ptr::null_mut();
        let rc = unsafe {
            ffi::goraw_cc_emit_obj(
                src_c.as_ptr(),
                if is_cpp { 1 } else { 0 },
                ptrs.as_ptr(),
                ptrs.len() as i32,
                &mut err,
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(format!("goraw_cc_emit_obj(rc={rc}): {}", take_err(err)))
        }
    }

    /// C/C++ исходник -> AST в JSON. `filter` — ast-dump-filter или None.
    pub fn cc_ast_json(
        source: &str,
        is_cpp: bool,
        args: &[String],
        filter: Option<&str>,
    ) -> Result<String, String> {
        let c_args: Vec<CString> = args.iter().map(|a| cstr(a)).collect();
        let ptrs: Vec<*const c_char> = c_args.iter().map(|c| c.as_ptr()).collect();
        let src_c = cstr(source);
        let filter_c = filter.map(cstr);
        let filter_ptr = filter_c
            .as_ref()
            .map(|c| c.as_ptr())
            .unwrap_or(std::ptr::null());
        let out = unsafe {
            ffi::goraw_cc_ast_json(
                src_c.as_ptr(),
                if is_cpp { 1 } else { 0 },
                ptrs.as_ptr(),
                ptrs.len() as i32,
                filter_ptr,
            )
        };
        if out.is_null() {
            return Err("goraw_cc_ast_json: clang вернул пусто (ошибка парса?)".to_string());
        }
        unsafe {
            let s = CStr::from_ptr(out).to_string_lossy().into_owned();
            ffi::goraw_cc_free(out);
            Ok(s)
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::Path;
    pub fn llc_to_obj(_: &str, _: &Path, _: bool, _: u8, _: &str) -> Result<(), String> {
        Err("встроенный LLVM-шим недоступен на этой платформе".to_string())
    }
    pub fn cc_emit_bc(_: &str, _: bool, _: &[String], _: &Path) -> Result<(), String> {
        Err("встроенный clang-шим недоступен на этой платформе".to_string())
    }
    pub fn cc_emit_obj(_: &str, _: bool, _: &[String], _: &Path) -> Result<(), String> {
        Err("встроенный clang-шим недоступен на этой платформе".to_string())
    }
    pub fn cc_ast_json(_: &str, _: bool, _: &[String], _: Option<&str>) -> Result<String, String> {
        Err("встроенный clang-шим недоступен на этой платформе".to_string())
    }
}

pub use imp::{cc_ast_json, cc_emit_bc, cc_emit_obj, llc_to_obj};
