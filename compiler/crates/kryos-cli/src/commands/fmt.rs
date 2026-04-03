//! `kryos fmt` — format Kryos source files.

use std::path::Path;

/// Execute the fmt command.
pub fn execute(files: &[String], check: bool) -> Result<(), String> {
    let targets = if files.is_empty() {
        // Collect all .kry files in the project.
        discover_kry_files(Path::new("."))?
    } else {
        files.to_vec()
    };

    if targets.is_empty() {
        eprintln!("kryos fmt: no .kry files found");
        return Ok(());
    }

    // TODO: Once a formatter is implemented in the driver or as a separate
    // crate, call it here. The formatter should:
    // 1. Parse the source to AST.
    // 2. Pretty-print the AST with canonical formatting rules.
    // 3. Write back (or diff in --check mode).

    if check {
        eprintln!("kryos fmt --check: formatter not yet connected");
        eprintln!("  would check {} file{}", targets.len(), plural(targets.len()));
    } else {
        eprintln!("kryos fmt: formatter not yet connected");
        eprintln!("  would format {} file{}", targets.len(), plural(targets.len()));
    }

    for t in &targets {
        eprintln!("  {t}");
    }

    Ok(())
}

/// Recursively find all `.kry` files under a directory.
fn discover_kry_files(dir: &Path) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    walk_dir(dir, &mut result)?;
    result.sort();
    Ok(result)
}

fn walk_dir(dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("cannot read `{}`: {e}", dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

        // Skip hidden directories and common non-source directories.
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
        }

        if path.is_dir() {
            walk_dir(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("kry") {
            out.push(path.display().to_string());
        }
    }

    Ok(())
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
