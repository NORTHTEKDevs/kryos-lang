//! LSP textDocument/formatting — delegate to kryos-fmt.
//!
//! Returns a single TextEdit replacing the entire document with the
//! formatted source. If formatting fails (e.g. parse error), returns
//! an empty edit array so the editor leaves the buffer untouched.

use serde_json::{json, Value};

pub fn format_document(source: &str) -> Value {
    let formatted = match kryos_fmt::format_source(source) {
        Ok(s) => s,
        Err(_) => return json!([]),
    };

    if formatted == source {
        return json!([]);
    }

    // Replace the entire document via one TextEdit spanning [0,0]..[end].
    let (end_line, end_char) = end_position(source);
    json!([
        {
            "range": {
                "start": { "line": 0, "character": 0 },
                "end":   { "line": end_line, "character": end_char },
            },
            "newText": formatted,
        }
    ])
}

fn end_position(source: &str) -> (u32, u32) {
    let mut line = 0u32;
    let mut col = 0u32;
    for ch in source.chars() {
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}
