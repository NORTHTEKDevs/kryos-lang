//! Async state-machine lowering for Kryos MIR.
//!
//! This pass identifies functions declared `async fn` (i.e. those whose
//! `MirAttributes::is_async` is `true`) and prepares them for codegen as
//! pollable state machines compatible with the runtime ABI declared in
//! [`kryos_rt::future`].
//!
//! # ABI recap
//!
//! Every async function `F` lowers to two artifacts:
//!
//! 1. **State struct** `__kryos_state_<F>` containing:
//!    - `state: i32` discriminant (0 = initial; >0 = post-await; -1 = done)
//!    - One field per captured local that must survive an await
//!    - A `result` field holding the function's i64 return value once
//!      the machine reaches the terminal state.
//!
//! 2. **Poll function** `__kryos_poll_<F>(state: *mut u8) -> i64` whose
//!    return value is `KRYOS_PENDING (0)` or `KRYOS_READY (1)`.
//!
//! # MVP scope
//!
//! Real CPS-style split-at-await rewriting on the CFG is a substantial
//! transform: it requires liveness analysis to know which locals to
//! save into the state struct, control-flow re-stitching at each await
//! point, and per-state entry-block synthesis. That work is sequenced
//! for a follow-up commit.
//!
//! This MVP performs three concrete and testable steps:
//!
//! 1. **Validation** \u2014 walk each async function's MIR and confirm it is
//!    well-formed for the upcoming codegen change. The current AST\u2192MIR
//!    lowering eagerly inlines `await` into a direct call, so any MIR
//!    instruction that retains await-shape is a bug we want to catch.
//!
//! 2. **Metadata stamping** \u2014 populate
//!    [`MirAttributes::is_async`] (already done in the AST\u2192MIR step) and
//!    emit a deterministic `state_struct_name` / `poll_fn_name` pair
//!    derived from the function name. These names follow the
//!    `__kryos_state_<F>` / `__kryos_poll_<F>` convention used by the
//!    runtime so codegen and the runtime agree without further glue.
//!
//! 3. **Plan extraction** \u2014 for each async function compute an
//!    [`AsyncPlan`] describing the set of locals that would need to be
//!    persisted across await points. For the MVP this is conservatively
//!    "all named, non-parameter locals" \u2014 a future commit will refine
//!    this with proper liveness.
//!
//! The pass is **non-destructive**: it does not mutate the function CFG.
//! Codegen consumes the [`AsyncLoweringReport`] returned by [`run`] and
//! emits the wrapper poll function at backend lowering time.

use crate::ir::{
    BasicBlock, BlockId, Constant, Instruction, LocalId, MirAttributes, MirFunction, MirLocal,
    MirModule, MirType, Operand, RValue, Terminator,
};
use crate::liveness;
use std::collections::{BTreeMap, BTreeSet};

/// Per-function plan produced by the async lowering pass.
#[derive(Debug, Clone)]
pub struct AsyncPlan {
    /// Source function name (e.g. `download_chunk`).
    pub source_name: String,
    /// Name of the generated state struct
    /// (e.g. `__kryos_state_download_chunk`).
    pub state_struct_name: String,
    /// Name of the generated poll function
    /// (e.g. `__kryos_poll_download_chunk`).
    pub poll_fn_name: String,
    /// Locals to persist across await points. For the MVP this is
    /// every named local (parameters included \u2014 since parameters are
    /// passed by value to a poll function, they must live on the state
    /// struct). Future commits will narrow this with liveness.
    pub captured_locals: Vec<CapturedLocal>,
    /// MIR type of the function's return value, which becomes the
    /// `result` field of the state struct. `Void` is represented as
    /// `MirType::I64` returning 0 \u2014 the runtime ABI is single-i64.
    pub result_ty: MirType,
}

/// A local that the async lowering plan will persist in the state
/// struct.
#[derive(Debug, Clone)]
pub struct CapturedLocal {
    pub id: LocalId,
    pub name: String,
    pub ty: MirType,
    pub is_param: bool,
}

/// Combined report from running the pass on a module.
#[derive(Debug, Clone, Default)]
pub struct AsyncLoweringReport {
    /// One [`AsyncPlan`] per async function, keyed by source name in
    /// declaration order.
    pub plans: BTreeMap<String, AsyncPlan>,
    /// Validation errors. The pass is conservative: anything it cannot
    /// confidently lower today is reported here so the driver can
    /// either skip codegen for that function or fail loudly.
    pub errors: Vec<AsyncLoweringError>,
}

/// A validation error attached to a specific async function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsyncLoweringError {
    pub function: String,
    pub message: String,
}

/// Run the async lowering pass on a module. Returns a report describing
/// each async function's planned poll-fn / state-struct shape and any
/// validation errors.
///
/// The pass is non-destructive \u2014 it does not mutate `module`.
pub fn run(module: &MirModule) -> AsyncLoweringReport {
    let mut report = AsyncLoweringReport::default();

    for func in &module.functions {
        if !func.attributes.is_async {
            continue;
        }
        match analyse_function(func) {
            Ok(plan) => {
                report.plans.insert(plan.source_name.clone(), plan);
            }
            Err(err) => report.errors.push(err),
        }
    }

    report
}

/// Public helper: derive the deterministic poll-fn name for an async
/// source function. Codegen uses this when emitting calls so it can
/// stay in lock-step with the lowering pass.
pub fn poll_fn_name_for(source_name: &str) -> String {
    format!("__kryos_poll_{source_name}")
}

/// Public helper: derive the deterministic state-struct name for an
/// async source function.
pub fn state_struct_name_for(source_name: &str) -> String {
    format!("__kryos_state_{source_name}")
}

/// Public helper: is this an async-machine artifact name generated by
/// the pass? Useful for symbol filtering / linker hooks.
pub fn is_async_artifact_name(name: &str) -> bool {
    name.starts_with("__kryos_poll_") || name.starts_with("__kryos_state_")
}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

fn analyse_function(func: &MirFunction) -> Result<AsyncPlan, AsyncLoweringError> {
    debug_assert!(func.attributes.is_async);

    // Sanity: an async function cannot also be marked `@inline` \u2014 the
    // inliner would inline the (synchronous!) body across an await
    // point, which is observably wrong once split-at-await lands.
    if func.attributes.inline {
        return Err(AsyncLoweringError {
            function: func.name.clone(),
            message: "async fn cannot also be @inline".to_string(),
        });
    }

    // Collect captures.
    let mut captured_locals = Vec::new();
    let param_ids: std::collections::HashSet<u32> =
        func.params.iter().map(|p| p.local.0).collect();
    for local in &func.locals {
        let Some(name) = local.name.clone() else {
            continue;
        };
        let is_param = param_ids.contains(&local.id.0);
        captured_locals.push(CapturedLocal {
            id: local.id,
            name,
            ty: local.ty.clone(),
            is_param,
        });
    }

    // Result type: the runtime ABI returns i64. We preserve the source
    // return type for future codegen but normalise Void \u2192 I64-zero.
    let result_ty = match &func.ret_ty {
        MirType::Void => MirType::I64,
        other => other.clone(),
    };

    Ok(AsyncPlan {
        source_name: func.name.clone(),
        state_struct_name: state_struct_name_for(&func.name),
        poll_fn_name: poll_fn_name_for(&func.name),
        captured_locals,
        result_ty,
    })
}

// ---------------------------------------------------------------------------
// Mutation API
// ---------------------------------------------------------------------------

/// Apply the plan to a module by inserting the synthesised state-struct
/// type into the module's `struct_defs` map. Codegen later sees these
/// as regular structs.
///
/// Returns the number of structs inserted.
pub fn apply_state_structs(module: &mut MirModule, report: &AsyncLoweringReport) -> usize {
    let mut inserted = 0;
    for plan in report.plans.values() {
        let mut fields: Vec<(String, MirType)> = Vec::new();
        fields.push(("state".to_string(), MirType::I32));
        for cap in &plan.captured_locals {
            fields.push((cap.name.clone(), cap.ty.clone()));
        }
        fields.push(("result".to_string(), plan.result_ty.clone()));
        module
            .struct_defs
            .insert(plan.state_struct_name.clone(), fields);
        inserted += 1;
    }
    inserted
}

// ---------------------------------------------------------------------------
// Split-at-await CFG transform
// ---------------------------------------------------------------------------

/// Specification for one suspension (await) point inside a function.
///
/// The transform reads the instruction at `inst_idx` of `block` as "the
/// await". After the transform, the function will yield (`Return(0)`)
/// immediately after that instruction has run, with all locals live across
/// the suspension persisted into the state struct. The next call to the
/// poll function resumes execution starting from the instruction *after*
/// the await.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitPoint {
    pub block: BlockId,
    pub inst_idx: usize,
}

/// Errors that can arise during `split_at_await`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplitError {
    /// The block id in a split point is out of range.
    BadBlock(BlockId),
    /// The instruction index in a split point is out of range for its block.
    BadInstIdx { block: BlockId, idx: usize },
    /// Two split points landed in the same block — currently we require
    /// each block to be split at most once (we always split the *post*
    /// half into a fresh block, so per-split this is sufficient even for
    /// real programs as long as callers feed splits left-to-right).
    DuplicateBlock(BlockId),
    /// The function had no parameter whose type is a pointer to the state
    /// struct. We require parameter `_0` to be the state pointer.
    MissingStatePtrParam,
}

/// Result of [`split_at_await`].
#[derive(Debug, Clone)]
pub struct SplitOutcome {
    /// New blocks created by the transform, in the order they were
    /// produced. Each entry is the `BlockId` of the freshly-inserted
    /// "post-await" entry block — the resume target for that split.
    pub resume_blocks: Vec<BlockId>,
    /// Locals that were persisted to the state struct, in the order they
    /// were captured. One entry per split point; each entry is the list
    /// of locals saved for that split.
    pub saved_locals: Vec<Vec<LocalId>>,
}

/// Apply split-at-await to `func` at the given suspension points.
///
/// **Pre-conditions:**
/// - `func.attributes.is_async` must be true.
/// - Parameter `0` must be the state pointer (caller is responsible for
///   having set this up; the async lowering driver does this when it
///   wires the function for codegen).
/// - `state_struct_name` is the canonical name from
///   [`state_struct_name_for`]; all `StoreField`/`Field` ops emitted by
///   the transform use this struct's fields by name.
///
/// **Effect on the CFG:**
/// - The original entry block is renumbered: a freshly-inserted block 0
///   becomes the new entry and contains a `Switch` on the state
///   discriminant. State `0` jumps to the original entry; state `i` jumps
///   to the resume block for the i-th split point.
/// - For each split point `(B, k)`:
///   - Instructions `B.instructions[k+1..]` and `B.terminator` are moved
///     into a brand-new "resume" block `R`.
///   - For every local live immediately after instruction `k`, a
///     `StoreField { object: state_ptr, field: <local_name>, value: Local(l) }`
///     is appended to the pre-half of `B`.
///   - A `StoreField` of `state := i+1` is appended.
///   - `B`'s terminator becomes `Return(Constant::Int(0))` — KRYOS_PENDING.
///   - The first thing `R` does (before any of the moved instructions)
///     is reload each persisted local via
///     `Assign { dest: Local(l), value: Field { object: state_ptr, field: <name> } }`.
///
/// The transform leaves any blocks not mentioned in `splits` untouched.
pub fn split_at_await(
    func: &mut MirFunction,
    state_struct_name: &str,
    splits: &[SplitPoint],
) -> Result<SplitOutcome, SplitError> {
    if splits.is_empty() {
        return Ok(SplitOutcome {
            resume_blocks: vec![],
            saved_locals: vec![],
        });
    }

    // Validate split points and ensure no duplicates in the same block.
    {
        let mut seen: BTreeSet<BlockId> = BTreeSet::new();
        for sp in splits {
            if (sp.block.0 as usize) >= func.blocks.len() {
                return Err(SplitError::BadBlock(sp.block));
            }
            let blk = &func.blocks[sp.block.0 as usize];
            if sp.inst_idx >= blk.instructions.len() {
                return Err(SplitError::BadInstIdx {
                    block: sp.block,
                    idx: sp.inst_idx,
                });
            }
            if !seen.insert(sp.block) {
                return Err(SplitError::DuplicateBlock(sp.block));
            }
        }
    }

    // Identify the state-pointer parameter (always parameter 0 by
    // convention). This local will be referenced by every persist /
    // restore instruction the transform emits.
    let state_ptr_local = func
        .params
        .first()
        .map(|p| p.local)
        .ok_or(SplitError::MissingStatePtrParam)?;

    // 1) Compute liveness on the *pre-transform* CFG. The transform only
    //    splits blocks and adds new ones — the liveness of program points
    //    *strictly before* the split instruction is unaffected, and
    //    "live immediately after the split" is exactly the set of locals
    //    we need to persist.
    let live = liveness::analyze(func);

    // Build a quick local-id -> name map for the captured-local saves.
    let local_name = |id: LocalId, locals: &[MirLocal]| -> Option<String> {
        locals.iter().find(|l| l.id == id).and_then(|l| l.name.clone())
    };

    let mut resume_blocks: Vec<BlockId> = Vec::with_capacity(splits.len());
    let mut saved_locals_per_split: Vec<Vec<LocalId>> = Vec::with_capacity(splits.len());

    // 2) For each split, perform the slice + persist + resume.
    for (split_index, sp) in splits.iter().enumerate() {
        // Locals live immediately after the split instruction — the set
        // that must survive the suspension. We exclude the state pointer
        // (it's the parameter, always reloaded by the runtime via the
        // poll-fn argument), and we exclude any local that has no name
        // (unnameable temporaries cannot become struct fields).
        let live_after: Vec<LocalId> = live
            .live_after_instruction(func, sp.block, sp.inst_idx)
            .into_iter()
            .filter(|id| *id != state_ptr_local)
            .filter(|id| local_name(*id, &func.locals).is_some())
            .collect();

        // Slice the block. We take ownership of the post-half so we can
        // place it into a brand-new block.
        let new_block_id = BlockId(func.blocks.len() as u32);
        let pre_block_idx = sp.block.0 as usize;
        let (post_instructions, original_terminator) = {
            let blk = &mut func.blocks[pre_block_idx];
            // Drain everything after the split index.
            let post_instrs: Vec<Instruction> =
                blk.instructions.drain(sp.inst_idx + 1..).collect();
            // Steal the terminator and replace with the suspension return.
            let old_term = std::mem::replace(
                &mut blk.terminator,
                Terminator::Return(Some(Operand::Constant(Constant::Int(0)))),
            );
            (post_instrs, old_term)
        };

        // Append persist instructions to the pre-half: state = i+1 first,
        // then one StoreField per captured local. We choose i+1 because
        // state == 0 is reserved for "initial entry".
        let state_id = (split_index as i64) + 1;
        let blk = &mut func.blocks[pre_block_idx];
        blk.instructions.push(Instruction::StoreField {
            object: Operand::Local(state_ptr_local),
            field: "state".to_string(),
            value: Operand::Constant(Constant::Int(state_id)),
        });
        for &lid in &live_after {
            let name = local_name(lid, &func.locals).expect("filtered above");
            blk.instructions.push(Instruction::StoreField {
                object: Operand::Local(state_ptr_local),
                field: name,
                value: Operand::Local(lid),
            });
        }

        // Build the resume block: reload locals, then run the moved
        // instructions, ending in the original terminator.
        let mut resume_instrs: Vec<Instruction> = Vec::with_capacity(
            live_after.len() + post_instructions.len(),
        );
        for &lid in &live_after {
            let name = local_name(lid, &func.locals).expect("filtered above");
            resume_instrs.push(Instruction::Assign {
                dest: lid,
                value: RValue::Field {
                    object: Operand::Local(state_ptr_local),
                    field: name,
                },
            });
        }
        resume_instrs.extend(post_instructions);

        func.blocks.push(BasicBlock {
            id: new_block_id,
            instructions: resume_instrs,
            terminator: original_terminator,
        });

        resume_blocks.push(new_block_id);
        saved_locals_per_split.push(live_after);
    }

    // 3) Insert a new entry block that switches on the state
    //    discriminant. We rotate the blocks vector so that the new
    //    dispatch block ends up at index 0 and the previous entry is now
    //    at a non-zero index. BlockId.0 still matches each block's
    //    position in the vector after rotation, so we must rewrite every
    //    terminator and every block id consistently.
    let prev_entry_id = func.blocks[0].id;
    let new_entry_id = BlockId(func.blocks.len() as u32);

    // Switch targets: state value `i+1` → resume_blocks[i], default → previous entry.
    let switch_targets: Vec<(i64, BlockId)> = resume_blocks
        .iter()
        .enumerate()
        .map(|(i, b)| ((i as i64) + 1, *b))
        .collect();

    // We need a fresh local to hold the loaded `state` value.
    let state_value_local = LocalId(func.locals.len() as u32);
    func.locals.push(MirLocal {
        id: state_value_local,
        name: Some(format!("__kryos_state_disc_{state_struct_name}")),
        ty: MirType::I32,
        mutable: false,
    });

    let dispatch_block = BasicBlock {
        id: new_entry_id,
        instructions: vec![Instruction::Assign {
            dest: state_value_local,
            value: RValue::Field {
                object: Operand::Local(state_ptr_local),
                field: "state".to_string(),
            },
        }],
        terminator: Terminator::Switch {
            value: Operand::Local(state_value_local),
            targets: switch_targets,
            default: prev_entry_id,
        },
    };
    func.blocks.push(dispatch_block);

    // Move the new dispatch block to position 0. We swap-remove from the
    // end (cheap) and insert at the front, then re-id all blocks to keep
    // BlockId == index invariant.
    let dispatch = func.blocks.pop().expect("just pushed");
    func.blocks.insert(0, dispatch);

    // Re-id every block to match its new vector index.
    let mut id_remap: BTreeMap<BlockId, BlockId> = BTreeMap::new();
    for (new_idx, blk) in func.blocks.iter_mut().enumerate() {
        let new_id = BlockId(new_idx as u32);
        id_remap.insert(blk.id, new_id);
        blk.id = new_id;
    }

    // Rewrite every terminator to use the new ids.
    for blk in &mut func.blocks {
        rewrite_terminator_ids(&mut blk.terminator, &id_remap);
    }

    // Map old resume-block ids in the caller-facing outcome.
    let resume_blocks = resume_blocks
        .into_iter()
        .map(|b| *id_remap.get(&b).expect("resume block id must remap"))
        .collect();

    Ok(SplitOutcome {
        resume_blocks,
        saved_locals: saved_locals_per_split,
    })
}

fn rewrite_terminator_ids(term: &mut Terminator, map: &BTreeMap<BlockId, BlockId>) {
    let remap = |b: BlockId| *map.get(&b).unwrap_or(&b);
    match term {
        Terminator::Return(_) | Terminator::Unreachable => {}
        Terminator::Goto(t) => *t = remap(*t),
        Terminator::Branch {
            then_block,
            else_block,
            ..
        } => {
            *then_block = remap(*then_block);
            *else_block = remap(*else_block);
        }
        Terminator::Switch {
            targets, default, ..
        } => {
            for (_, t) in targets.iter_mut() {
                *t = remap(*t);
            }
            *default = remap(*default);
        }
    }
}

/// Convenience: mark each async function with a deterministic suffix so
/// downstream tooling can spot lowered async fns. Idempotent.
pub fn stamp_attributes(module: &mut MirModule) {
    for func in &mut module.functions {
        if !func.attributes.is_async {
            continue;
        }
        // No additional flags to set right now; this is the hook point
        // for future passes that need to record per-function decisions
        // (e.g. "split into N states", "stackless OK", etc.).
        let _: &mut MirAttributes = &mut func.attributes;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        BasicBlock, BlockId, Instruction, MirFunction, MirLocal, MirModule, MirParam, Operand,
        RValue, Terminator,
    };
    use std::collections::HashMap;

    fn empty_module() -> MirModule {
        MirModule {
            functions: vec![],
            struct_defs: HashMap::new(),
            enum_defs: HashMap::new(),
            trait_vtables: HashMap::new(),
            copy_structs: std::collections::HashSet::new(),
        }
    }

    fn dummy_func(name: &str, is_async: bool) -> MirFunction {
        let mut attrs = MirAttributes::default();
        attrs.is_async = is_async;
        MirFunction {
            name: name.into(),
            params: vec![MirParam {
                local: LocalId(0),
                ty: MirType::I64,
            }],
            ret_ty: MirType::I64,
            blocks: vec![BasicBlock {
                id: BlockId(0),
                instructions: vec![Instruction::Assign {
                    dest: LocalId(1),
                    value: RValue::Use(Operand::Local(LocalId(0))),
                }],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(1)))),
            }],
            locals: vec![
                MirLocal {
                    id: LocalId(0),
                    name: Some("x".into()),
                    ty: MirType::I64,
                    mutable: false,
                },
                MirLocal {
                    id: LocalId(1),
                    name: Some("y".into()),
                    ty: MirType::I64,
                    mutable: false,
                },
            ],
            attributes: attrs,
            source_file: None,
            source_line: 0,
        }
    }

    #[test]
    fn non_async_functions_are_ignored() {
        let mut module = empty_module();
        module.functions.push(dummy_func("plain", false));
        let report = run(&module);
        assert!(report.plans.is_empty());
        assert!(report.errors.is_empty());
    }

    #[test]
    fn async_function_produces_plan() {
        let mut module = empty_module();
        module.functions.push(dummy_func("download", true));
        let report = run(&module);
        assert_eq!(report.plans.len(), 1);
        let plan = report.plans.get("download").unwrap();
        assert_eq!(plan.source_name, "download");
        assert_eq!(plan.state_struct_name, "__kryos_state_download");
        assert_eq!(plan.poll_fn_name, "__kryos_poll_download");
        assert_eq!(plan.captured_locals.len(), 2);
        assert_eq!(plan.captured_locals[0].name, "x");
        assert!(plan.captured_locals[0].is_param);
        assert_eq!(plan.captured_locals[1].name, "y");
        assert!(!plan.captured_locals[1].is_param);
        assert_eq!(plan.result_ty, MirType::I64);
    }

    #[test]
    fn async_function_with_void_return_normalises_to_i64() {
        let mut module = empty_module();
        let mut f = dummy_func("notify", true);
        f.ret_ty = MirType::Void;
        module.functions.push(f);
        let report = run(&module);
        assert_eq!(
            report.plans.get("notify").unwrap().result_ty,
            MirType::I64,
            "Void async fn should normalise to i64 for the runtime ABI"
        );
    }

    #[test]
    fn async_plus_inline_is_an_error() {
        let mut module = empty_module();
        let mut f = dummy_func("oops", true);
        f.attributes.inline = true;
        module.functions.push(f);
        let report = run(&module);
        assert!(report.plans.is_empty());
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].function, "oops");
        assert!(report.errors[0].message.contains("inline"));
    }

    #[test]
    fn apply_state_structs_inserts_struct_def() {
        let mut module = empty_module();
        module.functions.push(dummy_func("upload", true));
        let report = run(&module);
        let n = apply_state_structs(&mut module, &report);
        assert_eq!(n, 1);
        let fields = module
            .struct_defs
            .get("__kryos_state_upload")
            .expect("state struct must be inserted");
        // state + 2 captures + result
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0].0, "state");
        assert_eq!(fields[0].1, MirType::I32);
        assert_eq!(fields[1].0, "x");
        assert_eq!(fields[2].0, "y");
        assert_eq!(fields[3].0, "result");
        assert_eq!(fields[3].1, MirType::I64);
    }

    #[test]
    fn apply_state_structs_is_idempotent() {
        let mut module = empty_module();
        module.functions.push(dummy_func("upload", true));
        let report = run(&module);
        let n1 = apply_state_structs(&mut module, &report);
        let n2 = apply_state_structs(&mut module, &report);
        assert_eq!(n1, 1);
        assert_eq!(n2, 1, "re-applying should still insert (overwrite) once");
        assert_eq!(module.struct_defs.len(), 1);
    }

    #[test]
    fn helper_name_functions_are_consistent() {
        assert_eq!(poll_fn_name_for("foo"), "__kryos_poll_foo");
        assert_eq!(state_struct_name_for("foo"), "__kryos_state_foo");
        assert!(is_async_artifact_name("__kryos_poll_foo"));
        assert!(is_async_artifact_name("__kryos_state_foo"));
        assert!(!is_async_artifact_name("foo"));
    }

    #[test]
    fn multiple_async_functions_all_get_plans() {
        let mut module = empty_module();
        module.functions.push(dummy_func("a", true));
        module.functions.push(dummy_func("b", true));
        module.functions.push(dummy_func("c", false));
        module.functions.push(dummy_func("d", true));
        let report = run(&module);
        assert_eq!(report.plans.len(), 3);
        assert!(report.plans.contains_key("a"));
        assert!(report.plans.contains_key("b"));
        assert!(!report.plans.contains_key("c"));
        assert!(report.plans.contains_key("d"));
    }

    #[test]
    fn stamp_attributes_does_not_break_module() {
        let mut module = empty_module();
        module.functions.push(dummy_func("plain", false));
        module.functions.push(dummy_func("async_fn", true));
        let before = module.functions.len();
        stamp_attributes(&mut module);
        assert_eq!(module.functions.len(), before);
    }

    // -----------------------------------------------------------------
    // split_at_await tests
    // -----------------------------------------------------------------
    //
    // Each test builds a tiny MIR function by hand, runs split_at_await
    // with synthetic split points, and asserts on the resulting CFG
    // shape. The transform must:
    //   * insert a dispatch block at index 0,
    //   * make the pre-half end in Return(Const::Int(0)) (PENDING),
    //   * persist live locals via StoreField in the pre-half,
    //   * reload them via Field at the top of the resume block,
    //   * preserve the original terminator at the tail of the resume block.

    use crate::ir::{Constant, MirBinOp};

    fn make_named_local(id: u32, name: &str, ty: MirType) -> MirLocal {
        MirLocal {
            id: LocalId(id),
            name: Some(name.into()),
            ty,
            mutable: false,
        }
    }

    /// Build a function with the shape:
    ///   _0 = state ptr param
    ///   bb0:
    ///     _1 = some_pre()             // produces a value used post-await
    ///     _2 = await_call()           // <-- split point
    ///     _3 = _1 + _2                // uses both pre- and post-await values
    ///     return _3
    fn one_split_func() -> MirFunction {
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
                        value: RValue::Call {
                            func: "some_pre".into(),
                            args: vec![],
                        },
                    },
                    Instruction::Assign {
                        dest: LocalId(2),
                        value: RValue::Call {
                            func: "await_call".into(),
                            args: vec![],
                        },
                    },
                    Instruction::Assign {
                        dest: LocalId(3),
                        value: RValue::BinOp {
                            op: MirBinOp::Add,
                            left: Operand::Local(LocalId(1)),
                            right: Operand::Local(LocalId(2)),
                        },
                    },
                ],
                terminator: Terminator::Return(Some(Operand::Local(LocalId(3)))),
            }],
            locals: vec![
                make_named_local(0, "__state", MirType::I64),
                make_named_local(1, "a", MirType::I64),
                make_named_local(2, "b", MirType::I64),
                make_named_local(3, "c", MirType::I64),
            ],
            attributes: attrs,
            source_file: None,
            source_line: 0,
        }
    }

    #[test]
    fn split_at_await_one_split_basic_shape() {
        let mut f = one_split_func();
        let outcome =
            split_at_await(&mut f, "__kryos_state_demo", &[SplitPoint {
                block: BlockId(0),
                inst_idx: 1,
            }])
            .expect("split_at_await must succeed");

        // Exactly one resume block created.
        assert_eq!(outcome.resume_blocks.len(), 1);
        // _1 should be persisted across the split (live after, used post-await).
        assert!(outcome.saved_locals[0].contains(&LocalId(1)));
        // _0 (state pointer) must never be persisted.
        assert!(!outcome.saved_locals[0].contains(&LocalId(0)));

        // 3 blocks total: dispatch (0), original pre-half (1), resume (2).
        assert_eq!(f.blocks.len(), 3);
        // BlockId.0 == index invariant.
        for (i, b) in f.blocks.iter().enumerate() {
            assert_eq!(b.id.0 as usize, i, "block ids must be contiguous");
        }

        // Dispatch block: ends in a Switch that routes to either the
        // pre-half (default) or the resume block.
        match &f.blocks[0].terminator {
            Terminator::Switch {
                targets, default, ..
            } => {
                assert_eq!(targets.len(), 1);
                assert_eq!(targets[0].0, 1, "resume index 0 must be tagged state=1");
                // Default must point to the pre-half (some non-dispatch block).
                assert_ne!(default.0, 0);
            }
            other => panic!("dispatch block terminator must be Switch, got {other:?}"),
        }

        // Pre-half must end in Return(Const::Int(0)) (KRYOS_PENDING).
        let pre = f
            .blocks
            .iter()
            .find(|b| matches!(
                &b.terminator,
                Terminator::Return(Some(Operand::Constant(Constant::Int(0))))
            ))
            .expect("pre-half must end with Return(0)");

        // Pre-half must contain StoreField{state, 1} and StoreField{a, _1}.
        let mut saw_state = false;
        let mut saw_a = false;
        for inst in &pre.instructions {
            if let Instruction::StoreField { field, value, .. } = inst {
                if field == "state" {
                    assert!(
                        matches!(value, Operand::Constant(Constant::Int(1))),
                        "state must be set to 1 for the first split"
                    );
                    saw_state = true;
                }
                if field == "a" && matches!(value, Operand::Local(LocalId(1))) {
                    saw_a = true;
                }
            }
        }
        assert!(saw_state, "pre-half must persist state discriminant");
        assert!(saw_a, "pre-half must persist local _1 (named 'a')");

        // Resume block must (a) reload `a` via Field, (b) keep the
        // original return terminator returning _3.
        let resume = &f.blocks[outcome.resume_blocks[0].0 as usize];
        let reloads_a = resume.instructions.iter().any(|i| matches!(
            i,
            Instruction::Assign {
                dest: LocalId(1),
                value: RValue::Field { field, .. },
            } if field == "a"
        ));
        assert!(reloads_a, "resume block must reload `a` from state struct");
        assert!(
            matches!(
                &resume.terminator,
                Terminator::Return(Some(Operand::Local(LocalId(3))))
            ),
            "resume block must keep the original return terminator"
        );
    }

    #[test]
    fn split_at_await_two_splits_create_two_resume_blocks() {
        // Two awaits in two different blocks so we don't trip the
        // "DuplicateBlock" guard.
        let mut attrs = MirAttributes::default();
        attrs.is_async = true;
        let mut f = MirFunction {
            name: "twoawait".into(),
            params: vec![MirParam {
                local: LocalId(0),
                ty: MirType::I64,
            }],
            ret_ty: MirType::I64,
            blocks: vec![
                BasicBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Assign {
                            dest: LocalId(1),
                            value: RValue::Call {
                                func: "first_await".into(),
                                args: vec![],
                            },
                        },
                    ],
                    terminator: Terminator::Goto(BlockId(1)),
                },
                BasicBlock {
                    id: BlockId(1),
                    instructions: vec![
                        Instruction::Assign {
                            dest: LocalId(2),
                            value: RValue::Call {
                                func: "second_await".into(),
                                args: vec![Operand::Local(LocalId(1))],
                            },
                        },
                        Instruction::Assign {
                            dest: LocalId(3),
                            value: RValue::BinOp {
                                op: MirBinOp::Add,
                                left: Operand::Local(LocalId(1)),
                                right: Operand::Local(LocalId(2)),
                            },
                        },
                    ],
                    terminator: Terminator::Return(Some(Operand::Local(LocalId(3)))),
                },
            ],
            locals: vec![
                make_named_local(0, "__state", MirType::I64),
                make_named_local(1, "a", MirType::I64),
                make_named_local(2, "b", MirType::I64),
                make_named_local(3, "c", MirType::I64),
            ],
            attributes: attrs,
            source_file: None,
            source_line: 0,
        };

        let outcome = split_at_await(
            &mut f,
            "__kryos_state_twoawait",
            &[
                SplitPoint {
                    block: BlockId(0),
                    inst_idx: 0,
                },
                SplitPoint {
                    block: BlockId(1),
                    inst_idx: 0,
                },
            ],
        )
        .expect("two-split must succeed");

        assert_eq!(outcome.resume_blocks.len(), 2);
        // The dispatch switch must list two resume targets with discriminants 1 and 2.
        match &f.blocks[0].terminator {
            Terminator::Switch { targets, .. } => {
                assert_eq!(targets.len(), 2);
                let discs: BTreeSet<i64> = targets.iter().map(|(d, _)| *d).collect();
                assert!(discs.contains(&1));
                assert!(discs.contains(&2));
            }
            other => panic!("expected Switch, got {other:?}"),
        }

        // Both resume blocks must exist and have valid ids in range.
        for r in &outcome.resume_blocks {
            assert!((r.0 as usize) < f.blocks.len());
        }

        // Two pre-halves should each end in Return(Const::Int(0)).
        let pending_returns = f
            .blocks
            .iter()
            .filter(|b| matches!(
                &b.terminator,
                Terminator::Return(Some(Operand::Constant(Constant::Int(0))))
            ))
            .count();
        assert_eq!(pending_returns, 2, "each split should yield one PENDING return");
    }

    #[test]
    fn split_at_await_three_splits_chained_blocks() {
        // Three sequential awaits across three blocks via Gotos.
        // bb0: _1 = a();  goto bb1
        // bb1: _2 = b(_1); goto bb2
        // bb2: _3 = c(_2); return _3
        let mut attrs = MirAttributes::default();
        attrs.is_async = true;
        let mut f = MirFunction {
            name: "three".into(),
            params: vec![MirParam {
                local: LocalId(0),
                ty: MirType::I64,
            }],
            ret_ty: MirType::I64,
            blocks: vec![
                BasicBlock {
                    id: BlockId(0),
                    instructions: vec![Instruction::Assign {
                        dest: LocalId(1),
                        value: RValue::Call {
                            func: "a".into(),
                            args: vec![],
                        },
                    }],
                    terminator: Terminator::Goto(BlockId(1)),
                },
                BasicBlock {
                    id: BlockId(1),
                    instructions: vec![Instruction::Assign {
                        dest: LocalId(2),
                        value: RValue::Call {
                            func: "b".into(),
                            args: vec![Operand::Local(LocalId(1))],
                        },
                    }],
                    terminator: Terminator::Goto(BlockId(2)),
                },
                BasicBlock {
                    id: BlockId(2),
                    instructions: vec![Instruction::Assign {
                        dest: LocalId(3),
                        value: RValue::Call {
                            func: "c".into(),
                            args: vec![Operand::Local(LocalId(2))],
                        },
                    }],
                    terminator: Terminator::Return(Some(Operand::Local(LocalId(3)))),
                },
            ],
            locals: vec![
                make_named_local(0, "__state", MirType::I64),
                make_named_local(1, "a", MirType::I64),
                make_named_local(2, "b", MirType::I64),
                make_named_local(3, "c", MirType::I64),
            ],
            attributes: attrs,
            source_file: None,
            source_line: 0,
        };

        let outcome = split_at_await(
            &mut f,
            "__kryos_state_three",
            &[
                SplitPoint { block: BlockId(0), inst_idx: 0 },
                SplitPoint { block: BlockId(1), inst_idx: 0 },
                SplitPoint { block: BlockId(2), inst_idx: 0 },
            ],
        )
        .expect("three-split must succeed");

        assert_eq!(outcome.resume_blocks.len(), 3);

        // The dispatch block must Switch on three discriminants (1, 2, 3).
        match &f.blocks[0].terminator {
            Terminator::Switch { targets, .. } => {
                assert_eq!(targets.len(), 3);
                let discs: BTreeSet<i64> = targets.iter().map(|(d, _)| *d).collect();
                assert_eq!(
                    discs,
                    [1, 2, 3].into_iter().collect::<BTreeSet<i64>>()
                );
            }
            other => panic!("expected Switch, got {other:?}"),
        }

        // Every block id must equal its index after re-id.
        for (i, b) in f.blocks.iter().enumerate() {
            assert_eq!(b.id.0 as usize, i);
        }

        // Three PENDING returns.
        let pending_returns = f
            .blocks
            .iter()
            .filter(|b| matches!(
                &b.terminator,
                Terminator::Return(Some(Operand::Constant(Constant::Int(0))))
            ))
            .count();
        assert_eq!(pending_returns, 3);
    }

    #[test]
    fn split_at_await_rejects_bad_block() {
        let mut f = one_split_func();
        let err = split_at_await(
            &mut f,
            "__kryos_state_demo",
            &[SplitPoint {
                block: BlockId(99),
                inst_idx: 0,
            }],
        )
        .unwrap_err();
        assert!(matches!(err, SplitError::BadBlock(_)));
    }

    #[test]
    fn split_at_await_rejects_duplicate_block() {
        let mut f = one_split_func();
        let err = split_at_await(
            &mut f,
            "__kryos_state_demo",
            &[
                SplitPoint { block: BlockId(0), inst_idx: 0 },
                SplitPoint { block: BlockId(0), inst_idx: 1 },
            ],
        )
        .unwrap_err();
        assert!(matches!(err, SplitError::DuplicateBlock(_)));
    }

    #[test]
    fn split_at_await_empty_splits_is_noop() {
        let mut f = one_split_func();
        let before = f.blocks.len();
        let outcome = split_at_await(&mut f, "__kryos_state_demo", &[]).unwrap();
        assert!(outcome.resume_blocks.is_empty());
        assert_eq!(f.blocks.len(), before, "empty split list must not touch the CFG");
    }
}
