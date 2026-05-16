//! `kryos build` — compile a Kryos project or file.

use std::path::Path;

use kryos_driver::{Backend, BuildConfig, BuildMode, OutputType};
use kryos_errors::render_diagnostic;

/// Execute the build command.
#[allow(clippy::too_many_arguments)]
pub fn execute(
    path: &str,
    release: bool,
    backend_override: Option<&str>,
    target: Option<&str>,
    output: Option<&str>,
    emit_mir: bool,
    emit_llvm: bool,
    verbose: bool,
    skip_ownership: bool,
    lto: bool,
    debug_info: bool,
    use_cache: bool,
) -> Result<(), String> {
    let mode = if release {
        BuildMode::Release
    } else {
        BuildMode::Debug
    };

    let output_type = if emit_mir {
        OutputType::Mir
    } else if emit_llvm {
        OutputType::LlvmIr
    } else {
        OutputType::Binary
    };

    // Effective LTO: explicit flag wins; otherwise release implies LTO.
    let effective_lto = lto || release;

    let config = BuildConfig {
        input: path.to_string(),
        output: output.map(|s| s.to_string()),
        mode,
        output_type,
        target: target.map(|s| s.to_string()),
        capabilities: Vec::new(),
        verbose,
        skip_ownership,
        lto: effective_lto,
        debug_info,
        use_cache,
    };

    if verbose {
        eprintln!("kryos: build config = {:?}", config);
    }

    // Resolve target triple. If the user passed --target, validate and use
    // it; otherwise default to the host.
    let resolved_target = resolve_target(target)?;

    // For DWARF: resolve the absolute source path. If the user pointed at
    // a project directory, this stays None and codegen falls back to
    // skipping DI metadata.
    let source_file_path: Option<String> = if debug_info {
        let p = Path::new(path);
        if p.is_file() {
            std::fs::canonicalize(p)
                .ok()
                .and_then(|c| c.to_str().map(|s| s.to_string()))
        } else {
            None
        }
    } else {
        None
    };

    // Warn if cross-compile was requested in debug mode — Cranelift only
    // targets the host. Tell the user to add --release for LLVM cross.
    if target.is_some() && matches!(mode, BuildMode::Debug) && resolved_target != host_target_triple() {
        eprintln!(
            "warning: --target={resolved_target} requires --release (LLVM backend). \
             Cranelift only produces code for the host. Add --release to cross-compile."
        );
    }

    // Instantiate the appropriate codegen backend.
    //
    // Resolution order: explicit --backend > --release > default (cranelift).
    let backend: Box<dyn Backend> = match backend_override {
        Some("wasm") | Some("wasm32") => Box::new(kryos_codegen_wasm::WasmBackend::new(
            kryos_codegen_wasm::WasmOptions::default(),
        )),
        Some("llvm") => Box::new(kryos_codegen_llvm::LlvmBackend::new(
            kryos_codegen_llvm::EmitOptions {
                opt_level: kryos_codegen_llvm::OptLevel::O2,
                target_triple: Some(resolved_target.clone()),
                target_datalayout: None,
                lto: effective_lto,
                codegen_threads: 0,
                debug_info,
                source_file_path: source_file_path.clone(),
            },
        )),
        Some("cranelift") => Box::new(kryos_codegen_cranelift::CraneliftBackend::new()),
        Some(other) => {
            return Err(format!(
                "unknown --backend `{other}`. Valid values: cranelift, llvm, wasm"
            ));
        }
        None => match mode {
            BuildMode::Debug => Box::new(kryos_codegen_cranelift::CraneliftBackend::new()),
            BuildMode::Release => Box::new(kryos_codegen_llvm::LlvmBackend::new(
                kryos_codegen_llvm::EmitOptions {
                    opt_level: kryos_codegen_llvm::OptLevel::O2,
                    target_triple: Some(resolved_target.clone()),
                    target_datalayout: None,
                    // Default release builds to LTO on — the runtime helpers
                    // (kryos_array_get/set) are `#[inline]` and only inline
                    // across crate boundaries when LTO is enabled.
                    lto: effective_lto,
                    codegen_threads: 0,
                    debug_info,
                    source_file_path: source_file_path.clone(),
                },
            )),
        },
    };

    if verbose {
        eprintln!("kryos: target = {resolved_target}");
    }

    let p = Path::new(path);

    let result = if p.is_file() {
        kryos_driver::compile_file_with_backend(p, &config, Some(backend.as_ref()))
    } else if p.is_dir() {
        // Project directory -- look for kryos.toml.
        let manifest_path = p.join("kryos.toml");
        if !manifest_path.exists() {
            return Err(format!(
                "no kryos.toml found in `{}` -- run `kryos pkg init` to create one",
                p.display()
            ));
        }
        kryos_driver::compile_project_with_backend(p, &config, Some(backend.as_ref()))
    } else {
        return Err(format!("`{}` is not a file or directory", p.display()));
    };

    // Render diagnostics to stderr.
    let use_color = atty_stderr();
    for diag in &result.diagnostics {
        let rendered = render_diagnostic(diag, &result.source_map);
        if use_color {
            eprint!("{}", colorize_diagnostic(&rendered));
        } else {
            eprint!("{rendered}");
        }
    }

    if !result.success {
        let n = result.error_count();
        return Err(format!(
            "compilation failed with {n} error{}",
            if n == 1 { "" } else { "s" }
        ));
    }

    // Handle special output types.
    if emit_mir {
        if let Some(ref mir) = result.mir {
            println!("{mir}");
        }
    } else if emit_llvm {
        if let Some(ref ir) = result.llvm_ir {
            match output {
                Some(out_path) => {
                    std::fs::write(out_path, ir)
                        .map_err(|e| format!("failed to write `{out_path}`: {e}"))?;
                    eprintln!("kryos: wrote {out_path}");
                }
                None => print!("{ir}"),
            }
        }
    }

    if let Some(ref out) = result.output_path {
        if verbose {
            eprintln!("kryos: wrote {out}");
        }
    }

    Ok(())
}

/// Known target triples. Supplying `--target=help` prints this list.
/// We accept arbitrary triples too (LLVM will reject invalid ones at
/// codegen time); this list is just for tab-completion and validation hints.
const KNOWN_TARGETS: &[(&str, &str)] = &[
    ("x86_64-unknown-linux-gnu", "Linux on x86_64 (glibc)"),
    ("x86_64-unknown-linux-musl", "Linux on x86_64 (musl, fully static)"),
    ("aarch64-unknown-linux-gnu", "Linux on ARM64 (glibc)"),
    ("aarch64-unknown-linux-musl", "Linux on ARM64 (musl, fully static)"),
    ("x86_64-pc-windows-gnu", "Windows on x86_64 (MinGW)"),
    ("x86_64-pc-windows-msvc", "Windows on x86_64 (MSVC)"),
    ("aarch64-pc-windows-msvc", "Windows on ARM64 (MSVC)"),
    ("x86_64-apple-darwin", "macOS on x86_64"),
    ("aarch64-apple-darwin", "macOS on ARM64 (Apple Silicon)"),
    ("wasm32-unknown-unknown", "WebAssembly (browser)"),
    ("wasm32-wasi", "WebAssembly (WASI)"),
];

/// Resolve a target triple. `None` means use the host. `Some("help")` prints
/// the list and returns a special sentinel.
fn resolve_target(target: Option<&str>) -> Result<String, String> {
    match target {
        None => Ok(host_target_triple().to_string()),
        Some("help") | Some("list") => {
            println!("Known target triples:");
            println!();
            for (triple, desc) in KNOWN_TARGETS {
                println!("  {:35}  {}", triple, desc);
            }
            println!();
            println!("You may also pass an arbitrary LLVM triple; it will be forwarded as-is.");
            std::process::exit(0);
        }
        Some(t) => {
            // Warn (not error) if the triple isn't recognized; LLVM might still accept it.
            if !KNOWN_TARGETS.iter().any(|(known, _)| *known == t) {
                eprintln!(
                    "warning: target `{t}` is not in the known-good list. Run \
                     `kryos build --target=help` to see supported triples. Forwarding to LLVM."
                );
            }
            Ok(t.to_string())
        }
    }
}

/// The compile-time host triple.
fn host_target_triple() -> &'static str {
    if cfg!(target_os = "windows") {
        if cfg!(target_arch = "x86_64") {
            "x86_64-pc-windows-msvc"
        } else {
            "aarch64-pc-windows-msvc"
        }
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "aarch64-apple-darwin"
        } else {
            "x86_64-apple-darwin"
        }
    } else if cfg!(target_env = "musl") {
        if cfg!(target_arch = "aarch64") {
            "aarch64-unknown-linux-musl"
        } else {
            "x86_64-unknown-linux-musl"
        }
    } else if cfg!(target_arch = "aarch64") {
        "aarch64-unknown-linux-gnu"
    } else {
        "x86_64-unknown-linux-gnu"
    }
}

/// Check whether stderr is a terminal (for colored output).
fn atty_stderr() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }

    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let handle = std::io::stderr().as_raw_handle();
        let mut mode: u32 = 0;
        unsafe { windows_sys_get_console_mode(handle as _, &mut mode) != 0 }
    }

    #[cfg(not(windows))]
    {
        // On non-Windows platforms, check the TERM variable as a proxy.
        // A full isatty check would require the libc crate.
        std::env::var_os("TERM").is_some()
    }
}

#[cfg(windows)]
extern "system" {
    #[link_name = "GetConsoleMode"]
    fn windows_sys_get_console_mode(handle: *mut std::ffi::c_void, mode: *mut u32) -> i32;
}

/// Minimal ANSI colorization of diagnostic output.
fn colorize_diagnostic(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if line.starts_with("error") {
            out.push_str("\x1b[1;31m");
            out.push_str(line);
            out.push_str("\x1b[0m");
        } else if line.starts_with("warning") {
            out.push_str("\x1b[1;33m");
            out.push_str(line);
            out.push_str("\x1b[0m");
        } else if line.starts_with("info") || line.starts_with("help") {
            out.push_str("\x1b[1;36m");
            out.push_str(line);
            out.push_str("\x1b[0m");
        } else if line.contains("^^^") {
            out.push_str("\x1b[1;31m");
            out.push_str(line);
            out.push_str("\x1b[0m");
        } else {
            out.push_str(line);
        }
    }
    out
}
