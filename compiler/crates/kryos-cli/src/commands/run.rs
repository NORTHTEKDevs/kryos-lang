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

    let config = BuildConfig {
        input: file.to_string(),
        output: None,
        mode: BuildMode::Debug,
        output_type: OutputType::Binary,
        target: None,
        capabilities: Vec::new(),
        verbose: false,
    };

    // Compile first, then execute.
    let result = kryos_driver::compile_file(path, &config);

    for diag in &result.diagnostics {
        let rendered = render_diagnostic(diag, &result.source_map);
        eprint!("{rendered}");
    }

    if !result.success {
        return Err("compilation failed".to_string());
    }

    // TODO: Once the Cranelift JIT backend is wired up, execute in-process
    // instead of compiling to a temp binary. For now, if compile_file produces
    // an output binary, we exec it.
    match result.output_path {
        Some(ref bin) => {
            let status = std::process::Command::new(bin)
                .args(args)
                .status()
                .map_err(|e| format!("failed to execute `{bin}`: {e}"))?;
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
            Ok(())
        }
        None => Err("compiler did not produce an executable".to_string()),
    }
}
