//! LSP hover — show type information on hover.

use serde_json::Value;

/// Get hover information for a position in source text.
pub fn get_hover(source: &str, line: u32, character: u32) -> Option<Value> {
    let offset = line_col_to_offset(source, line, character);
    let word = get_word_at(source, offset)?;

    // Check if it's a keyword
    if let Some(info) = keyword_info(&word) {
        return Some(serde_json::json!({
            "contents": {
                "kind": "markdown",
                "value": info,
            }
        }));
    }

    // Check if it's a builtin type
    if let Some(info) = type_info(&word) {
        return Some(serde_json::json!({
            "contents": {
                "kind": "markdown",
                "value": info,
            }
        }));
    }

    // Check if it's a builtin function
    if let Some(info) = builtin_fn_info(&word) {
        return Some(serde_json::json!({
            "contents": {
                "kind": "markdown",
                "value": info,
            }
        }));
    }

    // Try to find the symbol in the parsed AST
    let tokens = kryos_lexer::Lexer::new(source, 0).tokenize();
    if let Ok(module) = kryos_parser::parse(tokens) {
        for decl in &module.declarations {
            match decl {
                kryos_ast::Decl::Function { name, params, ret_ty, .. } if *name == word => {
                    let params_str: Vec<String> = params.iter().map(|p| {
                        if let Some(ref ty) = p.ty {
                            format!("{}: {}", p.name, format_type(ty))
                        } else {
                            p.name.clone()
                        }
                    }).collect();
                    let ret = ret_ty.as_ref().map(|t| format!(" -> {}", format_type(t))).unwrap_or_default();
                    let sig = format!("```kryos\nfn {}({}){}\n```", name, params_str.join(", "), ret);
                    return Some(serde_json::json!({
                        "contents": { "kind": "markdown", "value": sig }
                    }));
                }
                kryos_ast::Decl::Struct { name: sname, fields, .. } if *sname == word => {
                    let fields_str: Vec<String> = fields.iter().map(|f| {
                        format!("    {}: {}", f.name, format_type(&f.ty))
                    }).collect();
                    let info = format!("```kryos\nstruct {} {{\n{}\n}}\n```", sname, fields_str.join(",\n"));
                    return Some(serde_json::json!({
                        "contents": { "kind": "markdown", "value": info }
                    }));
                }
                _ => {}
            }
        }
    }

    None
}

fn format_type(ty: &kryos_ast::TypeExpr) -> String {
    match ty {
        kryos_ast::TypeExpr::Simple { name, .. } => name.clone(),
        kryos_ast::TypeExpr::Generic { name, args, .. } => {
            let args_str: Vec<String> = args.iter().map(format_type).collect();
            format!("{}<{}>", name, args_str.join(", "))
        }
        kryos_ast::TypeExpr::Array { element, size, .. } => {
            if let Some(s) = size {
                format!("[{}; {}]", format_type(element), s)
            } else {
                format!("[{}]", format_type(element))
            }
        }
        kryos_ast::TypeExpr::Optional { inner, .. } => format!("?{}", format_type(inner)),
        kryos_ast::TypeExpr::Reference { inner, mutable, .. } => {
            if *mutable { format!("&mut {}", format_type(inner)) } else { format!("&{}", format_type(inner)) }
        }
        kryos_ast::TypeExpr::Shared { inner, .. } => format!("shared {}", format_type(inner)),
        _ => "?".to_string(),
    }
}

fn keyword_info(word: &str) -> Option<String> {
    Some(match word {
        "let" => "```kryos\nlet [mut] name [: Type] = value\n```\nDeclare a variable binding.".to_string(),
        "fn" => "```kryos\nfn name(params) -> ReturnType { body }\n```\nDeclare a function.".to_string(),
        "struct" => "```kryos\nstruct Name { field: Type }\n```\nDeclare a struct type.".to_string(),
        "enum" => "```kryos\nenum Name { Variant(Type) }\n```\nDeclare an enum type with variants.".to_string(),
        "trait" => "```kryos\ntrait Name { fn method(self) -> Type }\n```\nDeclare a trait (interface).".to_string(),
        "impl" => "```kryos\nimpl Name { fn method() {} }\n```\nImplement methods for a type.".to_string(),
        "shared" => "`shared value` — wrap a value in ARC (atomic reference counting) for safe sharing.".to_string(),
        "match" => "```kryos\nmatch value { pattern => expr }\n```\nPattern matching expression.".to_string(),
        "actor" => "```kryos\nactor Name { state_field: Type }\n```\nDeclare an actor with message handlers.".to_string(),
        _ => return None,
    })
}

fn type_info(word: &str) -> Option<String> {
    Some(match word {
        "i8" => "`i8` — 8-bit signed integer (-128 to 127)".to_string(),
        "i16" => "`i16` — 16-bit signed integer".to_string(),
        "i32" => "`i32` — 32-bit signed integer (default integer type)".to_string(),
        "i64" => "`i64` — 64-bit signed integer".to_string(),
        "u8" => "`u8` — 8-bit unsigned integer (0 to 255)".to_string(),
        "u16" => "`u16` — 16-bit unsigned integer".to_string(),
        "u32" => "`u32` — 32-bit unsigned integer".to_string(),
        "u64" => "`u64` — 64-bit unsigned integer".to_string(),
        "f32" => "`f32` — 32-bit floating point".to_string(),
        "f64" => "`f64` — 64-bit floating point (default float type)".to_string(),
        "bool" => "`bool` — boolean (`true` or `false`)".to_string(),
        "str" => "`str` — UTF-8 string".to_string(),
        "char" => "`char` — Unicode scalar value".to_string(),
        "Vec" => "`Vec<T>` — growable array".to_string(),
        "Map" => "`Map<K, V>` — hash map".to_string(),
        "Set" => "`Set<T>` — hash set".to_string(),
        "Option" => "`Option<T>` — optional value (`Some(T)` or `None`)".to_string(),
        "Result" => "`Result<T, E>` — success or error".to_string(),
        _ => return None,
    })
}

fn builtin_fn_info(word: &str) -> Option<String> {
    Some(match word {
        // I/O
        "println" => "```kryos\nfn println(s: str)\n```\nPrint string to stdout with a trailing newline.".to_string(),
        "print" => "```kryos\nfn print(s: str)\n```\nPrint string to stdout without a trailing newline.".to_string(),
        "eprintln" => "```kryos\nfn eprintln(s: str)\n```\nPrint string to stderr with a trailing newline.".to_string(),
        // Conversion
        "to_string" => "```kryos\nfn to_string(x) -> str\n```\nConvert any value to its string representation.".to_string(),
        "parse_int" => "```kryos\nfn parse_int(s: str) -> i64\n```\nParse a string as a 64-bit integer.".to_string(),
        "parse_float" => "```kryos\nfn parse_float(s: str) -> f64\n```\nParse a string as a 64-bit float.".to_string(),
        // Collections
        "len" => "```kryos\nfn len(x) -> i64\n```\nReturn the length of a string, array, or map.".to_string(),
        "push" => "```kryos\nfn push(arr, val)\n```\nAppend a value to the end of an array.".to_string(),
        "pop" => "```kryos\nfn pop(arr) -> val\n```\nRemove and return the last element of an array.".to_string(),
        // String
        "substr" => "```kryos\nfn substr(s: str, start: i64, end: i64) -> str\n```\nExtract a substring from start (inclusive) to end (exclusive).".to_string(),
        "contains" => "```kryos\nfn contains(haystack: str, needle: str) -> bool\n```\nReturn true if haystack contains the needle substring.".to_string(),
        "starts_with" => "```kryos\nfn starts_with(s: str, prefix: str) -> bool\n```\nReturn true if the string starts with the given prefix.".to_string(),
        "ends_with" => "```kryos\nfn ends_with(s: str, suffix: str) -> bool\n```\nReturn true if the string ends with the given suffix.".to_string(),
        "trim" => "```kryos\nfn trim(s: str) -> str\n```\nReturn the string with leading and trailing whitespace removed.".to_string(),
        "to_upper" => "```kryos\nfn to_upper(s: str) -> str\n```\nConvert all characters in the string to uppercase.".to_string(),
        "to_lower" => "```kryos\nfn to_lower(s: str) -> str\n```\nConvert all characters in the string to lowercase.".to_string(),
        "replace" => "```kryos\nfn replace(s: str, from: str, to: str) -> str\n```\nReplace all occurrences of `from` with `to` in the string.".to_string(),
        "split" => "```kryos\nfn split(s: str, delimiter: str) -> [str]\n```\nSplit a string by delimiter, returning an array of strings.".to_string(),
        "join" => "```kryos\nfn join(arr: [str], separator: str) -> str\n```\nJoin an array of strings with a separator.".to_string(),
        // Math
        "abs" => "```kryos\nfn abs(x) -> i64/f64\n```\nReturn the absolute value of a number.".to_string(),
        "min" => "```kryos\nfn min(a: i64, b: i64) -> i64\n```\nReturn the smaller of two values.".to_string(),
        "max" => "```kryos\nfn max(a: i64, b: i64) -> i64\n```\nReturn the larger of two values.".to_string(),
        "sqrt" => "```kryos\nfn sqrt(x: f64) -> f64\n```\nReturn the square root of a number.".to_string(),
        "floor" => "```kryos\nfn floor(x: f64) -> f64\n```\nRound a float down to the nearest integer value.".to_string(),
        "ceil" => "```kryos\nfn ceil(x: f64) -> f64\n```\nRound a float up to the nearest integer value.".to_string(),
        // Other
        "assert" => "```kryos\nfn assert(cond: bool, msg: str)\n```\nAssert that a condition is true, or panic with the given message.".to_string(),
        "time_now" => "```kryos\nfn time_now() -> i64\n```\nReturn the current unix timestamp in seconds.".to_string(),
        "file_read" => "```kryos\nfn file_read(path: str) -> str\n```\nRead the entire contents of a file as a string.".to_string(),
        "file_write" => "```kryos\nfn file_write(path: str, content: str)\n```\nWrite a string to a file, creating or overwriting it.".to_string(),
        "env_get" => "```kryos\nfn env_get(key: str) -> str\n```\nGet the value of an environment variable.".to_string(),
        _ => return None,
    })
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
        // Try one position back
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
