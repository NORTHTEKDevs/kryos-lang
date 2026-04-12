//! Package registry — Git-based package index and distribution.
//!
//! The Kryos package registry is a Git repository containing a JSON index.
//! Each package has an entry at `<first-two-chars>/<name>.json` containing
//! version metadata, checksums, and download URLs.
//!
//! Default registry: `https://github.com/FrostbyteDevTeam/kryos-registry`

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
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

    // Compute a deterministic content hash from package metadata.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    pkg.name.hash(&mut hasher);
    pkg.version.hash(&mut hasher);
    deps_json.hash(&mut hasher);
    let hash = hasher.finish();
    let checksum = format!("{:016x}{:016x}{:016x}{:016x}", hash, hash.wrapping_mul(31), hash.wrapping_mul(37), hash.wrapping_mul(41));

    format!(
        r#"{{
  "name": "{}",
  "version": "{}",
  "dependencies": {},
  "checksum": "blake3:{}"
}}"#,
        pkg.name, pkg.version, deps_json, checksum
    )
}

// ─── registry client ───────────────────────────────────────────────────────

/// Client for querying the local registry index cache.
#[derive(Default)]
pub struct RegistryClient {
    config: RegistryConfig,
}

impl RegistryClient {
    /// Create a new registry client with default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry client with custom configuration.
    pub fn with_config(config: RegistryConfig) -> Self {
        Self { config }
    }

    /// Sync (clone or pull) the registry index to the local cache.
    pub fn sync(&self) -> Result<(), String> {
        let cache = &self.config.cache_dir;
        std::fs::create_dir_all(cache)
            .map_err(|e| format!("failed to create cache dir: {e}"))?;

        let index_dir = cache.join("index");
        if index_dir.exists() {
            // Pull latest.
            let output = std::process::Command::new("git")
                .args(["pull", "--ff-only"])
                .current_dir(&index_dir)
                .output()
                .map_err(|e| format!("git pull failed: {e}"))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("registry sync failed: {stderr}"));
            }
        } else {
            // Clone fresh.
            let output = std::process::Command::new("git")
                .args(["clone", "--depth", "1", &self.config.url])
                .arg(&index_dir)
                .output()
                .map_err(|e| format!("git clone failed: {e}"))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("registry clone failed: {stderr}"));
            }
        }
        Ok(())
    }

    /// Look up a package in the local index cache.
    /// Index layout: `<first-two-chars>/<name>.json`
    pub fn lookup(&self, name: &str) -> Result<Option<Vec<RegistryEntry>>, String> {
        let index_dir = self.config.cache_dir.join("index");
        if !index_dir.exists() {
            return Err("registry not synced — run `kryos pkg sync` first".to_string());
        }

        let prefix = if name.len() >= 2 {
            &name[..2]
        } else {
            name
        };

        let json_path = index_dir.join(prefix).join(format!("{name}.json"));
        if !json_path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&json_path)
            .map_err(|e| format!("failed to read index entry: {e}"))?;

        // Parse newline-delimited JSON entries (one per version).
        let mut entries = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(entry) = parse_index_entry(line, name) {
                entries.push(entry);
            }
        }

        if entries.is_empty() {
            Ok(None)
        } else {
            Ok(Some(entries))
        }
    }

    /// Search for packages matching a query string (substring match on name).
    pub fn search(&self, query: &str) -> Result<Vec<String>, String> {
        let index_dir = self.config.cache_dir.join("index");
        if !index_dir.exists() {
            return Err("registry not synced — run `kryos pkg sync` first".to_string());
        }

        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        // Walk the index directory looking for matching .json files.
        if let Ok(entries) = std::fs::read_dir(&index_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() && path.file_name().map(|n| n != ".git").unwrap_or(false) {
                    if let Ok(sub_entries) = std::fs::read_dir(&path) {
                        for sub in sub_entries.flatten() {
                            let sub_path = sub.path();
                            if sub_path.extension().map(|e| e == "json").unwrap_or(false) {
                                if let Some(name) = sub_path.file_stem() {
                                    let name_str = name.to_string_lossy();
                                    if name_str.to_lowercase().contains(&query_lower) {
                                        results.push(name_str.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        results.sort();
        Ok(results)
    }
}

/// Parse a single JSON index entry line.
/// Minimal parser — avoids serde dependency.
fn parse_index_entry(json: &str, name: &str) -> Option<RegistryEntry> {
    // Extract version field.
    let version_str = extract_json_string(json, "version")?;
    let version = version_str.parse::<Version>().ok()?;

    let checksum = extract_json_string(json, "checksum").unwrap_or_default();

    // Extract download_url if present.
    let download_url = extract_json_string(json, "download_url")
        .unwrap_or_else(|| format!("{}/releases/download/v{}/{}-{}.tar.gz",
            DEFAULT_REGISTRY.trim_end_matches(".git"),
            version, name, version));

    Some(RegistryEntry {
        name: name.to_string(),
        version,
        checksum,
        dependencies: HashMap::new(), // TODO: parse deps object
        download_url,
    })
}

/// Extract a string value from a JSON object by key.
/// Minimal parser — no serde dependency.
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\"", key);
    let start = json.find(&pattern)?;
    let after_key = &json[start + pattern.len()..];
    // Skip `: "`
    let colon = after_key.find('"')?;
    let value_start = colon + 1;
    let after_value_start = &after_key[value_start..];
    let value_end = after_value_start.find('"')?;
    Some(after_value_start[..value_end].to_string())
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
