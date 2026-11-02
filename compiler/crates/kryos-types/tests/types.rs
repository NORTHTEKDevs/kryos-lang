//! Integration tests for the kryos-types crate.
#![allow(clippy::approx_constant)]

use kryos_ast::*;
use kryos_errors::Span;
use kryos_types::check::{type_check, TypeChecker};
use kryos_types::infer::InferenceEngine;
use kryos_types::ty::Type;

/// Dummy span for test AST nodes.
const S: Span = Span::DUMMY;

// ── Type from AST resolution ─────────────────────────────────────────

#[test]
fn resolve_simple_type_i32() {
    let mut checker = TypeChecker::new();
    let te = TypeExpr::Simple {
        name: "i32".to_string(),
        span: S,
    };
    let ty = checker.resolve_type_expr(&te);
    assert_eq!(ty, Type::I32);
}

#[test]
fn resolve_generic_type_vec_i32() {
    let mut checker = TypeChecker::new();
    let te = TypeExpr::Generic {
        name: "Vec".to_string(),
        args: vec![TypeExpr::Simple {
            name: "i32".to_string(),
            span: S,
        }],
        span: S,
    };
    let ty = checker.resolve_type_expr(&te);
    assert_eq!(
        ty,
        Type::Struct {
            name: "Vec".to_string(),
            generics: vec![Type::I32],
        }
    );
}

#[test]
fn resolve_array_type_i32_10() {
    let mut checker = TypeChecker::new();
    let te = TypeExpr::Array {
        element: Box::new(TypeExpr::Simple {
            name: "i32".to_string(),
            span: S,
        }),
        size: Some(10),
        span: S,
    };
    let ty = checker.resolve_type_expr(&te);
    assert_eq!(
        ty,
        Type::Array {
            element: Box::new(Type::I32),
            size: Some(10),
        }
    );
}

// ── Unification tests ────────────────────────────────────────────────

#[test]
fn unify_i32_with_i32_succeeds() {
    let mut engine = InferenceEngine::new();
    let result = engine.unify(&Type::I32, &Type::I32, S);
    assert!(result.is_ok());
}

#[test]
fn unify_i32_with_f64_fails() {
    let mut engine = InferenceEngine::new();
    let result = engine.unify(&Type::I32, &Type::F64, S);
    assert!(result.is_err());
}

#[test]
fn unify_var_with_i32_resolves() {
    let mut engine = InferenceEngine::new();
    let var = engine.fresh_var(); // Var(0)
    let result = engine.unify(&var, &Type::I32, S);
    assert!(result.is_ok());
    let resolved = engine.resolve(&var);
    assert_eq!(resolved, Type::I32);
}

#[test]
fn unify_two_vars_chain() {
    let mut engine = InferenceEngine::new();
    let v0 = engine.fresh_var(); // Var(0)
    let v1 = engine.fresh_var(); // Var(1)
                                 // Unify Var(0) with Var(1).
    engine.unify(&v0, &v1, S).unwrap();
    // Unify Var(1) with i32.
    engine.unify(&v1, &Type::I32, S).unwrap();
    // Both should resolve to i32.
    assert_eq!(engine.resolve(&v0), Type::I32);
    assert_eq!(engine.resolve(&v1), Type::I32);
}

// ── Inference from literals ──────────────────────────────────────────

/// Helper: directly infer a let-binding and check the variable's type.
/// Uses the TypeChecker's statement-level API in the global scope.
fn check_let_infer(name: &str, value: Expr) -> (Vec<kryos_errors::Diagnostic>, TypeChecker) {
    let mut checker = TypeChecker::new();
    let stmt = Stmt::Let {
        name: name.to_string(),
        mutable: false,
        ty: None,
        value: Some(value),
        pattern: None,
        span: S,
    };
    // Check the statement directly in the global scope (no function wrapper).
    checker.check_block(&Block {
        stmts: vec![stmt],
        span: S,
    });
    let diags = checker.diagnostics.clone();
    (diags, checker)
}

#[test]
fn infer_let_int_literal() {
    let (diags, checker) = check_let_infer("x", Expr::IntLiteral { value: 42, span: S });
    assert!(diags.is_empty(), "unexpected errors: {diags:?}");
    let ty = checker.env.lookup_var("x");
    assert_eq!(ty, Some(&Type::I64));
}

#[test]
fn infer_let_float_literal() {
    let (diags, checker) = check_let_infer(
        "x",
        Expr::FloatLiteral {
            value: 3.14,
            span: S,
        },
    );
    assert!(diags.is_empty(), "unexpected errors: {diags:?}");
    let ty = checker.env.lookup_var("x");
    assert_eq!(ty, Some(&Type::F64));
}

#[test]
fn infer_let_string_literal() {
    let (diags, checker) = check_let_infer(
        "x",
        Expr::StringLiteral {
            value: "hello".to_string(),
            span: S,
        },
    );
    assert!(diags.is_empty(), "unexpected errors: {diags:?}");
    let ty = checker.env.lookup_var("x");
    assert_eq!(ty, Some(&Type::Str));
}

#[test]
fn infer_let_bool_literal() {
    let (diags, checker) = check_let_infer(
        "x",
        Expr::BoolLiteral {
            value: true,
            span: S,
        },
    );
    assert!(diags.is_empty(), "unexpected errors: {diags:?}");
    let ty = checker.env.lookup_var("x");
    assert_eq!(ty, Some(&Type::Bool));
}

// ── Function type checking ───────────────────────────────────────────

/// Helper: build a module from declarations and type-check.
fn check_module(decls: Vec<Decl>) -> Vec<kryos_errors::Diagnostic> {
    let module = Module {
        name: "test".to_string(),
        declarations: decls,
        span: S,
    };
    type_check(&module)
}

#[test]
fn check_valid_add_function() {
    // fn add(a: i32, b: i32) -> i32 { return a + b }
    let diags = check_module(vec![Decl::Function {
        name: "add".to_string(),
        generics: vec![],
        params: vec![
            Param {
                name: "a".to_string(),
                ty: Some(TypeExpr::Simple {
                    name: "i32".to_string(),
                    span: S,
                }),
                default: None,
                span: S,
            },
            Param {
                name: "b".to_string(),
                ty: Some(TypeExpr::Simple {
                    name: "i32".to_string(),
                    span: S,
                }),
                default: None,
                span: S,
            },
        ],
        ret_ty: Some(TypeExpr::Simple {
            name: "i32".to_string(),
            span: S,
        }),
        body: Some(Block {
            stmts: vec![Stmt::Return {
                value: Some(Expr::BinaryOp {
                    op: BinOp::Add,
                    left: Box::new(Expr::Identifier {
                        name: "a".to_string(),
                        span: S,
                    }),
                    right: Box::new(Expr::Identifier {
                        name: "b".to_string(),
                        span: S,
                    }),
                    span: S,
                }),
                span: S,
            }],
            span: S,
        }),
        public: false,
        is_async: false,
        annotations: vec![],
        doc_comments: vec![],
        span: S,
    }]);
    assert!(diags.is_empty(), "expected no errors, got: {diags:?}");
}

// ── tracked-guard: must-use lint for Tracked<T> (W0400) ──────────────

/// `struct Tracked { value: i64 }` decl for the must-use tests. The lint keys
/// on the struct name, so a non-generic stand-in exercises the same path as the
/// real generic stdlib `Tracked<T>` without depending on generic inference.
fn tracked_struct_decl() -> Decl {
    Decl::Struct {
        name: "Tracked".to_string(),
        generics: vec![],
        fields: vec![StructField {
            name: "value".to_string(),
            ty: TypeExpr::Simple {
                name: "i64".to_string(),
                span: S,
            },
            public: true,
            default: None,
            span: S,
        }],
        public: true,
        annotations: vec![],
        doc_comments: vec![],
        span: S,
    }
}

fn tracked_literal() -> Expr {
    Expr::StructLiteral {
        name: "Tracked".to_string(),
        fields: vec![("value".to_string(), Expr::IntLiteral { value: 1, span: S })],
        span: S,
    }
}

#[test]
fn tracked_value_discarded_warns_w0400() {
    // fn drop_it() { Tracked { value: 1 } }   <- result discarded -> W0400
    let diags = check_module(vec![
        tracked_struct_decl(),
        Decl::Function {
            name: "drop_it".to_string(),
            generics: vec![],
            params: vec![],
            ret_ty: Some(TypeExpr::Simple {
                name: "void".to_string(),
                span: S,
            }),
            body: Some(Block {
                stmts: vec![Stmt::Expr {
                    expr: tracked_literal(),
                    span: S,
                }],
                span: S,
            }),
            public: false,
            is_async: false,
            annotations: vec![],
            doc_comments: vec![],
            span: S,
        },
    ]);
    assert!(
        diags.iter().any(|d| d.code.as_deref() == Some("W0400")),
        "expected a W0400 must-use warning for the discarded Tracked value, got: {diags:?}"
    );
}

#[test]
fn tracked_value_returned_no_warn() {
    // fn keep_it() -> Tracked { return Tracked { value: 1 } }  <- returned, not discarded
    let diags = check_module(vec![
        tracked_struct_decl(),
        Decl::Function {
            name: "keep_it".to_string(),
            generics: vec![],
            params: vec![],
            ret_ty: Some(TypeExpr::Simple {
                name: "Tracked".to_string(),
                span: S,
            }),
            body: Some(Block {
                stmts: vec![Stmt::Return {
                    value: Some(tracked_literal()),
                    span: S,
                }],
                span: S,
            }),
            public: false,
            is_async: false,
            annotations: vec![],
            doc_comments: vec![],
            span: S,
        },
    ]);
    assert!(
        !diags.iter().any(|d| d.code.as_deref() == Some("W0400")),
        "returned Tracked value must NOT trigger the must-use lint, got: {diags:?}"
    );
}

#[test]
fn tracked_value_block_tail_no_warn() {
    // fn block_tail() -> Tracked {
    //     let r = { Tracked { value: 1 } }   <- block tail is the block's VALUE, not a discard
    //     return r
    // }
    let diags = check_module(vec![
        tracked_struct_decl(),
        Decl::Function {
            name: "block_tail".to_string(),
            generics: vec![],
            params: vec![],
            ret_ty: Some(TypeExpr::Simple {
                name: "Tracked".to_string(),
                span: S,
            }),
            body: Some(Block {
                stmts: vec![
                    Stmt::Let {
                        name: "r".to_string(),
                        mutable: false,
                        ty: None,
                        value: Some(Expr::Block {
                            block: Block {
                                stmts: vec![Stmt::Expr {
                                    expr: tracked_literal(),
                                    span: S,
                                }],
                                span: S,
                            },
                            span: S,
                        }),
                        span: S,
                        pattern: None,
                    },
                    Stmt::Return {
                        value: Some(Expr::Identifier {
                            name: "r".to_string(),
                            span: S,
                        }),
                        span: S,
                    },
                ],
                span: S,
            }),
            public: false,
            is_async: false,
            annotations: vec![],
            doc_comments: vec![],
            span: S,
        },
    ]);
    assert!(
        !diags.iter().any(|d| d.code.as_deref() == Some("W0400")),
        "Tracked as a value-position block tail must NOT trigger the must-use lint, got: {diags:?}"
    );
}

#[test]
fn check_return_type_mismatch() {
    // fn bad() -> i32 { return "hello" }
    let diags = check_module(vec![Decl::Function {
        name: "bad".to_string(),
        generics: vec![],
        params: vec![],
        ret_ty: Some(TypeExpr::Simple {
            name: "i32".to_string(),
            span: S,
        }),
        body: Some(Block {
            stmts: vec![Stmt::Return {
                value: Some(Expr::StringLiteral {
                    value: "hello".to_string(),
                    span: S,
                }),
                span: S,
            }],
            span: S,
        }),
        public: false,
        is_async: false,
        annotations: vec![],
        doc_comments: vec![],
        span: S,
    }]);
    assert!(!diags.is_empty(), "expected type error for return mismatch");
    let msg = &diags[0].message;
    assert!(
        msg.contains("type mismatch")
            || (msg.contains("return") && msg.contains("i32") && msg.contains("str")),
        "unexpected diag message: {msg}"
    );
}

#[test]
fn check_binary_op_int_plus_float_error() {
    // 1 + 2.0 should be a type error (int + float without cast)
    let diags = check_module(vec![Decl::Function {
        name: "test".to_string(),
        generics: vec![],
        params: vec![],
        ret_ty: Some(TypeExpr::Simple {
            name: "void".to_string(),
            span: S,
        }),
        body: Some(Block {
            stmts: vec![Stmt::Expr {
                expr: Expr::BinaryOp {
                    op: BinOp::Add,
                    left: Box::new(Expr::IntLiteral { value: 1, span: S }),
                    right: Box::new(Expr::FloatLiteral {
                        value: 2.0,
                        span: S,
                    }),
                    span: S,
                },
                span: S,
            }],
            span: S,
        }),
        public: false,
        is_async: false,
        annotations: vec![],
        doc_comments: vec![],
        span: S,
    }]);
    assert!(!diags.is_empty(), "expected type error for int + float");
}

#[test]
fn check_binary_op_int_plus_int() {
    // 1 + 2 → i32 (no error)
    let module = Module {
        name: "test".to_string(),
        declarations: vec![Decl::Function {
            name: "test".to_string(),
            generics: vec![],
            params: vec![],
            ret_ty: Some(TypeExpr::Simple {
                name: "i32".to_string(),
                span: S,
            }),
            body: Some(Block {
                stmts: vec![Stmt::Return {
                    value: Some(Expr::BinaryOp {
                        op: BinOp::Add,
                        left: Box::new(Expr::IntLiteral { value: 1, span: S }),
                        right: Box::new(Expr::IntLiteral { value: 2, span: S }),
                        span: S,
                    }),
                    span: S,
                }],
                span: S,
            }),
            public: false,
            is_async: false,
            annotations: vec![],
            doc_comments: vec![],
            span: S,
        }],
        span: S,
    };
    let mut checker = TypeChecker::new();
    checker.check_module(&module);
    assert!(
        checker.diagnostics.is_empty(),
        "expected no errors for int + int: {:?}",
        checker.diagnostics
    );
}

#[test]
fn check_comparison_returns_bool() {
    // 1 < 2 → bool
    let module = Module {
        name: "test".to_string(),
        declarations: vec![Decl::Function {
            name: "test".to_string(),
            generics: vec![],
            params: vec![],
            ret_ty: Some(TypeExpr::Simple {
                name: "bool".to_string(),
                span: S,
            }),
            body: Some(Block {
                stmts: vec![Stmt::Return {
                    value: Some(Expr::BinaryOp {
                        op: BinOp::Lt,
                        left: Box::new(Expr::IntLiteral { value: 1, span: S }),
                        right: Box::new(Expr::IntLiteral { value: 2, span: S }),
                        span: S,
                    }),
                    span: S,
                }],
                span: S,
            }),
            public: false,
            is_async: false,
            annotations: vec![],
            doc_comments: vec![],
            span: S,
        }],
        span: S,
    };
    let mut checker = TypeChecker::new();
    checker.check_module(&module);
    assert!(
        checker.diagnostics.is_empty(),
        "expected no errors for comparison returning bool: {:?}",
        checker.diagnostics
    );
}

#[test]
fn check_shared_expr_type() {
    // shared x where x: i32 → Shared<i32>
    let mut checker = TypeChecker::new();

    // Directly define x in global scope and infer the shared expression.
    checker.env.define_var("x".to_string(), Type::I32);
    let ty = checker.infer_expr(&Expr::SharedExpr {
        inner: Box::new(Expr::Identifier {
            name: "x".to_string(),
            span: S,
        }),
        span: S,
    });

    assert!(
        checker.diagnostics.is_empty(),
        "expected no errors: {:?}",
        checker.diagnostics
    );
    assert_eq!(
        ty,
        Type::Shared {
            inner: Box::new(Type::I32),
        }
    );
}

#[test]
fn check_shared_expr_vec_type() {
    // shared x where x: Vec<i32> → Shared<Vec<i32>>
    let mut checker = TypeChecker::new();

    let vec_i32 = Type::Struct {
        name: "Vec".to_string(),
        generics: vec![Type::I32],
    };
    checker.env.define_var("x".to_string(), vec_i32.clone());

    let ty = checker.infer_expr(&Expr::SharedExpr {
        inner: Box::new(Expr::Identifier {
            name: "x".to_string(),
            span: S,
        }),
        span: S,
    });

    assert!(
        checker.diagnostics.is_empty(),
        "expected no errors: {:?}",
        checker.diagnostics
    );
    assert_eq!(
        ty,
        Type::Shared {
            inner: Box::new(vec_i32),
        }
    );
}

// ── Additional type resolution tests ─────────────────────────────────

#[test]
fn resolve_tuple_type() {
    let mut checker = TypeChecker::new();
    let te = TypeExpr::Tuple {
        elements: vec![
            TypeExpr::Simple {
                name: "i32".to_string(),
                span: S,
            },
            TypeExpr::Simple {
                name: "bool".to_string(),
                span: S,
            },
        ],
        span: S,
    };
    let ty = checker.resolve_type_expr(&te);
    assert_eq!(
        ty,
        Type::Tuple {
            elements: vec![Type::I32, Type::Bool],
        }
    );
}

#[test]
fn resolve_function_type() {
    let mut checker = TypeChecker::new();
    let te = TypeExpr::Function {
        params: vec![TypeExpr::Simple {
            name: "i32".to_string(),
            span: S,
        }],
        ret: Box::new(TypeExpr::Simple {
            name: "bool".to_string(),
            span: S,
        }),
        span: S,
    };
    let ty = checker.resolve_type_expr(&te);
    // NOTE: not a full assert_eq! against a hand-built `Type::Function` —
    // an un-annotated fn-typed position now carries a fresh capability-row
    // variable (`CapRow::var(id)`) whose exact id is allocator-order-
    // dependent, not a fixed value to hardcode. Assert the structural shape
    // (params/ret) and that the row was in fact left OPEN (inferred, not
    // silently defaulted to a closed empty set).
    match ty {
        Type::Function { params, ret, caps } => {
            assert_eq!(params, vec![Type::I32]);
            assert_eq!(*ret, Type::Bool);
            assert!(
                !caps.is_closed(),
                "an un-annotated fn-typed position must get an open row variable, not a closed default"
            );
        }
        other => panic!("expected Type::Function, got {other:?}"),
    }
}

#[test]
fn resolve_option_type() {
    let mut checker = TypeChecker::new();
    let te = TypeExpr::Optional {
        inner: Box::new(TypeExpr::Simple {
            name: "i32".to_string(),
            span: S,
        }),
        span: S,
    };
    let ty = checker.resolve_type_expr(&te);
    assert_eq!(
        ty,
        Type::Option {
            inner: Box::new(Type::I32),
        }
    );
}

#[test]
fn resolve_reference_type() {
    let mut checker = TypeChecker::new();
    let te = TypeExpr::Reference {
        inner: Box::new(TypeExpr::Simple {
            name: "i32".to_string(),
            span: S,
        }),
        mutable: true,
        span: S,
    };
    let ty = checker.resolve_type_expr(&te);
    assert_eq!(
        ty,
        Type::Reference {
            inner: Box::new(Type::I32),
            mutable: true,
        }
    );
}

#[test]
fn resolve_shared_type() {
    let mut checker = TypeChecker::new();
    let te = TypeExpr::Shared {
        inner: Box::new(TypeExpr::Simple {
            name: "i32".to_string(),
            span: S,
        }),
        span: S,
    };
    let ty = checker.resolve_type_expr(&te);
    assert_eq!(
        ty,
        Type::Shared {
            inner: Box::new(Type::I32),
        }
    );
}

#[test]
fn resolve_weak_type() {
    let mut checker = TypeChecker::new();
    let te = TypeExpr::Weak {
        inner: Box::new(TypeExpr::Simple {
            name: "i32".to_string(),
            span: S,
        }),
        span: S,
    };
    let ty = checker.resolve_type_expr(&te);
    assert_eq!(
        ty,
        Type::Weak {
            inner: Box::new(Type::I32),
        }
    );
}

#[test]
fn resolve_pointer_type() {
    let mut checker = TypeChecker::new();
    let te = TypeExpr::Pointer {
        inner: Box::new(TypeExpr::Simple {
            name: "u8".to_string(),
            span: S,
        }),
        mutable: false,
        span: S,
    };
    let ty = checker.resolve_type_expr(&te);
    assert_eq!(
        ty,
        Type::Pointer {
            inner: Box::new(Type::U8),
            mutable: false,
        }
    );
}

// ── Type display tests ───────────────────────────────────────────────

#[test]
fn type_display_formatting() {
    assert_eq!(format!("{}", Type::I32), "i32");
    assert_eq!(format!("{}", Type::Bool), "bool");
    assert_eq!(format!("{}", Type::Str), "str");
    assert_eq!(
        format!(
            "{}",
            Type::Array {
                element: Box::new(Type::I32),
                size: Some(10)
            }
        ),
        "[i32; 10]"
    );
    assert_eq!(
        format!(
            "{}",
            Type::Shared {
                inner: Box::new(Type::I32)
            }
        ),
        "shared i32"
    );
    assert_eq!(format!("{}", Type::Never), "!");
    assert_eq!(format!("{}", Type::Var(0)), "?T0");
}

// ── Type helper method tests ─────────────────────────────────────────

#[test]
fn type_is_numeric() {
    assert!(Type::I32.is_integer());
    assert!(Type::U64.is_integer());
    assert!(Type::F64.is_float());
    assert!(Type::I32.is_numeric());
    assert!(Type::F64.is_numeric());
    assert!(!Type::Bool.is_numeric());
    assert!(!Type::Str.is_numeric());
}

#[test]
fn type_has_vars() {
    assert!(Type::Var(0).has_vars());
    assert!(Type::Array {
        element: Box::new(Type::Var(1)),
        size: None,
    }
    .has_vars());
    assert!(!Type::I32.has_vars());
    assert!(!Type::Array {
        element: Box::new(Type::I32),
        size: Some(5),
    }
    .has_vars());
}

// ── Environment tests ────────────────────────────────────────────────

#[test]
fn env_scope_shadowing() {
    use kryos_types::TypeEnv;

    let mut env = TypeEnv::new();
    env.define_var("x".to_string(), Type::I32);
    assert_eq!(env.lookup_var("x"), Some(&Type::I32));

    env.push_scope();
    env.define_var("x".to_string(), Type::F64);
    assert_eq!(env.lookup_var("x"), Some(&Type::F64));

    env.pop_scope();
    assert_eq!(env.lookup_var("x"), Some(&Type::I32));
}

#[test]
fn env_function_lookup() {
    use kryos_types::env::FunctionSig;
    use kryos_types::TypeEnv;

    let mut env = TypeEnv::new();
    env.define_function(FunctionSig {
        name: "add".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        params: vec![("a".to_string(), Type::I32), ("b".to_string(), Type::I32)],
        ret: Type::I32,
        generic_cap_var_ids: vec![],
        own_cap_var: 0,
    });

    let sig = env.lookup_function("add").unwrap();
    assert_eq!(sig.name, "add");
    assert_eq!(sig.ret, Type::I32);
    assert_eq!(sig.params.len(), 2);
}

// ── Error recovery tests ─────────────────────────────────────────────

#[test]
fn error_type_unifies_with_anything() {
    let mut engine = InferenceEngine::new();
    assert!(engine.unify(&Type::Error, &Type::I32, S).is_ok());
    assert!(engine.unify(&Type::F64, &Type::Error, S).is_ok());
    assert!(engine.unify(&Type::Error, &Type::Error, S).is_ok());
}

#[test]
fn never_type_unifies_with_anything() {
    let mut engine = InferenceEngine::new();
    assert!(engine.unify(&Type::Never, &Type::I32, S).is_ok());
    assert!(engine.unify(&Type::Str, &Type::Never, S).is_ok());
}

// ── Attribute enforcement ────────────────────────────────────────────

#[test]
fn deprecated_function_emits_warning() {
    use kryos_errors::Level;

    let module = Module {
        name: "test".into(),
        declarations: vec![
            // @deprecated fn old_fn() -> i64 { return 1 }
            Decl::Function {
                name: "old_fn".into(),
                generics: vec![],
                params: vec![],
                ret_ty: Some(TypeExpr::Simple {
                    name: "i64".into(),
                    span: S,
                }),
                body: Some(Block {
                    stmts: vec![Stmt::Return {
                        value: Some(Expr::IntLiteral { value: 1, span: S }),
                        span: S,
                    }],
                    span: S,
                }),
                annotations: vec![kryos_ast::Annotation {
                    name: "deprecated".into(),
                    args: vec![],
                    span: S,
                }],
                public: false,
                is_async: false,
                doc_comments: vec![],
                span: S,
            },
            // fn main() -> i64 { return old_fn() }
            Decl::Function {
                name: "main".into(),
                generics: vec![],
                params: vec![],
                ret_ty: Some(TypeExpr::Simple {
                    name: "i64".into(),
                    span: S,
                }),
                body: Some(Block {
                    stmts: vec![Stmt::Return {
                        value: Some(Expr::FnCall {
                            callee: Box::new(Expr::Identifier {
                                name: "old_fn".into(),
                                span: S,
                            }),
                            args: vec![],
                            span: S,
                        }),
                        span: S,
                    }],
                    span: S,
                }),
                annotations: vec![],
                public: false,
                is_async: false,
                doc_comments: vec![],
                span: S,
            },
        ],
        span: S,
    };

    let diags = type_check(&module);
    let warnings: Vec<_> = diags.iter().filter(|d| d.level == Level::Warning).collect();
    assert!(!warnings.is_empty(), "expected a deprecation warning");
    assert!(
        warnings.iter().any(|d| d.message.contains("deprecated")),
        "warning should mention 'deprecated', got: {:?}",
        warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn pure_function_rejects_io_call() {
    use kryos_errors::Level;

    let module = Module {
        name: "test".into(),
        declarations: vec![
            // @pure fn bad(x: i64) -> i64 { println(x); return x }
            Decl::Function {
                name: "bad".into(),
                generics: vec![],
                params: vec![Param {
                    name: "x".into(),
                    ty: Some(TypeExpr::Simple {
                        name: "i64".into(),
                        span: S,
                    }),
                    default: None,
                    span: S,
                }],
                ret_ty: Some(TypeExpr::Simple {
                    name: "i64".into(),
                    span: S,
                }),
                body: Some(Block {
                    stmts: vec![
                        Stmt::Expr {
                            expr: Expr::FnCall {
                                callee: Box::new(Expr::Identifier {
                                    name: "println".into(),
                                    span: S,
                                }),
                                args: vec![Expr::Identifier {
                                    name: "x".into(),
                                    span: S,
                                }],
                                span: S,
                            },
                            span: S,
                        },
                        Stmt::Return {
                            value: Some(Expr::Identifier {
                                name: "x".into(),
                                span: S,
                            }),
                            span: S,
                        },
                    ],
                    span: S,
                }),
                annotations: vec![kryos_ast::Annotation {
                    name: "pure".into(),
                    args: vec![],
                    span: S,
                }],
                public: false,
                is_async: false,
                doc_comments: vec![],
                span: S,
            },
        ],
        span: S,
    };

    let diags = type_check(&module);
    let errors: Vec<_> = diags.iter().filter(|d| d.level == Level::Error).collect();
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("@pure") && d.message.contains("println")),
        "expected error about @pure calling println, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn pure_function_allows_pure_calls() {
    use kryos_errors::Level;

    let module = Module {
        name: "test".into(),
        declarations: vec![
            // @pure fn add(a: i64, b: i64) -> i64 { return a + b }
            Decl::Function {
                name: "add".into(),
                generics: vec![],
                params: vec![
                    Param {
                        name: "a".into(),
                        ty: Some(TypeExpr::Simple {
                            name: "i64".into(),
                            span: S,
                        }),
                        default: None,
                        span: S,
                    },
                    Param {
                        name: "b".into(),
                        ty: Some(TypeExpr::Simple {
                            name: "i64".into(),
                            span: S,
                        }),
                        default: None,
                        span: S,
                    },
                ],
                ret_ty: Some(TypeExpr::Simple {
                    name: "i64".into(),
                    span: S,
                }),
                body: Some(Block {
                    stmts: vec![Stmt::Return {
                        value: Some(Expr::BinaryOp {
                            op: BinOp::Add,
                            left: Box::new(Expr::Identifier {
                                name: "a".into(),
                                span: S,
                            }),
                            right: Box::new(Expr::Identifier {
                                name: "b".into(),
                                span: S,
                            }),
                            span: S,
                        }),
                        span: S,
                    }],
                    span: S,
                }),
                annotations: vec![kryos_ast::Annotation {
                    name: "pure".into(),
                    args: vec![],
                    span: S,
                }],
                public: false,
                is_async: false,
                doc_comments: vec![],
                span: S,
            },
            // @pure fn double(x: i64) -> i64 { return add(x, x) }
            Decl::Function {
                name: "double".into(),
                generics: vec![],
                params: vec![Param {
                    name: "x".into(),
                    ty: Some(TypeExpr::Simple {
                        name: "i64".into(),
                        span: S,
                    }),
                    default: None,
                    span: S,
                }],
                ret_ty: Some(TypeExpr::Simple {
                    name: "i64".into(),
                    span: S,
                }),
                body: Some(Block {
                    stmts: vec![Stmt::Return {
                        value: Some(Expr::FnCall {
                            callee: Box::new(Expr::Identifier {
                                name: "add".into(),
                                span: S,
                            }),
                            args: vec![
                                Expr::Identifier {
                                    name: "x".into(),
                                    span: S,
                                },
                                Expr::Identifier {
                                    name: "x".into(),
                                    span: S,
                                },
                            ],
                            span: S,
                        }),
                        span: S,
                    }],
                    span: S,
                }),
                annotations: vec![kryos_ast::Annotation {
                    name: "pure".into(),
                    args: vec![],
                    span: S,
                }],
                public: false,
                is_async: false,
                doc_comments: vec![],
                span: S,
            },
        ],
        span: S,
    };

    let diags = type_check(&module);
    let errors: Vec<_> = diags.iter().filter(|d| d.level == Level::Error).collect();
    assert!(
        errors.is_empty(),
        "pure function calling another pure function should be allowed, got errors: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ── Trait impl self-type resolution ─────────────────────────────────

#[test]
fn multiple_trait_impls_self_resolves_correctly() {
    // Regression test: when two structs implement the same trait, `self` in
    // earlier impl blocks must resolve to its own struct, not the last struct.
    //
    //   trait Area { fn area(self: Self) -> i64 }
    //   struct Circle { radius: i64 }
    //   impl Area for Circle { fn area(self: Circle) -> i64 { return self.radius * self.radius * 3 } }
    //   struct Rect { w: i64, h: i64 }
    //   impl Area for Rect { fn area(self: Rect) -> i64 { return self.w * self.h } }
    //   fn main() { let c = Circle { radius: 5 }; c.area() }
    let diags = check_module(vec![
        // trait Area { fn area(self) -> i64 }
        Decl::Trait {
            name: "Area".into(),
            generics: vec![],
            methods: vec![Decl::Function {
                name: "area".into(),
                generics: vec![],
                params: vec![Param {
                    name: "self".into(),
                    ty: None,
                    default: None,
                    span: S,
                }],
                ret_ty: Some(TypeExpr::Simple {
                    name: "i64".into(),
                    span: S,
                }),
                body: None,
                public: false,
                is_async: false,
                annotations: vec![],
                doc_comments: vec![],
                span: S,
            }],
            public: false,
            doc_comments: vec![],
            span: S,
        },
        // struct Circle { radius: i64 }
        Decl::Struct {
            name: "Circle".into(),
            generics: vec![],
            fields: vec![StructField {
                name: "radius".into(),
                ty: TypeExpr::Simple {
                    name: "i64".into(),
                    span: S,
                },
                public: false,
                default: None,
                span: S,
            }],
            public: false,
            annotations: vec![],
            doc_comments: vec![],
            span: S,
        },
        // impl Area for Circle { fn area(self: Circle) -> i64 { return self.radius * self.radius * 3 } }
        Decl::Impl {
            target: "Circle".into(),
            trait_name: Some("Area".into()),
            generics: vec![],
            methods: vec![Decl::Function {
                name: "area".into(),
                generics: vec![],
                params: vec![Param {
                    name: "self".into(),
                    ty: Some(TypeExpr::Simple {
                        name: "Circle".into(),
                        span: S,
                    }),
                    default: None,
                    span: S,
                }],
                ret_ty: Some(TypeExpr::Simple {
                    name: "i64".into(),
                    span: S,
                }),
                body: Some(Block {
                    stmts: vec![Stmt::Return {
                        value: Some(Expr::BinaryOp {
                            op: BinOp::Mul,
                            left: Box::new(Expr::BinaryOp {
                                op: BinOp::Mul,
                                left: Box::new(Expr::FieldAccess {
                                    object: Box::new(Expr::Identifier {
                                        name: "self".into(),
                                        span: S,
                                    }),
                                    field: "radius".into(),
                                    span: S,
                                }),
                                right: Box::new(Expr::FieldAccess {
                                    object: Box::new(Expr::Identifier {
                                        name: "self".into(),
                                        span: S,
                                    }),
                                    field: "radius".into(),
                                    span: S,
                                }),
                                span: S,
                            }),
                            right: Box::new(Expr::IntLiteral { value: 3, span: S }),
                            span: S,
                        }),
                        span: S,
                    }],
                    span: S,
                }),
                public: false,
                is_async: false,
                annotations: vec![],
                doc_comments: vec![],
                span: S,
            }],
            doc_comments: vec![],
            span: S,
        },
        // struct Rect { w: i64, h: i64 }
        Decl::Struct {
            name: "Rect".into(),
            generics: vec![],
            fields: vec![
                StructField {
                    name: "w".into(),
                    ty: TypeExpr::Simple {
                        name: "i64".into(),
                        span: S,
                    },
                    public: false,
                    default: None,
                    span: S,
                },
                StructField {
                    name: "h".into(),
                    ty: TypeExpr::Simple {
                        name: "i64".into(),
                        span: S,
                    },
                    public: false,
                    default: None,
                    span: S,
                },
            ],
            public: false,
            annotations: vec![],
            doc_comments: vec![],
            span: S,
        },
        // impl Area for Rect { fn area(self: Rect) -> i64 { return self.w * self.h } }
        Decl::Impl {
            target: "Rect".into(),
            trait_name: Some("Area".into()),
            generics: vec![],
            methods: vec![Decl::Function {
                name: "area".into(),
                generics: vec![],
                params: vec![Param {
                    name: "self".into(),
                    ty: Some(TypeExpr::Simple {
                        name: "Rect".into(),
                        span: S,
                    }),
                    default: None,
                    span: S,
                }],
                ret_ty: Some(TypeExpr::Simple {
                    name: "i64".into(),
                    span: S,
                }),
                body: Some(Block {
                    stmts: vec![Stmt::Return {
                        value: Some(Expr::BinaryOp {
                            op: BinOp::Mul,
                            left: Box::new(Expr::FieldAccess {
                                object: Box::new(Expr::Identifier {
                                    name: "self".into(),
                                    span: S,
                                }),
                                field: "w".into(),
                                span: S,
                            }),
                            right: Box::new(Expr::FieldAccess {
                                object: Box::new(Expr::Identifier {
                                    name: "self".into(),
                                    span: S,
                                }),
                                field: "h".into(),
                                span: S,
                            }),
                            span: S,
                        }),
                        span: S,
                    }],
                    span: S,
                }),
                public: false,
                is_async: false,
                annotations: vec![],
                doc_comments: vec![],
                span: S,
            }],
            doc_comments: vec![],
            span: S,
        },
        // fn main() { let c = Circle { radius: 5 }; c.area() }
        Decl::Function {
            name: "main".into(),
            generics: vec![],
            params: vec![],
            ret_ty: None,
            body: Some(Block {
                stmts: vec![
                    Stmt::Let {
                        name: "c".into(),
                        mutable: false,
                        ty: None,
                        value: Some(Expr::StructLiteral {
                            name: "Circle".into(),
                            fields: vec![("radius".into(), Expr::IntLiteral { value: 5, span: S })],
                            span: S,
                        }),
                        pattern: None,
                        span: S,
                    },
                    Stmt::Expr {
                        expr: Expr::MethodCall {
                            object: Box::new(Expr::Identifier {
                                name: "c".into(),
                                span: S,
                            }),
                            method: "area".into(),
                            args: vec![],
                            span: S,
                        },
                        span: S,
                    },
                ],
                span: S,
            }),
            public: false,
            is_async: false,
            annotations: vec![],
            doc_comments: vec![],
            span: S,
        },
    ]);
    assert!(
        diags.is_empty(),
        "expected no type errors when two structs implement the same trait, got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ── Pattern exhaustiveness ──────────────────────────────────────────

/// Helper: build a match expression inside a function and type-check.
fn check_match_exhaustive(subject: Expr, arms: Vec<MatchArm>) -> Vec<kryos_errors::Diagnostic> {
    let match_expr = Expr::MatchExpr {
        subject: Box::new(subject),
        arms,
        span: S,
    };
    // Wrap in a function body so type_check can process it.
    check_module(vec![Decl::Function {
        name: "test_fn".to_string(),
        generics: vec![],
        params: vec![],
        ret_ty: None,
        body: Some(Block {
            stmts: vec![Stmt::Expr {
                expr: match_expr,
                span: S,
            }],
            span: S,
        }),
        public: false,
        is_async: false,
        annotations: vec![],
        doc_comments: vec![],
        span: S,
    }])
}

#[test]
fn exhaustive_bool_complete() {
    use kryos_errors::Level;

    let diags = check_match_exhaustive(
        Expr::BoolLiteral {
            value: true,
            span: S,
        },
        vec![
            MatchArm {
                pattern: Pattern::Literal {
                    expr: Box::new(Expr::BoolLiteral {
                        value: true,
                        span: S,
                    }),
                    span: S,
                },
                guard: None,
                body: Box::new(Expr::IntLiteral { value: 1, span: S }),
                span: S,
            },
            MatchArm {
                pattern: Pattern::Literal {
                    expr: Box::new(Expr::BoolLiteral {
                        value: false,
                        span: S,
                    }),
                    span: S,
                },
                guard: None,
                body: Box::new(Expr::IntLiteral { value: 0, span: S }),
                span: S,
            },
        ],
    );
    let warnings: Vec<_> = diags.iter().filter(|d| d.level == Level::Warning).collect();
    assert!(
        warnings
            .iter()
            .all(|w| !w.message.contains("non-exhaustive")),
        "complete bool match should not warn, got: {:?}",
        warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn exhaustive_bool_missing_false() {
    use kryos_errors::Level;

    let diags = check_match_exhaustive(
        Expr::BoolLiteral {
            value: true,
            span: S,
        },
        vec![MatchArm {
            pattern: Pattern::Literal {
                expr: Box::new(Expr::BoolLiteral {
                    value: true,
                    span: S,
                }),
                span: S,
            },
            guard: None,
            body: Box::new(Expr::IntLiteral { value: 1, span: S }),
            span: S,
        }],
    );
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| d.level == Level::Error && d.message.contains("non-exhaustive"))
        .collect();
    assert!(!errors.is_empty(), "bool match missing false should error");
    assert!(
        errors[0].message.contains("false"),
        "error should mention `false`, got: {}",
        errors[0].message
    );
}

#[test]
fn exhaustive_enum_complete() {
    use kryos_errors::Level;

    // enum Color { Red, Green, Blue }
    // fn test_fn(c: Color) { match c { Color::Red => 1, ... } }
    let decls = vec![
        Decl::Enum {
            name: "Color".to_string(),
            generics: vec![],
            variants: vec![
                EnumVariant {
                    name: "Red".to_string(),
                    fields: vec![],
                    span: S,
                },
                EnumVariant {
                    name: "Green".to_string(),
                    fields: vec![],
                    span: S,
                },
                EnumVariant {
                    name: "Blue".to_string(),
                    fields: vec![],
                    span: S,
                },
            ],
            public: false,
            annotations: vec![],
            doc_comments: vec![],
            span: S,
        },
        Decl::Function {
            name: "test_fn".to_string(),
            generics: vec![],
            params: vec![Param {
                name: "c".to_string(),
                ty: Some(TypeExpr::Simple {
                    name: "Color".to_string(),
                    span: S,
                }),
                default: None,
                span: S,
            }],
            ret_ty: None,
            body: Some(Block {
                stmts: vec![Stmt::Expr {
                    expr: Expr::MatchExpr {
                        subject: Box::new(Expr::Identifier {
                            name: "c".to_string(),
                            span: S,
                        }),
                        arms: vec![
                            MatchArm {
                                pattern: Pattern::Enum {
                                    name: "Color".to_string(),
                                    variant: "Red".to_string(),
                                    fields: vec![],
                                    span: S,
                                },
                                guard: None,
                                body: Box::new(Expr::IntLiteral { value: 1, span: S }),
                                span: S,
                            },
                            MatchArm {
                                pattern: Pattern::Enum {
                                    name: "Color".to_string(),
                                    variant: "Green".to_string(),
                                    fields: vec![],
                                    span: S,
                                },
                                guard: None,
                                body: Box::new(Expr::IntLiteral { value: 2, span: S }),
                                span: S,
                            },
                            MatchArm {
                                pattern: Pattern::Enum {
                                    name: "Color".to_string(),
                                    variant: "Blue".to_string(),
                                    fields: vec![],
                                    span: S,
                                },
                                guard: None,
                                body: Box::new(Expr::IntLiteral { value: 3, span: S }),
                                span: S,
                            },
                        ],
                        span: S,
                    },
                    span: S,
                }],
                span: S,
            }),
            public: false,
            is_async: false,
            annotations: vec![],
            doc_comments: vec![],
            span: S,
        },
    ];
    let diags = check_module(decls);
    let warnings: Vec<_> = diags.iter().filter(|d| d.level == Level::Warning).collect();
    assert!(
        warnings
            .iter()
            .all(|w| !w.message.contains("non-exhaustive")),
        "complete enum match should not warn, got: {:?}",
        warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn exhaustive_enum_missing_variant() {
    use kryos_errors::Level;

    let decls = vec![
        Decl::Enum {
            name: "Color".to_string(),
            generics: vec![],
            variants: vec![
                EnumVariant {
                    name: "Red".to_string(),
                    fields: vec![],
                    span: S,
                },
                EnumVariant {
                    name: "Green".to_string(),
                    fields: vec![],
                    span: S,
                },
                EnumVariant {
                    name: "Blue".to_string(),
                    fields: vec![],
                    span: S,
                },
            ],
            public: false,
            annotations: vec![],
            doc_comments: vec![],
            span: S,
        },
        Decl::Function {
            name: "test_fn".to_string(),
            generics: vec![],
            params: vec![Param {
                name: "c".to_string(),
                ty: Some(TypeExpr::Simple {
                    name: "Color".to_string(),
                    span: S,
                }),
                default: None,
                span: S,
            }],
            ret_ty: None,
            body: Some(Block {
                stmts: vec![Stmt::Expr {
                    expr: Expr::MatchExpr {
                        subject: Box::new(Expr::Identifier {
                            name: "c".to_string(),
                            span: S,
                        }),
                        arms: vec![MatchArm {
                            pattern: Pattern::Enum {
                                name: "Color".to_string(),
                                variant: "Red".to_string(),
                                fields: vec![],
                                span: S,
                            },
                            guard: None,
                            body: Box::new(Expr::IntLiteral { value: 1, span: S }),
                            span: S,
                        }],
                        span: S,
                    },
                    span: S,
                }],
                span: S,
            }),
            public: false,
            is_async: false,
            annotations: vec![],
            doc_comments: vec![],
            span: S,
        },
    ];
    let diags = check_module(decls);
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| d.level == Level::Error && d.message.contains("non-exhaustive"))
        .collect();
    assert!(
        !errors.is_empty(),
        "enum match missing variants should error"
    );
    assert!(
        errors[0].message.contains("Blue") && errors[0].message.contains("Green"),
        "error should list missing variants, got: {}",
        errors[0].message
    );
}

#[test]
fn exhaustive_int_without_wildcard() {
    use kryos_errors::Level;

    let diags = check_match_exhaustive(
        Expr::IntLiteral { value: 42, span: S },
        vec![MatchArm {
            pattern: Pattern::Literal {
                expr: Box::new(Expr::IntLiteral { value: 42, span: S }),
                span: S,
            },
            guard: None,
            body: Box::new(Expr::IntLiteral { value: 1, span: S }),
            span: S,
        }],
    );
    let warnings: Vec<_> = diags
        .iter()
        .filter(|d| d.level == Level::Warning && d.message.contains("non-exhaustive"))
        .collect();
    assert!(
        !warnings.is_empty(),
        "integer match without wildcard should warn"
    );
}

#[test]
fn exhaustive_wildcard_covers_all() {
    use kryos_errors::Level;

    let diags = check_match_exhaustive(
        Expr::IntLiteral { value: 42, span: S },
        vec![
            MatchArm {
                pattern: Pattern::Literal {
                    expr: Box::new(Expr::IntLiteral { value: 42, span: S }),
                    span: S,
                },
                guard: None,
                body: Box::new(Expr::IntLiteral { value: 1, span: S }),
                span: S,
            },
            MatchArm {
                pattern: Pattern::Wildcard { span: S },
                guard: None,
                body: Box::new(Expr::IntLiteral { value: 0, span: S }),
                span: S,
            },
        ],
    );
    let warnings: Vec<_> = diags
        .iter()
        .filter(|d| d.level == Level::Warning && d.message.contains("non-exhaustive"))
        .collect();
    assert!(
        warnings.is_empty(),
        "wildcard should satisfy exhaustiveness, got: {:?}",
        warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ── Capability-row slots (LEDGER root R2) ────────────────────────────
//
// A closure stored into a slot must WIDEN the slot's row. Before R2 the first
// store bound the slot's row var and every later store met a resolved (closed)
// row, hit the closed/closed no-op in `unify`, and was dropped.

use kryos_types::ty::{CapBits, CapRow};

fn fn_with(caps: CapRow) -> Type {
    Type::Function {
        params: vec![],
        ret: Box::new(Type::Str),
        caps,
    }
}

fn bits_of(engine: &InferenceEngine, var: u32) -> CapBits {
    engine.resolve_cap_row(&CapRow::var(var)).concrete_bits()
}

#[test]
fn slot_row_widens_when_a_second_closure_is_stored() {
    let mut engine = InferenceEngine::new();
    let slot = engine.fresh_slot_cap_var();
    let slot_ty = fn_with(CapRow::var(slot));
    // First store: a pure closure binds the slot to the empty row.
    engine
        .unify(&slot_ty, &fn_with(CapRow::closed(CapBits::EMPTY)), S)
        .unwrap();
    assert_eq!(bits_of(&engine, slot), CapBits::EMPTY);
    // Second store: a privileged closure must reach the slot.
    engine
        .unify(&slot_ty, &fn_with(CapRow::closed(CapBits::FS_READ)), S)
        .unwrap();
    assert!(
        bits_of(&engine, slot).contains_bits(CapBits::FS_READ),
        "the second store was dropped: slot row is {}",
        engine.resolve_cap_row(&CapRow::var(slot)).display()
    );
    // The type carries the slot var, not a closed copy of what it held.
    match engine.resolve(&slot_ty) {
        Type::Function { caps, .. } => assert_eq!(caps, CapRow::var(slot)),
        other => panic!("expected a function type, got {other:?}"),
    }
}

#[test]
fn a_bound_row_that_is_not_a_slot_is_not_widened() {
    // A function's own total and its parameter / return vars are declaration
    // facts (or freshened per call site): widening one from inside a body would
    // change what every caller is charged.
    let mut engine = InferenceEngine::new();
    let v = engine.fresh_cap_var();
    let ty = fn_with(CapRow::var(v));
    engine
        .unify(&ty, &fn_with(CapRow::closed(CapBits::EMPTY)), S)
        .unwrap();
    engine
        .unify(&ty, &fn_with(CapRow::closed(CapBits::FS_READ)), S)
        .unwrap();
    assert_eq!(bits_of(&engine, v), CapBits::EMPTY);
}

#[test]
fn a_type_variable_bound_to_a_closure_becomes_a_slot() {
    // `fn push<T>(arr: [T], val: T)`: the first closure binds T, the second must
    // still widen the row T carries.
    let mut engine = InferenceEngine::new();
    let t = engine.fresh_var();
    engine
        .unify(&t, &fn_with(CapRow::closed(CapBits::EMPTY)), S)
        .unwrap();
    engine
        .unify(&t, &fn_with(CapRow::closed(CapBits::FS_READ)), S)
        .unwrap();
    match engine.resolve(&t) {
        Type::Function { caps, .. } => assert!(
            engine
                .resolve_cap_row(&caps)
                .concrete_bits()
                .contains_bits(CapBits::FS_READ),
            "second closure dropped: {}",
            engine.resolve_cap_row(&caps).display()
        ),
        other => panic!("expected a function type, got {other:?}"),
    }
}

#[test]
fn an_array_literal_element_row_is_the_union_of_every_element() {
    let mut engine = InferenceEngine::new();
    let first = fn_with(CapRow::closed(CapBits::EMPTY));
    let elem = engine.literal_slot_type(&first);
    engine
        .unify(&elem, &fn_with(CapRow::closed(CapBits::FS_READ)), S)
        .unwrap();
    match engine.resolve(&elem) {
        Type::Function { caps, .. } => assert!(
            engine
                .resolve_cap_row(&caps)
                .concrete_bits()
                .contains_bits(CapBits::FS_READ),
            "a literal typed by its first element drops the rest: {}",
            engine.resolve_cap_row(&caps).display()
        ),
        other => panic!("expected a function type, got {other:?}"),
    }
}

#[test]
fn a_type_without_a_function_is_left_alone_by_the_literal_slot_type() {
    let mut engine = InferenceEngine::new();
    assert_eq!(engine.literal_slot_type(&Type::I64), Type::I64);
}

#[test]
fn binding_a_row_var_to_itself_binds_nothing() {
    let mut engine = InferenceEngine::new();
    let v = engine.fresh_cap_var();
    engine.bind_cap_var(v, CapRow::var(v));
    // Still open: a self edge must not turn an unbound var into a bound-empty one.
    assert!(!engine.resolve_cap_row(&CapRow::var(v)).is_closed());
}

#[test]
fn a_bodys_store_into_a_global_slot_is_replayed_with_the_callers_closure() {
    // `fn register(reg, f) { reg.field = f }`: the body stores its OWN parameter
    // var `p` into the global slot. No call site ever binds `p` -- each freshens a
    // copy -- so the caller's closure only reaches the slot if the call replays
    // the store with its copy.
    let mut engine = InferenceEngine::new();
    let slot = engine.fresh_cap_var();
    engine.mark_slot_rows(&fn_with(CapRow::var(slot)), true);
    let p = engine.fresh_cap_var();
    let own = engine.fresh_cap_var();

    engine.begin_store_effects(&[p]);
    engine
        .unify(&fn_with(CapRow::var(slot)), &fn_with(CapRow::var(p)), S)
        .unwrap();
    engine.end_store_effects(Some(own));

    // A call site: its fresh copy of `p` is bound by the argument it passes.
    let p_call = engine.fresh_cap_var();
    let mut map = std::collections::HashMap::new();
    map.insert(p, p_call);
    engine.replay_store_effects(own, &map);
    engine
        .unify(
            &fn_with(CapRow::var(p_call)),
            &fn_with(CapRow::closed(CapBits::FS_READ)),
            S,
        )
        .unwrap();
    assert!(
        bits_of(&engine, slot).contains_bits(CapBits::FS_READ),
        "the registration call stored nothing the slot can see: {}",
        engine.resolve_cap_row(&CapRow::var(slot)).display()
    );
}

#[test]
fn a_local_slot_is_flattened_when_a_row_is_copied_into_a_call_site() {
    // A local slot's binding can mention the callee's own polymorphic param var;
    // `instantiate_row` must remap that var, so the local slot is chased like
    // any other var. A global slot stays a reference.
    let mut engine = InferenceEngine::new();
    let p = engine.fresh_cap_var();
    let local = engine.fresh_slot_cap_var();
    engine.bind_cap_var(local, CapRow::var(p));
    let p_call = engine.fresh_cap_var();
    let mut map = std::collections::HashMap::new();
    map.insert(p, p_call);
    let inst = engine.instantiate_row(&CapRow::var(local), &map);
    assert_eq!(inst, CapRow::var(p_call));

    let global = engine.fresh_cap_var();
    engine.mark_slot_rows(&fn_with(CapRow::var(global)), true);
    engine.bind_cap_var(global, CapRow::closed(CapBits::FS_READ));
    let inst = engine.instantiate_row(&CapRow::var(global), &map);
    assert_eq!(inst, CapRow::var(global));
}

// ── Signature vars are inputs (LEDGER root R2, the verifier's refutation) ──

#[test]
fn a_signature_var_is_not_narrowed_by_a_flow_into_a_closed_slot() {
    // `let mut a = [safe]; a = push(a, g); return a`: the array's element slot
    // already holds the EMPTY row when the parameter `g` meets it. The original
    // rule handed the destination's closed row to the source's unbound vars, so
    // `g`'s own var was bound to the empty row and every caller was told the
    // parameter is pure.
    let mut engine = InferenceEngine::new();
    let slot = engine.fresh_slot_cap_var();
    engine.bind_cap_var(slot, CapRow::closed(CapBits::EMPTY));
    let p = engine.fresh_cap_var();
    let own = engine.fresh_cap_var();

    engine.begin_store_effects(&[p]);
    engine
        .unify(&fn_with(CapRow::var(slot)), &fn_with(CapRow::var(p)), S)
        .unwrap();
    engine.end_store_effects(Some(own));

    assert!(
        !engine.resolve_cap_row(&CapRow::var(p)).is_closed(),
        "the parameter's var was narrowed to {}",
        engine.resolve_cap_row(&CapRow::var(p)).display()
    );
    assert!(
        engine
            .resolve_cap_row(&CapRow::var(slot))
            .var_ids()
            .contains(&p),
        "the slot does not mention the parameter it was handed"
    );
}

#[test]
fn a_call_site_var_still_takes_a_closed_destination_row() {
    // The original rule is still needed outside a body: `map(tools, invoke)`
    // unifies the element row with the row of `invoke`'s own parameter (a fresh
    // call-site var), and that var must take the element's row.
    let mut engine = InferenceEngine::new();
    let slot = engine.fresh_slot_cap_var();
    engine.bind_cap_var(slot, CapRow::closed(CapBits::FS_READ));
    let q = engine.fresh_cap_var();
    engine
        .unify(&fn_with(CapRow::var(slot)), &fn_with(CapRow::var(q)), S)
        .unwrap();
    assert_eq!(bits_of(&engine, q), CapBits::FS_READ);
}

#[test]
fn a_lambdas_annotated_parameter_is_one_slot_every_call_feeds() {
    // A lambda is a single monomorphic value: called with a pure function and
    // then with a privileged one, its parameter row is the union.
    let mut engine = InferenceEngine::new();
    let lg = engine.fresh_cap_var();
    engine.mark_lambda_param_rows(&fn_with(CapRow::var(lg)));
    engine
        .unify(&fn_with(CapRow::var(lg)), &fn_with(CapRow::closed(CapBits::EMPTY)), S)
        .unwrap();
    engine
        .unify(
            &fn_with(CapRow::var(lg)),
            &fn_with(CapRow::closed(CapBits::FS_READ)),
            S,
        )
        .unwrap();
    assert!(
        bits_of(&engine, lg).contains_bits(CapBits::FS_READ),
        "the second call was dropped: {}",
        engine.resolve_cap_row(&CapRow::var(lg)).display()
    );
}

#[test]
fn the_rows_in_a_lambdas_parameter_types_are_frozen_while_its_body_is_checked() {
    // Inside the lambda, the parameter's var is an input: a flow into a closed
    // slot must not narrow it. Once the body is done the freeze is lifted and
    // the original rule applies again (the lambda is handed to a higher-order
    // function that supplies the row).
    let mut engine = InferenceEngine::new();
    let slot = engine.fresh_slot_cap_var();
    engine.bind_cap_var(slot, CapRow::closed(CapBits::EMPTY));
    let lg = engine.fresh_cap_var();
    let param = fn_with(CapRow::var(lg));
    let frozen = engine.freeze_rows_of(&[param.clone()]);
    engine
        .unify(&fn_with(CapRow::var(slot)), &param, S)
        .unwrap();
    assert!(!engine.resolve_cap_row(&CapRow::var(lg)).is_closed());
    engine.unfreeze_rows(frozen);
    // After the body, the same flow binds it again.
    let slot2 = engine.fresh_slot_cap_var();
    engine.bind_cap_var(slot2, CapRow::closed(CapBits::FS_READ));
    engine
        .unify(&fn_with(CapRow::var(slot2)), &param, S)
        .unwrap();
    assert_eq!(bits_of(&engine, lg), CapBits::FS_READ);
}

#[test]
fn a_join_of_two_function_values_carries_both_rows() {
    // The arms of an `if` / `match`: typed by a slot every arm is unified into.
    let mut engine = InferenceEngine::new();
    let first = fn_with(CapRow::closed(CapBits::EMPTY));
    let joined = engine.literal_slot_type(&first);
    engine
        .unify(&joined, &fn_with(CapRow::closed(CapBits::FS_READ)), S)
        .unwrap();
    match engine.resolve(&joined) {
        Type::Function { caps, .. } => assert!(engine
            .resolve_cap_row(&caps)
            .concrete_bits()
            .contains_bits(CapBits::FS_READ)),
        other => panic!("expected a function type, got {other:?}"),
    }
}

#[test]
fn a_parameter_position_is_contravariant() {
    // `for_each(fns, |g: fn() -> str| ..)`: the element row E is what the
    // HOF hands the lambda, so it flows into the lambda's parameter row `lg`.
    // The lambda's var must take E's row, and E must NOT be widened by `lg`.
    let mut engine = InferenceEngine::new();
    let e = engine.fresh_slot_cap_var();
    engine.bind_cap_var(e, CapRow::closed(CapBits::FS_READ));
    let lg = engine.fresh_cap_var();
    let expected = Type::Function {
        params: vec![fn_with(CapRow::var(e))],
        ret: Box::new(Type::Void),
        caps: CapRow::closed(CapBits::EMPTY),
    };
    let found = Type::Function {
        params: vec![fn_with(CapRow::var(lg))],
        ret: Box::new(Type::Void),
        caps: CapRow::closed(CapBits::EMPTY),
    };
    engine.unify(&expected, &found, S).unwrap();
    assert_eq!(bits_of(&engine, lg), CapBits::FS_READ);
    // The slot was not widened by the lambda's parameter var.
    assert!(!engine.resolve_cap_row(&CapRow::var(e)).var_ids().contains(&lg));
}

fn array_of(element: Type) -> Type {
    Type::Array {
        element: Box::new(element),
        size: None,
    }
}

#[test]
fn a_store_into_a_container_the_body_was_handed_reaches_the_callers_slot() {
    // `fn put(reg: [fn() -> str], g: fn() -> str) { reg[0] = g }` called as
    // `put(fns, r)`. The body must NOT bind the parameter's storage var `e`
    // (reads through `reg` would be charged only what was stored), and the
    // caller's array slot must learn what the call stored.
    let mut engine = InferenceEngine::new();
    let e = engine.fresh_cap_var();
    let p = engine.fresh_cap_var();
    let own = engine.fresh_cap_var();
    engine.begin_store_effects(&[e, p]);
    engine.mark_input_storage(&[array_of(fn_with(CapRow::var(e))), fn_with(CapRow::var(p))]);
    engine
        .unify(&fn_with(CapRow::var(e)), &fn_with(CapRow::var(p)), S)
        .unwrap();
    engine.end_store_effects(Some(own));
    assert!(
        !engine.resolve_cap_row(&CapRow::var(e)).is_closed(),
        "the body bound the parameter's storage var to {}",
        engine.resolve_cap_row(&CapRow::var(e)).display()
    );

    // A call site. The reference is typed first, so the replay finds the
    // call's copy of the storage var still unbound.
    let e_call = engine.fresh_cap_var();
    let p_call = engine.fresh_cap_var();
    let mut map = std::collections::HashMap::new();
    map.insert(e, e_call);
    map.insert(p, p_call);
    engine.replay_store_effects(own, &map);
    // The caller's array was built from a pure function.
    let caller_slot = engine.fresh_slot_cap_var();
    engine.bind_cap_var(caller_slot, CapRow::closed(CapBits::EMPTY));
    // Arguments: the array, then the closure.
    engine
        .unify(&fn_with(CapRow::var(e_call)), &fn_with(CapRow::var(caller_slot)), S)
        .unwrap();
    engine
        .unify(
            &fn_with(CapRow::var(p_call)),
            &fn_with(CapRow::closed(CapBits::FS_READ)),
            S,
        )
        .unwrap();
    assert!(
        bits_of(&engine, caller_slot).contains_bits(CapBits::FS_READ),
        "the call stored a privileged closure into the caller's array and its slot did not learn: {}",
        engine.resolve_cap_row(&CapRow::var(caller_slot)).display()
    );
}

#[test]
fn a_store_of_named_privileged_bits_into_a_handed_container_is_replayed() {
    // `fn seed(reg: [fn() -> str]) { reg[0] = reader }`: the stored row is a
    // closed row the body names, not one of its own parameters.
    let mut engine = InferenceEngine::new();
    let e = engine.fresh_cap_var();
    let own = engine.fresh_cap_var();
    engine.begin_store_effects(&[e]);
    engine.mark_input_storage(&[array_of(fn_with(CapRow::var(e)))]);
    engine
        .unify(
            &fn_with(CapRow::var(e)),
            &fn_with(CapRow::closed(CapBits::FS_READ)),
            S,
        )
        .unwrap();
    engine.end_store_effects(Some(own));

    let e_call = engine.fresh_cap_var();
    let mut map = std::collections::HashMap::new();
    map.insert(e, e_call);
    let caller_slot = engine.fresh_slot_cap_var();
    engine.bind_cap_var(caller_slot, CapRow::closed(CapBits::EMPTY));
    engine
        .unify(&fn_with(CapRow::var(e_call)), &fn_with(CapRow::var(caller_slot)), S)
        .unwrap();
    engine.replay_store_effects(own, &map);
    assert!(bits_of(&engine, caller_slot).contains_bits(CapBits::FS_READ));
}

#[test]
fn a_read_only_container_parameter_records_no_store() {
    let mut engine = InferenceEngine::new();
    let e = engine.fresh_cap_var();
    let own = engine.fresh_cap_var();
    engine.begin_store_effects(&[e]);
    engine.mark_input_storage(&[array_of(fn_with(CapRow::var(e)))]);
    // The body only READS the elements into a local slot.
    let local = engine.fresh_slot_cap_var();
    engine
        .unify(&fn_with(CapRow::var(local)), &fn_with(CapRow::var(e)), S)
        .unwrap();
    engine.end_store_effects(Some(own));

    let e_call = engine.fresh_cap_var();
    let mut map = std::collections::HashMap::new();
    map.insert(e, e_call);
    let caller_slot = engine.fresh_slot_cap_var();
    engine.bind_cap_var(caller_slot, CapRow::closed(CapBits::EMPTY));
    engine
        .unify(&fn_with(CapRow::var(e_call)), &fn_with(CapRow::var(caller_slot)), S)
        .unwrap();
    engine.replay_store_effects(own, &map);
    assert_eq!(bits_of(&engine, caller_slot), CapBits::EMPTY);
}

#[test]
fn a_lambdas_store_into_an_annotated_container_parameter_reaches_every_arguments_slot() {
    // `let put = |reg: [fn() -> str], g: fn() -> str| { reg[0] = g }`. A lambda is one
    // monomorphic value: there is no per-call copy of the parameter's element row to
    // replay, so the store is written into the slot of each argument the row is bound
    // to, the moment it is bound to it.
    let mut engine = InferenceEngine::new();
    let e = engine.fresh_cap_var();
    let lg = engine.fresh_cap_var();
    let reg_ty = array_of(fn_with(CapRow::var(e)));
    let g_ty = fn_with(CapRow::var(lg));
    engine.mark_lambda_param_rows(&reg_ty);
    engine.mark_lambda_input_storage(&reg_ty);
    engine.mark_lambda_param_rows(&g_ty);
    engine.mark_lambda_input_storage(&g_ty);
    let frozen = engine.freeze_rows_of(&[reg_ty.clone(), g_ty.clone()]);
    // The body: reg[0] = g.
    engine
        .unify(&fn_with(CapRow::var(e)), &fn_with(CapRow::var(lg)), S)
        .unwrap();
    engine.unfreeze_rows(frozen);
    assert!(
        !engine.resolve_cap_row(&CapRow::var(e)).is_closed(),
        "the body bound the parameter's element row"
    );

    // First call: an array built from a pure function, and a privileged closure.
    let caller_a = engine.fresh_slot_cap_var();
    engine.bind_cap_var(caller_a, CapRow::closed(CapBits::EMPTY));
    engine
        .unify(&fn_with(CapRow::var(e)), &fn_with(CapRow::var(caller_a)), S)
        .unwrap();
    engine
        .unify(
            &fn_with(CapRow::var(lg)),
            &fn_with(CapRow::closed(CapBits::FS_READ)),
            S,
        )
        .unwrap();
    assert!(
        bits_of(&engine, caller_a).contains_bits(CapBits::FS_READ),
        "the lambda stored a privileged closure into the caller's array: {}",
        engine.resolve_cap_row(&CapRow::var(caller_a)).display()
    );

    // A second call with another array: its slot learns as well.
    let caller_b = engine.fresh_slot_cap_var();
    engine.bind_cap_var(caller_b, CapRow::closed(CapBits::EMPTY));
    engine
        .unify(&fn_with(CapRow::var(e)), &fn_with(CapRow::var(caller_b)), S)
        .unwrap();
    assert!(bits_of(&engine, caller_b).contains_bits(CapBits::FS_READ));
}
