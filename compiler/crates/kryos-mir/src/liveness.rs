//! Backward liveness analysis on the MIR control-flow graph.
//!
//! For each basic block we compute the set of locals that are *live* at the
//! start (`live_in`) and at the end (`live_out`). A local is live if its
//! current value will be read along some path before it is overwritten or
//! the function returns.
//!
//! The analysis is intentionally simple — it works on the CFG already
//! produced by [`crate::lower`] and does not require SSA. It is used by
//! [`crate::async_lower`] to decide which locals need to be persisted in
//! the async state struct across an await suspension point.
//!
//! # Algorithm
//!
//! Standard iterative dataflow:
//!
//! ```text
//!   live_out[B] = union of live_in[S] for every successor S of B
//!   live_in[B]  = (live_out[B] \ defs(B)) ∪ uses(B)
//! ```
//!
//! Iteration continues until a fixpoint. `defs` and `uses` are computed
//! per-instruction in reverse order so that within a block, a use that
//! occurs *before* a def in source order remains in `live_in`, while a use
//! that occurs *after* a def is masked. The terminator is treated as the
//! last "instruction" of the block.
//!
//! # Per-program-point queries
//!
//! [`Liveness::live_after_instruction`] returns the set of locals that are
//! live immediately after a given instruction inside a block. This is what
//! the split-at-await transform actually needs: at each suspension point,
//! locals live *after* the await must be persisted to the state struct.

use crate::ir::{
    BasicBlock, BlockId, Instruction, LocalId, MirFunction, Operand, RValue, Terminator,
};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

/// Result of running liveness analysis on a function.
#[derive(Debug, Clone, Default)]
pub struct Liveness {
    /// `live_in[b]` — locals live at the entry of block `b`.
    pub live_in: HashMap<BlockId, BTreeSet<LocalId>>,
    /// `live_out[b]` — locals live at the exit of block `b` (immediately
    /// before its terminator transfers control).
    pub live_out: HashMap<BlockId, BTreeSet<LocalId>>,
}

impl Liveness {
    /// Returns the set of locals live immediately **after** the instruction
    /// at `inst_idx` within block `block_id`. If `inst_idx == block.instructions.len()`
    /// this returns `live_out` for that block (i.e. live right before the terminator).
    ///
    /// Used by the split-at-await transform: pass the index of the await
    /// instruction and the result is exactly the set of locals that must
    /// survive the suspension.
    pub fn live_after_instruction(
        &self,
        func: &MirFunction,
        block_id: BlockId,
        inst_idx: usize,
    ) -> BTreeSet<LocalId> {
        let block = func.block(block_id);
        let mut live: BTreeSet<LocalId> = self.live_out.get(&block_id).cloned().unwrap_or_default();

        // Walk instructions backwards from the end of the block down to
        // (but not including) `inst_idx`. After this loop, `live` is the
        // set of locals live immediately after instruction `inst_idx`.
        for i in (inst_idx + 1..block.instructions.len()).rev() {
            transfer_instruction(&block.instructions[i], &mut live);
        }
        live
    }
}

/// Run liveness analysis on a function.
pub fn analyze(func: &MirFunction) -> Liveness {
    let n = func.blocks.len();
    let mut live_in: HashMap<BlockId, BTreeSet<LocalId>> = HashMap::with_capacity(n);
    let mut live_out: HashMap<BlockId, BTreeSet<LocalId>> = HashMap::with_capacity(n);
    for b in &func.blocks {
        live_in.insert(b.id, BTreeSet::new());
        live_out.insert(b.id, BTreeSet::new());
    }

    // Build predecessor map to drive a worklist that propagates changes
    // backwards.
    let mut preds: HashMap<BlockId, Vec<BlockId>> = HashMap::with_capacity(n);
    for b in &func.blocks {
        preds.entry(b.id).or_default();
    }
    for b in &func.blocks {
        for s in b.successors() {
            preds.entry(s).or_default().push(b.id);
        }
    }

    let mut worklist: VecDeque<BlockId> = func.blocks.iter().map(|b| b.id).collect();
    let mut on_queue: HashSet<BlockId> = worklist.iter().copied().collect();

    while let Some(bid) = worklist.pop_front() {
        on_queue.remove(&bid);
        let block = func.block(bid);

        // live_out[B] = union of live_in[S] for each successor S.
        let mut new_out: BTreeSet<LocalId> = BTreeSet::new();
        for s in block.successors() {
            if let Some(s_in) = live_in.get(&s) {
                new_out.extend(s_in.iter().copied());
            }
        }
        // The terminator itself reads operands at the very end of the
        // block — those are part of live_out *as seen from inside the
        // block*, not after, but we model it by folding terminator
        // uses into `new_out` before running the block backwards.
        // (Equivalent to: live just before terminator = live_after_term ∪ uses(term).)
        terminator_uses(&block.terminator, &mut new_out);

        // live_in[B] = transfer(new_out) walking instructions backwards.
        let mut new_in = new_out.clone();
        for inst in block.instructions.iter().rev() {
            transfer_instruction(inst, &mut new_in);
        }

        let changed_out = live_out.get(&bid) != Some(&new_out);
        let changed_in = live_in.get(&bid) != Some(&new_in);
        live_out.insert(bid, new_out);
        live_in.insert(bid, new_in);

        if changed_in || changed_out {
            // Re-queue predecessors.
            if let Some(ps) = preds.get(&bid) {
                for &p in ps {
                    if !on_queue.contains(&p) {
                        worklist.push_back(p);
                        on_queue.insert(p);
                    }
                }
            }
        }
    }

    Liveness { live_in, live_out }
}

// ---------------------------------------------------------------------------
// Transfer functions
// ---------------------------------------------------------------------------

/// Apply the backward transfer for a single instruction: remove the def(s)
/// from `live`, then add the use(s).
fn transfer_instruction(inst: &Instruction, live: &mut BTreeSet<LocalId>) {
    // Kill (defs) first, then gen (uses). Within an instruction the
    // canonical convention is that uses are evaluated before the def
    // takes effect, so a self-referential `_x = f(_x)` keeps `_x` live.
    let (defs, uses) = inst_defs_uses(inst);
    for d in &defs {
        live.remove(d);
    }
    for u in &uses {
        live.insert(*u);
    }
}

/// Compute (defs, uses) for an instruction.
fn inst_defs_uses(inst: &Instruction) -> (Vec<LocalId>, Vec<LocalId>) {
    let mut defs: Vec<LocalId> = Vec::new();
    let mut uses: Vec<LocalId> = Vec::new();
    match inst {
        Instruction::Assign { dest, value } => {
            defs.push(*dest);
            rvalue_uses(value, &mut uses);
        }
        Instruction::ArcRetain { ptr } | Instruction::ArcRelease { ptr } => {
            uses.push(*ptr);
        }
        Instruction::Drop { local } => {
            uses.push(*local);
        }
        Instruction::StoreField { object, value, .. } => {
            operand_uses(object, &mut uses);
            operand_uses(value, &mut uses);
        }
        Instruction::StoreDeref { ptr, value } => {
            operand_uses(ptr, &mut uses);
            operand_uses(value, &mut uses);
        }
        Instruction::Nop | Instruction::DebugLine(_) => {}
        Instruction::Spawn { args, .. } => {
            for a in args {
                operand_uses(a, &mut uses);
            }
        }
        Instruction::Send { channel, value } => {
            uses.push(*channel);
            uses.push(*value);
        }
        Instruction::Receive { dest, channel } => {
            defs.push(*dest);
            uses.push(*channel);
        }
        Instruction::ActorSpawn { dest, state, .. } => {
            defs.push(*dest);
            operand_uses(state, &mut uses);
        }
        Instruction::ActorSend { actor, args, .. } => {
            uses.push(*actor);
            for a in args {
                operand_uses(a, &mut uses);
            }
        }
        Instruction::ActorStateLoad {
            dest, state_ptr, ..
        } => {
            defs.push(*dest);
            uses.push(*state_ptr);
        }
        Instruction::ActorStateStore {
            state_ptr, value, ..
        } => {
            uses.push(*state_ptr);
            operand_uses(value, &mut uses);
        }
    }
    (defs, uses)
}

/// Append the local uses from an operand to `out`.
fn operand_uses(op: &Operand, out: &mut Vec<LocalId>) {
    if let Operand::Local(id) = op {
        out.push(*id);
    }
}

/// Append the local uses from an r-value to `out`.
fn rvalue_uses(rv: &RValue, out: &mut Vec<LocalId>) {
    match rv {
        RValue::Use(o) => operand_uses(o, out),
        RValue::BinOp { left, right, .. } => {
            operand_uses(left, out);
            operand_uses(right, out);
        }
        RValue::UnOp { operand, .. } => operand_uses(operand, out),
        RValue::Call { args, .. } => {
            for a in args {
                operand_uses(a, out);
            }
        }
        RValue::CallIndirect { callee, args } => {
            operand_uses(callee, out);
            for a in args {
                operand_uses(a, out);
            }
        }
        RValue::ConstInt(_)
        | RValue::ConstFloat(_)
        | RValue::ConstBool(_)
        | RValue::ConstString(_)
        | RValue::ConstNone => {}
        RValue::Array(items) | RValue::Tuple(items) => {
            for o in items {
                operand_uses(o, out);
            }
        }
        RValue::Struct { fields, .. } => {
            for (_, o) in fields {
                operand_uses(o, out);
            }
        }
        RValue::Field { object, .. } => operand_uses(object, out),
        RValue::Index { object, index } => {
            operand_uses(object, out);
            operand_uses(index, out);
        }
        RValue::ArcAlloc { inner } => operand_uses(inner, out),
        RValue::Cast { operand, .. } => operand_uses(operand, out),
        RValue::EnumVariant { fields, .. } => {
            for o in fields {
                operand_uses(o, out);
            }
        }
        RValue::EnumTag { operand } => operand_uses(operand, out),
        RValue::EnumPayload { operand, .. } => operand_uses(operand, out),
        RValue::Closure { captures, .. } => {
            for o in captures {
                operand_uses(o, out);
            }
        }
        RValue::Map(pairs) => {
            for (k, v) in pairs {
                operand_uses(k, out);
                operand_uses(v, out);
            }
        }
        RValue::StringConcat(items) => {
            for o in items {
                operand_uses(o, out);
            }
        }
        RValue::Range { start, end, .. } => {
            if let Some(s) = start {
                operand_uses(s, out);
            }
            if let Some(e) = end {
                operand_uses(e, out);
            }
        }
        RValue::AddrOf { local, .. } => out.push(*local),
        RValue::Deref { operand } => operand_uses(operand, out),
        RValue::Comptime(inner) => rvalue_uses(inner, out),
        RValue::MakeTraitObject { value, .. } => operand_uses(value, out),
        RValue::VtableCall { object, args, .. } => {
            operand_uses(object, out);
            for a in args {
                operand_uses(a, out);
            }
        }
    }
}

/// Fold the local uses from a terminator into `out`.
fn terminator_uses(term: &Terminator, out: &mut BTreeSet<LocalId>) {
    match term {
        Terminator::Return(Some(op)) => {
            if let Operand::Local(id) = op {
                out.insert(*id);
            }
        }
        Terminator::Return(None) => {}
        Terminator::Goto(_) => {}
        Terminator::Branch { cond, .. } => {
            if let Operand::Local(id) = cond {
                out.insert(*id);
            }
        }
        Terminator::Switch { value, .. } => {
            if let Operand::Local(id) = value {
                out.insert(*id);
            }
        }
        Terminator::Unreachable => {}
    }
}

/// Convenience for tests / external callers: collect the set of locals
/// live immediately after a given instruction index in a block. Returns
/// a sorted vector.
pub fn live_after(
    func: &MirFunction,
    live: &Liveness,
    block_id: BlockId,
    inst_idx: usize,
) -> Vec<LocalId> {
    let s = live.live_after_instruction(func, block_id, inst_idx);
    s.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Borrow shim for BasicBlock used by `MirFunction::block` — declared so
// the file is self-contained even if the IR module changes.
// ---------------------------------------------------------------------------

impl BasicBlock {
    /// Total number of "program points" inside a block (instructions + terminator).
    pub fn program_point_count(&self) -> usize {
        self.instructions.len() + 1
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        BasicBlock, BlockId, Constant, Instruction, LocalId, MirAttributes, MirFunction, MirLocal,
        MirParam, MirType, Operand, RValue, Terminator,
    };

    fn local(id: u32, name: &str, ty: MirType) -> MirLocal {
        MirLocal {
            id: LocalId(id),
            name: Some(name.into()),
            ty,
            mutable: false,
        }
    }

    /// Build a tiny function:
    ///   bb0:
    ///     _1 = _0          // x = param
    ///     _2 = f(_1)       // y = f(x)        <-- "await" candidate
    ///     _3 = _1 + _2     // z = x + y
    ///     return _3
    /// Expected: after the call at idx 1, both _1 and _2 are live (needed by _3).
    fn make_func() -> MirFunction {
        let mut attrs = MirAttributes::default();
        attrs.is_async = true;
        MirFunction {
            name: "demo".into(),
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
                        value: RValue::Use(Operand::Local(LocalId(0))),
                    },
                    Instruction::Assign {
                        dest: LocalId(2),
                        value: RValue::Call {
                            func: "f".into(),
                            args: vec![Operand::Local(LocalId(1))],
                        },
                    },
                    Instruction::Assign {
                        dest: LocalId(3),
                        value: RValue::BinOp {
                            op: crate::ir::MirBinOp::Add,
                            left: Operand::Local(LocalId(1)),
                            right: Operand::Local(LocalId(2)),
                        },
                    },
                ],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(3)))),
            }],
            locals: vec![
                local(0, "p", MirType::I64),
                local(1, "x", MirType::I64),
                local(2, "y", MirType::I64),
                local(3, "z", MirType::I64),
            ],
            attributes: attrs,
            source_file: None,
            source_line: 0,
        }
    }

    #[test]
    fn live_after_call_includes_both_operands_of_later_add() {
        let f = make_func();
        let live = analyze(&f);
        let s = live.live_after_instruction(&f, BlockId(0), 1);
        // _1 is needed by inst[2]; _2 is the result we just produced and is also needed by inst[2].
        assert!(s.contains(&LocalId(1)), "_1 must be live after the await");
        assert!(s.contains(&LocalId(2)), "_2 must be live after the await");
        // _3 has not been defined yet at this point.
        assert!(!s.contains(&LocalId(3)));
        // _0 is dead — the only use was at inst[0] which already ran.
        assert!(!s.contains(&LocalId(0)));
    }

    #[test]
    fn live_in_at_entry_contains_only_actually_used_locals() {
        let f = make_func();
        let live = analyze(&f);
        let entry = live.live_in.get(&BlockId(0)).unwrap();
        // _0 (the parameter) is used at inst[0]; expected live at entry.
        assert!(entry.contains(&LocalId(0)));
        // _1, _2, _3 are all defined before any use in this single block,
        // so they are not live at entry.
        assert!(!entry.contains(&LocalId(1)));
        assert!(!entry.contains(&LocalId(2)));
        assert!(!entry.contains(&LocalId(3)));
    }

    #[test]
    fn branch_propagates_through_both_successors() {
        // bb0:
        //   _1 = 1
        //   _2 = 2
        //   branch cond=_1 -> bb1 else bb2
        // bb1:
        //   _3 = _2
        //   return _3
        // bb2:
        //   return _2
        let mut attrs = MirAttributes::default();
        attrs.is_async = false;
        let f = MirFunction {
            name: "b".into(),
            params: vec![],
            ret_ty: MirType::I64,
            blocks: vec![
                BasicBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Assign {
                            dest: LocalId(1),
                            value: RValue::ConstInt(1),
                        },
                        Instruction::Assign {
                            dest: LocalId(2),
                            value: RValue::ConstInt(2),
                        },
                    ],
                    terminator: Terminator::Branch {
                        cond: Operand::Local(LocalId(1)),
                        then_block: BlockId(1),
                        else_block: BlockId(2),
                    },
                },
                BasicBlock {
                    id: BlockId(1),
                    instructions: vec![Instruction::Assign {
                        dest: LocalId(3),
                        value: RValue::Use(Operand::Local(LocalId(2))),
                    }],
                    terminator: Terminator::Return(Some(Operand::Local(LocalId(3)))),
                },
                BasicBlock {
                    id: BlockId(2),
                    instructions: vec![],
                    terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
                },
            ],
            locals: vec![
                local(1, "c", MirType::Bool),
                local(2, "v", MirType::I64),
                local(3, "r", MirType::I64),
            ],
            attributes: attrs,
            source_file: None,
            source_line: 0,
        };
        let live = analyze(&f);
        // After inst[0] (_1 = 1) and before inst[1] (_2 = 2):
        //   _1 is live (branch cond reads it),
        //   _2 is NOT yet defined, so it must NOT be live.
        let s0 = live.live_after_instruction(&f, BlockId(0), 0);
        assert!(s0.contains(&LocalId(1)));
        assert!(!s0.contains(&LocalId(2)));
        // After inst[1] (_2 = 2), both _1 and _2 are live: _1 by the branch
        // terminator, _2 by the successor blocks.
        let s1 = live.live_after_instruction(&f, BlockId(0), 1);
        assert!(s1.contains(&LocalId(1)));
        assert!(s1.contains(&LocalId(2)));
        // At entry of bb1: _2 must be live (used at inst[0]).
        assert!(live.live_in.get(&BlockId(1)).unwrap().contains(&LocalId(2)));
        // At entry of bb2: _2 live (returned).
        assert!(live.live_in.get(&BlockId(2)).unwrap().contains(&LocalId(2)));
    }

    #[test]
    fn loop_with_back_edge_reaches_fixpoint() {
        // bb0: _1 = 0; goto bb1
        // bb1: _2 = _1 + 1; branch _2 -> bb1 else bb2     (back-edge to itself)
        // bb2: return _2
        let mut attrs = MirAttributes::default();
        attrs.is_async = false;
        let f = MirFunction {
            name: "loop".into(),
            params: vec![],
            ret_ty: MirType::I64,
            blocks: vec![
                BasicBlock {
                    id: BlockId(0),
                    instructions: vec![Instruction::Assign {
                        dest: LocalId(1),
                        value: RValue::ConstInt(0),
                    }],
                    terminator: Terminator::Goto(BlockId(1)),
                },
                BasicBlock {
                    id: BlockId(1),
                    instructions: vec![Instruction::Assign {
                        dest: LocalId(2),
                        value: RValue::BinOp {
                            op: crate::ir::MirBinOp::Add,
                            left: Operand::Local(LocalId(1)),
                            right: Operand::Constant(Constant::Int(1)),
                        },
                    }],
                    terminator: Terminator::Branch {
                        cond: Operand::Local(LocalId(2)),
                        then_block: BlockId(1),
                        else_block: BlockId(2),
                    },
                },
                BasicBlock {
                    id: BlockId(2),
                    instructions: vec![],
                    terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
                },
            ],
            locals: vec![local(1, "i", MirType::I64), local(2, "j", MirType::I64)],
            attributes: attrs,
            source_file: None,
            source_line: 0,
        };
        let live = analyze(&f);
        // _1 lives across the back-edge into bb1 (used at inst[0] of bb1).
        assert!(live.live_in.get(&BlockId(1)).unwrap().contains(&LocalId(1)));
        // _2 lives at the end of bb1 (branch cond and returned by bb2).
        assert!(live.live_out.get(&BlockId(1)).unwrap().contains(&LocalId(2)));
    }
}
