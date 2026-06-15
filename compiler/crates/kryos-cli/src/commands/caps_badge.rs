//! Capability badging for the package registry (project 05).
//!
//! - `extract_package_caps` walks a project's `.kry` sources, unions the
//!   capability sets of all `@capabilities`-annotated functions, and produces a
//!   `CapsBadge` (capabilities, dangerous subset, annotation coverage).
//! - `badge` writes that badge to `target/caps.json` so `pkg publish`/`pack`
//!   can embed it in the registry index entry.
//! - `show` renders the badge of the latest registry version of a package.
//! - `audit` compares the two latest versions and fails (exit 1) on a dangerous
//!   capability escalation (or any new capability under `--strict`).
//!
//! Capability extraction reuses the same lexer/parser/AST walk as
//! `kryos manifest`; the comparison logic lives in `kryos_package::CapsBadge`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use kryos_ast::Decl;
use kryos_capabilities::{Capability, CapabilitySet};
use kryos_package::{CapsBadge, RegistryClient};

/// Recursively collect `.kry` files, skipping hidden dirs, `target`, `node_modules`.
fn collect_kry(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_file() && p.extension().is_some_and(|x| x == "kry") {
            out.push(p);
        } else if p.is_dir() {
            if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                if name.starts_with('.') || name == "target" || name == "node_modules" {
                    continue;
                }
            }
            collect_kry(&p, out);
        }
    }
}

/// Capability strings for an annotated function, or `None` if unannotated.
fn annotated_caps(annotations: &[kryos_ast::Annotation]) -> Option<Vec<String>> {
    if !annotations.iter().any(|a| a.name == "capabilities") {
        return None;
    }
    let set = CapabilitySet::from_annotations(annotations);
    let caps = if set.has(Capability::All) {
        vec!["all".to_string()]
    } else {
        set.iter().map(|c| c.to_string()).collect()
    };
    Some(caps)
}

/// Tally one function declaration into the running totals.
fn tally_fn(
    annotations: &[kryos_ast::Annotation],
    union: &mut BTreeSet<String>,
    annotated_fns: &mut usize,
    total_fns: &mut usize,
) {
    *total_fns += 1;
    if let Some(caps) = annotated_caps(annotations) {
        *annotated_fns += 1;
        for c in caps {
            union.insert(c);
        }
    }
}

/// Walk `scan_dir` (a file or directory) and build a capability badge.
pub fn extract_package_caps(scan_dir: &Path) -> CapsBadge {
    let mut files: Vec<PathBuf> = Vec::new();
    if scan_dir.is_file() {
        files.push(scan_dir.to_path_buf());
    } else {
        collect_kry(scan_dir, &mut files);
    }
    files.sort();

    let mut union: BTreeSet<String> = BTreeSet::new();
    let mut annotated_fns = 0usize;
    let mut total_fns = 0usize;

    for path in &files {
        let Ok(src) = fs::read_to_string(path) else { continue };
        let tokens = kryos_lexer::Lexer::new(&src, 0).tokenize();
        let module = match kryos_parser::parse(tokens) {
            Ok(m) => m,
            Err(_) => continue,
        };
        for decl in &module.declarations {
            match decl {
                Decl::Function { annotations, .. } => {
                    tally_fn(annotations, &mut union, &mut annotated_fns, &mut total_fns);
                }
                Decl::Impl { methods, .. } => {
                    for method in methods {
                        if let Decl::Function { annotations, .. } = method {
                            tally_fn(annotations, &mut union, &mut annotated_fns, &mut total_fns);
                        }
                    }
                }
                Decl::Trait { methods, .. } => {
                    for method in methods {
                        if let Decl::Function { annotations, .. } = method {
                            tally_fn(annotations, &mut union, &mut annotated_fns, &mut total_fns);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    CapsBadge::from_caps(union.into_iter().collect(), annotated_fns, total_fns, Vec::new())
}

/// `kryos pkg badge [path]` — extract a badge and write `target/caps.json`.
pub fn badge(path: Option<&str>) -> Result<(), String> {
    let project_dir = PathBuf::from(path.unwrap_or("."));
    if !project_dir.exists() {
        return Err(format!("kryos pkg badge: path '{}' does not exist", project_dir.display()));
    }
    let src = project_dir.join("src");
    let scan_dir = if src.exists() { src } else { project_dir.clone() };

    let b = extract_package_caps(&scan_dir);

    let target = project_dir.join("target");
    fs::create_dir_all(&target).map_err(|e| format!("kryos pkg badge: cannot create target/: {e}"))?;
    let out = target.join("caps.json");
    fs::write(&out, b.to_json()).map_err(|e| format!("kryos pkg badge: write error: {e}"))?;

    println!("wrote {}", out.display().to_string().replace('\\', "/"));
    let caps = if b.capabilities.is_empty() { "(none)".to_string() } else { b.capabilities.join(", ") };
    println!("capabilities : {caps}");
    println!("coverage     : {}% of functions annotated", b.annotation_coverage_pct);
    if b.is_dangerous() {
        println!("dangerous    : {}", b.dangerous.join(", "));
    }
    Ok(())
}

/// `kryos pkg show <name>` — render the capability badge of the latest version.
pub fn show(name: &str) -> Result<(), String> {
    let client = RegistryClient::new();
    client.sync()?;
    let entries = client
        .lookup(name)?
        .ok_or_else(|| format!("package '{name}' not found in registry"))?;

    let latest = entries
        .iter()
        .max_by(|a, b| a.version.cmp(&b.version))
        .expect("non-empty entries");

    println!("Package      : {} v{}", latest.name, latest.version);
    match &latest.capabilities {
        None => {
            println!("Capabilities : (no badge — package predates capability badging)");
        }
        Some(b) => {
            let caps = if b.capabilities.is_empty() { "(none)".to_string() } else { b.capabilities.join(", ") };
            let dang = if b.dangerous.is_empty() { "(none)".to_string() } else { b.dangerous.join(", ") };
            println!("Capabilities : {caps}");
            println!("Dangerous    : {dang}");
            println!("Coverage     : {}% of functions annotated", b.annotation_coverage_pct);
            if b.annotation_coverage_pct < 100 {
                println!("Note         : coverage < 100% — badge reflects annotated functions only, not a full sandbox proof");
            }
            if b.is_dangerous() {
                println!("Warning      : high-risk capabilities present ({dang}) — review before installing");
            }
        }
    }
    Ok(())
}

/// `kryos pkg audit <name> [--strict]` — fail on capability escalation between
/// the two latest versions. Exits 1 on a dangerous escalation (ffi/process), or
/// on any new capability under `--strict`.
pub fn audit(name: &str, strict: bool) -> Result<(), String> {
    let client = RegistryClient::new();
    client.sync()?;
    let mut entries = client
        .lookup(name)?
        .ok_or_else(|| format!("package '{name}' not found in registry"))?;

    if entries.len() < 2 {
        println!("kryos pkg audit: {name} has fewer than 2 published versions — nothing to compare");
        return Ok(());
    }
    entries.sort_by(|a, b| a.version.cmp(&b.version));
    let latest = &entries[entries.len() - 1];
    let prev = &entries[entries.len() - 2];

    println!(
        "Comparing {name} v{} (latest) vs v{} (previous)",
        latest.version, prev.version
    );

    let latest_caps = latest.capabilities.clone().unwrap_or_default();
    let prev_caps = prev.capabilities.clone().unwrap_or_default();

    let escalations = latest_caps.dangerous_escalations(&prev_caps);
    if !escalations.is_empty() {
        eprintln!(
            "AUDIT FAIL: {name} v{} adds dangerous capabilities not present in v{}: {}",
            latest.version,
            prev.version,
            escalations.join(", ")
        );
        eprintln!("Review the foreign-function / process calls added before installing.");
        std::process::exit(1);
    }

    let new_caps = latest_caps.new_capabilities(&prev_caps);
    if new_caps.is_empty() {
        println!("OK: no capability escalation between v{} and v{}", prev.version, latest.version);
        return Ok(());
    }

    if strict {
        eprintln!(
            "AUDIT FAIL (--strict): {name} v{} adds new capabilities: {}",
            latest.version,
            new_caps.join(", ")
        );
        std::process::exit(1);
    }
    println!("WARN: {name} v{} adds new capabilities: {}", latest.version, new_caps.join(", "));
    println!("(run with --strict to fail on any capability addition)");
    Ok(())
}
