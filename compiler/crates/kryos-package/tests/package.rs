//! Integration tests for kryos-package.

use kryos_package::{
    resolve, AvailablePackage, DepSpec, LockEntry, LockFile, Manifest, Op, PackageRegistry,
    ResolveError, Version, VersionReq,
};
use std::collections::HashMap;

// ─── Manifest parsing ───────────────────────────────────────────────

#[test]
fn parse_valid_manifest() {
    let toml_str = r#"
[package]
name = "my-project"
version = "1.0.0"
edition = "2026"

[dependencies]
serde = "github:kryos-lang/serde@^1.0.0"
http = "github:kryos-lang/http@~0.3.0"
local-lib = { path = "../local-lib" }

[capabilities]
allowed = ["net", "io", "ffi"]

[build]
target = "native"
optimization = "release"
"#;
    let m = Manifest::from_str(toml_str).unwrap();
    assert_eq!(m.package.name, "my-project");
    assert_eq!(m.package.version, "1.0.0");
    assert_eq!(m.package.edition, "2026");
    assert_eq!(m.dependencies.len(), 3);
    assert_eq!(m.capabilities.allowed, vec!["net", "io", "ffi"]);
    assert_eq!(m.build.target, "native");
    assert_eq!(m.build.optimization, "release");
}

#[test]
fn parse_manifest_all_fields() {
    let toml_str = r#"
[package]
name = "full-project"
version = "2.1.0"
edition = "2026"
description = "A fully-specified project"
authors = ["Alice", "Bob"]
license = "MIT"
repository = "https://github.com/kryos-lang/full-project"

[dependencies]
serde = "github:kryos-lang/serde@^1.0.0"
http = "github:kryos-lang/http@~0.3.0"
local-lib = { path = "../local-lib" }

[capabilities]
allowed = ["net", "io", "ffi"]

[build]
target = "wasm32"
optimization = "release"
"#;
    let m = Manifest::from_str(toml_str).unwrap();
    assert_eq!(
        m.package.description.as_deref(),
        Some("A fully-specified project")
    );
    assert_eq!(m.package.authors, vec!["Alice", "Bob"]);
    assert_eq!(m.package.license.as_deref(), Some("MIT"));
    assert_eq!(
        m.package.repository.as_deref(),
        Some("https://github.com/kryos-lang/full-project")
    );
    assert_eq!(m.build.target, "wasm32");
}

#[test]
fn parse_manifest_minimal() {
    let toml_str = r#"
[package]
name = "tiny"
version = "0.1.0"
"#;
    let m = Manifest::from_str(toml_str).unwrap();
    assert_eq!(m.package.name, "tiny");
    assert_eq!(m.package.version, "0.1.0");
    assert_eq!(m.package.edition, "2026"); // default
    assert!(m.dependencies.is_empty());
    assert!(m.capabilities.allowed.is_empty());
    assert_eq!(m.build.target, "native"); // default
    assert_eq!(m.build.optimization, "dev"); // default
}

// ─── Semver parsing ─────────────────────────────────────────────────

#[test]
fn semver_parse_basic() {
    let v: Version = "1.0.0".parse().unwrap();
    assert_eq!(v, Version::new(1, 0, 0));
    assert_eq!(v.pre, None);
}

#[test]
fn semver_parse_with_pre_release() {
    let v: Version = "1.2.3-beta.1".parse().unwrap();
    assert_eq!(v.major, 1);
    assert_eq!(v.minor, 2);
    assert_eq!(v.patch, 3);
    assert_eq!(v.pre.as_deref(), Some("beta.1"));
}

#[test]
fn semver_display() {
    let v = Version::new(3, 4, 5);
    assert_eq!(v.to_string(), "3.4.5");

    let v2 = Version::new(1, 0, 0).with_pre("rc.1");
    assert_eq!(v2.to_string(), "1.0.0-rc.1");
}

// ─── Semver ranges ──────────────────────────────────────────────────

#[test]
fn caret_range_matches() {
    let req: VersionReq = "^1.0.0".parse().unwrap();
    assert!(req.matches(&"1.0.0".parse().unwrap()));
    assert!(req.matches(&"1.5.0".parse().unwrap()));
    assert!(req.matches(&"1.99.99".parse().unwrap()));
    assert!(!req.matches(&"2.0.0".parse().unwrap()));
    assert!(!req.matches(&"0.9.9".parse().unwrap()));
}

#[test]
fn tilde_range_matches() {
    let req: VersionReq = "~1.2.0".parse().unwrap();
    assert!(req.matches(&"1.2.0".parse().unwrap()));
    assert!(req.matches(&"1.2.5".parse().unwrap()));
    assert!(req.matches(&"1.2.99".parse().unwrap()));
    assert!(!req.matches(&"1.3.0".parse().unwrap()));
    assert!(!req.matches(&"1.1.0".parse().unwrap()));
    assert!(!req.matches(&"2.0.0".parse().unwrap()));
}

#[test]
fn exact_range_matches() {
    let req: VersionReq = "=1.0.0".parse().unwrap();
    assert!(req.matches(&"1.0.0".parse().unwrap()));
    assert!(!req.matches(&"1.0.1".parse().unwrap()));
    assert!(!req.matches(&"0.9.9".parse().unwrap()));
    assert!(!req.matches(&"2.0.0".parse().unwrap()));
}

#[test]
fn comparison_ranges() {
    let ge: VersionReq = ">=1.0.0".parse().unwrap();
    assert!(ge.matches(&"1.0.0".parse().unwrap()));
    assert!(ge.matches(&"5.0.0".parse().unwrap()));
    assert!(!ge.matches(&"0.9.9".parse().unwrap()));

    let lt: VersionReq = "<2.0.0".parse().unwrap();
    assert!(lt.matches(&"1.99.99".parse().unwrap()));
    assert!(!lt.matches(&"2.0.0".parse().unwrap()));
    assert!(!lt.matches(&"3.0.0".parse().unwrap()));
}

// ─── DepSpec parsing ────────────────────────────────────────────────

#[test]
fn dep_spec_github_string() {
    let toml_str = r#"
[package]
name = "test"
version = "1.0.0"

[dependencies]
serde = "github:kryos-lang/serde@^1.0.0"
"#;
    let m = Manifest::from_str(toml_str).unwrap();
    match &m.dependencies["serde"] {
        DepSpec::Remote {
            source,
            version_req,
        } => {
            assert_eq!(source, "github:kryos-lang/serde");
            assert_eq!(version_req.op, Op::Caret);
            assert_eq!(version_req.version, Version::new(1, 0, 0));
        }
        other => panic!("expected Remote, got {other:?}"),
    }
}

#[test]
fn dep_spec_path_table() {
    let toml_str = r#"
[package]
name = "test"
version = "1.0.0"

[dependencies]
local-lib = { path = "../local-lib" }
"#;
    let m = Manifest::from_str(toml_str).unwrap();
    match &m.dependencies["local-lib"] {
        DepSpec::Path { path } => assert_eq!(path, "../local-lib"),
        other => panic!("expected Path, got {other:?}"),
    }
}

#[test]
fn dep_spec_version_ranges() {
    let toml_str = r#"
[package]
name = "test"
version = "1.0.0"

[dependencies]
a = "github:test/a@^1.0.0"
b = "github:test/b@~0.3.0"
c = "github:test/c@=2.0.0"
d = "github:test/d@>=1.5.0"
"#;
    let m = Manifest::from_str(toml_str).unwrap();

    match &m.dependencies["a"] {
        DepSpec::Remote { version_req, .. } => assert_eq!(version_req.op, Op::Caret),
        other => panic!("expected Remote, got {other:?}"),
    }
    match &m.dependencies["b"] {
        DepSpec::Remote { version_req, .. } => assert_eq!(version_req.op, Op::Tilde),
        other => panic!("expected Remote, got {other:?}"),
    }
    match &m.dependencies["c"] {
        DepSpec::Remote { version_req, .. } => assert_eq!(version_req.op, Op::Exact),
        other => panic!("expected Remote, got {other:?}"),
    }
    match &m.dependencies["d"] {
        DepSpec::Remote { version_req, .. } => assert_eq!(version_req.op, Op::GreaterEq),
        other => panic!("expected Remote, got {other:?}"),
    }
}

// ─── Lock file ──────────────────────────────────────────────────────

#[test]
fn lock_file_roundtrip() {
    let lock = LockFile {
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
    };

    let toml_str = lock.to_toml().unwrap();
    assert!(toml_str.contains("kryos.lock"));
    assert!(toml_str.contains("auto-generated"));

    let parsed = LockFile::from_str(&toml_str).unwrap();
    assert_eq!(lock, parsed);
}

#[test]
fn lock_file_from_toml_string() {
    let toml_str = r#"
[[package]]
name = "serde"
version = "1.2.0"
source = "github:kryos-lang/serde"
checksum = "sha256:abc123"

[[package]]
name = "http"
version = "0.3.5"
source = "github:kryos-lang/http"
checksum = "sha256:def456"
dependencies = ["serde"]
"#;
    let lock = LockFile::from_str(toml_str).unwrap();
    assert_eq!(lock.packages.len(), 2);
    assert_eq!(lock.get("serde").unwrap().version, "1.2.0");
    assert_eq!(lock.get("http").unwrap().dependencies, vec!["serde"]);
}

// ─── Resolution ─────────────────────────────────────────────────────

fn make_test_registry() -> PackageRegistry {
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
        name: "serde".into(),
        version: "2.0.0".parse().unwrap(),
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
fn circular_dependency_detection() {
    let mut reg = PackageRegistry::new();
    reg.add(AvailablePackage {
        name: "cycle-a".into(),
        version: "1.0.0".parse().unwrap(),
        source: "github:test/cycle-a".into(),
        dependencies: HashMap::from([(
            "cycle-b".into(),
            VersionReq::new(Op::Caret, "1.0.0".parse().unwrap()),
        )]),
        checksum: None,
    });
    reg.add(AvailablePackage {
        name: "cycle-b".into(),
        version: "1.0.0".parse().unwrap(),
        source: "github:test/cycle-b".into(),
        dependencies: HashMap::from([(
            "cycle-a".into(),
            VersionReq::new(Op::Caret, "1.0.0".parse().unwrap()),
        )]),
        checksum: None,
    });

    let mut deps = HashMap::new();
    deps.insert(
        "cycle-a".into(),
        DepSpec::Remote {
            source: "github:test/cycle-a".into(),
            version_req: "^1.0.0".parse().unwrap(),
        },
    );

    let err = resolve(&deps, &reg).unwrap_err();
    match err {
        ResolveError::CircularDependency { cycle } => {
            assert!(cycle.len() >= 2);
            assert!(cycle.contains(&"cycle-a".to_string()));
            assert!(cycle.contains(&"cycle-b".to_string()));
        }
        other => panic!("expected CircularDependency, got {other}"),
    }
}

#[test]
fn version_conflict_detection() {
    let mut reg = make_test_registry();

    // lib-x requires serde ^1.0.0
    reg.add(AvailablePackage {
        name: "lib-x".into(),
        version: "1.0.0".parse().unwrap(),
        source: "github:test/lib-x".into(),
        dependencies: HashMap::from([(
            "serde".into(),
            VersionReq::new(Op::Caret, "1.0.0".parse().unwrap()),
        )]),
        checksum: None,
    });
    // lib-y requires serde ^2.0.0
    reg.add(AvailablePackage {
        name: "lib-y".into(),
        version: "1.0.0".parse().unwrap(),
        source: "github:test/lib-y".into(),
        dependencies: HashMap::from([(
            "serde".into(),
            VersionReq::new(Op::Caret, "2.0.0".parse().unwrap()),
        )]),
        checksum: None,
    });

    let mut deps = HashMap::new();
    deps.insert(
        "lib-x".into(),
        DepSpec::Remote {
            source: "github:test/lib-x".into(),
            version_req: "^1.0.0".parse().unwrap(),
        },
    );
    deps.insert(
        "lib-y".into(),
        DepSpec::Remote {
            source: "github:test/lib-y".into(),
            version_req: "^1.0.0".parse().unwrap(),
        },
    );

    let err = resolve(&deps, &reg).unwrap_err();
    match err {
        ResolveError::VersionConflict { package, .. } => {
            assert_eq!(package, "serde");
        }
        other => panic!("expected VersionConflict, got {other}"),
    }
}

#[test]
fn resolve_picks_highest_matching() {
    let reg = make_test_registry();
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
    // Should pick 1.2.0 (highest in ^1.0.0 range, not 2.0.0)
    assert_eq!(serde.version, Version::new(1, 2, 0));
}

#[test]
fn resolve_transitive_deps() {
    let reg = make_test_registry();
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
    assert!(graph.get("serde").is_some()); // pulled in transitively
}
