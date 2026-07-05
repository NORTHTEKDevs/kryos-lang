//! Kryos compiler driver — orchestrates the full compilation pipeline.
//!
//! This crate ties together lexing, parsing, type checking, ownership analysis,
//! capability checking, MIR lowering, and code generation into a single
//! `compile_file` / `compile_project` / `check_file` API surface.
//!
//! Pipeline stages:
//!
//! ```text
//! source -> lex -> parse -> type check -> ownership -> capabilities -> MIR -> codegen -> link
//! ```
//!
//! Codegen backends are not compiled in directly. Instead they implement the
//! [`Backend`] trait and are provided at call time, so the driver crate can be
//! built without pulling in Cranelift or LLVM.

pub mod build_cache;
pub mod config;
pub mod pipeline;
pub mod resolve;
pub mod runtime;

// Re-export key types for convenient use from the CLI and other consumers.
pub use config::{BuildConfig, BuildMode, OutputType};
pub use kryos_capabilities::CapabilityMode;
pub use pipeline::{
    check_file, check_file_with_options, check_file_with_options_full, check_project,
    check_project_with_options, check_source, compile_file, compile_file_with_backend,
    compile_project, compile_project_with_backend, compile_source, render_diagnostics, Backend,
    BackendError, CompileResult,
};

/// The version of the Kryos compiler.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
