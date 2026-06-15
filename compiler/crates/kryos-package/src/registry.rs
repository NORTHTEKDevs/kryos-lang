//! Package registry — Git-based package index and distribution.
//!
//! The Kryos package registry is a Git repository containing a JSON index.
//! Each package has an entry at `<first-two-chars>/<name>.json` containing
//! version metadata, checksums, and download URLs.
//!
//! Default registry: `https://github.com/NORTHTEKDevs/kryos-registry`

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::caps::CapsBadge;
use crate::manifest::Manifest;
use crate::semver::Version;

/// Default registry URL.
pub const DEFAULT_REGISTRY: &str = "https://github.com/NORTHTEKDevs/kryos-registry";

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
    /// Capability badge (project 05). `None` for entries published before
    /// capability badging, or entries whose source carried no annotations.
    pub capabilities: Option<CapsBadge>,
}

/// Package tarball metadata for publishing.
#[derive(Debug)]
pub struct PublishPackage {
    pub name: String,
    pub version: Version,
    pub tarball_path: PathBuf,
    pub manifest: Manifest,
    /// Capability badge read from `target/caps.json` at pack time (project 05).
    pub caps_badge: Option<CapsBadge>,
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
    let version = manifest
        .package
        .version
        .parse::<Version>()
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
        std::fs::copy(abs_path, &dest).map_err(|e| format!("failed to copy {}: {e}", rel_path))?;
    }

    std::fs::write(&tarball_path, &listing)
        .map_err(|e| format!("failed to write package listing: {e}"))?;

    // Embed the capability badge if `kryos manifest --badge` wrote one to
    // target/caps.json before packing. Absent badge -> None (backward compatible).
    let caps_badge = read_caps_badge(project_dir);

    Ok(PublishPackage {
        name,
        version,
        tarball_path,
        manifest,
        caps_badge,
    })
}

/// Read a capability badge from `<project_dir>/target/caps.json` if present.
/// Returns `None` if the file is missing or not a valid `CapsBadge`.
pub fn read_caps_badge(project_dir: &Path) -> Option<CapsBadge> {
    let path = project_dir.join("target").join("caps.json");
    let text = std::fs::read_to_string(path).ok()?;
    CapsBadge::from_json(&text)
}

/// Generate a registry index entry JSON for a package.
///
/// The checksum is `sha256:<64-hex>` of the tarball bytes at
/// `pkg.tarball_path`. This matches the canonical kryos-registry
/// schema (one NDJSON line per published version, immutable). If the
/// tarball file cannot be read, the checksum field is emitted as
/// `sha256:unavailable` so the call still produces a syntactically
/// valid JSON line; downstream tools that need a real hash will
/// observe the literal and fail loudly rather than silently accept a
/// fake digest.
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

    let checksum = match std::fs::read(&pkg.tarball_path) {
        Ok(bytes) => format!("sha256:{}", crate::sha256::sha256_hex(&bytes)),
        Err(_) => "sha256:unavailable".to_string(),
    };

    // Embed the capability badge (project 05) as a `"capabilities"` object when
    // present. Omitted entirely when absent, so old tooling and pre-badging
    // entries remain byte-compatible.
    let caps_field = match &pkg.caps_badge {
        Some(b) => format!(",\n  \"capabilities\": {}", b.to_json()),
        None => String::new(),
    };

    format!(
        r#"{{
  "name": "{}",
  "version": "{}",
  "dependencies": {},
  "checksum": "{}"{}
}}"#,
        pkg.name, pkg.version, deps_json, checksum, caps_field
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
        std::fs::create_dir_all(cache).map_err(|e| format!("failed to create cache dir: {e}"))?;

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

        let prefix = if name.len() >= 2 { &name[..2] } else { name };

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
    let download_url = extract_json_string(json, "download_url").unwrap_or_else(|| {
        format!(
            "{}/releases/download/v{}/{}-{}.tar.gz",
            DEFAULT_REGISTRY.trim_end_matches(".git"),
            version,
            name,
            version
        )
    });

    // Accept both `dependencies` (canonical, used by the live registry)
    // and the historical `deps` shorthand from early-version index files
    // so old entries don't suddenly fail to resolve.
    let mut deps = extract_deps_object(json, "dependencies");
    if deps.is_empty() {
        deps = extract_deps_object(json, "deps");
    }

    // Optional capability badge (project 05). Parsed with serde_json so the
    // nested object is read robustly; absent/invalid -> None (backward compatible).
    let capabilities = serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| v.get("capabilities").cloned())
        .and_then(|c| serde_json::from_value::<CapsBadge>(c).ok());

    Some(RegistryEntry {
        name: name.to_string(),
        version,
        checksum,
        dependencies: deps,
        download_url,
        capabilities,
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

/// Extract a JSON object of string key-value pairs by key.
/// Parses `"key": { "a": "1", "b": "2" }` from a flat JSON line.
fn extract_deps_object(json: &str, key: &str) -> HashMap<String, String> {
    let pattern = format!("\"{}\"", key);
    let start = match json.find(&pattern) {
        Some(i) => i,
        None => return HashMap::new(),
    };
    let after_key = &json[start + pattern.len()..];
    // Find the opening `{`.
    let brace_start = match after_key.find('{') {
        Some(i) => i,
        None => return HashMap::new(),
    };
    let inner_start = brace_start + 1;
    // Find the matching closing `}`.
    let brace_end = match after_key[inner_start..].find('}') {
        Some(i) => inner_start + i,
        None => return HashMap::new(),
    };
    let inner = &after_key[inner_start..brace_end];

    // Parse `"key": "value"` pairs from the inner slice.
    let mut result = HashMap::new();
    let mut s = inner;
    while let Some(kq) = s.find('"') {
        s = &s[kq + 1..];
        let kend = match s.find('"') {
            Some(i) => i,
            None => break,
        };
        let dep_name = s[..kend].to_string();
        s = &s[kend + 1..];
        // Skip to next `"` (the value opening quote).
        let vq = match s.find('"') {
            Some(i) => i,
            None => break,
        };
        s = &s[vq + 1..];
        let vend = match s.find('"') {
            Some(i) => i,
            None => break,
        };
        let dep_ver = s[..vend].to_string();
        s = &s[vend + 1..];
        if !dep_name.is_empty() {
            result.insert(dep_name, dep_ver);
        }
    }
    result
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_canonical_registry_entry() {
        // This is the byte-for-byte index entry that lives at
        // https://github.com/NORTHTEKDevs/kryos-registry/blob/master/ht/http-router.json
        // — the canonical shape the live registry actually uses.
        let line = r#"{"name": "http-router", "version": "0.1.0", "dependencies": {}, "checksum": "sha256:176df653ffa02096dfc3c486afb553040fed2e7e9d00270b3b0ae127a3e99469", "download_url": "https://raw.githubusercontent.com/NORTHTEKDevs/kryos-registry/master/tarballs/http-router-0.1.0.tar.gz"}"#;

        let entry = parse_index_entry(line, "http-router").expect("must parse");
        assert_eq!(entry.name, "http-router");
        assert_eq!(entry.version.to_string(), "0.1.0");
        assert_eq!(
            entry.checksum,
            "sha256:176df653ffa02096dfc3c486afb553040fed2e7e9d00270b3b0ae127a3e99469"
        );
        assert!(entry.dependencies.is_empty());
        assert!(entry
            .download_url
            .starts_with("https://raw.githubusercontent.com/NORTHTEKDevs/kryos-registry/"));
    }

    #[test]
    fn parse_legacy_deps_shorthand_still_works() {
        // Older index files used `deps` instead of `dependencies`. Make sure
        // we don't suddenly fail to resolve them.
        let line = r#"{"name": "old-pkg", "version": "1.2.3", "deps": { "regex": "0.1.0" }, "checksum": "sha256:0000000000000000000000000000000000000000000000000000000000000000", "download_url": "https://example.com/old-pkg-1.2.3.tar.gz"}"#;
        let entry = parse_index_entry(line, "old-pkg").expect("must parse");
        assert_eq!(entry.version.to_string(), "1.2.3");
        assert_eq!(entry.dependencies.get("regex"), Some(&"0.1.0".to_string()));
    }

    #[test]
    fn parse_dependencies_with_values() {
        let line = r#"{"name": "app", "version": "0.2.0", "dependencies": {"json":"0.1.0","http-router":"0.1.0"}, "checksum": "sha256:1111111111111111111111111111111111111111111111111111111111111111", "download_url": "https://example.com/app-0.2.0.tar.gz"}"#;
        let entry = parse_index_entry(line, "app").expect("must parse");
        assert_eq!(entry.dependencies.len(), 2);
        assert_eq!(entry.dependencies.get("json"), Some(&"0.1.0".to_string()));
        assert_eq!(
            entry.dependencies.get("http-router"),
            Some(&"0.1.0".to_string())
        );
    }

    #[test]
    fn generate_index_entry_emits_sha256_of_tarball() {
        // Round-trip: write a known byte payload to a temp tarball path, run the
        // generator, and verify the checksum field matches the SHA-256 of those
        // bytes — *not* a stable hash of metadata.
        use crate::manifest::PackageInfo;
        use std::io::Write;
        let dir = std::env::temp_dir().join("kryos-registry-test");
        std::fs::create_dir_all(&dir).unwrap();
        let tarball_path = dir.join("demo-0.1.0.tar.gz");
        let body: &[u8] = b"hello kryos registry test";
        {
            let mut f = std::fs::File::create(&tarball_path).unwrap();
            f.write_all(body).unwrap();
        }
        let manifest = Manifest {
            package: PackageInfo {
                name: "demo".into(),
                version: "0.1.0".into(),
                edition: "2026".into(),
                description: None,
                authors: vec![],
                license: None,
                repository: None,
            },
            dependencies: Default::default(),
            build: Default::default(),
            capabilities: Default::default(),
        };
        let pkg = PublishPackage {
            name: "demo".into(),
            version: "0.1.0".parse().unwrap(),
            tarball_path: tarball_path.clone(),
            manifest,
            caps_badge: None,
        };
        let entry = generate_index_entry(&pkg);
        let expected = format!("sha256:{}", crate::sha256::sha256_hex(body));
        assert!(
            entry.contains(&expected),
            "expected checksum {expected} in entry:\n{entry}"
        );
        // Body sanity: the entry should be a single JSON object that names the
        // package, version, and an empty dependencies object.
        assert!(entry.contains("\"name\": \"demo\""));
        assert!(entry.contains("\"version\": \"0.1.0\""));
        assert!(entry.contains("\"dependencies\": {}"));
        let _ = std::fs::remove_file(&tarball_path);
    }

    #[test]
    fn generate_and_parse_entry_with_caps_round_trips() {
        use crate::caps::CapsBadge;
        use crate::manifest::PackageInfo;
        let badge = CapsBadge::from_caps(vec!["net".into(), "ffi".into()], 2, 3, vec![]);
        let manifest = Manifest {
            package: PackageInfo {
                name: "native-plugin".into(),
                version: "0.1.0".into(),
                edition: "2026".into(),
                description: None,
                authors: vec![],
                license: None,
                repository: None,
            },
            dependencies: Default::default(),
            build: Default::default(),
            capabilities: Default::default(),
        };
        let pkg = PublishPackage {
            name: "native-plugin".into(),
            version: "0.1.0".parse().unwrap(),
            tarball_path: std::env::temp_dir().join("does-not-exist-native-plugin.tar.gz"),
            manifest,
            caps_badge: Some(badge.clone()),
        };
        let entry = generate_index_entry(&pkg);
        assert!(entry.contains("\"capabilities\""), "entry must embed badge:\n{entry}");
        assert!(entry.contains("ffi"));

        // Parse it back (generate_index_entry emits a single multi-line object;
        // serde_json reads the capabilities sub-object regardless of newlines).
        let parsed = parse_index_entry(&entry, "native-plugin").expect("must parse");
        let caps = parsed.capabilities.expect("badge must round-trip");
        assert_eq!(caps, badge);
        assert_eq!(caps.dangerous, vec!["ffi"]);
        assert_eq!(caps.annotation_coverage_pct, 66);
    }

    #[test]
    fn parse_entry_without_caps_yields_none() {
        // Canonical pre-badging entry — no "capabilities" key.
        let line = r#"{"name": "http-router", "version": "0.1.0", "dependencies": {}, "checksum": "sha256:176df653ffa02096dfc3c486afb553040fed2e7e9d00270b3b0ae127a3e99469"}"#;
        let entry = parse_index_entry(line, "http-router").expect("must parse");
        assert!(entry.capabilities.is_none());
    }

    #[test]
    fn parse_single_line_entry_with_caps() {
        let line = r#"{"name": "http-client", "version": "0.3.1", "dependencies": {}, "checksum": "sha256:0000", "capabilities": {"schema":"kryos-caps/1","capabilities":["net"],"dangerous":[],"annotation_coverage_pct":80,"inferred_uncovered":[]}}"#;
        let entry = parse_index_entry(line, "http-client").expect("must parse");
        let caps = entry.capabilities.expect("badge present");
        assert_eq!(caps.capabilities, vec!["net"]);
        assert_eq!(caps.annotation_coverage_pct, 80);
        assert!(!caps.is_dangerous());
    }
}
