//! Lock file generation and parsing (`kryos.lock`).

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::resolve::{PackageSource, ResolvedGraph};

/// A single entry in the lock file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockEntry {
    pub name: String,
    pub version: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
}

/// The full lock file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockFile {
    #[serde(rename = "package")]
    pub packages: Vec<LockEntry>,
}

impl LockFile {
    /// Create a lock file from a resolved dependency graph.
    pub fn from_resolved(graph: &ResolvedGraph) -> Self {
        let mut packages: Vec<LockEntry> = graph
            .packages
            .iter()
            .map(|pkg| LockEntry {
                name: pkg.name.clone(),
                version: pkg.version.to_string(),
                source: match &pkg.source {
                    PackageSource::Remote(s) => s.clone(),
                    PackageSource::Path(p) => format!("path:{p}"),
                },
                checksum: None,
                dependencies: pkg.dependencies.clone(),
            })
            .collect();

        // Sort for deterministic output.
        packages.sort_by(|a, b| a.name.cmp(&b.name).then(a.version.cmp(&b.version)));

        LockFile { packages }
    }

    /// Parse a lock file from a TOML string.
    pub fn from_str(s: &str) -> Result<Self, String> {
        toml::from_str(s).map_err(|e| format!("failed to parse kryos.lock: {e}"))
    }

    /// Read and parse a lock file from disk.
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        Self::from_str(&content)
    }

    /// Serialize to TOML string with a header comment.
    pub fn to_toml(&self) -> Result<String, String> {
        let body = toml::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize kryos.lock: {e}"))?;
        Ok(format!(
            "# kryos.lock — auto-generated, do not edit\n\n{body}"
        ))
    }

    /// Write the lock file to disk.
    pub fn write_to_file(&self, path: &Path) -> Result<(), String> {
        let content = self.to_toml()?;
        std::fs::write(path, content)
            .map_err(|e| format!("failed to write {}: {e}", path.display()))
    }

    /// Look up a locked package by name.
    pub fn get(&self, name: &str) -> Option<&LockEntry> {
        self.packages.iter().find(|p| p.name == name)
    }

    /// Set the checksum for a locked package.
    pub fn set_checksum(&mut self, name: &str, checksum: String) {
        if let Some(entry) = self.packages.iter_mut().find(|p| p.name == name) {
            entry.checksum = Some(checksum);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::ResolvedPackage;
    use crate::semver::Version;

    fn sample_lock() -> LockFile {
        LockFile {
            packages: vec![
                LockEntry {
                    name: "http".into(),
                    version: "0.3.5".into(),
                    source: "github:kryos-lang/http".into(),
                    checksum: Some("sha256:def456".into()),
                    dependencies: vec!["serde".into()],
                },
                LockEntry {
                    name: "serde".into(),
                    version: "1.2.0".into(),
                    source: "github:kryos-lang/serde".into(),
                    checksum: Some("sha256:abc123".into()),
                    dependencies: vec![],
                },
            ],
        }
    }

    #[test]
    fn lock_roundtrip() {
        let lock = sample_lock();
        let toml_str = lock.to_toml().unwrap();
        // Strip the header comment for parsing
        let parsed = LockFile::from_str(&toml_str).unwrap();
        assert_eq!(lock, parsed);
    }

    #[test]
    fn lock_from_resolved_graph() {
        let graph = ResolvedGraph {
            packages: vec![
                ResolvedPackage {
                    name: "serde".into(),
                    version: Version::new(1, 2, 0),
                    source: PackageSource::Remote("github:kryos-lang/serde".into()),
                    dependencies: vec![],
                },
                ResolvedPackage {
                    name: "http".into(),
                    version: Version::new(0, 3, 5),
                    source: PackageSource::Remote("github:kryos-lang/http".into()),
                    dependencies: vec!["serde".into()],
                },
            ],
        };

        let lock = LockFile::from_resolved(&graph);
        assert_eq!(lock.packages.len(), 2);
        // Should be sorted by name
        assert_eq!(lock.packages[0].name, "http");
        assert_eq!(lock.packages[1].name, "serde");
    }

    #[test]
    fn lock_lookup() {
        let lock = sample_lock();
        let serde = lock.get("serde").unwrap();
        assert_eq!(serde.version, "1.2.0");
        assert!(lock.get("nonexistent").is_none());
    }

    #[test]
    fn set_checksum() {
        let mut lock = sample_lock();
        lock.set_checksum("serde", "sha256:new_hash".into());
        assert_eq!(
            lock.get("serde").unwrap().checksum.as_deref(),
            Some("sha256:new_hash")
        );
    }
}
