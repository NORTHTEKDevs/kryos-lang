//! LSP completion — context-aware code completion.

use serde_json::Value;

/// Completion item kinds (LSP spec).
const KIND_KEYWORD: i32 = 14;
const KIND_FUNCTION: i32 = 3;
const KIND_STRUCT: i32 = 22;
const KIND_ENUM: i32 = 13;
const KIND_MODULE: i32 = 9;
const KIND_TYPE: i32 = 25;

/// Get completions for a given position in source text.
pub fn get_completions(source: &str, line: u32, character: u32) -> Vec<Value> {
    let mut items = Vec::new();

    // Determine context from cursor position
    let offset = line_col_to_offset(source, line, character);
    let prefix = get_word_prefix(source, offset);

    // Always offer keywords
    let keywords = [
        "let", "mut", "fn", "return", "if", "else", "elif", "for", "while", "in",
        "break", "continue", "struct", "enum", "impl", "trait", "pub", "use",
        "extern", "as", "mod", "type", "actor", "spawn", "select", "match",
        "try", "catch", "throw", "shared", "weak", "move", "true", "false", "none",
    ];

    for kw in &keywords {
        if prefix.is_empty() || kw.starts_with(&prefix) {
            items.push(serde_json::json!({
                "label": kw,
                "kind": KIND_KEYWORD,
                "detail": "keyword",
            }));
        }
    }

    // Offer built-in types
    let types = [
        "i8", "i16", "i32", "i64", "i128",
        "u8", "u16", "u32", "u64", "u128",
        "f32", "f64", "bool", "str", "char", "usize", "isize",
        "Vec", "Map", "Set", "Option", "Result",
    ];

    for ty in &types {
        if prefix.is_empty() || ty.starts_with(&prefix) {
            items.push(serde_json::json!({
                "label": ty,
                "kind": KIND_TYPE,
                "detail": "type",
            }));
        }
    }

    // Offer standard library modules after "use std::"
    let line_text = get_line_text(source, line);
    if line_text.contains("use std::") || line_text.contains("std::") {
        let std_modules = [
            "io", "net", "math", "json", "collections", "string", "regex",
            "datetime", "crypto", "process", "term", "fs", "sync", "chan",
            "iter", "fmt", "map", "set", "config", "test", "server", "db",
        ];
        for m in &std_modules {
            if prefix.is_empty() || m.starts_with(&prefix) {
                items.push(serde_json::json!({
                    "label": m,
                    "kind": KIND_MODULE,
                    "detail": "std module",
                }));
            }
        }
    }

    // Parse the current file to offer local declarations
    let tokens = kryos_lexer::Lexer::new(source, 0).tokenize();
    if let Ok(module) = kryos_parser::parse(tokens) {
        for decl in &module.declarations {
            match decl {
                kryos_ast::Decl::Function { name, .. } => {
                    if prefix.is_empty() || name.starts_with(&prefix) {
                        items.push(serde_json::json!({
                            "label": name,
                            "kind": KIND_FUNCTION,
                            "detail": "function",
                        }));
                    }
                }
                kryos_ast::Decl::Struct { name, .. } => {
                    if prefix.is_empty() || name.starts_with(&prefix) {
                        items.push(serde_json::json!({
                            "label": name,
                            "kind": KIND_STRUCT,
                            "detail": "struct",
                        }));
                    }
                }
                kryos_ast::Decl::Enum { name, .. } => {
                    if prefix.is_empty() || name.starts_with(&prefix) {
                        items.push(serde_json::json!({
                            "label": name,
                            "kind": KIND_ENUM,
                            "detail": "enum",
                        }));
                    }
                }
                _ => {}
            }
        }
    }

    items
}

fn line_col_to_offset(source: &str, line: u32, character: u32) -> usize {
    let mut current_line = 0u32;
    let mut offset = 0usize;
    for (i, ch) in source.char_indices() {
        if current_line == line {
            let col = i - offset;
            if col as u32 >= character {
                return i;
            }
        }
        if ch == '\n' {
            current_line += 1;
            offset = i + 1;
        }
    }
    source.len()
}

fn get_word_prefix(source: &str, offset: usize) -> String {
    let bytes = source.as_bytes();
    let mut start = offset;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    source[start..offset].to_string()
}

fn get_line_text(source: &str, line: u32) -> &str {
    source.lines().nth(line as usize).unwrap_or("")
}
