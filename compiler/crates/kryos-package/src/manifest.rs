//! kryos.toml manifest parsing and serialization.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use crate::semver::VersionReq;

/// Top-level manifest (kryos.toml).
#[derive(Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub package: PackageInfo,
    #[serde(default)]
    pub dependencies: HashMap<String, DepSpec>,
    #[serde(default)]
    pub capabilities: CapabilitiesConfig,
    #[serde(default)]
    pub build: BuildConfig,
}

/// `[package]` section.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    #[serde(default = "default_edition")]
    pub edition: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
}

fn default_edition() -> String {
    "2026".to_string()
}

/// A dependency specification — either a simple string or an inline table.
///
/// Simple string form: `"github:kryos-lang/serde@^1.0.0"`
/// Table form: `{ path = "../local-lib" }` or `{ git = "...", version = "^1.0.0" }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepSpec {
    /// Remote dependency: `github:org/repo@version-req`
    Remote {
        source: String,
        version_req: VersionReq,
    },
    /// Path dependency: `{ path = "../local-lib" }`
    Path { path: String },
}

impl fmt::Display for DepSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DepSpec::Remote {
                source,
                version_req,
            } => write!(f, "{source}@{version_req}"),
            DepSpec::Path { path } => write!(f, "path:{path}"),
        }
    }
}

impl Serialize for DepSpec {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            DepSpec::Remote {
                source,
                version_req,
            } => {
                let s = format!("{source}@{version_req}");
                serializer.serialize_str(&s)
            }
            DepSpec::Path { path } => {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("path", path)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for DepSpec {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de;

        struct DepSpecVisitor;

        impl<'de> de::Visitor<'de> for DepSpecVisitor {
            type Value = DepSpec;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(
                    "a dependency string like \"github:org/repo@^1.0.0\" or a table with 'path'",
                )
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                parse_dep_string(value).map_err(de::Error::custom)
            }

            fn visit_map<M: de::MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut path: Option<String> = None;
                let mut git: Option<String> = None;
                let mut version: Option<String> = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "path" => path = Some(map.next_value()?),
                        "git" => git = Some(map.next_value()?),
                        "version" => version = Some(map.next_value()?),
                        other => {
                            // Skip unknown keys
                            let _ = map.next_value::<toml::Value>()?;
                            return Err(de::Error::unknown_field(
                                other,
                                &["path", "git", "version"],
                            ));
                        }
                    }
                }

                if let Some(p) = path {
                    Ok(DepSpec::Path { path: p })
                } else if let Some(g) = git {
                    let vr = version.ok_or_else(|| de::Error::missing_field("version"))?;
                    let version_req: VersionReq = vr.parse().map_err(de::Error::custom)?;
                    Ok(DepSpec::Remote {
                        source: g,
                        version_req,
                    })
                } else {
                    Err(de::Error::custom(
                        "dependency table must have either 'path' or 'git' field",
                    ))
                }
            }
        }

        deserializer.deserialize_any(DepSpecVisitor)
    }
}

/// Parse a dependency string like `"github:kryos-lang/serde@^1.0.0"`,
/// `"path:../mylib"`, or a bare relative/absolute path (`./foo`, `../foo`, `/abs`).
pub fn parse_dep_string(s: &str) -> Result<DepSpec, String> {
    // Explicit path: prefix
    if let Some(rest) = s.strip_prefix("path:") {
        return Ok(DepSpec::Path {
            path: rest.to_string(),
        });
    }
    // Bare path forms
    if s.starts_with("./") || s.starts_with("../") || s.starts_with('/') {
        return Ok(DepSpec::Path {
            path: s.to_string(),
        });
    }
    if let Some(at_idx) = s.rfind('@') {
        let source = &s[..at_idx];
        let version_str = &s[at_idx + 1..];
        let version_req: VersionReq = version_str.parse()?;
        Ok(DepSpec::Remote {
            source: source.to_string(),
            version_req,
        })
    } else if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        // Bare registry name (no `@version`) — default to the wildcard
        // requirement "*" so the resolver picks the latest version.
        let version_req: VersionReq = "*".parse()?;
        Ok(DepSpec::Remote {
            source: s.to_string(),
            version_req,
        })
    } else if let Ok(version_req) = s.parse::<VersionReq>() {
        // Pure version requirement like "*", "^0.1.0", ">=1.0.0".
        // The dep name (TOML key) will serve as the registry lookup key;
        // source is left empty so install() resolves it via the registry.
        Ok(DepSpec::Remote {
            source: String::new(),
            version_req,
        })
    } else {
        Err(format!(
            "invalid dependency spec: expected 'name', 'name@version', 'path:<dir>', or './<dir>', got '{s}'"
        ))
    }
}

/// `[capabilities]` section.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CapabilitiesConfig {
    #[serde(default)]
    pub allowed: Vec<String>,
}

/// `[build]` section.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BuildConfig {
    #[serde(default = "default_target")]
    pub target: String,
    #[serde(default = "default_optimization")]
    pub optimization: String,
    #[serde(default)]
    pub output: Option<String>,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            target: default_target(),
            optimization: default_optimization(),
            output: None,
        }
    }
}

fn default_target() -> String {
    "native".to_string()
}

fn default_optimization() -> String {
    "dev".to_string()
}

impl Manifest {
    /// Parse a `kryos.toml` from a string.
    pub fn from_str(s: &str) -> Result<Self, String> {
        toml::from_str(s).map_err(|e| format!("failed to parse kryos.toml: {e}"))
    }

    /// Read and parse a `kryos.toml` from a file path.
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        Self::from_str(&content)
    }

    /// Serialize back to TOML.
    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string_pretty(self).map_err(|e| format!("failed to serialize manifest: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_manifest() {
        let toml_str = r#"
[package]
name = "my-project"
version = "1.0.0"
edition = "2026"

[dependencies]
serde = "github:kryos-lang/serde@^1.0.0"
http = "github:kryos-lang/http@~0.3.0"

[capabilities]
allowed = ["net", "io"]

[build]
target = "native"
optimization = "release"
"#;
        let manifest = Manifest::from_str(toml_str).unwrap();
        assert_eq!(manifest.package.name, "my-project");
        assert_eq!(manifest.package.version, "1.0.0");
        assert_eq!(manifest.dependencies.len(), 2);
        assert_eq!(manifest.capabilities.allowed, vec!["net", "io"]);
        assert_eq!(manifest.build.target, "native");
        assert_eq!(manifest.build.optimization, "release");
    }

    #[test]
    fn parse_manifest_with_path_dep() {
        let toml_str = r#"
[package]
name = "my-project"
version = "1.0.0"

[dependencies]
local-lib = { path = "../local-lib" }
"#;
        let manifest = Manifest::from_str(toml_str).unwrap();
        match &manifest.dependencies["local-lib"] {
            DepSpec::Path { path } => assert_eq!(path, "../local-lib"),
            other => panic!("expected Path dep, got {other:?}"),
        }
    }

    #[test]
    fn parse_minimal_manifest() {
        let toml_str = r#"
[package]
name = "tiny"
version = "0.1.0"
"#;
        let manifest = Manifest::from_str(toml_str).unwrap();
        assert_eq!(manifest.package.name, "tiny");
        assert!(manifest.dependencies.is_empty());
        assert_eq!(manifest.build.target, "native");
        assert_eq!(manifest.build.optimization, "dev");
        assert_eq!(manifest.package.edition, "2026");
    }
}
