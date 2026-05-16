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
//! * `@derive(Eq)` — synthesises an inherent
//!   `impl <Name> { fn eq(self, other: <Name>) -> bool { ... } }` block that
//!   compares every field pairwise with `and`. Only valid when every field
//!   itself supports `==` (primitives, strings, and other structs that
//!   themselves derive `Eq`).
//! * `@derive(Hash)` — synthesises an inherent
//!   `impl <Name> { fn hash(self) -> i64 { ... } }` block that combines the
//!   field hashes using a FNV-style multiply-add fold. Primitive fields are
//!   used as-is; non-primitive fields fall back to `0` (stable but coarse).
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
                if derives.iter().any(|d| d == "Eq") {
                    synthesized.push(synthesize_eq_impl(name, fields, *span));
                }
                if derives.iter().any(|d| d == "Hash") {
                    synthesized.push(synthesize_hash_impl(name, fields, *span));
                }
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

/// Build `impl <name> { fn eq(self, other: <name>) -> bool { <body> } }`.
fn synthesize_eq_impl(name: &str, fields: &[StructField], span: Span) -> Decl {
    let body = build_eq_body(name, fields, span);

    let method = Decl::Function {
        name: "eq".to_string(),
        generics: Vec::<GenericParam>::new(),
        params: vec![
            Param {
                name: "self".to_string(),
                ty: None,
                default: None,
                span,
            },
            Param {
                name: "other".to_string(),
                ty: Some(TypeExpr::Simple {
                    name: name.to_string(),
                    span,
                }),
                default: None,
                span,
            },
        ],
        ret_ty: Some(TypeExpr::Simple {
            name: "bool".to_string(),
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

/// Build the body of the synthesised `eq`:
///   `return self.f1 == other.f1 and self.f2 == other.f2 and ...`
/// For a fieldless struct, returns `true`.
fn build_eq_body(_name: &str, fields: &[StructField], span: Span) -> Block {
    let expr: Expr = if fields.is_empty() {
        Expr::BoolLiteral { value: true, span }
    } else {
        let mut iter = fields.iter().map(|f| field_eq(&f.name, span));
        let mut acc = iter.next().expect("non-empty");
        for next in iter {
            acc = Expr::BinaryOp {
                op: BinOp::And,
                left: Box::new(acc),
                right: Box::new(next),
                span,
            };
        }
        acc
    };
    Block {
        stmts: vec![Stmt::Return {
            value: Some(expr),
            span,
        }],
        span,
    }
}

/// `self.<field> == other.<field>`
fn field_eq(field: &str, span: Span) -> Expr {
    let self_field = Expr::FieldAccess {
        object: Box::new(Expr::Identifier {
            name: "self".to_string(),
            span,
        }),
        field: field.to_string(),
        span,
    };
    let other_field = Expr::FieldAccess {
        object: Box::new(Expr::Identifier {
            name: "other".to_string(),
            span,
        }),
        field: field.to_string(),
        span,
    };
    Expr::BinaryOp {
        op: BinOp::Eq,
        left: Box::new(self_field),
        right: Box::new(other_field),
        span,
    }
}

/// Build `impl <name> { fn hash(self) -> i64 { <body> } }`.
fn synthesize_hash_impl(name: &str, fields: &[StructField], span: Span) -> Decl {
    let body = build_hash_body(fields, span);

    let method = Decl::Function {
        name: "hash".to_string(),
        generics: Vec::<GenericParam>::new(),
        params: vec![Param {
            name: "self".to_string(),
            ty: None,
            default: None,
            span,
        }],
        ret_ty: Some(TypeExpr::Simple {
            name: "i64".to_string(),
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

/// Build the body of the synthesised `hash`:
///   `return ((seed * 1099511628211) + h(self.f1)) * 1099511628211 + h(self.f2) + ...`
/// Hashable-primitive fields contribute their numeric value (cast to i64
/// where needed); non-primitive fields contribute 0.
fn build_hash_body(fields: &[StructField], span: Span) -> Block {
    let fnv_prime: i64 = 1099511628211;
    let fnv_offset: i64 = -3750763034362895579; // FNV-1a 64-bit offset basis as i64

    let mut expr: Expr = Expr::IntLiteral {
        value: fnv_offset,
        span,
    };
    for field in fields {
        // expr = (expr * fnv_prime) + field_hash
        let mul = Expr::BinaryOp {
            op: BinOp::Mul,
            left: Box::new(expr),
            right: Box::new(Expr::IntLiteral {
                value: fnv_prime,
                span,
            }),
            span,
        };
        expr = Expr::BinaryOp {
            op: BinOp::Add,
            left: Box::new(mul),
            right: Box::new(field_hash(field, span)),
            span,
        };
    }
    Block {
        stmts: vec![Stmt::Return {
            value: Some(expr),
            span,
        }],
        span,
    }
}

/// Hash contribution for a single field. Numeric primitives are passed
/// through; bool maps to 1/0 via a cast; everything else falls back to 0.
fn field_hash(field: &StructField, span: Span) -> Expr {
    let access = Expr::FieldAccess {
        object: Box::new(Expr::Identifier {
            name: "self".to_string(),
            span,
        }),
        field: field.name.clone(),
        span,
    };
    match &field.ty {
        TypeExpr::Simple { name, .. }
            if matches!(
                name.as_str(),
                "i8" | "i16"
                    | "i32"
                    | "i64"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "char"
                    | "bool"
            ) =>
        {
            // Cast to i64 so the fold expression stays i64-typed end-to-end.
            Expr::Cast {
                expr: Box::new(access),
                ty: TypeExpr::Simple {
                    name: "i64".to_string(),
                    span,
                },
                span,
            }
        }
        _ => Expr::IntLiteral { value: 0, span },
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
    fn derive_eq_synthesizes_impl() {
        let mut m = Module {
            name: "t".to_string(),
            declarations: vec![make_struct(
                "Point",
                vec![("x", "i64"), ("y", "i64")],
                vec!["Eq"],
            )],
            span: dummy_span(),
        };
        expand_derives(&mut m);
        assert_eq!(m.declarations.len(), 2, "should have appended an Impl");
        let Decl::Impl { target, methods, .. } = &m.declarations[1] else {
            panic!("expected impl, got {:?}", m.declarations[1]);
        };
        assert_eq!(target, "Point");
        let Decl::Function {
            name: mname,
            params,
            ..
        } = &methods[0]
        else {
            panic!("expected function method");
        };
        assert_eq!(mname, "eq");
        assert_eq!(params.len(), 2, "`eq` should have (self, other)");
        assert_eq!(params[0].name, "self");
        assert_eq!(params[1].name, "other");
    }

    #[test]
    fn derive_hash_synthesizes_impl() {
        let mut m = Module {
            name: "t".to_string(),
            declarations: vec![make_struct(
                "Point",
                vec![("x", "i64"), ("y", "i64")],
                vec!["Hash"],
            )],
            span: dummy_span(),
        };
        expand_derives(&mut m);
        assert_eq!(m.declarations.len(), 2, "should have appended an Impl");
        let Decl::Impl { target, methods, .. } = &m.declarations[1] else {
            panic!("expected impl, got {:?}", m.declarations[1]);
        };
        assert_eq!(target, "Point");
        let Decl::Function {
            name: mname,
            params,
            ret_ty,
            ..
        } = &methods[0]
        else {
            panic!("expected function method");
        };
        assert_eq!(mname, "hash");
        assert_eq!(params.len(), 1, "`hash` should have only (self,)");
        assert!(matches!(
            ret_ty,
            Some(TypeExpr::Simple { name, .. }) if name == "i64"
        ));
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
