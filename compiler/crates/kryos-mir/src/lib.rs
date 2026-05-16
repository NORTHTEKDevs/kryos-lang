//! Kryos Mid-Level Intermediate Representation (MIR).
//!
//! The MIR sits between the AST and codegen backends (Cranelift / LLVM).
//! It represents programs as a control-flow graph of basic blocks, where
//! each block contains a sequence of instructions and ends with a single
//! terminator (branch, return, switch, etc.).
//!
//! **Key types:**
//! - [`ir::MirModule`] — a lowered module containing functions
//! - [`ir::MirFunction`] — a function's CFG, locals, and parameters
//! - [`ir::BasicBlock`] — straight-line instructions + terminator
//! - [`ir::Instruction`] — assignments, ARC ops, drops
//! - [`ir::Terminator`] — branches, returns, switches
//!
//! **Passes:**
//! - [`lower::lower_module`] — AST -> MIR lowering

pub mod async_lower;
pub mod consteval;
pub mod display;
pub mod ir;
pub mod liveness;
pub mod lower;
pub mod optimize;

// Re-export key types for convenience.
pub use ir::*;
pub use lower::{lower_function, lower_module, lower_resolved_type, lower_type_expr};
