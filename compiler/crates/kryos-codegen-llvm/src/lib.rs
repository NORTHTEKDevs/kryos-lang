//! Kryos LLVM IR text codegen backend.
//!
//! This crate translates Kryos MIR into LLVM IR text format (`.ll` files).
//! The generated IR can be fed to `llc` or `clang` for optimization and
//! native code generation. No LLVM C/C++ libraries are required at build
//! time — we emit IR as plain text.
//!
//! # Usage
//! ```ignore
//! use kryos_codegen_llvm::{emit_module, EmitOptions, OptLevel};
//!
//! let ir = emit_module(&mir_module, &EmitOptions::default())?;
//! std::fs::write("output.ll", &ir)?;
//! // Then: llc -O2 output.ll -o output.o
//! //   or: clang -O2 output.ll -o output
//! ```

pub mod codegen;

use std::fmt;

// Re-export the main entry point and key types.
pub use codegen::LlvmCodegen;

// ---------------------------------------------------------------------------
// Optimization level
// ---------------------------------------------------------------------------

/// LLVM optimization level — controls `target-cpu` attributes and is
/// informational only (actual optimization is done by `llc`/`opt`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptLevel {
    #[default]
    O0,
    O1,
    O2,
    O3,
}

impl fmt::Display for OptLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OptLevel::O0 => write!(f, "O0"),
            OptLevel::O1 => write!(f, "O1"),
            OptLevel::O2 => write!(f, "O2"),
            OptLevel::O3 => write!(f, "O3"),
        }
    }
}

// ---------------------------------------------------------------------------
// Emit options
// ---------------------------------------------------------------------------

/// Options controlling LLVM IR emission.
#[derive(Debug, Clone, Default)]
pub struct EmitOptions {
    /// Optimization level (informational — placed as module flag).
    pub opt_level: OptLevel,
    /// Target triple, e.g. `x86_64-pc-linux-gnu`. When `None`, the
    /// `target triple` directive is omitted from the output.
    pub target_triple: Option<String>,
    /// Target data layout string. When `None`, omitted.
    pub target_datalayout: Option<String>,
}

// ---------------------------------------------------------------------------
// Codegen errors
// ---------------------------------------------------------------------------

/// Errors that can occur during LLVM IR emission.
#[derive(Debug, Clone)]
pub enum CodegenError {
    /// A MIR type that we cannot (yet) lower to LLVM IR.
    UnsupportedType(String),
    /// A MIR operation that we cannot (yet) lower.
    UnsupportedOperation(String),
    /// The MIR is structurally invalid (e.g. missing block, dangling local).
    InvalidMir(String),
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodegenError::UnsupportedType(msg) => write!(f, "unsupported type: {msg}"),
            CodegenError::UnsupportedOperation(msg) => write!(f, "unsupported operation: {msg}"),
            CodegenError::InvalidMir(msg) => write!(f, "invalid MIR: {msg}"),
        }
    }
}

impl std::error::Error for CodegenError {}

// ---------------------------------------------------------------------------
// Convenience top-level function
// ---------------------------------------------------------------------------

/// Emit LLVM IR text for an entire MIR module using the given options.
pub fn emit_module(
    module: &kryos_mir::ir::MirModule,
    options: &EmitOptions,
) -> Result<String, CodegenError> {
    let mut cg = LlvmCodegen::new(options.clone());
    cg.emit_module(module)
}
