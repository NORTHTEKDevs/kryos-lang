//! Linker invocation — constructs and runs system linker commands to produce
//! final executables, shared libraries, or WASM binaries.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::target::{Arch, Env, Os, Target};

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
    /// Enable Link-Time Optimization. Passes `-flto=thin` to the link
    /// driver so cross-module inlining of runtime helpers takes effect.
    /// Default: false.
    pub lto: bool,
    /// Emit/preserve debug info at link time (passes `-g` to the link
    /// driver). Required when the per-object clang invocation embedded
    /// DWARF and LTO is enabled — otherwise the link step would strip
    /// our `!llvm.dbg.cu` compile unit during cross-module merge.
    pub debug_info: bool,
}

impl Default for LinkerConfig {
    fn default() -> Self {
        Self {
            target: Target::host(),
            object_files: Vec::new(),
            runtime_lib: None,
            stdlib_native: None,
            output: PathBuf::from("a.out"),
            link_type: LinkType::Dynamic,
            extra_libs: Vec::new(),
            extra_lib_dirs: Vec::new(),
            lto: false,
            debug_info: false,
        }
    }
}

/// Errors that can occur during linking.
#[derive(Debug)]
pub enum LinkError {
    /// The system linker could not be found.
    LinkerNotFound(String),
    /// The linker process failed to start.
    SpawnFailed {
        linker: String,
        error: std::io::Error,
    },
    /// The linker exited with a non-zero status.
    LinkFailed {
        linker: String,
        exit_code: Option<i32>,
        stderr: String,
    },
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
            LinkError::LinkFailed {
                linker,
                exit_code,
                stderr,
            } => {
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
        let _ = ();
        return Err(LinkError::NoObjectFiles);
    }

    // When LTO is enabled the object files contain LLVM bitcode rather than
    // native machine code, so the linker must be clang (which knows how to
    // invoke LLVM's gold/lld plugin). Plain gcc/cc will fail with
    // "file format not recognized".
    let linker_path = if config.lto {
        find_clang_linker(&config.target)
            .or_else(|_| find_system_linker(&config.target))
            .map_err(LinkError::LinkerNotFound)?
    } else {
        find_system_linker(&config.target).map_err(LinkError::LinkerNotFound)?
    };

    let mut cmd = build_command(&linker_path, config);

    let output = cmd.output().map_err(|e| LinkError::SpawnFailed {
        linker: linker_path.display().to_string(),
        error: e,
    })?;

    if output.status.success() {
        Ok(())
    } else {
        // MSVC link.exe writes errors to stdout, not stderr
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let combined = if stderr.is_empty() {
            stdout
        } else {
            format!("{stderr}\n{stdout}")
        };
        Err(LinkError::LinkFailed {
            linker: linker_path.display().to_string(),
            exit_code: output.status.code(),
            stderr: combined,
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
        LinkType::Static => {
            cmd.arg("-static");
        }
        LinkType::SharedLib => {
            cmd.arg("-shared");
        }
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

    // LTO: pass through to link driver so the linker invokes LLVM's
    // cross-module optimizer. Combined with `-flto=thin` on the per-object
    // clang invocation this lets the linker inline runtime helpers
    // (kryos_array_get/set/len) directly into user code.
    if config.lto {
        cmd.arg("-flto=thin");
    }

    // Debug info: when -g was passed at compile time, also pass -g to
    // the link driver so LTO preserves our DWARF compile unit.
    if config.debug_info {
        cmd.arg("-g");
    }
}

/// Build a command line for MSVC's link.exe.
fn build_msvc_command(cmd: &mut Command, config: &LinkerConfig) {
    cmd.arg(format!("/OUT:{}", config.output.display()));
    cmd.arg("/NOLOGO");

    match config.link_type {
        LinkType::SharedLib => {
            cmd.arg("/DLL");
        }
        LinkType::Static => {
            cmd.arg("/LTCG");
        }
        LinkType::Dynamic => {
            cmd.arg("/SUBSYSTEM:CONSOLE");
            cmd.arg("/ENTRY:mainCRTStartup");
        }
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

    // Add MSVC CRT and Windows SDK library paths
    for lib_path in find_msvc_lib_paths(config.target.arch) {
        cmd.arg(format!("/LIBPATH:{}", lib_path.display()));
    }

    // Link the C runtime and kernel libraries.
    // Use the DLL CRT (/MD) to match how the Rust-based kryos_stdlib_native.lib
    // and kryos_rt.lib are compiled. Mixing libcmt.lib (static /MT) with libs
    // that embed /DEFAULTLIB:MSVCRT causes LNK4098 and unresolved __imp_* symbols.
    cmd.arg("/NODEFAULTLIB:libcmt.lib");
    cmd.arg("msvcrt.lib");
    cmd.arg("vcruntime.lib");
    cmd.arg("ucrt.lib");
    // Provides non-__imp_ definitions of printf/puts/etc. so codegen object
    // files (which call printf directly) link against the DLL CRT.
    cmd.arg("legacy_stdio_definitions.lib");
    cmd.arg("kernel32.lib");

    // Extra library search directories
    for dir in &config.extra_lib_dirs {
        cmd.arg(format!("/LIBPATH:{}", dir.display()));
    }

    // Extra libraries
    for lib in &config.extra_libs {
        cmd.arg(format!("{lib}.lib"));
    }
}

/// Find MSVC CRT and Windows SDK library directories.
fn find_msvc_lib_paths(arch: Arch) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let arch_dir = match arch {
        Arch::X86_64 => "x64",
        Arch::Aarch64 => "arm64",
        _ => return paths,
    };

    let program_files = [
        std::env::var("ProgramFiles(x86)").ok(),
        Some("C:\\Program Files (x86)".to_string()),
    ];

    let editions = ["BuildTools", "Enterprise", "Professional", "Community"];

    // Find MSVC CRT lib path
    for pf in program_files.iter().flatten() {
        for edition in &editions {
            let msvc_dir = PathBuf::from(pf)
                .join("Microsoft Visual Studio")
                .join("2022")
                .join(edition)
                .join("VC")
                .join("Tools")
                .join("MSVC");

            if let Ok(entries) = std::fs::read_dir(&msvc_dir) {
                let mut versions: Vec<PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect();
                versions.sort();
                versions.reverse();

                for ver_dir in versions {
                    let lib_dir = ver_dir.join("lib").join(arch_dir);
                    if lib_dir.is_dir() {
                        paths.push(lib_dir);
                        break;
                    }
                }
            }
            if !paths.is_empty() {
                break;
            }
        }
        if !paths.is_empty() {
            break;
        }
    }

    // Find Windows SDK lib paths (ucrt and um)
    let sdk_root = PathBuf::from("C:\\Program Files (x86)\\Windows Kits\\10\\Lib");
    if sdk_root.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&sdk_root) {
            let mut versions: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            versions.sort();
            versions.reverse();

            if let Some(sdk_ver) = versions.first() {
                let ucrt_dir = sdk_ver.join("ucrt").join(arch_dir);
                if ucrt_dir.is_dir() {
                    paths.push(ucrt_dir);
                }
                let um_dir = sdk_ver.join("um").join(arch_dir);
                if um_dir.is_dir() {
                    paths.push(um_dir);
                }
            }
        }
    }

    paths
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
/// - Windows/MSVC: VS Build Tools `link.exe`, then PATH
/// - Windows/GNU (MinGW): `gcc`, `cc`
/// - Unix: `cc`, `gcc`, `clang`
/// Find clang, used as the link driver when LTO is enabled so the linker
/// can invoke LLVM's LTO plugin to read bitcode object files.
pub fn find_clang_linker(_target: &Target) -> Result<PathBuf, String> {
    for name in &["clang", "clang-19", "clang-18", "clang-17", "clang-16"] {
        if let Some(p) = which(name) {
            return Ok(p);
        }
    }
    Err("could not find clang (required for LTO builds)".to_string())
}

pub fn find_system_linker(target: &Target) -> Result<PathBuf, String> {
    // On Windows/MSVC, try to find the real MSVC link.exe first
    if target.os == Os::Windows && target.env == Env::Msvc {
        if let Some(path) = find_msvc_link_exe(target.arch) {
            return Ok(path);
        }
    }

    let candidates: &[&str] = match (target.arch, target.os, target.env) {
        (Arch::Wasm32, _, _) => &["wasm-ld", "wasm-ld-18", "wasm-ld-17", "wasm-ld-16"],
        (_, Os::Windows, Env::Msvc) => &["link.exe"],
        (_, Os::Windows, _) => &["gcc", "cc"],
        _ => &["cc", "gcc", "clang"],
    };

    for name in candidates {
        if let Some(path) = which(name) {
            // On Windows, verify that a found `link.exe` is actually MSVC's
            // and not Git's /usr/bin/link (Unix hardlink command)
            if name == &"link.exe" {
                let path_str = path.to_string_lossy().to_lowercase();
                if path_str.contains("git")
                    || path_str.contains("usr/bin")
                    || path_str.contains("usr\\bin")
                {
                    continue;
                }
            }
            return Ok(path);
        }
    }

    Err(format!(
        "could not find a linker for target '{}'; searched for: {}",
        target.triple_string(),
        candidates.join(", "),
    ))
}

/// Search for MSVC's link.exe in Visual Studio Build Tools installation.
fn find_msvc_link_exe(arch: Arch) -> Option<PathBuf> {
    let host = if cfg!(target_arch = "x86_64") {
        "Hostx64"
    } else {
        "Hostx86"
    };
    let target_dir = match arch {
        Arch::X86_64 => "x64",
        Arch::Aarch64 => "arm64",
        _ => return None,
    };

    // Search common VS installation paths
    let program_files = [
        std::env::var("ProgramFiles(x86)").ok(),
        std::env::var("ProgramFiles").ok(),
        Some("C:\\Program Files (x86)".to_string()),
        Some("C:\\Program Files".to_string()),
    ];

    let editions = ["BuildTools", "Enterprise", "Professional", "Community"];

    for pf in program_files.iter().flatten() {
        for edition in &editions {
            let vs_dir = PathBuf::from(pf)
                .join("Microsoft Visual Studio")
                .join("2022")
                .join(edition)
                .join("VC")
                .join("Tools")
                .join("MSVC");

            if !vs_dir.is_dir() {
                continue;
            }

            // Find the latest MSVC version
            if let Ok(entries) = std::fs::read_dir(&vs_dir) {
                let mut versions: Vec<PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect();
                versions.sort();
                versions.reverse();

                for ver_dir in versions {
                    let link_exe = ver_dir
                        .join("bin")
                        .join(host)
                        .join(target_dir)
                        .join("link.exe");
                    if link_exe.is_file() {
                        return Some(link_exe);
                    }
                }
            }
        }
    }

    None
}

/// Search PATH for an executable by name.
fn which(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let extensions = if cfg!(windows) {
        vec![
            "".to_string(),
            ".exe".to_string(),
            ".cmd".to_string(),
            ".bat".to_string(),
        ]
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
