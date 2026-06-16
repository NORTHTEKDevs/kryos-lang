//! LSP textDocument/inlayHint — inline type hints for `let` bindings that
//! omit an explicit annotation, plus parameter-name hints at call sites.
//!
//! v3.1 ships a conservative version: only literal-typed let bindings get
//! hints (so we don't show wrong info while full inference is offline at the
//! LSP layer). Call-site parameter-name hints walk the parsed AST to find
//! known signatures.

use serde_json::{json, Value};
use std::collections::HashMap;

const HINT_KIND_TYPE: u8 = 1;
const HINT_KIND_PARAMETER: u8 = 2;

pub fn inlay_hints(source: &str, line_start: u32, line_end: u32) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let tokens = kryos_lexer::Lexer::new(source, 0).tokenize();
    let Ok(module) = kryos_parser::parse(tokens.clone()) else {
        return Value::Array(out);
    };

    // Build a signature table for parameter-name hints.
    let mut signatures: HashMap<String, Vec<String>> = HashMap::new();
    for decl in &module.declarations {
        if let kryos_ast::Decl::Function { name, params, .. } = decl {
            let names = params.iter().map(|p| p.name.clone()).collect();
            signatures.insert(name.clone(), names);
        }
    }

    push_let_type_hints(source, &tokens, line_start, line_end, &mut out);
    push_call_param_hints(source, &tokens, line_start, line_end, &signatures, &mut out);
    push_capability_hints(source, &module, line_start, line_end, &mut out);

    Value::Array(out)
}

/// Emit a capability-surface hint on each top-level `fn` declaration whose body
/// uses capability-gated builtins (e.g. `caps: io, net`). Reuses the compiler's
/// capability model via `crate::cap_surface`. Pure functions get no hint.
fn push_capability_hints(
    source: &str,
    module: &kryos_ast::Module,
    line_start: u32,
    line_end: u32,
    out: &mut Vec<Value>,
) {
    const HINT_KIND_CAPABILITY: u8 = 1; // rendered as a Type-kind hint
    for decl in &module.declarations {
        if let kryos_ast::Decl::Function {
            body: Some(body), ..
        } = decl
        {
            let caps = crate::cap_surface::function_caps(body);
            if caps.is_empty() {
                continue;
            }
            let label = crate::cap_surface::caps_label(&caps);
            // Anchor the hint just before the body's opening brace.
            let (ln, col) = offset_to_line_col(source, body.span.start as usize);
            if (line_start..=line_end).contains(&ln) {
                out.push(json!({
                    "position": { "line": ln, "character": col },
                    "label": format!("caps: {label} "),
                    "kind": HINT_KIND_CAPABILITY,
                    "paddingLeft": true,
                    "paddingRight": false,
                }));
            }
        }
    }
}

fn push_let_type_hints(
    source: &str,
    tokens: &[kryos_lexer::Token],
    line_start: u32,
    line_end: u32,
    out: &mut Vec<Value>,
) {
    use kryos_lexer::TokenKind as T;

    let mut i = 0;
    while i + 3 < tokens.len() {
        // Pattern: Let [Mut] Ident Eq <literal>
        let is_let = matches!(tokens[i].kind, T::Let);
        if !is_let {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if matches!(tokens[j].kind, T::Mut) {
            j += 1;
        }
        if !matches!(tokens[j].kind, T::Ident) {
            i += 1;
            continue;
        }
        let ident_end = tokens[j].span.end as usize;
        let ident = &tokens[j];
        j += 1;
        // No explicit annotation? token after Ident must be Eq (or PlusEq isn't valid for let).
        let has_annotation = matches!(tokens.get(j).map(|t| &t.kind), Some(T::Colon));
        if has_annotation {
            i += 1;
            continue;
        }
        if !matches!(tokens.get(j).map(|t| &t.kind), Some(T::Eq)) {
            i += 1;
            continue;
        }
        j += 1;
        // Infer type from the first token of the rhs.
        let Some(rhs) = tokens.get(j) else { break };
        let inferred = match rhs.kind {
            T::Integer => Some("i64"),
            T::Float => Some("f64"),
            T::String => Some("str"),
            T::True | T::False => Some("bool"),
            _ => None,
        };
        if let Some(ty) = inferred {
            let (ln, col) = offset_to_line_col(source, ident_end);
            if (line_start..=line_end).contains(&ln) {
                out.push(json!({
                    "position": { "line": ln, "character": col },
                    "label": format!(": {ty}"),
                    "kind": HINT_KIND_TYPE,
                    "paddingLeft": false,
                    "paddingRight": false,
                }));
            }
            let _ = ident;
        }
        i = j + 1;
    }
}

fn push_call_param_hints(
    source: &str,
    tokens: &[kryos_lexer::Token],
    line_start: u32,
    line_end: u32,
    signatures: &HashMap<String, Vec<String>>,
    out: &mut Vec<Value>,
) {
    use kryos_lexer::TokenKind as T;

    // Pattern: Ident LParen <arg> (Comma <arg>)* RParen
    let mut i = 0;
    while i + 1 < tokens.len() {
        if !matches!(tokens[i].kind, T::Ident) {
            i += 1;
            continue;
        }
        let name = &source[tokens[i].span.start as usize..tokens[i].span.end as usize];
        let Some(param_names) = signatures.get(name) else {
            i += 1;
            continue;
        };
        if !matches!(tokens[i + 1].kind, T::LParen) {
            i += 1;
            continue;
        }

        // Walk args, hint at each top-level arg start position.
        let mut depth = 1;
        let mut arg_idx = 0;
        let mut k = i + 2;
        let mut at_arg_start = true;
        while k < tokens.len() && depth > 0 {
            match tokens[k].kind {
                T::LParen | T::LBracket | T::LBrace => {
                    depth += 1;
                    at_arg_start = false;
                }
                T::RParen | T::RBracket | T::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                T::Comma if depth == 1 => {
                    arg_idx += 1;
                    at_arg_start = true;
                    k += 1;
                    continue;
                }
                _ => {}
            }
            if at_arg_start && depth >= 1 {
                if let Some(pname) = param_names.get(arg_idx) {
                    let arg_start = tokens[k].span.start as usize;
                    let (ln, col) = offset_to_line_col(source, arg_start);
                    if (line_start..=line_end).contains(&ln) {
                        out.push(json!({
                            "position": { "line": ln, "character": col },
                            "label": format!("{pname}:"),
                            "kind": HINT_KIND_PARAMETER,
                            "paddingLeft": false,
                            "paddingRight": true,
                        }));
                    }
                }
                at_arg_start = false;
            }
            k += 1;
        }
        i = k.max(i + 1);
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
