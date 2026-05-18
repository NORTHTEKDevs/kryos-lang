//! LSP textDocument/foldingRange — code folding for declarations and blocks.

use serde_json::{json, Value};

pub fn folding_ranges(source: &str) -> Value {
    let mut ranges: Vec<Value> = Vec::new();

    // Top-level decls: fold every `{ ... }` block at this point. Walk the
    // source for matching braces — cheap and robust regardless of parser
    // recovery state.
    push_brace_folds(source, &mut ranges);

    // Comment block folds: consecutive `//` lines collapse.
    push_comment_folds(source, &mut ranges);

    Value::Array(ranges)
}

fn push_brace_folds(source: &str, out: &mut Vec<Value>) {
    let bytes = source.as_bytes();
    let mut stack: Vec<usize> = Vec::new();
    let mut in_string = false;
    let mut in_line_comment = false;
    let mut escape = false;

    for (i, &b) in bytes.iter().enumerate() {
        if in_line_comment {
            if b == b'\n' {
                in_line_comment = false;
            }
            continue;
        }
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if b == b'\\' {
                escape = true;
                continue;
            }
            if b == b'"' {
                in_string = false;
            }
            continue;
        }
        if b == b'"' {
            in_string = true;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            in_line_comment = true;
            continue;
        }
        if b == b'{' {
            stack.push(i);
        } else if b == b'}' {
            if let Some(open) = stack.pop() {
                let (sl, _) = offset_to_line_col(source, open);
                let (el, _) = offset_to_line_col(source, i);
                if el > sl {
                    out.push(json!({
                        "startLine": sl,
                        "endLine": el.saturating_sub(1),
                        "kind": "region",
                    }));
                }
            }
        }
    }
}

fn push_comment_folds(source: &str, out: &mut Vec<Value>) {
    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_start().starts_with("//") {
            let start = i;
            while i + 1 < lines.len() && lines[i + 1].trim_start().starts_with("//") {
                i += 1;
            }
            if i > start {
                out.push(json!({
                    "startLine": start as u32,
                    "endLine": i as u32,
                    "kind": "comment",
                }));
            }
        }
        i += 1;
    }
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
