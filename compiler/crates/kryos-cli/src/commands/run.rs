//! `kryos run` — compile and execute a Kryos file.

use std::path::Path;
use std::time::Instant;

use kryos_driver::{BuildConfig, BuildMode, OutputType};
use kryos_errors::render_diagnostic;

/// Execute the run command without timing output (the historical entry point).
pub fn execute(file: &str, args: &[String]) -> Result<(), String> {
    execute_inner(file, args, false)
}

/// Execute with `--time` enabled — prints "compile: Xms, exec: Yms, total: Zms"
/// to stderr after the program exits.
pub fn execute_timed(file: &str, args: &[String]) -> Result<(), String> {
    execute_inner(file, args, true)
}

fn execute_inner(file: &str, args: &[String], time: bool) -> Result<(), String> {
    let total_start = Instant::now();
    let path = Path::new(file);

    if !path.exists() {
        return Err(format!("file not found: {file}"));
    }

    if !path.is_file() {
        return Err(format!(
            "`{file}` is not a file -- `kryos run` requires a source file"
        ));
    }

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext != "kry" {
        return Err(format!(
            "expected a .kry file, got `.{ext}` -- did you mean `kryos build`?"
        ));
    }

    // Use a temp directory for the output binary so we don't pollute the cwd
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("kryos_run");
    let exe_name = if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    };
    let tmp_dir = std::env::temp_dir();
    let tmp_output = tmp_dir.join(&exe_name);

    let config = BuildConfig {
        input: file.to_string(),
        output: Some(tmp_output.to_string_lossy().to_string()),
        mode: BuildMode::Debug,
        output_type: OutputType::Binary,
        target: None,
        capabilities: Vec::new(),
        verbose: false,
        skip_ownership: false,
        lto: false,
        debug_info: false,
        use_cache: false,
        split_async_awaits: true,
    };

    // Compile first, then execute.
    // `run` always uses the fast Cranelift backend (debug mode).
    let compile_start = Instant::now();
    let backend = kryos_codegen_cranelift::CraneliftBackend::new();
    let result = kryos_driver::compile_file_with_backend(path, &config, Some(&backend));
    let compile_elapsed = compile_start.elapsed();

    for diag in &result.diagnostics {
        let rendered = render_diagnostic(diag, &result.source_map);
        eprint!("{rendered}");
    }

    if !result.success {
        return Err("compilation failed".to_string());
    }

    match result.output_path {
        Some(ref bin) => {
            let exec_start = Instant::now();
            let status = std::process::Command::new(bin)
                .args(args)
                .status()
                .map_err(|e| format!("failed to execute `{bin}`: {e}"))?;
            let exec_elapsed = exec_start.elapsed();
            // Clean up the temp binary produced by `kryos run`
            let _ = std::fs::remove_file(bin);
            if time {
                let total = total_start.elapsed();
                eprintln!(
                    "\x1b[36mkryos run\x1b[0m  compile: {}ms, exec: {}ms, total: {}ms",
                    compile_elapsed.as_millis(),
                    exec_elapsed.as_millis(),
                    total.as_millis()
                );
            }
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
            Ok(())
        }
        None => Err("compiler did not produce an executable".to_string()),
    }
}
