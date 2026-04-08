//! `kryos check` — type-check without compiling.

use std::path::Path;

use kryos_errors::render_diagnostic;

/// Execute the check command.
pub fn execute(path: &str, skip_ownership: bool) -> Result<(), String> {
    let p = Path::new(path);

    let (diagnostics, source_map) = if p.is_file() {
        kryos_driver::check_file_with_options(p, skip_ownership)
    } else if p.is_dir() {
        let manifest_path = p.join("kryos.toml");
        if !manifest_path.exists() {
            return Err(format!(
                "no kryos.toml found in `{}` — run `kryos pkg init` to create one",
                p.display()
            ));
        }
        kryos_driver::check_project(p)
    } else {
        return Err(format!("`{}` is not a file or directory", p.display()));
    };

    let mut error_count = 0;
    let mut warning_count = 0;

    for diag in &diagnostics {
        let rendered = render_diagnostic(diag, &source_map);
        eprint!("{rendered}");
        if diag.is_error() {
            error_count += 1;
        } else {
            warning_count += 1;
        }
    }

    if error_count > 0 {
        Err(format!(
            "check failed: {error_count} error{}, {warning_count} warning{}",
            if error_count == 1 { "" } else { "s" },
            if warning_count == 1 { "" } else { "s" },
        ))
    } else {
        if warning_count > 0 {
            eprintln!(
                "check passed with {warning_count} warning{}",
                if warning_count == 1 { "" } else { "s" }
            );
        }
        Ok(())
    }
}
