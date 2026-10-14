//! Библиотека Goraw: общие модули для компилятора `gorawc` и
//! ассемблера `gorawas`.

// Глобальный аллокатор mimalloc (статически слинкован, см. build.rs). Ускоряет
// все бинари Goraw и убирает зависимость от аллокатора системного CRT. Включается
// только когда линкуется статический набор (cfg goraw_static_llvm).
#[cfg(goraw_static_llvm)]
mod mi_alloc {
    use std::alloc::{GlobalAlloc, Layout};
    use std::os::raw::c_void;
    unsafe extern "C" {
        fn mi_malloc_aligned(size: usize, alignment: usize) -> *mut c_void;
        fn mi_realloc_aligned(p: *mut c_void, newsize: usize, alignment: usize) -> *mut c_void;
        fn mi_zalloc_aligned(size: usize, alignment: usize) -> *mut c_void;
        fn mi_free(p: *mut c_void);
    }
    pub struct MiMalloc;
    unsafe impl GlobalAlloc for MiMalloc {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            unsafe { mi_malloc_aligned(l.size(), l.align()) as *mut u8 }
        }
        unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
            unsafe { mi_zalloc_aligned(l.size(), l.align()) as *mut u8 }
        }
        unsafe fn dealloc(&self, p: *mut u8, _l: Layout) {
            unsafe { mi_free(p as *mut c_void) }
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, new_size: usize) -> *mut u8 {
            unsafe { mi_realloc_aligned(p as *mut c_void, new_size, l.align()) as *mut u8 }
        }
    }
}
#[cfg(goraw_static_llvm)]
#[global_allocator]
static GLOBAL_ALLOC: mi_alloc::MiMalloc = mi_alloc::MiMalloc;

pub mod asm;
pub mod ast;
pub mod c_interop;
pub mod codegen;
pub mod diag;
pub mod lexer;
pub mod lsp;
pub mod mono;
pub mod parser;
pub mod pkg;
pub mod proto;
pub mod types;
pub mod cpp_transpiler;
pub mod cpp_to_goraw;
pub mod llvm_to_goraw;
pub mod opcodes;
pub mod backend;
pub mod linker;
pub mod x86_codegen;
pub mod native_cc;
