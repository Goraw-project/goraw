use std::collections::HashMap;
use std::io::{self, BufRead, Read, Write};

use crate::lsp::analysis::DocumentAnalysis;
use crate::lsp::protocol::*;

pub struct LspServer {
    documents: HashMap<String, DocumentAnalysis>,
}

impl LspServer {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
        }
    }

    pub fn run(&mut self) -> io::Result<()> {
        let stdin = io::stdin();
        let mut reader = io::BufReader::new(stdin.lock());
        let stdout = io::stdout();
        let mut writer = io::BufWriter::new(stdout.lock());

        loop {
            // Читаем заголовки JSON-RPC (Content-Length)
            let mut content_length: Option<usize> = None;
            let mut line = String::new();

            loop {
                line.clear();
                if reader.read_line(&mut line)? == 0 {
                    return Ok(()); // EOF
                }
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    break; // Конец заголовков
                }
                if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
                    if let Ok(len) = rest.trim().parse::<usize>() {
                        content_length = Some(len);
                    }
                }
            }

            let len = match content_length {
                Some(l) => l,
                None => continue,
            };

            let mut body_bytes = vec![0u8; len];
            reader.read_exact(&mut body_bytes)?;
            let body_str = match std::str::from_utf8(&body_bytes) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let v: serde_json::Value = match serde_json::from_str(body_str) {
                Ok(val) => val,
                Err(_) => continue,
            };

            self.handle_message(v, &mut writer)?;
        }
    }

    fn handle_message<W: Write>(
        &mut self,
        v: serde_json::Value,
        writer: &mut W,
    ) -> io::Result<()> {
        let method = match v.get("method").and_then(|m| m.as_str()) {
            Some(m) => m,
            None => return Ok(()),
        };
        let id = v.get("id").cloned();

        match method {
            "initialize" => {
                let res = InitializeResult {
                    capabilities: ServerCapabilities {
                        text_document_sync: 1, // Full sync
                        hover_provider: true,
                        definition_provider: true,
                        completion_provider: CompletionOptions {
                            trigger_characters: vec![".".to_string(), ":".to_string()],
                        },
                    },
                };
                if let Some(req_id) = id {
                    send_response(writer, req_id, res)?;
                }
            }
            "initialized" => {
                // Сервер готов
            }
            "textDocument/didOpen" => {
                if let Some(params) = v.get("params") {
                    if let Ok(p) = serde_json::from_value::<DidOpenTextDocumentParams>(params.clone()) {
                        let doc = DocumentAnalysis::new(p.text_document.uri.clone(), p.text_document.text);
                        let diags = doc.diagnostics.clone();
                        self.documents.insert(p.text_document.uri.clone(), doc);

                        // Отправляем диагностики клиенту
                        send_notification(
                            writer,
                            "textDocument/publishDiagnostics",
                            PublishDiagnosticsParams {
                                uri: p.text_document.uri,
                                diagnostics: diags,
                            },
                        )?;
                    }
                }
            }
            "textDocument/didChange" => {
                if let Some(params) = v.get("params") {
                    if let Ok(p) = serde_json::from_value::<DidChangeTextDocumentParams>(params.clone()) {
                        if let Some(last_change) = p.content_changes.into_iter().last() {
                            let doc = DocumentAnalysis::new(p.text_document.uri.clone(), last_change.text);
                            let diags = doc.diagnostics.clone();
                            self.documents.insert(p.text_document.uri.clone(), doc);

                            // Отправляем обновлённые диагностики
                            send_notification(
                                writer,
                                "textDocument/publishDiagnostics",
                                PublishDiagnosticsParams {
                                    uri: p.text_document.uri,
                                    diagnostics: diags,
                                },
                            )?;
                        }
                    }
                }
            }
            "textDocument/didClose" => {
                if let Some(params) = v.get("params") {
                    if let Some(uri) = params.get("textDocument").and_then(|td| td.get("uri")).and_then(|u| u.as_str()) {
                        self.documents.remove(uri);
                    }
                }
            }
            "textDocument/hover" => {
                if let Some(req_id) = id {
                    let mut hover_res = None;
                    if let Some(params) = v.get("params") {
                        if let Ok(p) = serde_json::from_value::<TextDocumentPositionParams>(params.clone()) {
                            if let Some(doc) = self.documents.get(&p.text_document.uri) {
                                hover_res = doc.hover(p.position);
                            }
                        }
                    }
                    send_response(writer, req_id, hover_res)?;
                }
            }
            "textDocument/definition" => {
                if let Some(req_id) = id {
                    let mut def_res = None;
                    if let Some(params) = v.get("params") {
                        if let Ok(p) = serde_json::from_value::<TextDocumentPositionParams>(params.clone()) {
                            if let Some(doc) = self.documents.get(&p.text_document.uri) {
                                def_res = doc.definition(p.position);
                            }
                        }
                    }
                    send_response(writer, req_id, def_res)?;
                }
            }
            "textDocument/completion" => {
                if let Some(req_id) = id {
                    let mut comp_res = Vec::new();
                    if let Some(params) = v.get("params") {
                        if let Ok(p) = serde_json::from_value::<TextDocumentPositionParams>(params.clone()) {
                            if let Some(doc) = self.documents.get(&p.text_document.uri) {
                                comp_res = doc.completion(p.position);
                            }
                        }
                    }
                    send_response(writer, req_id, comp_res)?;
                }
            }
            "shutdown" => {
                if let Some(req_id) = id {
                    send_response(writer, req_id, serde_json::Value::Null)?;
                }
            }
            "exit" => {
                std::process::exit(0);
            }
            _ => {
                // Неподдерживаемый метод, если это запрос с id — шлём пустой ответ
                if let Some(req_id) = id {
                    send_response(writer, req_id, serde_json::Value::Null)?;
                }
            }
        }

        Ok(())
    }
}

fn send_response<W: Write, T: serde::Serialize>(
    writer: &mut W,
    id: serde_json::Value,
    result: T,
) -> io::Result<()> {
    let res = Response {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(result),
        error: None,
    };
    let json_bytes = serde_json::to_vec(&res)?;
    write!(writer, "Content-Length: {}\r\n\r\n", json_bytes.len())?;
    writer.write_all(&json_bytes)?;
    writer.flush()
}

fn send_notification<W: Write, T: serde::Serialize>(
    writer: &mut W,
    method: &str,
    params: T,
) -> io::Result<()> {
    let notif = Notification {
        jsonrpc: "2.0".to_string(),
        method: method.to_string(),
        params,
    };
    let json_bytes = serde_json::to_vec(&notif)?;
    write!(writer, "Content-Length: {}\r\n\r\n", json_bytes.len())?;
    writer.write_all(&json_bytes)?;
    writer.flush()
}
