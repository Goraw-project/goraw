//! Библиотека Goraw: общие модули для компилятора `gorawc` и
//! ассемблера `gorawas`.

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
