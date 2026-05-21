//! LSP textDocument/semanticTokens/full — accurate syntax highlighting.
//!
//! Lexes the source then walks the token stream. For each Ident/TypeIdent
//! token we look up its role from a small in-file symbol map built from
//! the parsed AST (function names, struct/enum names, params, local lets).
//! Keywords, string literals, integer literals, and operators are
//! classified directly from the TokenKind.
//!
//! The LSP protocol requires a "legend" announcing which token types and
//! modifiers we use; clients map indices into the legend to colors via
//! their own theme. We return the legend at initialize time and the
//! per-document data here.

use std::collections::HashMap;

use serde_json::{json, Value};

// Token types — order matters; indices into this list are what we emit.
const T_KEYWORD: u32 = 0;
const T_TYPE: u32 = 1;
const T_FUNCTION: u32 = 2;
const T_VARIABLE: u32 = 3;
const T_PARAMETER: u32 = 4;
const T_STRING: u32 = 5;
const T_NUMBER: u32 = 6;
#[allow(dead_code)] // LSP protocol constant; reserved for doc-comment highlighting
const T_COMMENT: u32 = 7;
const T_OPERATOR: u32 = 8;
const T_PROPERTY: u32 = 9;
const T_ENUM_MEMBER: u32 = 10;
const T_MACRO: u32 = 11; // for builtins like println, len, to_string

pub fn legend() -> Value {
    json!({
        "tokenTypes": [
            "keyword", "type", "function", "variable", "parameter",
            "string", "number", "comment", "operator", "property",
            "enumMember", "macro"
        ],
        "tokenModifiers": [
            "declaration", "definition", "readonly", "static", "deprecated"
        ]
    })
}

pub fn semantic_tokens(source: &str) -> Value {
    let tokens = kryos_lexer::Lexer::new(source, 0).tokenize();
    let symbols = build_symbol_map(source);

    // The LSP wire format is a flat array of u32 deltas:
    //   [deltaLine, deltaStart, length, tokenType, tokenModifiers] *
    let mut data: Vec<u32> = Vec::new();
    let mut prev_line: u32 = 0;
    let mut prev_col: u32 = 0;

    for tok in &tokens {
        let Some((kind, line, col, len)) = classify(source, tok, &symbols) else {
            continue;
        };
        let delta_line = line - prev_line;
        let delta_start = if delta_line == 0 {
            col - prev_col
        } else {
            col
        };
        data.push(delta_line);
        data.push(delta_start);
        data.push(len);
        data.push(kind);
        data.push(0); // no modifiers in v3.14 first cut
        prev_line = line;
        prev_col = col;
    }

    json!({ "data": data })
}

fn classify(
    source: &str,
    tok: &kryos_lexer::Token,
    symbols: &HashMap<String, u32>,
) -> Option<(u32, u32, u32, u32)> {
    use kryos_lexer::TokenKind as T;
    let kind: u32 = match tok.kind {
        T::Fn | T::Let | T::Mut | T::If | T::Elif | T::Else | T::While | T::For | T::In
        | T::Break | T::Continue | T::Return | T::Match | T::Struct | T::Enum
        | T::Trait | T::Impl | T::Use | T::Pub | T::Extern | T::True | T::False | T::As => T_KEYWORD,
        T::String | T::InterpStart | T::InterpEnd => T_STRING,
        T::Integer | T::Float => T_NUMBER,
        T::Plus | T::Minus | T::Star | T::Slash | T::Percent | T::Eq | T::EqEq | T::BangEq
        | T::Lt | T::Gt | T::LtEq | T::GtEq | T::AmpAmp | T::PipePipe | T::Bang
        | T::PlusEq | T::MinusEq | T::StarEq | T::SlashEq | T::Arrow | T::FatArrow => T_OPERATOR,
        T::Ident => {
            let text = &source[tok.span.start as usize..tok.span.end as usize];
            // Builtin names get the macro color.
            if is_builtin(text) {
                T_MACRO
            } else if let Some(&k) = symbols.get(text) {
                k
            } else {
                T_VARIABLE
            }
        }
        T::TypeIdent => T_TYPE,
        _ => return None,
    };

    let start_offset = tok.span.start as usize;
    let (line, col) = offset_to_line_col(source, start_offset);
    let len = (tok.span.end - tok.span.start) as u32;
    Some((kind, line, col, len))
}

fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "println"
            | "print"
            | "len"
            | "to_string"
            | "parse_int"
            | "parse_float"
            | "args"
            | "env_get"
            | "file_read"
            | "file_write"
            | "file_exists"
            | "create_dir"
            | "push"
            | "substr"
            | "split_lines"
            | "char_code"
            | "contains"
            | "sqrt"
            | "pow"
            | "sin"
            | "cos"
            | "abs"
            | "min"
            | "max"
            | "assert"
            | "throw"
            | "spawn"
            | "await"
    )
}

fn build_symbol_map(source: &str) -> HashMap<String, u32> {
    let mut map: HashMap<String, u32> = HashMap::new();
    let tokens = kryos_lexer::Lexer::new(source, 0).tokenize();
    let Ok(module) = kryos_parser::parse(tokens) else {
        return map;
    };

    for decl in &module.declarations {
        match decl {
            kryos_ast::Decl::Function { name, params, .. } => {
                map.insert(name.clone(), T_FUNCTION);
                for p in params {
                    map.insert(p.name.clone(), T_PARAMETER);
                }
            }
            kryos_ast::Decl::Struct { name, fields, .. } => {
                map.insert(name.clone(), T_TYPE);
                for f in fields {
                    map.insert(f.name.clone(), T_PROPERTY);
                }
            }
            kryos_ast::Decl::Enum { name, variants, .. } => {
                map.insert(name.clone(), T_TYPE);
                for v in variants {
                    map.insert(v.name.clone(), T_ENUM_MEMBER);
                }
            }
            kryos_ast::Decl::Trait { name, .. }
            | kryos_ast::Decl::TypeAlias { name, .. }
            | kryos_ast::Decl::Actor { name, .. } => {
                map.insert(name.clone(), T_TYPE);
            }
            kryos_ast::Decl::Const { name, .. } => {
                map.insert(name.clone(), T_VARIABLE);
            }
            _ => {}
        }
    }
    map
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
