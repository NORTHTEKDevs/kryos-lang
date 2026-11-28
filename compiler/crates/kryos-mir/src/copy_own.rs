//! Which `@copy` struct locals own their value outright, so their scope-exit
//! `Drop` may really free it (LEDGER item "copy-assign-leak").
//!
//! Both backends skip every `Drop` of an `@copy` struct: a copy used to alias
//! the original's fields, and a returned value aliases the callee's storage,
//! so freeing any holder freed fields another still pointed at. Today every
//! `@copy` value is therefore leaked: one box per literal and per `let c = b`
//! copy on Cranelift, the cloned `str` fields on LLVM.
//!
//! The backends no longer alias on a copy: `dest = src` of an `@copy` struct
//! is a deep copy on both (Cranelift `emit_struct_deep_copy`, LLVM
//! `maybe_deep_copy_struct_fields`) and a struct literal builds a fresh value
//! that clones its `str` fields. So a local that is only ever
//!
//!   * defined by a struct literal or by a copy of another `@copy` local,
//!   * read through scalar / `str` field reads (a `str` read takes its own
//!     reference), copied from, or has a scalar field stored,
//!   * and dropped,
//!
//! is the sole owner of its value, and dropping it may free it. Anything else
//! (passed to a call, returned, pushed, captured, stored into an aggregate,
//! address taken, a parameter, an array / nested-struct / map field) leaves
//! the local out of the set and keeps the old leak-only behaviour, which is
//! memory safe.
//!
//! A dataflow check keeps a read from ever seeing nothing: at every read the
//! local must be DEFINITELY defined (on every path from entry) and NOT possibly
//! already dropped. A local that fails is dropped from the set.
//!
//! A `drop(q)` at a point NO path defines `q` (the else arm of a value-producing
//! if/else whose then arm declared `q`: lowering emits the scope-exit drop on
//! every arm) does nothing at run time. It is neither an event that
//! disqualifies the local nor left in the MIR: `drop_overwritten_copies`
//! deletes it (the SSA value may not even exist). A drop where `q` is defined
//! on some paths but not all (`let r = if c { let q = ..; q.a } else { 4 }`,
//! the scope-exit drop at the join; a loop body that declares it in one arm)
//! or possibly dropped already stays: both backends zero the variable / slot
//! after a real free and an unwritten one reads as null, so that drop frees
//! at most once. Reads still must be definitely defined and not dropped.
//!
//! Blocks no path from the entry reaches are ignored: lowering leaves them
//! behind after a tail if/else / else-if / match whose arms all `return`, each
//! holding a `drop(q); return`, and counting those as events used to
//! disqualify the local.
//!
//! The result is a set of local ids; the backends consult it where they lower
//! `Instruction::Drop`. The one instruction this module adds is the `drop` in
//! front of a definition that overwrites a live value of an owned local
//! (`drop_overwritten_copies`, run by the driver before codegen).

use std::collections::{HashMap, HashSet};

use crate::ir::*;
use crate::liveness::{inst_defs_uses, terminator_uses};

fn scalar_or_str(t: &MirType) -> bool {
    matches!(
        t,
        MirType::I8
            | MirType::I16
            | MirType::I32
            | MirType::I64
            | MirType::U8
            | MirType::U16
            | MirType::U32
            | MirType::U64
            | MirType::F32
            | MirType::F64
            | MirType::Bool
            | MirType::Char
            | MirType::Str
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ev {
    Def,
    Use,
    Drop,
}

/// The analysis result: the owned locals, and every definition of an owned
/// local that overwrites a value the local still holds (block index,
/// instruction index, local), where the old value must be freed first.
#[derive(Default)]
pub struct CopyOwn {
    pub owned: HashSet<u32>,
    pub overwrites: Vec<(usize, usize, u32)>,
    /// `drop(local)` of an owned local at a point no path defines it (the
    /// other arm of an if/else whose first arm declared it): a no-op that the
    /// driver deletes so no backend ever frees an undefined slot.
    pub undefined_drops: Vec<(usize, usize, u32)>,
}

/// The `@copy` struct locals of `func` whose `Drop` may free the value.
pub fn owned_copy_locals(
    func: &MirFunction,
    copy_structs: &HashSet<String>,
    struct_defs: &HashMap<String, Vec<(String, MirType)>>,
) -> HashSet<u32> {
    analyze(func, copy_structs, struct_defs).owned
}

/// Delete the `drop(local)` of an owned local at a point no path defines it
/// and insert `drop(local)` before every definition of an owned `@copy` local that
/// would overwrite a live value of it (`p = P { .. }` on a `let mut p`): the
/// overwritten value was never freed by anything. Only a local that is
/// DEFINITELY defined and NOT possibly dropped at that point qualifies, so the
/// inserted drop is the one free of that value. The backends re-run the
/// analysis on the final MIR and free only what it accepts, so a drop this
/// pass inserts that no longer qualifies stays a no-op (leak, never a double
/// free).
pub fn drop_overwritten_copies(module: &mut MirModule) {
    if module.copy_structs.is_empty() {
        return;
    }
    let copy_structs = module.copy_structs.clone();
    let struct_defs = module.struct_defs.clone();
    for func in module.functions.iter_mut() {
        let res = analyze(func, &copy_structs, &struct_defs);
        // (block, instruction, local, insert?) back to front so earlier
        // indices stay valid
        let mut pts: Vec<(usize, usize, u32, bool)> = res
            .overwrites
            .iter()
            .map(|&(b, i, l)| (b, i, l, true))
            .chain(res.undefined_drops.iter().map(|&(b, i, l)| (b, i, l, false)))
            .collect();
        pts.sort_by(|a, b| (b.0, b.1).cmp(&(a.0, a.1)));
        for (bi, ii, l, insert) in pts {
            if insert {
                func.blocks[bi]
                    .instructions
                    .insert(ii, Instruction::Drop { local: LocalId(l) });
            } else {
                func.blocks[bi].instructions.remove(ii);
            }
        }
    }
}

fn analyze(
    func: &MirFunction,
    copy_structs: &HashSet<String>,
    struct_defs: &HashMap<String, Vec<(String, MirType)>>,
) -> CopyOwn {
    let mut res = CopyOwn::default();
    let mut out = HashSet::new();
    if func.attributes.is_async || func.blocks.is_empty() || copy_structs.is_empty() {
        return res;
    }
    let params: HashSet<u32> = func.params.iter().map(|p| p.local.0).collect();
    let non_owned: HashSet<u32> = func.attributes.non_owned_locals.iter().copied().collect();
    // Candidate locals and their struct type name.
    let mut cands: HashMap<u32, String> = HashMap::new();
    let mut ty_of: HashMap<u32, &MirType> = HashMap::new();
    for l in &func.locals {
        ty_of.insert(l.id.0, &l.ty);
        if let MirType::Struct(n) = &l.ty {
            if copy_structs.contains(n.as_str())
                && !params.contains(&l.id.0)
                && !non_owned.contains(&l.id.0)
                && struct_defs
                    .get(n.as_str())
                    .is_some_and(|fs| fs.iter().all(|(_, t)| scalar_or_str(t)))
            {
                cands.insert(l.id.0, n.clone());
            }
        }
    }
    if cands.is_empty() {
        return res;
    }
    let same_struct = |a: u32, name: &str| -> bool {
        matches!(ty_of.get(&a), Some(MirType::Struct(n)) if n == name)
    };
    let scalar_field = |sname: &str, field: &str| -> bool {
        struct_defs
            .get(sname)
            .and_then(|fs| fs.iter().find(|(n, _)| n == field))
            .is_some_and(|(_, t)| scalar_or_str(t) && !matches!(t, MirType::Str))
    };
    let str_or_scalar_field = |sname: &str, field: &str| -> bool {
        struct_defs
            .get(sname)
            .and_then(|fs| fs.iter().find(|(n, _)| n == field))
            .is_some_and(|(_, t)| scalar_or_str(t))
    };

    let mut bad: HashSet<u32> = HashSet::new();

    // Classify one instruction: the candidate events it carries, marking a
    // candidate bad when it is touched in a way this analysis does not allow.
    let classify = |inst: &Instruction, bad: &mut HashSet<u32>| -> Vec<(u32, Ev)> {
        let mut evs: Vec<(u32, Ev)> = Vec::new();
        let (defs, uses) = inst_defs_uses(inst);
        // Which of this instruction's mentions are accounted for.
        let mut ok_def: Option<u32> = None;
        let mut ok_use: Option<u32> = None;
        match inst {
            Instruction::Assign { dest, value } => {
                match value {
                    RValue::Struct { name, .. } => {
                        if let Some(n) = cands.get(&dest.0) {
                            if n == name {
                                ok_def = Some(dest.0);
                                evs.push((dest.0, Ev::Def));
                            }
                        }
                    }
                    RValue::Use(Operand::Local(src)) => {
                        // dest = src: a deep copy of an @copy struct on both
                        // backends, provided the types agree.
                        if let Some(n) = cands.get(&dest.0) {
                            if same_struct(src.0, n) {
                                ok_def = Some(dest.0);
                                if cands.contains_key(&src.0) {
                                    ok_use = Some(src.0);
                                    evs.push((src.0, Ev::Use));
                                }
                                evs.push((dest.0, Ev::Def));
                            }
                        } else if let Some(n) = cands.get(&src.0) {
                            if same_struct(dest.0, n) {
                                ok_use = Some(src.0);
                                evs.push((src.0, Ev::Use));
                            }
                        }
                    }
                    RValue::Field { object: Operand::Local(o), field } => {
                        if let Some(n) = cands.get(&o.0) {
                            if str_or_scalar_field(n, field) {
                                ok_use = Some(o.0);
                                evs.push((o.0, Ev::Use));
                            }
                        }
                    }
                    _ => {}
                }
            }
            Instruction::StoreField { object: Operand::Local(o), field, .. } => {
                if let Some(n) = cands.get(&o.0) {
                    if scalar_field(n, field) {
                        ok_use = Some(o.0);
                        evs.push((o.0, Ev::Use));
                    }
                }
            }
            Instruction::Drop { local } => {
                if cands.contains_key(&local.0) {
                    ok_use = Some(local.0);
                    evs.push((local.0, Ev::Drop));
                }
            }
            _ => {}
        }
        // Any mention beyond the accounted ones disqualifies the candidate.
        for d in &defs {
            if cands.contains_key(&d.0) && ok_def != Some(d.0) {
                bad.insert(d.0);
            }
        }
        let mut seen_ok_use = false;
        for u in &uses {
            if cands.contains_key(&u.0) {
                if ok_use == Some(u.0) && !seen_ok_use {
                    seen_ok_use = true;
                } else if ok_def == Some(u.0) && ok_use != Some(u.0) {
                    // a def of `x` whose own use list names `x`: not a form
                    // accepted above
                    bad.insert(u.0);
                } else if ok_use == Some(u.0) {
                    // the same local used twice in one instruction
                    bad.insert(u.0);
                } else {
                    bad.insert(u.0);
                }
            }
        }
        evs
    };

    // Per-block event lists, and the terminator mentions.
    let nb = func.blocks.len();
    // Block ids are not dense (a lowered function can have bb0, bb2, bb3, bb4),
    // so a successor id is NOT an index into `func.blocks`; map id -> index.
    let idx_of: HashMap<u32, usize> = func
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (b.id.0, i))
        .collect();
    // Blocks no path from the entry reaches never execute, so nothing in them
    // is a use, a def or a drop at run time. Lowering leaves such blocks
    // behind (the join block of an if/else whose arms both `return`, the
    // arms' trailing `goto`), and a scope-exit `drop(q); return` in one is
    // the common case, so they must not disqualify the local.
    let mut reach = vec![false; nb];
    {
        let mut stack = vec![0usize];
        reach[0] = true;
        while let Some(bi) = stack.pop() {
            for s in func.blocks[bi].successors() {
                if let Some(&si) = idx_of.get(&s.0) {
                    if !reach[si] {
                        reach[si] = true;
                        stack.push(si);
                    }
                }
            }
        }
    }
    let mut block_evs: Vec<Vec<(u32, Ev, usize)>> = Vec::with_capacity(nb);
    for (bi, b) in func.blocks.iter().enumerate() {
        if !reach[bi] {
            block_evs.push(Vec::new());
            continue;
        }
        let mut v = Vec::new();
        for (ii, inst) in b.instructions.iter().enumerate() {
            v.extend(classify(inst, &mut bad).into_iter().map(|(l, e)| (l, e, ii)));
        }
        let mut tu = std::collections::BTreeSet::new();
        terminator_uses(&b.terminator, &mut tu);
        for u in tu {
            if cands.contains_key(&u.0) {
                bad.insert(u.0);
            }
        }
        block_evs.push(v);
    }

    // Forward dataflow. `defined`: must (intersection); `dropped`: may (union).
    let all: HashSet<u32> = cands.keys().copied().collect();
    let mut def_in: Vec<Option<HashSet<u32>>> = vec![None; nb];
    let mut drop_in: Vec<HashSet<u32>> = vec![HashSet::new(); nb];
    // `may`: defined on SOME path (union)
    let mut may_in: Vec<HashSet<u32>> = vec![HashSet::new(); nb];
    def_in[0] = Some(HashSet::new());
    let step = |evs: &[(u32, Ev, usize)],
                d: &mut HashSet<u32>,
                x: &mut HashSet<u32>,
                m: &mut HashSet<u32>| {
        for (l, e, _) in evs {
            match e {
                Ev::Def => {
                    d.insert(*l);
                    m.insert(*l);
                    x.remove(l);
                }
                Ev::Drop => {
                    x.insert(*l);
                }
                Ev::Use => {}
            }
        }
    };
    let mut changed = true;
    let mut guard = 0;
    while changed && guard < 10_000 {
        changed = false;
        guard += 1;
        for bi in 0..nb {
            let Some(din) = def_in[bi].clone() else { continue };
            let mut d = din;
            let mut x = drop_in[bi].clone();
            let mut m = may_in[bi].clone();
            step(&block_evs[bi], &mut d, &mut x, &mut m);
            for s in func.blocks[bi].successors() {
                let Some(&si) = idx_of.get(&s.0) else { continue };
                let new_def = match &def_in[si] {
                    None => d.clone(),
                    Some(cur) => cur.intersection(&d).copied().collect(),
                };
                if def_in[si].as_ref() != Some(&new_def) {
                    def_in[si] = Some(new_def);
                    changed = true;
                }
                let before = drop_in[si].len();
                drop_in[si].extend(x.iter().copied());
                if drop_in[si].len() != before {
                    changed = true;
                }
                let before = may_in[si].len();
                may_in[si].extend(m.iter().copied());
                if may_in[si].len() != before {
                    changed = true;
                }
            }
        }
    }
    if changed {
        // Did not converge: trust nothing.
        return res;
    }
    let _ = &all;

    // Check every event against the state it sees.
    let mut overwrites: Vec<(usize, usize, u32)> = Vec::new();
    let mut undefined_drops: Vec<(usize, usize, u32)> = Vec::new();
    for bi in 0..nb {
        let Some(din) = def_in[bi].clone() else {
            // Unreachable block (no events were recorded for it above).
            continue;
        };
        let mut d = din;
        let mut x = drop_in[bi].clone();
        let mut m = may_in[bi].clone();
        for (l, e, ii) in &block_evs[bi] {
            match e {
                Ev::Def => {
                    // A definition while the local still holds a live value
                    // overwrites it; remember the point unless the new value
                    // is built from the local itself.
                    if d.contains(l) && !x.contains(l) {
                        let (_, uses) = inst_defs_uses(&func.blocks[bi].instructions[*ii]);
                        if !uses.iter().any(|u| u.0 == *l) {
                            overwrites.push((bi, *ii, *l));
                        }
                    }
                    d.insert(*l);
                    m.insert(*l);
                    x.remove(l);
                }
                Ev::Use => {
                    if !d.contains(l) || x.contains(l) {
                        bad.insert(*l);
                    }
                }
                Ev::Drop => {
                    if !m.contains(l) {
                        // no path defines it here: a no-op, removed later
                        undefined_drops.push((bi, *ii, *l));
                        continue;
                    }
                    // Defined on only some paths, or possibly dropped
                    // already: both backends null the variable / slot after
                    // a real free and read an unwritten one as null, and a
                    // free of null does nothing, so the drop frees the value
                    // at most once.
                    x.insert(*l);
                }
            }
        }
    }

    for l in cands.keys() {
        if !bad.contains(l) {
            out.insert(*l);
        }
    }
    res.undefined_drops = undefined_drops.into_iter().filter(|(_, _, l)| out.contains(l)).collect();
    res.overwrites = overwrites.into_iter().filter(|(_, _, l)| out.contains(l)).collect();
    res.owned = out;
    res
}
