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
    /// `sha256:<hex>` canonical content hash (see [`content_checksum`]) of
    /// this package's `kryos.toml` + `src/` + `stdlib/` files. This is the
    /// value written into the registry index's `checksum` field, and the
    /// value `kryos pkg install` recomputes over a fetched package and
    /// compares against before trusting it (LEDGER item 1b).
    pub content_checksum: String,
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
    collect_kry_files(&src_dir, &src_dir, "src", &mut files)?;

    // Include stdlib/ if it exists in the project.
    let stdlib_dir = project_dir.join("stdlib");
    if stdlib_dir.exists() {
        collect_kry_files(&stdlib_dir, &stdlib_dir, "stdlib", &mut files)?;
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

    // Canonical content hash of exactly what was just packed -- this is
    // the value published to the registry index and the value `kryos pkg
    // install` will recompute over the fetched copy and verify against.
    let content_checksum = content_checksum(project_dir)?;

    Ok(PublishPackage {
        name,
        version,
        tarball_path,
        manifest,
        caps_badge,
        content_checksum,
    })
}

/// Canonical content-hash checksum of a package directory: `kryos.toml`
/// plus every `.kry` file under `src/` and `stdlib/` (if present), hashed
/// in a deterministic order (sorted by `/`-normalized relative path, so
/// the result does not depend on platform path separators or filesystem
/// iteration order).
///
/// This is the single source of truth for a package's content hash on
/// BOTH sides of the registry: `pack()` computes it at publish time (it
/// becomes the index entry's `checksum` field) and `fetch::fetch_resolved`
/// recomputes it over a fetched/cached package directory before trusting
/// it. Previously the published checksum hashed a placeholder "tarball"
/// (really just a text listing of file names, not their content) that
/// nothing on the install side ever re-derived or compared against --
/// see `tools/loop/LEDGER.md` item 1b for the full history.
pub fn content_checksum(project_dir: &Path) -> Result<String, String> {
    let mut files: Vec<(PathBuf, String)> = Vec::new();

    let manifest_path = project_dir.join("kryos.toml");
    if manifest_path.exists() {
        files.push((manifest_path, "kryos.toml".to_string()));
    }

    let src_dir = project_dir.join("src");
    if src_dir.exists() {
        collect_kry_files(&src_dir, &src_dir, "src", &mut files)?;
    }

    let stdlib_dir = project_dir.join("stdlib");
    if stdlib_dir.exists() {
        collect_kry_files(&stdlib_dir, &stdlib_dir, "stdlib", &mut files)?;
    }

    if files.is_empty() {
        return Err(format!(
            "no kryos.toml or .kry source files found under {} -- nothing to checksum",
            project_dir.display()
        ));
    }

    // Deterministic order regardless of `read_dir` iteration order.
    files.sort_by(|a, b| a.1.cmp(&b.1));

    let mut buf = Vec::new();
    for (abs, rel) in &files {
        let bytes =
            std::fs::read(abs).map_err(|e| format!("failed to read {}: {e}", abs.display()))?;
        // Length-prefix both the path and the content so no ambiguity is
        // introduced by concatenating variable-length fields back to back.
        buf.extend_from_slice(&(rel.len() as u32).to_le_bytes());
        buf.extend_from_slice(rel.as_bytes());
        buf.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        buf.extend_from_slice(&bytes);
    }

    Ok(format!("sha256:{}", crate::sha256::sha256_hex(&buf)))
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
/// The checksum is `pkg.content_checksum` -- the canonical `sha256:<hex>`
/// hash of the package's actual `kryos.toml` + `src/` + `stdlib/` content
/// (see [`content_checksum`]), computed by `pack()` at publish time. This
/// is the checksum `kryos pkg install` verifies a fetched package against
/// before trusting it (LEDGER item 1b) -- it previously hashed the
/// placeholder "tarball" file instead (a text listing of file names, not
/// their content), a value nothing on the install side ever re-derived.
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

    let checksum = pkg.content_checksum.clone();

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
                .args([
                    "-c",
                    "credential.helper=",
                    "-c",
                    "core.askpass=",
                    "pull",
                    "--ff-only",
                ])
                .current_dir(&index_dir)
                // Fail fast instead of hanging on an interactive/GUI
                // credential prompt if the registry remote becomes
                // unreachable/private -- see fetch::clone_and_guard's comment
                // for why all three flags together are required.
                .env("GIT_TERMINAL_PROMPT", "0")
                .output()
                .map_err(|e| format!("git pull failed: {e}"))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("registry sync failed: {stderr}"));
            }
        } else {
            // Clone fresh.
            let output = std::process::Command::new("git")
                .args([
                    "-c",
                    "credential.helper=",
                    "-c",
                    "core.askpass=",
                    "clone",
                    "--depth",
                    "1",
                    &self.config.url,
                ])
                .arg(&index_dir)
                // Fail fast instead of hanging on an interactive/GUI
                // credential prompt if the registry remote becomes
                // unreachable/private -- see fetch::clone_and_guard's comment
                // for why all three flags together are required.
                .env("GIT_TERMINAL_PROMPT", "0")
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
    prefix: &str,
    files: &mut Vec<(PathBuf, String)>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("failed to read directory {}: {e}", dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("directory entry error: {e}"))?;
        let path = entry.path();

        if path.is_dir() {
            collect_kry_files(&path, base, prefix, files)?;
        } else if path.extension().map(|e| e == "kry").unwrap_or(false) {
            let rel = path
                .strip_prefix(base)
                .map_err(|e| format!("path prefix error: {e}"))?;
            // Normalize to `/` regardless of host OS so the recorded
            // (and later hashed) relative path is platform-independent --
            // a `\`-joined path here would make `content_checksum` diverge
            // between Windows and Unix for the identical file tree.
            let rel_norm = rel.to_string_lossy().replace('\\', "/");
            files.push((path.clone(), format!("{prefix}/{rel_norm}")));
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
    fn generate_index_entry_emits_pkg_content_checksum_verbatim() {
        // generate_index_entry must publish exactly `pkg.content_checksum`
        // and must NOT re-derive anything from `tarball_path` -- the
        // tarball is a placeholder text listing of file NAMES, not their
        // content, so hashing it (the old behavior) never actually pinned
        // the package's real bytes. See LEDGER item 1b.
        use crate::manifest::PackageInfo;
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
        let expected =
            "sha256:deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string();
        let pkg = PublishPackage {
            name: "demo".into(),
            version: "0.1.0".parse().unwrap(),
            // Deliberately points at a file that does not exist -- proves
            // generate_index_entry never reads tarball_path for the checksum.
            tarball_path: std::env::temp_dir().join("does-not-exist-demo.tar.gz"),
            manifest,
            caps_badge: None,
            content_checksum: expected.clone(),
        };
        let entry = generate_index_entry(&pkg);
        assert!(
            entry.contains(&expected),
            "expected checksum {expected} in entry:\n{entry}"
        );
        // Body sanity: the entry should be a single JSON object that names the
        // package, version, and an empty dependencies object.
        assert!(entry.contains("\"name\": \"demo\""));
        assert!(entry.contains("\"version\": \"0.1.0\""));
        assert!(entry.contains("\"dependencies\": {}"));
    }

    #[test]
    fn content_checksum_is_deterministic_and_content_sensitive() {
        // Build a tiny on-disk package (kryos.toml + src/main.kry) --
        // exactly the shape content_checksum (and pack()) walk.
        let dir = std::env::temp_dir()
            .join(format!("kryos-content-checksum-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let src_dir = dir.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(
            dir.join("kryos.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(src_dir.join("main.kry"), "fn main() {}\n").unwrap();

        let original = content_checksum(&dir).unwrap();
        // Recomputing over identical, untouched content must be stable --
        // a non-deterministic checksum would make every install a false
        // mismatch.
        assert_eq!(content_checksum(&dir).unwrap(), original);

        // Tamper with the source content -- the exact class of tamper
        // `kryos pkg install` must reject (LEDGER item 1b's live repro).
        std::fs::write(
            src_dir.join("main.kry"),
            "fn main() { println(\"MALICIOUS_INJECTED_CONTENT\") }\n",
        )
        .unwrap();
        let tampered = content_checksum(&dir).unwrap();
        assert_ne!(
            original, tampered,
            "tampering with a source file must change the content checksum"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn content_checksum_distinguishes_stdlib_from_src_prefix() {
        // `collect_kry_files` takes an explicit `prefix` argument (`"src"`
        // vs `"stdlib"`) specifically so a file's hashed identity records
        // WHICH directory it came from, not just its filename. If the
        // prefix were ever hardcoded regardless of the caller (the exact
        // bug LEDGER item 1b fixed while in this file), a `stdlib/foo.kry`
        // and a `src/foo.kry` with byte-identical content would hash
        // IDENTICALLY -- silently letting a package smuggle content from
        // one tree into the other without changing the recorded checksum.
        let src_only = std::env::temp_dir().join(format!(
            "kryos-checksum-prefix-src-{}",
            std::process::id()
        ));
        let stdlib_only = std::env::temp_dir().join(format!(
            "kryos-checksum-prefix-stdlib-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&src_only);
        let _ = std::fs::remove_dir_all(&stdlib_only);

        let body = "fn helper() {}\n";
        std::fs::create_dir_all(src_only.join("src")).unwrap();
        std::fs::write(src_only.join("src").join("foo.kry"), body).unwrap();

        std::fs::create_dir_all(stdlib_only.join("stdlib")).unwrap();
        std::fs::write(stdlib_only.join("stdlib").join("foo.kry"), body).unwrap();

        let src_checksum = content_checksum(&src_only).unwrap();
        let stdlib_checksum = content_checksum(&stdlib_only).unwrap();
        assert_ne!(
            src_checksum, stdlib_checksum,
            "a `src/foo.kry` and a `stdlib/foo.kry` with identical bytes must NOT hash the \
             same -- the prefix must be part of the hashed path identity"
        );

        let _ = std::fs::remove_dir_all(&src_only);
        let _ = std::fs::remove_dir_all(&stdlib_only);
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
            content_checksum: "sha256:0000000000000000000000000000000000000000000000000000000000000000".into(),
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
