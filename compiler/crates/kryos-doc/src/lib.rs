//! Kryos documentation generator — extracts doc comments and signatures
//! from Kryos source files and renders them to markdown.

use kryos_ast::{Decl, Module, TypeExpr, GenericParam};
use kryos_lexer::Lexer;
use kryos_parser::parse;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// The kind of documented item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocKind {
    Function,
    Struct,
    Enum,
    Trait,
    TypeAlias,
    Actor,
    Constant,
}

impl std::fmt::Display for DocKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocKind::Function => write!(f, "fn"),
            DocKind::Struct => write!(f, "struct"),
            DocKind::Enum => write!(f, "enum"),
            DocKind::Trait => write!(f, "trait"),
            DocKind::TypeAlias => write!(f, "type"),
            DocKind::Actor => write!(f, "actor"),
            DocKind::Constant => write!(f, "const"),
        }
    }
}

/// A parameter description extracted from a function signature.
#[derive(Debug, Clone, PartialEq)]
pub struct DocParam {
    pub name: String,
    pub ty: Option<String>,
}

/// A single documented item extracted from source code.
#[derive(Debug, Clone, PartialEq)]
pub struct DocItem {
    pub name: String,
    pub kind: DocKind,
    pub signature: String,
    pub doc_comment: String,
    pub params: Vec<DocParam>,
    pub return_type: Option<String>,
    pub public: bool,
    pub fields: Vec<String>,
    pub variants: Vec<String>,
    pub methods: Vec<DocItem>,
    pub generics: Vec<String>,
}

impl DocItem {
    fn new(name: impl Into<String>, kind: DocKind) -> Self {
        Self {
            name: name.into(),
            kind,
            signature: String::new(),
            doc_comment: String::new(),
            params: Vec::new(),
            return_type: None,
            public: false,
            fields: Vec::new(),
            variants: Vec::new(),
            methods: Vec::new(),
            generics: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Type rendering helper
// ---------------------------------------------------------------------------

fn render_type_expr(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Simple { name, .. } => name.clone(),
        TypeExpr::Generic { name, args, .. } => {
            let args_str: Vec<String> = args.iter().map(render_type_expr).collect();
            format!("{}<{}>", name, args_str.join(", "))
        }
        TypeExpr::Array { element, size, .. } => {
            if let Some(s) = size {
                format!("[{}; {}]", render_type_expr(element), s)
            } else {
                format!("[{}]", render_type_expr(element))
            }
        }
        TypeExpr::Tuple { elements, .. } => {
            let elems: Vec<String> = elements.iter().map(render_type_expr).collect();
            format!("({})", elems.join(", "))
        }
        TypeExpr::Function { params, ret, .. } => {
            let params_str: Vec<String> = params.iter().map(render_type_expr).collect();
            format!("fn({}) -> {}", params_str.join(", "), render_type_expr(ret))
        }
        TypeExpr::Optional { inner, .. } => {
            format!("{}?", render_type_expr(inner))
        }
        TypeExpr::Reference { inner, mutable, .. } => {
            if *mutable {
                format!("&mut {}", render_type_expr(inner))
            } else {
                format!("&{}", render_type_expr(inner))
            }
        }
        TypeExpr::Shared { inner, .. } => format!("shared {}", render_type_expr(inner)),
        TypeExpr::Weak { inner, .. } => format!("weak {}", render_type_expr(inner)),
        TypeExpr::Pointer { inner, mutable, .. } => {
            if *mutable {
                format!("*mut {}", render_type_expr(inner))
            } else {
                format!("*{}", render_type_expr(inner))
            }
        }
        TypeExpr::DynTrait { trait_name, .. } => format!("dyn {trait_name}"),
        TypeExpr::Inferred { .. } => "_".to_string(),
    }
}

fn render_generics(generics: &[GenericParam]) -> String {
    if generics.is_empty() {
        return String::new();
    }
    let params: Vec<String> = generics
        .iter()
        .map(|g| {
            if g.bounds.is_empty() {
                g.name.clone()
            } else {
                format!("{}: {}", g.name, g.bounds.join(" + "))
            }
        })
        .collect();
    format!("<{}>", params.join(", "))
}

// ---------------------------------------------------------------------------
// Doc comment extraction from source text
// ---------------------------------------------------------------------------

/// Extract doc comments (`///`) that immediately precede a given byte offset
/// in the source code.
fn extract_preceding_doc_comment(source: &str, decl_start: u32) -> String {
    let lines: Vec<&str> = source[..decl_start as usize].lines().collect();
    let mut doc_lines: Vec<String> = Vec::new();

    // Walk backwards from the line just before the declaration
    for line in lines.iter().rev() {
        let trimmed = line.trim();
        if trimmed.starts_with("///") {
            let content = trimmed.strip_prefix("///").unwrap_or("");
            let content = content.strip_prefix(' ').unwrap_or(content);
            doc_lines.push(content.to_string());
        } else if trimmed.is_empty() {
            // Allow blank lines within doc comment blocks
            if !doc_lines.is_empty() {
                doc_lines.push(String::new());
            }
        } else {
            break;
        }
    }

    doc_lines.reverse();

    // Trim trailing blank lines
    while doc_lines.last().is_some_and(|l| l.is_empty()) {
        doc_lines.pop();
    }

    doc_lines.join("\n")
}

// ---------------------------------------------------------------------------
// Declaration -> DocItem conversion
// ---------------------------------------------------------------------------

fn doc_item_from_decl(decl: &Decl, source: &str) -> Option<DocItem> {
    match decl {
        Decl::Function {
            name,
            generics,
            params,
            ret_ty,
            public,
            span,
            ..
        } => {
            let mut item = DocItem::new(name, DocKind::Function);
            item.public = *public;
            item.generics = generics.iter().map(|g| g.name.clone()).collect();
            item.doc_comment = extract_preceding_doc_comment(source, span.start);

            // Build params
            item.params = params
                .iter()
                .map(|p| DocParam {
                    name: p.name.clone(),
                    ty: p.ty.as_ref().map(render_type_expr),
                })
                .collect();

            // Return type
            item.return_type = ret_ty.as_ref().map(render_type_expr);

            // Build signature
            let vis = if *public { "pub " } else { "" };
            let gen = render_generics(generics);
            let params_str: Vec<String> = params
                .iter()
                .map(|p| {
                    if let Some(ref ty) = p.ty {
                        format!("{}: {}", p.name, render_type_expr(ty))
                    } else {
                        p.name.clone()
                    }
                })
                .collect();
            let ret = ret_ty
                .as_ref()
                .map(|t| format!(" -> {}", render_type_expr(t)))
                .unwrap_or_default();
            item.signature = format!("{}fn {}{}({}){}", vis, name, gen, params_str.join(", "), ret);

            Some(item)
        }
        Decl::Struct {
            name,
            generics,
            fields,
            public,
            span,
            ..
        } => {
            let mut item = DocItem::new(name, DocKind::Struct);
            item.public = *public;
            item.generics = generics.iter().map(|g| g.name.clone()).collect();
            item.doc_comment = extract_preceding_doc_comment(source, span.start);
            item.fields = fields
                .iter()
                .map(|f| {
                    let vis = if f.public { "pub " } else { "" };
                    format!("{}{}: {}", vis, f.name, render_type_expr(&f.ty))
                })
                .collect();

            let vis = if *public { "pub " } else { "" };
            let gen = render_generics(generics);
            item.signature = format!("{}struct {}{}", vis, name, gen);

            Some(item)
        }
        Decl::Enum {
            name,
            generics,
            variants,
            public,
            span,
            ..
        } => {
            let mut item = DocItem::new(name, DocKind::Enum);
            item.public = *public;
            item.generics = generics.iter().map(|g| g.name.clone()).collect();
            item.doc_comment = extract_preceding_doc_comment(source, span.start);
            item.variants = variants
                .iter()
                .map(|v| {
                    if v.fields.is_empty() {
                        v.name.clone()
                    } else {
                        let fields: Vec<String> =
                            v.fields.iter().map(render_type_expr).collect();
                        format!("{}({})", v.name, fields.join(", "))
                    }
                })
                .collect();

            let vis = if *public { "pub " } else { "" };
            let gen = render_generics(generics);
            item.signature = format!("{}enum {}{}", vis, name, gen);

            Some(item)
        }
        Decl::Trait {
            name,
            generics,
            methods,
            public,
            span,
            ..
        } => {
            let mut item = DocItem::new(name, DocKind::Trait);
            item.public = *public;
            item.generics = generics.iter().map(|g| g.name.clone()).collect();
            item.doc_comment = extract_preceding_doc_comment(source, span.start);
            item.methods = methods
                .iter()
                .filter_map(|m| doc_item_from_decl(m, source))
                .collect();

            let vis = if *public { "pub " } else { "" };
            let gen = render_generics(generics);
            item.signature = format!("{}trait {}{}", vis, name, gen);

            Some(item)
        }
        Decl::TypeAlias {
            name,
            generics,
            ty,
            public,
            span,
            ..
        } => {
            let mut item = DocItem::new(name, DocKind::TypeAlias);
            item.public = *public;
            item.generics = generics.iter().map(|g| g.name.clone()).collect();
            item.doc_comment = extract_preceding_doc_comment(source, span.start);

            let vis = if *public { "pub " } else { "" };
            let gen = render_generics(generics);
            item.signature = format!("{}type {}{} = {}", vis, name, gen, render_type_expr(ty));

            Some(item)
        }
        Decl::Actor {
            name,
            state_fields,
            handlers,
            span,
            ..
        } => {
            let mut item = DocItem::new(name, DocKind::Actor);
            item.public = true;
            item.doc_comment = extract_preceding_doc_comment(source, span.start);
            item.fields = state_fields
                .iter()
                .map(|f| format!("{}: {}", f.name, render_type_expr(&f.ty)))
                .collect();
            item.methods = handlers
                .iter()
                .map(|h| {
                    let mut m = DocItem::new(&h.name, DocKind::Function);
                    m.params = h
                        .params
                        .iter()
                        .map(|p| DocParam {
                            name: p.name.clone(),
                            ty: p.ty.as_ref().map(render_type_expr),
                        })
                        .collect();
                    m.return_type = h.ret_ty.as_ref().map(render_type_expr);
                    let params_str: Vec<String> = h
                        .params
                        .iter()
                        .map(|p| {
                            if let Some(ref ty) = p.ty {
                                format!("{}: {}", p.name, render_type_expr(ty))
                            } else {
                                p.name.clone()
                            }
                        })
                        .collect();
                    let ret = h
                        .ret_ty
                        .as_ref()
                        .map(|t| format!(" -> {}", render_type_expr(t)))
                        .unwrap_or_default();
                    m.signature = format!("fn {}({}){}", h.name, params_str.join(", "), ret);
                    m
                })
                .collect();

            item.signature = format!("actor {}", name);

            Some(item)
        }
        Decl::Const {
            name,
            ty,
            public,
            span,
            ..
        } => {
            let mut item = DocItem::new(name, DocKind::Constant);
            item.public = *public;
            item.doc_comment = extract_preceding_doc_comment(source, span.start);

            let vis = if *public { "pub " } else { "" };
            let ty_str = ty
                .as_ref()
                .map(|t| format!(": {}", render_type_expr(t)))
                .unwrap_or_default();
            item.signature = format!("{}const {}{} = ...", vis, name, ty_str);

            Some(item)
        }
        // Impl blocks, imports, and externs don't generate standalone doc items
        Decl::Impl { .. } | Decl::Import { .. } | Decl::Extern { .. } => None,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Extract documentation items from Kryos source code.
///
/// Parses the source, walks the AST, and collects `DocItem`s for every
/// function, struct, enum, trait, type alias, and actor declaration.
/// Doc comments (`///`) immediately preceding a declaration are attached.
///
/// Returns an empty `Vec` if the source fails to parse.
pub fn extract_docs(source: &str) -> Vec<DocItem> {
    let file_id = 0u32;
    let tokens = Lexer::new(source, file_id).tokenize();
    let module = match parse(tokens) {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };

    extract_docs_from_module(&module, source)
}

/// Extract documentation items from an already-parsed module.
pub fn extract_docs_from_module(module: &Module, source: &str) -> Vec<DocItem> {
    module
        .declarations
        .iter()
        .filter_map(|d| doc_item_from_decl(d, source))
        .collect()
}

/// Render a list of `DocItem`s as a markdown document for a single module.
pub fn render_markdown(items: &[DocItem], module_name: &str) -> String {
    let mut out = String::new();

    // Module header
    out.push_str(&format!("# Module `{}`\n\n", module_name));

    // Separate items by kind
    let functions: Vec<&DocItem> = items.iter().filter(|i| i.kind == DocKind::Function).collect();
    let structs: Vec<&DocItem> = items.iter().filter(|i| i.kind == DocKind::Struct).collect();
    let enums: Vec<&DocItem> = items.iter().filter(|i| i.kind == DocKind::Enum).collect();
    let traits: Vec<&DocItem> = items.iter().filter(|i| i.kind == DocKind::Trait).collect();
    let type_aliases: Vec<&DocItem> = items.iter().filter(|i| i.kind == DocKind::TypeAlias).collect();
    let actors: Vec<&DocItem> = items.iter().filter(|i| i.kind == DocKind::Actor).collect();
    let constants: Vec<&DocItem> = items.iter().filter(|i| i.kind == DocKind::Constant).collect();

    // Summary section
    out.push_str("## Overview\n\n");
    if !functions.is_empty() {
        out.push_str(&format!("- **Functions:** {}\n", functions.len()));
    }
    if !structs.is_empty() {
        out.push_str(&format!("- **Structs:** {}\n", structs.len()));
    }
    if !enums.is_empty() {
        out.push_str(&format!("- **Enums:** {}\n", enums.len()));
    }
    if !traits.is_empty() {
        out.push_str(&format!("- **Traits:** {}\n", traits.len()));
    }
    if !type_aliases.is_empty() {
        out.push_str(&format!("- **Type aliases:** {}\n", type_aliases.len()));
    }
    if !actors.is_empty() {
        out.push_str(&format!("- **Actors:** {}\n", actors.len()));
    }
    if !constants.is_empty() {
        out.push_str(&format!("- **Constants:** {}\n", constants.len()));
    }
    out.push('\n');

    // Function index
    if !functions.is_empty() {
        out.push_str("## Function Index\n\n");
        out.push_str("| Function | Signature |\n");
        out.push_str("|----------|----------|\n");
        for f in &functions {
            out.push_str(&format!(
                "| [`{}`](#{}) | `{}` |\n",
                f.name,
                f.name.to_lowercase(),
                f.signature
            ));
        }
        out.push('\n');
    }

    // Render each kind in detail
    render_section(&mut out, "Structs", &structs);
    render_section(&mut out, "Enums", &enums);
    render_section(&mut out, "Traits", &traits);
    render_section(&mut out, "Type Aliases", &type_aliases);
    render_section(&mut out, "Actors", &actors);
    render_section(&mut out, "Constants", &constants);
    render_section(&mut out, "Functions", &functions);

    out
}

fn render_section(out: &mut String, title: &str, items: &[&DocItem]) {
    if items.is_empty() {
        return;
    }

    out.push_str(&format!("## {}\n\n", title));

    for item in items {
        render_doc_item(out, item, 3);
    }
}

fn render_doc_item(out: &mut String, item: &DocItem, heading_level: usize) {
    let hashes = "#".repeat(heading_level);
    out.push_str(&format!("{} `{}`\n\n", hashes, item.name));

    // Signature
    out.push_str(&format!("```kryos\n{}\n```\n\n", item.signature));

    // Doc comment
    if !item.doc_comment.is_empty() {
        out.push_str(&item.doc_comment);
        out.push_str("\n\n");
    }

    // Parameters
    if !item.params.is_empty() {
        out.push_str("**Parameters:**\n\n");
        for p in &item.params {
            let ty_str = p.ty.as_deref().unwrap_or("_");
            out.push_str(&format!("- `{}`: `{}`\n", p.name, ty_str));
        }
        out.push('\n');
    }

    // Return type
    if let Some(ref ret) = item.return_type {
        out.push_str(&format!("**Returns:** `{}`\n\n", ret));
    }

    // Fields (structs / actors)
    if !item.fields.is_empty() {
        out.push_str("**Fields:**\n\n");
        for f in &item.fields {
            out.push_str(&format!("- `{}`\n", f));
        }
        out.push('\n');
    }

    // Variants (enums)
    if !item.variants.is_empty() {
        out.push_str("**Variants:**\n\n");
        for v in &item.variants {
            out.push_str(&format!("- `{}`\n", v));
        }
        out.push('\n');
    }

    // Methods (traits / actors)
    if !item.methods.is_empty() {
        out.push_str("**Methods:**\n\n");
        for m in &item.methods {
            render_doc_item(out, m, heading_level + 1);
        }
    }
}

/// Render a module index page from multiple modules.
///
/// Each entry in `modules` is `(module_name, items)`.
pub fn render_module_index(modules: &[(String, Vec<DocItem>)]) -> String {
    let mut out = String::new();

    out.push_str("# Module Index\n\n");
    out.push_str("| Module | Functions | Structs | Enums | Traits |\n");
    out.push_str("|--------|-----------|---------|-------|--------|\n");

    for (name, items) in modules {
        let fn_count = items.iter().filter(|i| i.kind == DocKind::Function).count();
        let struct_count = items.iter().filter(|i| i.kind == DocKind::Struct).count();
        let enum_count = items.iter().filter(|i| i.kind == DocKind::Enum).count();
        let trait_count = items.iter().filter(|i| i.kind == DocKind::Trait).count();

        out.push_str(&format!(
            "| [`{}`]({}.md) | {} | {} | {} | {} |\n",
            name, name, fn_count, struct_count, enum_count, trait_count
        ));
    }

    out.push('\n');

    // Cross-reference section: list all types across modules
    let mut all_types: Vec<(&str, &str)> = Vec::new();
    for (mod_name, items) in modules {
        for item in items {
            if matches!(item.kind, DocKind::Struct | DocKind::Enum | DocKind::Trait | DocKind::TypeAlias | DocKind::Constant) {
                all_types.push((&item.name, mod_name));
            }
        }
    }

    if !all_types.is_empty() {
        all_types.sort_by_key(|(name, _)| *name);
        out.push_str("## Type Cross-Reference\n\n");
        out.push_str("| Type | Module |\n");
        out.push_str("|------|--------|\n");
        for (type_name, mod_name) in &all_types {
            out.push_str(&format!(
                "| `{}` | [`{}`]({}.md#{}) |\n",
                type_name,
                mod_name,
                mod_name,
                type_name.to_lowercase()
            ));
        }
        out.push('\n');
    }

    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_empty_source() {
        let items = extract_docs("");
        assert!(items.is_empty());
    }

    #[test]
    fn test_extract_function_basic() {
        let source = "fn add(a: i64, b: i64) -> i64 {\n    return a + b\n}\n";
        let items = extract_docs(source);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "add");
        assert_eq!(items[0].kind, DocKind::Function);
        assert_eq!(items[0].params.len(), 2);
        assert_eq!(items[0].params[0].name, "a");
        assert_eq!(items[0].params[0].ty.as_deref(), Some("i64"));
        assert_eq!(items[0].return_type.as_deref(), Some("i64"));
    }

    #[test]
    fn test_extract_doc_comment() {
        let source = "/// Absolute value of a number.\nfn abs(x: f64) -> f64 {\n    return x\n}\n";
        let items = extract_docs(source);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].doc_comment, "Absolute value of a number.");
    }

    #[test]
    fn test_extract_multi_line_doc_comment() {
        let source = "/// First line.\n/// Second line.\nfn foo() -> i64 {\n    return 0\n}\n";
        let items = extract_docs(source);
        assert_eq!(items.len(), 1);
        assert!(items[0].doc_comment.contains("First line."));
        assert!(items[0].doc_comment.contains("Second line."));
    }

    #[test]
    fn test_extract_struct() {
        let source = "/// A point in 2D space.\nstruct Point {\n    x: f64\n    y: f64\n}\n";
        let items = extract_docs(source);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "Point");
        assert_eq!(items[0].kind, DocKind::Struct);
        assert_eq!(items[0].fields.len(), 2);
        assert!(items[0].doc_comment.contains("point in 2D space"));
    }

    #[test]
    fn test_extract_enum() {
        let source = "/// Color options.\nenum Color {\n    Red\n    Green\n    Blue\n}\n";
        let items = extract_docs(source);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "Color");
        assert_eq!(items[0].kind, DocKind::Enum);
        assert_eq!(items[0].variants.len(), 3);
    }

    #[test]
    fn test_extract_pub_function() {
        let source = "pub fn greet(name: str) -> str {\n    return name\n}\n";
        let items = extract_docs(source);
        assert_eq!(items.len(), 1);
        assert!(items[0].public);
        assert!(items[0].signature.starts_with("pub fn"));
    }

    #[test]
    fn test_extract_multiple_declarations() {
        let source = concat!(
            "fn foo() -> i64 {\n    return 0\n}\n",
            "fn bar() -> i64 {\n    return 1\n}\n",
            "struct Baz {\n    x: i64\n}\n"
        );
        let items = extract_docs(source);
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn test_render_markdown_module_header() {
        let items = vec![DocItem::new("test_fn", DocKind::Function)];
        let md = render_markdown(&items, "math");
        assert!(md.starts_with("# Module `math`"));
    }

    #[test]
    fn test_render_markdown_function_index() {
        let mut item = DocItem::new("add", DocKind::Function);
        item.signature = "fn add(a: i64, b: i64) -> i64".to_string();
        let md = render_markdown(&[item], "math");
        assert!(md.contains("## Function Index"));
        assert!(md.contains("`add`"));
    }

    #[test]
    fn test_render_module_index() {
        let modules = vec![
            (
                "math".to_string(),
                vec![
                    DocItem::new("abs", DocKind::Function),
                    DocItem::new("Point", DocKind::Struct),
                ],
            ),
            (
                "string".to_string(),
                vec![DocItem::new("len", DocKind::Function)],
            ),
        ];
        let index = render_module_index(&modules);
        assert!(index.contains("# Module Index"));
        assert!(index.contains("`math`"));
        assert!(index.contains("`string`"));
        assert!(index.contains("Type Cross-Reference"));
        assert!(index.contains("`Point`"));
    }

    #[test]
    fn test_render_type_expr_simple() {
        let ty = TypeExpr::Simple {
            name: "i64".to_string(),
            span: kryos_errors::Span::DUMMY,
        };
        assert_eq!(render_type_expr(&ty), "i64");
    }

    #[test]
    fn test_render_type_expr_generic() {
        let ty = TypeExpr::Generic {
            name: "Vec".to_string(),
            args: vec![TypeExpr::Simple {
                name: "i64".to_string(),
                span: kryos_errors::Span::DUMMY,
            }],
            span: kryos_errors::Span::DUMMY,
        };
        assert_eq!(render_type_expr(&ty), "Vec<i64>");
    }

    #[test]
    fn test_render_type_expr_optional() {
        let ty = TypeExpr::Optional {
            inner: Box::new(TypeExpr::Simple {
                name: "str".to_string(),
                span: kryos_errors::Span::DUMMY,
            }),
            span: kryos_errors::Span::DUMMY,
        };
        assert_eq!(render_type_expr(&ty), "str?");
    }

    #[test]
    fn test_doc_kind_display() {
        assert_eq!(format!("{}", DocKind::Function), "fn");
        assert_eq!(format!("{}", DocKind::Struct), "struct");
        assert_eq!(format!("{}", DocKind::Enum), "enum");
        assert_eq!(format!("{}", DocKind::Trait), "trait");
    }

    #[test]
    fn test_render_markdown_empty_items() {
        let md = render_markdown(&[], "empty_module");
        assert!(md.contains("# Module `empty_module`"));
        assert!(md.contains("## Overview"));
    }
}
