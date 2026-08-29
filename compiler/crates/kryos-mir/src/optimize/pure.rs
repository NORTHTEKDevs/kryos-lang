//! `@pure` function optimization pass.
//!
//! Two optimizations for functions marked `@pure` (no side effects):
//!
//! 1. **Common subexpression elimination (CSE)** — within a basic block, if
//!    the same `@pure` function is called with identical arguments, the
//!    duplicate call is replaced with the earlier result.
//!
//! 2. **Dead pure call removal** — calls to `@pure` functions whose results
//!    are never used can be removed (normal DCE conservatively keeps all
//!    calls because they might have side effects).

use std::collections::{HashMap, HashSet};

use crate::ir::{Instruction, LocalId, MirModule, Operand, RValue};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run pure-function optimizations on the module.
///
/// Gathers the set of `@pure` functions, then runs CSE and dead-pure-call
/// removal on every function.
pub fn optimize_pure(module: &mut MirModule) {
    let pure_fns: HashSet<String> = module
        .functions
        .iter()
        .filter(|f| f.attributes.pure_fn)
        .map(|f| f.name.clone())
        .collect();

    if pure_fns.is_empty() {
        return;
    }

    // CSE first (may create dead assignments), then remove dead pure calls.
    for func in &mut module.functions {
        cse_pure_calls(func, &pure_fns);
    }
    remove_dead_pure_calls(module, &pure_fns);
}

// ---------------------------------------------------------------------------
// CSE for pure calls
// ---------------------------------------------------------------------------

/// A key that uniquely identifies a pure call: (function_name, arguments).
#[derive(Hash, Eq, PartialEq, Clone)]
struct PureCallKey {
    func: String,
    args: Vec<OperandKey>,
}

/// Hashable operand representation for CSE comparison.
#[derive(Hash, Eq, PartialEq, Clone)]
enum OperandKey {
    Local(u32),
    ConstInt(i64),
    ConstFloat(u64), // bit pattern for exact comparison
    ConstBool(bool),
    ConstString(String),
    ConstNone,
}

fn operand_to_key(op: &Operand) -> Option<OperandKey> {
    match op {
        Operand::Local(id) => Some(OperandKey::Local(id.0)),
        Operand::Constant(c) => match c {
            crate::ir::Constant::Int(v) => Some(OperandKey::ConstInt(*v)),
            crate::ir::Constant::Float(v) => Some(OperandKey::ConstFloat(v.to_bits())),
            crate::ir::Constant::Bool(v) => Some(OperandKey::ConstBool(*v)),
            crate::ir::Constant::Str(s) => Some(OperandKey::ConstString(s.clone())),
            crate::ir::Constant::None => Some(OperandKey::ConstNone),
        },
    }
}

fn make_call_key(func: &str, args: &[Operand]) -> Option<PureCallKey> {
    let arg_keys: Option<Vec<OperandKey>> = args.iter().map(operand_to_key).collect();
    arg_keys.map(|keys| PureCallKey {
        func: func.to_string(),
        args: keys,
    })
}

/// Within each basic block, replace duplicate calls to `@pure` functions
/// (same name + same arguments) with the result of the first call.
fn cse_pure_calls(func: &mut crate::ir::MirFunction, pure_fns: &HashSet<String>) {
    for block in &mut func.blocks {
        // Map from pure call key → the LocalId that holds the result.
        let mut seen: HashMap<PureCallKey, LocalId> = HashMap::new();

        for inst in &mut block.instructions {
            if let Instruction::Assign {
                dest,
                value: RValue::Call { func: callee, args },
            } = inst
            {
                if pure_fns.contains(callee.as_str()) {
                    if let Some(key) = make_call_key(callee, args) {
                        if let Some(&prev_dest) = seen.get(&key) {
                            // Replace duplicate call with Use of previous result.
                            *inst = Instruction::Assign {
                                dest: *dest,
                                value: RValue::Use(Operand::Local(prev_dest)),
                            };
                            continue;
                        }
                        seen.insert(key, *dest);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Dead pure call removal
// ---------------------------------------------------------------------------

/// Remove calls to `@pure` functions whose results are never used.
///
/// This is a targeted enhancement of DCE — normal DCE conservatively keeps
/// all calls (they might have side effects), but we know `@pure` calls don't.
fn remove_dead_pure_calls(module: &mut MirModule, pure_fns: &HashSet<String>) {
    for func in &mut module.functions {
        let used = collect_used_locals(func);

        for block in &mut func.blocks {
            block.instructions.retain(|inst| {
                if let Instruction::Assign {
                    dest,
                    value: RValue::Call { func: callee, .. },
                } = inst
                {
                    if pure_fns.contains(callee.as_str()) && !used.contains(&dest.0) {
                        return false; // dead pure call — remove
                    }
                }
                true
            });
        }
    }
}

/// Collect all locals that are read anywhere in a function.
fn collect_used_locals(func: &crate::ir::MirFunction) -> HashSet<u32> {
    let mut used = HashSet::new();
    for block in &func.blocks {
        for inst in &block.instructions {
            collect_inst_reads(inst, &mut used);
        }
        collect_term_reads(&block.terminator, &mut used);
    }
    used
}

fn collect_inst_reads(inst: &Instruction, used: &mut HashSet<u32>) {
    match inst {
        Instruction::Assign { value, .. } => collect_rvalue_reads(value, used),
        Instruction::ArcRetain { ptr } | Instruction::ArcRelease { ptr } => {
            used.insert(ptr.0);
        }
        Instruction::Drop { local } => {
            used.insert(local.0);
        }
        Instruction::DropIfNe { old, new, .. } => {
            used.insert(old.0);
            collect_operand_reads(new, used);
        }
        Instruction::Send { channel, value } => {
            used.insert(channel.0);
            used.insert(value.0);
        }
        Instruction::Receive { channel, .. } => {
            used.insert(channel.0);
        }
        Instruction::Spawn { args, .. } => {
            for arg in args {
                collect_operand_reads(arg, used);
            }
        }
        Instruction::ActorSpawn { state, .. } => {
            collect_operand_reads(state, used);
        }
        Instruction::ActorSend { actor, args, .. } => {
            used.insert(actor.0);
            for arg in args {
                collect_operand_reads(arg, used);
            }
        }
        Instruction::ActorStateLoad { state_ptr, .. } => {
            used.insert(state_ptr.0);
        }
        Instruction::ActorStateStore {
            state_ptr, value, ..
        } => {
            used.insert(state_ptr.0);
            collect_operand_reads(value, used);
        }
        Instruction::StoreField { object, value, .. } => {
            collect_operand_reads(object, used);
            collect_operand_reads(value, used);
        }
        Instruction::StoreDeref { ptr, value } => {
            collect_operand_reads(ptr, used);
            collect_operand_reads(value, used);
        }
        Instruction::Nop | Instruction::DebugLine(_) => {}
    }
}

fn collect_rvalue_reads(rv: &RValue, used: &mut HashSet<u32>) {
    match rv {
        RValue::Use(op)
        | RValue::Cast { operand: op, .. }
        | RValue::Deref { operand: op }
        | RValue::EnumTag { operand: op }
        | RValue::Field { object: op, .. }
        | RValue::ArcAlloc { inner: op } => {
            collect_operand_reads(op, used);
        }
        RValue::BinOp { left, right, .. }
        | RValue::Index {
            object: left,
            index: right,
        } => {
            collect_operand_reads(left, used);
            collect_operand_reads(right, used);
        }
        RValue::UnOp { operand, .. } => collect_operand_reads(operand, used),
        RValue::Call { args, .. } => {
            for a in args {
                collect_operand_reads(a, used);
            }
        }
        RValue::CallIndirect { callee, args } => {
            collect_operand_reads(callee, used);
            for a in args {
                collect_operand_reads(a, used);
            }
        }
        RValue::VtableCall { object, args, .. } => {
            collect_operand_reads(object, used);
            for a in args {
                collect_operand_reads(a, used);
            }
        }
        RValue::Array(elems) | RValue::Tuple(elems) | RValue::StringConcat(elems) => {
            for e in elems {
                collect_operand_reads(e, used);
            }
        }
        RValue::Struct { fields, .. } => {
            for (_, op) in fields {
                collect_operand_reads(op, used);
            }
        }
        RValue::Closure { captures, .. } => {
            for c in captures {
                collect_operand_reads(c, used);
            }
        }
        RValue::EnumVariant { fields, .. } => {
            for f in fields {
                collect_operand_reads(f, used);
            }
        }
        RValue::EnumPayload { operand, .. } => collect_operand_reads(operand, used),
        RValue::Map(pairs) => {
            for (k, v) in pairs {
                collect_operand_reads(k, used);
                collect_operand_reads(v, used);
            }
        }
        RValue::Range { start, end, .. } => {
            if let Some(s) = start {
                collect_operand_reads(s, used);
            }
            if let Some(e) = end {
                collect_operand_reads(e, used);
            }
        }
        RValue::AddrOf { local, .. } => {
            used.insert(local.0);
        }
        RValue::Comptime(inner) => collect_rvalue_reads(inner, used),
        RValue::MakeTraitObject { value, .. } => collect_operand_reads(value, used),
        RValue::ConstInt(_)
        | RValue::ConstFloat(_)
        | RValue::ConstBool(_)
        | RValue::ConstString(_)
        | RValue::ConstNone => {}
    }
}

fn collect_operand_reads(op: &Operand, used: &mut HashSet<u32>) {
    if let Operand::Local(id) = op {
        used.insert(id.0);
    }
}

fn collect_term_reads(term: &crate::ir::Terminator, used: &mut HashSet<u32>) {
    match term {
        crate::ir::Terminator::Return(Some(op)) => collect_operand_reads(op, used),
        crate::ir::Terminator::Branch { cond, .. } => collect_operand_reads(cond, used),
        crate::ir::Terminator::Switch { value, .. } => collect_operand_reads(value, used),
        _ => {}
    }
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

    fn pure_attrs() -> MirAttributes {
        MirAttributes {
            pure_fn: true,
            ..MirAttributes::default()
        }
    }

    /// Module with a `@pure fn double(x)` and a `main` that calls it twice
    /// with the same argument.
    fn make_cse_module() -> MirModule {
        MirModule {
            functions: vec![
                MirFunction {
                    name: "double".into(),
                    params: vec![MirParam {
                        local: LocalId(0),
                        ty: MirType::I64,
                    }],
                    ret_ty: MirType::I64,
                    blocks: vec![BasicBlock {
                        id: BlockId(0),
                        instructions: vec![Instruction::Assign {
                            dest: LocalId(1),
                            value: RValue::BinOp {
                                op: MirBinOp::Mul,
                                left: Operand::Local(LocalId(0)),
                                right: Operand::Constant(Constant::Int(2)),
                            },
                        }],
                        terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
                    }],
                    locals: vec![make_local(0), make_local(1)],
                    attributes: pure_attrs(),
                    source_file: None,
                    source_line: 0,
                },
                // fn main() -> i64 {
                //   _0 = double(5)
                //   _1 = double(5)  // same args — should be CSE'd
                //   _2 = _0 + _1
                //   return _2
                // }
                MirFunction {
                    name: "main".into(),
                    params: vec![],
                    ret_ty: MirType::I64,
                    blocks: vec![BasicBlock {
                        id: BlockId(0),
                        instructions: vec![
                            Instruction::Assign {
                                dest: LocalId(0),
                                value: RValue::Call {
                                    func: "double".into(),
                                    args: vec![Operand::Constant(Constant::Int(5))],
                                },
                            },
                            Instruction::Assign {
                                dest: LocalId(1),
                                value: RValue::Call {
                                    func: "double".into(),
                                    args: vec![Operand::Constant(Constant::Int(5))],
                                },
                            },
                            Instruction::Assign {
                                dest: LocalId(2),
                                value: RValue::BinOp {
                                    op: MirBinOp::Add,
                                    left: Operand::Local(LocalId(0)),
                                    right: Operand::Local(LocalId(1)),
                                },
                            },
                        ],
                        terminator: Terminator::Return(Some(Operand::Local(LocalId(2)))),
                    }],
                    locals: vec![make_local(0), make_local(1), make_local(2)],
                    attributes: MirAttributes::default(),
                    source_file: None,
                    source_line: 0,
                },
            ],
            struct_defs: Default::default(),
            enum_defs: Default::default(),
            trait_vtables: Default::default(),
            copy_structs: Default::default(),
        }
    }

    #[test]
    fn cse_eliminates_duplicate_pure_call() {
        let mut module = make_cse_module();
        optimize_pure(&mut module);

        let main = &module.functions[1];
        let insts = &main.blocks[0].instructions;

        // First call should remain.
        assert!(matches!(
            &insts[0],
            Instruction::Assign {
                value: RValue::Call { func, .. },
                ..
            } if func == "double"
        ));

        // Second call should be replaced with Use of _0.
        assert!(
            matches!(
                &insts[1],
                Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::Use(Operand::Local(LocalId(0))),
                }
            ),
            "second call should be CSE'd to Use(_0), got {:?}",
            insts[1]
        );
    }

    #[test]
    fn no_cse_for_different_args() {
        let mut module = make_cse_module();
        // Change second call's argument to 6 (different from 5).
        module.functions[1].blocks[0].instructions[1] = Instruction::Assign {
            dest: LocalId(1),
            value: RValue::Call {
                func: "double".into(),
                args: vec![Operand::Constant(Constant::Int(6))],
            },
        };

        optimize_pure(&mut module);

        let main = &module.functions[1];
        // Both calls should remain (different arguments).
        assert!(matches!(
            &main.blocks[0].instructions[0],
            Instruction::Assign {
                value: RValue::Call { .. },
                ..
            }
        ));
        assert!(matches!(
            &main.blocks[0].instructions[1],
            Instruction::Assign {
                value: RValue::Call { .. },
                ..
            }
        ));
    }

    #[test]
    fn no_cse_for_non_pure() {
        let mut module = make_cse_module();
        // Remove @pure from `double`.
        module.functions[0].attributes.pure_fn = false;

        optimize_pure(&mut module);

        let main = &module.functions[1];
        // Both calls should remain (not @pure).
        assert!(matches!(
            &main.blocks[0].instructions[1],
            Instruction::Assign {
                value: RValue::Call { .. },
                ..
            }
        ));
    }

    #[test]
    fn dead_pure_call_removed() {
        let mut module = MirModule {
            functions: vec![
                MirFunction {
                    name: "pure_fn".into(),
                    params: vec![],
                    ret_ty: MirType::I64,
                    blocks: vec![BasicBlock {
                        id: BlockId(0),
                        instructions: vec![],
                        terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(42)))),
                    }],
                    locals: vec![],
                    attributes: pure_attrs(),
                    source_file: None,
                    source_line: 0,
                },
                // fn main() { _0 = pure_fn(); return; }
                // _0 is never used — should be removed.
                MirFunction {
                    name: "main".into(),
                    params: vec![],
                    ret_ty: MirType::Void,
                    blocks: vec![BasicBlock {
                        id: BlockId(0),
                        instructions: vec![Instruction::Assign {
                            dest: LocalId(0),
                            value: RValue::Call {
                                func: "pure_fn".into(),
                                args: vec![],
                            },
                        }],
                        terminator: Terminator::Return(None),
                    }],
                    locals: vec![make_local(0)],
                    attributes: MirAttributes::default(),
                    source_file: None,
                    source_line: 0,
                },
            ],
            struct_defs: Default::default(),
            enum_defs: Default::default(),
            trait_vtables: Default::default(),
            copy_structs: Default::default(),
        };

        optimize_pure(&mut module);

        assert!(
            module.functions[1].blocks[0].instructions.is_empty(),
            "dead call to @pure function should be removed"
        );
    }

    #[test]
    fn dead_non_pure_call_kept() {
        let mut module = MirModule {
            functions: vec![
                MirFunction {
                    name: "impure_fn".into(),
                    params: vec![],
                    ret_ty: MirType::I64,
                    blocks: vec![BasicBlock {
                        id: BlockId(0),
                        instructions: vec![],
                        terminator: Terminator::Return(Some(Operand::Constant(Constant::Int(42)))),
                    }],
                    locals: vec![],
                    attributes: MirAttributes::default(), // NOT pure,
                    source_file: None,
                    source_line: 0,
                },
                MirFunction {
                    name: "main".into(),
                    params: vec![],
                    ret_ty: MirType::Void,
                    blocks: vec![BasicBlock {
                        id: BlockId(0),
                        instructions: vec![Instruction::Assign {
                            dest: LocalId(0),
                            value: RValue::Call {
                                func: "impure_fn".into(),
                                args: vec![],
                            },
                        }],
                        terminator: Terminator::Return(None),
                    }],
                    locals: vec![make_local(0)],
                    attributes: MirAttributes::default(),
                    source_file: None,
                    source_line: 0,
                },
            ],
            struct_defs: Default::default(),
            enum_defs: Default::default(),
            trait_vtables: Default::default(),
            copy_structs: Default::default(),
        };

        optimize_pure(&mut module);

        // Non-pure function call should NOT be removed even if result is unused.
        assert_eq!(module.functions[1].blocks[0].instructions.len(), 1);
    }

    #[test]
    fn empty_module_no_panic() {
        let mut module = MirModule {
            functions: vec![],
            struct_defs: Default::default(),
            enum_defs: Default::default(),
            trait_vtables: Default::default(),
            copy_structs: Default::default(),
        };
        optimize_pure(&mut module);
    }
}
