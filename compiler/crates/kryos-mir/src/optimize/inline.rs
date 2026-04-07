//! Function inlining pass.
//!
//! Inlines small, non-recursive leaf functions at their call sites.  A leaf
//! function is one that contains no `Call` or `CallIndirect` RValues.
//!
//! For robustness this pass handles only the simplest case:
//! - Single basic block callee (no branches).
//! - The callee is a leaf (no calls inside it).
//! - The callee has fewer than `threshold` instructions.
//! - The callee is not marked `@noinline` (not yet defined, but future-proof).
//! - Functions marked `@inline` bypass the threshold check.
//!
//! At each qualifying call site the inliner:
//! 1. Allocates fresh `LocalId`s for the callee's locals (offset by caller count).
//! 2. Emits assignments from arguments to the callee's parameter locals.
//! 3. Copies the callee's instructions with remapped locals.
//! 4. Replaces the call's result with the callee's return value local.

use std::collections::HashMap;

use crate::ir::{
    Instruction, LocalId, MirLocal, MirModule,
    Operand, RValue, Terminator,
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Inline small leaf functions across the entire module.
///
/// `threshold` is the maximum number of instructions a function may have to
/// be considered for inlining (functions with `@inline` ignore this limit).
pub fn inline_functions(module: &mut MirModule, threshold: usize) {
    // Phase 1: Identify inline candidates.
    let candidates = find_candidates(module, threshold);

    if candidates.is_empty() {
        return;
    }

    // Phase 2: For each function, scan call sites and inline candidates.
    // We work on function indices to satisfy the borrow checker.
    let func_count = module.functions.len();
    for fi in 0..func_count {
        inline_calls_in_function(module, fi, &candidates);
    }
}

// ---------------------------------------------------------------------------
// Candidate discovery
// ---------------------------------------------------------------------------

/// Information about an inlinable function.
#[derive(Clone)]
struct InlineCandidate {
    /// The callee's single basic block instructions (cloned for inlining).
    instructions: Vec<Instruction>,
    /// The callee's locals (for cloning).
    locals: Vec<MirLocal>,
    /// Parameter locals (indices into `locals`).
    param_locals: Vec<LocalId>,
    /// The return operand from the callee's terminator (if any).
    ret_operand: Option<Operand>,
}

/// Find functions eligible for inlining.
fn find_candidates(module: &MirModule, threshold: usize) -> HashMap<String, InlineCandidate> {
    let mut candidates = HashMap::new();

    for func in &module.functions {
        // Skip multi-block functions — only inline single-block callees.
        if func.blocks.len() != 1 {
            continue;
        }

        let block = &func.blocks[0];

        // Check instruction count vs threshold (unless @inline is set).
        if !func.attributes.inline && block.instructions.len() > threshold {
            continue;
        }

        // Skip non-leaf functions (functions that contain calls).
        if contains_call(&block.instructions) {
            continue;
        }

        // Extract return operand from the terminator.
        let ret_operand = match &block.terminator {
            Terminator::Return(op) => op.clone(),
            // If the single block doesn't return, skip (shouldn't happen for
            // well-formed functions, but be safe).
            _ => continue,
        };

        let param_locals: Vec<LocalId> = func.params.iter().map(|p| p.local).collect();

        candidates.insert(
            func.name.clone(),
            InlineCandidate {
                instructions: block.instructions.clone(),
                locals: func.locals.clone(),
                param_locals,
                ret_operand,
            },
        );
    }

    candidates
}

/// Returns `true` if any instruction contains a function call.
fn contains_call(instructions: &[Instruction]) -> bool {
    instructions.iter().any(|inst| match inst {
        Instruction::Assign {
            value: RValue::Call { .. } | RValue::CallIndirect { .. } | RValue::VtableCall { .. },
            ..
        } => true,
        Instruction::Spawn { .. } => true,
        _ => false,
    })
}

// ---------------------------------------------------------------------------
// Inlining
// ---------------------------------------------------------------------------

/// Scan a single function for call sites that can be inlined and perform
/// the inlining.
fn inline_calls_in_function(
    module: &mut MirModule,
    func_idx: usize,
    candidates: &HashMap<String, InlineCandidate>,
) {
    // We iterate block by block, instruction by instruction.  When we find
    // a call site to inline, we replace it with the callee's instructions.
    // Because we're expanding in-place, we process one block at a time and
    // rebuild its instruction list.

    let block_count = module.functions[func_idx].blocks.len();
    for bi in 0..block_count {
        let mut new_instructions: Vec<Instruction> = Vec::new();
        let old_instructions =
            std::mem::take(&mut module.functions[func_idx].blocks[bi].instructions);

        for inst in old_instructions {
            if let Instruction::Assign {
                dest,
                value: RValue::Call { ref func, ref args },
            } = inst
            {
                // Don't inline a function into itself (prevent trivial infinite expansion).
                if *func == module.functions[func_idx].name {
                    new_instructions.push(inst);
                    continue;
                }

                if let Some(candidate) = candidates.get(func.as_str()) {
                    // Inline this call site.
                    let local_offset = module.functions[func_idx].locals.len() as u32;

                    // Allocate new locals for the callee's locals.
                    for callee_local in &candidate.locals {
                        module.functions[func_idx].locals.push(MirLocal {
                            id: LocalId(callee_local.id.0 + local_offset),
                            name: callee_local.name.clone(),
                            ty: callee_local.ty.clone(),
                            mutable: callee_local.mutable,
                        });
                    }

                    // Assign arguments to parameter locals.
                    for (param_local, arg) in
                        candidate.param_locals.iter().zip(args.iter())
                    {
                        new_instructions.push(Instruction::Assign {
                            dest: LocalId(param_local.0 + local_offset),
                            value: RValue::Use(arg.clone()),
                        });
                    }

                    // Copy callee instructions with remapped locals.
                    for callee_inst in &candidate.instructions {
                        new_instructions
                            .push(remap_instruction(callee_inst, local_offset));
                    }

                    // Assign the callee's return value to the call's destination.
                    if let Some(ref ret_op) = candidate.ret_operand {
                        new_instructions.push(Instruction::Assign {
                            dest,
                            value: RValue::Use(remap_operand(ret_op, local_offset)),
                        });
                    }

                    continue; // skip pushing original call
                }
            }

            // Not a call or not a candidate — keep original instruction.
            new_instructions.push(inst);
        }

        module.functions[func_idx].blocks[bi].instructions = new_instructions;
    }
}

// ---------------------------------------------------------------------------
// Local remapping
// ---------------------------------------------------------------------------

fn remap_instruction(inst: &Instruction, offset: u32) -> Instruction {
    match inst {
        Instruction::Assign { dest, value } => Instruction::Assign {
            dest: LocalId(dest.0 + offset),
            value: remap_rvalue(value, offset),
        },
        Instruction::ArcRetain { ptr } => Instruction::ArcRetain {
            ptr: LocalId(ptr.0 + offset),
        },
        Instruction::ArcRelease { ptr } => Instruction::ArcRelease {
            ptr: LocalId(ptr.0 + offset),
        },
        Instruction::Drop { local } => Instruction::Drop {
            local: LocalId(local.0 + offset),
        },
        Instruction::Send { channel, value } => Instruction::Send {
            channel: LocalId(channel.0 + offset),
            value: LocalId(value.0 + offset),
        },
        Instruction::Receive { dest, channel } => Instruction::Receive {
            dest: LocalId(dest.0 + offset),
            channel: LocalId(channel.0 + offset),
        },
        Instruction::Spawn { func, args } => Instruction::Spawn {
            func: func.clone(),
            args: args.iter().map(|a| remap_operand(a, offset)).collect(),
        },
        Instruction::ActorSpawn {
            dest,
            dispatch_fn,
            state,
        } => Instruction::ActorSpawn {
            dest: LocalId(dest.0 + offset),
            dispatch_fn: dispatch_fn.clone(),
            state: remap_operand(state, offset),
        },
        Instruction::ActorSend {
            actor,
            handler_tag,
            args,
        } => Instruction::ActorSend {
            actor: LocalId(actor.0 + offset),
            handler_tag: *handler_tag,
            args: args.iter().map(|a| remap_operand(a, offset)).collect(),
        },
        Instruction::ActorStateLoad {
            dest,
            state_ptr,
            field_offset,
        } => Instruction::ActorStateLoad {
            dest: LocalId(dest.0 + offset),
            state_ptr: LocalId(state_ptr.0 + offset),
            field_offset: *field_offset,
        },
        Instruction::ActorStateStore {
            state_ptr,
            field_offset,
            value,
        } => Instruction::ActorStateStore {
            state_ptr: LocalId(state_ptr.0 + offset),
            field_offset: *field_offset,
            value: remap_operand(value, offset),
        },
        Instruction::StoreDeref { ptr, value } => Instruction::StoreDeref {
            ptr: remap_operand(ptr, offset),
            value: remap_operand(value, offset),
        },
        Instruction::Nop => Instruction::Nop,
    }
}

fn remap_rvalue(rv: &RValue, offset: u32) -> RValue {
    match rv {
        RValue::Use(op) => RValue::Use(remap_operand(op, offset)),
        RValue::BinOp { op, left, right } => RValue::BinOp {
            op: *op,
            left: remap_operand(left, offset),
            right: remap_operand(right, offset),
        },
        RValue::UnOp { op, operand } => RValue::UnOp {
            op: *op,
            operand: remap_operand(operand, offset),
        },
        RValue::Call { func, args } => RValue::Call {
            func: func.clone(),
            args: args.iter().map(|a| remap_operand(a, offset)).collect(),
        },
        RValue::CallIndirect { callee, args } => RValue::CallIndirect {
            callee: remap_operand(callee, offset),
            args: args.iter().map(|a| remap_operand(a, offset)).collect(),
        },
        RValue::ConstInt(v) => RValue::ConstInt(*v),
        RValue::ConstFloat(v) => RValue::ConstFloat(*v),
        RValue::ConstBool(v) => RValue::ConstBool(*v),
        RValue::ConstString(s) => RValue::ConstString(s.clone()),
        RValue::ConstNone => RValue::ConstNone,
        RValue::Array(elems) => {
            RValue::Array(elems.iter().map(|e| remap_operand(e, offset)).collect())
        }
        RValue::Tuple(elems) => {
            RValue::Tuple(elems.iter().map(|e| remap_operand(e, offset)).collect())
        }
        RValue::Struct { name, fields } => RValue::Struct {
            name: name.clone(),
            fields: fields
                .iter()
                .map(|(n, op)| (n.clone(), remap_operand(op, offset)))
                .collect(),
        },
        RValue::Field { object, field } => RValue::Field {
            object: remap_operand(object, offset),
            field: field.clone(),
        },
        RValue::Index { object, index } => RValue::Index {
            object: remap_operand(object, offset),
            index: remap_operand(index, offset),
        },
        RValue::ArcAlloc { inner } => RValue::ArcAlloc {
            inner: remap_operand(inner, offset),
        },
        RValue::Cast { operand, ty } => RValue::Cast {
            operand: remap_operand(operand, offset),
            ty: ty.clone(),
        },
        RValue::EnumVariant {
            enum_name,
            variant_idx,
            fields,
        } => RValue::EnumVariant {
            enum_name: enum_name.clone(),
            variant_idx: *variant_idx,
            fields: fields.iter().map(|f| remap_operand(f, offset)).collect(),
        },
        RValue::EnumTag { operand } => RValue::EnumTag {
            operand: remap_operand(operand, offset),
        },
        RValue::EnumPayload {
            operand,
            variant_idx,
            field_idx,
        } => RValue::EnumPayload {
            operand: remap_operand(operand, offset),
            variant_idx: *variant_idx,
            field_idx: *field_idx,
        },
        RValue::Closure {
            func_name,
            captures,
        } => RValue::Closure {
            func_name: func_name.clone(),
            captures: captures.iter().map(|c| remap_operand(c, offset)).collect(),
        },
        RValue::Map(pairs) => RValue::Map(
            pairs
                .iter()
                .map(|(k, v)| (remap_operand(k, offset), remap_operand(v, offset)))
                .collect(),
        ),
        RValue::StringConcat(parts) => {
            RValue::StringConcat(parts.iter().map(|p| remap_operand(p, offset)).collect())
        }
        RValue::Range {
            start,
            end,
            inclusive,
        } => RValue::Range {
            start: start.as_ref().map(|s| remap_operand(s, offset)),
            end: end.as_ref().map(|e| remap_operand(e, offset)),
            inclusive: *inclusive,
        },
        RValue::AddrOf { local, mutable } => RValue::AddrOf {
            local: LocalId(local.0 + offset),
            mutable: *mutable,
        },
        RValue::Deref { operand } => RValue::Deref {
            operand: remap_operand(operand, offset),
        },
        RValue::Comptime(inner) => RValue::Comptime(Box::new(remap_rvalue(inner, offset))),
        RValue::MakeTraitObject {
            value,
            concrete_type,
            trait_name,
        } => RValue::MakeTraitObject {
            value: remap_operand(value, offset),
            concrete_type: concrete_type.clone(),
            trait_name: trait_name.clone(),
        },
        RValue::VtableCall {
            object,
            method_index,
            args,
        } => RValue::VtableCall {
            object: remap_operand(object, offset),
            method_index: *method_index,
            args: args.iter().map(|a| remap_operand(a, offset)).collect(),
        },
    }
}

fn remap_operand(op: &Operand, offset: u32) -> Operand {
    match op {
        Operand::Local(id) => Operand::Local(LocalId(id.0 + offset)),
        Operand::Constant(c) => Operand::Constant(c.clone()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::*;

    fn make_local(id: u32, name: &str, ty: MirType) -> MirLocal {
        MirLocal {
            id: LocalId(id),
            name: Some(name.into()),
            ty,
            mutable: false,
        }
    }

    /// Build a simple module where `add_one(x)` returns `x + 1` and `main`
    /// calls `add_one(41)`.
    fn make_inline_test_module() -> MirModule {
        MirModule {
            functions: vec![
                // fn add_one(x: i64) -> i64 { return x + 1 }
                MirFunction {
                    name: "add_one".into(),
                    params: vec![crate::ir::MirParam {
                        local: LocalId(0),
                        ty: MirType::I64,
                    }],
                    ret_ty: MirType::I64,
                    blocks: vec![BasicBlock {
                        id: BlockId(0),
                        instructions: vec![Instruction::Assign {
                            dest: LocalId(1),
                            value: RValue::BinOp {
                                op: crate::ir::MirBinOp::Add,
                                left: Operand::Local(LocalId(0)),
                                right: Operand::Constant(Constant::Int(1)),
                            },
                        }],
                        terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
                    }],
                    locals: vec![
                        make_local(0, "x", MirType::I64),
                        make_local(1, "_tmp", MirType::I64),
                    ],
                    attributes: MirAttributes::default(),
                },
                // fn main() -> i64 { return add_one(41) }
                MirFunction {
                    name: "main".into(),
                    params: vec![],
                    ret_ty: MirType::I64,
                    blocks: vec![BasicBlock {
                        id: BlockId(0),
                        instructions: vec![Instruction::Assign {
                            dest: LocalId(0),
                            value: RValue::Call {
                                func: "add_one".into(),
                                args: vec![Operand::Constant(Constant::Int(41))],
                            },
                        }],
                        terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
                    }],
                    locals: vec![make_local(0, "result", MirType::I64)],
                    attributes: MirAttributes::default(),
                },
            ],
            struct_defs: Default::default(),
            enum_defs: Default::default(),
            trait_vtables: Default::default(),
        }
    }

    #[test]
    fn inline_simple_leaf_function() {
        let mut module = make_inline_test_module();
        inline_functions(&mut module, 20);

        let main = &module.functions[1];

        // The call should have been replaced by parameter assignment +
        // inlined body + result copy.
        assert!(
            main.blocks[0].instructions.len() >= 3,
            "expected at least 3 instructions after inlining, got {}",
            main.blocks[0].instructions.len()
        );

        // The call RValue should no longer be present.
        let has_call = main.blocks[0].instructions.iter().any(|inst| {
            matches!(
                inst,
                Instruction::Assign {
                    value: RValue::Call { .. },
                    ..
                }
            )
        });
        assert!(
            !has_call,
            "call to add_one should have been inlined away"
        );
    }

    #[test]
    fn do_not_inline_recursive() {
        // A function that calls itself should not be inlined.
        let mut module = MirModule {
            functions: vec![MirFunction {
                name: "rec".into(),
                params: vec![],
                ret_ty: MirType::I64,
                blocks: vec![BasicBlock {
                    id: BlockId(0),
                    instructions: vec![Instruction::Assign {
                        dest: LocalId(0),
                        value: RValue::Call {
                            func: "rec".into(),
                            args: vec![],
                        },
                    }],
                    terminator: Terminator::Return(Some(Operand::Local(LocalId(0)))),
                }],
                locals: vec![make_local(0, "r", MirType::I64)],
                attributes: MirAttributes::default(),
            }],
            struct_defs: Default::default(),
            enum_defs: Default::default(),
            trait_vtables: Default::default(),
        };

        inline_functions(&mut module, 20);

        // The call should still be there (contains_call makes it non-candidate,
        // plus self-call guard).
        let has_call = module.functions[0].blocks[0]
            .instructions
            .iter()
            .any(|inst| {
                matches!(
                    inst,
                    Instruction::Assign {
                        value: RValue::Call { .. },
                        ..
                    }
                )
            });
        assert!(has_call, "recursive function should not be inlined");
    }

    #[test]
    fn do_not_inline_over_threshold() {
        let mut module = make_inline_test_module();
        // Threshold 0 means nothing qualifies (unless @inline).
        inline_functions(&mut module, 0);

        let has_call = module.functions[1].blocks[0]
            .instructions
            .iter()
            .any(|inst| {
                matches!(
                    inst,
                    Instruction::Assign {
                        value: RValue::Call { .. },
                        ..
                    }
                )
            });
        assert!(has_call, "should not inline when over threshold");
    }

    #[test]
    fn force_inline_with_attribute() {
        let mut module = make_inline_test_module();
        // Mark add_one as @inline.
        module.functions[0].attributes.inline = true;
        // Threshold 0 normally blocks inlining, but @inline overrides.
        inline_functions(&mut module, 0);

        let has_call = module.functions[1].blocks[0]
            .instructions
            .iter()
            .any(|inst| {
                matches!(
                    inst,
                    Instruction::Assign {
                        value: RValue::Call { .. },
                        ..
                    }
                )
            });
        assert!(
            !has_call,
            "@inline should bypass threshold and force inlining"
        );
    }

    #[test]
    fn empty_module_no_panic() {
        let mut module = MirModule {
            functions: vec![],
            struct_defs: Default::default(),
            enum_defs: Default::default(),
            trait_vtables: Default::default(),
        };
        inline_functions(&mut module, 20);
    }
}
