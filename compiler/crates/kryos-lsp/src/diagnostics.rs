//! LSP diagnostics — converts Kryos compiler diagnostics to LSP format.

use kryos_errors::{Diagnostic, Level, SourceMap};
use serde_json::Value;

/// Convert a Kryos Diagnostic to an LSP diagnostic JSON object.
pub fn to_lsp_diagnostic(diag: &Diagnostic, source_map: &SourceMap) -> Option<Value> {
    let label = diag.labels.first()?;
    let _file = source_map.get_file(label.span.file_id)?;
    let (start_line, start_col) =
        source_map.offset_to_line_col(label.span.file_id, label.span.start);
    let (end_line, end_col) = source_map.offset_to_line_col(label.span.file_id, label.span.end);

    let severity = match diag.level {
        Level::Error => 1,
        Level::Warning => 2,
        Level::Info => 3,
        Level::Help => 4,
    };

    let mut result = serde_json::json!({
        "range": {
            "start": { "line": start_line - 1, "character": start_col - 1 },
            "end": { "line": end_line - 1, "character": end_col - 1 },
        },
        "severity": severity,
        "source": "kryos",
        "message": diag.message,
    });

    if let Some(ref code) = diag.code {
        result["code"] = Value::String(code.clone());
    }

    // Add related information from additional labels
    if diag.labels.len() > 1 {
        let related: Vec<Value> = diag.labels[1..]
            .iter()
            .filter_map(|label| {
                let f = source_map.get_file(label.span.file_id)?;
                let (sl, sc) = source_map.offset_to_line_col(label.span.file_id, label.span.start);
                let (el, ec) = source_map.offset_to_line_col(label.span.file_id, label.span.end);
                Some(serde_json::json!({
                    "location": {
                        "uri": format!("file:///{}", f.name.replace('\\', "/")),
                        "range": {
                            "start": { "line": sl - 1, "character": sc - 1 },
                            "end": { "line": el - 1, "character": ec - 1 },
                        }
                    },
                    "message": label.message,
                }))
            })
            .collect();

        if !related.is_empty() {
            result["relatedInformation"] = Value::Array(related);
        }
    }

    Some(result)
}

/// Build a publishDiagnostics notification for a file.
pub fn publish_diagnostics(uri: &str, diagnostics: Vec<Value>) -> Value {
    crate::protocol::make_notification(
        "textDocument/publishDiagnostics",
        serde_json::json!({
            "uri": uri,
            "diagnostics": diagnostics,
        }),
    )
}

/// Run the compiler pipeline on source text and return LSP diagnostics.
pub fn check_source(source: &str, file_uri: &str) -> (Vec<Value>, kryos_errors::SourceMap) {
    let mut source_map = kryos_errors::SourceMap::default();
    let file_id = source_map.add_file(file_uri.to_string(), source.to_string());

    let tokens = kryos_lexer::Lexer::new(source, file_id).tokenize();

    let module = match kryos_parser::parse(tokens) {
        Ok(module) => module,
        Err(errors) => {
            let lsp_diags: Vec<Value> = errors
                .iter()
                .filter_map(|d| to_lsp_diagnostic(d, &source_map))
                .collect();
            return (lsp_diags, source_map);
        }
    };

    // Run type checker
    let type_diags = kryos_types::type_check(&module);

    let lsp_diags: Vec<Value> = type_diags
        .iter()
        .filter_map(|d| to_lsp_diagnostic(d, &source_map))
        .collect();

    (lsp_diags, source_map)
}
