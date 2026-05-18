//! LSP textDocument/references and textDocument/rename.
//!
//! Finds all occurrences of the identifier under the cursor across the
//! workspace, ignoring matches inside string and comment spans. This is a
//! lexer-driven scan rather than a full name-resolution analysis — sufficient
//! for the common rename/find-references case until full resolve lands.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::goto_def;

/// Return `Location[]` for every reference to the identifier under the cursor.
pub fn find_references(
    source: &str,
    uri: &str,
    line: u32,
    character: u32,
    workspace_root: Option<&str>,
    documents: &HashMap<String, String>,
    include_declaration: bool,
) -> Value {
    let Some(word) = goto_def::word_at_position(source, line, character) else {
        return json!([]);
    };
    if is_keyword_or_builtin(&word) {
        return json!([]);
    }

    let mut locations: Vec<Value> = Vec::new();
    let in_decl = if include_declaration { None } else { decl_offset(source, &word) };
    push_word_hits(source, uri, &word, in_decl, &mut locations);

    if let Some(root_uri) = workspace_root {
        if let Some(root_path) = parse_uri_to_path(root_uri) {
            for path in collect_kry_files(&root_path) {
                let file_uri = path_to_uri(&path);
                if file_uri == uri {
                    continue;
                }
                let src = if let Some(s) = documents.get(&file_uri) {
                    s.clone()
                } else {
                    match std::fs::read_to_string(&path) {
                        Ok(s) => s,
                        Err(_) => continue,
                    }
                };
                push_word_hits(&src, &file_uri, &word, None, &mut locations);
            }
        }
    }

    Value::Array(locations)
}

/// Compute a `WorkspaceEdit` mapping every reference site to `new_name`.
pub fn prepare_rename(
    source: &str,
    uri: &str,
    line: u32,
    character: u32,
    new_name: &str,
    workspace_root: Option<&str>,
    documents: &HashMap<String, String>,
) -> Value {
    if !is_valid_identifier(new_name) {
        return json!(null);
    }

    let refs = find_references(
        source,
        uri,
        line,
        character,
        workspace_root,
        documents,
        true, // rename must include the declaration
    );

    let mut changes: HashMap<String, Vec<Value>> = HashMap::new();
    if let Value::Array(arr) = refs {
        for loc in arr {
            let Some(u) = loc.get("uri").and_then(|v| v.as_str()) else { continue };
            let Some(range) = loc.get("range") else { continue };
            let edit = json!({
                "range": range,
                "newText": new_name,
            });
            changes.entry(u.to_string()).or_default().push(edit);
        }
    }

    json!({ "changes": changes })
}

fn push_word_hits(
    source: &str,
    uri: &str,
    word: &str,
    skip_offset: Option<usize>,
    out: &mut Vec<Value>,
) {
    let tokens = kryos_lexer::Lexer::new(source, 0).tokenize();
    for tok in tokens {
        if !matches!(
            tok.kind,
            kryos_lexer::TokenKind::Ident | kryos_lexer::TokenKind::TypeIdent
        ) {
            continue;
        }
        let span = tok.span;
        let start = span.start as usize;
        let end = span.end as usize;
        if end > source.len() || start >= end {
            continue;
        }
        let slice = &source[start..end];
        if slice != word {
            continue;
        }
        if Some(start) == skip_offset {
            continue;
        }
        let (sl, sc) = offset_to_line_col(source, start);
        let (el, ec) = offset_to_line_col(source, end);
        out.push(json!({
            "uri": uri,
            "range": {
                "start": { "line": sl, "character": sc },
                "end":   { "line": el, "character": ec },
            }
        }));
    }
}

fn decl_offset(source: &str, name: &str) -> Option<usize> {
    let tokens = kryos_lexer::Lexer::new(source, 0).tokenize();
    let module = kryos_parser::parse(tokens).ok()?;
    for decl in &module.declarations {
        let (n, span) = match decl {
            kryos_ast::Decl::Function { name, span, .. }
            | kryos_ast::Decl::Struct { name, span, .. }
            | kryos_ast::Decl::Enum { name, span, .. }
            | kryos_ast::Decl::Trait { name, span, .. }
            | kryos_ast::Decl::Actor { name, span, .. }
            | kryos_ast::Decl::TypeAlias { name, span, .. }
            | kryos_ast::Decl::Const { name, span, .. } => (name, span),
            _ => continue,
        };
        if n == name {
            // Span covers the whole declaration — locate the name inside it.
            let body = &source[span.start as usize..span.end as usize];
            if let Some(idx) = body.find(name) {
                return Some(span.start as usize + idx);
            }
        }
    }
    None
}

fn is_keyword_or_builtin(s: &str) -> bool {
    matches!(
        s,
        "fn" | "let" | "mut" | "if" | "elif" | "else" | "while" | "for" | "in" | "loop"
            | "break" | "continue" | "return" | "match" | "struct" | "enum" | "trait" | "impl"
            | "actor" | "type" | "use" | "pub" | "true" | "false" | "as" | "async" | "await"
            | "spawn" | "throw" | "extern" | "self" | "Self" | "i64" | "f64" | "bool" | "str"
            | "void"
    )
}

fn is_valid_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn collect_kry_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_recursive(root, 0, 4, &mut out);
    out
}

fn collect_recursive(dir: &Path, depth: u32, max_depth: u32, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if path.extension().is_some_and(|e| e == "kry") {
                out.push(path);
            }
        } else if path.is_dir() && depth < max_depth {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if !name.starts_with('.') && name != "target" && name != "node_modules" {
                    collect_recursive(&path, depth + 1, max_depth, out);
                }
            }
        }
    }
}

fn parse_uri_to_path(uri: &str) -> Option<PathBuf> {
    let path_str = uri.strip_prefix("file:///")?;
    let decoded = path_str.replace("%20", " ");
    #[cfg(not(target_os = "windows"))]
    let decoded = format!("/{decoded}");
    Some(PathBuf::from(decoded))
}

fn path_to_uri(path: &Path) -> String {
    let canonical = path.to_string_lossy().replace('\\', "/");
    if canonical.starts_with('/') {
        format!("file://{canonical}")
    } else {
        format!("file:///{canonical}")
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
