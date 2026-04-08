//! Dead code elimination pass.
//!
//! Two sub-passes:
//!
//! 1. **Unreachable block removal** — BFS from the entry block (block 0);
//!    any block not reached is dead and gets removed.
//!
//! 2. **Dead assignment removal** — collect all `LocalId`s that are actually
//!    read (appear as operands anywhere). Any `Assign { dest }` where `dest`
//!    is never read AND the RValue has no side effects can be removed.

use std::collections::{HashSet, VecDeque};

use crate::ir::{
    BasicBlock, BlockId, Instruction, MirFunction, MirModule, Operand, RValue,
    Terminator,
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run dead code elimination on every function in the module.
pub fn eliminate_dead_code(module: &mut MirModule) {
    for func in &mut module.functions {
        remove_unreachable_blocks(func);
        remove_dead_assignments(func);
    }
}

// ---------------------------------------------------------------------------
// Pass 1: Unreachable block removal
// ---------------------------------------------------------------------------

/// Remove basic blocks that are not reachable from the entry block (block 0).
fn remove_unreachable_blocks(func: &mut MirFunction) {
    if func.blocks.is_empty() {
        return;
    }

    // BFS from block 0.
    let mut reachable: HashSet<u32> = HashSet::new();
    let mut queue: VecDeque<u32> = VecDeque::new();
    queue.push_back(0);
    reachable.insert(0);

    while let Some(idx) = queue.pop_front() {
        if let Some(block) = func.blocks.get(idx as usize) {
            for succ in block.successors() {
                if reachable.insert(succ.0) {
                    queue.push_back(succ.0);
                }
            }
        }
    }

    // If every block is reachable, nothing to do.
    if reachable.len() == func.blocks.len() {
        return;
    }

    // Retain only reachable blocks and renumber them.
    // Build old-to-new index mapping.
    let old_blocks: Vec<BasicBlock> = std::mem::take(&mut func.blocks);
    let mut old_to_new: std::collections::HashMap<u32, u32> = Default::default();
    let mut new_idx = 0u32;

    for block in &old_blocks {
        if reachable.contains(&block.id.0) {
            old_to_new.insert(block.id.0, new_idx);
            new_idx += 1;
        }
    }

    // Rebuild blocks with renumbered IDs and terminators.
    for block in old_blocks {
        if !reachable.contains(&block.id.0) {
            continue;
        }
        let new_id = BlockId(old_to_new[&block.id.0]);
        let new_terminator = remap_terminator(&block.terminator, &old_to_new);
        func.blocks.push(BasicBlock {
            id: new_id,
            instructions: block.instructions,
            terminator: new_terminator,
        });
    }
}

/// Remap BlockIds in a terminator according to the old-to-new mapping.
fn remap_terminator(
    term: &Terminator,
    map: &std::collections::HashMap<u32, u32>,
) -> Terminator {
    match term {
        Terminator::Return(op) => Terminator::Return(op.clone()),
        Terminator::Unreachable => Terminator::Unreachable,
        Terminator::Goto(b) => {
            Terminator::Goto(BlockId(*map.get(&b.0).unwrap_or(&b.0)))
        }
        Terminator::Branch {
            cond,
            then_block,
            else_block,
        } => Terminator::Branch {
            cond: cond.clone(),
            then_block: BlockId(*map.get(&then_block.0).unwrap_or(&then_block.0)),
            else_block: BlockId(*map.get(&else_block.0).unwrap_or(&else_block.0)),
        },
        Terminator::Switch {
            value,
            targets,
            default,
        } => Terminator::Switch {
            value: value.clone(),
            targets: targets
                .iter()
                .map(|(v, b)| (*v, BlockId(*map.get(&b.0).unwrap_or(&b.0))))
                .collect(),
            default: BlockId(*map.get(&default.0).unwrap_or(&default.0)),
        },
    }
}

// ---------------------------------------------------------------------------
// Pass 2: Dead assignment removal
// ---------------------------------------------------------------------------

/// Remove assignments to locals that are never read, provided the RValue
/// has no side effects.
fn remove_dead_assignments(func: &mut MirFunction) {
    // Collect all locals that are read anywhere in the function.
    let used = collect_used_locals(func);

    for block in &mut func.blocks {
        block.instructions.retain(|inst| {
            if let Instruction::Assign { dest, value } = inst {
                if !used.contains(&dest.0) && !has_side_effects(value) {
                    return false; // dead — remove
                }
            }
            true
        });
    }
}

/// Collect all `LocalId`s that appear as operands (reads) anywhere in the
/// function — instructions, terminators, and nested within RValues.
fn collect_used_locals(func: &MirFunction) -> HashSet<u32> {
    let mut used = HashSet::new();

    for block in &func.blocks {
        for inst in &block.instructions {
            collect_locals_in_instruction(inst, &mut used);
        }
        collect_locals_in_terminator(&block.terminator, &mut used);
    }

    used
}

fn collect_locals_in_instruction(inst: &Instruction, used: &mut HashSet<u32>) {
    match inst {
        Instruction::Assign { value, .. } => collect_locals_in_rvalue(value, used),
        Instruction::ArcRetain { ptr } | Instruction::ArcRelease { ptr } => {
            used.insert(ptr.0);
        }
        Instruction::Drop { local } => {
            used.insert(local.0);
        }
        Instruction::Spawn { args, .. } => {
            for arg in args {
                collect_locals_in_operand(arg, used);
            }
        }
        Instruction::Send { channel, value } => {
            used.insert(channel.0);
            used.insert(value.0);
        }
        Instruction::Receive { channel, .. } => {
            used.insert(channel.0);
        }
        Instruction::ActorSpawn { state, .. } => {
            collect_locals_in_operand(state, used);
        }
        Instruction::ActorSend { actor, args, .. } => {
            used.insert(actor.0);
            for arg in args {
                collect_locals_in_operand(arg, used);
            }
        }
        Instruction::ActorStateLoad { state_ptr, .. } => {
            used.insert(state_ptr.0);
        }
        Instruction::ActorStateStore {
            state_ptr, value, ..
        } => {
            used.insert(state_ptr.0);
            collect_locals_in_operand(value, used);
        }
        Instruction::StoreField { object, value, .. } => {
            collect_locals_in_operand(object, used);
            collect_locals_in_operand(value, used);
        }
        Instruction::StoreDeref { ptr, value } => {
            collect_locals_in_operand(ptr, used);
            collect_locals_in_operand(value, used);
        }
        Instruction::Nop => {}
    }
}

fn collect_locals_in_rvalue(rv: &RValue, used: &mut HashSet<u32>) {
    match rv {
        RValue::Use(op) => collect_locals_in_operand(op, used),
        RValue::BinOp { left, right, .. } => {
            collect_locals_in_operand(left, used);
            collect_locals_in_operand(right, used);
        }
        RValue::UnOp { operand, .. } => collect_locals_in_operand(operand, used),
        RValue::Call { args, .. } => {
            for arg in args {
                collect_locals_in_operand(arg, used);
            }
        }
        RValue::CallIndirect { callee, args } => {
            collect_locals_in_operand(callee, used);
            for arg in args {
                collect_locals_in_operand(arg, used);
            }
        }
        RValue::Array(elems) | RValue::Tuple(elems) => {
            for e in elems {
                collect_locals_in_operand(e, used);
            }
        }
        RValue::Struct { fields, .. } => {
            for (_, op) in fields {
                collect_locals_in_operand(op, used);
            }
        }
        RValue::Field { object, .. } => collect_locals_in_operand(object, used),
        RValue::Index { object, index } => {
            collect_locals_in_operand(object, used);
            collect_locals_in_operand(index, used);
        }
        RValue::ArcAlloc { inner } => collect_locals_in_operand(inner, used),
        RValue::Cast { operand, .. } => collect_locals_in_operand(operand, used),
        RValue::EnumVariant { fields, .. } => {
            for f in fields {
                collect_locals_in_operand(f, used);
            }
        }
        RValue::EnumTag { operand } => collect_locals_in_operand(operand, used),
        RValue::EnumPayload { operand, .. } => collect_locals_in_operand(operand, used),
        RValue::Closure { captures, .. } => {
            for c in captures {
                collect_locals_in_operand(c, used);
            }
        }
        RValue::Map(pairs) => {
            for (k, v) in pairs {
                collect_locals_in_operand(k, used);
                collect_locals_in_operand(v, used);
            }
        }
        RValue::StringConcat(parts) => {
            for p in parts {
                collect_locals_in_operand(p, used);
            }
        }
        RValue::Range { start, end, .. } => {
            if let Some(s) = start {
                collect_locals_in_operand(s, used);
            }
            if let Some(e) = end {
                collect_locals_in_operand(e, used);
            }
        }
        RValue::AddrOf { local, .. } => {
            used.insert(local.0);
        }
        RValue::Deref { operand } => collect_locals_in_operand(operand, used),
        RValue::Comptime(inner) => collect_locals_in_rvalue(inner, used),
        RValue::MakeTraitObject { value, .. } => {
            collect_locals_in_operand(value, used);
        }
        RValue::VtableCall { object, args, .. } => {
            collect_locals_in_operand(object, used);
            for a in args {
                collect_locals_in_operand(a, used);
            }
        }
        // Constants have no local references.
        RValue::ConstInt(_)
        | RValue::ConstFloat(_)
        | RValue::ConstBool(_)
        | RValue::ConstString(_)
        | RValue::ConstNone => {}
    }
}

fn collect_locals_in_operand(op: &Operand, used: &mut HashSet<u32>) {
    if let Operand::Local(id) = op {
        used.insert(id.0);
    }
}

fn collect_locals_in_terminator(term: &Terminator, used: &mut HashSet<u32>) {
    match term {
        Terminator::Return(Some(op)) => collect_locals_in_operand(op, used),
        Terminator::Branch { cond, .. } => collect_locals_in_operand(cond, used),
        Terminator::Switch { value, .. } => collect_locals_in_operand(value, used),
        Terminator::Return(None) | Terminator::Goto(_) | Terminator::Unreachable => {}
    }
}

/// Returns `true` if the RValue may have side effects and should not be
/// removed even if its result is unused.
fn has_side_effects(rv: &RValue) -> bool {
    matches!(
        rv,
        RValue::Call { .. }
            | RValue::CallIndirect { .. }
            | RValue::VtableCall { .. }
            | RValue::ArcAlloc { .. }
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
    fn remove_unreachable_block() {
        // bb0 -> Return, bb1 is dead (no predecessor).
        let mut func = MirFunction {
            name: "test".into(),
            params: vec![],
            ret_ty: MirType::Void,
            blocks: vec![
                BasicBlock {
                    id: BlockId(0),
                    instructions: vec![],
                    terminator: Terminator::Return(None),
                },
                BasicBlock {
                    id: BlockId(1),
                    instructions: vec![Instruction::Nop],
                    terminator: Terminator::Unreachable,
                },
            ],
            locals: vec![],
            attributes: MirAttributes::default(),
        };

        remove_unreachable_blocks(&mut func);

        assert_eq!(func.blocks.len(), 1);
        assert_eq!(func.blocks[0].id, BlockId(0));
    }

    #[test]
    fn keep_reachable_blocks() {
        // bb0 -> Goto(bb1) -> Return
        let mut func = MirFunction {
            name: "test".into(),
            params: vec![],
            ret_ty: MirType::Void,
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
            locals: vec![],
            attributes: MirAttributes::default(),
        };

        remove_unreachable_blocks(&mut func);

        assert_eq!(func.blocks.len(), 2);
    }

    #[test]
    fn remove_dead_assignment_unused_local() {
        // _0 = 42 (never read) — should be removed.
        let mut func = MirFunction {
            name: "test".into(),
            params: vec![],
            ret_ty: MirType::Void,
            blocks: vec![BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::ConstInt(42),
                }],
                terminator: Terminator::Return(None),
            }],
            locals: vec![make_local(0)],
            attributes: MirAttributes::default(),
        };

        remove_dead_assignments(&mut func);

        assert!(func.blocks[0].instructions.is_empty());
    }

    #[test]
    fn keep_used_assignment() {
        // _0 = 42, return _0 — _0 is used, keep it.
        let mut func = MirFunction {
            name: "test".into(),
            params: vec![],
            ret_ty: MirType::I64,
            blocks: vec![BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::ConstInt(42),
                }],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
            }],
            locals: vec![make_local(0)],
            attributes: MirAttributes::default(),
        };

        remove_dead_assignments(&mut func);

        assert_eq!(func.blocks[0].instructions.len(), 1);
    }

    #[test]
    fn keep_side_effectful_dead_assignment() {
        // _0 = call foo() — even though _0 is never read, the call has side effects.
        let mut func = MirFunction {
            name: "test".into(),
            params: vec![],
            ret_ty: MirType::Void,
            blocks: vec![BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(0),
                    value: RValue::Call {
                        func: "foo".into(),
                        args: vec![],
                    },
                }],
                terminator: Terminator::Return(None),
            }],
            locals: vec![make_local(0)],
            attributes: MirAttributes::default(),
        };

        remove_dead_assignments(&mut func);

        // Should keep the call even though _0 is dead.
        assert_eq!(func.blocks[0].instructions.len(), 1);
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
        };

        eliminate_dead_code(&mut module);
        assert_eq!(module.functions[0].blocks.len(), 1);
    }

    #[test]
    fn unreachable_with_branch_renumbering() {
        // bb0 -> Branch(cond, bb1, bb3), bb1 -> Return, bb2 (dead), bb3 -> Return
        // After DCE: bb0 -> Branch(cond, bb1, bb2), bb1 -> Return, bb2 -> Return
        let mut func = MirFunction {
            name: "test".into(),
            params: vec![],
            ret_ty: MirType::Void,
            blocks: vec![
                BasicBlock {
                    id: BlockId(0),
                    instructions: vec![],
                    terminator: Terminator::Branch {
                        cond: Operand::Constant(Constant::Bool(true)),
                        then_block: BlockId(1),
                        else_block: BlockId(3),
                    },
                },
                BasicBlock {
                    id: BlockId(1),
                    instructions: vec![],
                    terminator: Terminator::Return(None),
                },
                BasicBlock {
                    id: BlockId(2),
                    instructions: vec![],
                    terminator: Terminator::Unreachable,
                },
                BasicBlock {
                    id: BlockId(3),
                    instructions: vec![],
                    terminator: Terminator::Return(None),
                },
            ],
            locals: vec![],
            attributes: MirAttributes::default(),
        };

        remove_unreachable_blocks(&mut func);

        assert_eq!(func.blocks.len(), 3);
        // bb3 should now be bb2
        match &func.blocks[0].terminator {
            Terminator::Branch {
                then_block,
                else_block,
                ..
            } => {
                assert_eq!(then_block.0, 1);
                assert_eq!(else_block.0, 2); // was 3, renumbered to 2
            }
            other => panic!("expected Branch, got {other:?}"),
        }
    }
}
