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

use std::cell::RefCell;
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
    /// I/O error during file-based emission.
    Io(String),
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodegenError::UnsupportedType(msg) => write!(f, "unsupported type: {msg}"),
            CodegenError::UnsupportedOperation(msg) => write!(f, "unsupported operation: {msg}"),
            CodegenError::InvalidMir(msg) => write!(f, "invalid MIR: {msg}"),
            CodegenError::Io(msg) => write!(f, "I/O error: {msg}"),
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

/// Live state held between `begin_module` / `emit_function_inc` / `finish_module` calls.
struct IncrementalState {
    cg: codegen::LlvmCodegen,
    writer: std::io::BufWriter<std::fs::File>,
    ll_path: PathBuf,
    has_void_main: bool,
}

/// The LLVM IR codegen backend — implements the driver's Backend trait.
pub struct LlvmBackend {
    options: EmitOptions,
    /// Interior-mutable session state for incremental per-function emission.
    inc: RefCell<Option<IncrementalState>>,
}

impl LlvmBackend {
    pub fn new(options: EmitOptions) -> Self {
        Self {
            options,
            inc: RefCell::new(None),
        }
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
        // 1. Find clang on the system.
        let clang = find_llvm_compiler().ok_or_else(|| {
            kryos_driver::BackendError::new(
                "could not find clang; install LLVM or set LLVM_PATH environment variable",
            )
        })?;

        // 2. Emit LLVM IR text and write directly to temp .ll file.
        //    The IR String is scoped so it is freed immediately after the write,
        //    avoiding holding 1-2 GB in memory while clang runs.
        let tmp_dir = std::env::temp_dir();
        let ll_path = tmp_dir.join("kryos_llvm_tmp.ll");
        let obj_path = tmp_dir.join("kryos_llvm_tmp.o");

        {
            let ir = self.emit_ir(module)?;
            std::fs::write(&ll_path, ir.as_bytes()).map_err(|e| {
                kryos_driver::BackendError::new(format!(
                    "failed to write temp .ll file '{}': {e}",
                    ll_path.display()
                ))
            })?;
            // ir drops here -- 1-2 GB freed before clang runs
        }

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

    fn supports_incremental(&self) -> bool {
        true
    }

    fn begin_module(
        &self,
        header: &kryos_mir::ir::MirModuleHeader,
        functions: &[kryos_mir::ir::MirFunction],
    ) -> Result<(), kryos_driver::BackendError> {
        use std::io::Write;

        let _clang = find_llvm_compiler().ok_or_else(|| {
            kryos_driver::BackendError::new(
                "could not find clang; install LLVM or set LLVM_PATH environment variable",
            )
        })?;

        let ll_path = std::env::temp_dir().join("kryos_llvm_inc.ll");
        let file = std::fs::File::create(&ll_path).map_err(|e| {
            kryos_driver::BackendError::new(format!(
                "failed to create temp .ll file '{}': {e}",
                ll_path.display()
            ))
        })?;
        let mut writer = std::io::BufWriter::new(file);

        let mut cg = codegen::LlvmCodegen::new(self.options.clone());

        // Prescan all functions for cross-function metadata (signatures, closures, ARC).
        cg.prescan_all(header, functions);

        // Detect whether the user's main returns void (no return type) so we can
        // emit the C-ABI wrapper later.
        let has_void_main = functions
            .iter()
            .any(|f| f.name == "main" && f.ret_ty == kryos_mir::ir::MirType::Void);

        // Emit module header (target triple, datalayout, struct type defs, string globals).
        cg.emit_header_section();
        let header_text = cg.take_output();
        writer.write_all(header_text.as_bytes()).map_err(|e| {
            kryos_driver::BackendError::new(format!("failed to write .ll header: {e}"))
        })?;

        // Store the clang path in a thread-local / drop it on finish. For now store ll_path.
        // We keep clang path discovery deferred to finish_module (rediscovery is cheap).
        *self.inc.borrow_mut() = Some(IncrementalState {
            cg,
            writer,
            ll_path,
            has_void_main,
        });

        Ok(())
    }

    fn emit_function_inc(
        &self,
        func: kryos_mir::ir::MirFunction,
    ) -> Result<(), kryos_driver::BackendError> {
        use std::io::Write;

        let mut borrow = self.inc.borrow_mut();
        let state = borrow.as_mut().ok_or_else(|| {
            kryos_driver::BackendError::new("emit_function_inc called before begin_module")
        })?;

        state
            .cg
            .emit_one_function_inc(&func, state.has_void_main)
            .map_err(|e| kryos_driver::BackendError::new(e.to_string()))?;

        let text = state.cg.take_output();
        state.writer.write_all(text.as_bytes()).map_err(|e| {
            kryos_driver::BackendError::new(format!(
                "failed to write function '{}' IR: {e}",
                func.name
            ))
        })?;

        Ok(())
    }

    fn finish_module(&self) -> Result<Vec<u8>, kryos_driver::BackendError> {
        use std::io::Write;

        let state = self.inc.borrow_mut().take().ok_or_else(|| {
            kryos_driver::BackendError::new("finish_module called before begin_module")
        })?;

        let IncrementalState {
            mut cg,
            mut writer,
            ll_path,
            has_void_main,
        } = state;

        // Emit footer (closure droppers, type drop helpers, main wrapper if needed).
        cg.emit_footer_section(has_void_main);
        let footer_text = cg.take_output();
        writer.write_all(footer_text.as_bytes()).map_err(|e| {
            kryos_driver::BackendError::new(format!("failed to write .ll footer: {e}"))
        })?;
        writer.flush().map_err(|e| {
            kryos_driver::BackendError::new(format!("failed to flush .ll file: {e}"))
        })?;
        drop(writer); // close the file before clang reads it

        // Run clang to compile .ll -> .o
        let clang = find_llvm_compiler()
            .ok_or_else(|| kryos_driver::BackendError::new("could not find clang"))?;
        let obj_path = std::env::temp_dir().join("kryos_llvm_inc.o");
        let opt_flag = format!("-{}", self.options.opt_level);
        let mut cmd = std::process::Command::new(&clang);
        cmd.arg(&opt_flag)
            .arg("-c")
            .arg(&ll_path)
            .arg("-o")
            .arg(&obj_path);
        if let Some(ref triple) = self.options.target_triple {
            cmd.arg(format!("--target={triple}"));
        }

        let output = cmd.output().map_err(|e| {
            kryos_driver::BackendError::new(format!(
                "failed to execute clang at '{}': {e}",
                clang.display()
            ))
        })?;

        let _ = std::fs::remove_file(&ll_path);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let _ = std::fs::remove_file(&obj_path);
            return Err(kryos_driver::BackendError::new(format!(
                "clang compilation failed:\n{stderr}"
            )));
        }

        let bytes = std::fs::read(&obj_path).map_err(|e| {
            kryos_driver::BackendError::new(format!(
                "failed to read object file '{}': {e}",
                obj_path.display()
            ))
        })?;
        let _ = std::fs::remove_file(&obj_path);

        Ok(bytes)
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
        if let Ok(output) = std::process::Command::new("which").arg("clang").output() {
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
