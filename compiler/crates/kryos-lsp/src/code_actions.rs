//! LSP textDocument/codeAction — quick-fix suggestions.
//!
//! For each diagnostic in the range that ends with `did you mean \`X\`?`,
//! produce a CodeAction that replaces the misspelled token with `X`.
//! This is the single most common quick-fix path and is mechanically
//! derivable from diagnostics the type checker already emits.

use serde_json::{json, Value};

use crate::diagnostics;
use crate::goto_def;

pub fn code_actions(source: &str, uri: &str, range_start_line: u32, range_end_line: u32) -> Value {
    let (diags, _) = diagnostics::check_source(source, uri);
    let mut actions: Vec<Value> = Vec::new();

    for diag in &diags {
        let dl = diag.get("range").and_then(|r| r.get("start"))
            .and_then(|s| s.get("line")).and_then(|n| n.as_u64()).unwrap_or(0) as u32;
        if dl < range_start_line || dl > range_end_line {
            continue;
        }

        // Try to extract "did you mean `X`?" from the diagnostic message or
        // related notes. The diagnostics module already wires note content
        // into the message field when emitting via render_diagnostic.
        let msg = diag.get("message").and_then(|m| m.as_str()).unwrap_or("");
        let Some(suggestion) = extract_did_you_mean(msg) else {
            continue;
        };

        // Locate the misspelled word in the source at the diagnostic's start.
        let dline = dl;
        let dchar = diag
            .get("range")
            .and_then(|r| r.get("start"))
            .and_then(|s| s.get("character"))
            .and_then(|n| n.as_u64())
            .unwrap_or(0) as u32;

        let Some(word) = goto_def::word_at_position(source, dline, dchar) else {
            continue;
        };
        if word == suggestion {
            continue;
        }

        // Compute the precise range of the word so the rename edit overlays
        // exactly the misspelled token.
        let Some(range) = word_range(source, dline, dchar) else {
            continue;
        };

        actions.push(json!({
            "title": format!("Replace `{word}` with `{suggestion}`"),
            "kind": "quickfix",
            "isPreferred": true,
            "edit": {
                "changes": {
                    uri: [{
                        "range": range,
                        "newText": suggestion,
                    }]
                }
            }
        }));
    }

    Value::Array(actions)
}

fn extract_did_you_mean(msg: &str) -> Option<String> {
    let needle = "did you mean `";
    let pos = msg.find(needle)?;
    let after = &msg[pos + needle.len()..];
    let end = after.find('`')?;
    Some(after[..end].to_string())
}

fn word_range(source: &str, line: u32, character: u32) -> Option<Value> {
    let offset = line_col_to_offset(source, line, character)?;
    let bytes = source.as_bytes();
    if offset >= bytes.len() {
        return None;
    }
    let mut start = offset;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    let mut end = offset;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    if start == end {
        return None;
    }
    let (sl, sc) = offset_to_line_col(source, start);
    let (el, ec) = offset_to_line_col(source, end);
    Some(json!({
        "start": { "line": sl, "character": sc },
        "end":   { "line": el, "character": ec },
    }))
}

fn line_col_to_offset(source: &str, line: u32, character: u32) -> Option<usize> {
    let mut current_line = 0u32;
    let mut line_start = 0usize;
    for (i, ch) in source.char_indices() {
        if current_line == line {
            let col = (i - line_start) as u32;
            if col >= character {
                return Some(i);
            }
        }
        if ch == '\n' {
            current_line += 1;
            line_start = i + 1;
        }
    }
    Some(source.len())
}

fn offset_to_line_col(source: &str, offset: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in source.char_indices() {
        if i == offset {
            return (line, col);
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}
