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
pub mod proto;
pub mod types;
