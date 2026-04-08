//! `kryos run` — compile and execute a Kryos file.

use std::path::Path;

use kryos_driver::{BuildConfig, BuildMode, OutputType};
use kryos_errors::render_diagnostic;

/// Execute the run command.
pub fn execute(file: &str, args: &[String]) -> Result<(), String> {
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
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("kryos_run");
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
    };

    // Compile first, then execute.
    // `run` always uses the fast Cranelift backend (debug mode).
    let backend = kryos_codegen_cranelift::CraneliftBackend::new();
    let result = kryos_driver::compile_file_with_backend(path, &config, Some(&backend));

    for diag in &result.diagnostics {
        let rendered = render_diagnostic(diag, &result.source_map);
        eprint!("{rendered}");
    }

    if !result.success {
        return Err("compilation failed".to_string());
    }

    match result.output_path {
        Some(ref bin) => {
            let status = std::process::Command::new(bin)
                .args(args)
                .status()
                .map_err(|e| format!("failed to execute `{bin}`: {e}"))?;
            // Clean up the temp binary produced by `kryos run`
            let _ = std::fs::remove_file(bin);
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
            Ok(())
        }
        None => Err("compiler did not produce an executable".to_string()),
    }
}
