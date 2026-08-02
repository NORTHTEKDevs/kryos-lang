//! Integration tests for the package-install checksum verification path.
//!
//! `tools/loop/LEDGER.md` item 1b: `kryos pkg install`/`add` used to never
//! verify a downloaded/cached package's content against anything -- the
//! documented "tarballs are pinned by hash" claim was false. These tests
//! prove the fix both ways: a tampered package is REJECTED, and a
//! legitimate one still installs -- including the exact repro from the
//! ledger (a package already sitting in the shared local cache gets
//! mutated in place, then a second project depends on the same
//! name+version and must not silently reuse the poisoned copy).

use std::io::Write;

use kryos_package::fetch::{self, verify_package_checksum};
use kryos_package::registry::content_checksum;
use kryos_package::resolve::{PackageSource, ResolvedGraph, ResolvedPackage};
use kryos_package::Version;

fn write_fixture_package(dir: &std::path::Path, body: &str) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("kryos.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src").join("main.kry"), body).unwrap();
}

#[test]
fn verify_package_checksum_accepts_matching_content() {
    let dir = std::env::temp_dir().join(format!(
        "kryos-checksum-verify-ok-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    write_fixture_package(&dir, "fn main() { println(\"legit\") }\n");

    let checksum = content_checksum(&dir).unwrap();
    assert!(verify_package_checksum(&dir, Some(&checksum), "fixture", "0.1.0").is_ok());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verify_package_checksum_rejects_tampered_content() {
    let dir = std::env::temp_dir().join(format!(
        "kryos-checksum-verify-tamper-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    write_fixture_package(&dir, "fn main() { println(\"legit\") }\n");

    let good_checksum = content_checksum(&dir).unwrap();

    // Legit content verifies clean.
    assert!(verify_package_checksum(&dir, Some(&good_checksum), "fixture", "0.1.0").is_ok());

    // Tamper with the fetched content in place -- the exact live repro
    // from LEDGER item 1b (mutate a cached package's file after fetch).
    std::fs::write(
        dir.join("src").join("main.kry"),
        "fn main() { println(\"MALICIOUS_INJECTED_CONTENT\") }\n",
    )
    .unwrap();

    let err =
        verify_package_checksum(&dir, Some(&good_checksum), "fixture", "0.1.0").unwrap_err();
    assert!(
        err.contains("checksum mismatch"),
        "expected a checksum-mismatch error, got: {err}"
    );
    // The error must name the expected hash so a human can diagnose it.
    assert!(
        err.contains(&good_checksum),
        "error should name the expected hash: {err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verify_package_checksum_rejects_missing_checksum() {
    let dir = std::env::temp_dir().join(format!(
        "kryos-checksum-verify-missing-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    write_fixture_package(&dir, "fn main() {}\n");

    // No checksum recorded at all (None) -- must be rejected exactly like
    // a mismatch, not silently allowed through. This is the "missing
    // checksum is the same hole with extra steps" requirement.
    let err = verify_package_checksum(&dir, None, "fixture", "0.1.0").unwrap_err();
    assert!(
        err.contains("no checksum"),
        "expected a missing-checksum error, got: {err}"
    );

    // An empty-string checksum -- what the index parser yields for a
    // literally absent "checksum" field -- must be rejected the same way.
    let err2 = verify_package_checksum(&dir, Some(""), "fixture", "0.1.0").unwrap_err();
    assert!(
        err2.contains("no checksum"),
        "expected a missing-checksum error, got: {err2}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fetch_resolved_rejects_a_tampered_cache_entry_and_wipes_it() {
    // Reproduces LEDGER item 1b's exact scenario end to end through
    // `fetch_resolved`, without a real network fetch: pre-populate the
    // shared package cache as if a previous `kryos pkg install` had
    // already fetched it, tamper with the cached copy, then call
    // fetch_resolved again (as a second project depending on the same
    // name+version would) and confirm it is rejected instead of silently
    // reused, and that the poisoned cache entry is removed afterward.
    let cache = fetch::cache_dir();
    std::fs::create_dir_all(&cache).unwrap();
    let name = format!("ledger1b-fixture-{}", std::process::id());
    let version: Version = "0.1.0".parse().unwrap();
    let dest = cache.join(format!("{name}-{version}"));
    let _ = std::fs::remove_dir_all(&dest);
    write_fixture_package(&dest, "fn main() { println(\"legit\") }\n");
    let good_checksum = content_checksum(&dest).unwrap();

    let make_graph = |checksum: &str| ResolvedGraph {
        packages: vec![ResolvedPackage {
            name: name.clone(),
            version: version.clone(),
            source: PackageSource::Remote(format!(
                "github_subdir:test/test/packages/{name}/{version}"
            )),
            dependencies: vec![],
            checksum: Some(checksum.to_string()),
        }],
    };

    // A legitimate cache hit (matching checksum) must succeed and must
    // NOT re-fetch over the network (dest already exists).
    let fetched = fetch::fetch_resolved(&make_graph(&good_checksum))
        .expect("legitimate cache hit must install cleanly");
    assert_eq!(fetched.len(), 1);
    assert!(dest.exists(), "legitimate package must remain cached");

    // Now tamper with the SAME on-disk cache entry, simulating a second
    // project reusing a poisoned/mutated cache -- LEDGER item 1b's exact
    // live repro (`echo MALICIOUS_INJECTED_CONTENT >> cached/src/lib.kry`
    // then a second project's `kryos pkg install` silently reused it).
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(dest.join("src").join("main.kry"))
            .unwrap();
        writeln!(f, "// MALICIOUS_INJECTED_CONTENT").unwrap();
    }

    let err = fetch::fetch_resolved(&make_graph(&good_checksum))
        .expect_err("a tampered cache entry must be rejected, not silently reused");
    assert!(
        err.contains("checksum mismatch"),
        "expected a checksum-mismatch error, got: {err}"
    );
    assert!(
        !dest.exists(),
        "a rejected/tampered cache entry must be removed, not left behind for a later run to trust"
    );

    let _ = std::fs::remove_dir_all(&dest);
}

#[test]
fn fetch_resolved_rejects_a_package_with_no_recorded_checksum() {
    let cache = fetch::cache_dir();
    std::fs::create_dir_all(&cache).unwrap();
    let name = format!("ledger1b-nochecksum-{}", std::process::id());
    let version: Version = "0.1.0".parse().unwrap();
    let dest = cache.join(format!("{name}-{version}"));
    let _ = std::fs::remove_dir_all(&dest);
    write_fixture_package(&dest, "fn main() {}\n");

    let graph = ResolvedGraph {
        packages: vec![ResolvedPackage {
            name: name.clone(),
            version: version.clone(),
            source: PackageSource::Remote(format!(
                "github_subdir:test/test/packages/{name}/{version}"
            )),
            dependencies: vec![],
            checksum: None,
        }],
    };

    let err = fetch::fetch_resolved(&graph)
        .expect_err("a package with no recorded checksum must be refused, not installed");
    assert!(
        err.contains("no checksum"),
        "expected a missing-checksum error, got: {err}"
    );
    assert!(
        !dest.exists(),
        "an unverifiable cache entry must be removed, not left behind"
    );

    let _ = std::fs::remove_dir_all(&dest);
}
