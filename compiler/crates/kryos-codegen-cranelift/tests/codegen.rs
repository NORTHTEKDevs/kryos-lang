//! Integration tests for the Cranelift codegen backend.

use std::collections::HashMap;
use kryos_mir::ir::*;
use kryos_codegen_cranelift::jit;
use kryos_codegen_cranelift::codegen;

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
            MirParam { local: LocalId(0), ty: MirType::I32 },
            MirParam { local: LocalId(1), ty: MirType::I32 },
        ],
        MirType::I32,
        vec![
            MirLocal { id: LocalId(0), name: Some("a".into()), ty: MirType::I32, mutable: false },
            MirLocal { id: LocalId(1), name: Some("b".into()), ty: MirType::I32, mutable: false },
            MirLocal { id: LocalId(2), name: Some("result".into()), ty: MirType::I32, mutable: false },
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
            MirParam { local: LocalId(0), ty: MirType::I32 },
            MirParam { local: LocalId(1), ty: MirType::I32 },
        ],
        MirType::I32,
        vec![
            MirLocal { id: LocalId(0), name: Some("a".into()), ty: MirType::I32, mutable: false },
            MirLocal { id: LocalId(1), name: Some("b".into()), ty: MirType::I32, mutable: false },
            MirLocal { id: LocalId(2), name: Some("cond".into()), ty: MirType::Bool, mutable: false },
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
        vec![MirParam { local: LocalId(0), ty: MirType::I64 }],
        MirType::I64,
        vec![
            MirLocal { id: LocalId(0), name: Some("n".into()), ty: MirType::I64, mutable: false },
            MirLocal { id: LocalId(1), name: Some("i".into()), ty: MirType::I64, mutable: true },
            MirLocal { id: LocalId(2), name: Some("sum".into()), ty: MirType::I64, mutable: true },
            MirLocal { id: LocalId(3), name: Some("cond".into()), ty: MirType::Bool, mutable: false },
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
    assert_eq!(f(0), 0);       // sum of empty range
    assert_eq!(f(1), 0);       // sum(0..1) = 0
    assert_eq!(f(5), 10);      // 0+1+2+3+4 = 10
    assert_eq!(f(10), 45);     // 0+1+2+...+9 = 45
    assert_eq!(f(100), 4950);  // n*(n-1)/2 = 4950
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
        vec![MirParam { local: LocalId(0), ty: MirType::Shared(Box::new(MirType::I64)) }],
        MirType::Void,
        vec![
            MirLocal {
                id: LocalId(0),
                name: Some("ptr".into()),
                ty: MirType::Shared(Box::new(MirType::I64)),
                mutable: false,
            },
        ],
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
            MirParam { local: LocalId(0), ty: MirType::F64 },
            MirParam { local: LocalId(1), ty: MirType::F64 },
        ],
        MirType::F64,
        vec![
            MirLocal { id: LocalId(0), name: Some("a".into()), ty: MirType::F64, mutable: false },
            MirLocal { id: LocalId(1), name: Some("b".into()), ty: MirType::F64, mutable: false },
            MirLocal { id: LocalId(2), name: Some("sum".into()), ty: MirType::F64, mutable: false },
            MirLocal { id: LocalId(3), name: Some("product".into()), ty: MirType::F64, mutable: false },
            MirLocal { id: LocalId(4), name: Some("result".into()), ty: MirType::F64, mutable: false },
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
    assert!(
        result.abs() < f64::EPSILON,
        "expected 0.0, got {result}"
    );

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
            MirParam { local: LocalId(0), ty: MirType::F64 },
            MirParam { local: LocalId(1), ty: MirType::F64 },
        ],
        MirType::F64,
        vec![
            MirLocal { id: LocalId(0), name: Some("a".into()), ty: MirType::F64, mutable: false },
            MirLocal { id: LocalId(1), name: Some("b".into()), ty: MirType::F64, mutable: false },
            MirLocal { id: LocalId(2), name: Some("result".into()), ty: MirType::F64, mutable: false },
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
        vec![MirParam { local: LocalId(0), ty: MirType::I64 }],
        MirType::I64,
        vec![
            MirLocal { id: LocalId(0), name: Some("x".into()), ty: MirType::I64, mutable: false },
        ],
        vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
        }],
    );

    let module = MirModule {
        functions: vec![func],
        struct_defs: HashMap::new(),
    };

    let obj_bytes = codegen::compile_module(&module).expect("AOT compilation failed");
    assert!(obj_bytes.len() > 10, "object file too small: {} bytes", obj_bytes.len());
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
            MirParam { local: LocalId(0), ty: MirType::I64 },
            MirParam { local: LocalId(1), ty: MirType::I64 },
        ],
        MirType::I64,
        vec![
            MirLocal { id: LocalId(0), name: Some("a".into()), ty: MirType::I64, mutable: false },
            MirLocal { id: LocalId(1), name: Some("b".into()), ty: MirType::I64, mutable: false },
            MirLocal { id: LocalId(2), name: Some("diff".into()), ty: MirType::I64, mutable: false },
            MirLocal { id: LocalId(3), name: Some("product".into()), ty: MirType::I64, mutable: false },
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
    };

    let obj_bytes = codegen::compile_module(&module).expect("AOT compilation of struct access failed");
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
    };

    let obj_bytes =
        codegen::compile_module(&module).expect("AOT compilation of f64 struct access failed");
    assert!(
        obj_bytes.len() > 10,
        "object file too small: {} bytes",
        obj_bytes.len()
    );
}
