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
