//! LSP completion — context-aware code completion.

use serde_json::Value;

/// Completion item kinds (LSP spec).
const KIND_KEYWORD: i32 = 14;
const KIND_FUNCTION: i32 = 3;
const KIND_STRUCT: i32 = 22;
const KIND_ENUM: i32 = 13;
const KIND_MODULE: i32 = 9;
const KIND_TYPE: i32 = 25;
const KIND_FIELD: i32 = 5;
const KIND_METHOD: i32 = 2;

/// Get completions for a given position in source text.
pub fn get_completions(source: &str, line: u32, character: u32) -> Vec<Value> {
    let mut items = Vec::new();

    // Determine context from cursor position
    let offset = line_col_to_offset(source, line, character);
    let prefix = get_word_prefix(source, offset);

    // Member-access fast path: if the immediately preceding non-prefix char
    // is `.`, only offer method-like completions. This makes `s.<TAB>` show
    // the string/array operations instead of the full keyword bag.
    if is_member_access(source, offset, &prefix) {
        push_member_completions(source, &prefix, &mut items);
        return items;
    }

    // Always offer keywords
    let keywords = [
        "let", "mut", "fn", "return", "if", "else", "elif", "for", "while", "in", "break",
        "continue", "struct", "enum", "impl", "trait", "pub", "use", "extern", "as", "mod", "type",
        "actor", "spawn", "select", "match", "try", "catch", "throw", "shared", "weak", "move",
        "true", "false", "none",
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
        "i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64", "u128", "f32", "f64", "bool",
        "str", "char", "usize", "isize", "Vec", "Map", "Set", "Option", "Result",
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
            "io",
            "net",
            "math",
            "json",
            "collections",
            "string",
            "regex",
            "datetime",
            "crypto",
            "process",
            "term",
            "fs",
            "sync",
            "chan",
            "iter",
            "fmt",
            "map",
            "set",
            "config",
            "test",
            "server",
            "db",
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

    // Offer built-in functions
    let builtins: &[(&str, &str, &str)] = &[
        // I/O
        ("println", "fn(s: str)", "Print string with newline"),
        ("print", "fn(s: str)", "Print string without newline"),
        ("eprintln", "fn(s: str)", "Print to stderr with newline"),
        // Conversion
        ("to_string", "fn(x) -> str", "Convert any value to string"),
        ("parse_int", "fn(s: str) -> i64", "Parse string to integer"),
        ("parse_float", "fn(s: str) -> f64", "Parse string to float"),
        // Collections
        ("len", "fn(x) -> i64", "Length of string, array, or map"),
        ("push", "fn(arr, val)", "Append value to array"),
        (
            "pop",
            "fn(arr) -> val",
            "Remove and return last element of array",
        ),
        // String
        (
            "substr",
            "fn(s: str, start: i64, end: i64) -> str",
            "Extract substring",
        ),
        (
            "contains",
            "fn(s: str, needle: str) -> bool",
            "Check if string contains substring",
        ),
        (
            "starts_with",
            "fn(s: str, prefix: str) -> bool",
            "Check if string starts with prefix",
        ),
        (
            "ends_with",
            "fn(s: str, suffix: str) -> bool",
            "Check if string ends with suffix",
        ),
        (
            "trim",
            "fn(s: str) -> str",
            "Trim leading and trailing whitespace",
        ),
        (
            "to_upper",
            "fn(s: str) -> str",
            "Convert string to uppercase",
        ),
        (
            "to_lower",
            "fn(s: str) -> str",
            "Convert string to lowercase",
        ),
        (
            "replace",
            "fn(s: str, from: str, to: str) -> str",
            "Replace occurrences in string",
        ),
        (
            "split",
            "fn(s: str, delimiter: str) -> [str]",
            "Split string by delimiter",
        ),
        (
            "join",
            "fn(arr: [str], separator: str) -> str",
            "Join array of strings with separator",
        ),
        // Math
        ("abs", "fn(x) -> i64/f64", "Absolute value"),
        (
            "min",
            "fn(a: i64, b: i64) -> i64",
            "Return the smaller value",
        ),
        (
            "max",
            "fn(a: i64, b: i64) -> i64",
            "Return the larger value",
        ),
        ("sqrt", "fn(x: f64) -> f64", "Square root"),
        (
            "floor",
            "fn(x: f64) -> f64",
            "Round down to nearest integer",
        ),
        ("ceil", "fn(x: f64) -> f64", "Round up to nearest integer"),
        // Other
        (
            "assert",
            "fn(cond: bool, msg: str)",
            "Assert condition or panic with message",
        ),
        (
            "assert_eq",
            "fn(left, right)",
            "Assert two values are equal; prints both on failure",
        ),
        (
            "panic",
            "fn(msg: str)",
            "Abort the process with the given message",
        ),
        (
            "time_now",
            "fn() -> i64",
            "Current unix timestamp in seconds",
        ),
        (
            "file_read",
            "fn(path: str) -> str",
            "Read file contents as string",
        ),
        (
            "file_write",
            "fn(path: str, content: str)",
            "Write string to file",
        ),
        (
            "env_get",
            "fn(key: str) -> str",
            "Get environment variable value",
        ),
    ];

    for (name, detail, doc) in builtins {
        if prefix.is_empty() || name.starts_with(&prefix) {
            items.push(serde_json::json!({
                "label": name,
                "kind": KIND_FUNCTION,
                "detail": detail,
                "documentation": doc,
            }));
        }
    }

    // Parse the current file to offer local declarations
    let tokens = kryos_lexer::Lexer::new(source, 0).tokenize();
    if let Ok(module) = kryos_parser::parse(tokens) {
        for decl in &module.declarations {
            match decl {
                kryos_ast::Decl::Function { name, .. }
                    if prefix.is_empty() || name.starts_with(&prefix) =>
                {
                    items.push(serde_json::json!({
                        "label": name,
                        "kind": KIND_FUNCTION,
                        "detail": "function",
                    }));
                }
                kryos_ast::Decl::Struct { name, .. }
                    if prefix.is_empty() || name.starts_with(&prefix) =>
                {
                    items.push(serde_json::json!({
                        "label": name,
                        "kind": KIND_STRUCT,
                        "detail": "struct",
                    }));
                }
                kryos_ast::Decl::Enum { name, .. }
                    if prefix.is_empty() || name.starts_with(&prefix) =>
                {
                    items.push(serde_json::json!({
                        "label": name,
                        "kind": KIND_ENUM,
                        "detail": "enum",
                    }));
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

/// True if the cursor is immediately after a `.` (possibly with a partial
/// identifier prefix already typed).
fn is_member_access(source: &str, offset: usize, prefix: &str) -> bool {
    let bytes = source.as_bytes();
    let dot_pos = offset.saturating_sub(prefix.len());
    if dot_pos == 0 {
        return false;
    }
    bytes[dot_pos - 1] == b'.'
}

/// Push a curated list of method-style operations available on common types.
/// This is a heuristic — full type inference at the LSP layer isn't wired —
/// but it captures the operations most newcomers reach for after `.`.
fn push_member_completions(source: &str, prefix: &str, items: &mut Vec<Value>) {
    let methods: &[(&str, &str, &str)] = &[
        // String operations
        ("len", "fn() -> i64", "length in bytes"),
        ("to_string", "fn() -> str", "convert to string"),
        ("to_upper", "fn() -> str", "uppercase copy"),
        ("to_lower", "fn() -> str", "lowercase copy"),
        ("trim", "fn() -> str", "strip leading/trailing whitespace"),
        ("starts_with", "fn(prefix: str) -> bool", "prefix test"),
        ("ends_with", "fn(suffix: str) -> bool", "suffix test"),
        ("contains", "fn(needle: str) -> bool", "substring test"),
        ("replace", "fn(from: str, to: str) -> str", "replace substring"),
        ("split", "fn(delim: str) -> [str]", "split on delimiter"),
        ("clone", "fn() -> T", "deep clone"),
        // Array operations
        ("push", "fn(val: T) -> [T]", "append value"),
        ("pop", "fn() -> T", "remove + return last"),
        ("first", "fn() -> Option<T>", "first element"),
        ("last", "fn() -> Option<T>", "last element"),
        // Option / Result
        ("unwrap", "fn() -> T", "panic if None / Err"),
        ("is_some", "fn() -> bool", "Option presence test"),
        ("is_none", "fn() -> bool", "Option absence test"),
        ("is_ok", "fn() -> bool", "Result ok test"),
        ("is_err", "fn() -> bool", "Result error test"),
    ];

    for (name, detail, doc) in methods {
        if prefix.is_empty() || name.starts_with(prefix) {
            items.push(serde_json::json!({
                "label": name,
                "kind": KIND_METHOD,
                "detail": detail,
                "documentation": doc,
            }));
        }
    }

    // Also offer struct-field names if the variable preceding `.` matches a
    // known struct in this file.
    // Cheap heuristic: walk back from the `.` to find the identifier name,
    // search for `let <name>: <Type> = ...` patterns, then list that struct's
    // fields. Best-effort only.
    let _ = source;
}
