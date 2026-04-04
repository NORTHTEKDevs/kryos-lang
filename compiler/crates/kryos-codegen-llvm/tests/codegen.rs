//! Integration tests for the LLVM IR text emitter.

use std::collections::HashMap;
use kryos_codegen_llvm::{emit_module, EmitOptions, OptLevel};
use kryos_mir::ir::*;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Shorthand to build a minimal MIR module with a single function.
fn module_with(func: MirFunction) -> MirModule {
    MirModule {
        functions: vec![func],
        struct_defs: HashMap::new(),
    }
}

/// Build a simple `fn add(a: i32, b: i32) -> i32 { return a + b; }`.
fn make_add_function() -> MirFunction {
    MirFunction {
        name: "add".into(),
        params: vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::I32,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::I32,
            },
        ],
        ret_ty: MirType::I32,
        locals: vec![
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
                name: None,
                ty: MirType::I32,
                mutable: false,
            },
        ],
        blocks: vec![BasicBlock {
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
    }
}

// ---------------------------------------------------------------------------
// 1. Simple add function
// ---------------------------------------------------------------------------

#[test]
fn test_emit_add_function() {
    let module = module_with(make_add_function());
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    // Should define the function with correct signature.
    assert!(ir.contains("define i32 @add(i32 %_0, i32 %_1)"));
    // Should contain the add instruction.
    assert!(ir.contains("add i32 %_0, %_1"));
    // Should contain a return.
    assert!(ir.contains("ret i32 %_2"));
}

// ---------------------------------------------------------------------------
// 2. If/else (branch)
// ---------------------------------------------------------------------------

#[test]
fn test_emit_branch() {
    let func = MirFunction {
        name: "choose".into(),
        params: vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::Bool,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::I32,
            },
            MirParam {
                local: LocalId(2),
                ty: MirType::I32,
            },
        ],
        ret_ty: MirType::I32,
        locals: vec![
            MirLocal {
                id: LocalId(0),
                name: Some("cond".into()),
                ty: MirType::Bool,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: Some("a".into()),
                ty: MirType::I32,
                mutable: false,
            },
            MirLocal {
                id: LocalId(2),
                name: Some("b".into()),
                ty: MirType::I32,
                mutable: false,
            },
        ],
        blocks: vec![
            // bb0: entry — branch on cond.
            BasicBlock {
                id: BlockId(0),
                instructions: vec![],
                terminator: Terminator::Branch {
                    cond: Operand::Local(LocalId(0)),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            // bb1: return a.
            BasicBlock {
                id: BlockId(1),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
            },
            // bb2: return b.
            BasicBlock {
                id: BlockId(2),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
            },
        ],
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("br i1 %_0, label %bb1, label %bb2"));
    assert!(ir.contains("ret i32 %_1"));
    assert!(ir.contains("ret i32 %_2"));
}

// ---------------------------------------------------------------------------
// 3. Return
// ---------------------------------------------------------------------------

#[test]
fn test_emit_return_void() {
    let func = MirFunction {
        name: "noop".into(),
        params: vec![],
        ret_ty: MirType::Void,
        locals: vec![],
        blocks: vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![],
            terminator: Terminator::Return(None),
        }],
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("define void @noop()"));
    assert!(ir.contains("ret void"));
}

#[test]
fn test_emit_return_value() {
    let func = MirFunction {
        name: "forty_two".into(),
        params: vec![],
        ret_ty: MirType::I32,
        locals: vec![MirLocal {
            id: LocalId(0),
            name: None,
            ty: MirType::I32,
            mutable: false,
        }],
        blocks: vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(0),
                value: RValue::ConstInt(42),
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
        }],
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("ret i32 %_0"));
}

// ---------------------------------------------------------------------------
// 4. ARC runtime declarations
// ---------------------------------------------------------------------------

#[test]
fn test_arc_declarations_present_when_used() {
    let func = MirFunction {
        name: "use_arc".into(),
        params: vec![MirParam {
            local: LocalId(0),
            ty: MirType::Ptr(Box::new(MirType::I32)),
        }],
        ret_ty: MirType::Void,
        locals: vec![MirLocal {
            id: LocalId(0),
            name: Some("p".into()),
            ty: MirType::Ptr(Box::new(MirType::I32)),
            mutable: false,
        }],
        blocks: vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::ArcRetain { ptr: LocalId(0) },
                Instruction::ArcRelease { ptr: LocalId(0) },
            ],
            terminator: Terminator::Return(None),
        }],
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("declare ptr @kryos_arc_alloc(i64, ptr)"));
    assert!(ir.contains("declare void @kryos_arc_retain(ptr)"));
    assert!(ir.contains("declare void @kryos_arc_release(ptr)"));
    // The function body should contain the calls.
    assert!(ir.contains("call void @kryos_arc_retain(ptr %_0)"));
    assert!(ir.contains("call void @kryos_arc_release(ptr %_0)"));
}

#[test]
fn test_arc_declarations_absent_when_unused() {
    // Simple function with no ARC ops.
    let module = module_with(make_add_function());
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(!ir.contains("kryos_arc_alloc"));
    assert!(!ir.contains("kryos_arc_retain"));
    assert!(!ir.contains("kryos_arc_release"));
}

// ---------------------------------------------------------------------------
// 5. String constants as globals
// ---------------------------------------------------------------------------

#[test]
fn test_string_constants_emitted_as_globals() {
    let func = MirFunction {
        name: "greet".into(),
        params: vec![],
        ret_ty: MirType::Str,
        locals: vec![MirLocal {
            id: LocalId(0),
            name: None,
            ty: MirType::Str,
            mutable: false,
        }],
        blocks: vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(0),
                value: RValue::ConstString("hello world".into()),
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
        }],
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    // Should emit a global constant with the string + null terminator.
    assert!(ir.contains("@.str.0 = private unnamed_addr constant [12 x i8] c\"hello world\\00\""));
    // The function body should reference it via GEP.
    assert!(ir.contains("getelementptr [12 x i8], ptr @.str.0"));
}

// ---------------------------------------------------------------------------
// 6. Optimization levels and target triple
// ---------------------------------------------------------------------------

#[test]
fn test_target_triple_in_output() {
    let module = module_with(make_add_function());

    let opts = EmitOptions {
        opt_level: OptLevel::O2,
        target_triple: Some("x86_64-pc-linux-gnu".into()),
        target_datalayout: Some("e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128".into()),
    };

    let ir = emit_module(&module, &opts).unwrap();

    assert!(ir.contains("target triple = \"x86_64-pc-linux-gnu\""));
    assert!(ir.contains("target datalayout = \"e-m:e-p270:32:32"));
}

#[test]
fn test_no_triple_when_unspecified() {
    let module = module_with(make_add_function());
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(!ir.contains("target triple"));
    assert!(!ir.contains("target datalayout"));
}

#[test]
fn test_different_opt_levels_compile() {
    let module = module_with(make_add_function());

    for level in &[OptLevel::O0, OptLevel::O1, OptLevel::O2, OptLevel::O3] {
        let opts = EmitOptions {
            opt_level: *level,
            ..EmitOptions::default()
        };
        let result = emit_module(&module, &opts);
        assert!(result.is_ok(), "Failed with opt level {:?}", level);
    }
}

// ---------------------------------------------------------------------------
// 7. Type mapping
// ---------------------------------------------------------------------------

#[test]
fn test_type_mapping_integers() {
    use kryos_codegen_llvm::codegen::mir_type_to_llvm;

    assert_eq!(mir_type_to_llvm(&MirType::I8), "i8");
    assert_eq!(mir_type_to_llvm(&MirType::I16), "i16");
    assert_eq!(mir_type_to_llvm(&MirType::I32), "i32");
    assert_eq!(mir_type_to_llvm(&MirType::I64), "i64");
    assert_eq!(mir_type_to_llvm(&MirType::I128), "i128");
}

#[test]
fn test_type_mapping_unsigned_to_signed() {
    use kryos_codegen_llvm::codegen::mir_type_to_llvm;

    // LLVM has no unsigned — they map to the same iN types.
    assert_eq!(mir_type_to_llvm(&MirType::U8), "i8");
    assert_eq!(mir_type_to_llvm(&MirType::U16), "i16");
    assert_eq!(mir_type_to_llvm(&MirType::U32), "i32");
    assert_eq!(mir_type_to_llvm(&MirType::U64), "i64");
    assert_eq!(mir_type_to_llvm(&MirType::U128), "i128");
}

#[test]
fn test_type_mapping_float() {
    use kryos_codegen_llvm::codegen::mir_type_to_llvm;

    assert_eq!(mir_type_to_llvm(&MirType::F32), "float");
    assert_eq!(mir_type_to_llvm(&MirType::F64), "double");
}

#[test]
fn test_type_mapping_bool_char_str_void() {
    use kryos_codegen_llvm::codegen::mir_type_to_llvm;

    assert_eq!(mir_type_to_llvm(&MirType::Bool), "i1");
    assert_eq!(mir_type_to_llvm(&MirType::Char), "i32");
    assert_eq!(mir_type_to_llvm(&MirType::Str), "ptr");
    assert_eq!(mir_type_to_llvm(&MirType::Void), "void");
}

#[test]
fn test_type_mapping_ptr_shared() {
    use kryos_codegen_llvm::codegen::mir_type_to_llvm;

    // Opaque pointers since LLVM 15+.
    assert_eq!(mir_type_to_llvm(&MirType::Ptr(Box::new(MirType::I32))), "ptr");
    assert_eq!(
        mir_type_to_llvm(&MirType::Shared(Box::new(MirType::I64))),
        "ptr"
    );
}

#[test]
fn test_type_mapping_array() {
    use kryos_codegen_llvm::codegen::mir_type_to_llvm;

    assert_eq!(
        mir_type_to_llvm(&MirType::Array(Box::new(MirType::I32), Some(10))),
        "[10 x i32]"
    );
    // Unsized array — pointer.
    assert_eq!(
        mir_type_to_llvm(&MirType::Array(Box::new(MirType::I32), None)),
        "ptr"
    );
}

#[test]
fn test_type_mapping_tuple() {
    use kryos_codegen_llvm::codegen::mir_type_to_llvm;

    assert_eq!(
        mir_type_to_llvm(&MirType::Tuple(vec![MirType::I32, MirType::F64])),
        "{ i32, double }"
    );
}

#[test]
fn test_type_mapping_struct() {
    use kryos_codegen_llvm::codegen::mir_type_to_llvm;

    assert_eq!(mir_type_to_llvm(&MirType::Struct("Point".into())), "%Point");
}

#[test]
fn test_type_mapping_function() {
    use kryos_codegen_llvm::codegen::mir_type_to_llvm;

    // Function types are represented as opaque pointers.
    assert_eq!(
        mir_type_to_llvm(&MirType::Function {
            params: vec![MirType::I32],
            ret: Box::new(MirType::I64),
        }),
        "ptr"
    );
}

// ---------------------------------------------------------------------------
// 8. Additional instruction coverage
// ---------------------------------------------------------------------------

#[test]
fn test_float_operations() {
    let func = MirFunction {
        name: "fadd_test".into(),
        params: vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::F64,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::F64,
            },
        ],
        ret_ty: MirType::F64,
        locals: vec![
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
                name: None,
                ty: MirType::F64,
                mutable: false,
            },
        ],
        blocks: vec![BasicBlock {
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
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("fadd double %_0, %_1"));
}

#[test]
fn test_comparison_operations() {
    let func = MirFunction {
        name: "lt_test".into(),
        params: vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::I32,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::I32,
            },
        ],
        ret_ty: MirType::Bool,
        locals: vec![
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
                name: None,
                ty: MirType::Bool,
                mutable: false,
            },
        ],
        blocks: vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(2),
                value: RValue::BinOp {
                    op: MirBinOp::Lt,
                    left: Operand::Local(LocalId(0)),
                    right: Operand::Local(LocalId(1)),
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
        }],
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("icmp slt i32 %_0, %_1"));
}

#[test]
fn test_switch_terminator() {
    let func = MirFunction {
        name: "switch_test".into(),
        params: vec![MirParam {
            local: LocalId(0),
            ty: MirType::I32,
        }],
        ret_ty: MirType::I32,
        locals: vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I32,
                mutable: false,
            },
        ],
        blocks: vec![
            // bb0: switch on x.
            BasicBlock {
                id: BlockId(0),
                instructions: vec![],
                terminator: Terminator::Switch {
                    value: Operand::Local(LocalId(0)),
                    targets: vec![(1, BlockId(1)), (2, BlockId(2))],
                    default: BlockId(3),
                },
            },
            // bb1: return 10.
            BasicBlock {
                id: BlockId(1),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(10)))),
            },
            // bb2: return 20.
            BasicBlock {
                id: BlockId(2),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(20)))),
            },
            // bb3: default — return 0.
            BasicBlock {
                id: BlockId(3),
                instructions: vec![],
                terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(0)))),
            },
        ],
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("switch i32 %_0, label %bb3"));
    assert!(ir.contains("i32 1, label %bb1"));
    assert!(ir.contains("i32 2, label %bb2"));
}

#[test]
fn test_goto_terminator() {
    let func = MirFunction {
        name: "goto_test".into(),
        params: vec![],
        ret_ty: MirType::Void,
        locals: vec![],
        blocks: vec![
            BasicBlock {
                id: BlockId(0),
                instructions: vec![],
                terminator: Terminator::Goto(BlockId(1)),
            },
            BasicBlock {
                id: BlockId(1),
                instructions: vec![],
                terminator: Terminator::Return(None),
            },
        ],
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("br label %bb1"));
}

#[test]
fn test_unreachable_terminator() {
    let func = MirFunction {
        name: "unreachable_test".into(),
        params: vec![],
        ret_ty: MirType::Void,
        locals: vec![],
        blocks: vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![],
            terminator: Terminator::Unreachable,
        }],
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("unreachable"));
}

#[test]
fn test_function_call() {
    let func = MirFunction {
        name: "caller".into(),
        params: vec![],
        ret_ty: MirType::I32,
        locals: vec![
            MirLocal {
                id: LocalId(0),
                name: None,
                ty: MirType::I32,
                mutable: false,
            },
        ],
        blocks: vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(0),
                value: RValue::Call {
                    func: "get_value".into(),
                    args: vec![
                        Operand::Constant(Constant::Int(1)),
                        Operand::Constant(Constant::Int(2)),
                    ],
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
        }],
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("call i32 @get_value(i64 1, i64 2)"));
}

#[test]
fn test_module_header() {
    let module = module_with(make_add_function());
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("; ModuleID = 'kryos_module'"));
    assert!(ir.contains("source_filename = \"kryos_module\""));
}

#[test]
fn test_multiple_functions() {
    let func1 = make_add_function();
    let func2 = MirFunction {
        name: "sub".into(),
        params: vec![
            MirParam {
                local: LocalId(0),
                ty: MirType::I32,
            },
            MirParam {
                local: LocalId(1),
                ty: MirType::I32,
            },
        ],
        ret_ty: MirType::I32,
        locals: vec![
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
                name: None,
                ty: MirType::I32,
                mutable: false,
            },
        ],
        blocks: vec![BasicBlock {
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
    };

    let module = MirModule {
        functions: vec![func1, func2],
        struct_defs: HashMap::new(),
    };
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("define i32 @add("));
    assert!(ir.contains("define i32 @sub("));
    assert!(ir.contains("sub i32 %_0, %_1"));
}

#[test]
fn test_unary_neg() {
    let func = MirFunction {
        name: "neg_test".into(),
        params: vec![MirParam {
            local: LocalId(0),
            ty: MirType::I32,
        }],
        ret_ty: MirType::I32,
        locals: vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I32,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: None,
                ty: MirType::I32,
                mutable: false,
            },
        ],
        blocks: vec![BasicBlock {
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
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("sub i32 0, %_0"));
}

#[test]
fn test_const_bool() {
    let func = MirFunction {
        name: "bool_test".into(),
        params: vec![],
        ret_ty: MirType::Bool,
        locals: vec![MirLocal {
            id: LocalId(0),
            name: None,
            ty: MirType::Bool,
            mutable: false,
        }],
        blocks: vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(0),
                value: RValue::ConstBool(true),
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
        }],
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("add i1 1, 0"));
    assert!(ir.contains("ret i1 %_0"));
}

#[test]
fn test_drop_is_noop() {
    let func = MirFunction {
        name: "drop_test".into(),
        params: vec![MirParam {
            local: LocalId(0),
            ty: MirType::I32,
        }],
        ret_ty: MirType::Void,
        locals: vec![MirLocal {
            id: LocalId(0),
            name: Some("x".into()),
            ty: MirType::I32,
            mutable: false,
        }],
        blocks: vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Drop {
                local: LocalId(0),
            }],
            terminator: Terminator::Return(None),
        }],
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    // Drop emits a comment, not a crash.
    assert!(ir.contains("; drop (no-op)"));
}

#[test]
fn test_nop_instruction() {
    let func = MirFunction {
        name: "nop_test".into(),
        params: vec![],
        ret_ty: MirType::Void,
        locals: vec![],
        blocks: vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Nop],
            terminator: Terminator::Return(None),
        }],
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    // Should compile without errors; nop produces no output.
    assert!(ir.contains("ret void"));
}

#[test]
fn test_cast_int_to_float() {
    let func = MirFunction {
        name: "cast_test".into(),
        params: vec![MirParam {
            local: LocalId(0),
            ty: MirType::I32,
        }],
        ret_ty: MirType::F64,
        locals: vec![
            MirLocal {
                id: LocalId(0),
                name: Some("x".into()),
                ty: MirType::I32,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: None,
                ty: MirType::F64,
                mutable: false,
            },
        ],
        blocks: vec![BasicBlock {
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
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("sitofp i32 %_0 to double"));
}
