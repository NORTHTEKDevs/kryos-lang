//! Reference counting for `dyn Trait` values (fat boxes).
//!
//! A trait object is an ARC box `[data, drop_fn, method_0, ..]` (see
//! `RValue::MakeTraitObject`). Lowering treats a `dyn` local like any other
//! binding: it emits a scope-end `Drop` for every named one and nothing else.
//! This module is the module-level pass that makes those drops sound, by
//! giving every dyn local exactly the references it will release:
//!
//! * a named local, and any producer's result (`MakeTraitObject`, a user call,
//!   a vtable or indirect call), owns ONE reference after its definition; a
//!   definition that merely aliases (a field read, `Receive`, ...) retains;
//! * a copy of a dyn local (`let t = s`), or a use that can outlive the call
//!   (a struct field, a return, a closure capture, a runtime builtin, an async
//!   callee, a spawn / channel / actor message), takes its own reference;
//! * a use as an argument of a user function, a vtable call or a call through
//!   a function value is a BORROW (a dyn parameter is borrowed, and a callee
//!   that keeps it takes its own reference by the rule above);
//! * an unnamed temp that owns a reference and has no named holder is released
//!   after its last use in its block. Its last use MOVES the reference when it
//!   is a copy / owning use, so a fresh temp costs no retain at all.
//!
//! Every default is the leak-safe one: a use this pass does not understand
//! retains, a temp it cannot place a release for is never released. A missed
//! retain would be a use-after-free, so anything that is not provably a borrow
//! retains.

use std::collections::{HashMap, HashSet};

use crate::ir::*;

const RETAIN_FN: &str = "kryos_fn_retain";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Use {
    /// Lent for the duration of a call; the callee keeps nothing.
    Borrow,
    /// `dest = src`: the new holder needs a reference of its own.
    Copy,
    /// Stored somewhere that outlives the instruction.
    Own,
    /// `return src`.
    Ret,
}

/// Where a use sits: an instruction, or the block terminator.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum At {
    Inst(usize),
    Term,
}

fn dyn_local(dyn_ids: &HashSet<u32>, op: &Operand) -> Option<u32> {
    match op {
        Operand::Local(l) if dyn_ids.contains(&l.0) => Some(l.0),
        _ => None,
    }
}

/// Every use of a dyn local by `inst`, one entry per operand position.
fn inst_uses(
    inst: &Instruction,
    dyn_ids: &HashSet<u32>,
    borrows: &dyn Fn(&str) -> bool,
) -> Vec<(u32, Use)> {
    let mut out: Vec<(u32, Use)> = Vec::new();
    let mut push = |op: &Operand, k: Use| {
        if let Some(l) = dyn_local(dyn_ids, op) {
            out.push((l, k));
        }
    };
    match inst {
        Instruction::Assign { value, .. } => match value {
            RValue::Use(op) => push(op, Use::Copy),
            RValue::Call { func, args } => {
                // The compensating retain / guarded release helpers only bump
                // or drop a count on the handle they are given; they store
                // nothing.
                let helper = matches!(func.as_str(), "kryos_fn_retain" | "kryos_fn_release_if_ne");
                let k = if borrows(func) || helper { Use::Borrow } else { Use::Own };
                for a in args {
                    push(a, k);
                }
            }
            RValue::CallIndirect { callee, args } => {
                push(callee, Use::Borrow);
                for a in args {
                    push(a, Use::Borrow);
                }
            }
            RValue::VtableCall { object, args, .. } => {
                push(object, Use::Borrow);
                for a in args {
                    push(a, Use::Borrow);
                }
            }
            RValue::MakeTraitObject { value, .. } => push(value, Use::Own),
            RValue::BinOp { left, right, .. } => {
                push(left, Use::Own);
                push(right, Use::Own);
            }
            RValue::UnOp { operand, .. }
            | RValue::Cast { operand, .. }
            | RValue::EnumTag { operand }
            | RValue::EnumPayload { operand, .. }
            | RValue::Deref { operand } => push(operand, Use::Own),
            RValue::ArcAlloc { inner } => push(inner, Use::Own),
            RValue::Array(ops) | RValue::Tuple(ops) | RValue::StringConcat(ops) => {
                for o in ops {
                    push(o, Use::Own);
                }
            }
            RValue::EnumVariant { fields, .. } => {
                for o in fields {
                    push(o, Use::Own);
                }
            }
            RValue::Struct { fields, .. } => {
                for (_, o) in fields {
                    push(o, Use::Own);
                }
            }
            RValue::Field { object, .. } => push(object, Use::Borrow),
            RValue::Index { object, index } => {
                push(object, Use::Own);
                push(index, Use::Own);
            }
            RValue::Closure { captures, .. } => {
                for o in captures {
                    push(o, Use::Own);
                }
            }
            RValue::Map(entries) => {
                for (k, v) in entries {
                    push(k, Use::Own);
                    push(v, Use::Own);
                }
            }
            RValue::Range { start, end, .. } => {
                if let Some(s) = start {
                    push(s, Use::Own);
                }
                if let Some(e) = end {
                    push(e, Use::Own);
                }
            }
            RValue::AddrOf { local, .. } => {
                if dyn_ids.contains(&local.0) {
                    out.push((local.0, Use::Own));
                }
            }
            RValue::Comptime(inner) => {
                // Not reachable for a dyn local in practice; stay leak-safe.
                let wrapped = Instruction::Assign {
                    dest: LocalId(u32::MAX),
                    value: (**inner).clone(),
                };
                return inst_uses(&wrapped, dyn_ids, borrows);
            }
            RValue::ConstInt(_)
            | RValue::ConstFloat(_)
            | RValue::ConstBool(_)
            | RValue::ConstString(_)
            | RValue::ConstNone => {}
        },
        Instruction::StoreField { object, value, .. } => {
            push(object, Use::Borrow);
            push(value, Use::Own);
        }
        Instruction::StoreDeref { ptr, value } => {
            push(ptr, Use::Own);
            push(value, Use::Own);
        }
        Instruction::Spawn { args, .. } => {
            for a in args {
                push(a, Use::Own);
            }
        }
        Instruction::Send { channel, value } => {
            for l in [channel, value] {
                if dyn_ids.contains(&l.0) {
                    out.push((l.0, Use::Own));
                }
            }
        }
        Instruction::ActorSpawn { state, .. } => push(state, Use::Own),
        Instruction::ActorSend { args, .. } => {
            for a in args {
                push(a, Use::Own);
            }
        }
        Instruction::ActorStateStore { value, .. } => push(value, Use::Own),
        Instruction::ArcRetain { .. }
        | Instruction::ArcRelease { .. }
        | Instruction::Drop { .. }
        | Instruction::DropIfNe { .. }
        | Instruction::Nop
        | Instruction::DebugLine(_)
        | Instruction::Receive { .. }
        | Instruction::ActorStateLoad { .. } => {}
    }
    out
}

/// The dyn local an instruction defines, and whether the value it holds is a
/// reference it already owns (a producer) or an alias that must be retained.
#[derive(Clone, Copy)]
enum DefKind {
    /// Brand new reference (trait object construction, user call result).
    Owned,
    /// `dest = src`: the source's `Use::Copy` provides the reference.
    Copied,
    /// A borrowed alias (field read, channel receive, ...): retain it.
    Alias,
}

fn inst_def(
    inst: &Instruction,
    dyn_ids: &HashSet<u32>,
    borrows: &dyn Fn(&str) -> bool,
    user_fn: &dyn Fn(&str) -> bool,
) -> Option<(u32, DefKind)> {
    match inst {
        Instruction::Assign { dest, value } if dyn_ids.contains(&dest.0) => {
            let k = match value {
                RValue::MakeTraitObject { .. } | RValue::VtableCall { .. } | RValue::CallIndirect { .. } => {
                    DefKind::Owned
                }
                // A user function returns an owned reference (its own
                // `return` retains a borrowed value). A callee that is not
                // user code is not trusted to.
                RValue::Call { func, .. } if borrows(func) || user_fn(func) => DefKind::Owned,
                RValue::Use(_) => DefKind::Copied,
                _ => DefKind::Alias,
            };
            Some((dest.0, k))
        }
        Instruction::Receive { dest, .. } | Instruction::ActorStateLoad { dest, .. }
            if dyn_ids.contains(&dest.0) =>
        {
            Some((dest.0, DefKind::Alias))
        }
        _ => None,
    }
}

/// Can `from` reach itself through a path that never enters a block of
/// `barriers`?
fn reaches_without(succs: &[Vec<usize>], from: usize, barriers: &HashSet<usize>) -> bool {
    let mut seen: HashSet<usize> = HashSet::new();
    let mut stack: Vec<usize> = succs[from].clone();
    while let Some(b) = stack.pop() {
        if b == from {
            return true;
        }
        if barriers.contains(&b) || !seen.insert(b) {
            continue;
        }
        stack.extend(succs[b].iter().copied());
    }
    false
}

fn is_exception_check(inst: &Instruction) -> bool {
    matches!(inst, Instruction::Assign { value: RValue::Call { func, .. }, .. }
        if func == "kryos_exception_check")
}

/// Run the pass over every function of the module.
pub fn own_dyn_refs(functions: &mut [MirFunction]) {
    let user_names: HashSet<String> = functions.iter().map(|f| f.name.clone()).collect();
    let async_names: HashSet<String> = functions
        .iter()
        .filter(|f| f.attributes.is_async)
        .map(|f| f.name.clone())
        .collect();
    let borrows = |f: &str| user_names.contains(f) && !async_names.contains(f);
    // A user function's result is an owned reference (async ones return a
    // future, never a dyn, so the distinction does not matter for defs).
    let user_fn = |f: &str| user_names.contains(f);
    for func in functions.iter_mut() {
        own_dyn_refs_in(func, &borrows, &user_fn);
    }
}

#[derive(Default)]
struct Edits {
    /// Retains to insert BEFORE instruction `i` of the block.
    retain_before: HashMap<usize, Vec<u32>>,
    /// Retains to insert AFTER instruction `i` of the block.
    retain_after: HashMap<usize, Vec<u32>>,
    /// Releases (Drop) to insert AFTER instruction `i` of the block.
    release_after: HashMap<usize, Vec<u32>>,
    /// Retains right before the block terminator.
    retain_at_term: Vec<u32>,
}

fn own_dyn_refs_in(
    func: &mut MirFunction,
    borrows: &dyn Fn(&str) -> bool,
    user_fn: &dyn Fn(&str) -> bool,
) {
    let dyn_ids: HashSet<u32> = func
        .locals
        .iter()
        .filter(|l| matches!(l.ty, MirType::DynTrait(_)))
        .map(|l| l.id.0)
        .collect();
    if dyn_ids.is_empty() {
        return;
    }
    let param_ids: HashSet<u32> = func.params.iter().map(|p| p.local.0).collect();
    // A spawn block's wrapper is only ever started by `Spawn`, which retained
    // every dyn argument for the thread (an owning use): the wrapper releases
    // those references when it returns.
    let spawn_wrapper = func.name.starts_with("__spawn_") || func.name.starts_with("__coopspawn_");
    let spawn_owned_params: Vec<LocalId> = if spawn_wrapper {
        func.params
            .iter()
            .filter(|p| dyn_ids.contains(&p.local.0))
            .map(|p| p.local)
            .collect()
    } else {
        Vec::new()
    };
    let named: HashSet<u32> = func
        .locals
        .iter()
        .filter(|l| l.name.is_some())
        .map(|l| l.id.0)
        .collect();

    let nblocks = func.blocks.len();
    // Block ids are dense indices in lowering; map defensively anyway.
    let bidx: HashMap<u32, usize> = func
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (b.id.0, i))
        .collect();
    let succs: Vec<Vec<usize>> = func
        .blocks
        .iter()
        .map(|b| {
            let ids: Vec<BlockId> = match &b.terminator {
                Terminator::Goto(t) => vec![*t],
                Terminator::Branch { then_block, else_block, .. } => vec![*then_block, *else_block],
                Terminator::Switch { targets, default, .. } => {
                    let mut v: Vec<BlockId> = targets.iter().map(|(_, t)| *t).collect();
                    v.push(*default);
                    v
                }
                Terminator::Return(_) | Terminator::Unreachable => vec![],
            };
            ids.iter().filter_map(|t| bidx.get(&t.0).copied()).collect()
        })
        .collect();

    // uses[l] = (block, at, kind); defs[l] = (block, inst index, kind)
    let mut uses: HashMap<u32, Vec<(usize, At, Use)>> = HashMap::new();
    let mut defs: HashMap<u32, Vec<(usize, usize, DefKind)>> = HashMap::new();
    for (bi, b) in func.blocks.iter().enumerate() {
        for (ii, inst) in b.instructions.iter().enumerate() {
            for (l, k) in inst_uses(inst, &dyn_ids, borrows) {
                uses.entry(l).or_default().push((bi, At::Inst(ii), k));
            }
            if let Some((l, k)) = inst_def(inst, &dyn_ids, borrows, user_fn) {
                defs.entry(l).or_default().push((bi, ii, k));
            }
        }
        if let Terminator::Return(Some(op)) = &b.terminator {
            if let Some(l) = dyn_local(&dyn_ids, op) {
                uses.entry(l).or_default().push((bi, At::Term, Use::Ret));
            }
        }
    }

    let mut edits: Vec<Edits> = (0..nblocks).map(|_| Edits::default()).collect();
    // `Copy` uses whose source gives its reference away (a move).
    let mut moved: HashSet<(usize, usize, u32)> = HashSet::new();

    let mut ids: Vec<u32> = dyn_ids.iter().copied().collect();
    ids.sort_unstable();
    for l in ids {
        let l_uses = uses.get(&l).cloned().unwrap_or_default();
        let l_defs = defs.get(&l).cloned().unwrap_or_default();

        // Definitions that alias retain right after the definition.
        for (bi, ii, k) in &l_defs {
            if matches!(k, DefKind::Alias) {
                edits[*bi].retain_after.entry(*ii).or_default().push(l);
            }
        }

        let is_param = param_ids.contains(&l);
        let is_named = named.contains(&l);

        if is_param || is_named {
            // Holder: keeps its own reference for its whole scope (a named
            // local is dropped at scope end by lowering, a param is borrowed).
            for (bi, at, k) in &l_uses {
                match k {
                    Use::Borrow => {}
                    Use::Copy | Use::Own => match at {
                        At::Inst(ii) => edits[*bi].retain_before.entry(*ii).or_default().push(l),
                        At::Term => edits[*bi].retain_at_term.push(l),
                    },
                    Use::Ret => {
                        if is_param {
                            edits[*bi].retain_at_term.push(l);
                        } else {
                            // The scope-end Drop lowering put before the
                            // `return` releases this local, so the returned
                            // reference needs a retain taken before it. No
                            // Drop in the block means lowering already moved
                            // the local out: nothing to retain.
                            let drop_at = func.blocks[*bi]
                                .instructions
                                .iter()
                                .position(|i| matches!(i, Instruction::Drop { local } if local.0 == l));
                            if let Some(d) = drop_at {
                                edits[*bi].retain_before.entry(d).or_default().push(l);
                            }
                        }
                    }
                }
            }
            continue;
        }

        // Unnamed temp. It owns one reference after each definition.
        let def_blocks: HashSet<usize> = l_defs.iter().map(|(b, _, _)| *b).collect();
        let single_block = def_blocks.len() == 1
            && l_defs.len() == 1
            && l_uses.iter().all(|(b, at, _)| {
                *b == l_defs[0].0 && match at {
                    At::Inst(i) => *i > l_defs[0].1,
                    At::Term => true,
                }
            });

        if l_uses.is_empty() {
            // Nothing consumes it (an unused coercion): release it at once.
            for (bi, ii, _) in &l_defs {
                edits[*bi].release_after.entry(*ii).or_default().push(l);
            }
            continue;
        }

        if single_block {
            let bi = l_defs[0].0;
            // Order the uses; the last one gives the reference away or
            // releases it.
            let mut ordered: Vec<(usize, At, Use)> = l_uses.clone();
            ordered.sort_by_key(|(_, at, _)| match at {
                At::Inst(i) => *i,
                At::Term => usize::MAX,
            });
            // Uses at the very same instruction (`f(t, t)`) count separately.
            let last_at = ordered.last().map(|(_, at, _)| *at).unwrap();
            for (_, at, k) in &ordered {
                let is_last_instr = *at == last_at;
                match k {
                    Use::Borrow => {}
                    Use::Copy | Use::Own | Use::Ret => {
                        if is_last_instr {
                            // Moved. (A second owning operand of the same
                            // last instruction still needs its own.)
                            if let At::Inst(ii) = at {
                                if !moved.insert((bi, *ii, l)) {
                                    edits[bi].retain_before.entry(*ii).or_default().push(l);
                                }
                            }
                        } else if let At::Inst(ii) = at {
                            edits[bi].retain_before.entry(*ii).or_default().push(l);
                        }
                    }
                }
            }
            // Release after the last use unless the last use moved it.
            let last_moves = ordered
                .iter()
                .any(|(_, at, k)| *at == last_at && matches!(k, Use::Copy | Use::Own | Use::Ret));
            if !last_moves {
                if let At::Inst(ii) = last_at {
                    edits[bi].release_after.entry(ii).or_default().push(l);
                }
            }
            continue;
        }

        // Cross-block temp (an if / match value slot): a single owning use
        // that runs at most once per definition can take the reference.
        // That holds when the use cannot run again before a definition
        // re-runs: no path from the use back to itself that avoids every
        // definition block. A loop around the whole if / match is fine (the
        // slot is redefined on every pass); requiring the use to be off every
        // cycle left a slot inside any loop leak-safe forever, one fat box per
        // iteration.
        if let [(ub, _, k)] = l_uses.as_slice() {
            if matches!(k, Use::Copy | Use::Own | Use::Ret)
                && !l_defs.is_empty()
                && !def_blocks.contains(ub)
                && !reaches_without(&succs, *ub, &def_blocks)
            {
                // Moved: no retain, no release.
                continue;
            }
        }
        // Leak-safe: every owning use retains, the temp is never released.
        for (bi, at, k) in &l_uses {
            if matches!(k, Use::Copy | Use::Own | Use::Ret) {
                match at {
                    At::Inst(ii) => edits[*bi].retain_before.entry(*ii).or_default().push(l),
                    At::Term => edits[*bi].retain_at_term.push(l),
                }
            }
        }
    }

    // Materialize the edits.
    let mut next_local = func.locals.iter().map(|l| l.id.0).max().unwrap_or(0) + 1;
    let mut new_locals: Vec<MirLocal> = Vec::new();
    let fresh_sink = |next_local: &mut u32, new_locals: &mut Vec<MirLocal>| -> LocalId {
        let id = LocalId(*next_local);
        *next_local += 1;
        new_locals.push(MirLocal {
            id,
            name: None,
            ty: MirType::I64,
            mutable: false,
        });
        id
    };
    let retain_inst = |l: u32, sink: LocalId| Instruction::Assign {
        dest: sink,
        value: RValue::Call {
            func: RETAIN_FN.to_string(),
            args: vec![Operand::Local(LocalId(l))],
        },
    };

    for (bi, e) in edits.into_iter().enumerate() {
        if e.retain_before.is_empty()
            && e.retain_after.is_empty()
            && e.release_after.is_empty()
            && e.retain_at_term.is_empty()
        {
            continue;
        }
        let old = std::mem::take(&mut func.blocks[bi].instructions);
        let mut out: Vec<Instruction> = Vec::with_capacity(old.len() + 4);
        let n = old.len();
        let mut iter = old.into_iter().enumerate().peekable();
        while let Some((ii, inst)) = iter.next() {
            if let Some(ls) = e.retain_before.get(&ii) {
                for l in ls {
                    let sink = fresh_sink(&mut next_local, &mut new_locals);
                    out.push(retain_inst(*l, sink));
                }
            }
            out.push(inst);
            let after_retains = e.retain_after.get(&ii);
            let after_releases = e.release_after.get(&ii);
            if after_retains.is_some() || after_releases.is_some() {
                // Keep a call and its exception check adjacent.
                if let Some((_, nxt)) = iter.peek() {
                    if is_exception_check(nxt) {
                        let (_, chk) = iter.next().unwrap();
                        out.push(chk);
                    }
                }
                if let Some(ls) = after_retains {
                    for l in ls {
                        let sink = fresh_sink(&mut next_local, &mut new_locals);
                        out.push(retain_inst(*l, sink));
                    }
                }
                if let Some(ls) = after_releases {
                    for l in ls {
                        out.push(Instruction::Drop { local: LocalId(*l) });
                    }
                }
            }
        }
        debug_assert!(n > 0 || out.is_empty());
        for l in &e.retain_at_term {
            let sink = fresh_sink(&mut next_local, &mut new_locals);
            out.push(retain_inst(*l, sink));
        }
        func.blocks[bi].instructions = out;
    }
    for b in func.blocks.iter_mut() {
        if matches!(b.terminator, Terminator::Return(_)) {
            for p in &spawn_owned_params {
                b.instructions.push(Instruction::Drop { local: *p });
            }
        }
    }
    func.locals.extend(new_locals);
}
