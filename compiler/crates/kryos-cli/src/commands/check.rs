//! `kryos check` — type-check without compiling.

use std::path::Path;
use std::time::Duration;

use kryos_driver::CapabilityMode;
use kryos_errors::render_diagnostic;

/// Execute the check command, polling for mtime changes every 300ms.
/// Loops forever (Ctrl-C to exit).
pub fn execute_watch(path: &str, skip_ownership: bool, cap_mode: CapabilityMode) -> Result<(), String> {
    let p = Path::new(path);
    if !p.exists() {
        return Err(format!("kryos check: '{}' does not exist", path));
    }
    let mut last_mtime = std::fs::metadata(p).and_then(|m| m.modified()).ok();
    eprintln!(
        "\x1b[1mkryos check --watch\x1b[0m {} (every 300ms; Ctrl-C to exit)",
        p.display()
    );
    eprintln!();
    let _ = execute(path, skip_ownership, cap_mode);
    loop {
        std::thread::sleep(Duration::from_millis(300));
        let cur = std::fs::metadata(p).and_then(|m| m.modified()).ok();
        if cur != last_mtime {
            last_mtime = cur;
            eprintln!();
            eprintln!("\x1b[33mwatch:\x1b[0m change detected, re-checking…");
            let _ = execute(path, skip_ownership, cap_mode);
        }
    }
}

/// Execute the check command.
pub fn execute(path: &str, skip_ownership: bool, cap_mode: CapabilityMode) -> Result<(), String> {
    let p = Path::new(path);

    let (diagnostics, source_map) = if p.is_file() {
        kryos_driver::check_file_with_options_full(p, skip_ownership, cap_mode)
    } else if p.is_dir() {
        let manifest_path = p.join("kryos.toml");
        if !manifest_path.exists() {
            return Err(format!(
                "no kryos.toml found in `{}` — run `kryos pkg init` to create one",
                p.display()
            ));
        }
        kryos_driver::check_project_with_options(p, cap_mode)
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
