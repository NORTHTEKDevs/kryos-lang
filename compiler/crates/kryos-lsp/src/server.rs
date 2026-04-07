//! LSP server — main event loop handling JSON-RPC messages.

use std::collections::HashMap;
use serde_json::Value;

use crate::protocol::{self, Message};
use crate::diagnostics;
use crate::completion;
use crate::hover;
use crate::goto_def;

/// The Kryos Language Server.
pub struct LspServer {
    /// File contents indexed by URI.
    documents: HashMap<String, String>,
    /// Whether the server has been initialized.
    initialized: bool,
    /// Whether shutdown was requested.
    shutdown_requested: bool,
}

impl LspServer {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
            initialized: false,
            shutdown_requested: false,
        }
    }

    /// Run the LSP server on stdin/stdout.
    pub fn run(&mut self) {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let mut reader = stdin.lock();
        let mut writer = stdout.lock();

        loop {
            let msg = match protocol::read_message(&mut reader) {
                Ok(Some(value)) => value,
                Ok(None) => break, // EOF
                Err(_) => continue,
            };

            let parsed = match protocol::parse_message(msg) {
                Some(m) => m,
                None => continue,
            };

            match parsed {
                Message::Request(req) => {
                    let response = self.handle_request(&req.method, &req.id, &req.params);
                    if let Some(resp) = response {
                        let _ = protocol::write_message(&mut writer, &resp);
                    }
                }
                Message::Notification(notif) => {
                    let notifications = self.handle_notification(&notif.method, &notif.params);
                    for n in notifications {
                        let _ = protocol::write_message(&mut writer, &n);
                    }
                }
                Message::Response(_) => {} // We don't send requests, so ignore responses
            }

            if self.shutdown_requested {
                break;
            }
        }
    }

    pub fn handle_request(&mut self, method: &str, id: &Value, params: &Value) -> Option<Value> {
        match method {
            "initialize" => {
                self.initialized = true;
                Some(protocol::make_response(id.clone(), serde_json::json!({
                    "capabilities": {
                        "textDocumentSync": {
                            "openClose": true,
                            "change": 1, // Full sync
                        },
                        "completionProvider": {
                            "triggerCharacters": [".", ":"],
                        },
                        "hoverProvider": true,
                        "definitionProvider": true,
                    },
                    "serverInfo": {
                        "name": "kryos-lsp",
                        "version": "0.1.0",
                    }
                })))
            }
            "shutdown" => {
                self.shutdown_requested = true;
                Some(protocol::make_response(id.clone(), Value::Null))
            }
            "textDocument/completion" => {
                let uri = params.pointer("/textDocument/uri")?.as_str()?;
                let line = params.pointer("/position/line")?.as_u64()? as u32;
                let character = params.pointer("/position/character")?.as_u64()? as u32;

                if let Some(source) = self.documents.get(uri) {
                    let items = completion::get_completions(source, line, character);
                    Some(protocol::make_response(id.clone(), serde_json::json!({ "items": items })))
                } else {
                    Some(protocol::make_response(id.clone(), serde_json::json!({ "items": [] })))
                }
            }
            "textDocument/hover" => {
                let uri = params.pointer("/textDocument/uri")?.as_str()?;
                let line = params.pointer("/position/line")?.as_u64()? as u32;
                let character = params.pointer("/position/character")?.as_u64()? as u32;

                if let Some(source) = self.documents.get(uri) {
                    if let Some(hover_result) = hover::get_hover(source, line, character) {
                        Some(protocol::make_response(id.clone(), hover_result))
                    } else {
                        Some(protocol::make_response(id.clone(), Value::Null))
                    }
                } else {
                    Some(protocol::make_response(id.clone(), Value::Null))
                }
            }
            "textDocument/definition" => {
                let uri = params.pointer("/textDocument/uri")?.as_str()?;
                let line = params.pointer("/position/line")?.as_u64()? as u32;
                let character = params.pointer("/position/character")?.as_u64()? as u32;

                if let Some(source) = self.documents.get(uri) {
                    if let Some(location) = goto_def::goto_definition(source, line, character) {
                        let mut result = location;
                        result["uri"] = Value::String(uri.to_string());
                        Some(protocol::make_response(id.clone(), result))
                    } else {
                        Some(protocol::make_response(id.clone(), Value::Null))
                    }
                } else {
                    Some(protocol::make_response(id.clone(), Value::Null))
                }
            }
            _ => {
                Some(protocol::make_error_response(id.clone(), -32601, &format!("method not found: {method}")))
            }
        }
    }

    pub fn handle_notification(&mut self, method: &str, params: &Value) -> Vec<Value> {
        match method {
            "initialized" => Vec::new(),
            "textDocument/didOpen" => {
                if let (Some(uri), Some(text)) = (
                    params.pointer("/textDocument/uri").and_then(|v| v.as_str()),
                    params.pointer("/textDocument/text").and_then(|v| v.as_str()),
                ) {
                    self.documents.insert(uri.to_string(), text.to_string());
                    self.publish_diagnostics(uri)
                } else {
                    Vec::new()
                }
            }
            "textDocument/didChange" => {
                if let (Some(uri), Some(changes)) = (
                    params.pointer("/textDocument/uri").and_then(|v| v.as_str()),
                    params.get("contentChanges").and_then(|v| v.as_array()),
                ) {
                    // Full sync — take the last change's text
                    if let Some(last) = changes.last() {
                        if let Some(text) = last.get("text").and_then(|v| v.as_str()) {
                            self.documents.insert(uri.to_string(), text.to_string());
                            return self.publish_diagnostics(uri);
                        }
                    }
                }
                Vec::new()
            }
            "textDocument/didClose" => {
                if let Some(uri) = params.pointer("/textDocument/uri").and_then(|v| v.as_str()) {
                    self.documents.remove(uri);
                    // Clear diagnostics
                    vec![diagnostics::publish_diagnostics(uri, Vec::new())]
                } else {
                    Vec::new()
                }
            }
            "exit" => {
                std::process::exit(if self.shutdown_requested { 0 } else { 1 });
            }
            _ => Vec::new(),
        }
    }

    fn publish_diagnostics(&self, uri: &str) -> Vec<Value> {
        if let Some(source) = self.documents.get(uri) {
            let (diags, _) = diagnostics::check_source(source, uri);
            vec![diagnostics::publish_diagnostics(uri, diags)]
        } else {
            Vec::new()
        }
    }
}

impl Default for LspServer {
    fn default() -> Self {
        Self::new()
    }
}
