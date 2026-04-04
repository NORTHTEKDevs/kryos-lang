//! Integration tests for kryos-mir: lowering, CFG construction, ARC ops, drops.

use kryos_errors::Span;
use kryos_ast::{
    self as ast,
    expr::{BinOp, UnOp, Param, MatchArm, Pattern},
    stmt::{Block, Stmt},
    decl::{Decl, Module},
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
        annotations: vec![],
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
        annotations: vec![],
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

fn make_generic_param_bounded(name: &str, bounds: Vec<&str>) -> ast::decl::GenericParam {
    ast::decl::GenericParam {
        name: name.to_string(),
        bounds: bounds.into_iter().map(|s| s.to_string()).collect(),
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
                annotations: vec![],
                span: S,
            }],
            public: false,
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
                annotations: vec![],
                span: S,
            }],
            public: false,
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
                annotations: vec![],
                span: S,
            }],
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
