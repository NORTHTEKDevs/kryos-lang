//! Kryos documentation generator — extracts doc comments and signatures
//! from Kryos source files and renders them to markdown.

use kryos_ast::{Decl, GenericParam, Module, TypeExpr};
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
            item.signature = format!(
                "{}fn {}{}({}){}",
                vis,
                name,
                gen,
                params_str.join(", "),
                ret
            );

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
                        let fields: Vec<String> = v.fields.iter().map(render_type_expr).collect();
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
    let functions: Vec<&DocItem> = items
        .iter()
        .filter(|i| i.kind == DocKind::Function)
        .collect();
    let structs: Vec<&DocItem> = items.iter().filter(|i| i.kind == DocKind::Struct).collect();
    let enums: Vec<&DocItem> = items.iter().filter(|i| i.kind == DocKind::Enum).collect();
    let traits: Vec<&DocItem> = items.iter().filter(|i| i.kind == DocKind::Trait).collect();
    let type_aliases: Vec<&DocItem> = items
        .iter()
        .filter(|i| i.kind == DocKind::TypeAlias)
        .collect();
    let actors: Vec<&DocItem> = items.iter().filter(|i| i.kind == DocKind::Actor).collect();
    let constants: Vec<&DocItem> = items
        .iter()
        .filter(|i| i.kind == DocKind::Constant)
        .collect();

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
            if matches!(
                item.kind,
                DocKind::Struct
                    | DocKind::Enum
                    | DocKind::Trait
                    | DocKind::TypeAlias
                    | DocKind::Constant
            ) {
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
// HTML rendering
// ---------------------------------------------------------------------------

/// Escape a string for safe insertion into HTML text/attribute content.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Convert a name into a stable HTML id slug (lowercase, alnum + dash).
fn slug(name: &str) -> String {
    let mut s = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else {
            s.push('-');
        }
    }
    s
}

/// Render a turn-key HTML page wrapping `body`.
fn html_page(title: &str, body: &str) -> String {
    let css = include_str_default_css();
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n  <title>{title}</title>\n  <style>\n{css}\n  </style>\n</head>\n<body>\n<main class=\"kryos-doc\">\n{body}\n</main>\n</body>\n</html>\n",
        title = html_escape(title),
        css = css,
        body = body,
    )
}

fn include_str_default_css() -> &'static str {
    r#"    :root { color-scheme: light dark; }
    body { margin: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; line-height: 1.6; }
    main.kryos-doc { max-width: 960px; margin: 0 auto; padding: 2rem 1.25rem 4rem; }
    h1, h2, h3, h4, h5 { line-height: 1.25; margin-top: 1.8em; margin-bottom: 0.5em; }
    h1 { border-bottom: 1px solid #8884; padding-bottom: 0.3em; }
    h2 { border-bottom: 1px solid #8882; padding-bottom: 0.2em; }
    code, pre { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
    code { background: #8881; padding: 0.1em 0.35em; border-radius: 4px; font-size: 0.92em; }
    pre { background: #8881; padding: 1em; border-radius: 6px; overflow-x: auto; }
    pre code { background: transparent; padding: 0; }
    table { border-collapse: collapse; width: 100%; margin: 1em 0; }
    th, td { text-align: left; padding: 0.45em 0.7em; border-bottom: 1px solid #8883; }
    th { font-weight: 600; }
    a { color: #2563eb; text-decoration: none; }
    a:hover { text-decoration: underline; }
    .kryos-kind { display: inline-block; font-size: 0.72em; font-weight: 600; padding: 0.1em 0.5em; border-radius: 999px; background: #2563eb22; color: #2563eb; vertical-align: middle; margin-left: 0.4em; }
    .kryos-item { margin: 1.4em 0 1.8em; padding-top: 0.4em; }
    .kryos-item-sig { margin-bottom: 0.75em; }
    .kryos-meta { font-size: 0.9em; opacity: 0.85; }
    @media (prefers-color-scheme: dark) {
      body { background: #0f1115; color: #e6e6e6; }
      a { color: #60a5fa; }
      .kryos-kind { background: #60a5fa22; color: #60a5fa; }
    }"#
}

fn kind_label(kind: &DocKind) -> &'static str {
    match kind {
        DocKind::Function => "fn",
        DocKind::Struct => "struct",
        DocKind::Enum => "enum",
        DocKind::Trait => "trait",
        DocKind::TypeAlias => "type",
        DocKind::Actor => "actor",
        DocKind::Constant => "const",
    }
}

fn render_html_doc_item(out: &mut String, item: &DocItem, heading_level: usize) {
    let level = heading_level.clamp(2, 6);
    let id = slug(&item.name);
    out.push_str(&format!(
        "<section class=\"kryos-item\" id=\"{id}\">\n  <h{level}><code>{name}</code><span class=\"kryos-kind\">{kind}</span></h{level}>\n",
        id = html_escape(&id),
        level = level,
        name = html_escape(&item.name),
        kind = kind_label(&item.kind),
    ));
    out.push_str(&format!(
        "  <pre class=\"kryos-item-sig\"><code>{}</code></pre>\n",
        html_escape(&item.signature),
    ));
    if !item.doc_comment.is_empty() {
        // Doc comment is rendered as plain text paragraphs (no markdown
        // interpretation here — keep it simple and predictable).
        for para in item.doc_comment.split("\n\n") {
            let trimmed = para.trim();
            if trimmed.is_empty() {
                continue;
            }
            out.push_str(&format!("  <p>{}</p>\n", html_escape(trimmed)));
        }
    }
    if !item.params.is_empty() {
        out.push_str("  <p><strong>Parameters:</strong></p>\n  <ul>\n");
        for p in &item.params {
            let ty_str = p.ty.as_deref().unwrap_or("_");
            out.push_str(&format!(
                "    <li><code>{}</code>: <code>{}</code></li>\n",
                html_escape(&p.name),
                html_escape(ty_str),
            ));
        }
        out.push_str("  </ul>\n");
    }
    if let Some(ref ret) = item.return_type {
        out.push_str(&format!(
            "  <p><strong>Returns:</strong> <code>{}</code></p>\n",
            html_escape(ret),
        ));
    }
    if !item.fields.is_empty() {
        out.push_str("  <p><strong>Fields:</strong></p>\n  <ul>\n");
        for f in &item.fields {
            out.push_str(&format!("    <li><code>{}</code></li>\n", html_escape(f)));
        }
        out.push_str("  </ul>\n");
    }
    if !item.variants.is_empty() {
        out.push_str("  <p><strong>Variants:</strong></p>\n  <ul>\n");
        for v in &item.variants {
            out.push_str(&format!("    <li><code>{}</code></li>\n", html_escape(v)));
        }
        out.push_str("  </ul>\n");
    }
    if !item.methods.is_empty() {
        out.push_str("  <p><strong>Methods:</strong></p>\n");
        for m in &item.methods {
            render_html_doc_item(out, m, heading_level + 1);
        }
    }
    out.push_str("</section>\n");
}

fn render_html_section(out: &mut String, title: &str, items: &[&DocItem]) {
    if items.is_empty() {
        return;
    }
    out.push_str(&format!(
        "<h2 id=\"{}\">{}</h2>\n",
        slug(title),
        html_escape(title)
    ));
    for item in items {
        render_html_doc_item(out, item, 3);
    }
}

/// Render a module's docs as a full HTML page.
pub fn render_html(items: &[DocItem], module_name: &str) -> String {
    let mut body = String::new();
    body.push_str(&format!(
        "<h1>Module <code>{}</code></h1>\n",
        html_escape(module_name)
    ));

    let functions: Vec<&DocItem> = items
        .iter()
        .filter(|i| i.kind == DocKind::Function)
        .collect();
    let structs: Vec<&DocItem> = items.iter().filter(|i| i.kind == DocKind::Struct).collect();
    let enums: Vec<&DocItem> = items.iter().filter(|i| i.kind == DocKind::Enum).collect();
    let traits: Vec<&DocItem> = items.iter().filter(|i| i.kind == DocKind::Trait).collect();
    let type_aliases: Vec<&DocItem> = items
        .iter()
        .filter(|i| i.kind == DocKind::TypeAlias)
        .collect();
    let actors: Vec<&DocItem> = items.iter().filter(|i| i.kind == DocKind::Actor).collect();
    let constants: Vec<&DocItem> = items
        .iter()
        .filter(|i| i.kind == DocKind::Constant)
        .collect();

    body.push_str("<h2>Overview</h2>\n<ul>\n");
    let mut any_overview = false;
    for (label, count) in [
        ("Functions", functions.len()),
        ("Structs", structs.len()),
        ("Enums", enums.len()),
        ("Traits", traits.len()),
        ("Type aliases", type_aliases.len()),
        ("Actors", actors.len()),
        ("Constants", constants.len()),
    ] {
        if count > 0 {
            body.push_str(&format!(
                "  <li><strong>{}:</strong> {}</li>\n",
                label, count
            ));
            any_overview = true;
        }
    }
    if !any_overview {
        body.push_str("  <li><em>(empty module)</em></li>\n");
    }
    body.push_str("</ul>\n");

    if !functions.is_empty() {
        body.push_str("<h2>Function Index</h2>\n<table>\n  <thead><tr><th>Function</th><th>Signature</th></tr></thead>\n  <tbody>\n");
        for f in &functions {
            body.push_str(&format!(
                "    <tr><td><a href=\"#{id}\"><code>{name}</code></a></td><td><code>{sig}</code></td></tr>\n",
                id = html_escape(&slug(&f.name)),
                name = html_escape(&f.name),
                sig = html_escape(&f.signature),
            ));
        }
        body.push_str("  </tbody>\n</table>\n");
    }

    render_html_section(&mut body, "Structs", &structs);
    render_html_section(&mut body, "Enums", &enums);
    render_html_section(&mut body, "Traits", &traits);
    render_html_section(&mut body, "Type Aliases", &type_aliases);
    render_html_section(&mut body, "Actors", &actors);
    render_html_section(&mut body, "Constants", &constants);
    render_html_section(&mut body, "Functions", &functions);

    html_page(&format!("Module {} — Kryos", module_name), &body)
}

/// Render a module index page as HTML linking to each module's page.
pub fn render_html_index(modules: &[(String, Vec<DocItem>)]) -> String {
    let mut body = String::new();
    body.push_str("<h1>Module Index</h1>\n");
    body.push_str("<table>\n  <thead><tr><th>Module</th><th>Functions</th><th>Structs</th><th>Enums</th><th>Traits</th></tr></thead>\n  <tbody>\n");
    for (name, items) in modules {
        let fn_count = items.iter().filter(|i| i.kind == DocKind::Function).count();
        let struct_count = items.iter().filter(|i| i.kind == DocKind::Struct).count();
        let enum_count = items.iter().filter(|i| i.kind == DocKind::Enum).count();
        let trait_count = items.iter().filter(|i| i.kind == DocKind::Trait).count();
        body.push_str(&format!(
            "    <tr><td><a href=\"{name}.html\"><code>{name}</code></a></td><td>{fns}</td><td>{ss}</td><td>{es}</td><td>{ts}</td></tr>\n",
            name = html_escape(name),
            fns = fn_count,
            ss = struct_count,
            es = enum_count,
            ts = trait_count,
        ));
    }
    body.push_str("  </tbody>\n</table>\n");

    let mut all_types: Vec<(&str, &str)> = Vec::new();
    for (mod_name, items) in modules {
        for item in items {
            if matches!(
                item.kind,
                DocKind::Struct
                    | DocKind::Enum
                    | DocKind::Trait
                    | DocKind::TypeAlias
                    | DocKind::Constant
            ) {
                all_types.push((&item.name, mod_name));
            }
        }
    }
    if !all_types.is_empty() {
        all_types.sort_by_key(|(name, _)| *name);
        body.push_str("<h2>Type Cross-Reference</h2>\n<table>\n  <thead><tr><th>Type</th><th>Module</th></tr></thead>\n  <tbody>\n");
        for (type_name, mod_name) in &all_types {
            body.push_str(&format!(
                "    <tr><td><code>{tn}</code></td><td><a href=\"{mn}.html#{anchor}\"><code>{mn}</code></a></td></tr>\n",
                tn = html_escape(type_name),
                mn = html_escape(mod_name),
                anchor = html_escape(&slug(type_name)),
            ));
        }
        body.push_str("  </tbody>\n</table>\n");
    }

    html_page("Module Index — Kryos", &body)
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
