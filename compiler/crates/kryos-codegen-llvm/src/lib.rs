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
use std::path::PathBuf;

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

// ---------------------------------------------------------------------------
// LlvmBackend — driver-compatible backend wrapper
// ---------------------------------------------------------------------------

/// The LLVM IR codegen backend — implements the driver's Backend trait.
pub struct LlvmBackend {
    options: EmitOptions,
}

impl LlvmBackend {
    pub fn new(options: EmitOptions) -> Self {
        Self { options }
    }
}

impl Default for LlvmBackend {
    fn default() -> Self {
        Self::new(EmitOptions::default())
    }
}

impl kryos_driver::Backend for LlvmBackend {
    fn compile(
        &self,
        module: &kryos_mir::ir::MirModule,
    ) -> Result<Vec<u8>, kryos_driver::BackendError> {
        // 1. Emit LLVM IR text.
        let ir = self.emit_ir(module)?;

        // 2. Find clang on the system.
        let clang = find_llvm_compiler().ok_or_else(|| {
            kryos_driver::BackendError::new(
                "could not find clang; install LLVM or set LLVM_PATH environment variable",
            )
        })?;

        // 3. Write IR to a temp .ll file.
        let tmp_dir = std::env::temp_dir();
        let ll_path = tmp_dir.join("kryos_llvm_tmp.ll");
        let obj_path = tmp_dir.join("kryos_llvm_tmp.o");

        std::fs::write(&ll_path, &ir).map_err(|e| {
            kryos_driver::BackendError::new(format!(
                "failed to write temp .ll file '{}': {e}",
                ll_path.display()
            ))
        })?;

        // 4. Run clang to compile .ll -> .o
        let opt_flag = format!("-{}", self.options.opt_level);
        let mut cmd = std::process::Command::new(&clang);
        cmd.arg(&opt_flag)
            .arg("-c")
            .arg(&ll_path)
            .arg("-o")
            .arg(&obj_path);

        // Pass target triple if specified.
        if let Some(ref triple) = self.options.target_triple {
            cmd.arg(format!("--target={triple}"));
        }

        let output = cmd.output().map_err(|e| {
            kryos_driver::BackendError::new(format!(
                "failed to execute clang at '{}': {e}",
                clang.display()
            ))
        })?;

        // Clean up .ll file (best effort).
        let _ = std::fs::remove_file(&ll_path);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let _ = std::fs::remove_file(&obj_path);
            return Err(kryos_driver::BackendError::new(format!(
                "clang compilation failed:\n{stderr}"
            )));
        }

        // 5. Read the .o bytes.
        let bytes = std::fs::read(&obj_path).map_err(|e| {
            kryos_driver::BackendError::new(format!(
                "failed to read object file '{}': {e}",
                obj_path.display()
            ))
        })?;

        // Clean up .o file (best effort).
        let _ = std::fs::remove_file(&obj_path);

        Ok(bytes)
    }

    fn emit_ir(
        &self,
        module: &kryos_mir::ir::MirModule,
    ) -> Result<String, kryos_driver::BackendError> {
        emit_module(module, &self.options)
            .map_err(|e| kryos_driver::BackendError::new(e.to_string()))
    }

    fn name(&self) -> &str {
        "llvm"
    }
}

// ---------------------------------------------------------------------------
// LLVM tool discovery
// ---------------------------------------------------------------------------

/// Search for a clang/clang.exe compiler on the system.
///
/// Checks (in order):
/// 1. `LLVM_PATH` environment variable
/// 2. Common installation paths (Windows: `C:\Program Files\LLVM\bin`)
/// 3. `PATH` via `which`/`where`
fn find_llvm_compiler() -> Option<PathBuf> {
    // 1. Check LLVM_PATH env var.
    if let Ok(llvm_path) = std::env::var("LLVM_PATH") {
        let candidate = PathBuf::from(&llvm_path).join("clang.exe");
        if candidate.exists() {
            return Some(candidate);
        }
        let candidate = PathBuf::from(&llvm_path).join("clang");
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // 2. Check common installation paths.
    let common_paths: &[&str] = &[
        r"C:\Program Files\LLVM\bin\clang.exe",
        r"C:\Program Files (x86)\LLVM\bin\clang.exe",
        "/usr/bin/clang",
        "/usr/local/bin/clang",
        "/opt/homebrew/bin/clang",
    ];

    for path in common_paths {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }

    // 3. Try PATH lookup.
    #[cfg(windows)]
    {
        if let Ok(output) = std::process::Command::new("where")
            .arg("clang.exe")
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(first_line) = stdout.lines().next() {
                    let p = PathBuf::from(first_line.trim());
                    if p.exists() {
                        return Some(p);
                    }
                }
            }
        }
    }

    #[cfg(not(windows))]
    {
        if let Ok(output) = std::process::Command::new("which")
            .arg("clang")
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(first_line) = stdout.lines().next() {
                    let p = PathBuf::from(first_line.trim());
                    if p.exists() {
                        return Some(p);
                    }
                }
            }
        }
    }

    None
}
