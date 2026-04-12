//! LSP go-to-definition — find the definition of a symbol.

use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Extract the word under the cursor at the given position.
pub fn word_at_position(source: &str, line: u32, character: u32) -> Option<String> {
    let offset = line_col_to_offset(source, line, character);
    get_word_at(source, offset)
}

/// Find the definition location of a symbol within a single source string.
/// Returns a JSON object with just the `range` (no URI).
pub fn goto_definition(source: &str, line: u32, character: u32) -> Option<Value> {
    let offset = line_col_to_offset(source, line, character);
    let word = get_word_at(source, offset)?;

    find_declaration_in_source(source, &word)
}

/// Search a source string for a declaration with the given name.
/// Returns a JSON object with the `range` if found.
fn find_declaration_in_source(source: &str, name: &str) -> Option<Value> {
    let tokens = kryos_lexer::Lexer::new(source, 0).tokenize();
    let module = kryos_parser::parse(tokens).ok()?;

    for decl in &module.declarations {
        let (decl_name, span) = match decl {
            kryos_ast::Decl::Function { name, span, .. } => (name, span),
            kryos_ast::Decl::Struct { name, span, .. } => (name, span),
            kryos_ast::Decl::Enum { name, span, .. } => (name, span),
            kryos_ast::Decl::Trait { name, span, .. } => (name, span),
            kryos_ast::Decl::Actor { name, span, .. } => (name, span),
            kryos_ast::Decl::TypeAlias { name, span, .. } => (name, span),
            kryos_ast::Decl::Const { name, span, .. } => (name, span),
            _ => continue,
        };

        if *decl_name == name {
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

/// Search across all .kry files in the workspace for a declaration matching `name`.
///
/// `documents` contains already-open file contents (keyed by URI).
/// `workspace_root` is the file-system path of the workspace folder.
/// `current_uri` is the URI of the file the request originated from (skipped to avoid
/// re-searching the file that was already checked).
///
/// Returns a full LSP `Location` (uri + range) if found.
pub fn goto_definition_cross_file(
    name: &str,
    current_uri: &str,
    workspace_root: &str,
    documents: &HashMap<String, String>,
) -> Option<Value> {
    let root = parse_uri_to_path(workspace_root)?;

    // Collect .kry files from the workspace root (non-recursive first pass,
    // then one level of subdirectories — keeps it cheap).
    let kry_files = collect_kry_files(&root);

    for file_path in &kry_files {
        let file_uri = path_to_uri(file_path);

        // Skip the file we already searched in the caller.
        if file_uri == current_uri {
            continue;
        }

        // Prefer the in-memory content if the file is already open in the editor.
        let source = if let Some(content) = documents.get(&file_uri) {
            content.clone()
        } else {
            match std::fs::read_to_string(file_path) {
                Ok(s) => s,
                Err(_) => continue,
            }
        };

        if let Some(mut location) = find_declaration_in_source(&source, name) {
            location["uri"] = Value::String(file_uri);
            return Some(location);
        }
    }

    None
}

/// Collect all `.kry` files under `root`, searching up to 3 directory levels deep.
fn collect_kry_files(root: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    collect_kry_files_recursive(root, 0, 3, &mut results);
    results
}

fn collect_kry_files_recursive(dir: &Path, depth: u32, max_depth: u32, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "kry" {
                    out.push(path);
                }
            }
        } else if path.is_dir() && depth < max_depth {
            // Skip hidden directories and common non-source directories
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if !name.starts_with('.') && name != "target" && name != "node_modules" {
                    collect_kry_files_recursive(&path, depth + 1, max_depth, out);
                }
            }
        }
    }
}

/// Convert a `file://` URI to a filesystem `PathBuf`.
fn parse_uri_to_path(uri: &str) -> Option<PathBuf> {
    let path_str = uri.strip_prefix("file:///")?;
    // URI-decode percent-encoded characters (basic: spaces)
    let decoded = path_str.replace("%20", " ");
    // On Windows the URI looks like file:///C:/foo, giving us "C:/foo" after stripping.
    // On Unix it looks like file:///home/foo, giving us "home/foo" — prepend '/'.
    #[cfg(not(target_os = "windows"))]
    let decoded = format!("/{decoded}");
    Some(PathBuf::from(decoded))
}

/// Convert a filesystem path to a `file://` URI.
fn path_to_uri(path: &Path) -> String {
    let canonical = path.to_string_lossy().replace('\\', "/");
    // On Windows, paths start with e.g. "C:/", so we get file:///C:/...
    // On Unix, paths start with "/", so we get file:///...
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
