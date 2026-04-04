//! `kryos fmt` — format Kryos source files.

use std::path::Path;

/// Execute the fmt command.
pub fn execute(files: &[String], check: bool) -> Result<(), String> {
    let targets = if files.is_empty() {
        discover_kry_files(Path::new("."))?
    } else {
        files.to_vec()
    };

    if targets.is_empty() {
        eprintln!("kryos fmt: no .kry files found");
        return Ok(());
    }

    let mut unformatted = 0usize;

    for path in &targets {
        let source = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read `{path}`: {e}"))?;

        let formatted = kryos_fmt::format_source(&source).map_err(|diags| {
            let msgs: Vec<String> = diags.iter().map(|d| d.message.clone()).collect();
            format!("parse error in `{path}`: {}", msgs.join("; "))
        })?;

        if source == formatted {
            continue;
        }

        if check {
            eprintln!("  would reformat {path}");
            unformatted += 1;
        } else {
            std::fs::write(path, &formatted)
                .map_err(|e| format!("cannot write `{path}`: {e}"))?;
            eprintln!("  formatted {path}");
        }
    }

    if check && unformatted > 0 {
        return Err(format!(
            "{unformatted} file{} would be reformatted",
            if unformatted == 1 { "" } else { "s" }
        ));
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
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read `{}`: {e}", dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

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
