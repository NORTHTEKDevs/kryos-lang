//! Integration tests for the Cranelift codegen backend.
#![allow(clippy::approx_constant)]

use kryos_codegen_cranelift::codegen;
use kryos_codegen_cranelift::jit;
use kryos_mir::ir::*;
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Helper: build a MirFunction from parts
// ---------------------------------------------------------------------------

fn make_function(
    name: &str,
    params: Vec<MirParam>,
    ret_ty: MirType,
    locals: Vec<MirLocal>,
    blocks: Vec<BasicBlock>,
) -> MirFunction {
    MirFunction {
        name: name.to_string(),
        params,
        ret_ty,
        blocks,
        locals,
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
    }
}

// ---------------------------------------------------------------------------
// Test 1: Add two i32s and return the result
// ---------------------------------------------------------------------------

#[test]
fn jit_add_two_i32() {
    // fn add(a: i32, b: i32) -> i32 {
    //     let result = a + b;
    //     return result;
    // }
    let func = make_function(
        "add",
        vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::I32,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::I32,
            },
        ],
        MirType::I32,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("a".into()),
                ty: MirType::I32,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::I32,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("result".into()),
                ty: MirType::I32,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(2),
                value: RValue::BinOp {
                    op: MirBinOp::Add,
                    left: Operand::Local(LocalId(0)),
                    right: Operand::Local(LocalId(1)),
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
        }],
    );

    let ptr = jit::jit_compile_function(&func).expect("JIT compilation failed");
    let f: extern "C" fn(i32, i32) -> i32 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(3, 4), 7);
    assert_eq!(f(-10, 25), 15);
    assert_eq!(f(0, 0), 0);
    assert_eq!(f(i32::MAX, 0), i32::MAX);
}

// ---------------------------------------------------------------------------
// Test 2: If/else (branch terminator)
// ---------------------------------------------------------------------------

#[test]
fn jit_branch_if_else() {
    // fn max(a: i32, b: i32) -> i32 {
    //     let cond = a > b;
    //     if cond { return a; } else { return b; }
    // }
    let func = make_function(
        "max",
        vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::I32,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::I32,
            },
        ],
        MirType::I32,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("a".into()),
                ty: MirType::I32,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::I32,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("cond".into()),
                ty: MirType::Bool,
                mutable: false,
            },
        ],
        vec![
            // bb0: compute cond = a > b, branch
            BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(2),
                    value: RValue::BinOp {
                        op: MirBinOp::Gt,
                        left: Operand::Local(LocalId(0)),
                        right: Operand::Local(LocalId(1)),
                    },
                }],
                terminator: Terminator::Branch {
                    cond: Operand::Local(LocalId(2)),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            // bb1: return a
            BasicBlock {
                id: BlockId(1),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
            },
            // bb2: return b
            BasicBlock {
                id: BlockId(2),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
            },
        ],
    );

    let ptr = jit::jit_compile_function(&func).expect("JIT compilation failed");
    let f: extern "C" fn(i32, i32) -> i32 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(10, 5), 10);
    assert_eq!(f(3, 7), 7);
    assert_eq!(f(5, 5), 5); // a > b is false when equal, so returns b
}

// ---------------------------------------------------------------------------
// Test 3: Loop with goto terminators
// ---------------------------------------------------------------------------

#[test]
fn jit_loop_count() {
    // fn count_to_n(n: i64) -> i64 {
    //     let i: i64 = 0;
    //     let sum: i64 = 0;
    //   loop:
    //     let cond = i < n;
    //     if !cond goto exit;
    //     sum = sum + i;
    //     i = i + 1;
    //     goto loop;
    //   exit:
    //     return sum;
    // }
    //
    // sum(0..n) = n*(n-1)/2
    let func = make_function(
        "count_to_n",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("n".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("i".into()),
                ty: MirType::I64,
                mutable: true,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("sum".into()),
                ty: MirType::I64,
                mutable: true,
            },
            MirLocal {
                id: LocalId(3),
                name: Some("cond".into()),
                ty: MirType::Bool,
                mutable: false,
            },
        ],
        vec![
            // bb0: init i=0, sum=0, goto loop
            BasicBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Assign {
                        dest: LocalId(1),
                        value: RValue::ConstInt(0),
                    },
                    Instruction::Assign {
                        dest: LocalId(2),
                        value: RValue::ConstInt(0),
                    },
                ],
                terminator: Terminator::Goto(BlockId(1)),
            },
            // bb1 (loop header): cond = i < n, branch
            BasicBlock {
                id: BlockId(1),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(3),
                    value: RValue::BinOp {
                        op: MirBinOp::Lt,
                        left: Operand::Local(LocalId(1)),
                        right: Operand::Local(LocalId(0)),
                    },
                }],
                terminator: Terminator::Branch {
                    cond: Operand::Local(LocalId(3)),
                    then_block: BlockId(2),
                    else_block: BlockId(3),
                },
            },
            // bb2 (loop body): sum += i, i += 1, goto loop
            BasicBlock {
                id: BlockId(2),
                instructions: vec![
                    Instruction::Assign {
                        dest: LocalId(2),
                        value: RValue::BinOp {
                            op: MirBinOp::Add,
                            left: Operand::Local(LocalId(2)),
                            right: Operand::Local(LocalId(1)),
                        },
                    },
                    Instruction::Assign {
                        dest: LocalId(1),
                        value: RValue::BinOp {
                            op: MirBinOp::Add,
                            left: Operand::Local(LocalId(1)),
                            right: Operand::Constant(Constant::Int(1)),
                        },
                    },
                ],
                terminator: Terminator::Goto(BlockId(1)),
            },
            // bb3 (exit): return sum
            BasicBlock {
                id: BlockId(3),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
            },
        ],
    );

    let ptr = jit::jit_compile_function(&func).expect("JIT compilation failed");
    let f: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(0), 0); // sum of empty range
    assert_eq!(f(1), 0); // sum(0..1) = 0
    assert_eq!(f(5), 10); // 0+1+2+3+4 = 10
    assert_eq!(f(10), 45); // 0+1+2+...+9 = 45
    assert_eq!(f(100), 4950); // n*(n-1)/2 = 4950
}

// ---------------------------------------------------------------------------
// Test 4: ARC retain/release calls are emitted (IR-level check)
// ---------------------------------------------------------------------------

#[test]
fn aot_arc_calls_emitted() {
    // Build a function that does:
    //   arc_retain(ptr)
    //   arc_release(ptr)
    //   return void
    //
    // We compile to an object file and verify it doesn't error.
    // The object file should contain references to kryos_arc_retain/release.
    let func = make_function(
        "arc_test",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::Shared(Box::new(MirType::I64)),
        }],
        MirType::Void,
        vec![MirLocal {
            id: LocalId(0),
            name: Some("ptr".into()),
            ty: MirType::Shared(Box::new(MirType::I64)),
            mutable: false,
        }],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::ArcRetain { ptr: LocalId(0) },
                Instruction::ArcRelease { ptr: LocalId(0) },
            ],
            terminator: Terminator::Return(None),
        }],
    );

    let module = MirModule {
        functions: vec![func],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };

    let obj_bytes = codegen::compile_module(&module).expect("AOT compilation failed");

    // Verify the object file is non-empty and contains the symbol references.
    assert!(!obj_bytes.is_empty(), "object file should not be empty");

    // The object file bytes should contain the function name and ARC symbol names.
    let obj_str = String::from_utf8_lossy(&obj_bytes);
    assert!(
        obj_str.contains("kryos_arc_retain"),
        "object file should reference kryos_arc_retain"
    );
    assert!(
        obj_str.contains("kryos_arc_release"),
        "object file should reference kryos_arc_release"
    );
}

// ---------------------------------------------------------------------------
// Test 5: Float arithmetic
// ---------------------------------------------------------------------------

#[test]
fn jit_float_arithmetic() {
    // fn float_ops(a: f64, b: f64) -> f64 {
    //     let sum = a + b;
    //     let product = a * b;
    //     let result = sum - product;
    //     return result;
    // }
    let func = make_function(
        "float_ops",
        vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::F64,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::F64,
            },
        ],
        MirType::F64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("a".into()),
                ty: MirType::F64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::F64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("sum".into()),
                ty: MirType::F64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(3),
                name: Some("product".into()),
                ty: MirType::F64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(4),
                name: Some("result".into()),
                ty: MirType::F64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::Assign {
                    dest: LocalId(2),
                    value: RValue::BinOp {
                        op: MirBinOp::Add,
                        left: Operand::Local(LocalId(0)),
                        right: Operand::Local(LocalId(1)),
                    },
                },
                Instruction::Assign {
                    dest: LocalId(3),
                    value: RValue::BinOp {
                        op: MirBinOp::Mul,
                        left: Operand::Local(LocalId(0)),
                        right: Operand::Local(LocalId(1)),
                    },
                },
                Instruction::Assign {
                    dest: LocalId(4),
                    value: RValue::BinOp {
                        op: MirBinOp::Sub,
                        left: Operand::Local(LocalId(2)),
                        right: Operand::Local(LocalId(3)),
                    },
                },
            ],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(4)))),
        }],
    );

    let ptr = jit::jit_compile_function(&func).expect("JIT compilation failed");
    let f: extern "C" fn(f64, f64) -> f64 = unsafe { std::mem::transmute(ptr) };

    // (2.0 + 3.0) - (2.0 * 3.0) = 5.0 - 6.0 = -1.0
    let result = f(2.0, 3.0);
    assert!(
        (result - (-1.0)).abs() < f64::EPSILON,
        "expected -1.0, got {result}"
    );

    // (0.0 + 0.0) - (0.0 * 0.0) = 0.0
    let result = f(0.0, 0.0);
    assert!(result.abs() < f64::EPSILON, "expected 0.0, got {result}");

    // (1.5 + 2.5) - (1.5 * 2.5) = 4.0 - 3.75 = 0.25
    let result = f(1.5, 2.5);
    assert!(
        (result - 0.25).abs() < f64::EPSILON,
        "expected 0.25, got {result}"
    );
}

// ---------------------------------------------------------------------------
// Test 6: Float division
// ---------------------------------------------------------------------------

#[test]
fn jit_float_division() {
    // fn divide(a: f64, b: f64) -> f64 {
    //     return a / b;
    // }
    let func = make_function(
        "divide",
        vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::F64,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::F64,
            },
        ],
        MirType::F64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("a".into()),
                ty: MirType::F64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::F64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("result".into()),
                ty: MirType::F64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(2),
                value: RValue::BinOp {
                    op: MirBinOp::Div,
                    left: Operand::Local(LocalId(0)),
                    right: Operand::Local(LocalId(1)),
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
        }],
    );

    let ptr = jit::jit_compile_function(&func).expect("JIT compilation failed");
    let f: extern "C" fn(f64, f64) -> f64 = unsafe { std::mem::transmute(ptr) };

    let result = f(10.0, 3.0);
    assert!(
        (result - 10.0 / 3.0).abs() < f64::EPSILON,
        "expected {}, got {result}",
        10.0_f64 / 3.0
    );

    let result = f(1.0, 2.0);
    assert!(
        (result - 0.5).abs() < f64::EPSILON,
        "expected 0.5, got {result}"
    );
}

// ---------------------------------------------------------------------------
// Test 7: AOT compilation of a simple module
// ---------------------------------------------------------------------------

#[test]
fn aot_compile_simple_module() {
    let func = make_function(
        "identity",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::I64,
        vec![MirLocal {
            id: LocalId(0),
            name: Some("x".into()),
            ty: MirType::I64,
            mutable: false,
        }],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
        }],
    );

    let module = MirModule {
        functions: vec![func],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };

    let obj_bytes = codegen::compile_module(&module).expect("AOT compilation failed");
    assert!(
        obj_bytes.len() > 10,
        "object file too small: {} bytes",
        obj_bytes.len()
    );
}

// ---------------------------------------------------------------------------
// Test 8: Integer subtraction and multiplication
// ---------------------------------------------------------------------------

#[test]
fn jit_int_sub_mul() {
    // fn compute(a: i64, b: i64) -> i64 {
    //     let diff = a - b;
    //     let product = diff * b;
    //     return product;
    // }
    let func = make_function(
        "compute",
        vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::I64,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::I64,
            },
        ],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("a".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("diff".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(3),
                name: Some("product".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::Assign {
                    dest: LocalId(2),
                    value: RValue::BinOp {
                        op: MirBinOp::Sub,
                        left: Operand::Local(LocalId(0)),
                        right: Operand::Local(LocalId(1)),
                    },
                },
                Instruction::Assign {
                    dest: LocalId(3),
                    value: RValue::BinOp {
                        op: MirBinOp::Mul,
                        left: Operand::Local(LocalId(2)),
                        right: Operand::Local(LocalId(1)),
                    },
                },
            ],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(3)))),
        }],
    );

    let ptr = jit::jit_compile_function(&func).expect("JIT compilation failed");
    let f: extern "C" fn(i64, i64) -> i64 = unsafe { std::mem::transmute(ptr) };

    // (10 - 3) * 3 = 21
    assert_eq!(f(10, 3), 21);
    // (5 - 5) * 5 = 0
    assert_eq!(f(5, 5), 0);
    // (0 - 7) * 7 = -49
    assert_eq!(f(0, 7), -49);
}

// ---------------------------------------------------------------------------
// Test: Struct creation and field access (AOT)
// ---------------------------------------------------------------------------

#[test]
fn aot_struct_field_access() {
    // struct Point { x: i64, y: i64 }
    //
    // fn get_y() -> i64 {
    //     let p = Point { x: 10, y: 42 }
    //     let result = p.y
    //     return result
    // }
    let mut struct_defs = HashMap::new();
    struct_defs.insert(
        "Point".to_string(),
        vec![
            ("x".to_string(), MirType::I64),
            ("y".to_string(), MirType::I64),
        ],
    );

    let func = make_function(
        "get_y",
        vec![],
        MirType::I64,
        vec![
            // _0: Point (pointer to struct on stack)
            MirLocal {
                id: LocalId(0),
                name: Some("p".into()),
                ty: MirType::Struct("Point".into()),
                mutable: false,
            },
            // _1: i64 (result of p.y)
            MirLocal {
                id: LocalId(1),
                name: Some("result".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                // _0 = Point { x: 10, y: 42 }
                Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::Struct {
                        name: "Point".into(),
                        fields: vec![
                            ("x".to_string(), Operand::Constant(Constant::Int(10))),
                            ("y".to_string(), Operand::Constant(Constant::Int(42))),
                        ],
                    },
                },
                // _1 = _0.y
                Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::Field {
                        object: Operand::Local(LocalId(0)),
                        field: "y".into(),
                    },
                },
            ],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
    );

    let module = MirModule {
        functions: vec![func],
        struct_defs,
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };

    let obj_bytes =
        codegen::compile_module(&module).expect("AOT compilation of struct access failed");
    assert!(
        obj_bytes.len() > 10,
        "object file too small: {} bytes",
        obj_bytes.len()
    );
}

// ---------------------------------------------------------------------------
// Test: Struct with f64 fields (AOT)
// ---------------------------------------------------------------------------

#[test]
fn aot_struct_f64_field_access() {
    // struct Vec2 { x: f64, y: f64 }
    //
    // fn get_x() -> f64 {
    //     let v = Vec2 { x: 3.14, y: 2.72 }
    //     let result = v.x
    //     return result
    // }
    let mut struct_defs = HashMap::new();
    struct_defs.insert(
        "Vec2".to_string(),
        vec![
            ("x".to_string(), MirType::F64),
            ("y".to_string(), MirType::F64),
        ],
    );

    let func = make_function(
        "get_x",
        vec![],
        MirType::F64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("v".into()),
                ty: MirType::Struct("Vec2".into()),
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("result".into()),
                ty: MirType::F64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::Struct {
                        name: "Vec2".into(),
                        fields: vec![
                            ("x".to_string(), Operand::Constant(Constant::Float(3.14))),
                            ("y".to_string(), Operand::Constant(Constant::Float(2.72))),
                        ],
                    },
                },
                Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::Field {
                        object: Operand::Local(LocalId(0)),
                        field: "x".into(),
                    },
                },
            ],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
    );

    let module = MirModule {
        functions: vec![func],
        struct_defs,
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };

    let obj_bytes =
        codegen::compile_module(&module).expect("AOT compilation of f64 struct access failed");
    assert!(
        obj_bytes.len() > 10,
        "object file too small: {} bytes",
        obj_bytes.len()
    );
}

// ===========================================================================
// NEW TESTS (30+)
// ===========================================================================

// ---------------------------------------------------------------------------
// Arithmetic & Types (1–5)
// ---------------------------------------------------------------------------

// 1. F64 addition, return result
#[test]
fn jit_float_add() {
    let func = make_function(
        "float_add",
        vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::F64,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::F64,
            },
        ],
        MirType::F64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("a".into()),
                ty: MirType::F64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::F64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("r".into()),
                ty: MirType::F64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(2),
                value: RValue::BinOp {
                    op: MirBinOp::Add,
                    left: Operand::Local(LocalId(0)),
                    right: Operand::Local(LocalId(1)),
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
        }],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(f64, f64) -> f64 = unsafe { std::mem::transmute(ptr) };
    assert!((f(1.5, 2.5) - 4.0).abs() < f64::EPSILON);
    assert!((f(-1.0, 1.0) - 0.0).abs() < f64::EPSILON);
    assert!((f(0.1, 0.2) - 0.3).abs() < 1e-15);
}

// 2. F64 less-than returning bool (i64 0/1)
#[test]
fn jit_float_comparison() {
    // fn lt(a: f64, b: f64) -> i64 {
    //     let cond = a < b;        // Bool
    //     let r = cast cond as i64;
    //     return r;
    // }
    let func = make_function(
        "float_lt",
        vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::F64,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::F64,
            },
        ],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("a".into()),
                ty: MirType::F64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::F64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("cond".into()),
                ty: MirType::Bool,
                mutable: false,
            },
            MirLocal {
                id: LocalId(3),
                name: Some("r".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![
            BasicBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Assign {
                        dest: LocalId(2),
                        value: RValue::BinOp {
                            op: MirBinOp::Lt,
                            left: Operand::Local(LocalId(0)),
                            right: Operand::Local(LocalId(1)),
                        },
                    },
                    // Branch on cond to return 1 or 0
                ],
                terminator: Terminator::Branch {
                    cond: Operand::Local(LocalId(2)),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            BasicBlock {
                id: BlockId(1),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(1)))),
            },
            BasicBlock {
                id: BlockId(2),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(0)))),
            },
        ],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(f64, f64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(1.0, 2.0), 1); // 1.0 < 2.0
    assert_eq!(f(3.0, 2.0), 0); // 3.0 < 2.0 is false
    assert_eq!(f(2.0, 2.0), 0); // equal is not less-than
}

// 3. Bool AND/OR operations
#[test]
fn jit_bool_and_or() {
    // fn and_or(a: i64, b: i64) -> i64 {
    //     let ca = a > 0;    // Bool
    //     let cb = b > 0;    // Bool
    //     let r_and = ca AND cb;
    //     let r_or  = ca OR cb;
    //     // return 2*r_and + r_or as i64 via branching
    //     // Actually, simpler: return branch-derived value
    // }
    //
    // We'll test AND: returns 1 if both > 0, else 0
    let func = make_function(
        "bool_and",
        vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::I64,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::I64,
            },
        ],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("a".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("ca".into()),
                ty: MirType::Bool,
                mutable: false,
            },
            MirLocal {
                id: LocalId(3),
                name: Some("cb".into()),
                ty: MirType::Bool,
                mutable: false,
            },
            MirLocal {
                id: LocalId(4),
                name: Some("r".into()),
                ty: MirType::Bool,
                mutable: false,
            },
        ],
        vec![
            BasicBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Assign {
                        dest: LocalId(2),
                        value: RValue::BinOp {
                            op: MirBinOp::Gt,
                            left: Operand::Local(LocalId(0)),
                            right: Operand::Constant(Constant::Int(0)),
                        },
                    },
                    Instruction::Assign {
                        dest: LocalId(3),
                        value: RValue::BinOp {
                            op: MirBinOp::Gt,
                            left: Operand::Local(LocalId(1)),
                            right: Operand::Constant(Constant::Int(0)),
                        },
                    },
                    Instruction::Assign {
                        dest: LocalId(4),
                        value: RValue::BinOp {
                            op: MirBinOp::And,
                            left: Operand::Local(LocalId(2)),
                            right: Operand::Local(LocalId(3)),
                        },
                    },
                ],
                terminator: Terminator::Branch {
                    cond: Operand::Local(LocalId(4)),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            BasicBlock {
                id: BlockId(1),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(1)))),
            },
            BasicBlock {
                id: BlockId(2),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(0)))),
            },
        ],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64, i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(1, 1), 1);
    assert_eq!(f(1, 0), 0);
    assert_eq!(f(0, 1), 0);
    assert_eq!(f(0, 0), 0);
}

// 4. i32 subtraction returning negative
#[test]
fn jit_i32_subtract() {
    let func = make_function(
        "sub32",
        vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::I32,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::I32,
            },
        ],
        MirType::I32,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("a".into()),
                ty: MirType::I32,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::I32,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("r".into()),
                ty: MirType::I32,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(2),
                value: RValue::BinOp {
                    op: MirBinOp::Sub,
                    left: Operand::Local(LocalId(0)),
                    right: Operand::Local(LocalId(1)),
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
        }],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i32, i32) -> i32 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(3, 7), -4);
    assert_eq!(f(0, 100), -100);
    assert_eq!(f(10, 10), 0);
}

// 5. I64 modulo operation
#[test]
fn jit_i64_modulo() {
    let func = make_function(
        "modulo",
        vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::I64,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::I64,
            },
        ],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("a".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("r".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(2),
                value: RValue::BinOp {
                    op: MirBinOp::Mod,
                    left: Operand::Local(LocalId(0)),
                    right: Operand::Local(LocalId(1)),
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
        }],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64, i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(10, 3), 1);
    assert_eq!(f(15, 5), 0);
    assert_eq!(f(7, 4), 3);
}

// ---------------------------------------------------------------------------
// Control Flow (6–10)
// ---------------------------------------------------------------------------

// 6. If-then-else returning different values
#[test]
fn jit_if_then_else() {
    // fn pick(cond: i64) -> i64 {
    //     let c = cond > 0;
    //     if c { return 42; } else { return 99; }
    // }
    let func = make_function(
        "pick",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("cond".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("c".into()),
                ty: MirType::Bool,
                mutable: false,
            },
        ],
        vec![
            BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::BinOp {
                        op: MirBinOp::Gt,
                        left: Operand::Local(LocalId(0)),
                        right: Operand::Constant(Constant::Int(0)),
                    },
                }],
                terminator: Terminator::Branch {
                    cond: Operand::Local(LocalId(1)),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            BasicBlock {
                id: BlockId(1),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(42)))),
            },
            BasicBlock {
                id: BlockId(2),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(99)))),
            },
        ],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(1), 42);
    assert_eq!(f(0), 99);
    assert_eq!(f(-5), 99);
}

// 7. Sum 1..N via while loop
#[test]
fn jit_while_loop_sum() {
    // fn sum_to(n: i64) -> i64 {
    //     let i = 1, sum = 0;
    //   loop: if i <= n { sum += i; i += 1; goto loop; } else { return sum; }
    // }
    let func = make_function(
        "sum_to",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("n".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("i".into()),
                ty: MirType::I64,
                mutable: true,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("sum".into()),
                ty: MirType::I64,
                mutable: true,
            },
            MirLocal {
                id: LocalId(3),
                name: Some("cond".into()),
                ty: MirType::Bool,
                mutable: false,
            },
        ],
        vec![
            BasicBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Assign {
                        dest: LocalId(1),
                        value: RValue::ConstInt(1),
                    },
                    Instruction::Assign {
                        dest: LocalId(2),
                        value: RValue::ConstInt(0),
                    },
                ],
                terminator: Terminator::Goto(BlockId(1)),
            },
            BasicBlock {
                id: BlockId(1),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(3),
                    value: RValue::BinOp {
                        op: MirBinOp::LtEq,
                        left: Operand::Local(LocalId(1)),
                        right: Operand::Local(LocalId(0)),
                    },
                }],
                terminator: Terminator::Branch {
                    cond: Operand::Local(LocalId(3)),
                    then_block: BlockId(2),
                    else_block: BlockId(3),
                },
            },
            BasicBlock {
                id: BlockId(2),
                instructions: vec![
                    Instruction::Assign {
                        dest: LocalId(2),
                        value: RValue::BinOp {
                            op: MirBinOp::Add,
                            left: Operand::Local(LocalId(2)),
                            right: Operand::Local(LocalId(1)),
                        },
                    },
                    Instruction::Assign {
                        dest: LocalId(1),
                        value: RValue::BinOp {
                            op: MirBinOp::Add,
                            left: Operand::Local(LocalId(1)),
                            right: Operand::Constant(Constant::Int(1)),
                        },
                    },
                ],
                terminator: Terminator::Goto(BlockId(1)),
            },
            BasicBlock {
                id: BlockId(3),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
            },
        ],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(0), 0); // sum of empty range
    assert_eq!(f(1), 1); // 1
    assert_eq!(f(5), 15); // 1+2+3+4+5
    assert_eq!(f(10), 55); // 1+2+...+10
    assert_eq!(f(100), 5050); // n*(n+1)/2
}

// 8. Nested if: classify a number as positive/negative/zero
#[test]
fn jit_nested_if() {
    // fn classify(x: i64) -> i64 {
    //     if x > 0 { return 1; }
    //     else if x < 0 { return -1; }
    //     else { return 0; }
    // }
    let func = make_function(
        "classify",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("c1".into()),
                ty: MirType::Bool,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("c2".into()),
                ty: MirType::Bool,
                mutable: false,
            },
        ],
        vec![
            // bb0: if x > 0 goto bb1 else bb2
            BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::BinOp {
                        op: MirBinOp::Gt,
                        left: Operand::Local(LocalId(0)),
                        right: Operand::Constant(Constant::Int(0)),
                    },
                }],
                terminator: Terminator::Branch {
                    cond: Operand::Local(LocalId(1)),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            // bb1: return 1
            BasicBlock {
                id: BlockId(1),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(1)))),
            },
            // bb2: if x < 0 goto bb3 else bb4
            BasicBlock {
                id: BlockId(2),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(2),
                    value: RValue::BinOp {
                        op: MirBinOp::Lt,
                        left: Operand::Local(LocalId(0)),
                        right: Operand::Constant(Constant::Int(0)),
                    },
                }],
                terminator: Terminator::Branch {
                    cond: Operand::Local(LocalId(2)),
                    then_block: BlockId(3),
                    else_block: BlockId(4),
                },
            },
            // bb3: return -1
            BasicBlock {
                id: BlockId(3),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(-1)))),
            },
            // bb4: return 0
            BasicBlock {
                id: BlockId(4),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(0)))),
            },
        ],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(42), 1);
    assert_eq!(f(-10), -1);
    assert_eq!(f(0), 0);
}

// 9. Early return from middle of function
#[test]
fn jit_early_return() {
    // fn early(x: i64) -> i64 {
    //     if x == 0 { return 100; }  // early exit
    //     let r = x * 2;
    //     return r;
    // }
    let func = make_function(
        "early",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("cond".into()),
                ty: MirType::Bool,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("r".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![
            BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::BinOp {
                        op: MirBinOp::Eq,
                        left: Operand::Local(LocalId(0)),
                        right: Operand::Constant(Constant::Int(0)),
                    },
                }],
                terminator: Terminator::Branch {
                    cond: Operand::Local(LocalId(1)),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            // bb1: early return
            BasicBlock {
                id: BlockId(1),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(100)))),
            },
            // bb2: normal path
            BasicBlock {
                id: BlockId(2),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(2),
                    value: RValue::BinOp {
                        op: MirBinOp::Mul,
                        left: Operand::Local(LocalId(0)),
                        right: Operand::Constant(Constant::Int(2)),
                    },
                }],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
            },
        ],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(0), 100);
    assert_eq!(f(5), 10);
    assert_eq!(f(-3), -6);
}

// 10. Multi-block value flow (phi-like)
#[test]
fn jit_multi_block_phi() {
    // fn abs_val(x: i64) -> i64 {
    //     let result;
    //     if x >= 0 { result = x; } else { result = -x; }
    //     return result;
    // }
    // Uses mutable result assigned in different branches, then merged via goto.
    let func = make_function(
        "abs_val",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("cond".into()),
                ty: MirType::Bool,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("result".into()),
                ty: MirType::I64,
                mutable: true,
            },
        ],
        vec![
            BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::BinOp {
                        op: MirBinOp::GtEq,
                        left: Operand::Local(LocalId(0)),
                        right: Operand::Constant(Constant::Int(0)),
                    },
                }],
                terminator: Terminator::Branch {
                    cond: Operand::Local(LocalId(1)),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            // bb1: result = x
            BasicBlock {
                id: BlockId(1),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(2),
                    value: RValue::Use(Operand::Local(LocalId(0))),
                }],
                terminator: Terminator::Goto(BlockId(3)),
            },
            // bb2: result = 0 - x (negate)
            BasicBlock {
                id: BlockId(2),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(2),
                    value: RValue::BinOp {
                        op: MirBinOp::Sub,
                        left: Operand::Constant(Constant::Int(0)),
                        right: Operand::Local(LocalId(0)),
                    },
                }],
                terminator: Terminator::Goto(BlockId(3)),
            },
            // bb3: return result
            BasicBlock {
                id: BlockId(3),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
            },
        ],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(10), 10);
    assert_eq!(f(-10), 10);
    assert_eq!(f(0), 0);
}

// ---------------------------------------------------------------------------
// Functions (11–15)
// ---------------------------------------------------------------------------

// 11. Function A calls function B (AOT compilation of cross-function call)
#[test]
fn aot_call_other_function() {
    // fn double(x: i64) -> i64 { return x * 2; }
    // fn main(x: i64) -> i64 { return double(x); }
    let func_double = make_function(
        "double",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("r".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(1),
                value: RValue::BinOp {
                    op: MirBinOp::Mul,
                    left: Operand::Local(LocalId(0)),
                    right: Operand::Constant(Constant::Int(2)),
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
    );

    let func_main = make_function(
        "main_fn",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("r".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(1),
                value: RValue::Call {
                    func: "double".to_string(),
                    args: vec![Operand::Local(LocalId(0))],
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
    );

    let module = MirModule {
        functions: vec![func_double, func_main],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };

    let obj_bytes = codegen::compile_module(&module).expect("AOT cross-call failed");
    assert!(obj_bytes.len() > 10, "object file too small");
    // Verify both function names appear in the object
    let obj_str = String::from_utf8_lossy(&obj_bytes);
    assert!(
        obj_str.contains("double"),
        "should contain function 'double'"
    );
    assert!(
        obj_str.contains("main_fn"),
        "should contain function 'main_fn'"
    );
}

// 12. Recursive factorial (AOT compilation)
#[test]
fn aot_recursive_factorial() {
    // fn factorial(n: i64) -> i64 {
    //     if n <= 1 { return 1; }
    //     let sub = n - 1;
    //     let rec = factorial(sub);
    //     return n * rec;
    // }
    let func = make_function(
        "factorial",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("n".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("cond".into()),
                ty: MirType::Bool,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("sub".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(3),
                name: Some("rec".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(4),
                name: Some("result".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![
            // bb0: if n <= 1 goto base else goto recurse
            BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::BinOp {
                        op: MirBinOp::LtEq,
                        left: Operand::Local(LocalId(0)),
                        right: Operand::Constant(Constant::Int(1)),
                    },
                }],
                terminator: Terminator::Branch {
                    cond: Operand::Local(LocalId(1)),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            // bb1: base case
            BasicBlock {
                id: BlockId(1),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(1)))),
            },
            // bb2: recursive case
            BasicBlock {
                id: BlockId(2),
                instructions: vec![
                    Instruction::Assign {
                        dest: LocalId(2),
                        value: RValue::BinOp {
                            op: MirBinOp::Sub,
                            left: Operand::Local(LocalId(0)),
                            right: Operand::Constant(Constant::Int(1)),
                        },
                    },
                    Instruction::Assign {
                        dest: LocalId(3),
                        value: RValue::Call {
                            func: "factorial".to_string(),
                            args: vec![Operand::Local(LocalId(2))],
                        },
                    },
                    Instruction::Assign {
                        dest: LocalId(4),
                        value: RValue::BinOp {
                            op: MirBinOp::Mul,
                            left: Operand::Local(LocalId(0)),
                            right: Operand::Local(LocalId(3)),
                        },
                    },
                ],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(4)))),
            },
        ],
    );

    let module = MirModule {
        functions: vec![func],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };
    let obj_bytes = codegen::compile_module(&module).expect("AOT recursive factorial failed");
    assert!(obj_bytes.len() > 10, "object file too small");
    let obj_str = String::from_utf8_lossy(&obj_bytes);
    assert!(
        obj_str.contains("factorial"),
        "should contain function 'factorial'"
    );
}

// 13. Void function (no return value)
#[test]
fn jit_void_function() {
    let func = make_function(
        "void_fn",
        vec![],
        MirType::Void,
        vec![],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Nop],
            terminator: Terminator::Return(None),
        }],
    );
    // Should compile without error
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn() = unsafe { std::mem::transmute(ptr) };
    f(); // should not crash
}

// 14. Function with 6 parameters
#[test]
fn jit_many_params() {
    // fn sum6(a,b,c,d,e,f: i64) -> i64 { return a+b+c+d+e+f; }
    let func = make_function(
        "sum6",
        vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::I64,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::I64,
            },
            MirParam {
                local: LocalId(2),
                ty: MirType::I64,
            },
            MirParam {
                local: LocalId(3),
                ty: MirType::I64,
            },
            MirParam {
                local: LocalId(4),
                ty: MirType::I64,
            },
            MirParam {
                local: LocalId(5),
                ty: MirType::I64,
            },
        ],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("a".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("c".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(3),
                name: Some("d".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(4),
                name: Some("e".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(5),
                name: Some("f".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(6),
                name: Some("t1".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(7),
                name: Some("t2".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(8),
                name: Some("t3".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(9),
                name: Some("t4".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(10),
                name: Some("t5".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::Assign {
                    dest: LocalId(6),
                    value: RValue::BinOp {
                        op: MirBinOp::Add,
                        left: Operand::Local(LocalId(0)),
                        right: Operand::Local(LocalId(1)),
                    },
                },
                Instruction::Assign {
                    dest: LocalId(7),
                    value: RValue::BinOp {
                        op: MirBinOp::Add,
                        left: Operand::Local(LocalId(6)),
                        right: Operand::Local(LocalId(2)),
                    },
                },
                Instruction::Assign {
                    dest: LocalId(8),
                    value: RValue::BinOp {
                        op: MirBinOp::Add,
                        left: Operand::Local(LocalId(7)),
                        right: Operand::Local(LocalId(3)),
                    },
                },
                Instruction::Assign {
                    dest: LocalId(9),
                    value: RValue::BinOp {
                        op: MirBinOp::Add,
                        left: Operand::Local(LocalId(8)),
                        right: Operand::Local(LocalId(4)),
                    },
                },
                Instruction::Assign {
                    dest: LocalId(10),
                    value: RValue::BinOp {
                        op: MirBinOp::Add,
                        left: Operand::Local(LocalId(9)),
                        right: Operand::Local(LocalId(5)),
                    },
                },
            ],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(10)))),
        }],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64, i64, i64, i64, i64, i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(1, 2, 3, 4, 5, 6), 21);
    assert_eq!(f(10, 20, 30, 40, 50, 60), 210);
}

// 15. Call chain: A calls B calls C (3 functions AOT compiled)
#[test]
fn aot_call_chain() {
    // fn add1(x: i64) -> i64 { return x + 1; }
    // fn add2(x: i64) -> i64 { return add1(add1(x)); }
    // fn add4(x: i64) -> i64 { return add2(add2(x)); }
    let func_add1 = make_function(
        "add1",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("r".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(1),
                value: RValue::BinOp {
                    op: MirBinOp::Add,
                    left: Operand::Local(LocalId(0)),
                    right: Operand::Constant(Constant::Int(1)),
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
    );

    let func_add2 = make_function(
        "add2",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("t1".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("t2".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::Call {
                        func: "add1".to_string(),
                        args: vec![Operand::Local(LocalId(0))],
                    },
                },
                Instruction::Assign {
                    dest: LocalId(2),
                    value: RValue::Call {
                        func: "add1".to_string(),
                        args: vec![Operand::Local(LocalId(1))],
                    },
                },
            ],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
        }],
    );

    let func_add4 = make_function(
        "add4",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("t1".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("t2".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::Call {
                        func: "add2".to_string(),
                        args: vec![Operand::Local(LocalId(0))],
                    },
                },
                Instruction::Assign {
                    dest: LocalId(2),
                    value: RValue::Call {
                        func: "add2".to_string(),
                        args: vec![Operand::Local(LocalId(1))],
                    },
                },
            ],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
        }],
    );

    let module = MirModule {
        functions: vec![func_add1, func_add2, func_add4],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };
    let obj_bytes = codegen::compile_module(&module).expect("AOT call chain failed");
    assert!(obj_bytes.len() > 10, "object file too small");
    let obj_str = String::from_utf8_lossy(&obj_bytes);
    assert!(obj_str.contains("add1"), "should contain function 'add1'");
    assert!(obj_str.contains("add2"), "should contain function 'add2'");
    assert!(obj_str.contains("add4"), "should contain function 'add4'");
}

// ---------------------------------------------------------------------------
// Structs & Memory (16–20)
// ---------------------------------------------------------------------------

// 16. Struct with 3 i64 fields — AOT compile succeeds
#[test]
fn aot_struct_three_fields() {
    let mut struct_defs = HashMap::new();
    struct_defs.insert(
        "Triple".to_string(),
        vec![
            ("a".to_string(), MirType::I64),
            ("b".to_string(), MirType::I64),
            ("c".to_string(), MirType::I64),
        ],
    );

    let func = make_function(
        "get_c",
        vec![],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("t".into()),
                ty: MirType::Struct("Triple".into()),
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("r".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::Struct {
                        name: "Triple".into(),
                        fields: vec![
                            ("a".to_string(), Operand::Constant(Constant::Int(1))),
                            ("b".to_string(), Operand::Constant(Constant::Int(2))),
                            ("c".to_string(), Operand::Constant(Constant::Int(3))),
                        ],
                    },
                },
                Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::Field {
                        object: Operand::Local(LocalId(0)),
                        field: "c".into(),
                    },
                },
            ],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
    );

    let module = MirModule {
        functions: vec![func],
        struct_defs,
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };
    let obj_bytes = codegen::compile_module(&module).expect("AOT failed");
    assert!(obj_bytes.len() > 10);
}

// 17. Struct with mixed i64 and f64 fields — AOT compile
#[test]
fn aot_struct_mixed_types() {
    let mut struct_defs = HashMap::new();
    struct_defs.insert(
        "Mixed".to_string(),
        vec![
            ("count".to_string(), MirType::I64),
            ("value".to_string(), MirType::F64),
            ("flag".to_string(), MirType::Bool),
        ],
    );

    let func = make_function(
        "get_count",
        vec![],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("m".into()),
                ty: MirType::Struct("Mixed".into()),
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("r".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::Struct {
                        name: "Mixed".into(),
                        fields: vec![
                            ("count".to_string(), Operand::Constant(Constant::Int(42))),
                            (
                                "value".to_string(),
                                Operand::Constant(Constant::Float(3.14)),
                            ),
                            ("flag".to_string(), Operand::Constant(Constant::Bool(true))),
                        ],
                    },
                },
                Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::Field {
                        object: Operand::Local(LocalId(0)),
                        field: "count".into(),
                    },
                },
            ],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
    );

    let module = MirModule {
        functions: vec![func],
        struct_defs,
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };
    let obj_bytes = codegen::compile_module(&module).expect("AOT failed");
    assert!(obj_bytes.len() > 10);
}

// 18. Multiple struct defs in a single module
#[test]
fn aot_multiple_struct_defs() {
    let mut struct_defs = HashMap::new();
    struct_defs.insert(
        "PointA".to_string(),
        vec![
            ("x".to_string(), MirType::I64),
            ("y".to_string(), MirType::I64),
        ],
    );
    struct_defs.insert(
        "PointB".to_string(),
        vec![
            ("x".to_string(), MirType::F64),
            ("y".to_string(), MirType::F64),
        ],
    );

    let func_a = make_function(
        "make_a",
        vec![],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("p".into()),
                ty: MirType::Struct("PointA".into()),
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("r".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::Struct {
                        name: "PointA".into(),
                        fields: vec![
                            ("x".to_string(), Operand::Constant(Constant::Int(10))),
                            ("y".to_string(), Operand::Constant(Constant::Int(20))),
                        ],
                    },
                },
                Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::Field {
                        object: Operand::Local(LocalId(0)),
                        field: "x".into(),
                    },
                },
            ],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
    );

    let func_b = make_function(
        "make_b",
        vec![],
        MirType::F64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("p".into()),
                ty: MirType::Struct("PointB".into()),
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("r".into()),
                ty: MirType::F64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::Struct {
                        name: "PointB".into(),
                        fields: vec![
                            ("x".to_string(), Operand::Constant(Constant::Float(1.5))),
                            ("y".to_string(), Operand::Constant(Constant::Float(2.5))),
                        ],
                    },
                },
                Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::Field {
                        object: Operand::Local(LocalId(0)),
                        field: "y".into(),
                    },
                },
            ],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
    );

    let module = MirModule {
        functions: vec![func_a, func_b],
        struct_defs,
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };
    let obj_bytes = codegen::compile_module(&module).expect("AOT failed");
    assert!(obj_bytes.len() > 10);
}

// 19. Struct field access — first field
#[test]
fn aot_struct_first_field() {
    let mut struct_defs = HashMap::new();
    struct_defs.insert(
        "Pair".to_string(),
        vec![
            ("first".to_string(), MirType::I64),
            ("second".to_string(), MirType::I64),
        ],
    );

    let func = make_function(
        "get_first",
        vec![],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("p".into()),
                ty: MirType::Struct("Pair".into()),
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("r".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::Struct {
                        name: "Pair".into(),
                        fields: vec![
                            ("first".to_string(), Operand::Constant(Constant::Int(100))),
                            ("second".to_string(), Operand::Constant(Constant::Int(200))),
                        ],
                    },
                },
                Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::Field {
                        object: Operand::Local(LocalId(0)),
                        field: "first".into(),
                    },
                },
            ],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
    );

    let module = MirModule {
        functions: vec![func],
        struct_defs,
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };
    let obj_bytes = codegen::compile_module(&module).expect("AOT failed");
    assert!(obj_bytes.len() > 10);
}

// 20. Empty function body with a Nop instruction
#[test]
fn aot_nop_instruction() {
    let func = make_function(
        "nop_fn",
        vec![],
        MirType::Void,
        vec![],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Nop, Instruction::Nop],
            terminator: Terminator::Return(None),
        }],
    );
    let module = MirModule {
        functions: vec![func],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };
    let obj_bytes = codegen::compile_module(&module).expect("AOT failed");
    assert!(obj_bytes.len() > 10);
}

// ---------------------------------------------------------------------------
// Drop & Cleanup (21–25) — AOT compilation tests
// ---------------------------------------------------------------------------

// 21. Drop for Str type (AOT compile — should reference kryos_string_free)
#[test]
fn aot_drop_string() {
    let func = make_function(
        "drop_str_fn",
        vec![],
        MirType::Void,
        vec![MirLocal {
            id: LocalId(0),
            name: Some("s".into()),
            ty: MirType::Str,
            mutable: false,
        }],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::ConstString("hello".to_string()),
                },
                Instruction::Drop { local: LocalId(0) },
            ],
            terminator: Terminator::Return(None),
        }],
    );
    let module = MirModule {
        functions: vec![func],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };
    let obj_bytes = codegen::compile_module(&module).expect("AOT compilation failed");
    assert!(!obj_bytes.is_empty());
    let obj_str = String::from_utf8_lossy(&obj_bytes);
    assert!(
        obj_str.contains("kryos_string_free"),
        "Drop of Str should reference kryos_string_free"
    );
}

// 22. Drop for Array type (AOT compile)
#[test]
fn aot_drop_array() {
    let func = make_function(
        "drop_arr_fn",
        vec![],
        MirType::Void,
        vec![MirLocal {
            id: LocalId(0),
            name: Some("arr".into()),
            ty: MirType::Array(Box::new(MirType::I64), None),
            mutable: false,
        }],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::ConstInt(0), // placeholder, just to have something
                },
                Instruction::Drop { local: LocalId(0) },
            ],
            terminator: Terminator::Return(None),
        }],
    );
    let module = MirModule {
        functions: vec![func],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };
    let obj_bytes = codegen::compile_module(&module).expect("AOT compilation failed");
    assert!(!obj_bytes.is_empty());
    let obj_str = String::from_utf8_lossy(&obj_bytes);
    assert!(
        obj_str.contains("kryos_array_free"),
        "Drop of Array should reference kryos_array_free"
    );
}

// 23. Drop of struct type (AOT compile)
#[test]
fn aot_drop_struct() {
    let mut struct_defs = HashMap::new();
    struct_defs.insert(
        "Droppable".to_string(),
        vec![("val".to_string(), MirType::I64)],
    );

    let func = make_function(
        "drop_struct_fn",
        vec![],
        MirType::Void,
        vec![MirLocal {
            id: LocalId(0),
            name: Some("d".into()),
            ty: MirType::Struct("Droppable".into()),
            mutable: false,
        }],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::Struct {
                        name: "Droppable".into(),
                        fields: vec![("val".to_string(), Operand::Constant(Constant::Int(42)))],
                    },
                },
                Instruction::Drop { local: LocalId(0) },
            ],
            terminator: Terminator::Return(None),
        }],
    );

    let module = MirModule {
        functions: vec![func],
        struct_defs,
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };
    let obj_bytes = codegen::compile_module(&module).expect("AOT compilation failed");
    assert!(!obj_bytes.is_empty());
}

// 24. Drop for i64 type should be a no-op (AOT compile succeeds)
#[test]
fn aot_drop_primitive_noop() {
    let func = make_function(
        "drop_int_fn",
        vec![],
        MirType::Void,
        vec![MirLocal {
            id: LocalId(0),
            name: Some("n".into()),
            ty: MirType::I64,
            mutable: false,
        }],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::ConstInt(42),
                },
                Instruction::Drop { local: LocalId(0) },
            ],
            terminator: Terminator::Return(None),
        }],
    );
    let module = MirModule {
        functions: vec![func],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };
    let obj_bytes = codegen::compile_module(&module).expect("AOT compilation failed");
    // A primitive i64 Drop should compile without error (it's a no-op at runtime).
    assert!(!obj_bytes.is_empty(), "object file should not be empty");
}

// 25. Multiple drops in reverse order
#[test]
fn aot_multiple_drops() {
    let func = make_function(
        "multi_drop_fn",
        vec![],
        MirType::Void,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("s1".into()),
                ty: MirType::Str,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("s2".into()),
                ty: MirType::Str,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::ConstString("first".to_string()),
                },
                Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::ConstString("second".to_string()),
                },
                // Drop in reverse order
                Instruction::Drop { local: LocalId(1) },
                Instruction::Drop { local: LocalId(0) },
            ],
            terminator: Terminator::Return(None),
        }],
    );
    let module = MirModule {
        functions: vec![func],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };
    let obj_bytes = codegen::compile_module(&module).expect("AOT compilation failed");
    assert!(!obj_bytes.is_empty());
}

// ---------------------------------------------------------------------------
// Misc Operations (26–30)
// ---------------------------------------------------------------------------

// 26. Bitwise ops: AND, OR, XOR, SHL, SHR
#[test]
fn jit_bitwise_ops() {
    // fn bitwise(a: i64, b: i64) -> i64 {
    //     let t1 = a & b;
    //     let t2 = a | b;
    //     let t3 = a ^ b;
    //     return t3;
    // }
    let func = make_function(
        "bitwise",
        vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::I64,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::I64,
            },
        ],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("a".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("t1".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(3),
                name: Some("t2".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(4),
                name: Some("t3".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::Assign {
                    dest: LocalId(2),
                    value: RValue::BinOp {
                        op: MirBinOp::BitAnd,
                        left: Operand::Local(LocalId(0)),
                        right: Operand::Local(LocalId(1)),
                    },
                },
                Instruction::Assign {
                    dest: LocalId(3),
                    value: RValue::BinOp {
                        op: MirBinOp::BitOr,
                        left: Operand::Local(LocalId(0)),
                        right: Operand::Local(LocalId(1)),
                    },
                },
                Instruction::Assign {
                    dest: LocalId(4),
                    value: RValue::BinOp {
                        op: MirBinOp::BitXor,
                        left: Operand::Local(LocalId(0)),
                        right: Operand::Local(LocalId(1)),
                    },
                },
            ],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(4)))),
        }],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64, i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    // XOR: 0xFF ^ 0x0F = 0xF0
    assert_eq!(f(0xFF, 0x0F), 0xF0);
    // XOR: same values = 0
    assert_eq!(f(42, 42), 0);
    // XOR: 0 ^ x = x
    assert_eq!(f(0, 123), 123);
}

// 27. Bitwise AND separately
#[test]
fn jit_bitwise_and() {
    let func = make_function(
        "bit_and",
        vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::I64,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::I64,
            },
        ],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("a".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("r".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(2),
                value: RValue::BinOp {
                    op: MirBinOp::BitAnd,
                    left: Operand::Local(LocalId(0)),
                    right: Operand::Local(LocalId(1)),
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
        }],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64, i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(0xFF, 0x0F), 0x0F);
    assert_eq!(f(0, 0xFF), 0);
    assert_eq!(f(0b1010, 0b1100), 0b1000);
}

// 28. Shift left and right
#[test]
fn jit_shift_ops() {
    // fn shl(a: i64, b: i64) -> i64 { return a << b; }
    let func = make_function(
        "shl_fn",
        vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::I64,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::I64,
            },
        ],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("a".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("r".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(2),
                value: RValue::BinOp {
                    op: MirBinOp::Shl,
                    left: Operand::Local(LocalId(0)),
                    right: Operand::Local(LocalId(1)),
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
        }],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64, i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(1, 4), 16); // 1 << 4 = 16
    assert_eq!(f(3, 2), 12); // 3 << 2 = 12
    assert_eq!(f(1, 0), 1); // 1 << 0 = 1
}

// 29. Not-equal comparison
#[test]
fn jit_comparison_neq() {
    // fn neq(a: i64, b: i64) -> i64 {
    //     if a != b { return 1; } else { return 0; }
    // }
    let func = make_function(
        "neq_fn",
        vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::I64,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::I64,
            },
        ],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("a".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("c".into()),
                ty: MirType::Bool,
                mutable: false,
            },
        ],
        vec![
            BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(2),
                    value: RValue::BinOp {
                        op: MirBinOp::Neq,
                        left: Operand::Local(LocalId(0)),
                        right: Operand::Local(LocalId(1)),
                    },
                }],
                terminator: Terminator::Branch {
                    cond: Operand::Local(LocalId(2)),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            BasicBlock {
                id: BlockId(1),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(1)))),
            },
            BasicBlock {
                id: BlockId(2),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(0)))),
            },
        ],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64, i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(1, 2), 1); // not equal
    assert_eq!(f(5, 5), 0); // equal
    assert_eq!(f(0, 1), 1); // not equal
}

// 30. Unary negate (integer)
#[test]
fn jit_unary_negate() {
    let func = make_function(
        "negate",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("r".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(1),
                value: RValue::UnOp {
                    op: MirUnOp::Neg,
                    operand: Operand::Local(LocalId(0)),
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(5), -5);
    assert_eq!(f(-3), 3);
    assert_eq!(f(0), 0);
}

// 31. Unary NOT (boolean)
#[test]
fn jit_unary_not() {
    // fn not_fn(x: i64) -> i64 {
    //     let b = x > 0;   // Bool
    //     let nb = !b;      // Not
    //     if nb { return 1; } else { return 0; }
    // }
    let func = make_function(
        "not_fn",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::Bool,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("nb".into()),
                ty: MirType::Bool,
                mutable: false,
            },
        ],
        vec![
            BasicBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Assign {
                        dest: LocalId(1),
                        value: RValue::BinOp {
                            op: MirBinOp::Gt,
                            left: Operand::Local(LocalId(0)),
                            right: Operand::Constant(Constant::Int(0)),
                        },
                    },
                    Instruction::Assign {
                        dest: LocalId(2),
                        value: RValue::UnOp {
                            op: MirUnOp::Not,
                            operand: Operand::Local(LocalId(1)),
                        },
                    },
                ],
                terminator: Terminator::Branch {
                    cond: Operand::Local(LocalId(2)),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            BasicBlock {
                id: BlockId(1),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(1)))),
            },
            BasicBlock {
                id: BlockId(2),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(0)))),
            },
        ],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    // NOT(x > 0): if x > 0, b=true, nb=false -> returns 0
    // if x <= 0, b=false, nb=true -> returns 1
    assert_eq!(f(5), 0);
    assert_eq!(f(0), 1);
    assert_eq!(f(-1), 1);
}

// 32. Cast i32 to i64 (integer widening)
#[test]
fn jit_cast_i32_to_i64() {
    // fn widen(x: i32) -> i64 { return x as i64; }
    let func = make_function(
        "widen",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I32,
        }],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I32,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("r".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(1),
                value: RValue::Cast {
                    operand: Operand::Local(LocalId(0)),
                    ty: MirType::I64,
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i32) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(42), 42i64);
    assert_eq!(f(-10), -10i64);
    assert_eq!(f(0), 0i64);
}

// 33. Cast i64 to i32 (narrowing)
#[test]
fn jit_cast_i64_to_i32() {
    let func = make_function(
        "narrow",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::I32,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("r".into()),
                ty: MirType::I32,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(1),
                value: RValue::Cast {
                    operand: Operand::Local(LocalId(0)),
                    ty: MirType::I32,
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64) -> i32 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(42), 42i32);
    assert_eq!(f(-1), -1i32);
    assert_eq!(f(0), 0i32);
}

// 34. Cast i64 to f64
#[test]
fn jit_cast_int_to_float() {
    let func = make_function(
        "to_float",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::F64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("r".into()),
                ty: MirType::F64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(1),
                value: RValue::Cast {
                    operand: Operand::Local(LocalId(0)),
                    ty: MirType::F64,
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64) -> f64 = unsafe { std::mem::transmute(ptr) };
    assert!((f(10) - 10.0).abs() < f64::EPSILON);
    assert!((f(-5) - (-5.0)).abs() < f64::EPSILON);
    assert!((f(0) - 0.0).abs() < f64::EPSILON);
}

// 35. Cast f64 to i64
#[test]
fn jit_cast_float_to_int() {
    let func = make_function(
        "to_int",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::F64,
        }],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::F64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("r".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(1),
                value: RValue::Cast {
                    operand: Operand::Local(LocalId(0)),
                    ty: MirType::I64,
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(f64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(10.9), 10); // truncates toward zero
    assert_eq!(f(-3.7), -3);
    assert_eq!(f(0.0), 0);
}

// 36. Return constant directly
#[test]
fn jit_return_constant() {
    let func = make_function(
        "const_fn",
        vec![],
        MirType::I64,
        vec![],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![],
            terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(12345)))),
        }],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn() -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(), 12345);
}

// 37. Use (copy) a parameter
#[test]
fn jit_use_copy() {
    let func = make_function(
        "identity64",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("r".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(1),
                value: RValue::Use(Operand::Local(LocalId(0))),
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(42), 42);
    assert_eq!(f(-1), -1);
    assert_eq!(f(0), 0);
}

// 38. Switch (multi-way branch) terminator
#[test]
fn jit_switch_terminator() {
    // fn classify_digit(x: i64) -> i64 {
    //     switch x {
    //         0 => return 100,
    //         1 => return 200,
    //         2 => return 300,
    //         _ => return -1,
    //     }
    // }
    let func = make_function(
        "classify_digit",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        MirType::I64,
        vec![MirLocal {
            id: LocalId(0),
            name: Some("x".into()),
            ty: MirType::I64,
            mutable: false,
        }],
        vec![
            BasicBlock {
                id: BlockId(0),
                instructions: vec![],
                terminator: Terminator::Switch {
                    value: Operand::Local(LocalId(0)),
                    targets: vec![(0, BlockId(1)), (1, BlockId(2)), (2, BlockId(3))],
                    default: BlockId(4),
                },
            },
            BasicBlock {
                id: BlockId(1),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(100)))),
            },
            BasicBlock {
                id: BlockId(2),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(200)))),
            },
            BasicBlock {
                id: BlockId(3),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(300)))),
            },
            BasicBlock {
                id: BlockId(4),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(-1)))),
            },
        ],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(0), 100);
    assert_eq!(f(1), 200);
    assert_eq!(f(2), 300);
    assert_eq!(f(3), -1);
    assert_eq!(f(99), -1);
}

// 38b. Switch on an i32 subject -- regression for verifier error
// "arg 1 has type i64, expected i32". Case constants must be sized to the
// subject value's type, not unconditionally i64.
#[test]
fn jit_switch_on_i32_subject() {
    // fn classify_i32(x: i32) -> i64 {
    //     match x { 0 => 100, 1 => 200, _ => -1 }
    // }
    let func = make_function(
        "classify_i32",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::I32,
        }],
        MirType::I64,
        vec![MirLocal {
            id: LocalId(0),
            name: Some("x".into()),
            ty: MirType::I32,
            mutable: false,
        }],
        vec![
            BasicBlock {
                id: BlockId(0),
                instructions: vec![],
                terminator: Terminator::Switch {
                    value: Operand::Local(LocalId(0)),
                    targets: vec![(0, BlockId(1)), (1, BlockId(2))],
                    default: BlockId(3),
                },
            },
            BasicBlock {
                id: BlockId(1),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(100)))),
            },
            BasicBlock {
                id: BlockId(2),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(200)))),
            },
            BasicBlock {
                id: BlockId(3),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(-1)))),
            },
        ],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT compile must succeed without verifier error");
    let f: extern "C" fn(i32) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(0), 100);
    assert_eq!(f(1), 200);
    assert_eq!(f(42), -1);
}

// 39. ConstBool usage
#[test]
fn jit_const_bool() {
    // fn always_true() -> i64 {
    //     let b = true;
    //     if b { return 1; } else { return 0; }
    // }
    let func = make_function(
        "always_true",
        vec![],
        MirType::I64,
        vec![MirLocal {
            id: LocalId(0),
            name: Some("b".into()),
            ty: MirType::Bool,
            mutable: false,
        }],
        vec![
            BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::ConstBool(true),
                }],
                terminator: Terminator::Branch {
                    cond: Operand::Local(LocalId(0)),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            BasicBlock {
                id: BlockId(1),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(1)))),
            },
            BasicBlock {
                id: BlockId(2),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(0)))),
            },
        ],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn() -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(), 1);
}

// 40. ConstFloat usage
#[test]
fn jit_const_float() {
    let func = make_function(
        "get_pi",
        vec![],
        MirType::F64,
        vec![MirLocal {
            id: LocalId(0),
            name: Some("pi".into()),
            ty: MirType::F64,
            mutable: false,
        }],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(0),
                value: RValue::ConstFloat(3.14159265358979),
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
        }],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    assert!((f() - 3.14159265358979).abs() < f64::EPSILON);
}

// 41. Float negate (unary)
#[test]
fn jit_float_negate() {
    let func = make_function(
        "fneg",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::F64,
        }],
        MirType::F64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::F64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("r".into()),
                ty: MirType::F64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(1),
                value: RValue::UnOp {
                    op: MirUnOp::Neg,
                    operand: Operand::Local(LocalId(0)),
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(f64) -> f64 = unsafe { std::mem::transmute(ptr) };
    assert!((f(3.0) - (-3.0)).abs() < f64::EPSILON);
    assert!((f(-7.5) - 7.5).abs() < f64::EPSILON);
    assert!((f(0.0) - 0.0).abs() < f64::EPSILON);
}

// 42. Bool OR operation
#[test]
fn jit_bool_or() {
    let func = make_function(
        "bool_or",
        vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::I64,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::I64,
            },
        ],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("a".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("ca".into()),
                ty: MirType::Bool,
                mutable: false,
            },
            MirLocal {
                id: LocalId(3),
                name: Some("cb".into()),
                ty: MirType::Bool,
                mutable: false,
            },
            MirLocal {
                id: LocalId(4),
                name: Some("r".into()),
                ty: MirType::Bool,
                mutable: false,
            },
        ],
        vec![
            BasicBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Assign {
                        dest: LocalId(2),
                        value: RValue::BinOp {
                            op: MirBinOp::Gt,
                            left: Operand::Local(LocalId(0)),
                            right: Operand::Constant(Constant::Int(0)),
                        },
                    },
                    Instruction::Assign {
                        dest: LocalId(3),
                        value: RValue::BinOp {
                            op: MirBinOp::Gt,
                            left: Operand::Local(LocalId(1)),
                            right: Operand::Constant(Constant::Int(0)),
                        },
                    },
                    Instruction::Assign {
                        dest: LocalId(4),
                        value: RValue::BinOp {
                            op: MirBinOp::Or,
                            left: Operand::Local(LocalId(2)),
                            right: Operand::Local(LocalId(3)),
                        },
                    },
                ],
                terminator: Terminator::Branch {
                    cond: Operand::Local(LocalId(4)),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            BasicBlock {
                id: BlockId(1),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(1)))),
            },
            BasicBlock {
                id: BlockId(2),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(0)))),
            },
        ],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64, i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(1, 1), 1);
    assert_eq!(f(1, 0), 1);
    assert_eq!(f(0, 1), 1);
    assert_eq!(f(0, 0), 0);
}

// 43. Shift right
#[test]
fn jit_shift_right() {
    let func = make_function(
        "shr_fn",
        vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::I64,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::I64,
            },
        ],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("a".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("r".into()),
                ty: MirType::I64,
                mutable: false,
            },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(2),
                value: RValue::BinOp {
                    op: MirBinOp::Shr,
                    left: Operand::Local(LocalId(0)),
                    right: Operand::Local(LocalId(1)),
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
        }],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64, i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(16, 4), 1); // 16 >> 4 = 1
    assert_eq!(f(255, 4), 15); // 255 >> 4 = 15
    assert_eq!(f(1, 0), 1); // 1 >> 0 = 1
}

// 44. AOT: ARC retain + release emitted in a module
#[test]
fn aot_arc_retain_release() {
    // Build a function that does arc_retain + arc_release on a Shared param.
    // This re-verifies the ARC symbols from test 4 with a simpler function.
    let func = make_function(
        "arc_rr_test",
        vec![MirParam {
            local: LocalId(0),
            ty: MirType::Shared(Box::new(MirType::I64)),
        }],
        MirType::Void,
        vec![MirLocal {
            id: LocalId(0),
            name: Some("ptr".into()),
            ty: MirType::Shared(Box::new(MirType::I64)),
            mutable: false,
        }],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::ArcRetain { ptr: LocalId(0) },
                Instruction::ArcRelease { ptr: LocalId(0) },
            ],
            terminator: Terminator::Return(None),
        }],
    );
    let module = MirModule {
        functions: vec![func],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };
    let obj_bytes = codegen::compile_module(&module).expect("AOT failed");
    assert!(!obj_bytes.is_empty());
    let obj_str = String::from_utf8_lossy(&obj_bytes);
    assert!(
        obj_str.contains("kryos_arc_retain"),
        "should reference kryos_arc_retain"
    );
    assert!(
        obj_str.contains("kryos_arc_release"),
        "should reference kryos_arc_release"
    );
}

// 45. Equality comparison
#[test]
fn jit_comparison_eq() {
    let func = make_function(
        "eq_fn",
        vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::I64,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::I64,
            },
        ],
        MirType::I64,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("a".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("b".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("c".into()),
                ty: MirType::Bool,
                mutable: false,
            },
        ],
        vec![
            BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(2),
                    value: RValue::BinOp {
                        op: MirBinOp::Eq,
                        left: Operand::Local(LocalId(0)),
                        right: Operand::Local(LocalId(1)),
                    },
                }],
                terminator: Terminator::Branch {
                    cond: Operand::Local(LocalId(2)),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            BasicBlock {
                id: BlockId(1),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(1)))),
            },
            BasicBlock {
                id: BlockId(2),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(0)))),
            },
        ],
    );
    let ptr = jit::jit_compile_function(&func).expect("JIT failed");
    let f: extern "C" fn(i64, i64) -> i64 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(f(5, 5), 1);
    assert_eq!(f(5, 6), 0);
    assert_eq!(f(0, 0), 1);
}
