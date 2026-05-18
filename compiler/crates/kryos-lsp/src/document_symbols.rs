//! LSP textDocument/documentSymbol — returns the outline for a single file.

use serde_json::{json, Value};

/// LSP SymbolKind constants (subset used by Kryos).
const SK_FUNCTION: u8 = 12;
const SK_CONSTANT: u8 = 14;
const SK_VARIABLE: u8 = 13;
const SK_STRUCT: u8 = 23;
const SK_ENUM: u8 = 10;
const SK_ENUM_MEMBER: u8 = 22;
const SK_INTERFACE: u8 = 11; // trait
const SK_CLASS: u8 = 5; // impl / actor
const SK_TYPE_PARAMETER: u8 = 26;
const SK_FIELD: u8 = 8;
const SK_MODULE: u8 = 2; // extern block

/// Return DocumentSymbol[] for the source (hierarchical outline).
pub fn document_symbols(source: &str) -> Value {
    let tokens = kryos_lexer::Lexer::new(source, 0).tokenize();
    let module = match kryos_parser::parse(tokens) {
        Ok(m) => m,
        Err(_) => return json!([]),
    };

    let mut out: Vec<Value> = Vec::new();
    for decl in &module.declarations {
        if let Some(sym) = decl_to_symbol(source, decl) {
            out.push(sym);
        }
    }

    Value::Array(out)
}

fn decl_to_symbol(source: &str, decl: &kryos_ast::Decl) -> Option<Value> {
    match decl {
        kryos_ast::Decl::Function {
            name,
            params,
            ret_ty,
            span,
            is_async,
            ..
        } => {
            let kind = SK_FUNCTION;
            let detail = function_signature(name, params, ret_ty.as_ref(), *is_async);
            Some(make_symbol(source, name, &detail, kind, span, None))
        }
        kryos_ast::Decl::Struct {
            name, fields, span, ..
        } => {
            let children: Vec<Value> = fields
                .iter()
                .map(|f| {
                    make_symbol(
                        source,
                        &f.name,
                        &type_to_string(&f.ty),
                        SK_FIELD,
                        &f.span,
                        None,
                    )
                })
                .collect();
            Some(make_symbol(
                source,
                name,
                "struct",
                SK_STRUCT,
                span,
                Some(children),
            ))
        }
        kryos_ast::Decl::Enum {
            name,
            variants,
            span,
            ..
        } => {
            let children: Vec<Value> = variants
                .iter()
                .map(|v| {
                    make_symbol(source, &v.name, "variant", SK_ENUM_MEMBER, &v.span, None)
                })
                .collect();
            Some(make_symbol(
                source,
                name,
                "enum",
                SK_ENUM,
                span,
                Some(children),
            ))
        }
        kryos_ast::Decl::Trait {
            name,
            methods,
            span,
            ..
        } => {
            let children: Vec<Value> = methods
                .iter()
                .filter_map(|m| decl_to_symbol(source, m))
                .collect();
            Some(make_symbol(
                source,
                name,
                "trait",
                SK_INTERFACE,
                span,
                Some(children),
            ))
        }
        kryos_ast::Decl::Impl {
            target,
            trait_name,
            methods,
            span,
            ..
        } => {
            let label = match trait_name {
                Some(t) => format!("impl {t} for {target}"),
                None => format!("impl {target}"),
            };
            let children: Vec<Value> = methods
                .iter()
                .filter_map(|m| decl_to_symbol(source, m))
                .collect();
            Some(make_symbol(
                source,
                &label,
                "impl",
                SK_CLASS,
                span,
                Some(children),
            ))
        }
        kryos_ast::Decl::Actor {
            name,
            state_fields,
            handlers,
            span,
            ..
        } => {
            let mut children: Vec<Value> = state_fields
                .iter()
                .map(|f| {
                    make_symbol(
                        source,
                        &f.name,
                        &type_to_string(&f.ty),
                        SK_FIELD,
                        &f.span,
                        None,
                    )
                })
                .collect();
            for h in handlers {
                children.push(make_symbol(
                    source,
                    &h.name,
                    "handler",
                    SK_FUNCTION,
                    &h.span,
                    None,
                ));
            }
            Some(make_symbol(
                source,
                name,
                "actor",
                SK_CLASS,
                span,
                Some(children),
            ))
        }
        kryos_ast::Decl::TypeAlias { name, span, .. } => Some(make_symbol(
            source,
            name,
            "type alias",
            SK_TYPE_PARAMETER,
            span,
            None,
        )),
        kryos_ast::Decl::Const {
            name,
            ty,
            mutable,
            span,
            ..
        } => {
            let kind = if *mutable { SK_VARIABLE } else { SK_CONSTANT };
            let detail = match ty {
                Some(t) => type_to_string(t),
                None => if *mutable { "let mut".into() } else { "let".into() },
            };
            Some(make_symbol(source, name, &detail, kind, span, None))
        }
        kryos_ast::Decl::Extern { abi, items, span, .. } => {
            let children: Vec<Value> = items
                .iter()
                .filter_map(|d| decl_to_symbol(source, d))
                .collect();
            Some(make_symbol(
                source,
                &format!("extern \"{abi}\""),
                "extern",
                SK_MODULE,
                span,
                Some(children),
            ))
        }
        kryos_ast::Decl::Import { .. } => None,
    }
}

fn make_symbol(
    source: &str,
    name: &str,
    detail: &str,
    kind: u8,
    span: &kryos_ast::Span,
    children: Option<Vec<Value>>,
) -> Value {
    let (start_line, start_col) = offset_to_line_col(source, span.start as usize);
    let (end_line, end_col) = offset_to_line_col(source, span.end as usize);
    let range = json!({
        "start": { "line": start_line, "character": start_col },
        "end":   { "line": end_line,   "character": end_col },
    });

    let mut obj = json!({
        "name": name,
        "detail": detail,
        "kind": kind,
        "range": range.clone(),
        "selectionRange": range,
    });

    if let Some(kids) = children {
        if !kids.is_empty() {
            obj["children"] = Value::Array(kids);
        }
    }
    obj
}

fn function_signature(
    name: &str,
    params: &[kryos_ast::Param],
    ret_ty: Option<&kryos_ast::TypeExpr>,
    is_async: bool,
) -> String {
    let prefix = if is_async { "async fn " } else { "fn " };
    let params_s = params
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
    format!("{prefix}{name}({params_s}){ret}")
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
