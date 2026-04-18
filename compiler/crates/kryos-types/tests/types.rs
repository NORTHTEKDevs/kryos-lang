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
    assert_eq!(
        ty,
        Type::Function {
            params: vec![Type::I32],
            ret: Box::new(Type::Bool),
        }
    );
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
