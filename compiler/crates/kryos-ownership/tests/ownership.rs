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
        is_async: false,
            annotations: vec![],
            doc_comments: vec![],
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

/// Helper: a string literal (copy type — str is copy in Kryos).
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

/// Helper: a non-copy value (opaque function call whose return type
/// is unknown and therefore defaults to non-copy).
fn non_copy_val(tag: &str) -> Expr {
    call(&format!("make_{}", tag), vec![])
}

// ── Test: Move on assignment ────────────────────────────────────

#[test]
fn move_on_assignment_error() {
    // let x = make_obj()   (non-copy)
    // let a = x            ← moves x
    // use(x)               ← error: use of moved value
    let module = module_with_fn(vec![
        let_move("x", non_copy_val("obj")),
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

// ── Test: Function call borrows (does not move) ────────────────

#[test]
fn function_call_borrows_not_moves() {
    // let x = make_obj()   (non-copy)
    // func(x)              ← borrows x (passed by pointer in codegen)
    // use(x)               ← OK: x is still owned
    let module = module_with_fn(vec![
        let_move("x", non_copy_val("obj")),
        expr_stmt(call("func", vec![ident("x")])),
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
        "function args are implicit borrows, should not move, got: {:?}",
        move_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
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
    // let ch = make_chan()  (non-copy)
    // let x = make_data()  (non-copy)
    // send(ch, x)   ← moves x into channel
    // use(x)        ← error
    let module = module_with_fn(vec![
        let_move("ch", non_copy_val("chan")),
        let_move("x", non_copy_val("data")),
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
    // let x = make_obj()   (non-copy)
    // if cond { let a = x } else { /* x not moved */ }
    //
    // Since we need a condition variable, define `cond` first.
    let module = module_with_fn(vec![
        let_typed("cond", "bool", Expr::BoolLiteral { value: true, span: dummy_span() }),
        let_move("x", non_copy_val("obj")),
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
    // let x = make_obj()   (non-copy)
    // return x             ← moves x
    // use(x)               ← error (dead code, but still a move error)
    let module = module_with_fn(vec![
        let_move("x", non_copy_val("obj")),
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
                doc_comments: vec![],
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
        is_async: false,
                annotations: vec![],
                doc_comments: vec![],
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
    // let x = make_obj()   (non-copy)
    // let a = x            ← moves x
    // let b = x            ← error: already moved
    let module = module_with_fn(vec![
        let_move("x", non_copy_val("obj")),
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
    // let x = make_obj()   (non-copy)
    // spawn x              ← moves x into spawned task
    // use(x)               ← error
    let module = module_with_fn(vec![
        let_move("x", non_copy_val("obj")),
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

// ── Test: BinaryOp result is copy ──────────────────────────────

#[test]
fn binary_op_result_is_copy() {
    // let a: i64 = 10
    // let b: i64 = 20
    // let c = a + b    ← binary op on copy types produces copy
    // use(a)           ← OK: a was not moved (copy)
    // use(b)           ← OK: b was not moved (copy)
    let module = module_with_fn(vec![
        let_typed("a", "i64", int_lit(10)),
        let_typed("b", "i64", int_lit(20)),
        let_move(
            "c",
            Expr::BinaryOp {
                op: BinOp::Add,
                left: Box::new(ident("a")),
                right: Box::new(ident("b")),
                span: dummy_span(),
            },
        ),
        expr_stmt(call("use", vec![ident("a")])),
        expr_stmt(call("use", vec![ident("b")])),
    ]);
    let result = analyze_ownership(&module);
    let move_errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.message.contains("use of moved value"))
        .collect();
    assert!(
        move_errors.is_empty(),
        "binary op on copy types should not move operands, got: {:?}",
        move_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: UnaryOp result is copy ───────────────────────────────

#[test]
fn unary_op_result_is_copy() {
    // let a: i64 = 42
    // let b = -a       ← unary op on copy type produces copy
    // use(a)           ← OK: a was not moved
    let module = module_with_fn(vec![
        let_typed("a", "i64", int_lit(42)),
        let_move(
            "b",
            Expr::UnaryOp {
                op: UnOp::Neg,
                operand: Box::new(ident("a")),
                span: dummy_span(),
            },
        ),
        expr_stmt(call("use", vec![ident("a")])),
    ]);
    let result = analyze_ownership(&module);
    let move_errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.message.contains("use of moved value"))
        .collect();
    assert!(
        move_errors.is_empty(),
        "unary op on copy type should not move operand, got: {:?}",
        move_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: FnCall with copy return type ─────────────────────────

#[test]
fn fn_call_with_copy_return_type() {
    // fn compute() -> i64 { 42 }
    // fn main() {
    //   let x = compute()   ← returns i64 (copy), so x is copy
    //   let a = x           ← copy
    //   use(x)              ← OK: x is still alive
    // }
    let module = Module {
        name: "test".into(),
        declarations: vec![
            Decl::Function {
                name: "compute".into(),
                generics: vec![],
                params: vec![],
                ret_ty: Some(TypeExpr::Simple {
                    name: "i64".into(),
                    span: dummy_span(),
                }),
                body: Some(Block {
                    stmts: vec![Stmt::Return {
                        value: Some(int_lit(42)),
                        span: dummy_span(),
                    }],
                    span: dummy_span(),
                }),
                public: false,
        is_async: false,
                annotations: vec![],
                doc_comments: vec![],
                span: dummy_span(),
            },
            Decl::Function {
                name: "main".into(),
                generics: vec![],
                params: vec![],
                ret_ty: None,
                body: Some(Block {
                    stmts: vec![
                        let_move("x", call("compute", vec![])),
                        let_move("a", ident("x")),
                        expr_stmt(call("use", vec![ident("x")])),
                    ],
                    span: dummy_span(),
                }),
                public: false,
        is_async: false,
                annotations: vec![],
                doc_comments: vec![],
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
        "fn call returning copy type should produce copy value, got: {:?}",
        move_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: FnCall with non-copy return type still moves ─────────

#[test]
fn fn_call_with_non_copy_return_moves() {
    // fn create() -> String { "hello" }
    // fn main() {
    //   let x = create()   ← returns String (non-copy)
    //   let a = x           ← moves x
    //   use(x)              ← error: x was moved
    // }
    let module = Module {
        name: "test".into(),
        declarations: vec![
            Decl::Function {
                name: "create".into(),
                generics: vec![],
                params: vec![],
                ret_ty: Some(TypeExpr::Simple {
                    name: "String".into(),
                    span: dummy_span(),
                }),
                body: Some(Block {
                    stmts: vec![Stmt::Return {
                        value: Some(string_lit("hello")),
                        span: dummy_span(),
                    }],
                    span: dummy_span(),
                }),
                public: false,
        is_async: false,
                annotations: vec![],
                doc_comments: vec![],
                span: dummy_span(),
            },
            Decl::Function {
                name: "main".into(),
                generics: vec![],
                params: vec![],
                ret_ty: None,
                body: Some(Block {
                    stmts: vec![
                        let_move("x", call("create", vec![])),
                        let_move("a", ident("x")),
                        expr_stmt(call("use", vec![ident("x")])),
                    ],
                    span: dummy_span(),
                }),
                public: false,
        is_async: false,
                annotations: vec![],
                doc_comments: vec![],
                span: dummy_span(),
            },
        ],
        span: dummy_span(),
    };
    let result = analyze_ownership(&module);
    assert!(
        result.errors.iter().any(|e| e.message.contains("use of moved value")),
        "fn call returning non-copy type should move, got: {:?}",
        result.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: MethodCall returning copy type ───────────────────────

#[test]
fn method_call_len_is_copy() {
    // let s = "hello"
    // let n = s.len()     ← .len() returns usize (copy)
    // let m = n           ← copy
    // use(n)              ← OK: n is still alive
    let module = module_with_fn(vec![
        let_move("s", string_lit("hello")),
        let_move(
            "n",
            Expr::MethodCall {
                object: Box::new(ident("s")),
                method: "len".into(),
                args: vec![],
                span: dummy_span(),
            },
        ),
        let_move("m", ident("n")),
        expr_stmt(call("use", vec![ident("n")])),
    ]);
    let result = analyze_ownership(&module);
    let move_errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.message.contains("use of moved value"))
        .collect();
    assert!(
        move_errors.is_empty(),
        ".len() should return copy type, got: {:?}",
        move_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: Computed expression passed as fn arg is copy ─────────

#[test]
fn binary_op_as_fn_arg_is_copy() {
    // let a: i64 = 10
    // let b: i64 = 20
    // func(a + b)       ← a+b is copy, so a and b are used (not moved)
    // use(a)            ← OK
    // use(b)            ← OK
    let module = module_with_fn(vec![
        let_typed("a", "i64", int_lit(10)),
        let_typed("b", "i64", int_lit(20)),
        expr_stmt(call(
            "func",
            vec![Expr::BinaryOp {
                op: BinOp::Add,
                left: Box::new(ident("a")),
                right: Box::new(ident("b")),
                span: dummy_span(),
            }],
        )),
        expr_stmt(call("use", vec![ident("a")])),
        expr_stmt(call("use", vec![ident("b")])),
    ]);
    let result = analyze_ownership(&module);
    let move_errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.message.contains("use of moved value"))
        .collect();
    assert!(
        move_errors.is_empty(),
        "binary op passed as arg should be copy, got: {:?}",
        move_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: MatchExpr returning copy type ────────────────────────

#[test]
fn match_expr_copy_result() {
    // let x: i64 = 1
    // let y = match x { 1 => 10, _ => 20 }   ← all arms return i64 (copy)
    // let z = y                                ← copy
    // use(y)                                   ← OK
    let module = module_with_fn(vec![
        let_typed("x", "i64", int_lit(1)),
        let_move(
            "y",
            Expr::MatchExpr {
                subject: Box::new(ident("x")),
                arms: vec![
                    MatchArm {
                        pattern: Pattern::Literal {
                            expr: Box::new(int_lit(1)),
                            span: dummy_span(),
                        },
                        guard: None,
                        body: Box::new(int_lit(10)),
                        span: dummy_span(),
                    },
                    MatchArm {
                        pattern: Pattern::Wildcard { span: dummy_span() },
                        guard: None,
                        body: Box::new(int_lit(20)),
                        span: dummy_span(),
                    },
                ],
                span: dummy_span(),
            },
        ),
        let_move("z", ident("y")),
        expr_stmt(call("use", vec![ident("y")])),
    ]);
    let result = analyze_ownership(&module);
    let move_errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.message.contains("use of moved value"))
        .collect();
    assert!(
        move_errors.is_empty(),
        "match returning copy values should produce copy, got: {:?}",
        move_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: IfExpr returning copy type ───────────────────────────

#[test]
fn if_expr_copy_result() {
    // let cond: bool = true
    // let x = if cond { 10 } else { 20 }   ← both branches return i64 (copy)
    // let y = x                              ← copy
    // use(x)                                 ← OK
    let module = module_with_fn(vec![
        let_typed(
            "cond",
            "bool",
            Expr::BoolLiteral {
                value: true,
                span: dummy_span(),
            },
        ),
        let_move(
            "x",
            Expr::IfExpr {
                condition: Box::new(ident("cond")),
                then_branch: Block {
                    stmts: vec![Stmt::Expr {
                        expr: int_lit(10),
                        span: dummy_span(),
                    }],
                    span: dummy_span(),
                },
                else_branch: Some(Block {
                    stmts: vec![Stmt::Expr {
                        expr: int_lit(20),
                        span: dummy_span(),
                    }],
                    span: dummy_span(),
                }),
                span: dummy_span(),
            },
        ),
        let_move("y", ident("x")),
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
        "if expr returning copy values should produce copy, got: {:?}",
        move_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ── Test: Builtin fn call is copy ──────────────────────────────

#[test]
fn builtin_fn_call_is_copy() {
    // let x = len(arr)    ← builtin len returns usize (copy)
    // let y = x           ← copy
    // use(x)              ← OK
    let module = module_with_fn(vec![
        let_move("arr", string_lit("hello")),
        let_move("x", call("len", vec![ident("arr")])),
        let_move("y", ident("x")),
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
        "builtin len() should return copy type, got: {:?}",
        move_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}
