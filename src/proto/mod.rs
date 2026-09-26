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
    compile_ext(file, src, true)
}

/// Компилирует `.proto` в исходник Goraw с возможностью отключить повтор prelude.
pub fn compile_ext(file: &str, src: &str, include_prelude: bool) -> (Option<String>, Diags) {
    let mut diags = Diags::new(file, src);
    let ast = {
        let mut p = parser::Parser::new(src, &mut diags);
        p.parse_file()
    };
    if diags.has_errors() {
        return (None, diags);
    }
    let desc = descriptor::resolve(&ast);
    let code = codegen_goraw::generate_ext(&desc, include_prelude);
    (Some(code), diags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proto_pb2_compilation() {
        let proto_src = r#"
edition = "2023";
package test;

message Item {
    int32 id = 1;
    string name = 2;
}

message Bundle {
    repeated string tags = 1;
    repeated Item items = 2;
    map<string, int32> dict = 3;
    oneof payload {
        string text = 4;
        int32 code = 5;
    }
}
"#;
        let (code, diags) = compile("test.proto", proto_src);
        assert!(!diags.has_errors(), "diags: {:?}", diags.items);
        let gw = code.expect("generated code");
        assert!(gw.contains("struct Bundle_Dict_Entry"));
        assert!(gw.contains("const BUNDLE_PAYLOAD_NOT_SET: i32 = 0;"));
        assert!(gw.contains("const BUNDLE_PAYLOAD_TEXT: i32 = 4;"));
        assert!(gw.contains("const BUNDLE_PAYLOAD_CODE: i32 = 5;"));
        assert!(gw.contains("encode_Bundle"));
        assert!(gw.contains("decode_Bundle"));
    }

    #[test]
    fn test_proto_service_rpc_compilation() {
        let proto_src = r#"
edition = "2023";
package api;

message HelloRequest {
    string name = 1;
}

message HelloReply {
    string message = 1;
}

service Greeter {
    rpc SayHello (HelloRequest) returns (HelloReply);
}
"#;
        let (code, diags) = compile("greeter.proto", proto_src);
        assert!(!diags.has_errors(), "diags: {:?}", diags.items);
        let gw = code.expect("generated code");
        assert!(gw.contains("const RPC_GREETER_SAYHELLO_ID: i32 = 1;"));
        assert!(gw.contains("fn rpc_Greeter_SayHello_path() -> str"));
        assert!(gw.contains("fn rpc_Greeter_SayHello_encode_request"));
        assert!(gw.contains("fn rpc_Greeter_SayHello_decode_response"));
        assert!(gw.contains("fn rpc_Greeter_SayHello_decode_request"));
        assert!(gw.contains("fn rpc_Greeter_SayHello_encode_response"));
        assert!(gw.contains("fn rpc_Greeter_method_id"));
    }
}

