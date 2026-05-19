//! `kryos tree` — print the project's dependency tree.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use kryos_package::{DepSpec, Manifest};

#[derive(Debug, Clone, Default)]
pub struct TreeOptions {
    pub path: Option<String>,
    /// Show transitive dependencies (default: only direct).
    pub transitive: bool,
}

pub fn execute(opts: TreeOptions) -> Result<(), String> {
    let root = match opts.path.as_deref() {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir().map_err(|e| format!("kryos tree: {e}"))?,
    };
    let manifest_path = root.join("kryos.toml");
    if !manifest_path.is_file() {
        return Err(format!(
            "kryos tree: no kryos.toml at {} — run `kryos pkg init` first",
            root.display()
        ));
    }

    let manifest = Manifest::from_file(&manifest_path)?;
    println!(
        "\x1b[1m{}\x1b[0m v{}",
        manifest.package.name, manifest.package.version
    );

    let mut deps: Vec<(&String, &DepSpec)> = manifest.dependencies.iter().collect();
    deps.sort_by(|a, b| a.0.cmp(b.0));

    if deps.is_empty() {
        println!("  (no dependencies)");
        return Ok(());
    }

    let mut visited: HashSet<String> = HashSet::new();
    let last_idx = deps.len() - 1;
    for (i, (name, spec)) in deps.iter().enumerate() {
        let is_last = i == last_idx;
        let prefix = if is_last { "└── " } else { "├── " };
        println!(
            "{prefix}\x1b[36m{name}\x1b[0m {}",
            format_dep_spec(spec)
        );
        if opts.transitive {
            visited.insert((*name).clone());
            walk_transitive(&root, spec, &mut visited, is_last, "");
        }
    }

    Ok(())
}

fn format_dep_spec(spec: &DepSpec) -> String {
    match spec {
        DepSpec::Remote { source, version_req } => format!("({source}@{version_req})"),
        DepSpec::Path { path } => format!("(path = \"{path}\")"),
    }
}

fn walk_transitive(
    project_root: &Path,
    spec: &DepSpec,
    visited: &mut HashSet<String>,
    parent_is_last: bool,
    parent_prefix: &str,
) {
    let path = match spec {
        DepSpec::Path { path } => project_root.join(path),
        DepSpec::Remote { .. } => return,
    };
    let manifest_path = path.join("kryos.toml");
    let Ok(manifest) = Manifest::from_file(&manifest_path) else {
        return;
    };
    let mut deps: Vec<(&String, &DepSpec)> = manifest.dependencies.iter().collect();
    deps.sort_by(|a, b| a.0.cmp(b.0));
    if deps.is_empty() {
        return;
    }
    let connector = if parent_is_last { "    " } else { "│   " };
    let new_prefix = format!("{parent_prefix}{connector}");
    let last_idx = deps.len() - 1;
    for (i, (name, sub_spec)) in deps.iter().enumerate() {
        if visited.contains(*name) {
            println!("{new_prefix}└── \x1b[33m{name}\x1b[0m (cycle)");
            continue;
        }
        let is_last = i == last_idx;
        let twig = if is_last { "└── " } else { "├── " };
        println!(
            "{new_prefix}{twig}\x1b[36m{name}\x1b[0m {}",
            format_dep_spec(sub_spec)
        );
        visited.insert((*name).clone());
        walk_transitive(&path, sub_spec, visited, is_last, &new_prefix);
    }
}
