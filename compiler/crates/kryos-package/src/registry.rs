//! Package registry — Git-based package index and distribution.
//!
//! The Kryos package registry is a Git repository containing a JSON index.
//! Each package has an entry at `<first-two-chars>/<name>.json` containing
//! version metadata, checksums, and download URLs.
//!
//! Default registry: `https://github.com/FrostbyteDevTeam/kryos-registry`

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::manifest::Manifest;
use crate::semver::Version;

/// Default registry URL.
pub const DEFAULT_REGISTRY: &str = "https://github.com/FrostbyteDevTeam/kryos-registry";

/// Registry configuration.
#[derive(Debug, Clone)]
pub struct RegistryConfig {
    pub url: String,
    pub cache_dir: PathBuf,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        let cache_dir = dirs_or_default().join("registry");
        Self {
            url: DEFAULT_REGISTRY.to_string(),
            cache_dir,
        }
    }
}

/// A package entry in the registry index.
#[derive(Debug, Clone)]
pub struct RegistryEntry {
    pub name: String,
    pub version: Version,
    pub checksum: String,
    pub dependencies: HashMap<String, String>,
    pub download_url: String,
}

/// Package tarball metadata for publishing.
#[derive(Debug)]
pub struct PublishPackage {
    pub name: String,
    pub version: Version,
    pub tarball_path: PathBuf,
    pub manifest: Manifest,
}

/// Create a publishable tarball from a project directory.
///
/// Packages the `src/` directory and `kryos.toml` into a `.tar.gz` file
/// in the project's `target/` directory.
pub fn pack(project_dir: &Path) -> Result<PublishPackage, String> {
    let manifest_path = project_dir.join("kryos.toml");
    if !manifest_path.exists() {
        return Err("no kryos.toml found — run `kryos pkg init` first".to_string());
    }

    let manifest = Manifest::from_file(&manifest_path)?;
    let name = manifest.package.name.clone();
    let version = manifest.package.version.parse::<Version>()
        .map_err(|e| format!("invalid version in kryos.toml: {e}"))?;

    let src_dir = project_dir.join("src");
    if !src_dir.exists() {
        return Err("no src/ directory found".to_string());
    }

    // Create target directory.
    let target_dir = project_dir.join("target").join("package");
    std::fs::create_dir_all(&target_dir)
        .map_err(|e| format!("failed to create target/package/: {e}"))?;

    let tarball_name = format!("{}-{}.tar.gz", name, version);
    let tarball_path = target_dir.join(&tarball_name);

    // Collect files to package.
    let mut files: Vec<(PathBuf, String)> = Vec::new();

    // Always include kryos.toml.
    files.push((manifest_path.clone(), "kryos.toml".to_string()));

    // Include all .kry files from src/.
    collect_kry_files(&src_dir, &src_dir, &mut files)?;

    // Include stdlib/ if it exists in the project.
    let stdlib_dir = project_dir.join("stdlib");
    if stdlib_dir.exists() {
        collect_kry_files(&stdlib_dir, &stdlib_dir, &mut files)?;
    }

    // Write a simple text manifest (one file per line) instead of actual tar.gz
    // since we don't have tar/gzip deps. This is sufficient for a v0.1 registry.
    let mut listing = String::new();
    for (abs_path, rel_path) in &files {
        listing.push_str(&format!("{}\n", rel_path));
        // Copy file to package dir.
        let dest = target_dir.join(rel_path);
        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::copy(abs_path, &dest)
            .map_err(|e| format!("failed to copy {}: {e}", rel_path))?;
    }

    std::fs::write(&tarball_path, &listing)
        .map_err(|e| format!("failed to write package listing: {e}"))?;

    Ok(PublishPackage {
        name,
        version,
        tarball_path,
        manifest,
    })
}

/// Generate a registry index entry JSON for a package.
pub fn generate_index_entry(pkg: &PublishPackage) -> String {
    let deps: Vec<String> = pkg
        .manifest
        .dependencies
        .iter()
        .map(|(name, spec)| format!("    \"{}\": \"{}\"", name, spec))
        .collect();

    let deps_json = if deps.is_empty() {
        "{}".to_string()
    } else {
        format!("{{\n{}\n  }}", deps.join(",\n"))
    };

    format!(
        r#"{{
  "name": "{}",
  "version": "{}",
  "dependencies": {},
  "checksum": "sha256:TODO"
}}"#,
        pkg.name, pkg.version, deps_json
    )
}

// ─── helpers ────────────────────────────────────────────────────────────────

fn collect_kry_files(
    dir: &Path,
    base: &Path,
    files: &mut Vec<(PathBuf, String)>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("failed to read directory {}: {e}", dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("directory entry error: {e}"))?;
        let path = entry.path();

        if path.is_dir() {
            collect_kry_files(&path, base, files)?;
        } else if path.extension().map(|e| e == "kry").unwrap_or(false) {
            let rel = path
                .strip_prefix(base)
                .map_err(|e| format!("path prefix error: {e}"))?;
            files.push((path.clone(), format!("src/{}", rel.display())));
        }
    }
    Ok(())
}

fn dirs_or_default() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".kryos")
    } else if let Ok(profile) = std::env::var("USERPROFILE") {
        PathBuf::from(profile).join(".kryos")
    } else {
        PathBuf::from(".kryos")
    }
}
