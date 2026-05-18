//! `kryos clean` — remove build artifacts from a Kryos project.
//!
//! Targets the conventional spots: `target/`, any `.exe`/`.pdb`/`.o`/`.obj`/
//! `.ll`/`.wasm` file at the project root, and stale `.kry.lock` artifacts.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct CleanOptions {
    pub path: Option<String>,
    /// Print what would be removed without actually removing.
    pub dry_run: bool,
}

pub fn execute(opts: CleanOptions) -> Result<(), String> {
    let root = match opts.path.as_deref() {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir().map_err(|e| format!("kryos clean: {e}"))?,
    };
    if !root.exists() {
        return Err(format!("kryos clean: '{}' does not exist", root.display()));
    }

    let mut removed_dirs = 0usize;
    let mut removed_files = 0usize;

    // 1. target/ directory.
    let target = root.join("target");
    if target.is_dir() {
        if opts.dry_run {
            eprintln!("\x1b[33mwould remove dir:\x1b[0m {}", target.display());
        } else {
            fs::remove_dir_all(&target)
                .map_err(|e| format!("kryos clean: remove {}: {e}", target.display()))?;
            eprintln!("\x1b[32mremoved dir:\x1b[0m {}", target.display());
        }
        removed_dirs += 1;
    }

    // 2. Artifact files at the project root (single-file projects).
    for ext in [
        "exe", "pdb", "o", "obj", "ll", "wasm", "lib", "a", "wat",
    ] {
        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() && p.extension().is_some_and(|e| e == ext) {
                    if opts.dry_run {
                        eprintln!("\x1b[33mwould remove file:\x1b[0m {}", p.display());
                    } else if let Err(e) = fs::remove_file(&p) {
                        eprintln!("\x1b[31mfailed:\x1b[0m {}: {e}", p.display());
                        continue;
                    } else {
                        eprintln!("\x1b[32mremoved file:\x1b[0m {}", p.display());
                    }
                    removed_files += 1;
                }
            }
        }
    }

    // 3. .kry.lock stale lock files.
    walk_for_locks(&root, &mut |p| {
        if opts.dry_run {
            eprintln!("\x1b[33mwould remove file:\x1b[0m {}", p.display());
        } else if let Err(e) = fs::remove_file(p) {
            eprintln!("\x1b[31mfailed:\x1b[0m {}: {e}", p.display());
            return;
        } else {
            eprintln!("\x1b[32mremoved file:\x1b[0m {}", p.display());
        }
        removed_files += 1;
    });

    eprintln!();
    if opts.dry_run {
        eprintln!(
            "kryos clean: dry-run — would have removed {removed_dirs} dir(s) + {removed_files} file(s)"
        );
    } else {
        eprintln!("kryos clean: removed {removed_dirs} dir(s) + {removed_files} file(s)");
    }
    Ok(())
}

fn walk_for_locks(dir: &Path, sink: &mut dyn FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file() && p.file_name().is_some_and(|n| n == "kryos.lock") {
            sink(&p);
        } else if p.is_dir() {
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') || name == "target" || name == "node_modules" {
                    continue;
                }
            }
            walk_for_locks(&p, sink);
        }
    }
}
