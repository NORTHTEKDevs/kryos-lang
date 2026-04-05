//! `kryos build` — compile a Kryos project or file.

use std::path::Path;

use kryos_driver::{Backend, BuildConfig, BuildMode, OutputType};
use kryos_errors::render_diagnostic;

/// Execute the build command.
pub fn execute(
    path: &str,
    release: bool,
    target: Option<&str>,
    output: Option<&str>,
    emit_mir: bool,
    emit_llvm: bool,
    verbose: bool,
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

    let config = BuildConfig {
        input: path.to_string(),
        output: output.map(|s| s.to_string()),
        mode,
        output_type,
        target: target.map(|s| s.to_string()),
        capabilities: Vec::new(),
        verbose,
    };

    if verbose {
        eprintln!("kryos: build config = {:?}", config);
    }

    // Instantiate the appropriate codegen backend based on build mode.
    let backend: Box<dyn Backend> = match mode {
        BuildMode::Debug => Box::new(kryos_codegen_cranelift::CraneliftBackend::new()),
        BuildMode::Release => {
            let triple = if cfg!(target_os = "windows") {
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
            } else {
                "x86_64-unknown-linux-gnu"
            };
            Box::new(kryos_codegen_llvm::LlvmBackend::new(
                kryos_codegen_llvm::EmitOptions {
                    opt_level: kryos_codegen_llvm::OptLevel::O2,
                    target_triple: Some(triple.to_string()),
                    target_datalayout: None,
                },
            ))
        }
    };

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
