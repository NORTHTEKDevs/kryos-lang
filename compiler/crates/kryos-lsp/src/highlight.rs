//! LSP textDocument/documentHighlight — highlight every occurrence of the
//! identifier under the cursor inside the current file.

use serde_json::{json, Value};

use crate::goto_def;

const KIND_TEXT: u8 = 1;
const KIND_WRITE: u8 = 3;

pub fn document_highlight(source: &str, line: u32, character: u32) -> Value {
    let Some(word) = goto_def::word_at_position(source, line, character) else {
        return json!([]);
    };

    // Match-only-identifiers via the lexer (skip strings + comments).
    let tokens = kryos_lexer::Lexer::new(source, 0).tokenize();
    let mut out: Vec<Value> = Vec::new();
    for (idx, tok) in tokens.iter().enumerate() {
        if !matches!(
            tok.kind,
            kryos_lexer::TokenKind::Ident | kryos_lexer::TokenKind::TypeIdent
        ) {
            continue;
        }
        let start = tok.span.start as usize;
        let end = tok.span.end as usize;
        if end > source.len() || start >= end {
            continue;
        }
        if &source[start..end] != word {
            continue;
        }

        // Write highlight if followed by `=` (excluding `==`). Otherwise read.
        let kind = is_write_site(&tokens, idx).then_some(KIND_WRITE).unwrap_or(KIND_TEXT);

        let (sl, sc) = offset_to_line_col(source, start);
        let (el, ec) = offset_to_line_col(source, end);
        out.push(json!({
            "range": {
                "start": { "line": sl, "character": sc },
                "end":   { "line": el, "character": ec },
            },
            "kind": kind,
        }));
    }
    Value::Array(out)
}

fn is_write_site(tokens: &[kryos_lexer::Token], idx: usize) -> bool {
    let next = tokens.get(idx + 1);
    match next.map(|t| &t.kind) {
        Some(kryos_lexer::TokenKind::Eq) => true,
        Some(kryos_lexer::TokenKind::PlusEq)
        | Some(kryos_lexer::TokenKind::MinusEq)
        | Some(kryos_lexer::TokenKind::StarEq)
        | Some(kryos_lexer::TokenKind::SlashEq) => true,
        _ => false,
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
