//! MIR optimization passes.
//!
//! All passes operate on MIR before codegen, benefiting both Cranelift and LLVM
//! backends.  Passes are gated behind `--release` in the pipeline (see
//! `kryos-driver`).
//!
//! ## Pass ordering
//!
//! 1. **Inline** — exposes more constant operands for subsequent passes.
//! 2. **Constant fold** — evaluate constant expressions at compile time.
//! 3. **Pure** — CSE and dead-call elimination for `@pure` functions.
//! 4. **DCE** — remove unreachable blocks and dead assignments.
//! 5. **TCO** — convert tail-recursive calls to loops.
//! 6. **Strength reduction** — replace expensive ops with cheaper equivalents.

pub mod constant_fold;
pub mod dce;
pub mod inline;
pub mod pure;
pub mod strength;
pub mod tco;

use crate::ir::MirModule;

/// Run all optimization passes on the module.
///
/// The pass order is chosen deliberately: inlining first (to expose more
/// constants), then fold, then pure optimizations (CSE + dead pure call
/// removal), then general DCE, convert tail calls, and finally reduce
/// strength of remaining operations.
pub fn optimize(module: &mut MirModule) {
    inline::inline_functions(module, 20);
    constant_fold::fold_constants(module);
    pure::optimize_pure(module);
    dce::eliminate_dead_code(module);
    tco::optimize_tail_calls(module);
    strength::reduce_strength(module);
}
