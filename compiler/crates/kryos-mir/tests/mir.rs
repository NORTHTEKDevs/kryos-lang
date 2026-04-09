//! Integration tests for kryos-mir: lowering, CFG construction, ARC ops, drops.
#![allow(clippy::approx_constant, clippy::match_like_matches_macro)]

use kryos_errors::Span;
use kryos_ast::{
    self as ast,
    expr::{BinOp, UnOp, Param, MatchArm, Pattern},
    stmt::{Block, Stmt},
    decl::{Decl, MessageHandler, Module, StructField},
    types::TypeExpr,
};
use kryos_mir::{
    ir::*,
    lower::lower_module,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const S: Span = Span::DUMMY;

fn simple_ty(name: &str) -> TypeExpr {
    TypeExpr::Simple {
        name: name.to_string(),
        span: S,
    }
}

fn int_lit(value: i64) -> ast::Expr {
    ast::Expr::IntLiteral { value, span: S }
}

fn float_lit(value: f64) -> ast::Expr {
    ast::Expr::FloatLiteral { value, span: S }
}

fn bool_lit(value: bool) -> ast::Expr {
    ast::Expr::BoolLiteral { value, span: S }
}

fn str_lit(value: &str) -> ast::Expr {
    ast::Expr::StringLiteral {
        value: value.to_string(),
        span: S,
    }
}

fn ident(name: &str) -> ast::Expr {
    ast::Expr::Identifier {
        name: name.to_string(),
        span: S,
    }
}

fn block(stmts: Vec<Stmt>) -> Block {
    Block { stmts, span: S }
}

fn expr_stmt(expr: ast::Expr) -> Stmt {
    Stmt::Expr { expr: expr.clone(), span: expr.span() }
}

fn make_module(decls: Vec<Decl>) -> Module {
    Module {
        name: "test".to_string(),
        declarations: decls,
        span: S,
    }
}

fn make_fn(name: &str, params: Vec<Param>, ret_ty: Option<TypeExpr>, body: Block) -> Decl {
    Decl::Function {
        name: name.to_string(),
        generics: vec![],
        params,
        ret_ty,
        body: Some(body),
        public: false,
        is_async: false,
        annotations: vec![],
        doc_comments: vec![],
        span: S,
    }
}

fn make_generic_fn(
    name: &str,
    generics: Vec<ast::decl::GenericParam>,
    params: Vec<Param>,
    ret_ty: Option<TypeExpr>,
    body: Block,
) -> Decl {
    Decl::Function {
        name: name.to_string(),
        generics,
        params,
        ret_ty,
        body: Some(body),
        public: false,
        is_async: false,
        annotations: vec![],
        doc_comments: vec![],
        span: S,
    }
}

fn make_generic_param(name: &str) -> ast::decl::GenericParam {
    ast::decl::GenericParam {
        name: name.to_string(),
        bounds: vec![],
        span: S,
    }
}

fn make_param(name: &str, ty: &str) -> Param {
    Param {
        name: name.to_string(),
        ty: Some(simple_ty(ty)),
        default: None,
        span: S,
    }
}

// ---------------------------------------------------------------------------
// Test 1: Simple function lowers to one block with Return
// ---------------------------------------------------------------------------

#[test]
fn simple_function_one_block_return() {
    let module = make_module(vec![make_fn(
        "empty",
        vec![],
        None,
        block(vec![Stmt::Return { value: None, span: S }]),
    )]);

    let mir = lower_module(&module);
    assert_eq!(mir.functions.len(), 1);

    let f = &mir.functions[0];
    assert_eq!(f.name, "empty");
    // Return statement creates a new block after it, so we get 2 blocks.
    assert!(f.block_count() >= 1);

    // The first block should have a Return terminator.
    let entry = f.entry_block();
    assert!(matches!(entry.terminator, Terminator::Return(None)));
}

// ---------------------------------------------------------------------------
// Test 2: If/else produces branch + merge blocks
// ---------------------------------------------------------------------------

#[test]
fn if_else_produces_branch_merge() {
    let module = make_module(vec![make_fn(
        "check",
        vec![make_param("x", "i64")],
        Some(simple_ty("void")),
        block(vec![Stmt::If {
            condition: bool_lit(true),
            then_block: block(vec![Stmt::Return {
                value: Some(int_lit(1)),
                span: S,
            }]),
            elif_clauses: vec![],
            else_block: Some(block(vec![Stmt::Return {
                value: Some(int_lit(2)),
                span: S,
            }])),
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    // Should have at least 4 blocks: entry, then, else, merge.
    assert!(f.block_count() >= 4, "expected >= 4 blocks, got {}", f.block_count());

    // Entry block should end with Branch.
    let entry = f.entry_block();
    assert!(
        matches!(entry.terminator, Terminator::Branch { .. }),
        "expected Branch terminator on entry, got {:?}",
        entry.terminator
    );
}

// ---------------------------------------------------------------------------
// Test 3: While loop produces back-edge (Goto to header)
// ---------------------------------------------------------------------------

#[test]
fn while_loop_back_edge() {
    let module = make_module(vec![make_fn(
        "looper",
        vec![],
        None,
        block(vec![Stmt::While {
            condition: bool_lit(true),
            body: block(vec![expr_stmt(int_lit(42))]),
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    // Should have: entry -> goto header, header (branch), body (goto header), exit.
    assert!(f.block_count() >= 4, "expected >= 4 blocks for while, got {}", f.block_count());

    // Entry block should Goto the header.
    assert!(
        matches!(f.entry_block().terminator, Terminator::Goto(_)),
        "entry should Goto header"
    );

    // Find a block that has Goto back to the header (back-edge).
    let header_id = match f.entry_block().terminator {
        Terminator::Goto(id) => id,
        _ => panic!("expected Goto"),
    };

    let has_back_edge = f.blocks.iter().any(|bb| {
        bb.id != f.entry_block().id && matches!(bb.terminator, Terminator::Goto(target) if target == header_id)
    });
    assert!(has_back_edge, "should have a back-edge to header");
}

// ---------------------------------------------------------------------------
// Test 4: For loop desugars correctly
// ---------------------------------------------------------------------------

#[test]
fn for_loop_desugars() {
    let module = make_module(vec![make_fn(
        "iterate",
        vec![],
        None,
        block(vec![Stmt::For {
            parallel: false,
            pattern: Pattern::Ident {
                name: "item".to_string(),
                mutable: false,
                span: S,
            },
            iterable: ident("collection"),
            body: block(vec![expr_stmt(ident("item"))]),
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    // Should have blocks for: entry, header, body, exit + merge.
    assert!(f.block_count() >= 4, "expected >= 4 blocks for for-loop, got {}", f.block_count());

    // Should have a local named "_idx".
    let has_idx = f.locals.iter().any(|l| l.name.as_deref() == Some("_idx"));
    assert!(has_idx, "for loop should create _idx local");

    // Should have a local named "item".
    let has_item = f.locals.iter().any(|l| l.name.as_deref() == Some("item"));
    assert!(has_item, "for loop should create item local from pattern");

    // Should have a Call to "len".
    let has_len_call = f.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign { value: RValue::Call { func, .. }, .. } => func == "len",
            _ => false,
        })
    });
    assert!(has_len_call, "for loop should emit len() call");
}

// ---------------------------------------------------------------------------
// Test 5: Let binding -> Assign instruction
// ---------------------------------------------------------------------------

#[test]
fn let_binding_assign() {
    let module = make_module(vec![make_fn(
        "bind",
        vec![],
        None,
        block(vec![Stmt::Let {
            name: "x".to_string(),
            mutable: false,
            ty: Some(simple_ty("i64")),
            value: Some(int_lit(42)),
            pattern: None,
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    // Should have a local named "x".
    let x_local = f.locals.iter().find(|l| l.name.as_deref() == Some("x"));
    assert!(x_local.is_some(), "should have local 'x'");
    let x_id = x_local.unwrap().id;

    // The entry block should have an Assign instruction to x.
    let has_assign = f.entry_block().instructions.iter().any(|inst| {
        matches!(inst, Instruction::Assign { dest, value: RValue::ConstInt(42) } if *dest == x_id)
    });
    assert!(has_assign, "should have Assign {{ x = const 42 }}");
}

// ---------------------------------------------------------------------------
// Test 6: Function call -> Call RValue
// ---------------------------------------------------------------------------

#[test]
fn function_call_rvalue() {
    let module = make_module(vec![make_fn(
        "caller",
        vec![],
        None,
        block(vec![Stmt::Let {
            name: "result".to_string(),
            mutable: false,
            ty: None,
            value: Some(ast::Expr::FnCall {
                callee: Box::new(ident("foo")),
                args: vec![int_lit(1), int_lit(2)],
                span: S,
            }),
            pattern: None,
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    let has_call = f.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign {
                value: RValue::Call { func, args },
                ..
            } => func == "foo" && args.len() == 2,
            _ => false,
        })
    });
    assert!(has_call, "should have Call RValue to 'foo' with 2 args");
}

// ---------------------------------------------------------------------------
// Test 7: Binary op -> BinOp RValue
// ---------------------------------------------------------------------------

#[test]
fn binary_op_rvalue() {
    let module = make_module(vec![make_fn(
        "math",
        vec![],
        None,
        block(vec![Stmt::Let {
            name: "sum".to_string(),
            mutable: false,
            ty: None,
            value: Some(ast::Expr::BinaryOp {
                op: BinOp::Add,
                left: Box::new(int_lit(3)),
                right: Box::new(int_lit(4)),
                span: S,
            }),
            pattern: None,
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    let has_binop = f.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign {
                value: RValue::BinOp { op, .. },
                ..
            } => *op == MirBinOp::Add,
            _ => false,
        })
    });
    assert!(has_binop, "should have BinOp::Add RValue");
}

// ---------------------------------------------------------------------------
// Test 8: Constants lowered correctly
// ---------------------------------------------------------------------------

#[test]
fn constants_lowered() {
    let module = make_module(vec![make_fn(
        "consts",
        vec![],
        None,
        block(vec![
            Stmt::Let {
                name: "a".into(),
                mutable: false,
                ty: None,
                value: Some(int_lit(99)),
                pattern: None,
                span: S,
            },
            Stmt::Let {
                name: "b".into(),
                mutable: false,
                ty: None,
                value: Some(float_lit(3.14)),
                pattern: None,
                span: S,
            },
            Stmt::Let {
                name: "c".into(),
                mutable: false,
                ty: None,
                value: Some(bool_lit(true)),
                pattern: None,
                span: S,
            },
            Stmt::Let {
                name: "d".into(),
                mutable: false,
                ty: None,
                value: Some(str_lit("hello")),
                pattern: None,
                span: S,
            },
            Stmt::Let {
                name: "e".into(),
                mutable: false,
                ty: None,
                value: Some(ast::Expr::NoneLiteral { span: S }),
                pattern: None,
                span: S,
            },
        ]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    let insts: Vec<&Instruction> = f
        .blocks
        .iter()
        .flat_map(|bb| bb.instructions.iter())
        .collect();

    let has_int = insts.iter().any(|i| matches!(i, Instruction::Assign { value: RValue::ConstInt(99), .. }));
    let has_float = insts.iter().any(|i| matches!(i, Instruction::Assign { value: RValue::ConstFloat(v), .. } if (*v - 3.14).abs() < 0.001));
    let has_bool = insts.iter().any(|i| matches!(i, Instruction::Assign { value: RValue::ConstBool(true), .. }));
    let has_string = insts.iter().any(|i| matches!(i, Instruction::Assign { value: RValue::ConstString(s), .. } if s == "hello"));
    let has_none = insts.iter().any(|i| matches!(i, Instruction::Assign { value: RValue::ConstNone, .. }));

    assert!(has_int, "should have ConstInt(99)");
    assert!(has_float, "should have ConstFloat(3.14)");
    assert!(has_bool, "should have ConstBool(true)");
    assert!(has_string, "should have ConstString(\"hello\")");
    assert!(has_none, "should have ConstNone");
}

// ---------------------------------------------------------------------------
// Test 9: Drop at scope exit
// ---------------------------------------------------------------------------

#[test]
fn drop_at_scope_exit() {
    let module = make_module(vec![make_fn(
        "scoped",
        vec![],
        None,
        block(vec![
            Stmt::Let {
                name: "x".into(),
                mutable: false,
                ty: Some(simple_ty("i64")),
                value: Some(int_lit(1)),
                pattern: None,
                span: S,
            },
            Stmt::Let {
                name: "y".into(),
                mutable: false,
                ty: Some(simple_ty("i64")),
                value: Some(int_lit(2)),
                pattern: None,
                span: S,
            },
        ]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    // Should have Drop instructions for x and y (in reverse order).
    let drops: Vec<&Instruction> = f
        .blocks
        .iter()
        .flat_map(|bb| bb.instructions.iter())
        .filter(|i| matches!(i, Instruction::Drop { .. }))
        .collect();

    assert!(drops.len() >= 2, "should have at least 2 Drop instructions, got {}", drops.len());
}

// ---------------------------------------------------------------------------
// Test 10: ArcRetain / ArcRelease for shared values
// ---------------------------------------------------------------------------

#[test]
fn arc_retain_release_shared() {
    let module = make_module(vec![make_fn(
        "shared_test",
        vec![],
        None,
        block(vec![Stmt::Let {
            name: "s".into(),
            mutable: false,
            ty: None,
            value: Some(ast::Expr::SharedExpr {
                inner: Box::new(int_lit(42)),
                span: S,
            }),
            pattern: None,
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    // Should have ArcRetain for the shared value.
    let has_retain = f.blocks.iter().any(|bb| {
        bb.instructions
            .iter()
            .any(|i| matches!(i, Instruction::ArcRetain { .. }))
    });
    assert!(has_retain, "should have ArcRetain for shared value");

    // Should have ArcAlloc in the assignment RValue.
    let has_arc_alloc = f.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|i| match i {
            Instruction::Assign {
                value: RValue::ArcAlloc { .. },
                ..
            } => true,
            _ => false,
        })
    });
    assert!(has_arc_alloc, "should have ArcAlloc RValue for shared expr");
}

// ---------------------------------------------------------------------------
// Test 11: Nested if/else produces correct CFG
// ---------------------------------------------------------------------------

#[test]
fn nested_if_else_cfg() {
    let module = make_module(vec![make_fn(
        "nested",
        vec![],
        None,
        block(vec![Stmt::If {
            condition: bool_lit(true),
            then_block: block(vec![Stmt::If {
                condition: bool_lit(false),
                then_block: block(vec![expr_stmt(int_lit(1))]),
                elif_clauses: vec![],
                else_block: Some(block(vec![expr_stmt(int_lit(2))])),
                span: S,
            }]),
            elif_clauses: vec![],
            else_block: Some(block(vec![expr_stmt(int_lit(3))])),
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    // Nested if/else should produce more blocks than a flat if/else.
    // Outer: entry + then + else + merge = 4.
    // Inner: + then + else + merge = 3.
    // Total >= 7 blocks.
    assert!(
        f.block_count() >= 7,
        "nested if/else should produce >= 7 blocks, got {}",
        f.block_count()
    );

    // Count Branch terminators — should be at least 2 (outer + inner).
    let branch_count = f
        .blocks
        .iter()
        .filter(|bb| matches!(bb.terminator, Terminator::Branch { .. }))
        .count();
    assert!(
        branch_count >= 2,
        "should have >= 2 Branch terminators for nested if/else, got {}",
        branch_count
    );
}

// ---------------------------------------------------------------------------
// Test 12: Match -> Switch terminator
// ---------------------------------------------------------------------------

#[test]
fn match_produces_switch() {
    let module = make_module(vec![make_fn(
        "matcher",
        vec![make_param("val", "i64")],
        None,
        block(vec![expr_stmt(ast::Expr::MatchExpr {
            subject: Box::new(ident("val")),
            arms: vec![
                MatchArm {
                    pattern: Pattern::Literal {
                        expr: Box::new(int_lit(1)),
                        span: S,
                    },
                    guard: None,
                    body: Box::new(int_lit(10)),
                    span: S,
                },
                MatchArm {
                    pattern: Pattern::Literal {
                        expr: Box::new(int_lit(2)),
                        span: S,
                    },
                    guard: None,
                    body: Box::new(int_lit(20)),
                    span: S,
                },
                MatchArm {
                    pattern: Pattern::Wildcard { span: S },
                    guard: None,
                    body: Box::new(int_lit(0)),
                    span: S,
                },
            ],
            span: S,
        })]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    // Should have a Switch terminator somewhere.
    let has_switch = f.blocks.iter().any(|bb| {
        matches!(&bb.terminator, Terminator::Switch { targets, .. } if targets.len() == 2)
    });
    assert!(has_switch, "match should produce Switch terminator with 2 targets");
}

// ---------------------------------------------------------------------------
// Test 13: Display formatting (bonus — verifies pretty-print)
// ---------------------------------------------------------------------------

#[test]
fn display_formatting() {
    let module = make_module(vec![make_fn(
        "display_test",
        vec![make_param("x", "i64")],
        Some(simple_ty("i64")),
        block(vec![Stmt::Return {
            value: Some(ident("x")),
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let output = format!("{}", mir);

    assert!(output.contains("fn display_test"), "display should contain function name");
    assert!(output.contains("return"), "display should contain return");
    assert!(output.contains("bb0:"), "display should contain block labels");
}

// ---------------------------------------------------------------------------
// Test 14: Unary op lowering
// ---------------------------------------------------------------------------

#[test]
fn unary_op_lowering() {
    let module = make_module(vec![make_fn(
        "negate",
        vec![],
        None,
        block(vec![Stmt::Let {
            name: "neg".into(),
            mutable: false,
            ty: None,
            value: Some(ast::Expr::UnaryOp {
                op: UnOp::Neg,
                operand: Box::new(int_lit(5)),
                span: S,
            }),
            pattern: None,
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    let has_unop = f.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign {
                value: RValue::UnOp { op, .. },
                ..
            } => *op == MirUnOp::Neg,
            _ => false,
        })
    });
    assert!(has_unop, "should have UnOp::Neg RValue");
}

// ---------------------------------------------------------------------------
// Test 15: Type lowering from TypeExpr
// ---------------------------------------------------------------------------

#[test]
fn type_lowering() {
    use kryos_mir::lower::lower_type_expr;

    assert_eq!(lower_type_expr(&simple_ty("i32")), MirType::I32);
    assert_eq!(lower_type_expr(&simple_ty("bool")), MirType::Bool);
    assert_eq!(lower_type_expr(&simple_ty("str")), MirType::Str);
    assert_eq!(lower_type_expr(&simple_ty("void")), MirType::Void);
    assert_eq!(
        lower_type_expr(&TypeExpr::Array {
            element: Box::new(simple_ty("i64")),
            size: Some(10),
            span: S,
        }),
        MirType::Array(Box::new(MirType::I64), Some(10))
    );
    assert_eq!(
        lower_type_expr(&TypeExpr::Shared {
            inner: Box::new(simple_ty("i32")),
            span: S,
        }),
        MirType::Shared(Box::new(MirType::I32))
    );
}

// ---------------------------------------------------------------------------
// Test 16: Block successors
// ---------------------------------------------------------------------------

#[test]
fn block_successors() {
    let bb_return = BasicBlock {
        id: BlockId(0),
        instructions: vec![],
        terminator: Terminator::Return(None),
    };
    assert_eq!(bb_return.successors().len(), 0);

    let bb_goto = BasicBlock {
        id: BlockId(1),
        instructions: vec![],
        terminator: Terminator::Goto(BlockId(5)),
    };
    assert_eq!(bb_goto.successors(), vec![BlockId(5)]);

    let bb_branch = BasicBlock {
        id: BlockId(2),
        instructions: vec![],
        terminator: Terminator::Branch {
            cond: Operand::Constant(Constant::Bool(true)),
            then_block: BlockId(3),
            else_block: BlockId(4),
        },
    };
    assert_eq!(bb_branch.successors(), vec![BlockId(3), BlockId(4)]);

    let bb_switch = BasicBlock {
        id: BlockId(3),
        instructions: vec![],
        terminator: Terminator::Switch {
            value: Operand::Constant(Constant::Int(0)),
            targets: vec![(1, BlockId(10)), (2, BlockId(11))],
            default: BlockId(12),
        },
    };
    assert_eq!(
        bb_switch.successors(),
        vec![BlockId(10), BlockId(11), BlockId(12)]
    );
}

// ---------------------------------------------------------------------------
// Test 17: Generic identity function — monomorphization
// ---------------------------------------------------------------------------

#[test]
fn generic_identity_monomorphized() {
    // fn id<T>(x: T) -> T { x }
    // fn main() { let a = id(42); let b = id(3.14); }
    let module = make_module(vec![
        make_generic_fn(
            "id",
            vec![make_generic_param("T")],
            vec![Param {
                name: "x".into(),
                ty: Some(simple_ty("T")),
                default: None,
                span: S,
            }],
            Some(simple_ty("T")),
            block(vec![Stmt::Return {
                value: Some(ident("x")),
                span: S,
            }]),
        ),
        make_fn(
            "main",
            vec![],
            None,
            block(vec![
                Stmt::Let {
                    name: "a".into(),
                    mutable: false,
                    ty: None,
                    value: Some(ast::Expr::FnCall {
                        callee: Box::new(ident("id")),
                        args: vec![int_lit(42)],
                        span: S,
                    }),
                    pattern: None,
                    span: S,
                },
                Stmt::Let {
                    name: "b".into(),
                    mutable: false,
                    ty: None,
                    value: Some(ast::Expr::FnCall {
                        callee: Box::new(ident("id")),
                        args: vec![float_lit(3.14)],
                        span: S,
                    }),
                    pattern: None,
                    span: S,
                },
            ]),
        ),
    ]);

    let mir = lower_module(&module);

    // The generic `id` should NOT appear as a function.
    assert!(
        !mir.functions.iter().any(|f| f.name == "id"),
        "generic template 'id' should not be in output functions"
    );

    // Instead, we should have monomorphized specializations.
    let has_id_i64 = mir.functions.iter().any(|f| f.name == "id___i64");
    let has_id_f64 = mir.functions.iter().any(|f| f.name == "id___f64");
    assert!(has_id_i64, "should have monomorphized id___i64");
    assert!(has_id_f64, "should have monomorphized id___f64");

    // main should exist and call the mangled names.
    let main_fn = mir.functions.iter().find(|f| f.name == "main").unwrap();
    let calls: Vec<String> = main_fn
        .blocks
        .iter()
        .flat_map(|bb| bb.instructions.iter())
        .filter_map(|inst| match inst {
            Instruction::Assign { value: RValue::Call { func, .. }, .. } => Some(func.clone()),
            _ => None,
        })
        .collect();
    assert!(calls.contains(&"id___i64".to_string()), "main should call id___i64");
    assert!(calls.contains(&"id___f64".to_string()), "main should call id___f64");
}

// ---------------------------------------------------------------------------
// Test 18: Generic function with multiple type params
// ---------------------------------------------------------------------------

#[test]
fn generic_two_type_params() {
    // fn pair<A, B>(a: A, b: B) -> A { a }
    // fn main() { let x = pair(1, true); }
    let module = make_module(vec![
        make_generic_fn(
            "pair",
            vec![make_generic_param("A"), make_generic_param("B")],
            vec![
                Param { name: "a".into(), ty: Some(simple_ty("A")), default: None, span: S },
                Param { name: "b".into(), ty: Some(simple_ty("B")), default: None, span: S },
            ],
            Some(simple_ty("A")),
            block(vec![Stmt::Return { value: Some(ident("a")), span: S }]),
        ),
        make_fn(
            "main",
            vec![],
            None,
            block(vec![Stmt::Let {
                name: "x".into(),
                mutable: false,
                ty: None,
                value: Some(ast::Expr::FnCall {
                    callee: Box::new(ident("pair")),
                    args: vec![int_lit(1), bool_lit(true)],
                    span: S,
                }),
                pattern: None,
                span: S,
            }]),
        ),
    ]);

    let mir = lower_module(&module);

    // Should produce pair___i64_bool.
    let has_specialized = mir.functions.iter().any(|f| f.name == "pair___i64_bool");
    assert!(has_specialized, "should have monomorphized pair___i64_bool");
}

// ---------------------------------------------------------------------------
// Test 19: Trait definition tracked in lowering context
// ---------------------------------------------------------------------------

#[test]
fn trait_definition_tracked() {
    // trait Printable { fn print(self) -> void }
    // fn main() {}
    let module = make_module(vec![
        Decl::Trait {
            name: "Printable".into(),
            generics: vec![],
            methods: vec![Decl::Function {
                name: "print".into(),
                generics: vec![],
                params: vec![Param {
                    name: "self".into(),
                    ty: None,
                    default: None,
                    span: S,
                }],
                ret_ty: None,
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
        make_fn("main", vec![], None, block(vec![])),
    ]);

    let mir = lower_module(&module);
    // Should compile without errors — trait decls don't produce functions.
    assert_eq!(mir.functions.len(), 1); // only main
    assert_eq!(mir.functions[0].name, "main");
}

// ---------------------------------------------------------------------------
// Test 20: Impl for trait registers mangled methods
// ---------------------------------------------------------------------------

#[test]
fn impl_for_trait_methods() {
    // trait Greetable { fn greet(self) -> i64 }
    // struct Dog { age: i64 }
    // impl Greetable for Dog { fn greet(self) -> i64 { 42 } }
    // fn main() {}
    let module = make_module(vec![
        Decl::Trait {
            name: "Greetable".into(),
            generics: vec![],
            methods: vec![Decl::Function {
                name: "greet".into(),
                generics: vec![],
                params: vec![Param { name: "self".into(), ty: None, default: None, span: S }],
                ret_ty: Some(simple_ty("i64")),
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
        Decl::Struct {
            name: "Dog".into(),
            generics: vec![],
            fields: vec![ast::decl::StructField {
                name: "age".into(),
                ty: simple_ty("i64"),
                public: false,
                default: None,
                span: S,
            }],
            public: false,
            annotations: vec![],
            doc_comments: vec![],
            span: S,
        },
        Decl::Impl {
            target: "Dog".into(),
            trait_name: Some("Greetable".into()),
            generics: vec![],
            methods: vec![Decl::Function {
                name: "greet".into(),
                generics: vec![],
                params: vec![Param { name: "self".into(), ty: None, default: None, span: S }],
                ret_ty: Some(simple_ty("i64")),
                body: Some(block(vec![Stmt::Return { value: Some(int_lit(42)), span: S }])),
                public: false,
                is_async: false,
                annotations: vec![],
                doc_comments: vec![],
                span: S,
            }],
            doc_comments: vec![],
            span: S,
        },
        make_fn("main", vec![], None, block(vec![])),
    ]);

    let mir = lower_module(&module);

    // Should have Dog__greet as a function.
    let has_greet = mir.functions.iter().any(|f| f.name == "Dog__greet");
    assert!(has_greet, "should have Dog__greet from impl Greetable for Dog");

    // Should have main.
    let has_main = mir.functions.iter().any(|f| f.name == "main");
    assert!(has_main, "should have main");
}

// ---------------------------------------------------------------------------
// Test 21: Deduplicated monomorphization (same specialization called twice)
// ---------------------------------------------------------------------------

#[test]
fn monomorphization_deduplication() {
    // fn id<T>(x: T) -> T { x }
    // fn main() { let a = id(1); let b = id(2); }
    // Both calls use i64 — should only produce one id___i64.
    let module = make_module(vec![
        make_generic_fn(
            "id",
            vec![make_generic_param("T")],
            vec![Param { name: "x".into(), ty: Some(simple_ty("T")), default: None, span: S }],
            Some(simple_ty("T")),
            block(vec![Stmt::Return { value: Some(ident("x")), span: S }]),
        ),
        make_fn(
            "main",
            vec![],
            None,
            block(vec![
                Stmt::Let {
                    name: "a".into(), mutable: false, ty: None,
                    value: Some(ast::Expr::FnCall {
                        callee: Box::new(ident("id")),
                        args: vec![int_lit(1)],
                        span: S,
                    }),
                    pattern: None, span: S,
                },
                Stmt::Let {
                    name: "b".into(), mutable: false, ty: None,
                    value: Some(ast::Expr::FnCall {
                        callee: Box::new(ident("id")),
                        args: vec![int_lit(2)],
                        span: S,
                    }),
                    pattern: None, span: S,
                },
            ]),
        ),
    ]);

    let mir = lower_module(&module);

    let id_i64_count = mir.functions.iter().filter(|f| f.name == "id___i64").count();
    assert_eq!(id_i64_count, 1, "should only have one id___i64, not duplicates");
}

// ---------------------------------------------------------------------------
// Test 22: Option::Some and Option::None as prelude enums
// ---------------------------------------------------------------------------

#[test]
fn option_some_none_prelude() {
    // fn main() { let x = Some(42); let y = None; }
    let module = make_module(vec![make_fn(
        "main",
        vec![],
        None,
        block(vec![
            Stmt::Let {
                name: "x".into(),
                mutable: false,
                ty: None,
                value: Some(ast::Expr::FnCall {
                    callee: Box::new(ident("Some")),
                    args: vec![int_lit(42)],
                    span: S,
                }),
                pattern: None,
                span: S,
            },
            Stmt::Let {
                name: "y".into(),
                mutable: false,
                ty: None,
                value: Some(ident("None")),
                pattern: None,
                span: S,
            },
        ]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    // `Some(42)` should lower to EnumVariant { enum_name: "Option", variant_idx: 0 }
    let has_some = f.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign {
                value: RValue::EnumVariant { enum_name, variant_idx, fields },
                ..
            } => enum_name == "Option" && *variant_idx == 0 && fields.len() == 1,
            _ => false,
        })
    });
    assert!(has_some, "should have EnumVariant for Some(42)");

    // `None` should lower to EnumVariant { enum_name: "Option", variant_idx: 1, fields: [] }
    let has_none = f.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign {
                value: RValue::EnumVariant { enum_name, variant_idx, fields },
                ..
            } => enum_name == "Option" && *variant_idx == 1 && fields.is_empty(),
            _ => false,
        })
    });
    assert!(has_none, "should have EnumVariant for None");
}

// ---------------------------------------------------------------------------
// Test 23: Result::Ok and Result::Err as prelude enums
// ---------------------------------------------------------------------------

#[test]
fn result_ok_err_prelude() {
    // fn main() { let x = Ok(1); let y = Err(42); }
    let module = make_module(vec![make_fn(
        "main",
        vec![],
        None,
        block(vec![
            Stmt::Let {
                name: "x".into(),
                mutable: false,
                ty: None,
                value: Some(ast::Expr::FnCall {
                    callee: Box::new(ident("Ok")),
                    args: vec![int_lit(1)],
                    span: S,
                }),
                pattern: None,
                span: S,
            },
            Stmt::Let {
                name: "y".into(),
                mutable: false,
                ty: None,
                value: Some(ast::Expr::FnCall {
                    callee: Box::new(ident("Err")),
                    args: vec![int_lit(42)],
                    span: S,
                }),
                pattern: None,
                span: S,
            },
        ]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    let has_ok = f.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign {
                value: RValue::EnumVariant { enum_name, variant_idx, .. },
                ..
            } => enum_name == "Result" && *variant_idx == 0,
            _ => false,
        })
    });
    assert!(has_ok, "should have EnumVariant for Ok(1)");

    let has_err = f.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign {
                value: RValue::EnumVariant { enum_name, variant_idx, .. },
                ..
            } => enum_name == "Result" && *variant_idx == 1,
            _ => false,
        })
    });
    assert!(has_err, "should have EnumVariant for Err(42)");
}

// ---------------------------------------------------------------------------
// Test 24: try/catch produces Switch on Result tag
// ---------------------------------------------------------------------------

#[test]
fn try_catch_lowering() {
    // fn main() { try { 42 } catch e { -1 } }
    let module = make_module(vec![make_fn(
        "main",
        vec![],
        None,
        block(vec![Stmt::TryCatch {
            try_block: block(vec![expr_stmt(int_lit(42))]),
            catch_name: "e".into(),
            catch_block: block(vec![expr_stmt(int_lit(-1))]),
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    // Should have a Switch terminator (branching on Result tag).
    let has_switch = f.blocks.iter().any(|bb| {
        matches!(&bb.terminator, Terminator::Switch { .. })
    });
    assert!(has_switch, "try/catch should produce Switch on Result tag");

    // Should have a local named "e" (the catch binding).
    let has_e = f.locals.iter().any(|l| l.name.as_deref() == Some("e"));
    assert!(has_e, "catch should bind error to variable 'e'");

    // Should have EnumVariant for Result::Ok (wrapping try body result).
    let has_ok = f.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign {
                value: RValue::EnumVariant { enum_name, variant_idx, .. },
                ..
            } => enum_name == "Result" && *variant_idx == 0,
            _ => false,
        })
    });
    assert!(has_ok, "try body should be wrapped in Result::Ok");

    // Should have EnumPayload extraction (for the catch arm).
    let has_err_extract = f.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign {
                value: RValue::EnumPayload { variant_idx: 1, .. },
                ..
            } => true,
            _ => false,
        })
    });
    assert!(has_err_extract, "catch arm should extract Err payload");
}

// ---------------------------------------------------------------------------
// Test 25: throw outside try calls kryos_exception_throw and returns
// ---------------------------------------------------------------------------

#[test]
fn throw_produces_result_err() {
    // fn main() { throw 99 }
    let module = make_module(vec![make_fn(
        "main",
        vec![],
        None,
        block(vec![Stmt::Throw {
            expr: int_lit(99),
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    // throw outside a try block should emit a call to kryos_exception_throw.
    let has_throw_call = f.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign {
                value: RValue::Call { func, .. },
                ..
            } => func == "kryos_exception_throw",
            _ => false,
        })
    });
    assert!(has_throw_call, "throw should call kryos_exception_throw");

    // The block containing the throw call should terminate with Return.
    let has_return = f.blocks.iter().any(|bb| {
        let has_call = bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign {
                value: RValue::Call { func, .. },
                ..
            } => func == "kryos_exception_throw",
            _ => false,
        });
        has_call && matches!(bb.terminator, Terminator::Return(_))
    });
    assert!(has_return, "throw should return after kryos_exception_throw");
}

// ---------------------------------------------------------------------------
// Test 26: Lambda creates anonymous function + Closure RValue
// ---------------------------------------------------------------------------

#[test]
fn lambda_creates_closure() {
    // fn main() { let f = |x: i64| -> i64 { x }; }
    let module = make_module(vec![make_fn(
        "main",
        vec![],
        None,
        block(vec![Stmt::Let {
            name: "f".into(),
            mutable: false,
            ty: None,
            value: Some(ast::Expr::Lambda {
                params: vec![Param {
                    name: "x".into(),
                    ty: Some(simple_ty("i64")),
                    default: None,
                    span: S,
                }],
                ret_ty: Some(simple_ty("i64")),
                body: Box::new(ident("x")),
                span: S,
            }),
            pattern: None,
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);

    // Should have the anonymous lambda function __lambda_0.
    let has_lambda = mir.functions.iter().any(|f| f.name.starts_with("__lambda_"));
    assert!(has_lambda, "lambda should produce an anonymous __lambda_N function");

    // main should have a Closure rvalue.
    let main_fn = mir.functions.iter().find(|f| f.name == "main").unwrap();
    let has_closure = main_fn.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign {
                value: RValue::Closure { func_name, .. },
                ..
            } => func_name.starts_with("__lambda_"),
            _ => false,
        })
    });
    assert!(has_closure, "main should assign a Closure rvalue for the lambda");
}

// ---------------------------------------------------------------------------
// Test 27: Lambda with captures includes free variables
// ---------------------------------------------------------------------------

#[test]
fn lambda_captures_free_variables() {
    // fn main() { let y = 10; let f = |x: i64| -> i64 { x }; }
    // Note: `y` is not referenced in the lambda body, so captures should be empty.
    // A lambda that references an outer variable would capture it.
    let module = make_module(vec![make_fn(
        "main",
        vec![],
        None,
        block(vec![
            Stmt::Let {
                name: "y".into(),
                mutable: false,
                ty: None,
                value: Some(int_lit(10)),
                pattern: None,
                span: S,
            },
            Stmt::Let {
                name: "f".into(),
                mutable: false,
                ty: None,
                value: Some(ast::Expr::Lambda {
                    params: vec![Param {
                        name: "x".into(),
                        ty: Some(simple_ty("i64")),
                        default: None,
                        span: S,
                    }],
                    ret_ty: Some(simple_ty("i64")),
                    body: Box::new(ident("x")),
                    span: S,
                }),
                pattern: None,
                span: S,
            },
        ]),
    )]);

    let mir = lower_module(&module);

    // Lambda without captures: Closure should have empty captures vec.
    let main_fn = mir.functions.iter().find(|f| f.name == "main").unwrap();
    let closure_captures: Vec<usize> = main_fn
        .blocks
        .iter()
        .flat_map(|bb| bb.instructions.iter())
        .filter_map(|inst| match inst {
            Instruction::Assign {
                value: RValue::Closure { captures, .. },
                ..
            } => Some(captures.len()),
            _ => None,
        })
        .collect();
    assert_eq!(closure_captures, vec![0], "lambda not referencing outer vars should have 0 captures");
}

// ---------------------------------------------------------------------------
// Test 28: Pipe expression desugars to function call
// ---------------------------------------------------------------------------

#[test]
fn pipe_expression_desugars() {
    // fn double(x: i64) -> i64 { x }
    // fn main() { let r = 5 |> double; }
    let module = make_module(vec![
        make_fn(
            "double",
            vec![Param {
                name: "x".into(),
                ty: Some(simple_ty("i64")),
                default: None,
                span: S,
            }],
            Some(simple_ty("i64")),
            block(vec![Stmt::Return {
                value: Some(ident("x")),
                span: S,
            }]),
        ),
        make_fn(
            "main",
            vec![],
            None,
            block(vec![Stmt::Let {
                name: "r".into(),
                mutable: false,
                ty: None,
                value: Some(ast::Expr::PipeExpr {
                    left: Box::new(int_lit(5)),
                    right: Box::new(ident("double")),
                    span: S,
                }),
                pattern: None,
                span: S,
            }]),
        ),
    ]);

    let mir = lower_module(&module);
    let main_fn = mir.functions.iter().find(|f| f.name == "main").unwrap();

    // Pipe `5 |> double` should desugar to `call double(5)`.
    let calls: Vec<(String, usize)> = main_fn
        .blocks
        .iter()
        .flat_map(|bb| bb.instructions.iter())
        .filter_map(|inst| match inst {
            Instruction::Assign {
                value: RValue::Call { func, args },
                ..
            } => Some((func.clone(), args.len())),
            _ => None,
        })
        .collect();
    let has_double_call = calls.iter().any(|(name, nargs)| name == "double" && *nargs == 1);
    assert!(has_double_call, "pipe should desugar to call double(5) with 1 arg");
}

// ---------------------------------------------------------------------------
// Test 29: Pipe expression with args: a |> f(b) -> f(a, b)
// ---------------------------------------------------------------------------

#[test]
fn pipe_expression_with_args() {
    // fn add(a: i64, b: i64) -> i64 { a }
    // fn main() { let r = 1 |> add(2); }
    let module = make_module(vec![
        make_fn(
            "add",
            vec![
                Param { name: "a".into(), ty: Some(simple_ty("i64")), default: None, span: S },
                Param { name: "b".into(), ty: Some(simple_ty("i64")), default: None, span: S },
            ],
            Some(simple_ty("i64")),
            block(vec![Stmt::Return { value: Some(ident("a")), span: S }]),
        ),
        make_fn(
            "main",
            vec![],
            None,
            block(vec![Stmt::Let {
                name: "r".into(),
                mutable: false,
                ty: None,
                value: Some(ast::Expr::PipeExpr {
                    left: Box::new(int_lit(1)),
                    right: Box::new(ast::Expr::FnCall {
                        callee: Box::new(ident("add")),
                        args: vec![int_lit(2)],
                        span: S,
                    }),
                    span: S,
                }),
                pattern: None,
                span: S,
            }]),
        ),
    ]);

    let mir = lower_module(&module);
    let main_fn = mir.functions.iter().find(|f| f.name == "main").unwrap();

    // `1 |> add(2)` should desugar to `call add(1, 2)` — 2 args.
    let calls: Vec<(String, usize)> = main_fn
        .blocks
        .iter()
        .flat_map(|bb| bb.instructions.iter())
        .filter_map(|inst| match inst {
            Instruction::Assign {
                value: RValue::Call { func, args },
                ..
            } => Some((func.clone(), args.len())),
            _ => None,
        })
        .collect();
    let has_add_2args = calls.iter().any(|(name, nargs)| name == "add" && *nargs == 2);
    assert!(has_add_2args, "pipe with args should desugar to call add(1, 2) with 2 args");
}

// ---------------------------------------------------------------------------
// Test 30: Type alias tracked (doesn't produce function)
// ---------------------------------------------------------------------------

#[test]
fn type_alias_tracked() {
    // type Num = i64
    // fn main() {}
    let module = make_module(vec![
        Decl::TypeAlias {
            name: "Num".into(),
            generics: vec![],
            ty: simple_ty("i64"),
            public: false,
            span: S,
        },
        make_fn("main", vec![], None, block(vec![])),
    ]);

    let mir = lower_module(&module);
    // Type aliases don't produce functions — only main should exist.
    assert_eq!(mir.functions.len(), 1);
    assert_eq!(mir.functions[0].name, "main");
}

// ---------------------------------------------------------------------------
// Test 31: Extern block registers function signatures
// ---------------------------------------------------------------------------

#[test]
fn extern_block_registers_signatures() {
    // extern "C" { fn puts(s: str) -> i64; }
    // fn main() { puts("hello"); }
    let module = make_module(vec![
        Decl::Extern {
            abi: "C".into(),
            items: vec![Decl::Function {
                name: "puts".into(),
                generics: vec![],
                params: vec![Param {
                    name: "s".into(),
                    ty: Some(simple_ty("str")),
                    default: None,
                    span: S,
                }],
                ret_ty: Some(simple_ty("i64")),
                body: None,
                public: false,
                is_async: false,
                annotations: vec![],
                doc_comments: vec![],
                span: S,
            }],
            span: S,
        },
        make_fn(
            "main",
            vec![],
            None,
            block(vec![expr_stmt(ast::Expr::FnCall {
                callee: Box::new(ident("puts")),
                args: vec![str_lit("hello")],
                span: S,
            })]),
        ),
    ]);

    let mir = lower_module(&module);

    // main should call `puts` — it's an extern, so no function body is emitted for it.
    let main_fn = mir.functions.iter().find(|f| f.name == "main").unwrap();
    let calls: Vec<String> = main_fn
        .blocks
        .iter()
        .flat_map(|bb| bb.instructions.iter())
        .filter_map(|inst| match inst {
            Instruction::Assign {
                value: RValue::Call { func, .. },
                ..
            } => Some(func.clone()),
            _ => None,
        })
        .collect();
    assert!(calls.contains(&"puts".to_string()), "main should call extern function puts");

    // `puts` should NOT appear as a lowered MIR function (it has no body).
    let has_puts_fn = mir.functions.iter().any(|f| f.name == "puts");
    assert!(!has_puts_fn, "extern function should not be emitted as MIR function");
}

// ---------------------------------------------------------------------------
// Test 32: Interpolated string produces StringConcat
// ---------------------------------------------------------------------------

#[test]
fn interpolated_string_lowering() {
    // fn main() { let s = f"hello {42} world"; }
    use ast::expr::StringPart;
    let module = make_module(vec![make_fn(
        "main",
        vec![],
        None,
        block(vec![Stmt::Let {
            name: "s".into(),
            mutable: false,
            ty: None,
            value: Some(ast::Expr::InterpolatedString {
                parts: vec![
                    StringPart::Literal("hello ".into()),
                    StringPart::Expr(Box::new(int_lit(42))),
                    StringPart::Literal(" world".into()),
                ],
                span: S,
            }),
            pattern: None,
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    // Should have a StringConcat rvalue with 3 parts.
    let has_concat = f.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign {
                value: RValue::StringConcat(parts),
                ..
            } => parts.len() == 3,
            _ => false,
        })
    });
    assert!(has_concat, "interpolated string should produce StringConcat with 3 parts");
}

// ---------------------------------------------------------------------------
// Test 33: Map literal produces Map RValue
// ---------------------------------------------------------------------------

#[test]
fn map_literal_lowering() {
    // fn main() { let m = { 1: "a", 2: "b" }; }
    let module = make_module(vec![make_fn(
        "main",
        vec![],
        None,
        block(vec![Stmt::Let {
            name: "m".into(),
            mutable: false,
            ty: None,
            value: Some(ast::Expr::MapLiteral {
                entries: vec![
                    (int_lit(1), str_lit("a")),
                    (int_lit(2), str_lit("b")),
                ],
                span: S,
            }),
            pattern: None,
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    // Should have a Map rvalue with 2 entries.
    let has_map = f.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign {
                value: RValue::Map(entries),
                ..
            } => entries.len() == 2,
            _ => false,
        })
    });
    assert!(has_map, "map literal should produce Map rvalue with 2 entries");
}

// ---------------------------------------------------------------------------
// Test 34: Char literal lowered as ConstInt
// ---------------------------------------------------------------------------

#[test]
fn char_literal_lowering() {
    // fn main() { let c = 'A'; }
    let module = make_module(vec![make_fn(
        "main",
        vec![],
        None,
        block(vec![Stmt::Let {
            name: "c".into(),
            mutable: false,
            ty: None,
            value: Some(ast::Expr::CharLiteral { value: 'A', span: S }),
            pattern: None,
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    // 'A' is 65 as i64.
    let has_char = f.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign {
                value: RValue::ConstInt(65),
                ..
            } => true,
            _ => false,
        })
    });
    assert!(has_char, "char literal 'A' should lower to ConstInt(65)");
}

// ---------------------------------------------------------------------------
// Test 35: Spawn statement emits Spawn instruction
// ---------------------------------------------------------------------------

#[test]
fn spawn_statement_emits_instruction() {
    // fn work() -> i64 { 42 }
    // fn main() { spawn work(); }
    let module = make_module(vec![
        make_fn(
            "work",
            vec![],
            Some(simple_ty("i64")),
            block(vec![Stmt::Return { value: Some(int_lit(42)), span: S }]),
        ),
        make_fn(
            "main",
            vec![],
            None,
            block(vec![Stmt::Spawn {
                expr: ast::Expr::FnCall {
                    callee: Box::new(ident("work")),
                    args: vec![],
                    span: S,
                },
                span: S,
            }]),
        ),
    ]);

    let mir = lower_module(&module);
    let main_fn = mir.functions.iter().find(|f| f.name == "main").unwrap();

    let has_spawn = main_fn.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| matches!(inst, Instruction::Spawn { .. }))
    });
    assert!(has_spawn, "spawn statement should produce Spawn instruction");
}

// ---------------------------------------------------------------------------
// Test 36: Actor declaration lowers handlers as mangled functions
// ---------------------------------------------------------------------------

#[test]
fn actor_handlers_lowered() {
    // actor Counter { count: i64; handle increment(n: i64) -> void { } }
    // fn main() {}
    use ast::decl::{StructField, MessageHandler};
    let module = make_module(vec![
        Decl::Actor {
            name: "Counter".into(),
            state_fields: vec![StructField {
                name: "count".into(),
                ty: simple_ty("i64"),
                public: false,
                default: None,
                span: S,
            }],
            handlers: vec![MessageHandler {
                name: "increment".into(),
                params: vec![Param {
                    name: "n".into(),
                    ty: Some(simple_ty("i64")),
                    default: None,
                    span: S,
                }],
                ret_ty: None,
                body: block(vec![]),
                span: S,
            }],
            annotations: vec![],
            span: S,
        },
        make_fn("main", vec![], None, block(vec![])),
    ]);

    let mir = lower_module(&module);

    // Actor handler should be lowered as Counter__increment.
    let has_handler = mir.functions.iter().any(|f| f.name == "Counter__increment");
    assert!(has_handler, "actor handler should produce Counter__increment function");

    // Actor state should be registered as a struct def.
    assert!(
        mir.struct_defs.contains_key("Counter"),
        "actor state should be registered as struct def"
    );

    // main should exist.
    assert!(mir.functions.iter().any(|f| f.name == "main"));
}

// ---------------------------------------------------------------------------
// Test 37: Select statement produces try_recv polling loop
// ---------------------------------------------------------------------------

#[test]
fn select_statement_produces_try_recv_polling() {
    // fn main() { select { msg from ch1 { } } }
    use ast::stmt::SelectBranch;
    let module = make_module(vec![make_fn(
        "main",
        vec![],
        None,
        block(vec![
            Stmt::Let {
                name: "ch1".into(),
                mutable: false,
                ty: None,
                value: Some(int_lit(0)),
                pattern: None,
                span: S,
            },
            Stmt::Select {
                branches: vec![SelectBranch {
                    pattern: "msg".into(),
                    channel: ident("ch1"),
                    body: block(vec![]),
                    span: S,
                }],
                span: S,
            },
        ]),
    )]);

    let mir = lower_module(&module);
    let main_fn = mir.functions.iter().find(|f| f.name == "main").unwrap();

    // Should have Branch terminators (try_recv status check + closed check).
    let branch_count = main_fn.blocks.iter().filter(|bb| {
        matches!(&bb.terminator, Terminator::Branch { .. })
    }).count();
    assert!(branch_count >= 2, "select should have Branch terminators for status check and closed check, got {branch_count}");

    // Should call kryos_chan_try_recv_status_i64 (not the old sentinel-based version).
    let has_try_recv_status = main_fn.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| matches!(inst, Instruction::Assign {
            value: RValue::Call { func, .. }, ..
        } if func == "kryos_chan_try_recv_status_i64"))
    });
    assert!(has_try_recv_status, "select should call kryos_chan_try_recv_status_i64");

    // Should call kryos_chan_last_recv_i64 to retrieve the value.
    let has_last_recv = main_fn.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| matches!(inst, Instruction::Assign {
            value: RValue::Call { func, .. }, ..
        } if func == "kryos_chan_last_recv_i64"))
    });
    assert!(has_last_recv, "select should call kryos_chan_last_recv_i64");

    // Should call kryos_chan_is_closed_i64 for closed-channel detection.
    let has_is_closed = main_fn.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| matches!(inst, Instruction::Assign {
            value: RValue::Call { func, .. }, ..
        } if func == "kryos_chan_is_closed_i64"))
    });
    assert!(has_is_closed, "select should call kryos_chan_is_closed_i64 for closed detection");

    // Should have Goto terminators for the poll loop back-edge.
    let goto_count = main_fn.blocks.iter().filter(|bb| {
        matches!(&bb.terminator, Terminator::Goto(_))
    }).count();
    assert!(goto_count >= 2, "select should have Goto terminators for poll loop");
}

// ---------------------------------------------------------------------------
// Test 38: Comptime block wraps inner RValue
// ---------------------------------------------------------------------------

#[test]
fn comptime_block_lowering() {
    // fn main() { let x = comptime { 42 }; }
    let module = make_module(vec![make_fn(
        "main",
        vec![],
        None,
        block(vec![Stmt::Let {
            name: "x".into(),
            mutable: false,
            ty: None,
            value: Some(ast::Expr::ComptimeBlock {
                body: block(vec![expr_stmt(int_lit(42))]),
                span: S,
            }),
            pattern: None,
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    // Should have a Comptime(ConstInt(42)) rvalue.
    let has_comptime = f.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign {
                value: RValue::Comptime(inner),
                ..
            } => matches!(inner.as_ref(), RValue::ConstInt(42)),
            _ => false,
        })
    });
    assert!(has_comptime, "comptime block should produce Comptime(ConstInt(42))");
}

// ---------------------------------------------------------------------------
// Test 39: Range expression lowering
// ---------------------------------------------------------------------------

#[test]
fn range_expression_lowering() {
    // fn main() { let r = 0..10; }
    let module = make_module(vec![make_fn(
        "main",
        vec![],
        None,
        block(vec![Stmt::Let {
            name: "r".into(),
            mutable: false,
            ty: None,
            value: Some(ast::Expr::RangeExpr {
                start: Some(Box::new(int_lit(0))),
                end: Some(Box::new(int_lit(10))),
                inclusive: false,
                span: S,
            }),
            pattern: None,
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    let has_range = f.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign {
                value: RValue::Range { start: Some(_), end: Some(_), inclusive: false },
                ..
            } => true,
            _ => false,
        })
    });
    assert!(has_range, "range expression should produce Range rvalue");
}

// ---------------------------------------------------------------------------
// Test 40: Inclusive range expression
// ---------------------------------------------------------------------------

#[test]
fn inclusive_range_expression() {
    // fn main() { let r = 1..=5; }
    let module = make_module(vec![make_fn(
        "main",
        vec![],
        None,
        block(vec![Stmt::Let {
            name: "r".into(),
            mutable: false,
            ty: None,
            value: Some(ast::Expr::RangeExpr {
                start: Some(Box::new(int_lit(1))),
                end: Some(Box::new(int_lit(5))),
                inclusive: true,
                span: S,
            }),
            pattern: None,
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let f = &mir.functions[0];

    let has_range = f.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign {
                value: RValue::Range { inclusive: true, .. },
                ..
            } => true,
            _ => false,
        })
    });
    assert!(has_range, "inclusive range should have inclusive=true");
}

// ---------------------------------------------------------------------------
// Test 41: Comptime pass folds integer addition
// ---------------------------------------------------------------------------

#[test]
fn comptime_folds_int_addition() {
    use kryos_mir::consteval::run_comptime_pass;
    use std::collections::HashMap;

    let mut module = MirModule {
        functions: vec![MirFunction {
            name: "test".into(),
            params: vec![],
            ret_ty: MirType::I64,
            locals: vec![MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I64,
                mutable: false,
            }],
            blocks: vec![BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::Comptime(Box::new(RValue::BinOp {
                        op: MirBinOp::Add,
                        left: Operand::Constant(Constant::Int(2)),
                        right: Operand::Constant(Constant::Int(3)),
                    })),
                }],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
            }],
            attributes: MirAttributes::default(),
        }],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
    };

    run_comptime_pass(&mut module);

    // After consteval, the Comptime wrapper should be gone and the value folded.
    let instr = &module.functions[0].blocks[0].instructions[0];
    if let Instruction::Assign { value, .. } = instr {
        match value {
            RValue::ConstInt(5) => {} // correct
            other => panic!("expected ConstInt(5), got: {other:?}"),
        }
    } else {
        panic!("expected Assign instruction");
    }
}

// ---------------------------------------------------------------------------
// Test 42: Comptime pass unwraps non-constant expressions
// ---------------------------------------------------------------------------

#[test]
fn comptime_non_const_unwraps() {
    use kryos_mir::consteval::run_comptime_pass;
    use std::collections::HashMap;

    let mut module = MirModule {
        functions: vec![MirFunction {
            name: "test".into(),
            params: vec![],
            ret_ty: MirType::I64,
            locals: vec![MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I64,
                mutable: false,
            }],
            blocks: vec![BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(0),
                    // Comptime wrapping a local variable use -- can't fold.
                    value: RValue::Comptime(Box::new(RValue::Use(Operand::Local(LocalId(0))))),
                }],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
            }],
            attributes: MirAttributes::default(),
        }],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
    };

    run_comptime_pass(&mut module);

    let instr = &module.functions[0].blocks[0].instructions[0];
    if let Instruction::Assign { value, .. } = instr {
        // Should have unwrapped the Comptime wrapper.
        match value {
            RValue::Use(Operand::Local(LocalId(0))) => {} // correct -- unwrapped
            other => panic!("expected Use(Local(0)), got: {other:?}"),
        }
    } else {
        panic!("expected Assign instruction");
    }
}

// ---------------------------------------------------------------------------
// Test 43: Comptime pass folds boolean logic
// ---------------------------------------------------------------------------

#[test]
fn comptime_folds_bool_logic() {
    use kryos_mir::consteval::run_comptime_pass;
    use std::collections::HashMap;

    let mut module = MirModule {
        functions: vec![MirFunction {
            name: "test".into(),
            params: vec![],
            ret_ty: MirType::Bool,
            locals: vec![MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::Bool,
                mutable: false,
            }],
            blocks: vec![BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::Comptime(Box::new(RValue::BinOp {
                        op: MirBinOp::And,
                        left: Operand::Constant(Constant::Bool(true)),
                        right: Operand::Constant(Constant::Bool(false)),
                    })),
                }],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
            }],
            attributes: MirAttributes::default(),
        }],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
    };

    run_comptime_pass(&mut module);

    let instr = &module.functions[0].blocks[0].instructions[0];
    if let Instruction::Assign { value, .. } = instr {
        match value {
            RValue::ConstBool(false) => {} // correct
            other => panic!("expected ConstBool(false), got: {other:?}"),
        }
    } else {
        panic!("expected Assign instruction");
    }
}

// ---------------------------------------------------------------------------
// Test 44: Comptime pass folds unary negation
// ---------------------------------------------------------------------------

#[test]
fn comptime_folds_unary_neg() {
    use kryos_mir::consteval::run_comptime_pass;
    use std::collections::HashMap;

    let mut module = MirModule {
        functions: vec![MirFunction {
            name: "test".into(),
            params: vec![],
            ret_ty: MirType::I64,
            locals: vec![MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I64,
                mutable: false,
            }],
            blocks: vec![BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::Comptime(Box::new(RValue::UnOp {
                        op: MirUnOp::Neg,
                        operand: Operand::Constant(Constant::Int(42)),
                    })),
                }],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
            }],
            attributes: MirAttributes::default(),
        }],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
    };

    run_comptime_pass(&mut module);

    let instr = &module.functions[0].blocks[0].instructions[0];
    if let Instruction::Assign { value, .. } = instr {
        match value {
            RValue::ConstInt(-42) => {} // correct
            other => panic!("expected ConstInt(-42), got: {other:?}"),
        }
    } else {
        panic!("expected Assign instruction");
    }
}

// ---------------------------------------------------------------------------
// Test 45: Comptime pass folds float multiplication
// ---------------------------------------------------------------------------

#[test]
fn comptime_folds_float_mul() {
    use kryos_mir::consteval::run_comptime_pass;
    use std::collections::HashMap;

    let mut module = MirModule {
        functions: vec![MirFunction {
            name: "test".into(),
            params: vec![],
            ret_ty: MirType::F64,
            locals: vec![MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::F64,
                mutable: false,
            }],
            blocks: vec![BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::Comptime(Box::new(RValue::BinOp {
                        op: MirBinOp::Mul,
                        left: Operand::Constant(Constant::Float(3.0)),
                        right: Operand::Constant(Constant::Float(7.0)),
                    })),
                }],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
            }],
            attributes: MirAttributes::default(),
        }],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
    };

    run_comptime_pass(&mut module);

    let instr = &module.functions[0].blocks[0].instructions[0];
    if let Instruction::Assign { value, .. } = instr {
        match value {
            RValue::ConstFloat(v) if (*v - 21.0).abs() < f64::EPSILON => {} // correct
            other => panic!("expected ConstFloat(21.0), got: {other:?}"),
        }
    } else {
        panic!("expected Assign instruction");
    }
}

// ---------------------------------------------------------------------------
// Actor dispatch loop generation
// ---------------------------------------------------------------------------

#[test]
fn actor_generates_dispatch_and_handlers() {
    // actor Counter {
    //     count: i64
    //     fn increment(amount: i64) { }
    //     fn reset() { }
    // }
    // fn main() { let c = Counter() }
    let module = make_module(vec![
        Decl::Actor {
            name: "Counter".into(),
            state_fields: vec![StructField {
                name: "count".into(),
                ty: simple_ty("i64"),
                public: false,
                default: None,
                span: S,
            }],
            handlers: vec![
                MessageHandler {
                    name: "increment".into(),
                    params: vec![make_param("amount", "i64")],
                    ret_ty: None,
                    body: block(vec![]),
                    span: S,
                },
                MessageHandler {
                    name: "reset".into(),
                    params: vec![],
                    ret_ty: None,
                    body: block(vec![]),
                    span: S,
                },
            ],
            annotations: vec![],
            span: S,
        },
        make_fn("main", vec![], None, block(vec![
            Stmt::Let {
                name: "c".into(),
                ty: None,
                value: Some(ast::Expr::FnCall {
                    callee: Box::new(ident("Counter")),
                    args: vec![],
                    span: S,
                }),
                mutable: false,
                pattern: None,
                span: S,
            },
        ])),
    ]);

    let mir = lower_module(&module);

    // Should have: Counter__increment, Counter__reset, Counter__dispatch, main
    let names: Vec<&str> = mir.functions.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"Counter__increment"), "missing Counter__increment: {names:?}");
    assert!(names.contains(&"Counter__reset"), "missing Counter__reset: {names:?}");
    assert!(names.contains(&"Counter__dispatch"), "missing Counter__dispatch: {names:?}");
    assert!(names.contains(&"main"), "missing main: {names:?}");

    // Dispatch function should have a recv loop with Switch terminator.
    let dispatch = mir.functions.iter().find(|f| f.name == "Counter__dispatch").unwrap();
    assert!(dispatch.params.len() == 1, "dispatch takes 1 param (state)");
    assert!(dispatch.ret_ty == MirType::Void, "dispatch returns void");
    // Should have: bb_poll, bb_switch, bb_exit, bb_h1 (increment), bb_h2 (reset)
    assert!(dispatch.blocks.len() == 5,
        "expected 5 blocks in dispatch, got {}", dispatch.blocks.len());

    // Check for Switch terminator.
    let has_switch = dispatch.blocks.iter().any(|b| matches!(b.terminator, Terminator::Switch { .. }));
    assert!(has_switch, "dispatch must have a Switch terminator");

    // Check for kryos_actor_recv_i64 call.
    let has_recv = dispatch.blocks.iter().any(|b| {
        b.instructions.iter().any(|i| matches!(i,
            Instruction::Assign { value: RValue::Call { func, .. }, .. }
            if func == "kryos_actor_recv_i64"
        ))
    });
    assert!(has_recv, "dispatch must call kryos_actor_recv_i64");

    // Main should have an ActorSpawn instruction.
    let main_fn = mir.functions.iter().find(|f| f.name == "main").unwrap();
    let has_spawn = main_fn.blocks.iter().any(|b| {
        b.instructions.iter().any(|i| matches!(i, Instruction::ActorSpawn { .. }))
    });
    assert!(has_spawn, "main must have an ActorSpawn instruction");
}

#[test]
fn actor_method_call_generates_actor_send() {
    // actor Greeter {
    //     fn greet(msg: i64) { }
    // }
    // fn main() {
    //     let g = Greeter()
    //     g.greet(42)
    // }
    let module = make_module(vec![
        Decl::Actor {
            name: "Greeter".into(),
            state_fields: vec![],
            handlers: vec![MessageHandler {
                name: "greet".into(),
                params: vec![make_param("msg", "i64")],
                ret_ty: None,
                body: block(vec![]),
                span: S,
            }],
            annotations: vec![],
            span: S,
        },
        make_fn("main", vec![], None, block(vec![
            Stmt::Let {
                name: "g".into(),
                ty: None,
                value: Some(ast::Expr::FnCall {
                    callee: Box::new(ident("Greeter")),
                    args: vec![],
                    span: S,
                }),
                mutable: false,
                pattern: None,
                span: S,
            },
            expr_stmt(ast::Expr::MethodCall {
                object: Box::new(ident("g")),
                method: "greet".into(),
                args: vec![int_lit(42)],
                span: S,
            }),
        ])),
    ]);

    let mir = lower_module(&module);

    let main_fn = mir.functions.iter().find(|f| f.name == "main").unwrap();

    // Should have an ActorSend instruction with handler_tag=1 and one arg.
    let has_send = main_fn.blocks.iter().any(|b| {
        b.instructions.iter().any(|i| match i {
            Instruction::ActorSend { handler_tag, args, .. } => {
                *handler_tag == 1 && args.len() == 1
            }
            _ => false,
        })
    });
    assert!(has_send, "main must have ActorSend with tag=1 and 1 arg");
}

// ---------------------------------------------------------------------------
// Parallel for generates Spawn instructions
// ---------------------------------------------------------------------------

#[test]
fn parallel_for_generates_spawns() {
    // parallel for i in range(0, 100) { i }
    let module = make_module(vec![make_fn(
        "par_work",
        vec![],
        None,
        block(vec![Stmt::For {
            parallel: true,
            pattern: Pattern::Ident {
                name: "i".to_string(),
                mutable: false,
                span: S,
            },
            iterable: ast::Expr::FnCall {
                callee: Box::new(ident("range")),
                args: vec![int_lit(0), int_lit(100)],
                span: S,
            },
            body: block(vec![expr_stmt(ident("i"))]),
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);

    // The main function should contain Spawn instructions (4 chunks).
    let main_fn = &mir.functions[0];
    let spawn_count = main_fn.blocks.iter().flat_map(|b| &b.instructions).filter(|i| {
        matches!(i, Instruction::Spawn { .. })
    }).count();
    assert_eq!(spawn_count, 4, "parallel for should emit 4 Spawn instructions, got {spawn_count}");

    // There should be generated __spawn_N wrapper functions.
    let spawn_fns: Vec<_> = mir.functions.iter()
        .filter(|f| f.name.starts_with("__spawn_"))
        .collect();
    assert_eq!(spawn_fns.len(), 4, "should generate 4 spawn wrapper functions, got {}", spawn_fns.len());
}

#[test]
fn parallel_for_non_range_falls_back() {
    // parallel for item in collection { item }
    // Non-range iterable should fall back to sequential for-loop (no spawns).
    let module = make_module(vec![make_fn(
        "par_fallback",
        vec![],
        None,
        block(vec![Stmt::For {
            parallel: true,
            pattern: Pattern::Ident {
                name: "item".to_string(),
                mutable: false,
                span: S,
            },
            iterable: ident("collection"),
            body: block(vec![expr_stmt(ident("item"))]),
            span: S,
        }]),
    )]);

    let mir = lower_module(&module);
    let main_fn = &mir.functions[0];

    // No Spawn instructions — should be a regular for-loop.
    let spawn_count = main_fn.blocks.iter().flat_map(|b| &b.instructions).filter(|i| {
        matches!(i, Instruction::Spawn { .. })
    }).count();
    assert_eq!(spawn_count, 0, "non-range parallel for should NOT emit spawns, got {spawn_count}");

    // Should have a Call to "len" (normal for-loop desugaring).
    let has_len_call = main_fn.blocks.iter().any(|bb| {
        bb.instructions.iter().any(|inst| match inst {
            Instruction::Assign { value: RValue::Call { func, .. }, .. } => func == "len",
            _ => false,
        })
    });
    assert!(has_len_call, "fallback should emit len() call like regular for");
}
