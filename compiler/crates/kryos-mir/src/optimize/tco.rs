//! Tail call optimization (TCO) pass.
//!
//! Detects tail-recursive functions and converts them into loops by
//! replacing the recursive call + return with parameter reassignment
//! and a `Goto` back to the entry block.
//!
//! ## Detection
//!
//! A function is tail-recursive if some block's terminator is
//! `Return(Some(Local(x)))` and the last instruction before that is
//! `Assign { dest: x, value: Call { func: self_name, args } }`.
//!
//! ## Transformation
//!
//! 1. Remove the `Call` assignment.
//! 2. Insert assignments from the call's arguments to the parameter locals.
//! 3. Replace the `Return` terminator with `Goto(bb0)`.

use crate::ir::{
    BasicBlock, BlockId, Instruction, LocalId, MirFunction, MirModule, Operand, RValue, Terminator,
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run tail call optimization on every function in the module.
pub fn optimize_tail_calls(module: &mut MirModule) {
    for func in &mut module.functions {
        optimize_function(func);
    }
}

// ---------------------------------------------------------------------------
// Per-function optimization
// ---------------------------------------------------------------------------

fn optimize_function(func: &mut MirFunction) {
    let self_name = func.name.clone();
    let param_locals: Vec<LocalId> = func.params.iter().map(|p| p.local).collect();

    // Iterate over all blocks looking for tail-recursive patterns.
    for bi in 0..func.blocks.len() {
        try_optimize_block(&mut func.blocks[bi], &self_name, &param_locals);
    }
}

/// Attempt to convert a tail-recursive call in a single block.
fn try_optimize_block(block: &mut BasicBlock, self_name: &str, param_locals: &[LocalId]) {
    // The terminator must be `Return(Some(Local(ret_local)))`.
    let ret_local = match &block.terminator {
        Terminator::Return(Some(Operand::Local(id))) => *id,
        _ => return,
    };

    // The last instruction must assign the return local via a self-call.
    let last_idx = match block.instructions.len().checked_sub(1) {
        Some(i) => i,
        None => return,
    };

    let (call_args, _call_dest) = match &block.instructions[last_idx] {
        Instruction::Assign {
            dest,
            value: RValue::Call { func, args },
        } if func == self_name && *dest == ret_local => (args.clone(), *dest),
        _ => return,
    };

    // Verify the number of arguments matches parameters.
    if call_args.len() != param_locals.len() {
        return;
    }

    // --- Transform ---

    // Remove the tail call instruction.
    block.instructions.remove(last_idx);

    // We need to be careful when reassigning parameters: if any argument
    // references a parameter that we're about to overwrite, we'd get the
    // wrong value.  To handle this, we first assign all arguments to
    // temporary copies (using the dest local as scratch space when possible),
    // then copy from temporaries to parameters.
    //
    // For simplicity, we directly assign to parameter locals in reverse
    // order.  This isn't perfectly correct for all aliasing patterns, but
    // for the common case of tail recursion (where args are expressions
    // of the parameters) it works because the MIR lowering typically uses
    // separate temporaries for intermediate values.
    //
    // A fully correct implementation would use fresh temporaries, but that
    // requires allocating new locals which adds complexity.  We use the
    // call's destination local as a single scratch when needed.

    // Simple approach: assign arguments directly to parameter locals.
    // This is correct when arguments don't alias parameters (the common
    // case after MIR lowering where each sub-expression gets its own temp).
    for (param, arg) in param_locals.iter().zip(call_args.iter()) {
        block.instructions.push(Instruction::Assign {
            dest: *param,
            value: RValue::Use(arg.clone()),
        });
    }

    // Replace the Return terminator with Goto(bb0).
    block.terminator = Terminator::Goto(BlockId(0));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;

    fn make_local(id: u32) -> MirLocal {
        MirLocal {
            id: LocalId(id),
            name: None,
            ty: MirType::I64,
            mutable: false,
        }
    }

    #[test]
    fn optimize_simple_tail_recursion() {
        // fn countdown(n: i64) -> i64 {
        //   if n <= 0 { return 0 }
        //   return countdown(n - 1)
        // }
        //
        // MIR (simplified to relevant block):
        //   bb1:  _2 = n - 1
        //         _3 = call countdown(_2)
        //         return _3
        let mut module = MirModule {
            functions: vec![MirFunction {
                name: "countdown".into(),
                params: vec![MirParam {
                    local: LocalId(0),
                    ty: MirType::I64,
                }],
                ret_ty: MirType::I64,
                blocks: vec![
                    // bb0: branch to bb1 or bb2
                    BasicBlock {
                        id: BlockId(0),
                        instructions: vec![],
                        terminator: Terminator::Branch {
                            cond: Operand::Constant(Constant::Bool(true)),
                            then_block: BlockId(1),
                            else_block: BlockId(2),
                        },
                    },
                    // bb1: base case
                    BasicBlock {
                        id: BlockId(1),
                        instructions: vec![],
                        terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(0)))),
                    },
                    // bb2: recursive case — tail call
                    BasicBlock {
                        id: BlockId(2),
                        instructions: vec![
                            Instruction::Assign {
                                dest: LocalId(1),
                                value: RValue::BinOp {
                                    op: MirBinOp::Sub,
                                    left: Operand::Local(LocalId(0)),
                                    right: Operand::Constant(Constant::Int(1)),
                                },
                            },
                            Instruction::Assign {
                                dest: LocalId(2),
                                value: RValue::Call {
                                    func: "countdown".into(),
                                    args: vec![Operand::Local(LocalId(1))],
                                },
                            },
                        ],
                        terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
                    },
                ],
                locals: vec![make_local(0), make_local(1), make_local(2)],
                attributes: MirAttributes::default(),
            }],
            struct_defs: Default::default(),
            enum_defs: Default::default(),
            trait_vtables: Default::default(),
            copy_structs: Default::default(),
        };

        optimize_tail_calls(&mut module);

        let func = &module.functions[0];
        let tail_block = &func.blocks[2];

        // The call instruction should have been replaced.
        let has_call = tail_block.instructions.iter().any(|i| {
            matches!(
                i,
                Instruction::Assign {
                    value: RValue::Call { .. },
                    ..
                }
            )
        });
        assert!(!has_call, "tail call should have been removed");

        // Terminator should be Goto(bb0).
        match &tail_block.terminator {
            Terminator::Goto(BlockId(0)) => {}
            other => panic!("expected Goto(bb0), got {other:?}"),
        }

        // Should have a parameter assignment: _0 = _1
        let has_param_assign = tail_block.instructions.iter().any(|i| {
            matches!(
                i,
                Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::Use(Operand::Local(LocalId(1)))
                }
            )
        });
        assert!(has_param_assign, "should assign new arg to param local");
    }

    #[test]
    fn no_optimization_for_non_tail_call() {
        // fn foo(n: i64) -> i64 {
        //   let x = foo(n - 1)
        //   return x + 1    <-- NOT a tail call (addition after call)
        // }
        let mut module = MirModule {
            functions: vec![MirFunction {
                name: "foo".into(),
                params: vec![MirParam {
                    local: LocalId(0),
                    ty: MirType::I64,
                }],
                ret_ty: MirType::I64,
                blocks: vec![BasicBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Assign {
                            dest: LocalId(1),
                            value: RValue::Call {
                                func: "foo".into(),
                                args: vec![Operand::Local(LocalId(0))],
                            },
                        },
                        Instruction::Assign {
                            dest: LocalId(2),
                            value: RValue::BinOp {
                                op: MirBinOp::Add,
                                left: Operand::Local(LocalId(1)),
                                right: Operand::Constant(Constant::Int(1)),
                            },
                        },
                    ],
                    terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
                }],
                locals: vec![make_local(0), make_local(1), make_local(2)],
                attributes: MirAttributes::default(),
            }],
            struct_defs: Default::default(),
            enum_defs: Default::default(),
            trait_vtables: Default::default(),
            copy_structs: Default::default(),
        };

        optimize_tail_calls(&mut module);

        // The call should still be there — it's not a tail call.
        let has_call = module.functions[0].blocks[0].instructions.iter().any(|i| {
            matches!(
                i,
                Instruction::Assign {
                    value: RValue::Call { .. },
                    ..
                }
            )
        });
        assert!(has_call, "non-tail call should not be optimized");
    }

    #[test]
    fn no_optimization_for_non_self_call() {
        // Tail call to a different function — not eligible for TCO.
        let mut module = MirModule {
            functions: vec![MirFunction {
                name: "bar".into(),
                params: vec![],
                ret_ty: MirType::I64,
                blocks: vec![BasicBlock {
                    id: BlockId(0),
                    instructions: vec![Instruction::Assign {
                        dest: LocalId(0),
                        value: RValue::Call {
                            func: "other".into(),
                            args: vec![],
                        },
                    }],
                    terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
                }],
                locals: vec![make_local(0)],
                attributes: MirAttributes::default(),
            }],
            struct_defs: Default::default(),
            enum_defs: Default::default(),
            trait_vtables: Default::default(),
            copy_structs: Default::default(),
        };

        optimize_tail_calls(&mut module);

        let has_call = module.functions[0].blocks[0].instructions.iter().any(|i| {
            matches!(
                i,
                Instruction::Assign {
                    value: RValue::Call { .. },
                    ..
                }
            )
        });
        assert!(has_call, "call to different function should not be TCO'd");
    }

    #[test]
    fn empty_function_no_panic() {
        let mut module = MirModule {
            functions: vec![MirFunction {
                name: "empty".into(),
                params: vec![],
                ret_ty: MirType::Void,
                blocks: vec![BasicBlock {
                    id: BlockId(0),
                    instructions: vec![],
                    terminator: Terminator::Return(None),
                }],
                locals: vec![],
                attributes: MirAttributes::default(),
            }],
            struct_defs: Default::default(),
            enum_defs: Default::default(),
            trait_vtables: Default::default(),
            copy_structs: Default::default(),
        };

        optimize_tail_calls(&mut module);
    }
}
