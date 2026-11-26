//! Reference counting for `shared T` handles (arc blocks): ONE ownership rule.
//!
//! A `shared T` value is a pointer to an arc block that holds a `T`. Lowering
//! treats a handle like any other binding and decides per consumer shape what
//! to drop or retain (a `let` retains an extra time, a `Some(s)` moves, a call
//! result is never released, ...), which is why every value linked through
//! `shared` leaked or, when patched shape by shape, was freed twice. This
//! module-level pass replaces all of that with one rule (LEDGER item 137):
//!
//! * a handle SLOT owns EXACTLY ONE reference. A slot is a local, a struct
//!   field, an enum payload, an array element, a map value, a global. A
//!   producer (`shared x`, the result of a user function) hands its
//!   destination one; a read that merely aliases (a field, an array element, an
//!   enum payload, a runtime call) retains it;
//! * every copy retains: `dest = src`, a store into an aggregate or container
//!   (struct literal, variant, array, `push`, `StoreField`, a global), a
//!   `return` of a handle the function does not itself own (a parameter, a
//!   named local);
//! * a use that only lends the handle for the duration of the instruction
//!   (an argument of a user function, a `*h` read, a comparison) retains
//!   nothing, and the closure / spawn / channel instructions retain by
//!   themselves in both backends;
//! * every overwrite of a slot releases the reference it owned (a local
//!   assigned again, `s.h = ..`, `xs[i] = ..`, `G = ..`), and so does every
//!   scope exit (the end of the function for a local, the drop of the
//!   aggregate or array that holds the slot) and the last use of a
//!   block-local temporary.
//!
//! The arc's own destructor (`kryos_arc_set_drop`, registered where the arc is
//! allocated) releases what the block holds when the LAST reference goes, so
//! a chain of `shared` links is freed link by link.
//!
//! An unnamed temporary whose whole life is one block is handled precisely:
//! its last use MOVES the reference (a fresh `shared make(..)` flowing into a
//! variant costs no atomic operation at all) and the temporary is released
//! right after a last use that only borrows it (at the end of the block when a
//! `*h` alias of its contents was taken). Every other handle local (a named
//! binding, a slot assigned in several blocks) keeps its own reference until it
//! is overwritten or the function returns: a block-structured scope has no
//! need for per-path release points, and a release is null-safe.
//!
//! A value read OUT of a handle (`*h` of a struct, enum, str, array, map) is a
//! copy that takes its own reference at the read; that is decided where it is
//! lowered (`lower_shared_deref_owned`), not here, because the copy's owner is
//! an ordinary value local.
//!
//! Every default is the leak-safe one: a use this pass does not understand
//! retains, a read it cannot prove owned retains. A missed retain would be a
//! use-after-free, so anything that is not provably a borrow takes a
//! reference of its own.
//!
//! The pass deletes the `Drop` / `ArcRetain` / `ArcRelease` instructions
//! lowering put on handle locals (including the extra retain of `let a =
//! shared x`) and derives all of them again. Closure capture boxes
//! (`__capbox_*`) and async functions keep their lowering-time treatment. It
//! runs on the finished module (`own_shared_refs_module`, called by the
//! driver), so lowering's own tests keep seeing lowering's output.
//!
//! NOT covered (they keep leaking, safely): a handle held in a tuple (a tuple
//! of handles is never released), a map slot overwritten by a second insert
//! under the same key, a handle in an `@copy` struct, and values reached only
//! through a `weak` reference.

use std::collections::{HashMap, HashSet};

use crate::ir::*;

/// One more reference to an arc block, returning it. Shared with closures;
/// both backends declare it for a MIR call whose result is discarded.
const RETAIN_FN: &str = "kryos_fn_retain";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Use {
    /// Lent for the duration of the instruction; the user keeps nothing.
    Borrow,
    /// A borrow whose RESULT aliases what the handle's arc holds (`*h`): the
    /// alias is valid only while the arc is, so a temporary handle is released
    /// at the end of its block, after everything that reads through the alias.
    Alias,
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DefKind {
    /// A brand new reference (`shared x`, a user function's result).
    Owned,
    /// `dest = src`: the source's `Use::Copy` provides the reference.
    Copied,
    /// A borrowed alias (a field / element / payload read, a runtime call):
    /// retain it.
    Alias,
    /// `dest = 0`: the slot is cleared, nothing to retain.
    Cleared,
}

fn handle(ids: &HashSet<u32>, op: &Operand) -> Option<u32> {
    match op {
        Operand::Local(l) if ids.contains(&l.0) => Some(l.0),
        _ => None,
    }
}

/// What the pass needs of a callee's signature: which parameters and whether
/// the result are `shared` handles. A callee typed that way follows the handle
/// contract (it borrows its handle parameters, takes a reference of its own for
/// whatever it keeps or returns, hands back an owned reference). One whose
/// parameter or result is typed otherwise -- a generic instance or a closure
/// whose handle type was erased to an integer -- does none of that: a handle
/// passed to it is stored or returned with no reference taken, so the caller
/// takes one for it and treats the result as a read.
struct Sig {
    handle_params: Vec<bool>,
    handle_ret: bool,
}

/// The callees the pass knows by name: user functions that are not async.
struct Callees {
    fns: HashMap<String, Sig>,
}

impl Sig {
    fn of_function_type(params: &[MirType], ret: &MirType) -> Sig {
        Sig {
            handle_params: params.iter().map(|p| matches!(p, MirType::Shared(_))).collect(),
            handle_ret: matches!(ret, MirType::Shared(_)),
        }
    }
}

// ---------------------------------------------------------------------------
// `json_*` node handles (LEDGER item A12)
// ---------------------------------------------------------------------------
//
// The builtin JSON API hands out nodes as plain `i64` handles. A node is an arc
// block in the runtime (kryos-stdlib-native `json.rs`): a producer returns a
// block with ONE reference, a container (`json_object` / `json_array`) takes a
// reference of its own on every child it stores and releases them when it goes,
// `json_get` / `json_get_index` return the child WITHOUT a reference (an
// alias). The handle's static type is `i64`, so nothing in lowering knew it
// owned anything and every node was leaked (+83.7MB per 750k `json_string`).
//
// The rule above applies to such a handle local unchanged: it owns one
// reference, a copy retains, an overwrite and the function's end release, an
// alias read retains. Because the type is a plain integer the pass only takes
// a local on when it can PROVE the integer is a node handle and that no
// integer copy of it escapes: every definition is a node producer / child read
// / a copy of another such local / the empty handle (0, -1), and every use
// lends it to a `json_*` reader, compares it, or copies it to another such
// local. Any other use (an argument of a user function, a struct / array /
// closure slot, a `return`, string interpolation, arithmetic) leaves the local
// out and the node is leaked, as before: a missed release is a leak, a missed
// retain would be a use-after-free. A parent whose child alias escapes is left
// out too, because the child is only valid while a reference to the parent (or
// to the child itself) is held.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum JsonFn {
    /// Returns a fresh node with one reference.
    Producer,
    /// Returns a child of argument 0 (no reference taken).
    ChildRead,
    /// Reads argument 0 and returns a scalar or a string.
    Reader,
    No,
}

const JSON_PRODUCERS: &[&str] =
    &["json_parse", "json_string", "json_object", "json_array", "json_number", "json_bool", "json_null"];
const JSON_CHILD_READS: &[&str] = &["json_get", "json_get_index"];
const JSON_READERS: &[&str] = &[
    "json_stringify",
    "json_to_str",
    "json_to_int",
    "json_to_float",
    "json_is_null",
    "json_length",
    "json_type",
];

/// The builtin a call names; a user function of the same name (std::json
/// `json_string(v) -> JsonValue`) is not the builtin.
fn json_kind(func: &str, callees: &Callees) -> JsonFn {
    if callees.fns.contains_key(func) {
        return JsonFn::No;
    }
    if JSON_PRODUCERS.contains(&func) {
        JsonFn::Producer
    } else if JSON_CHILD_READS.contains(&func) {
        JsonFn::ChildRead
    } else if JSON_READERS.contains(&func) {
        JsonFn::Reader
    } else {
        JsonFn::No
    }
}

/// The `i64` locals of `func` that provably hold `json_*` node handles with no
/// escaping integer copy (see above). Fixpoint: dropping a local can drop
/// others.
fn json_handle_locals(func: &MirFunction, callees: &Callees, addr_taken: &HashSet<u32>) -> HashSet<u32> {
    let param_ids: HashSet<u32> = func.params.iter().map(|p| p.local.0).collect();
    let mut cand: HashSet<u32> = func
        .locals
        .iter()
        .filter(|l| l.ty == MirType::I64)
        .filter(|l| !param_ids.contains(&l.id.0) && !addr_taken.contains(&l.id.0))
        .filter(|l| !l.name.as_deref().is_some_and(|n| n.starts_with("__capbox_")))
        .map(|l| l.id.0)
        .collect();
    // Only code that calls a json builtin can have a handle.
    let mentions_json = func.blocks.iter().any(|b| {
        b.instructions.iter().any(|i| match i {
            Instruction::Assign { value: RValue::Call { func: f, .. }, .. } => {
                json_kind(f, callees) != JsonFn::No
            }
            _ => false,
        })
    });
    if !mentions_json || cand.is_empty() {
        return HashSet::new();
    }
    let is_empty_handle = |v: &RValue| match v {
        RValue::ConstInt(0) | RValue::ConstInt(-1) => true,
        RValue::Use(Operand::Constant(Constant::Int(0))) | RValue::Use(Operand::Constant(Constant::Int(-1))) => true,
        _ => false,
    };
    // Definitions: every one must be a producer, a child read, a copy, or an
    // empty handle; a local with no definition is out.
    let mut has_def: HashSet<u32> = HashSet::new();
    for b in &func.blocks {
        for inst in &b.instructions {
            match inst {
                Instruction::Assign { dest, value } if cand.contains(&dest.0) => {
                    let ok = match value {
                        RValue::Call { func: f, .. } => {
                            matches!(json_kind(f, callees), JsonFn::Producer | JsonFn::ChildRead)
                        }
                        RValue::Use(Operand::Local(_)) => true,
                        v => is_empty_handle(v),
                    };
                    if ok {
                        has_def.insert(dest.0);
                    } else {
                        cand.remove(&dest.0);
                    }
                }
                Instruction::Receive { dest, .. }
                | Instruction::ActorStateLoad { dest, .. }
                | Instruction::ActorSpawn { dest, .. } => {
                    cand.remove(&dest.0);
                }
                _ => {}
            }
        }
    }
    cand.retain(|l| has_def.contains(l));
    let no_fn_locals: HashMap<u32, Sig> = HashMap::new();
    loop {
        let before = cand.len();
        let mut drop_set: Vec<u32> = Vec::new();
        for b in &func.blocks {
            for inst in &b.instructions {
                if let Instruction::Assign { dest, value } = inst {
                    // A copy's source must be a node handle too (a plain integer
                    // would be retained as if it were an arc).
                    if cand.contains(&dest.0) {
                        if let RValue::Use(Operand::Local(src)) = value {
                            if !cand.contains(&src.0) {
                                drop_set.push(dest.0);
                            }
                        }
                    }
                    // A child alias that is not tracked pins its parent out.
                    if let RValue::Call { func: f, args } = value {
                        if json_kind(f, callees) == JsonFn::ChildRead && !cand.contains(&dest.0) {
                            if let Some(Operand::Local(p)) = args.first() {
                                drop_set.push(p.0);
                            }
                        }
                    }
                }
                let uses = inst_uses(inst, &cand, callees, &no_fn_locals);
                if uses.is_empty() {
                    continue;
                }
                match inst {
                    Instruction::Assign { value: RValue::Call { func: f, args }, .. }
                        if matches!(json_kind(f, callees), JsonFn::Reader | JsonFn::ChildRead) =>
                    {
                        for (i, a) in args.iter().enumerate() {
                            if i > 0 {
                                if let Operand::Local(l) = a {
                                    drop_set.push(l.0);
                                }
                            }
                        }
                    }
                    Instruction::Assign {
                        value:
                            RValue::BinOp {
                                op:
                                    MirBinOp::Eq
                                    | MirBinOp::Neq
                                    | MirBinOp::Lt
                                    | MirBinOp::Gt
                                    | MirBinOp::LtEq
                                    | MirBinOp::GtEq,
                                ..
                            },
                        ..
                    } => {}
                    Instruction::Assign { dest, value: RValue::Use(Operand::Local(_)) } if cand.contains(&dest.0) => {}
                    _ => drop_set.extend(uses.iter().map(|(l, _)| *l)),
                }
            }
            let mut term_ops: Vec<&Operand> = Vec::new();
            match &b.terminator {
                Terminator::Return(Some(o)) => term_ops.push(o),
                Terminator::Branch { cond, .. } => term_ops.push(cond),
                Terminator::Switch { value, .. } => term_ops.push(value),
                _ => {}
            }
            for o in term_ops {
                if let Operand::Local(l) = o {
                    drop_set.push(l.0);
                }
            }
        }
        for l in drop_set {
            cand.remove(&l);
        }
        if cand.len() == before {
            break;
        }
    }
    cand
}

/// Every use of a handle local by `inst`, one entry per operand position.
fn inst_uses(
    inst: &Instruction,
    ids: &HashSet<u32>,
    callees: &Callees,
    fn_locals: &HashMap<u32, Sig>,
) -> Vec<(u32, Use)> {
    let mut out: Vec<(u32, Use)> = Vec::new();
    let mut push = |op: &Operand, k: Use| {
        if let Some(l) = handle(ids, op) {
            out.push((l, k));
        }
    };
    match inst {
        Instruction::Assign { value, .. } => match value {
            RValue::Use(op) => push(op, Use::Copy),
            RValue::Call { func, args }
                if matches!(json_kind(func, callees), JsonFn::Reader | JsonFn::ChildRead) =>
            {
                // A `json_*` reader lends its node argument; nothing else it
                // is given is a handle.
                for (i, a) in args.iter().enumerate() {
                    push(a, if i == 0 { Use::Borrow } else { Use::Own });
                }
            }
            RValue::Call { func, args } => {
                // A user function BORROWS its handle parameters (a callee that
                // keeps one takes its own reference). Anything else (`push`, the
                // map / array stores, an unknown builtin, a parameter typed as
                // something else) may store the handle: it gets its own
                // reference.
                match callees.fns.get(func.as_str()) {
                    Some(sig) => {
                        for (i, a) in args.iter().enumerate() {
                            let k = if sig.handle_params.get(i).copied().unwrap_or(false) {
                                Use::Borrow
                            } else {
                                Use::Own
                            };
                            push(a, k);
                        }
                    }
                    None => {
                        for a in args {
                            push(a, Use::Own);
                        }
                    }
                }
            }
            RValue::CallIndirect { callee, args } => {
                push(callee, Use::Borrow);
                let sig = match callee {
                    Operand::Local(f) => fn_locals.get(&f.0),
                    _ => None,
                };
                for (i, a) in args.iter().enumerate() {
                    // A closure typed `fn(shared T)` borrows; one whose
                    // parameter is typed otherwise (erased) may keep the handle.
                    let k = match sig {
                        Some(sig) if !sig.handle_params.get(i).copied().unwrap_or(false) => Use::Own,
                        _ => Use::Borrow,
                    };
                    push(a, k);
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
                push(left, Use::Borrow);
                push(right, Use::Borrow);
            }
            RValue::UnOp { operand, .. } => push(operand, Use::Borrow),
            RValue::Deref { operand } => push(operand, Use::Alias),
            RValue::Cast { operand, .. }
            | RValue::EnumTag { operand }
            | RValue::EnumPayload { operand, .. } => push(operand, Use::Own),
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
                push(object, Use::Borrow);
                push(index, Use::Borrow);
            }
            // Both backends retain a handle captured by a closure.
            RValue::Closure { captures, .. } => {
                for o in captures {
                    push(o, Use::Borrow);
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
                if ids.contains(&local.0) {
                    out.push((local.0, Use::Own));
                }
            }
            RValue::Comptime(inner) => {
                let wrapped = Instruction::Assign {
                    dest: LocalId(u32::MAX),
                    value: (**inner).clone(),
                };
                return inst_uses(&wrapped, ids, callees, fn_locals);
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
            push(ptr, Use::Borrow);
            push(value, Use::Own);
        }
        // Spawn, channel and actor sends retain a handle argument themselves.
        Instruction::Spawn { args, .. } => {
            for a in args {
                push(a, Use::Borrow);
            }
        }
        Instruction::Send { channel, value } => {
            for l in [channel, value] {
                if ids.contains(&l.0) {
                    out.push((l.0, Use::Borrow));
                }
            }
        }
        Instruction::ActorSend { args, .. } => {
            for a in args {
                push(a, Use::Borrow);
            }
        }
        Instruction::ActorSpawn { state, .. } => push(state, Use::Own),
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

/// The handle local an instruction defines, and what the value it holds is.
fn inst_def(
    inst: &Instruction,
    ids: &HashSet<u32>,
    callees: &Callees,
    fn_locals: &HashMap<u32, Sig>,
) -> Option<(u32, DefKind)> {
    match inst {
        Instruction::Assign { dest, value } if ids.contains(&dest.0) => {
            let k = match value {
                RValue::ArcAlloc { .. } => DefKind::Owned,
                RValue::VtableCall { .. } => DefKind::Owned,
                // A closure typed to return a handle returns an owned one; an
                // erased one returns whatever it was given, unretained.
                RValue::CallIndirect { callee, .. } => match callee {
                    Operand::Local(f) => match fn_locals.get(&f.0) {
                        Some(sig) if !sig.handle_ret => DefKind::Alias,
                        _ => DefKind::Owned,
                    },
                    _ => DefKind::Owned,
                },
                // A `json_*` constructor returns a fresh node with one reference.
                RValue::Call { func, .. } if json_kind(func, callees) == JsonFn::Producer => DefKind::Owned,
                // A user function typed to return a handle returns an owned
                // reference (its own `return` retains a borrowed handle).
                RValue::Call { func, .. }
                    if callees.fns.get(func.as_str()).is_some_and(|sig| sig.handle_ret) =>
                {
                    DefKind::Owned
                }
                RValue::Use(Operand::Local(src)) if ids.contains(&src.0) => DefKind::Copied,
                RValue::Use(Operand::Constant(Constant::Int(0))) => DefKind::Cleared,
                _ => DefKind::Alias,
            };
            Some((dest.0, k))
        }
        Instruction::Receive { dest, .. } | Instruction::ActorStateLoad { dest, .. }
            if ids.contains(&dest.0) =>
        {
            Some((dest.0, DefKind::Alias))
        }
        _ => None,
    }
}

fn is_exception_check(inst: &Instruction) -> bool {
    matches!(inst, Instruction::Assign { value: RValue::Call { func, .. }, .. }
        if func == "kryos_exception_check")
}

/// Run the pass over a finished module: every function, with the module's
/// struct definitions (the field types a store into a handle field needs).
/// Lowering leaves the per-shape retains / drops of handles in place (its own
/// unit tests look at them); this replaces them with the one rule.
pub fn own_shared_refs_module(module: &mut MirModule) {
    own_shared_refs(&mut module.functions, &module.struct_defs);
}

/// Run the pass over every function of the module.
pub fn own_shared_refs(
    functions: &mut [MirFunction],
    struct_defs: &HashMap<String, Vec<(String, MirType)>>,
) {
    if std::env::var_os("KRYOS_NO_SHARED_OWN").is_some() {
        return;
    }
    let callees = Callees {
        fns: functions
            .iter()
            .filter(|f| !f.attributes.is_async)
            .map(|f| {
                let ptys: Vec<MirType> = f.params.iter().map(|p| p.ty.clone()).collect();
                (f.name.clone(), Sig::of_function_type(&ptys, &f.ret_ty))
            })
            .collect(),
    };
    for func in functions.iter_mut() {
        own_shared_refs_in(func, &callees);
        release_overwritten_fields(func, struct_defs);
        release_overwritten_elements(func);
    }
}

/// `xs[i] = h` over an array of `shared` handles, and `G = h` over a global
/// that holds one, overwrite the reference the slot owned (the stored value
/// took its own by the ordinary rule): read the old value before the store and
/// release it after. A slot that was never written reads as 0 and releases
/// nothing.
fn release_overwritten_elements(func: &mut MirFunction) {
    if func.attributes.is_async {
        return;
    }
    let mut next_local = func.locals.iter().map(|l| l.id.0).max().unwrap_or(0) + 1;
    let mut new_locals: Vec<MirLocal> = Vec::new();
    let is_handle_array = |locals: &[MirLocal], id: LocalId| {
        locals.iter().any(|l| {
            l.id == id
                && matches!(&l.ty, MirType::Array(elem, _) if matches!(elem.as_ref(), MirType::Shared(_)))
        })
    };
    let is_handle = |locals: &[MirLocal], op: &Operand| match op {
        Operand::Local(id) => locals.iter().any(|l| l.id == *id && matches!(l.ty, MirType::Shared(_))),
        _ => false,
    };
    for bi in 0..func.blocks.len() {
        // The call that reads the value the store is about to replace.
        let hit = |inst: &Instruction| -> Option<RValue> {
            match inst {
                Instruction::Assign {
                    value: RValue::Call { func: f, args }, ..
                } if f == "kryos_array_set" && args.len() == 3 => match &args[0] {
                    Operand::Local(arr) if is_handle_array(&func.locals, *arr) => Some(RValue::Call {
                        func: "kryos_array_get".to_string(),
                        args: vec![Operand::Local(*arr), args[1].clone()],
                    }),
                    _ => None,
                },
                Instruction::Assign {
                    value: RValue::Call { func: f, args }, ..
                } if f == "kryos_global_set"
                    && args.len() == 2
                    && matches!(args[0], Operand::Constant(Constant::Str(_)))
                    && is_handle(&func.locals, &args[1]) =>
                {
                    Some(RValue::Call {
                        func: "kryos_global_get".to_string(),
                        args: vec![args[0].clone()],
                    })
                }
                _ => None,
            }
        };
        if !func.blocks[bi].instructions.iter().any(|i| hit(i).is_some()) {
            continue;
        }
        let old = std::mem::take(&mut func.blocks[bi].instructions);
        let mut out: Vec<Instruction> = Vec::with_capacity(old.len() + 4);
        for inst in old {
            if let Some(read_old) = hit(&inst) {
                let old_el = LocalId(next_local);
                let sink = LocalId(next_local + 1);
                next_local += 2;
                new_locals.push(MirLocal { id: old_el, name: None, ty: MirType::I64, mutable: false });
                new_locals.push(MirLocal { id: sink, name: None, ty: MirType::I64, mutable: false });
                out.push(Instruction::Assign { dest: old_el, value: read_old });
                out.push(inst);
                out.push(Instruction::Assign {
                    dest: sink,
                    value: RValue::Call {
                        func: "kryos_fn_release_if_ne".to_string(),
                        args: vec![Operand::Local(old_el), Operand::Constant(Constant::Int(0))],
                    },
                });
            } else {
                out.push(inst);
            }
        }
        func.blocks[bi].instructions = out;
    }
    func.locals.extend(new_locals);
}

/// A store into a struct field that holds a `shared` handle overwrites the
/// reference the slot owned: read the old handle, store, release it. (The
/// value being stored took its own reference by the ordinary rule, so
/// `s.h = s.h` nets to nothing.) A zeroed field reads as null and releases
/// nothing.
fn release_overwritten_fields(
    func: &mut MirFunction,
    struct_defs: &HashMap<String, Vec<(String, MirType)>>,
) {
    if func.attributes.is_async {
        return;
    }
    let local_ty = |locals: &[MirLocal], id: LocalId| -> Option<MirType> {
        locals.iter().find(|l| l.id == id).map(|l| l.ty.clone())
    };
    let mut next_local = func.locals.iter().map(|l| l.id.0).max().unwrap_or(0) + 1;
    let mut new_locals: Vec<MirLocal> = Vec::new();
    for bi in 0..func.blocks.len() {
        let mut hits: Vec<(usize, LocalId, String, MirType)> = Vec::new();
        for (ii, inst) in func.blocks[bi].instructions.iter().enumerate() {
            let Instruction::StoreField { object: Operand::Local(o), field, .. } = inst else {
                continue;
            };
            let Some(MirType::Struct(sname)) = local_ty(&func.locals, *o) else { continue };
            let Some(defs) = struct_defs.get(&sname) else { continue };
            let Some((_, fty @ MirType::Shared(_))) = defs.iter().find(|(n, _)| n == field) else {
                continue;
            };
            hits.push((ii, *o, field.clone(), fty.clone()));
        }
        if hits.is_empty() {
            continue;
        }
        let old = std::mem::take(&mut func.blocks[bi].instructions);
        let mut out: Vec<Instruction> = Vec::with_capacity(old.len() + hits.len() * 2);
        for (ii, inst) in old.into_iter().enumerate() {
            let hit = hits.iter().find(|h| h.0 == ii);
            if let Some((_, o, field, fty)) = hit {
                let tmp = LocalId(next_local);
                next_local += 1;
                new_locals.push(MirLocal { id: tmp, name: None, ty: fty.clone(), mutable: false });
                out.push(Instruction::Assign {
                    dest: tmp,
                    value: RValue::Field { object: Operand::Local(*o), field: field.clone() },
                });
                out.push(inst);
                out.push(Instruction::ArcRelease { ptr: tmp });
            } else {
                out.push(inst);
            }
        }
        func.blocks[bi].instructions = out;
    }
    func.locals.extend(new_locals);
}

/// The release of the reference a handle local owns: a `Drop` for a `shared`
/// local (type-directed), the guarded runtime release for a `json_*` node
/// handle, whose static type is a plain `i64` (the call is the same one the
/// element-overwrite rule uses, so every backend already knows it; 0 and -1 are
/// no-ops).
fn release_inst(l: LocalId, json_ids: &HashSet<u32>, next_local: &mut u32, new_locals: &mut Vec<MirLocal>) -> Instruction {
    if !json_ids.contains(&l.0) {
        return Instruction::Drop { local: l };
    }
    let sink = LocalId(*next_local);
    *next_local += 1;
    new_locals.push(MirLocal { id: sink, name: None, ty: MirType::I64, mutable: false });
    Instruction::Assign {
        dest: sink,
        value: RValue::Call {
            func: "kryos_fn_release_if_ne".to_string(),
            args: vec![Operand::Local(l), Operand::Constant(Constant::Int(0))],
        },
    }
}

#[derive(Default)]
struct Edits {
    /// Instructions to delete (lowering's own drops / retains of handles).
    remove: HashSet<usize>,
    /// Retains to insert BEFORE instruction `i` of the block.
    retain_before: HashMap<usize, Vec<u32>>,
    /// Retains to insert AFTER instruction `i` (and its exception check).
    retain_after: HashMap<usize, Vec<u32>>,
    /// Releases (Drop) to insert BEFORE instruction `i`: an overwritten slot.
    release_before: HashMap<usize, Vec<u32>>,
    /// Releases to insert AFTER instruction `i` (and its exception check).
    release_after: HashMap<usize, Vec<u32>>,
    /// Instructions `i` whose destination is also read by the instruction:
    /// the result goes through a fresh temp so the old value is released
    /// after the right-hand side was evaluated.
    via_temp: HashSet<usize>,
    /// Retains right before the block terminator.
    retain_at_term: Vec<u32>,
    /// Releases right before the block terminator (after the retains).
    release_at_term: Vec<u32>,
}

fn own_shared_refs_in(func: &mut MirFunction, callees: &Callees) {
    if func.attributes.is_async {
        return;
    }
    // Locals holding a function value: what their call may do with a handle.
    let fn_locals: HashMap<u32, Sig> = func
        .locals
        .iter()
        .filter_map(|l| match &l.ty {
            MirType::Function { params, ret } => Some((l.id.0, Sig::of_function_type(params, ret))),
            _ => None,
        })
        .collect();
    let param_ids: HashSet<u32> = func.params.iter().map(|p| p.local.0).collect();
    let spawn_wrapper = func.name.starts_with("__spawn_") || func.name.starts_with("__coopspawn_");
    let mut addr_taken: HashSet<u32> = HashSet::new();
    for b in &func.blocks {
        for inst in &b.instructions {
            if let Instruction::Assign { value: RValue::AddrOf { local, .. }, .. } = inst {
                addr_taken.insert(local.0);
            }
        }
    }
    // Every local of a shared type, except closure capture boxes (balanced by
    // lowering and the closure env) and address-taken ones.
    let shared_ids: HashSet<u32> = func
        .locals
        .iter()
        .filter(|l| matches!(l.ty, MirType::Shared(_)))
        .filter(|l| !l.name.as_deref().is_some_and(|n| n.starts_with("__capbox_")))
        .filter(|l| !addr_taken.contains(&l.id.0))
        .map(|l| l.id.0)
        .collect();
    let json_ids = json_handle_locals(func, callees, &addr_taken);
    let ids: HashSet<u32> = shared_ids.union(&json_ids).copied().collect();
    if ids.is_empty() {
        return;
    }
    // A parameter that the body reassigns is outside the model.
    for b in &func.blocks {
        for inst in &b.instructions {
            if let Some((d, _)) = inst_def(inst, &ids, callees, &fn_locals) {
                if param_ids.contains(&d) {
                    return;
                }
            }
        }
    }
    let named: HashSet<u32> = func
        .locals
        .iter()
        .filter(|l| l.name.is_some())
        .map(|l| l.id.0)
        .collect();

    let nblocks = func.blocks.len();
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
    // in_cycle[b]: block b can reach itself.
    let in_cycle: Vec<bool> = (0..nblocks)
        .map(|b| {
            let mut seen: HashSet<usize> = HashSet::new();
            let mut stack: Vec<usize> = succs[b].clone();
            while let Some(x) = stack.pop() {
                if x == b {
                    return true;
                }
                if seen.insert(x) {
                    stack.extend(succs[x].iter().copied());
                }
            }
            false
        })
        .collect();

    // uses[l] = (block, at, kind); defs[l] = (block, inst index, kind)
    let mut uses: HashMap<u32, Vec<(usize, At, Use)>> = HashMap::new();
    let mut defs: HashMap<u32, Vec<(usize, usize, DefKind)>> = HashMap::new();
    let mut edits: Vec<Edits> = (0..nblocks).map(|_| Edits::default()).collect();
    for (bi, b) in func.blocks.iter().enumerate() {
        for (ii, inst) in b.instructions.iter().enumerate() {
            match inst {
                Instruction::Drop { local } | Instruction::ArcRetain { ptr: local } | Instruction::ArcRelease { ptr: local }
                    if ids.contains(&local.0) =>
                {
                    edits[bi].remove.insert(ii);
                    continue;
                }
                _ => {}
            }
            for (l, k) in inst_uses(inst, &ids, callees, &fn_locals) {
                uses.entry(l).or_default().push((bi, At::Inst(ii), k));
            }
            if let Some((l, k)) = inst_def(inst, &ids, callees, &fn_locals) {
                defs.entry(l).or_default().push((bi, ii, k));
            }
        }
        if let Terminator::Return(Some(op)) = &b.terminator {
            if let Some(l) = handle(&ids, op) {
                uses.entry(l).or_default().push((bi, At::Term, Use::Ret));
            }
        }
    }

    // `Copy` / `Own` / `Ret` uses whose source gives its reference away.
    let mut moved: HashSet<(usize, usize, u32)> = HashSet::new();
    // B-class locals released at every function exit.
    let mut exit_release: Vec<u32> = Vec::new();

    let mut sorted: Vec<u32> = ids.iter().copied().collect();
    sorted.sort_unstable();
    for l in sorted {
        let l_uses = uses.get(&l).cloned().unwrap_or_default();
        let l_defs = defs.get(&l).cloned().unwrap_or_default();

        // A parameter is borrowed: it owns nothing, so an owning use (a
        // store, a copy, a return) takes a reference of its own.
        if param_ids.contains(&l) {
            for (bi, at, k) in &l_uses {
                match k {
                    Use::Borrow | Use::Alias => {}
                    Use::Copy | Use::Own => match at {
                        At::Inst(ii) => edits[*bi].retain_before.entry(*ii).or_default().push(l),
                        At::Term => edits[*bi].retain_at_term.push(l),
                    },
                    Use::Ret => edits[*bi].retain_at_term.push(l),
                }
            }
            // A spawn block's wrapper is only ever started by `Spawn`, which
            // retained every handle argument for the thread: that reference is
            // released when the wrapper returns.
            if spawn_wrapper {
                exit_release.push(l);
            }
            continue;
        }

        // A-class: an unnamed temp defined once and used only later in the
        // same block. Precise in-block accounting, moves included.
        let a_class = !named.contains(&l)
            && l_defs.len() == 1
            && l_uses.iter().all(|(b, at, _)| {
                *b == l_defs[0].0
                    && match at {
                        At::Inst(i) => *i > l_defs[0].1,
                        At::Term => true,
                    }
            });
        if a_class {
            let (bi, di, dk) = l_defs[0];
            if l_uses.is_empty() {
                // Nothing consumes it: a reference it owns is released at
                // once; an unused alias never took one.
                if matches!(dk, DefKind::Owned | DefKind::Copied) {
                    edits[bi].release_after.entry(di).or_default().push(l);
                }
                continue;
            }
            // The temp owns one reference after its definition. An alias
            // read retains it right after the definition.
            if dk == DefKind::Alias {
                edits[bi].retain_after.entry(di).or_default().push(l);
            }
            let mut ordered: Vec<(usize, At, Use)> = l_uses.clone();
            ordered.sort_by_key(|(_, at, _)| match at {
                At::Inst(i) => *i,
                At::Term => usize::MAX,
            });
            let last_at = ordered.last().map(|(_, at, _)| *at).unwrap();
            for (_, at, k) in &ordered {
                let is_last = *at == last_at;
                match k {
                    Use::Borrow | Use::Alias => {}
                    Use::Copy | Use::Own | Use::Ret => {
                        if is_last {
                            // Moved. A second owning operand of the same last
                            // instruction still needs its own reference.
                            if let At::Inst(ii) = at {
                                if !moved.insert((bi, *ii, l)) {
                                    edits[bi].retain_before.entry(*ii).or_default().push(l);
                                }
                            } else if !moved.insert((bi, usize::MAX, l)) {
                                edits[bi].retain_at_term.push(l);
                            }
                        } else if let At::Inst(ii) = at {
                            edits[bi].retain_before.entry(*ii).or_default().push(l);
                        }
                    }
                }
            }
            let last_moves = ordered
                .iter()
                .any(|(_, at, k)| *at == last_at && matches!(k, Use::Copy | Use::Own | Use::Ret));
            if !last_moves {
                // A deref of the temp hands out an alias of the arc's contents
                // (`(*mk(i)).name`): the field read that follows the deref is
                // a later instruction, so the temp lives to the end of its
                // block.
                let aliased = ordered.iter().any(|(_, _, k)| *k == Use::Alias);
                match last_at {
                    At::Inst(_) if aliased => edits[bi].release_at_term.push(l),
                    At::Inst(ii) => edits[bi].release_after.entry(ii).or_default().push(l),
                    At::Term => edits[bi].release_at_term.push(l),
                }
            }
            continue;
        }

        // B-class: owns its reference from each definition until it is
        // overwritten or the function returns.
        for (bi, ii, k) in &l_defs {
            if *k == DefKind::Alias {
                edits[*bi].retain_after.entry(*ii).or_default().push(l);
            }
            // A previous value may still be held when the slot is defined
            // again (another definition, or a loop around this one).
            let reassigned = l_defs.len() > 1 || in_cycle[*bi];
            if reassigned {
                let reads_self = inst_uses(&func.blocks[*bi].instructions[*ii], &ids, callees, &fn_locals)
                    .iter()
                    .any(|(u, _)| *u == l);
                if reads_self {
                    edits[*bi].via_temp.insert(*ii);
                } else {
                    edits[*bi].release_before.entry(*ii).or_default().push(l);
                }
            }
        }
        for (bi, at, k) in &l_uses {
            match k {
                Use::Borrow | Use::Alias => {}
                Use::Copy | Use::Own => match at {
                    At::Inst(ii) => edits[*bi].retain_before.entry(*ii).or_default().push(l),
                    At::Term => edits[*bi].retain_at_term.push(l),
                },
                Use::Ret => edits[*bi].retain_at_term.push(l),
            }
        }
        if !l_defs.is_empty() {
            exit_release.push(l);
        }
    }

    // Exits: every Return block releases the B-class handles it may hold.
    if !exit_release.is_empty() {
        for (bi, b) in func.blocks.iter().enumerate() {
            if matches!(b.terminator, Terminator::Return(_)) {
                for l in &exit_release {
                    edits[bi].release_at_term.push(*l);
                }
            }
        }
    }

    // Materialize the edits.
    let mut next_local = func.locals.iter().map(|l| l.id.0).max().unwrap_or(0) + 1;
    let mut new_locals: Vec<MirLocal> = Vec::new();
    let handle_ty = |l: u32, locals: &[MirLocal]| -> MirType {
        locals
            .iter()
            .find(|x| x.id.0 == l)
            .map(|x| x.ty.clone())
            .unwrap_or_else(|| MirType::Shared(Box::new(MirType::I64)))
    };
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
    let mut changed = false;
    for (bi, e) in edits.into_iter().enumerate() {
        if e.remove.is_empty()
            && e.retain_before.is_empty()
            && e.retain_after.is_empty()
            && e.release_before.is_empty()
            && e.release_after.is_empty()
            && e.via_temp.is_empty()
            && e.retain_at_term.is_empty()
            && e.release_at_term.is_empty()
        {
            continue;
        }
        changed = true;
        let old = std::mem::take(&mut func.blocks[bi].instructions);
        let mut out: Vec<Instruction> = Vec::with_capacity(old.len() + 4);
        let mut iter = old.into_iter().enumerate().peekable();
        while let Some((ii, inst)) = iter.next() {
            if e.remove.contains(&ii) {
                continue;
            }
            if let Some(ls) = e.retain_before.get(&ii) {
                for l in ls {
                    let sink = fresh_sink(&mut next_local, &mut new_locals);
                    out.push(retain_inst(*l, sink));
                }
            }
            let mut temp_dest: Option<(LocalId, LocalId)> = None;
            if e.via_temp.contains(&ii) {
                // `l = rhs(l)`: evaluate into a fresh handle temp, release the
                // old value of `l`, then move the temp into `l`.
                if let Instruction::Assign { dest, value } = &inst {
                    let ty = handle_ty(dest.0, &func.locals);
                    let tmp = LocalId(next_local);
                    next_local += 1;
                    new_locals.push(MirLocal { id: tmp, name: None, ty, mutable: false });
                    out.push(Instruction::Assign { dest: tmp, value: value.clone() });
                    temp_dest = Some((*dest, tmp));
                }
            }
            if temp_dest.is_none() {
                if let Some(ls) = e.release_before.get(&ii) {
                    for l in ls {
                        out.push(release_inst(LocalId(*l), &json_ids, &mut next_local, &mut new_locals));
                    }
                }
                out.push(inst);
            }
            let after_retains = e.retain_after.get(&ii);
            let after_releases = e.release_after.get(&ii);
            if after_retains.is_some() || after_releases.is_some() || temp_dest.is_some() {
                // Keep a call and its exception check adjacent.
                if let Some((_, nxt)) = iter.peek() {
                    if is_exception_check(nxt) {
                        let (_, chk) = iter.next().unwrap();
                        out.push(chk);
                    }
                }
                if let Some((dest, tmp)) = temp_dest {
                    // The temp holds the new reference (retain an alias read
                    // first); the old value of the slot goes, the slot takes
                    // the temp's reference.
                    if after_retains.is_some_and(|v| v.contains(&dest.0)) {
                        let sink = fresh_sink(&mut next_local, &mut new_locals);
                        out.push(retain_inst(tmp.0, sink));
                    }
                    out.push(release_inst(dest, &json_ids, &mut next_local, &mut new_locals));
                    out.push(Instruction::Assign {
                        dest,
                        value: RValue::Use(Operand::Local(tmp)),
                    });
                    if let Some(ls) = after_retains {
                        for l in ls {
                            if *l != dest.0 {
                                let sink = fresh_sink(&mut next_local, &mut new_locals);
                                out.push(retain_inst(*l, sink));
                            }
                        }
                    }
                } else if let Some(ls) = after_retains {
                    for l in ls {
                        let sink = fresh_sink(&mut next_local, &mut new_locals);
                        out.push(retain_inst(*l, sink));
                    }
                }
                if let Some(ls) = after_releases {
                    for l in ls {
                        out.push(release_inst(LocalId(*l), &json_ids, &mut next_local, &mut new_locals));
                    }
                }
            }
        }
        for l in &e.retain_at_term {
            let sink = fresh_sink(&mut next_local, &mut new_locals);
            out.push(retain_inst(*l, sink));
        }
        for l in &e.release_at_term {
            out.push(release_inst(LocalId(*l), &json_ids, &mut next_local, &mut new_locals));
        }
        func.blocks[bi].instructions = out;
    }
    func.locals.extend(new_locals);
    if changed {
        // Cranelift's exception cleanup would release a named handle local on
        // an unwind whatever its state; this pass owns their lifetimes.
        let mut set: HashSet<u32> = func.attributes.non_owned_locals.iter().copied().collect();
        for l in &ids {
            if named.contains(l) && set.insert(*l) {
                func.attributes.non_owned_locals.push(*l);
            }
        }
    }
}
