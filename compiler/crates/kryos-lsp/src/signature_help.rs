//! LSP textDocument/signatureHelp — pop up the active function signature
//! and highlight the current parameter while the user is typing args.

use serde_json::{json, Value};

pub fn signature_help(source: &str, line: u32, character: u32) -> Value {
    let Some((fn_name, active_param)) = find_call_context(source, line, character) else {
        return json!(null);
    };

    let tokens = kryos_lexer::Lexer::new(source, 0).tokenize();
    let Ok(module) = kryos_parser::parse(tokens) else {
        return json!(null);
    };

    for decl in &module.declarations {
        if let kryos_ast::Decl::Function {
            name,
            params,
            ret_ty,
            is_async,
            doc_comments,
            ..
        } = decl
        {
            if name == &fn_name {
                let label = render_signature(name, params, ret_ty.as_ref(), *is_async);
                let mut param_info: Vec<Value> = Vec::new();
                for p in params {
                    let p_label = match &p.ty {
                        Some(t) => format!("{}: {}", p.name, type_to_string(t)),
                        None => p.name.clone(),
                    };
                    param_info.push(json!({ "label": p_label }));
                }
                let doc = doc_comments.join("\n");
                return json!({
                    "signatures": [{
                        "label": label,
                        "documentation": doc,
                        "parameters": param_info,
                    }],
                    "activeSignature": 0,
                    "activeParameter": active_param,
                });
            }
        }
    }

    json!(null)
}

/// Walk backward from the cursor through balanced brackets/braces. If we
/// land on `Ident '('` at depth 0, return `(name, current_arg_index)`.
fn find_call_context(source: &str, line: u32, character: u32) -> Option<(String, u32)> {
    let offset = line_col_to_offset(source, line, character);
    let bytes = source.as_bytes();

    let mut paren_depth: i32 = 0;
    let mut bracket_depth: i32 = 0;
    let mut brace_depth: i32 = 0;
    let mut comma_count: u32 = 0;
    let mut i = offset;
    while i > 0 {
        i -= 1;
        let b = bytes[i];
        match b {
            b')' => paren_depth += 1,
            b']' => bracket_depth += 1,
            b'}' => brace_depth += 1,
            b'[' => {
                if bracket_depth == 0 {
                    return None;
                }
                bracket_depth -= 1;
            }
            b'{' => {
                if brace_depth == 0 {
                    return None;
                }
                brace_depth -= 1;
            }
            b',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                comma_count += 1;
            }
            b'(' => {
                if paren_depth == 0 {
                    // Found the open paren of the active call. Grab the ident to its left.
                    let mut j = i;
                    while j > 0 && bytes[j - 1] == b' ' {
                        j -= 1;
                    }
                    let name_end = j;
                    let mut name_start = j;
                    while name_start > 0
                        && (bytes[name_start - 1].is_ascii_alphanumeric()
                            || bytes[name_start - 1] == b'_')
                    {
                        name_start -= 1;
                    }
                    if name_start == name_end {
                        return None;
                    }
                    let _ = name_end;
                    let name = std::str::from_utf8(&bytes[name_start..j]).ok()?.to_string();
                    return Some((name, comma_count));
                }
                paren_depth -= 1;
            }
            _ => {}
        }
    }
    None
}

fn render_signature(
    name: &str,
    params: &[kryos_ast::Param],
    ret_ty: Option<&kryos_ast::TypeExpr>,
    is_async: bool,
) -> String {
    let prefix = if is_async { "async fn " } else { "fn " };
    let ps = params
        .iter()
        .map(|p| match &p.ty {
            Some(t) => format!("{}: {}", p.name, type_to_string(t)),
            None => p.name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let ret = match ret_ty {
        Some(t) => format!(" -> {}", type_to_string(t)),
        None => String::new(),
    };
    format!("{prefix}{name}({ps}){ret}")
}

fn type_to_string(ty: &kryos_ast::TypeExpr) -> String {
    match ty {
        kryos_ast::TypeExpr::Simple { name, .. } => name.clone(),
        kryos_ast::TypeExpr::Generic { name, args, .. } => {
            let inner = args.iter().map(type_to_string).collect::<Vec<_>>().join(", ");
            format!("{name}<{inner}>")
        }
        kryos_ast::TypeExpr::Array { element, .. } => format!("[{}]", type_to_string(element)),
        kryos_ast::TypeExpr::Tuple { elements, .. } => {
            let inner = elements.iter().map(type_to_string).collect::<Vec<_>>().join(", ");
            format!("({inner})")
        }
        kryos_ast::TypeExpr::Optional { inner, .. } => format!("Option<{}>", type_to_string(inner)),
        kryos_ast::TypeExpr::Function { params, ret, .. } => {
            let p = params.iter().map(type_to_string).collect::<Vec<_>>().join(", ");
            format!("fn({p}) -> {}", type_to_string(ret))
        }
        kryos_ast::TypeExpr::DynTrait { trait_name, .. } => format!("dyn {trait_name}"),
        kryos_ast::TypeExpr::Reference { inner, mutable, .. } => {
            if *mutable {
                format!("&mut {}", type_to_string(inner))
            } else {
                format!("&{}", type_to_string(inner))
            }
        }
        kryos_ast::TypeExpr::Shared { inner, .. } => format!("Arc<{}>", type_to_string(inner)),
        kryos_ast::TypeExpr::Weak { inner, .. } => format!("Weak<{}>", type_to_string(inner)),
        kryos_ast::TypeExpr::Pointer { inner, mutable, .. } => {
            if *mutable {
                format!("*mut {}", type_to_string(inner))
            } else {
                format!("*const {}", type_to_string(inner))
            }
        }
        kryos_ast::TypeExpr::Inferred { .. } => "_".to_string(),
    }
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
