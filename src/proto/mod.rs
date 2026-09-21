//! `gorawpb` — компилятор Protobuf (Editions) в код на Goraw.
//! Clean-room реализация: wire-формат и спека Editions публичны, код Google
//! не используется. Пайплайн: .proto -> AST -> дескрипторы (+резолвинг
//! features) -> кодоген в Goraw.

pub mod ast;
pub mod codegen_goraw;
pub mod descriptor;
pub mod parser;

use crate::diag::Diags;

/// Компилирует `.proto` в исходник Goraw. При ошибках возвращает None и Diags.
pub fn compile(file: &str, src: &str) -> (Option<String>, Diags) {
    let mut diags = Diags::new(file, src);
    let ast = {
        let mut p = parser::Parser::new(src, &mut diags);
        p.parse_file()
    };
    if diags.has_errors() {
        return (None, diags);
    }
    let desc = descriptor::resolve(&ast);
    let code = codegen_goraw::generate(&desc);
    (Some(code), diags)
}
