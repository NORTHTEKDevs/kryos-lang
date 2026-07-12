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

/// Read a Kryos source file, tolerating the encodings real editors
/// produce. A UTF-8 BOM (Windows Notepad's default save) is stripped —
/// previously it reached the lexer and produced a cryptic "unexpected
/// token error" on line 1. UTF-16 (what PowerShell `>` redirection
/// writes) is detected by its BOM and rejected with an actionable
/// message instead of a token-soup diagnostic.
pub fn read_source(path: &std::path::Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file is UTF-16 encoded (PowerShell's `>` writes UTF-16); \
             Kryos sources must be UTF-8 — resave as UTF-8 or use `| Out-File -Encoding utf8`",
        ));
    }
    let body = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        &bytes[..]
    };
    // BOM-less UTF-16 of ASCII text is byte-valid UTF-8 (interleaved
    // NULs), so it would sail through to the lexer as token soup.
    // Detect it by RATIO: UTF-16 ASCII text is ~50% NUL bytes. A stray
    // raw NUL inside a string literal (e.g. the wasm magic "\0asm" in
    // kryos-plugin-sandbox's tests) stays legal.
    if !body.is_empty() {
        let nuls = body.iter().filter(|&&b| b == 0).count();
        if nuls * 4 >= body.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "file is ~half NUL bytes — it is likely UTF-16 encoded; \
                 Kryos sources must be UTF-8",
            ));
        }
    }
    String::from_utf8(body.to_vec()).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("file is not valid UTF-8 (byte offset {}): Kryos sources must be UTF-8", e.utf8_error().valid_up_to()),
        )
    })
}
