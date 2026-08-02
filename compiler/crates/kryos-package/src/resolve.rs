//! Dependency resolution — resolves version ranges to concrete versions,
//! detects conflicts and circular dependencies.

use std::collections::{HashMap, HashSet};

use crate::manifest::DepSpec;
use crate::semver::{Version, VersionReq};

/// A resolved package in the dependency graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: Version,
    pub source: PackageSource,
    pub dependencies: Vec<String>,
    /// The `sha256:<hex>` checksum recorded for this exact version in the
    /// registry index, if any. `None` for path dependencies (local
    /// filesystem trust) and for a remote package the registry has no
    /// recorded checksum for -- `fetch::fetch_resolved` treats `None` the
    /// same as a mismatch and refuses to install (see LEDGER item 1b).
    pub checksum: Option<String>,
}

/// Where a package comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageSource {
    /// Remote registry / GitHub source.
    Remote(String),
    /// Local filesystem path.
    Path(String),
}

/// An available version in the registry for resolution.
#[derive(Debug, Clone)]
pub struct AvailablePackage {
    pub name: String,
    pub version: Version,
    pub source: String,
    pub dependencies: HashMap<String, VersionReq>,
    /// The `sha256:<hex>` checksum this exact version is recorded with in
    /// the registry index, if the caller has one. `None` for a source
    /// resolved from a manifest with no registry backing (e.g. a bare path
    /// dependency's transitive deps).
    pub checksum: Option<String>,
}

/// A registry of available packages for resolution.
#[derive(Debug, Default)]
pub struct PackageRegistry {
    /// Map from package name to list of available versions.
    packages: HashMap<String, Vec<AvailablePackage>>,
}

impl PackageRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an available package version.
    pub fn add(&mut self, pkg: AvailablePackage) {
        self.packages.entry(pkg.name.clone()).or_default().push(pkg);
    }

    /// Find the best matching version for a requirement.
    /// Returns the highest version that satisfies the requirement.
    pub fn find_best_match(&self, name: &str, req: &VersionReq) -> Option<&AvailablePackage> {
        self.packages
            .get(name)?
            .iter()
            .filter(|p| req.matches(&p.version))
            .max_by(|a, b| a.version.cmp(&b.version))
    }

    /// Get all available versions for a package.
    pub fn get_versions(&self, name: &str) -> Option<&[AvailablePackage]> {
        self.packages.get(name).map(|v| v.as_slice())
    }
}

/// Errors that can occur during dependency resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// No version of a package satisfies the requirement.
    NoMatchingVersion {
        package: String,
        requirement: String,
    },
    /// Two dependencies require incompatible versions.
    VersionConflict {
        package: String,
        req_a: String,
        from_a: String,
        req_b: String,
        from_b: String,
    },
    /// Circular dependency detected.
    CircularDependency { cycle: Vec<String> },
    /// Package not found in registry.
    PackageNotFound { package: String },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::NoMatchingVersion {
                package,
                requirement,
            } => {
                write!(f, "no version of '{package}' satisfies {requirement}")
            }
            ResolveError::VersionConflict {
                package,
                req_a,
                from_a,
                req_b,
                from_b,
            } => {
                write!(
                    f,
                    "version conflict for '{package}': {from_a} requires {req_a}, \
                     but {from_b} requires {req_b}"
                )
            }
            ResolveError::CircularDependency { cycle } => {
                write!(f, "circular dependency detected: {}", cycle.join(" -> "))
            }
            ResolveError::PackageNotFound { package } => {
                write!(f, "package '{package}' not found in registry")
            }
        }
    }
}

/// The resolved dependency graph.
#[derive(Debug, Default)]
pub struct ResolvedGraph {
    pub packages: Vec<ResolvedPackage>,
}

impl ResolvedGraph {
    /// Look up a resolved package by name.
    pub fn get(&self, name: &str) -> Option<&ResolvedPackage> {
        self.packages.iter().find(|p| p.name == name)
    }
}

/// Resolve a set of top-level dependencies against a registry.
///
/// This performs a simple greedy resolution:
/// 1. For each dependency, find the best matching version.
/// 2. Recursively resolve transitive dependencies.
/// 3. Detect version conflicts (same package required at incompatible versions).
/// 4. Detect circular dependencies via DFS cycle detection.
///
/// Path dependencies read version info and transitive deps from their
/// `kryos.toml` manifests on disk, so they participate fully in the graph.
pub fn resolve(
    root_deps: &HashMap<String, DepSpec>,
    registry: &PackageRegistry,
) -> Result<ResolvedGraph, ResolveError> {
    let mut resolved: HashMap<String, ResolvedPackage> = HashMap::new();
    let mut version_constraints: HashMap<String, (VersionReq, String)> = HashMap::new();
    let mut visiting: HashSet<String> = HashSet::new();
    let mut visit_stack: Vec<String> = Vec::new();

    // Process each root dependency.
    for (name, spec) in root_deps {
        match spec {
            DepSpec::Path { path } => {
                if resolved.contains_key(name) {
                    continue;
                }
                // Read version and transitive deps from the path dep's manifest.
                let (version, transitive_deps) = read_path_dep_info(name, path);
                let dep_names: Vec<String> = transitive_deps.keys().cloned().collect();

                resolved.insert(
                    name.clone(),
                    ResolvedPackage {
                        name: name.clone(),
                        version,
                        source: PackageSource::Path(path.clone()),
                        dependencies: dep_names,
                        checksum: None,
                    },
                );

                // Recursively resolve the path dependency's own dependencies.
                for (dep_name, dep_spec) in &transitive_deps {
                    match dep_spec {
                        DepSpec::Path { path: dep_path } => {
                            if !resolved.contains_key(dep_name) {
                                let (v, _) = read_path_dep_info(dep_name, dep_path);
                                resolved.insert(
                                    dep_name.clone(),
                                    ResolvedPackage {
                                        name: dep_name.clone(),
                                        version: v,
                                        source: PackageSource::Path(dep_path.clone()),
                                        dependencies: Vec::new(),
                                        checksum: None,
                                    },
                                );
                            }
                        }
                        DepSpec::Remote {
                            source,
                            version_req,
                        } => {
                            resolve_recursive(
                                dep_name,
                                version_req,
                                source,
                                name,
                                registry,
                                &mut resolved,
                                &mut version_constraints,
                                &mut visiting,
                                &mut visit_stack,
                            )?;
                        }
                    }
                }
            }
            DepSpec::Remote {
                source,
                version_req,
            } => {
                resolve_recursive(
                    name,
                    version_req,
                    source,
                    "root",
                    registry,
                    &mut resolved,
                    &mut version_constraints,
                    &mut visiting,
                    &mut visit_stack,
                )?;
            }
        }
    }

    Ok(ResolvedGraph {
        packages: resolved.into_values().collect(),
    })
}

/// Read version and dependencies from a path dependency's `kryos.toml`.
///
/// Returns `(version, dependencies)`. Falls back to `0.0.0` with no deps
/// if the manifest cannot be read (the directory may not contain one yet).
fn read_path_dep_info(name: &str, path: &str) -> (Version, HashMap<String, DepSpec>) {
    let manifest_path = std::path::Path::new(path).join("kryos.toml");
    if !manifest_path.exists() {
        return (Version::new(0, 0, 0), HashMap::new());
    }
    match crate::manifest::Manifest::from_file(&manifest_path) {
        Ok(manifest) => {
            let version = manifest
                .package
                .version
                .parse::<Version>()
                .unwrap_or_else(|_| {
                    eprintln!(
                        "warning: invalid version in {}/kryos.toml, using 0.0.0",
                        name
                    );
                    Version::new(0, 0, 0)
                });
            (version, manifest.dependencies)
        }
        Err(e) => {
            eprintln!("warning: cannot read {}/kryos.toml: {e}", name);
            (Version::new(0, 0, 0), HashMap::new())
        }
    }
}

fn resolve_recursive(
    name: &str,
    req: &VersionReq,
    _source_hint: &str,
    required_by: &str,
    registry: &PackageRegistry,
    resolved: &mut HashMap<String, ResolvedPackage>,
    version_constraints: &mut HashMap<String, (VersionReq, String)>,
    visiting: &mut HashSet<String>,
    visit_stack: &mut Vec<String>,
) -> Result<(), ResolveError> {
    // Circular dependency check.
    if visiting.contains(name) {
        let cycle_start = visit_stack.iter().position(|n| n == name).unwrap();
        let mut cycle: Vec<String> = visit_stack[cycle_start..].to_vec();
        cycle.push(name.to_string());
        return Err(ResolveError::CircularDependency { cycle });
    }

    // Already resolved — check for version conflict.
    if let Some(existing) = resolved.get(name) {
        if !req.matches(&existing.version) {
            let (prev_req, prev_from) = version_constraints.get(name).unwrap();
            return Err(ResolveError::VersionConflict {
                package: name.to_string(),
                req_a: prev_req.to_string(),
                from_a: prev_from.clone(),
                req_b: req.to_string(),
                from_b: required_by.to_string(),
            });
        }
        return Ok(());
    }

    // Check registry has this package.
    if registry.get_versions(name).is_none() {
        return Err(ResolveError::PackageNotFound {
            package: name.to_string(),
        });
    }

    // Find best matching version.
    let pkg =
        registry
            .find_best_match(name, req)
            .ok_or_else(|| ResolveError::NoMatchingVersion {
                package: name.to_string(),
                requirement: req.to_string(),
            })?;

    let pkg_version = pkg.version.clone();
    let pkg_source = pkg.source.clone();
    let pkg_deps = pkg.dependencies.clone();
    let pkg_checksum = pkg.checksum.clone();

    // Mark as visiting for cycle detection.
    visiting.insert(name.to_string());
    visit_stack.push(name.to_string());

    // Resolve transitive dependencies.
    let mut dep_names = Vec::new();
    for (dep_name, dep_req) in &pkg_deps {
        dep_names.push(dep_name.clone());
        resolve_recursive(
            dep_name,
            dep_req,
            &pkg_source,
            name,
            registry,
            resolved,
            version_constraints,
            visiting,
            visit_stack,
        )?;
    }

    visiting.remove(name);
    visit_stack.pop();

    // Store resolved package.
    version_constraints.insert(name.to_string(), (req.clone(), required_by.to_string()));
    resolved.insert(
        name.to_string(),
        ResolvedPackage {
            name: name.to_string(),
            version: pkg_version,
            source: PackageSource::Remote(pkg_source),
            dependencies: dep_names,
            checksum: pkg_checksum,
        },
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semver::Op;

    fn make_registry() -> PackageRegistry {
        let mut reg = PackageRegistry::new();
        reg.add(AvailablePackage {
            name: "serde".into(),
            version: "1.0.0".parse().unwrap(),
            source: "github:kryos-lang/serde".into(),
            dependencies: HashMap::new(),
            checksum: None,
        });
        reg.add(AvailablePackage {
            name: "serde".into(),
            version: "1.2.0".parse().unwrap(),
            source: "github:kryos-lang/serde".into(),
            dependencies: HashMap::new(),
            checksum: None,
        });
        reg.add(AvailablePackage {
            name: "http".into(),
            version: "0.3.5".parse().unwrap(),
            source: "github:kryos-lang/http".into(),
            dependencies: HashMap::from([(
                "serde".into(),
                VersionReq::new(Op::Caret, "1.0.0".parse().unwrap()),
            )]),
            checksum: None,
        });
        reg
    }

    #[test]
    fn resolve_simple() {
        let reg = make_registry();
        let mut deps = HashMap::new();
        deps.insert(
            "serde".into(),
            DepSpec::Remote {
                source: "github:kryos-lang/serde".into(),
                version_req: "^1.0.0".parse().unwrap(),
            },
        );

        let graph = resolve(&deps, &reg).unwrap();
        let serde = graph.get("serde").unwrap();
        assert_eq!(serde.version, "1.2.0".parse().unwrap()); // picks highest
    }

    #[test]
    fn resolve_transitive() {
        let reg = make_registry();
        let mut deps = HashMap::new();
        deps.insert(
            "http".into(),
            DepSpec::Remote {
                source: "github:kryos-lang/http".into(),
                version_req: "~0.3.0".parse().unwrap(),
            },
        );

        let graph = resolve(&deps, &reg).unwrap();
        assert!(graph.get("http").is_some());
        assert!(graph.get("serde").is_some()); // transitive
    }

    #[test]
    fn resolve_package_not_found() {
        let reg = PackageRegistry::new();
        let mut deps = HashMap::new();
        deps.insert(
            "nonexistent".into(),
            DepSpec::Remote {
                source: "github:kryos-lang/nonexistent".into(),
                version_req: "^1.0.0".parse().unwrap(),
            },
        );

        let err = resolve(&deps, &reg).unwrap_err();
        assert!(matches!(err, ResolveError::PackageNotFound { .. }));
    }

    #[test]
    fn detect_circular_dependency() {
        let mut reg = PackageRegistry::new();

        // A depends on B, B depends on A
        reg.add(AvailablePackage {
            name: "a".into(),
            version: "1.0.0".parse().unwrap(),
            source: "github:test/a".into(),
            dependencies: HashMap::from([(
                "b".into(),
                VersionReq::new(Op::Caret, "1.0.0".parse().unwrap()),
            )]),
            checksum: None,
        });
        reg.add(AvailablePackage {
            name: "b".into(),
            version: "1.0.0".parse().unwrap(),
            source: "github:test/b".into(),
            dependencies: HashMap::from([(
                "a".into(),
                VersionReq::new(Op::Caret, "1.0.0".parse().unwrap()),
            )]),
            checksum: None,
        });

        let mut deps = HashMap::new();
        deps.insert(
            "a".into(),
            DepSpec::Remote {
                source: "github:test/a".into(),
                version_req: "^1.0.0".parse().unwrap(),
            },
        );

        let err = resolve(&deps, &reg).unwrap_err();
        match err {
            ResolveError::CircularDependency { cycle } => {
                assert!(cycle.contains(&"a".to_string()));
                assert!(cycle.contains(&"b".to_string()));
            }
            other => panic!("expected CircularDependency, got {other:?}"),
        }
    }

    #[test]
    fn detect_version_conflict() {
        let mut reg = PackageRegistry::new();

        reg.add(AvailablePackage {
            name: "serde".into(),
            version: "1.0.0".parse().unwrap(),
            source: "github:kryos-lang/serde".into(),
            dependencies: HashMap::new(),
            checksum: None,
        });
        reg.add(AvailablePackage {
            name: "serde".into(),
            version: "2.0.0".parse().unwrap(),
            source: "github:kryos-lang/serde".into(),
            dependencies: HashMap::new(),
            checksum: None,
        });
        // lib-a requires serde ^1.0.0
        reg.add(AvailablePackage {
            name: "lib-a".into(),
            version: "1.0.0".parse().unwrap(),
            source: "github:test/lib-a".into(),
            dependencies: HashMap::from([(
                "serde".into(),
                VersionReq::new(Op::Caret, "1.0.0".parse().unwrap()),
            )]),
            checksum: None,
        });
        // lib-b requires serde ^2.0.0
        reg.add(AvailablePackage {
            name: "lib-b".into(),
            version: "1.0.0".parse().unwrap(),
            source: "github:test/lib-b".into(),
            dependencies: HashMap::from([(
                "serde".into(),
                VersionReq::new(Op::Caret, "2.0.0".parse().unwrap()),
            )]),
            checksum: None,
        });

        let mut deps = HashMap::new();
        deps.insert(
            "lib-a".into(),
            DepSpec::Remote {
                source: "github:test/lib-a".into(),
                version_req: "^1.0.0".parse().unwrap(),
            },
        );
        deps.insert(
            "lib-b".into(),
            DepSpec::Remote {
                source: "github:test/lib-b".into(),
                version_req: "^1.0.0".parse().unwrap(),
            },
        );

        let err = resolve(&deps, &reg).unwrap_err();
        assert!(matches!(err, ResolveError::VersionConflict { .. }));
    }
}
