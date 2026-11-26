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
//! Two dataflow checks keep a real free from ever running twice or on
//! nothing: at every drop and every read the local must be DEFINITELY defined
//! (on every path from entry) and NOT possibly already dropped. A local that
//! fails either is dropped from the set.
//!
//! The result is a set of local ids; the backends consult it where they lower
//! `Instruction::Drop`. No instruction is added or changed.

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

/// The `@copy` struct locals of `func` whose `Drop` may free the value.
pub fn owned_copy_locals(
    func: &MirFunction,
    copy_structs: &HashSet<String>,
    struct_defs: &HashMap<String, Vec<(String, MirType)>>,
) -> HashSet<u32> {
    let mut out = HashSet::new();
    if func.attributes.is_async || func.blocks.is_empty() || copy_structs.is_empty() {
        return out;
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
        return out;
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
    let mut block_evs: Vec<Vec<(u32, Ev)>> = Vec::with_capacity(nb);
    for b in &func.blocks {
        let mut v = Vec::new();
        for inst in &b.instructions {
            v.extend(classify(inst, &mut bad));
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
    def_in[0] = Some(HashSet::new());
    let step = |evs: &[(u32, Ev)], d: &mut HashSet<u32>, x: &mut HashSet<u32>| {
        for (l, e) in evs {
            match e {
                Ev::Def => {
                    d.insert(*l);
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
            step(&block_evs[bi], &mut d, &mut x);
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
            }
        }
    }
    if changed {
        // Did not converge: trust nothing.
        return out;
    }
    let _ = &all;

    // Check every event against the state it sees.
    for bi in 0..nb {
        let Some(din) = def_in[bi].clone() else {
            // Unreachable block: any candidate event in it is unprovable.
            for (l, _) in &block_evs[bi] {
                bad.insert(*l);
            }
            continue;
        };
        let mut d = din;
        let mut x = drop_in[bi].clone();
        for (l, e) in &block_evs[bi] {
            match e {
                Ev::Def => {
                    d.insert(*l);
                    x.remove(l);
                }
                Ev::Use => {
                    if !d.contains(l) || x.contains(l) {
                        bad.insert(*l);
                    }
                }
                Ev::Drop => {
                    if !d.contains(l) || x.contains(l) {
                        bad.insert(*l);
                    }
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
    out
}
