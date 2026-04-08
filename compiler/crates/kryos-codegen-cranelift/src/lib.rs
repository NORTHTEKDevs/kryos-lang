//! Cranelift codegen backend for the Kryos programming language.
//!
//! Translates Kryos MIR into native code via Cranelift, supporting both
//! ahead-of-time compilation (producing object files) and JIT compilation
//! (producing executable function pointers for `kryos run` and REPL).

#![allow(
    clippy::result_large_err,
    clippy::too_many_arguments,
    clippy::if_same_then_else
)]

pub mod codegen;
pub mod jit;

use std::fmt;

/// Errors that can occur during Cranelift code generation.
#[derive(Debug)]
pub enum CodegenError {
    /// An unsupported MIR type was encountered.
    UnsupportedType(String),
    /// An unsupported MIR operation was encountered.
    UnsupportedOperation(String),
    /// Cranelift module-level error.
    Module(cranelift_module::ModuleError),
    /// Cranelift codegen verification error.
    Codegen(String),
    /// ISA / target configuration error.
    Target(String),
    /// Internal logic error (should never happen).
    Internal(String),
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodegenError::UnsupportedType(t) => write!(f, "unsupported type: {t}"),
            CodegenError::UnsupportedOperation(op) => write!(f, "unsupported operation: {op}"),
            CodegenError::Module(e) => write!(f, "module error: {e}"),
            CodegenError::Codegen(e) => write!(f, "codegen error: {e}"),
            CodegenError::Target(e) => write!(f, "target error: {e}"),
            CodegenError::Internal(e) => write!(f, "internal error: {e}"),
        }
    }
}

impl std::error::Error for CodegenError {}

impl From<cranelift_module::ModuleError> for CodegenError {
    fn from(e: cranelift_module::ModuleError) -> Self {
        CodegenError::Module(e)
    }
}

/// The Cranelift codegen backend.
pub struct CraneliftBackend {
    _private: (),
}

impl CraneliftBackend {
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// AOT compile a MIR module to object file bytes.
    pub fn compile_module(&self, module: &kryos_mir::ir::MirModule) -> Result<Vec<u8>, CodegenError> {
        codegen::compile_module(module)
    }

    /// JIT compile a single MIR function and return a pointer to executable memory.
    pub fn jit_compile_function(
        &self,
        func: &kryos_mir::ir::MirFunction,
    ) -> Result<*const u8, CodegenError> {
        jit::jit_compile_function(func)
    }
}

impl Default for CraneliftBackend {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// kryos_driver::Backend implementation
// ---------------------------------------------------------------------------

impl kryos_driver::Backend for CraneliftBackend {
    fn compile(&self, module: &kryos_mir::ir::MirModule) -> Result<Vec<u8>, kryos_driver::BackendError> {
        self.compile_module(module)
            .map_err(|e| kryos_driver::BackendError::new(e.to_string()))
    }

    fn emit_ir(&self, _module: &kryos_mir::ir::MirModule) -> Result<String, kryos_driver::BackendError> {
        Err(kryos_driver::BackendError::unsupported(
            "LLVM IR emission not supported by Cranelift backend",
        ))
    }

    fn name(&self) -> &str {
        "cranelift"
    }
}
