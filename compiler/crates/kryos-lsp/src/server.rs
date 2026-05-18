//! LSP server — main event loop handling JSON-RPC messages.

use serde_json::Value;
use std::collections::HashMap;

use crate::completion;
use crate::diagnostics;
use crate::document_symbols;
use crate::folding;
use crate::formatting;
use crate::goto_def;
use crate::highlight;
use crate::hover;
use crate::inlay_hints;
use crate::protocol::{self, Message};
use crate::references;
use crate::signature_help;
use crate::workspace_symbols;

/// The Kryos Language Server.
pub struct LspServer {
    /// File contents indexed by URI.
    documents: HashMap<String, String>,
    /// Whether the server has been initialized.
    initialized: bool,
    /// Whether shutdown was requested.
    shutdown_requested: bool,
    /// Workspace root URI from the initialize request (e.g. "file:///C:/project").
    workspace_root: Option<String>,
}

impl LspServer {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
            initialized: false,
            shutdown_requested: false,
            workspace_root: None,
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
                // Capture workspace root URI for cross-file features.
                // Try rootUri first (LSP 3.x), fall back to rootPath.
                if let Some(root_uri) = params.get("rootUri").and_then(|v| v.as_str()) {
                    self.workspace_root = Some(root_uri.to_string());
                } else if let Some(root_path) = params.get("rootPath").and_then(|v| v.as_str()) {
                    // Convert rootPath to a URI
                    let normalized = root_path.replace('\\', "/");
                    if normalized.starts_with('/') {
                        self.workspace_root = Some(format!("file://{normalized}"));
                    } else {
                        self.workspace_root = Some(format!("file:///{normalized}"));
                    }
                }
                Some(protocol::make_response(
                    id.clone(),
                    serde_json::json!({
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
                            "documentSymbolProvider": true,
                            "workspaceSymbolProvider": true,
                            "referencesProvider": true,
                            "renameProvider": true,
                            "documentHighlightProvider": true,
                            "foldingRangeProvider": true,
                            "documentFormattingProvider": true,
                            "signatureHelpProvider": {
                                "triggerCharacters": ["(", ","],
                            },
                            "inlayHintProvider": true,
                        },
                        "serverInfo": {
                            "name": "kryos-lsp",
                            "version": "0.2.0",
                        }
                    }),
                ))
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
                    Some(protocol::make_response(
                        id.clone(),
                        serde_json::json!({ "items": items }),
                    ))
                } else {
                    Some(protocol::make_response(
                        id.clone(),
                        serde_json::json!({ "items": [] }),
                    ))
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
                    // First: try to find the definition in the current file.
                    if let Some(location) = goto_def::goto_definition(source, line, character) {
                        let mut result = location;
                        result["uri"] = Value::String(uri.to_string());
                        return Some(protocol::make_response(id.clone(), result));
                    }

                    // Second: cross-file lookup — scan workspace .kry files.
                    if let Some(ref workspace_root) = self.workspace_root {
                        if let Some(word) = goto_def::word_at_position(source, line, character) {
                            if let Some(location) = goto_def::goto_definition_cross_file(
                                &word,
                                uri,
                                workspace_root,
                                &self.documents,
                            ) {
                                return Some(protocol::make_response(id.clone(), location));
                            }
                        }
                    }

                    Some(protocol::make_response(id.clone(), Value::Null))
                } else {
                    Some(protocol::make_response(id.clone(), Value::Null))
                }
            }
            "textDocument/documentSymbol" => {
                let uri = params.pointer("/textDocument/uri")?.as_str()?;
                let result = if let Some(source) = self.documents.get(uri) {
                    document_symbols::document_symbols(source)
                } else {
                    serde_json::json!([])
                };
                Some(protocol::make_response(id.clone(), result))
            }
            "workspace/symbol" => {
                let query = params.pointer("/query").and_then(|v| v.as_str()).unwrap_or("");
                let result = workspace_symbols::workspace_symbols(
                    query,
                    self.workspace_root.as_deref(),
                    &self.documents,
                );
                Some(protocol::make_response(id.clone(), result))
            }
            "textDocument/references" => {
                let uri = params.pointer("/textDocument/uri")?.as_str()?;
                let line = params.pointer("/position/line")?.as_u64()? as u32;
                let character = params.pointer("/position/character")?.as_u64()? as u32;
                let include_decl = params
                    .pointer("/context/includeDeclaration")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let result = if let Some(source) = self.documents.get(uri) {
                    references::find_references(
                        source,
                        uri,
                        line,
                        character,
                        self.workspace_root.as_deref(),
                        &self.documents,
                        include_decl,
                    )
                } else {
                    serde_json::json!([])
                };
                Some(protocol::make_response(id.clone(), result))
            }
            "textDocument/rename" => {
                let uri = params.pointer("/textDocument/uri")?.as_str()?;
                let line = params.pointer("/position/line")?.as_u64()? as u32;
                let character = params.pointer("/position/character")?.as_u64()? as u32;
                let new_name = params.pointer("/newName")?.as_str()?;
                let result = if let Some(source) = self.documents.get(uri) {
                    references::prepare_rename(
                        source,
                        uri,
                        line,
                        character,
                        new_name,
                        self.workspace_root.as_deref(),
                        &self.documents,
                    )
                } else {
                    Value::Null
                };
                Some(protocol::make_response(id.clone(), result))
            }
            "textDocument/documentHighlight" => {
                let uri = params.pointer("/textDocument/uri")?.as_str()?;
                let line = params.pointer("/position/line")?.as_u64()? as u32;
                let character = params.pointer("/position/character")?.as_u64()? as u32;
                let result = if let Some(source) = self.documents.get(uri) {
                    highlight::document_highlight(source, line, character)
                } else {
                    serde_json::json!([])
                };
                Some(protocol::make_response(id.clone(), result))
            }
            "textDocument/foldingRange" => {
                let uri = params.pointer("/textDocument/uri")?.as_str()?;
                let result = if let Some(source) = self.documents.get(uri) {
                    folding::folding_ranges(source)
                } else {
                    serde_json::json!([])
                };
                Some(protocol::make_response(id.clone(), result))
            }
            "textDocument/formatting" => {
                let uri = params.pointer("/textDocument/uri")?.as_str()?;
                let result = if let Some(source) = self.documents.get(uri) {
                    formatting::format_document(source)
                } else {
                    serde_json::json!([])
                };
                Some(protocol::make_response(id.clone(), result))
            }
            "textDocument/signatureHelp" => {
                let uri = params.pointer("/textDocument/uri")?.as_str()?;
                let line = params.pointer("/position/line")?.as_u64()? as u32;
                let character = params.pointer("/position/character")?.as_u64()? as u32;
                let result = if let Some(source) = self.documents.get(uri) {
                    signature_help::signature_help(source, line, character)
                } else {
                    Value::Null
                };
                Some(protocol::make_response(id.clone(), result))
            }
            "textDocument/inlayHint" => {
                let uri = params.pointer("/textDocument/uri")?.as_str()?;
                let start = params.pointer("/range/start/line")?.as_u64()? as u32;
                let end = params.pointer("/range/end/line")?.as_u64()? as u32;
                let result = if let Some(source) = self.documents.get(uri) {
                    inlay_hints::inlay_hints(source, start, end)
                } else {
                    serde_json::json!([])
                };
                Some(protocol::make_response(id.clone(), result))
            }
            _ => Some(protocol::make_error_response(
                id.clone(),
                -32601,
                &format!("method not found: {method}"),
            )),
        }
    }

    pub fn handle_notification(&mut self, method: &str, params: &Value) -> Vec<Value> {
        match method {
            "initialized" => Vec::new(),
            "textDocument/didOpen" => {
                if let (Some(uri), Some(text)) = (
                    params.pointer("/textDocument/uri").and_then(|v| v.as_str()),
                    params
                        .pointer("/textDocument/text")
                        .and_then(|v| v.as_str()),
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
