//! Linker invocation — constructs and runs system linker commands to produce
//! final executables, shared libraries, or WASM binaries.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::fmt;

use crate::target::{Target, Arch, Os, Env};

/// How to link the output binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkType {
    /// Produce a fully static executable.
    Static,
    /// Produce a dynamically linked executable (default).
    Dynamic,
    /// Produce a shared library (.so / .dylib / .dll).
    SharedLib,
}

/// Configuration for a single link invocation.
#[derive(Debug, Clone)]
pub struct LinkerConfig {
    /// Target triple for the output.
    pub target: Target,
    /// Object files (.o) produced by codegen.
    pub object_files: Vec<PathBuf>,
    /// Path to the Kryos runtime library (libkryos_rt.a).
    pub runtime_lib: Option<PathBuf>,
    /// Path to the Kryos native stdlib library (libkryos_stdlib_native.a).
    pub stdlib_native: Option<PathBuf>,
    /// Output binary path.
    pub output: PathBuf,
    /// Link type (static, dynamic, or shared lib).
    pub link_type: LinkType,
    /// Extra library names to link (-l flags).
    pub extra_libs: Vec<String>,
    /// Extra library search directories (-L flags).
    pub extra_lib_dirs: Vec<PathBuf>,
}

/// Errors that can occur during linking.
#[derive(Debug)]
pub enum LinkError {
    /// The system linker could not be found.
    LinkerNotFound(String),
    /// The linker process failed to start.
    SpawnFailed { linker: String, error: std::io::Error },
    /// The linker exited with a non-zero status.
    LinkFailed { linker: String, exit_code: Option<i32>, stderr: String },
    /// No object files were provided.
    NoObjectFiles,
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkError::LinkerNotFound(msg) => write!(f, "linker not found: {msg}"),
            LinkError::SpawnFailed { linker, error } => {
                write!(f, "failed to spawn linker '{linker}': {error}")
            }
            LinkError::LinkFailed { linker, exit_code, stderr } => {
                write!(
                    f,
                    "linker '{linker}' failed (exit code: {})\n{stderr}",
                    exit_code.map_or("unknown".to_string(), |c| c.to_string())
                )
            }
            LinkError::NoObjectFiles => write!(f, "no object files provided to linker"),
        }
    }
}

impl std::error::Error for LinkError {}

/// Run the system linker with the given configuration.
pub fn link(config: &LinkerConfig) -> Result<(), LinkError> {
    if config.object_files.is_empty() {
        return Err(LinkError::NoObjectFiles);
    }

    let linker_path = find_system_linker(&config.target)
        .map_err(LinkError::LinkerNotFound)?;

    let mut cmd = build_command(&linker_path, config);

    let output = cmd.output().map_err(|e| LinkError::SpawnFailed {
        linker: linker_path.display().to_string(),
        error: e,
    })?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Err(LinkError::LinkFailed {
            linker: linker_path.display().to_string(),
            exit_code: output.status.code(),
            stderr,
        })
    }
}

/// Build the linker `Command` from the config. This is separated from `link`
/// so that tests can inspect the constructed command line without executing it.
pub fn build_command(linker_path: &Path, config: &LinkerConfig) -> Command {
    let mut cmd = Command::new(linker_path);

    match (config.target.os, config.target.env) {
        (Os::Windows, Env::Msvc) => build_msvc_command(&mut cmd, config),
        _ if config.target.is_wasm() => build_wasm_command(&mut cmd, config),
        _ => build_unix_command(&mut cmd, config),
    }

    cmd
}

/// Build a command line for Unix-like linkers (cc, gcc, clang).
fn build_unix_command(cmd: &mut Command, config: &LinkerConfig) {
    // Output path
    cmd.arg("-o").arg(&config.output);

    // Link type flags
    match config.link_type {
        LinkType::Static => { cmd.arg("-static"); }
        LinkType::SharedLib => { cmd.arg("-shared"); }
        LinkType::Dynamic => {} // default behavior
    }

    // Object files
    for obj in &config.object_files {
        cmd.arg(obj);
    }

    // Runtime and stdlib libraries
    if let Some(ref rt) = config.runtime_lib {
        cmd.arg(rt);
    }
    if let Some(ref stdlib) = config.stdlib_native {
        cmd.arg(stdlib);
    }

    // Extra library search directories
    for dir in &config.extra_lib_dirs {
        cmd.arg("-L").arg(dir);
    }

    // Extra libraries
    for lib in &config.extra_libs {
        cmd.arg(format!("-l{lib}"));
    }
}

/// Build a command line for MSVC's link.exe.
fn build_msvc_command(cmd: &mut Command, config: &LinkerConfig) {
    cmd.arg(format!("/OUT:{}", config.output.display()));

    match config.link_type {
        LinkType::SharedLib => { cmd.arg("/DLL"); }
        LinkType::Static => { cmd.arg("/LTCG"); }
        LinkType::Dynamic => {}
    }

    // Object files
    for obj in &config.object_files {
        cmd.arg(obj);
    }

    // Runtime and stdlib libraries
    if let Some(ref rt) = config.runtime_lib {
        cmd.arg(rt);
    }
    if let Some(ref stdlib) = config.stdlib_native {
        cmd.arg(stdlib);
    }

    // Extra library search directories
    for dir in &config.extra_lib_dirs {
        cmd.arg(format!("/LIBPATH:{}", dir.display()));
    }

    // Extra libraries
    for lib in &config.extra_libs {
        cmd.arg(format!("{lib}.lib"));
    }
}

/// Build a command line for wasm-ld.
fn build_wasm_command(cmd: &mut Command, config: &LinkerConfig) {
    cmd.arg("-o").arg(&config.output);
    cmd.arg("--no-entry");
    cmd.arg("--export-dynamic");

    // Object files
    for obj in &config.object_files {
        cmd.arg(obj);
    }

    // Runtime and stdlib libraries
    if let Some(ref rt) = config.runtime_lib {
        cmd.arg(rt);
    }
    if let Some(ref stdlib) = config.stdlib_native {
        cmd.arg(stdlib);
    }

    // Extra library search directories
    for dir in &config.extra_lib_dirs {
        cmd.arg("-L").arg(dir);
    }

    // Extra libraries
    for lib in &config.extra_libs {
        cmd.arg(format!("-l{lib}"));
    }
}

/// Locate a system linker on PATH appropriate for the given target.
///
/// Search order:
/// - WASM targets: `wasm-ld`
/// - Windows/MSVC: `link.exe`
/// - Windows/GNU (MinGW): `gcc`, `cc`
/// - Unix: `cc`, `gcc`, `clang`
pub fn find_system_linker(target: &Target) -> Result<PathBuf, String> {
    let candidates: &[&str] = match (target.arch, target.os, target.env) {
        (Arch::Wasm32, _, _) => &["wasm-ld", "wasm-ld-18", "wasm-ld-17", "wasm-ld-16"],
        (_, Os::Windows, Env::Msvc) => &["link.exe"],
        (_, Os::Windows, _) => &["gcc", "cc"],
        _ => &["cc", "gcc", "clang"],
    };

    for name in candidates {
        if let Some(path) = which(name) {
            return Ok(path);
        }
    }

    Err(format!(
        "could not find a linker for target '{}'; searched for: {}",
        target.triple_string(),
        candidates.join(", "),
    ))
}

/// Search PATH for an executable by name.
fn which(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let extensions = if cfg!(windows) {
        vec!["".to_string(), ".exe".to_string(), ".cmd".to_string(), ".bat".to_string()]
    } else {
        vec!["".to_string()]
    };

    for dir in std::env::split_paths(&path_var) {
        for ext in &extensions {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Extract the command line as a vector of strings for testing/debugging.
pub fn command_to_args(cmd: &Command) -> Vec<String> {
    let mut args = vec![cmd.get_program().to_string_lossy().to_string()];
    args.extend(cmd.get_args().map(|a| a.to_string_lossy().to_string()));
    args
}
