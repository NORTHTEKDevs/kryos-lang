//! LSP go-to-definition — find the definition of a symbol.

use serde_json::Value;

/// Find the definition location of a symbol at the given position.
pub fn goto_definition(source: &str, line: u32, character: u32) -> Option<Value> {
    let offset = line_col_to_offset(source, line, character);
    let word = get_word_at(source, offset)?;

    let tokens = kryos_lexer::Lexer::new(source, 0).tokenize();
    let module = kryos_parser::parse(tokens).ok()?;

    // Search declarations for matching name
    for decl in &module.declarations {
        let (name, span) = match decl {
            kryos_ast::Decl::Function { name, span, .. } => (name, span),
            kryos_ast::Decl::Struct { name, span, .. } => (name, span),
            kryos_ast::Decl::Enum { name, span, .. } => (name, span),
            kryos_ast::Decl::Trait { name, span, .. } => (name, span),
            kryos_ast::Decl::Actor { name, span, .. } => (name, span),
            kryos_ast::Decl::TypeAlias { name, span, .. } => (name, span),
            _ => continue,
        };

        if *name == word {
            // Convert span to LSP location
            let (start_line, start_col) = offset_to_line_col(source, span.start as usize);
            let (end_line, end_col) = offset_to_line_col(source, span.end as usize);

            return Some(serde_json::json!({
                "range": {
                    "start": { "line": start_line, "character": start_col },
                    "end": { "line": end_line, "character": end_col },
                }
            }));
        }
    }

    None
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

fn line_col_to_offset(source: &str, line: u32, character: u32) -> usize {
    let mut current_line = 0u32;
    let mut line_start = 0usize;
    for (i, ch) in source.char_indices() {
        if current_line == line {
            let col = i - line_start;
            if col as u32 >= character {
                return i;
            }
        }
        if ch == '\n' {
            current_line += 1;
            line_start = i + 1;
        }
    }
    source.len()
}

fn get_word_at(source: &str, offset: usize) -> Option<String> {
    let bytes = source.as_bytes();
    if offset >= bytes.len() || !(bytes[offset].is_ascii_alphanumeric() || bytes[offset] == b'_') {
        if offset > 0 && (bytes[offset - 1].is_ascii_alphanumeric() || bytes[offset - 1] == b'_') {
            return get_word_at(source, offset - 1);
        }
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
    Some(source[start..end].to_string())
}
