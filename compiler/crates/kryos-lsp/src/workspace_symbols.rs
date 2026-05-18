//! LSP workspace/symbol — search every top-level declaration across the
//! workspace, fuzzy-matching the query string.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const SK_FUNCTION: u8 = 12;
const SK_CONSTANT: u8 = 14;
const SK_VARIABLE: u8 = 13;
const SK_STRUCT: u8 = 23;
const SK_ENUM: u8 = 10;
const SK_INTERFACE: u8 = 11;
const SK_CLASS: u8 = 5;
const SK_TYPE_PARAMETER: u8 = 26;

pub fn workspace_symbols(
    query: &str,
    workspace_root: Option<&str>,
    documents: &HashMap<String, String>,
) -> Value {
    let mut out: Vec<Value> = Vec::new();

    // Open documents first (always available even without an explicit root).
    for (uri, source) in documents {
        push_matches(source, uri, query, &mut out);
    }

    if let Some(root_uri) = workspace_root {
        if let Some(root_path) = parse_uri_to_path(root_uri) {
            for path in collect_kry_files(&root_path) {
                let file_uri = path_to_uri(&path);
                if documents.contains_key(&file_uri) {
                    continue;
                }
                let Ok(source) = std::fs::read_to_string(&path) else { continue };
                push_matches(&source, &file_uri, query, &mut out);
            }
        }
    }

    Value::Array(out)
}

fn push_matches(source: &str, uri: &str, query: &str, out: &mut Vec<Value>) {
    let tokens = kryos_lexer::Lexer::new(source, 0).tokenize();
    let Ok(module) = kryos_parser::parse(tokens) else { return };

    for decl in &module.declarations {
        let (name, kind, span) = match decl {
            kryos_ast::Decl::Function { name, span, .. } => (name.clone(), SK_FUNCTION, span),
            kryos_ast::Decl::Struct { name, span, .. } => (name.clone(), SK_STRUCT, span),
            kryos_ast::Decl::Enum { name, span, .. } => (name.clone(), SK_ENUM, span),
            kryos_ast::Decl::Trait { name, span, .. } => (name.clone(), SK_INTERFACE, span),
            kryos_ast::Decl::Actor { name, span, .. } => (name.clone(), SK_CLASS, span),
            kryos_ast::Decl::TypeAlias { name, span, .. } => (name.clone(), SK_TYPE_PARAMETER, span),
            kryos_ast::Decl::Const { name, mutable, span, .. } => {
                let k = if *mutable { SK_VARIABLE } else { SK_CONSTANT };
                (name.clone(), k, span)
            }
            _ => continue,
        };

        if !fuzzy_match(&name, query) {
            continue;
        }

        let (sl, sc) = offset_to_line_col(source, span.start as usize);
        let (el, ec) = offset_to_line_col(source, span.end as usize);

        out.push(json!({
            "name": name,
            "kind": kind,
            "location": {
                "uri": uri,
                "range": {
                    "start": { "line": sl, "character": sc },
                    "end":   { "line": el, "character": ec },
                }
            }
        }));
    }
}

/// Subsequence fuzzy match: every char of `query` appears in `name` in order.
fn fuzzy_match(name: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let name_lower = name.to_lowercase();
    let mut it = name_lower.chars();
    for qc in query.to_lowercase().chars() {
        match it.find(|c| *c == qc) {
            Some(_) => {}
            None => return false,
        }
    }
    true
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
