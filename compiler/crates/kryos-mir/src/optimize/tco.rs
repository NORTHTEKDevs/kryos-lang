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
    BasicBlock, BlockId, Instruction, LocalId, MirFunction, MirLocal, MirModule, MirType, Operand,
    RValue, Terminator,
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

    // Detect whether any block is actually a tail-call to self. If so, we'll
    // reassign param locals in the loop back-edge, so they must be marked
    // mutable so the backend codegen emits them as alloca + store/load
    // rather than SSA temporaries (which would conflict with the function's
    // entry-block parameter SSA names like %_0).
    let has_tail_call = func.blocks.iter().any(|b| is_tail_self_call(b, &self_name));
    if !has_tail_call {
        return;
    }
    for pl in &param_locals {
        if let Some(local) = func.locals.iter_mut().find(|l| l.id == *pl) {
            local.mutable = true;
        }
    }

    // Next free local id for fresh snapshot temporaries (see below).
    let mut next_id = func
        .locals
        .iter()
        .map(|l| l.id.0)
        .max()
        .map(|m| m + 1)
        .unwrap_or(0);

    for bi in 0..func.blocks.len() {
        if !is_tail_self_call(&func.blocks[bi], &self_name) {
            continue;
        }
        let last_idx = func.blocks[bi].instructions.len() - 1;
        let call_args = match &func.blocks[bi].instructions[last_idx] {
            Instruction::Assign {
                value: RValue::Call { args, .. },
                ..
            } => args.clone(),
            _ => continue,
        };
        // Arity must match the parameter list for a 1:1 param reassignment.
        if call_args.len() != param_locals.len() {
            continue;
        }

        // Snapshot every argument into a fresh temporary BEFORE writing any
        // parameter. Without this, reassigning params in sequence corrupts any
        // argument that references a parameter reassigned earlier in the list:
        // e.g. tail-calling `go(b, a, ..)` (a swap) would compute `a = b` then
        // `b = a` and read the already-overwritten `a`, yielding `b == a`. The
        // temporaries capture all reads first, so the param writes see the
        // original values (correct parallel assignment).
        let mut temps: Vec<LocalId> = Vec::with_capacity(param_locals.len());
        for pl in &param_locals {
            let ty = func
                .locals
                .iter()
                .find(|l| l.id == *pl)
                .map(|l| l.ty.clone())
                .unwrap_or(MirType::I64);
            let tid = LocalId(next_id);
            next_id += 1;
            func.locals.push(MirLocal {
                id: tid,
                name: None,
                ty,
                mutable: false,
            });
            temps.push(tid);
        }

        let block = &mut func.blocks[bi];
        // Remove the tail-call instruction.
        block.instructions.remove(last_idx);
        // 1. Capture each argument value into its snapshot temp (all reads).
        for (temp, arg) in temps.iter().zip(call_args.iter()) {
            block.instructions.push(Instruction::Assign {
                dest: *temp,
                value: RValue::Use(arg.clone()),
            });
        }
        // 2. Copy snapshots into the parameter locals (all writes).
        for (param, temp) in param_locals.iter().zip(temps.iter()) {
            block.instructions.push(Instruction::Assign {
                dest: *param,
                value: RValue::Use(Operand::Local(*temp)),
            });
        }
        // 3. Loop back to the entry block.
        block.terminator = Terminator::Goto(BlockId(0));
    }
}

/// Return true if this block ends with a tail-recursive self-call.
fn is_tail_self_call(block: &BasicBlock, self_name: &str) -> bool {
    let ret_local = match &block.terminator {
        Terminator::Return(Some(Operand::Local(id))) => *id,
        _ => return false,
    };
    let last_idx = match block.instructions.len().checked_sub(1) {
        Some(i) => i,
        None => return false,
    };
    matches!(
        &block.instructions[last_idx],
        Instruction::Assign {
            dest,
            value: RValue::Call { func, .. },
        } if func == self_name && *dest == ret_local
    )
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
                source_file: None,
                source_line: 0,
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

        // The arg is snapshotted into a fresh temp first, then copied to the
        // param: `<temp> = use(_1); _0 = use(<temp>)`.
        let snapshot_temp = tail_block.instructions.iter().find_map(|i| match i {
            Instruction::Assign {
                dest,
                value: RValue::Use(Operand::Local(src)),
            } if *src == LocalId(1) => Some(*dest),
            _ => None,
        });
        assert!(snapshot_temp.is_some(), "should snapshot arg into a temp");
        let temp = snapshot_temp.unwrap();
        let has_param_assign = tail_block.instructions.iter().any(|i| {
            matches!(
                i,
                Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::Use(Operand::Local(s)),
                } if *s == temp
            )
        });
        assert!(has_param_assign, "should assign snapshot temp to param local");
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
                source_file: None,
                source_line: 0,
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
                source_file: None,
                source_line: 0,
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
                source_file: None,
                source_line: 0,
            }],
            struct_defs: Default::default(),
            enum_defs: Default::default(),
            trait_vtables: Default::default(),
            copy_structs: Default::default(),
        };

        optimize_tail_calls(&mut module);
    }
}
