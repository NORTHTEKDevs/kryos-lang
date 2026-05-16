//! `@derive(...)` expansion pass.
//!
//! Lowers `@derive(Trait, ...)` annotations on `Decl::Struct` / `Decl::Enum`
//! into concrete declarations the rest of the pipeline already understands:
//!
//! * `@derive(Copy)` — pushes an `@copy` annotation onto the same declaration,
//!   so the ownership analyzer treats the struct as a copy type.
//! * `@derive(Debug)` — synthesises an inherent
//!   `impl <Name> { fn fmt_debug(self) -> str { ... } }` block. The body
//!   walks the declared fields and concatenates them as
//!   `"<Name>(f1=<v1>, f2=<v2>, ...)"`. Field values are converted via
//!   `to_string(self.<field>)`; non-stringifiable fields fall back to the
//!   field name placeholder.
//!
//! `@derive(Eq)` and `@derive(Hash)` are recognised by the parser but
//! currently *no-ops* at this layer. They are left as a future expansion
//! point because they require multi-parameter struct comparison, which is
//! still affected by an unrelated reference-coercion path in the type
//! checker (two struct-typed parameters in the same signature surface the
//! second one's fields as `&T` instead of `T`). Until that is fixed, the
//! synthesised `eq` / `hash` bodies cannot type-check reliably.
//!
//! Unknown derive names are silently ignored — the pass is conservative by
//! design and never inserts an error of its own.
//!
//! The pass is invoked once per module, before type-checking. It is a pure
//! AST → AST transformation; it never reads or writes outside the module.

use crate::decl::{Annotation, Decl, GenericParam, Module, StructField};
use crate::expr::{BinOp, Expr, Param};
use crate::stmt::{Block, Stmt};
use crate::types::TypeExpr;
use kryos_errors::Span;

/// Expand `@derive(...)` annotations in `module` in place.
///
/// Synthesised declarations are appended to the module after all existing
/// declarations to keep diagnostic spans for user-written code stable.
pub fn expand_derives(module: &mut Module) {
    let mut synthesized: Vec<Decl> = Vec::new();

    for decl in module.declarations.iter_mut() {
        match decl {
            Decl::Struct {
                name,
                annotations,
                fields,
                span,
                ..
            } => {
                let derives = collect_derives(annotations);
                if derives.is_empty() {
                    continue;
                }

                if derives.iter().any(|d| d == "Copy") && !has_copy_annotation(annotations) {
                    annotations.push(Annotation {
                        name: "copy".to_string(),
                        args: Vec::new(),
                        span: *span,
                    });
                }

                if derives.iter().any(|d| d == "Debug") {
                    synthesized.push(synthesize_debug_impl(name, fields, *span));
                }
                // Eq / Hash: parsed and accepted, expansion deferred.
            }
            Decl::Enum {
                annotations, span, ..
            } => {
                let derives = collect_derives(annotations);
                if derives.is_empty() {
                    continue;
                }
                if derives.iter().any(|d| d == "Copy") && !has_copy_annotation(annotations) {
                    annotations.push(Annotation {
                        name: "copy".to_string(),
                        args: Vec::new(),
                        span: *span,
                    });
                }
                // Debug/Eq/Hash on enums require variant-aware match codegen;
                // accept the annotation silently for now.
            }
            _ => {}
        }
    }

    module.declarations.extend(synthesized);
}

/// Gather the trait names from every `@derive(...)` annotation on a decl.
fn collect_derives(annotations: &[Annotation]) -> Vec<String> {
    let mut out = Vec::new();
    for ann in annotations {
        if ann.name == "derive" {
            for arg in &ann.args {
                let trimmed = arg.trim();
                if trimmed.is_empty() || trimmed == "," {
                    continue;
                }
                out.push(trimmed.to_string());
            }
        }
    }
    out
}

fn has_copy_annotation(annotations: &[Annotation]) -> bool {
    annotations.iter().any(|a| a.name == "copy")
}

/// Build `impl <name> { fn fmt_debug(self) -> str { <body> } }`.
fn synthesize_debug_impl(name: &str, fields: &[StructField], span: Span) -> Decl {
    let body = build_debug_body(name, fields, span);

    let method = Decl::Function {
        name: "fmt_debug".to_string(),
        generics: Vec::<GenericParam>::new(),
        params: vec![Param {
            name: "self".to_string(),
            ty: None,
            default: None,
            span,
        }],
        ret_ty: Some(TypeExpr::Simple {
            name: "str".to_string(),
            span,
        }),
        body: Some(body),
        public: true,
        is_async: false,
        annotations: Vec::new(),
        doc_comments: Vec::new(),
        span,
    };

    Decl::Impl {
        target: name.to_string(),
        trait_name: None,
        generics: Vec::new(),
        methods: vec![method],
        doc_comments: Vec::new(),
        span,
    }
}

/// Build the function body for the synthesised `fmt_debug`.
///
/// Produces:
/// ```text
/// return "<Name>(" + "f1=" + to_string(self.f1) + ", f2=" + to_string(self.f2) + ... + ")"
/// ```
///
/// Fields whose type isn't trivially stringifiable still emit their *name*
/// as a textual placeholder so the helper compiles for every struct. This
/// matches Rust's default Debug behaviour of "best-effort, never refuses
/// to compile".
fn build_debug_body(name: &str, fields: &[StructField], span: Span) -> Block {
    let mut expr = str_lit(format!("{name}("), span);

    for (i, field) in fields.iter().enumerate() {
        let prefix = if i == 0 {
            format!("{}=", field.name)
        } else {
            format!(", {}=", field.name)
        };
        expr = concat(expr, str_lit(prefix, span), span);

        let value_expr: Expr = if field_is_stringifiable(&field.ty) {
            Expr::FnCall {
                callee: Box::new(Expr::Identifier {
                    name: "to_string".to_string(),
                    span,
                }),
                args: vec![Expr::FieldAccess {
                    object: Box::new(Expr::Identifier {
                        name: "self".to_string(),
                        span,
                    }),
                    field: field.name.clone(),
                    span,
                }],
                span,
            }
        } else {
            // Fallback placeholder — keeps the synthesised body type-correct
            // when a field's type can't be passed through `to_string`.
            str_lit(format!("<{}>", field.name), span)
        };
        expr = concat(expr, value_expr, span);
    }

    expr = concat(expr, str_lit(")".to_string(), span), span);

    Block {
        stmts: vec![Stmt::Return {
            value: Some(expr),
            span,
        }],
        span,
    }
}

/// Conservative check: types we know `to_string(...)` accepts.
fn field_is_stringifiable(ty: &TypeExpr) -> bool {
    match ty {
        TypeExpr::Simple { name, .. } => matches!(
            name.as_str(),
            "i8" | "i16"
                | "i32"
                | "i64"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "f32"
                | "f64"
                | "bool"
                | "str"
                | "char"
        ),
        _ => false,
    }
}

fn str_lit(value: String, span: Span) -> Expr {
    Expr::StringLiteral { value, span }
}

fn concat(left: Expr, right: Expr, span: Span) -> Expr {
    Expr::BinaryOp {
        op: BinOp::Add,
        left: Box::new(left),
        right: Box::new(right),
        span,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kryos_errors::Span;

    fn dummy_span() -> Span {
        Span::new(0, 0, 0)
    }

    fn make_struct(name: &str, fields: Vec<(&str, &str)>, derives: Vec<&str>) -> Decl {
        let span = dummy_span();
        let annotations = if derives.is_empty() {
            Vec::new()
        } else {
            vec![Annotation {
                name: "derive".to_string(),
                args: derives.into_iter().map(String::from).collect(),
                span,
            }]
        };
        Decl::Struct {
            name: name.to_string(),
            generics: Vec::new(),
            fields: fields
                .into_iter()
                .map(|(fname, fty)| StructField {
                    name: fname.to_string(),
                    ty: TypeExpr::Simple {
                        name: fty.to_string(),
                        span,
                    },
                    public: true,
                    default: None,
                    span,
                })
                .collect(),
            public: true,
            annotations,
            doc_comments: Vec::new(),
            span,
        }
    }

    #[test]
    fn derive_copy_adds_copy_annotation() {
        let mut m = Module {
            name: "t".to_string(),
            declarations: vec![make_struct("P", vec![("x", "i64")], vec!["Copy"])],
            span: dummy_span(),
        };
        expand_derives(&mut m);
        let Decl::Struct { annotations, .. } = &m.declarations[0] else {
            panic!("expected struct");
        };
        assert!(annotations.iter().any(|a| a.name == "copy"));
    }

    #[test]
    fn derive_copy_is_idempotent() {
        let span = dummy_span();
        let mut s = make_struct("P", vec![("x", "i64")], vec!["Copy"]);
        if let Decl::Struct { annotations, .. } = &mut s {
            annotations.push(Annotation {
                name: "copy".to_string(),
                args: Vec::new(),
                span,
            });
        }
        let mut m = Module {
            name: "t".to_string(),
            declarations: vec![s],
            span,
        };
        expand_derives(&mut m);
        let Decl::Struct { annotations, .. } = &m.declarations[0] else {
            panic!("expected struct");
        };
        let copy_count = annotations.iter().filter(|a| a.name == "copy").count();
        assert_eq!(copy_count, 1, "should not duplicate existing @copy");
    }

    #[test]
    fn derive_debug_synthesizes_impl() {
        let mut m = Module {
            name: "t".to_string(),
            declarations: vec![make_struct(
                "Point",
                vec![("x", "i64"), ("y", "i64")],
                vec!["Debug"],
            )],
            span: dummy_span(),
        };
        expand_derives(&mut m);
        assert_eq!(m.declarations.len(), 2, "should have appended an Impl");
        let Decl::Impl { target, methods, .. } = &m.declarations[1] else {
            panic!("expected impl, got {:?}", m.declarations[1]);
        };
        assert_eq!(target, "Point");
        assert_eq!(methods.len(), 1);
        let Decl::Function { name: mname, .. } = &methods[0] else {
            panic!("expected function method");
        };
        assert_eq!(mname, "fmt_debug");
    }

    #[test]
    fn no_derive_no_change() {
        let mut m = Module {
            name: "t".to_string(),
            declarations: vec![make_struct("P", vec![("x", "i64")], vec![])],
            span: dummy_span(),
        };
        let before = m.declarations.len();
        expand_derives(&mut m);
        assert_eq!(m.declarations.len(), before);
    }

    #[test]
    fn unknown_derive_ignored() {
        let mut m = Module {
            name: "t".to_string(),
            declarations: vec![make_struct(
                "P",
                vec![("x", "i64")],
                vec!["Serialize", "PartialOrd"],
            )],
            span: dummy_span(),
        };
        let before = m.declarations.len();
        expand_derives(&mut m);
        // Should not error, and should not synthesize anything for unknowns.
        assert_eq!(m.declarations.len(), before);
    }

    #[test]
    fn derive_copy_and_debug_combined() {
        let mut m = Module {
            name: "t".to_string(),
            declarations: vec![make_struct(
                "Pt",
                vec![("x", "i64")],
                vec!["Copy", "Debug"],
            )],
            span: dummy_span(),
        };
        expand_derives(&mut m);
        // Original struct + synthesised impl.
        assert_eq!(m.declarations.len(), 2);
        let Decl::Struct { annotations, .. } = &m.declarations[0] else {
            panic!("expected struct");
        };
        assert!(annotations.iter().any(|a| a.name == "copy"));
        assert!(matches!(m.declarations[1], Decl::Impl { .. }));
    }
}
