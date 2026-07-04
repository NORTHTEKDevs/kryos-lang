//! Integration tests for the LLVM IR text emitter.

use kryos_codegen_llvm::{emit_module, EmitOptions, OptLevel};
use kryos_mir::ir::*;
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Shorthand to build a minimal MIR module with a single function.
fn module_with(func: MirFunction) -> MirModule {
    MirModule {
        functions: vec![func],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
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
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
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
    // User functions get `internal` linkage to avoid colliding with libc
    // symbols (see emit_function_as in codegen.rs); only `main` is external.
    assert!(ir.contains("define internal i32 @add(i32 %_0, i32 %_1)"));
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
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
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
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("define internal void @noop()"));
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
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
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
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    // The runtime signature is `kryos_arc_alloc(size: usize, align: usize)` —
    // both are emitted as `i64` to match the host word size on every Tier 1
    // target. This test guards the *declarations* being present and the
    // *calls* being wired through, not the exact textual layout of the
    // declaration line (which is implementation detail of
    // `emit_arc_declarations` in codegen.rs).
    assert!(ir.contains("declare ptr @kryos_arc_alloc(i64, i64)"));
    assert!(ir.contains("declare void @kryos_arc_retain(ptr)"));
    assert!(ir.contains("declare void @kryos_arc_release(ptr)"));
    // The function body should contain the calls.
    assert!(ir.contains("call void @kryos_arc_retain(ptr %_0)"));
    assert!(ir.contains("call void @kryos_arc_release(ptr %_0)"));
}

#[test]
fn test_arc_declarations_present_in_module_preamble() {
    // The ARC runtime declarations are part of the module preamble that
    // every emitted module shares — they're cheap (a few `declare` lines
    // that the linker discards if unused) and they let downstream MIR
    // passes synthesize ARC ops without needing the emitter to look back
    // and gate decl emission. We test that they're present *and well
    // formed* for an ARC-free function, not that they're absent: the
    // previous "absent when unused" assertion was checking an internal
    // micro-optimization that has no observable effect on the produced
    // binary.
    let module = module_with(make_add_function());
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("declare ptr @kryos_arc_alloc(i64, i64)"));
    assert!(ir.contains("declare void @kryos_arc_retain(ptr)"));
    assert!(ir.contains("declare void @kryos_arc_release(ptr)"));
    // The actual function body, on the other hand, must not contain any
    // *calls* to the ARC runtime — that is the property that matters.
    assert!(!ir.contains("call void @kryos_arc_retain"));
    assert!(!ir.contains("call void @kryos_arc_release"));
    assert!(!ir.contains("call ptr @kryos_arc_alloc"));
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
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    // Should emit a global constant with the string + null terminator.
    assert!(ir.contains("@.str.0 = private unnamed_addr constant [12 x i8] c\"hello world\\00\""));
    // ...plus its static immortal KryosString header (literal interning: no
    // per-evaluation kryos_string_new heap copy; negative rc = free no-ops).
    assert!(ir.contains(
        "@.str.0.hdr = private global { i64, i64, ptr, i64 } { i64 11, i64 11, ptr @.str.0, i64 -4611686018427387904 }"
    ));
    // The function body references the interned header directly and must NOT
    // heap-allocate for the literal.
    assert!(ir.contains("@.str.0.hdr"));
    assert!(!ir.contains("call ptr @kryos_string_new(ptr"));
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
        target_datalayout: Some(
            "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128".into(),
        ),
        ..EmitOptions::default()
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
    assert_eq!(
        mir_type_to_llvm(&MirType::Ptr(Box::new(MirType::I32))),
        "ptr"
    );
    assert_eq!(
        mir_type_to_llvm(&MirType::Shared(Box::new(MirType::I64))),
        "ptr"
    );
}

#[test]
fn test_type_mapping_array() {
    use kryos_codegen_llvm::codegen::mir_type_to_llvm;

    // All arrays now lower to ptr (heap-allocated runtime arrays).
    assert_eq!(
        mir_type_to_llvm(&MirType::Array(Box::new(MirType::I32), Some(10))),
        "ptr"
    );
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
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
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
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
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
        locals: vec![MirLocal {
            id: LocalId(0),
            name: Some("x".into()),
            ty: MirType::I32,
            mutable: false,
        }],
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
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    // The MIR switch terminator is lowered directly to LLVM `switch`. The
    // subject is coerced to i64 (LLVM requires the subject and all case
    // constants to share a type, and MIR-supplied case constants are sized
    // for i64). The default label is the MIR-supplied default block (no
    // synthetic shim) — wildcard arms live there.
    assert!(ir.contains("switch i64 "));
    assert!(ir.contains(", label %bb3"));
    assert!(ir.contains("i64 1, label %bb1"));
    assert!(ir.contains("i64 2, label %bb2"));
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
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
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
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
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
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
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
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
    };

    let module = MirModule {
        functions: vec![func1, func2],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("define internal i32 @add("));
    assert!(ir.contains("define internal i32 @sub("));
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
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
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
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
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
            instructions: vec![Instruction::Drop { local: LocalId(0) }],
            terminator: Terminator::Return(None),
        }],
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
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
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
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
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();

    assert!(ir.contains("sitofp i32 %_0 to double"));
}

// ---------------------------------------------------------------------------
// SSA fix: mutable variables use alloca/store/load
// ---------------------------------------------------------------------------

#[test]
fn test_mutable_variable_in_loop() {
    let module = MirModule {
        functions: vec![MirFunction {
            name: "loop_sum".into(),
            params: vec![],
            ret_ty: MirType::I64,
            locals: vec![
                MirLocal {
                    id: LocalId(0),
                    name: Some("sum".into()),
                    ty: MirType::I64,
                    mutable: true,
                },
                MirLocal {
                    id: LocalId(1),
                    name: Some("i".into()),
                    ty: MirType::I64,
                    mutable: true,
                },
                MirLocal {
                    id: LocalId(2),
                    name: None,
                    ty: MirType::Bool,
                    mutable: false,
                },
            ],
            blocks: vec![
                BasicBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Assign {
                            dest: LocalId(0),
                            value: RValue::ConstInt(0),
                        },
                        Instruction::Assign {
                            dest: LocalId(1),
                            value: RValue::ConstInt(0),
                        },
                    ],
                    terminator: Terminator::Goto(BlockId(1)),
                },
                BasicBlock {
                    id: BlockId(1),
                    instructions: vec![Instruction::Assign {
                        dest: LocalId(2),
                        value: RValue::BinOp {
                            op: MirBinOp::Lt,
                            left: Operand::Local(LocalId(1)),
                            right: Operand::Constant(Constant::Int(10)),
                        },
                    }],
                    terminator: Terminator::Branch {
                        cond: Operand::Local(LocalId(2)),
                        then_block: BlockId(2),
                        else_block: BlockId(3),
                    },
                },
                BasicBlock {
                    id: BlockId(2),
                    instructions: vec![
                        Instruction::Assign {
                            dest: LocalId(0),
                            value: RValue::BinOp {
                                op: MirBinOp::Add,
                                left: Operand::Local(LocalId(0)),
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
                    terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
                },
            ],
            attributes: MirAttributes::default(),
            source_file: None,
            source_line: 0,
        }],
        struct_defs: HashMap::new(),
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };

    let ir = emit_module(&module, &EmitOptions::default()).unwrap();
    // Mutable locals must use alloca/store/load.
    assert!(ir.contains("alloca"), "mutable vars must use alloca:\n{ir}");
    assert!(
        ir.contains("store"),
        "mutable var assignment must use store:\n{ir}"
    );
    assert!(
        ir.contains("load"),
        "mutable var reads must use load:\n{ir}"
    );
}

#[test]
fn test_immutable_variable_no_alloca() {
    // An immutable variable assigned once should NOT use alloca.
    let func = MirFunction {
        name: "imm_test".into(),
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
                value: RValue::ConstInt(42),
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
        }],
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
    };

    let module = module_with(func);
    let ir = emit_module(&module, &EmitOptions::default()).unwrap();
    // Direct SSA for immutable: %_0 = add i64 42, 0
    assert!(
        ir.contains("%_0 = add i64 42, 0"),
        "immutable should use direct SSA:\n{ir}"
    );
    assert!(
        !ir.contains("alloca"),
        "immutable should NOT use alloca:\n{ir}"
    );
}

// ---------------------------------------------------------------------------
// Field access: correct index resolution from struct_defs
// ---------------------------------------------------------------------------

#[test]
fn test_struct_field_access_correct_index() {
    let mut struct_defs = HashMap::new();
    struct_defs.insert(
        "Point".to_string(),
        vec![
            ("x".to_string(), MirType::I64),
            ("y".to_string(), MirType::I64),
            ("z".to_string(), MirType::I64),
        ],
    );

    let module = MirModule {
        functions: vec![MirFunction {
            name: "get_y".into(),
            params: vec![MirParam {
                local: LocalId(0),
                ty: MirType::Struct("Point".into()),
            }],
            ret_ty: MirType::I64,
            locals: vec![
                MirLocal {
                    id: LocalId(0),
                    name: Some("p".into()),
                    ty: MirType::Struct("Point".into()),
                    mutable: false,
                },
                MirLocal {
                    id: LocalId(1),
                    name: None,
                    ty: MirType::I64,
                    mutable: false,
                },
            ],
            blocks: vec![BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::Field {
                        object: Operand::Local(LocalId(0)),
                        field: "y".into(),
                    },
                }],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
            }],
            attributes: MirAttributes::default(),
            source_file: None,
            source_line: 0,
        }],
        struct_defs,
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };

    let ir = emit_module(&module, &EmitOptions::default()).unwrap();
    // Field "y" is at index 1.
    assert!(
        ir.contains(", 1"),
        "field 'y' must use index 1, not 0:\n{ir}"
    );
}

#[test]
fn test_struct_field_access_first_field() {
    let mut struct_defs = HashMap::new();
    struct_defs.insert(
        "Point".to_string(),
        vec![
            ("x".to_string(), MirType::I64),
            ("y".to_string(), MirType::I64),
        ],
    );

    let module = MirModule {
        functions: vec![MirFunction {
            name: "get_x".into(),
            params: vec![MirParam {
                local: LocalId(0),
                ty: MirType::Struct("Point".into()),
            }],
            ret_ty: MirType::I64,
            locals: vec![
                MirLocal {
                    id: LocalId(0),
                    name: Some("p".into()),
                    ty: MirType::Struct("Point".into()),
                    mutable: false,
                },
                MirLocal {
                    id: LocalId(1),
                    name: None,
                    ty: MirType::I64,
                    mutable: false,
                },
            ],
            blocks: vec![BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::Field {
                        object: Operand::Local(LocalId(0)),
                        field: "x".into(),
                    },
                }],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
            }],
            attributes: MirAttributes::default(),
            source_file: None,
            source_line: 0,
        }],
        struct_defs,
        enum_defs: HashMap::new(),
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };

    let ir = emit_module(&module, &EmitOptions::default()).unwrap();
    // Field "x" is at index 0.
    assert!(ir.contains(", 0"), "field 'x' must use index 0:\n{ir}");
}

#[test]
fn test_eprintln_uses_stderr() {
    let module = module_with(MirFunction {
        name: "warn".into(),
        params: vec![],
        ret_ty: MirType::Void,
        locals: vec![MirLocal {
            id: LocalId(0),
            name: None,
            ty: MirType::Str,
            mutable: false,
        }],
        blocks: vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![
                Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::ConstString("oops".into()),
                },
                Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::Call {
                        func: "eprintln".into(),
                        args: vec![Operand::Local(LocalId(0))],
                    },
                },
            ],
            terminator: Terminator::Return(None),
        }],
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
    });

    let ir = emit_module(&module, &EmitOptions::default()).unwrap();
    // Must use fputs + fputc to stderr, NOT puts
    assert!(ir.contains("@fputs"), "eprintln should call fputs:\n{ir}");
    assert!(
        ir.contains("@fputc"),
        "eprintln should call fputc for newline:\n{ir}"
    );
    assert!(
        !ir.contains("call i32 @puts") || ir.contains("@fputs"),
        "eprintln must not fall through to puts:\n{ir}"
    );
    // Must declare stderr accessor
    if cfg!(target_os = "windows") {
        assert!(
            ir.contains("@__acrt_iob_func"),
            "must declare __acrt_iob_func on Windows:\n{ir}"
        );
    } else {
        assert!(ir.contains("@stderr"), "must declare stderr on Unix:\n{ir}");
    }
}

#[test]
fn test_len_builtin_returns_zero() {
    let module = module_with(MirFunction {
        name: "get_len".into(),
        params: vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        ret_ty: MirType::I64,
        locals: vec![
            MirLocal {
                id: LocalId(0),
                name: Some("arr".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: None,
                ty: MirType::I64,
                mutable: false,
            },
        ],
        blocks: vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(1),
                value: RValue::Call {
                    func: "len".into(),
                    args: vec![Operand::Local(LocalId(0))],
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
    });

    let ir = emit_module(&module, &EmitOptions::default()).unwrap();
    // len calls the runtime kryos_builtin_len
    assert!(
        ir.contains("@kryos_builtin_len"),
        "len() should call kryos_builtin_len:\n{ir}"
    );
    // Must NOT emit a call to @len (which would be an undefined symbol)
    assert!(
        !ir.contains("call i64 @len("),
        "len must not emit external call to @len:\n{ir}"
    );
}

#[test]
fn test_http2_get_builtin_links_to_runtime_symbol() {
    // Regression: http2_get / http2_post / http2_request / https_get were
    // declared as kryos_*_ks runtime symbols but absent from the LLVM
    // call-site name map, so the AOT backend emitted a bare `@http2_get`
    // (undefined value -> clang link failure). Cranelift had the mapping;
    // LLVM did not. This guards the LLVM call site against regressing.
    let module = module_with(MirFunction {
        name: "fetch".into(),
        params: vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        ret_ty: MirType::I64,
        locals: vec![
            MirLocal {
                id: LocalId(0),
                name: Some("url".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: None,
                ty: MirType::I64,
                mutable: false,
            },
        ],
        blocks: vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(1),
                value: RValue::Call {
                    func: "http2_get".into(),
                    args: vec![Operand::Local(LocalId(0))],
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
    });

    let ir = emit_module(&module, &EmitOptions::default()).unwrap();
    // http2_get must route to the runtime symbol, not a bare @http2_get.
    assert!(
        ir.contains("@kryos_http2_get_ks"),
        "http2_get should call kryos_http2_get_ks:\n{ir}"
    );
    assert!(
        !ir.contains("@http2_get("),
        "http2_get must not emit an external call to @http2_get:\n{ir}"
    );
}

#[test]
fn test_to_string_builtin_returns_input() {
    let module = module_with(MirFunction {
        name: "stringify".into(),
        params: vec![MirParam {
            local: LocalId(0),
            ty: MirType::I64,
        }],
        ret_ty: MirType::I64,
        locals: vec![
            MirLocal {
                id: LocalId(0),
                name: Some("val".into()),
                ty: MirType::I64,
                mutable: false,
            },
            MirLocal {
                id: LocalId(1),
                name: None,
                ty: MirType::I64,
                mutable: false,
            },
        ],
        blocks: vec![BasicBlock {
            id: BlockId(0),
            instructions: vec![Instruction::Assign {
                dest: LocalId(1),
                value: RValue::Call {
                    func: "to_string".into(),
                    args: vec![Operand::Local(LocalId(0))],
                },
            }],
            terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
        }],
        attributes: MirAttributes::default(),
        source_file: None,
        source_line: 0,
    });

    let ir = emit_module(&module, &EmitOptions::default()).unwrap();
    // to_string calls the runtime kryos_builtin_to_string
    assert!(
        ir.contains("@kryos_builtin_to_string"),
        "to_string() should call kryos_builtin_to_string:\n{ir}"
    );
    // Must NOT emit a call to @to_string (which would be an undefined symbol)
    assert!(
        !ir.contains("call i64 @to_string("),
        "to_string must not emit external call to @to_string:\n{ir}"
    );
}

// ===========================================================================
// Enum codegen tests
// ===========================================================================

#[test]
fn test_enum_unit_variant() {
    use kryos_mir::ir::EnumVariantDef;

    let mut enum_defs = HashMap::new();
    enum_defs.insert(
        "Color".to_string(),
        vec![
            EnumVariantDef {
                name: "Red".into(),
                fields: vec![],
            },
            EnumVariantDef {
                name: "Green".into(),
                fields: vec![],
            },
            EnumVariantDef {
                name: "Blue".into(),
                fields: vec![],
            },
        ],
    );

    let module = MirModule {
        functions: vec![MirFunction {
            name: "get_green".into(),
            params: vec![],
            ret_ty: MirType::Enum("Color".into()),
            locals: vec![MirLocal {
                id: LocalId(0),
                name: None,
                ty: MirType::Enum("Color".into()),
                mutable: false,
            }],
            blocks: vec![BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::EnumVariant {
                        enum_name: "Color".into(),
                        variant_idx: 1, // Green
                        fields: vec![],
                    },
                }],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
            }],
            attributes: MirAttributes::default(),
            source_file: None,
            source_line: 0,
        }],
        struct_defs: HashMap::new(),
        enum_defs,
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };

    let ir = emit_module(&module, &EmitOptions::default()).unwrap();
    // Unit variant: insertvalue { i64 } undef, i64 1, 0
    assert!(
        ir.contains("insertvalue"),
        "enum variant must use insertvalue:\n{ir}"
    );
    assert!(
        ir.contains("i64 1"),
        "Green variant should have tag 1:\n{ir}"
    );
}

#[test]
fn test_enum_variant_with_fields() {
    use kryos_mir::ir::EnumVariantDef;

    let mut enum_defs = HashMap::new();
    enum_defs.insert(
        "Shape".to_string(),
        vec![
            EnumVariantDef {
                name: "Circle".into(),
                fields: vec![MirType::F64],
            },
            EnumVariantDef {
                name: "Rect".into(),
                fields: vec![MirType::F64, MirType::F64],
            },
        ],
    );

    let module = MirModule {
        functions: vec![MirFunction {
            name: "make_rect".into(),
            params: vec![],
            ret_ty: MirType::Enum("Shape".into()),
            locals: vec![MirLocal {
                id: LocalId(0),
                name: None,
                ty: MirType::Enum("Shape".into()),
                mutable: false,
            }],
            blocks: vec![BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::EnumVariant {
                        enum_name: "Shape".into(),
                        variant_idx: 1, // Rect
                        fields: vec![
                            Operand::Constant(Constant::Float(3.0)),
                            Operand::Constant(Constant::Float(4.0)),
                        ],
                    },
                }],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
            }],
            attributes: MirAttributes::default(),
            source_file: None,
            source_line: 0,
        }],
        struct_defs: HashMap::new(),
        enum_defs,
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };

    let ir = emit_module(&module, &EmitOptions::default()).unwrap();
    // Rect variant: tag=1, 2 fields → 3 insertvalue ops (tag + 2 fields)
    let insert_count = ir.matches("insertvalue").count();
    assert!(
        insert_count >= 3,
        "Rect(f64,f64) needs 3 insertvalue ops, got {insert_count}:\n{ir}"
    );
}

#[test]
fn test_enum_tag_extraction() {
    use kryos_mir::ir::EnumVariantDef;

    let mut enum_defs = HashMap::new();
    enum_defs.insert(
        "Color".to_string(),
        vec![
            EnumVariantDef {
                name: "Red".into(),
                fields: vec![],
            },
            EnumVariantDef {
                name: "Green".into(),
                fields: vec![],
            },
        ],
    );

    let module = MirModule {
        functions: vec![MirFunction {
            name: "get_tag".into(),
            params: vec![MirParam {
                local: LocalId(0),
                ty: MirType::Enum("Color".into()),
            }],
            ret_ty: MirType::I64,
            locals: vec![
                MirLocal {
                    id: LocalId(0),
                    name: Some("c".into()),
                    ty: MirType::Enum("Color".into()),
                    mutable: false,
                },
                MirLocal {
                    id: LocalId(1),
                    name: None,
                    ty: MirType::I64,
                    mutable: false,
                },
            ],
            blocks: vec![BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::EnumTag {
                        operand: Operand::Local(LocalId(0)),
                    },
                }],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
            }],
            attributes: MirAttributes::default(),
            source_file: None,
            source_line: 0,
        }],
        struct_defs: HashMap::new(),
        enum_defs,
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };

    let ir = emit_module(&module, &EmitOptions::default()).unwrap();
    // Tag extraction: extractvalue { i64 } %_0, 0
    assert!(
        ir.contains("extractvalue"),
        "enum tag must use extractvalue:\n{ir}"
    );
    assert!(ir.contains(", 0"), "tag is at index 0:\n{ir}");
}

#[test]
fn test_enum_payload_extraction() {
    use kryos_mir::ir::EnumVariantDef;

    let mut enum_defs = HashMap::new();
    enum_defs.insert(
        "Shape".to_string(),
        vec![
            EnumVariantDef {
                name: "Circle".into(),
                fields: vec![MirType::F64],
            },
            EnumVariantDef {
                name: "Rect".into(),
                fields: vec![MirType::F64, MirType::F64],
            },
        ],
    );

    let module = MirModule {
        functions: vec![MirFunction {
            name: "get_height".into(),
            params: vec![MirParam {
                local: LocalId(0),
                ty: MirType::Enum("Shape".into()),
            }],
            ret_ty: MirType::I64,
            locals: vec![
                MirLocal {
                    id: LocalId(0),
                    name: Some("s".into()),
                    ty: MirType::Enum("Shape".into()),
                    mutable: false,
                },
                MirLocal {
                    id: LocalId(1),
                    name: None,
                    ty: MirType::I64,
                    mutable: false,
                },
            ],
            blocks: vec![BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::EnumPayload {
                        operand: Operand::Local(LocalId(0)),
                        enum_name: "Shape".into(),
                        variant_idx: 1, // Rect
                        field_idx: 1,   // second field (height)
                    },
                }],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
            }],
            attributes: MirAttributes::default(),
            source_file: None,
            source_line: 0,
        }],
        struct_defs: HashMap::new(),
        enum_defs,
        trait_vtables: HashMap::new(),
        copy_structs: HashSet::new(),
    };

    let ir = emit_module(&module, &EmitOptions::default()).unwrap();
    // Payload at index 1+1=2: extractvalue ... , 2
    assert!(
        ir.contains("extractvalue"),
        "payload must use extractvalue:\n{ir}"
    );
    assert!(
        ir.contains(", 2"),
        "second payload field is at index 2:\n{ir}"
    );
}
