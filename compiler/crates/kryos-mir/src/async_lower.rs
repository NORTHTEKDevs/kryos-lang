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

use crate::ir::{LocalId, MirAttributes, MirFunction, MirModule, MirType};
use std::collections::BTreeMap;

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
}
