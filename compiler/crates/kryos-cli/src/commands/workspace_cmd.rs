//! `kryos workspace ...` — multi-package workspace orchestration.
//!
//! A workspace is a directory with a `kryos.toml` that has a
//! `[workspace]` section listing member packages by path:
//!
//! ```toml
//! [workspace]
//! members = [
//!     "packages/kryos-test-ext",
//!     "packages/kryos-http-router",
//!     "apps/server",
//! ]
//! ```
//!
//! `kryos workspace list` enumerates members.
//! `kryos workspace check` runs `kryos check` over every member.
//! `kryos workspace test` runs `kryos test` over every member.

use std::path::{Path, PathBuf};

use kryos_package::Manifest;

#[derive(Debug, Clone, Copy)]
pub enum WorkspaceAction {
    List,
    Check,
    Test,
}

#[derive(Debug, Clone)]
pub struct WorkspaceOptions {
    pub action: WorkspaceAction,
    /// Workspace root (default: cwd). Must contain a kryos.toml with [workspace].
    pub root: Option<String>,
}

pub fn execute(opts: WorkspaceOptions) -> Result<(), String> {
    let root = match opts.root.as_deref() {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir().map_err(|e| format!("kryos workspace: {e}"))?,
    };

    let manifest_path = root.join("kryos.toml");
    if !manifest_path.is_file() {
        return Err(format!(
            "kryos workspace: no kryos.toml at {}",
            root.display()
        ));
    }

    // Re-parse the toml to find the [workspace] section — the standard
    // Manifest type only exposes the [package] block.
    let toml_src = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("kryos workspace: read {}: {e}", manifest_path.display()))?;
    let members = parse_workspace_members(&toml_src);
    if members.is_empty() {
        return Err(
            "kryos workspace: no [workspace] members declared in kryos.toml".into(),
        );
    }

    match opts.action {
        WorkspaceAction::List => list_members(&root, &members),
        WorkspaceAction::Check => run_check_each(&root, &members),
        WorkspaceAction::Test => run_test_each(&root, &members),
    }
}

fn parse_workspace_members(toml: &str) -> Vec<String> {
    // Find `[workspace]` section and pull the `members = [ ... ]` array.
    // Light parsing — avoids dragging in a full TOML dependency.
    let mut in_workspace = false;
    let mut collecting = false;
    let mut buffer = String::new();
    for line in toml.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_workspace = t == "[workspace]";
            continue;
        }
        if !in_workspace {
            continue;
        }
        if collecting {
            buffer.push_str(line);
            buffer.push('\n');
            if t.ends_with(']') {
                break;
            }
            continue;
        }
        if let Some(rest) = t.strip_prefix("members") {
            let rest = rest.trim_start().trim_start_matches('=').trim();
            if rest.contains('[') {
                buffer.push_str(rest);
                buffer.push('\n');
                if rest.ends_with(']') {
                    break;
                }
                collecting = true;
            }
        }
    }
    let inside = buffer
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']');
    inside
        .split(',')
        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn list_members(root: &Path, members: &[String]) -> Result<(), String> {
    println!("\x1b[1mworkspace\x1b[0m {}", root.display());
    for m in members {
        let path = root.join(m);
        let manifest_path = path.join("kryos.toml");
        match Manifest::from_file(&manifest_path) {
            Ok(mf) => {
                println!(
                    "  \x1b[36m{:<30}\x1b[0m v{}  {}",
                    mf.package.name,
                    mf.package.version,
                    m,
                );
            }
            Err(_) => {
                println!(
                    "  \x1b[31m<unreadable>\x1b[0m              {}  ({})",
                    m, manifest_path.display()
                );
            }
        }
    }
    Ok(())
}

fn run_check_each(root: &Path, members: &[String]) -> Result<(), String> {
    let mut failures = 0usize;
    for m in members {
        let path = root.join(m);
        eprintln!("\x1b[1m[check]\x1b[0m {}", m);
        match super::check::execute(&path.to_string_lossy(), false, false) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("  \x1b[31mfailed\x1b[0m: {e}");
                failures += 1;
            }
        }
    }
    if failures > 0 {
        Err(format!("{failures} member(s) failed check"))
    } else {
        Ok(())
    }
}

fn run_test_each(root: &Path, members: &[String]) -> Result<(), String> {
    let mut failures = 0usize;
    for m in members {
        let path = root.join(m);
        eprintln!("\x1b[1m[test]\x1b[0m {}", m);
        let opts = super::test_cmd::TestOptions {
            path: Some(path.to_string_lossy().to_string()),
            ..Default::default()
        };
        match super::test_cmd::execute(opts) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("  \x1b[31mfailed\x1b[0m: {e}");
                failures += 1;
            }
        }
    }
    if failures > 0 {
        Err(format!("{failures} member(s) failed tests"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_inline_array() {
        let toml = "[workspace]\nmembers = [\"a\", \"b\", \"c\"]\n";
        let m = parse_workspace_members(toml);
        assert_eq!(m, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_multiline_array() {
        let toml = "[workspace]\nmembers = [\n    \"foo\",\n    \"bar\",\n]\n";
        let m = parse_workspace_members(toml);
        assert_eq!(m, vec!["foo", "bar"]);
    }

    #[test]
    fn parse_no_workspace_section() {
        let toml = "[package]\nname = \"x\"\n";
        let m = parse_workspace_members(toml);
        assert!(m.is_empty());
    }
}
