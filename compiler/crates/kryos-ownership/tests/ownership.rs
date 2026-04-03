//! Integration tests for the kryos-ownership crate.

use kryos_ast::*;
use kryos_errors::Span;
use kryos_ownership::{analyze_ownership, ArcReason};

fn dummy_span() -> Span {
    Span::DUMMY
}

/// Build a minimal module wrapping statements in a function.
fn module_with_fn(stmts: Vec<Stmt>) -> Module {
    Module {
        name: "test".into(),
        declarations: vec![Decl::Function {
            name: "main".into(),
            generics: vec![],
            params: vec![],
            ret_ty: None,
            body: Some(Block {
                stmts,
                span: dummy_span(),
            }),
            public: false,
            annotations: vec![],
            span: dummy_span(),
        }],
        span: dummy_span(),
    }
}

/// Helper: create a let statement binding `name = value_expr`.
fn let_move(name: &str, value: Expr) -> Stmt {
    Stmt::Let {
        name: name.into(),
        mutable: false,
        ty: None,
        value: Some(value),
        pattern: None,
        span: Span::new(0, 0, 10),
    }
}

/// Helper: create a let statement with type annotation.
fn let_typed(name: &str, ty_name: &str, value: Expr) -> Stmt {
    Stmt::Let {
        name: name.into(),
        mutable: false,
        ty: Some(TypeExpr::Simple {
            name: ty_name.into(),
            span: dummy_span(),
        }),
        value: Some(value),
        pattern: None,
        span: Span::new(0, 0, 10),
    }
}

/// Helper: create a mutable let statement.
fn let_mut_stmt(name: &str, value: Expr) -> Stmt {
    Stmt::Let {
        name: name.into(),
        mutable: true,
        ty: None,
        value: Some(value),
        pattern: None,
        span: Span::new(0, 0, 10),
    }
}

/// Helper: create an uninitialized let.
fn let_uninit(name: &str, ty_name: &str) -> Stmt {
    Stmt::Let {
        name: name.into(),
        mutable: false,
        ty: Some(TypeExpr::Simple {
            name: ty_name.into(),
            span: dummy_span(),
        }),
        value: None,
        pattern: None,
        span: Span::new(0, 0, 10),
    }
}

/// Helper: create an identifier expression.
fn ident(name: &str) -> Expr {
    Expr::Identifier {
        name: name.into(),
        span: Span::new(0, 20, 30),
    }
}

/// Helper: create a function call expression.
fn call(func_name: &str, args: Vec<Expr>) -> Expr {
    Expr::FnCall {
        callee: Box::new(ident(func_name)),
        args,
        span: Span::new(0, 20, 30),
    }
}

/// Helper: wrap expression in statement.
fn expr_stmt(expr: Expr) -> Stmt {
    Stmt::Expr {
        expr: expr.clone(),
        span: expr.span(),
    }
}

/// Helper: assignment statement.
fn assign(name: &str, value: Expr) -> Stmt {
    Stmt::Assign {
        target: ident(name),
        op: AssignOp::Assign,
        value,
        span: Span::new(0, 30, 40),
    }
}

/// Helper: a string literal (non-copy type).
fn string_lit(s: &str) -> Expr {
    Expr::StringLiteral {
        value: s.into(),
        span: Span::new(0, 0, 5),
    }
}

/// Helper: an integer literal (copy type).
fn int_lit(v: i64) -> Expr {
    Expr::IntLiteral {
        value: v,
        span: Span::new(0, 0, 5),
    }
}

// ── Test: Move on assignment ────────────────────────────────────

#[test]
fn move_on_assignment_error() {
    // let x = "hello"
    // let a = x      ← moves x
    // use(x)         ← error: use of moved value
    let module = module_with_fn(vec![
        let_move("x", string_lit("hello")),
        let_move("a", ident("x")),
        expr_stmt(call("use", vec![ident("x")])),
    ]);
    let result = analyze_ownership(&module);
    assert!(
        result.errors.iter().any(|e| e.message.contains("use of moved value")),
        "expected 'use of moved value' error, got: {:?}",
        result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: Move on function call ─────────────────────────────────

#[test]
fn move_on_function_call_error() {
    // let x = "hello"
    // func(x)        ← moves x
    // use(x)         ← error: use of moved value
    let module = module_with_fn(vec![
        let_move("x", string_lit("hello")),
        expr_stmt(call("func", vec![ident("x")])),
        expr_stmt(call("use", vec![ident("x")])),
    ]);
    let result = analyze_ownership(&module);
    assert!(
        result.errors.iter().any(|e| e.message.contains("use of moved value")),
        "expected 'use of moved value' error, got: {:?}",
        result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: Copy type survives ────────────────────────────────────

#[test]
fn copy_type_survives() {
    // let x: i32 = 42
    // let a: i32 = x  ← copy, x remains owned
    // use(x)          ← OK: x is still alive
    let module = module_with_fn(vec![
        let_typed("x", "i32", int_lit(42)),
        let_typed("a", "i32", ident("x")),
        expr_stmt(call("use", vec![ident("x")])),
    ]);
    let result = analyze_ownership(&module);
    let move_errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.message.contains("use of moved value"))
        .collect();
    assert!(
        move_errors.is_empty(),
        "copy types should not produce move errors, got: {:?}",
        move_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: Shared wrapping ───────────────────────────────────────

#[test]
fn shared_wrapping_arc_insertion() {
    // let s = shared "hello"
    let module = module_with_fn(vec![Stmt::Let {
        name: "s".into(),
        mutable: false,
        ty: None,
        value: Some(Expr::SharedExpr {
            inner: Box::new(string_lit("hello")),
            span: Span::new(0, 5, 20),
        }),
        pattern: None,
        span: Span::new(0, 0, 25),
    }]);
    let result = analyze_ownership(&module);
    assert!(
        result.arc_insertions.iter().any(|i| i.variable == "s"
            && i.reason == ArcReason::ExplicitShared),
        "expected ARC insertion for shared variable 's'"
    );
    assert!(result.errors.is_empty(), "no errors expected");
}

// ── Test: Closure capture mutable detection ─────────────────────

#[test]
fn closure_capture_mut_arc_insertion() {
    // let mut counter = 0
    // let inc = || counter + 1   ← captures mutable 'counter'
    let module = module_with_fn(vec![
        let_mut_stmt("counter", int_lit(0)),
        let_move(
            "inc",
            Expr::Lambda {
                params: vec![],
                ret_ty: None,
                body: Box::new(Expr::BinaryOp {
                    op: BinOp::Add,
                    left: Box::new(ident("counter")),
                    right: Box::new(int_lit(1)),
                    span: dummy_span(),
                }),
                span: Span::new(0, 20, 40),
            },
        ),
    ]);
    let result = analyze_ownership(&module);
    assert!(
        result.arc_insertions.iter().any(|i| i.variable == "counter"
            && i.reason == ArcReason::ClosureCaptureMut),
        "expected ARC insertion for mutable capture 'counter', got: {:?}",
        result.arc_insertions
    );
}

// ── Test: Channel send moves ────────────────────────────────────

#[test]
fn channel_send_moves_value() {
    // let x = "data"
    // send(ch, x)   ← moves x into channel
    // use(x)        ← error
    let module = module_with_fn(vec![
        let_move("ch", string_lit("chan")),
        let_move("x", string_lit("data")),
        expr_stmt(call("send", vec![ident("ch"), ident("x")])),
        expr_stmt(call("use", vec![ident("x")])),
    ]);
    let result = analyze_ownership(&module);
    assert!(
        result.errors.iter().any(|e| e.message.contains("use of moved value")),
        "send should move the value, got errors: {:?}",
        result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: Re-assignment after move ──────────────────────────────

#[test]
fn reassignment_after_move_valid() {
    // let x = "hello"
    // let a = x        ← moves x
    // x = "world"      ← re-assigns x
    // use(x)           ← OK: x is owned again
    let module = module_with_fn(vec![
        Stmt::Let {
            name: "x".into(),
            mutable: true,
            ty: None,
            value: Some(string_lit("hello")),
            pattern: None,
            span: Span::new(0, 0, 10),
        },
        let_move("a", ident("x")),
        assign("x", string_lit("world")),
        expr_stmt(call("use", vec![ident("x")])),
    ]);
    let result = analyze_ownership(&module);
    let move_errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.message.contains("use of moved value"))
        .collect();
    assert!(
        move_errors.is_empty(),
        "re-assignment should reset ownership, got: {:?}",
        move_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: Uninitialized use ─────────────────────────────────────

#[test]
fn uninitialized_use_error() {
    // let x: i32
    // use(x)       ← error: uninitialized
    let module = module_with_fn(vec![
        let_uninit("x", "i32"),
        expr_stmt(call("use", vec![ident("x")])),
    ]);
    let result = analyze_ownership(&module);
    assert!(
        result.errors.iter().any(|e| e.message.contains("uninitialized")),
        "expected 'uninitialized' error, got: {:?}",
        result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: Conditional move detection ────────────────────────────

#[test]
fn conditional_move_warning() {
    // let x = "hello"
    // if cond { let a = x } else { /* x not moved */ }
    //
    // Since we need a condition variable, define `cond` first.
    let module = module_with_fn(vec![
        let_typed("cond", "bool", Expr::BoolLiteral { value: true, span: dummy_span() }),
        let_move("x", string_lit("hello")),
        Stmt::If {
            condition: ident("cond"),
            then_block: Block {
                stmts: vec![let_move("a", ident("x"))],
                span: dummy_span(),
            },
            elif_clauses: vec![],
            else_block: Some(Block {
                stmts: vec![],
                span: dummy_span(),
            }),
            span: Span::new(0, 50, 100),
        },
    ]);
    let result = analyze_ownership(&module);
    assert!(
        result.errors.iter().any(|e| e.message.contains("moved in one branch")),
        "expected conditional move warning, got: {:?}",
        result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: Struct partial move detection ─────────────────────────

#[test]
fn struct_partial_move() {
    // let p = Point { x: 1, y: 2 }
    // let a = p.x           ← partial move
    // use(p.x)              ← error: field was moved
    let module = module_with_fn(vec![
        let_move(
            "p",
            Expr::StructLiteral {
                name: "Point".into(),
                fields: vec![
                    ("x".into(), string_lit("a")),
                    ("y".into(), string_lit("b")),
                ],
                span: dummy_span(),
            },
        ),
        let_move(
            "a",
            Expr::FieldAccess {
                object: Box::new(ident("p")),
                field: "x".into(),
                span: Span::new(0, 20, 25),
            },
        ),
        expr_stmt(Expr::FieldAccess {
            object: Box::new(ident("p")),
            field: "x".into(),
            span: Span::new(0, 30, 35),
        }),
    ]);
    let result = analyze_ownership(&module);
    assert!(
        result.errors.iter().any(|e| e.message.contains("partially moved")),
        "expected partial move error, got: {:?}",
        result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: Weak reference creation ───────────────────────────────

#[test]
fn weak_reference_no_move() {
    // let x = "hello"
    // let w = weak x    ← weak ref, doesn't move x
    // use(x)            ← OK: x is still owned
    let module = module_with_fn(vec![
        let_move("x", string_lit("hello")),
        Stmt::Let {
            name: "w".into(),
            mutable: false,
            ty: None,
            value: Some(Expr::WeakExpr {
                inner: Box::new(ident("x")),
                span: Span::new(0, 10, 20),
            }),
            pattern: None,
            span: Span::new(0, 10, 25),
        },
        expr_stmt(call("use", vec![ident("x")])),
    ]);
    let result = analyze_ownership(&module);
    let move_errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.message.contains("use of moved value"))
        .collect();
    assert!(
        move_errors.is_empty(),
        "weak reference should not move the source, got: {:?}",
        move_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: Multiple shared refs valid ────────────────────────────

#[test]
fn multiple_shared_refs_valid() {
    // let s = shared "hello"
    // let a = s      ← shared values can be cloned/used multiple times
    // let b = s      ← still OK
    // use(s)         ← still OK
    let module = module_with_fn(vec![
        Stmt::Let {
            name: "s".into(),
            mutable: false,
            ty: None,
            value: Some(Expr::SharedExpr {
                inner: Box::new(string_lit("hello")),
                span: Span::new(0, 5, 20),
            }),
            pattern: None,
            span: Span::new(0, 0, 25),
        },
        let_move("a", ident("s")),
        let_move("b", ident("s")),
        expr_stmt(call("use", vec![ident("s")])),
    ]);
    let result = analyze_ownership(&module);
    let move_errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.message.contains("use of moved value"))
        .collect();
    assert!(
        move_errors.is_empty(),
        "shared values should be usable multiple times, got: {:?}",
        move_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: Return moves ownership out ────────────────────────────

#[test]
fn return_moves_ownership() {
    // let x = "hello"
    // return x         ← moves x
    // use(x)           ← error (dead code, but still a move error)
    let module = module_with_fn(vec![
        let_move("x", string_lit("hello")),
        Stmt::Return {
            value: Some(ident("x")),
            span: Span::new(0, 15, 25),
        },
        expr_stmt(call("use", vec![ident("x")])),
    ]);
    let result = analyze_ownership(&module);
    assert!(
        result.errors.iter().any(|e| e.message.contains("use of moved value")),
        "return should move the value, got: {:?}",
        result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: Copy type i32 by literal inference ────────────────────

#[test]
fn literal_int_is_copy() {
    // let x = 42       ← int literal is copy
    // let a = x        ← copy
    // use(x)           ← OK
    let module = module_with_fn(vec![
        let_move("x", int_lit(42)),
        let_move("a", ident("x")),
        expr_stmt(call("use", vec![ident("x")])),
    ]);
    let result = analyze_ownership(&module);
    let move_errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.message.contains("use of moved value"))
        .collect();
    assert!(
        move_errors.is_empty(),
        "integer literals should be copy, got: {:?}",
        move_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: Bool copy type ────────────────────────────────────────

#[test]
fn bool_is_copy_type() {
    // let x: bool = true
    // let a: bool = x
    // use(x)         ← OK
    let module = module_with_fn(vec![
        let_typed(
            "x",
            "bool",
            Expr::BoolLiteral {
                value: true,
                span: dummy_span(),
            },
        ),
        let_typed("a", "bool", ident("x")),
        expr_stmt(call("use", vec![ident("x")])),
    ]);
    let result = analyze_ownership(&module);
    let move_errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.message.contains("use of moved value"))
        .collect();
    assert!(
        move_errors.is_empty(),
        "bool should be a copy type, got: {:?}",
        move_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: @copy struct annotation ───────────────────────────────

#[test]
fn copy_struct_annotation() {
    // @copy struct Vec2 { x: f64, y: f64 }
    // fn main() { let a = v; let b = v; use(v); }
    //
    // Vec2 is @copy so v stays owned.
    let module = Module {
        name: "test".into(),
        declarations: vec![
            Decl::Struct {
                name: "Vec2".into(),
                generics: vec![],
                fields: vec![
                    StructField {
                        name: "x".into(),
                        ty: TypeExpr::Simple {
                            name: "f64".into(),
                            span: dummy_span(),
                        },
                        public: true,
                        default: None,
                        span: dummy_span(),
                    },
                    StructField {
                        name: "y".into(),
                        ty: TypeExpr::Simple {
                            name: "f64".into(),
                            span: dummy_span(),
                        },
                        public: true,
                        default: None,
                        span: dummy_span(),
                    },
                ],
                public: true,
                annotations: vec![Annotation {
                    name: "copy".into(),
                    args: vec![],
                    span: dummy_span(),
                }],
                span: dummy_span(),
            },
            Decl::Function {
                name: "main".into(),
                generics: vec![],
                params: vec![Param {
                    name: "v".into(),
                    ty: Some(TypeExpr::Simple {
                        name: "Vec2".into(),
                        span: dummy_span(),
                    }),
                    default: None,
                    span: Span::new(0, 0, 5),
                }],
                ret_ty: None,
                body: Some(Block {
                    stmts: vec![
                        let_move("a", ident("v")),
                        let_move("b", ident("v")),
                        expr_stmt(call("use", vec![ident("v")])),
                    ],
                    span: dummy_span(),
                }),
                public: false,
                annotations: vec![],
                span: dummy_span(),
            },
        ],
        span: dummy_span(),
    };
    let result = analyze_ownership(&module);
    let move_errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.message.contains("use of moved value"))
        .collect();
    assert!(
        move_errors.is_empty(),
        "@copy structs should not produce move errors, got: {:?}",
        move_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: Actor state fields get ARC insertion ──────────────────

#[test]
fn actor_state_arc_boundary() {
    let module = Module {
        name: "test".into(),
        declarations: vec![Decl::Actor {
            name: "Counter".into(),
            state_fields: vec![StructField {
                name: "count".into(),
                ty: TypeExpr::Simple {
                    name: "i64".into(),
                    span: dummy_span(),
                },
                public: false,
                default: None,
                span: dummy_span(),
            }],
            handlers: vec![],
            annotations: vec![],
            span: Span::new(0, 0, 100),
        }],
        span: dummy_span(),
    };
    let result = analyze_ownership(&module);
    assert!(
        result
            .arc_insertions
            .iter()
            .any(|i| i.variable == "count" && i.reason == ArcReason::ActorBoundary),
        "actor state fields should get ARC insertion"
    );
}

// ── Test: Double move error ─────────────────────────────────────

#[test]
fn double_move_error() {
    // let x = "hello"
    // let a = x       ← moves x
    // let b = x       ← error: already moved
    let module = module_with_fn(vec![
        let_move("x", string_lit("hello")),
        let_move("a", ident("x")),
        let_move("b", ident("x")),
    ]);
    let result = analyze_ownership(&module);
    assert!(
        result.errors.iter().any(|e| e.message.contains("use of moved value")),
        "double move should error"
    );
}

// ── Test: No errors on clean code ───────────────────────────────

#[test]
fn clean_code_no_errors() {
    // let x = "hello"
    // let y = "world"
    // use(x)
    // use(y)
    let module = module_with_fn(vec![
        let_move("x", string_lit("hello")),
        let_move("y", string_lit("world")),
        expr_stmt(call("use_val", vec![ident("x")])),
        expr_stmt(call("use_val", vec![ident("y")])),
    ]);
    let result = analyze_ownership(&module);
    assert!(
        result.errors.is_empty(),
        "clean code should have no errors, got: {:?}",
        result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: ARC summary utility ───────────────────────────────────

#[test]
fn arc_summary_counts() {
    use kryos_ownership::arc::summarize;

    let insertions = vec![
        kryos_ownership::ArcInsertion {
            variable: "a".into(),
            scope_span: dummy_span(),
            reason: ArcReason::ExplicitShared,
        },
        kryos_ownership::ArcInsertion {
            variable: "b".into(),
            scope_span: dummy_span(),
            reason: ArcReason::ClosureCaptureMut,
        },
        kryos_ownership::ArcInsertion {
            variable: "c".into(),
            scope_span: dummy_span(),
            reason: ArcReason::ActorBoundary,
        },
        kryos_ownership::ArcInsertion {
            variable: "d".into(),
            scope_span: dummy_span(),
            reason: ArcReason::ExplicitShared,
        },
    ];
    let summary = summarize(&insertions);
    assert_eq!(summary.total, 4);
    assert_eq!(summary.explicit_shared, 2);
    assert_eq!(summary.closure_capture_mut, 1);
    assert_eq!(summary.actor_boundary, 1);
}

// ── Test: Spawn moves value ─────────────────────────────────────

#[test]
fn spawn_moves_value() {
    // let x = "hello"
    // spawn x        ← moves x into spawned task
    // use(x)         ← error
    let module = module_with_fn(vec![
        let_move("x", string_lit("hello")),
        Stmt::Spawn {
            expr: ident("x"),
            span: Span::new(0, 15, 25),
        },
        expr_stmt(call("use", vec![ident("x")])),
    ]);
    let result = analyze_ownership(&module);
    assert!(
        result.errors.iter().any(|e| e.message.contains("use of moved value")),
        "spawn should move the value"
    );
}
