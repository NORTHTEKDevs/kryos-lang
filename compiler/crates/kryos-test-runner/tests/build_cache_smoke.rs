//! End-to-end smoke test for the cross-build artifact cache.
//!
//! Drives the actual `kryos` CLI to verify that:
//!   1. Opting in with `--cache` populates `$KRYOS_CACHE_DIR` after the
//!      first build (cold).
//!   2. A second build of the same source on the same target/mode is
//!      restored byte-for-byte from the cache (warm hit) and the resulting
//!      executable still runs correctly.
//!   3. Editing the source invalidates the entry (new key, fresh build).
//!   4. `--no-cache` neither reads nor writes the cache.
//!
//! All cache state is confined to a per-test temp directory via
//! `KRYOS_CACHE_DIR`, so the user's real cache is never touched.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn kryos_binary() -> PathBuf {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for profile in &["release", "debug"] {
        let mut path = base.join("target").join(profile).join("kryos");
        if cfg!(windows) {
            path.set_extension("exe");
        }
        if path.exists() {
            return path;
        }
    }
    panic!("kryos binary not found. Build with `cargo build --release -p kryos-cli` first.");
}

/// Count `.bin` entries in the cache root. Used as a coarse proxy for
/// cache population.
fn count_bin_entries(cache_root: &std::path::Path) -> usize {
    let build_dir = cache_root.join("build");
    let Ok(it) = fs::read_dir(&build_dir) else {
        return 0;
    };
    it.filter_map(Result::ok)
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s == "bin")
                .unwrap_or(false)
        })
        .count()
}

#[test]
fn build_cache_roundtrip_with_cli() {
    // Used to be skipped on Windows back when 'kryos build --release'
    // couldn't link on Windows/MSVC. That path now works end-to-end
    // (see Windows CI), so the test runs on every platform. The
    // executable extension is already handled by the `cfg!(windows)`
    // branch below, and the byte-exact comparison holds because the
    // MSVC linker is deterministic for a given toolchain.
    let kryos = kryos_binary();
    let work = std::env::temp_dir().join(format!(
        "kryos_build_cache_smoke_{}",
        std::process::id()
    ));
    let cache_root = work.join("cache");
    let project = work.join("project");
    fs::create_dir_all(&project).expect("create project dir");
    fs::create_dir_all(&cache_root).expect("create cache dir");

    let src_path = project.join("prog.kry");
    let src_v1 = "fn main() -> i64 {\n    return 7\n}\n";
    fs::write(&src_path, src_v1).expect("write source v1");

    let exe_path = project.join(if cfg!(windows) { "prog.exe" } else { "prog" });

    // --- (1) Cold build with --cache: should produce a cache entry. ---
    let cold = Command::new(&kryos)
        .args(["build", "--cache"])
        .arg(&src_path)
        .arg("-o")
        .arg(&exe_path)
        .env("KRYOS_CACHE_DIR", &cache_root)
        .current_dir(&project)
        .output()
        .expect("run kryos build (cold)");
    assert!(
        cold.status.success(),
        "cold build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&cold.stdout),
        String::from_utf8_lossy(&cold.stderr),
    );
    assert!(exe_path.exists(), "cold build produced no executable");

    assert_eq!(
        count_bin_entries(&cache_root),
        1,
        "expected exactly one cache entry after cold build"
    );

    // Capture the produced executable bytes so we can compare against
    // a cache-restored copy in step (2).
    let cold_bytes = fs::read(&exe_path).expect("read cold exe");
    let cold_exit = Command::new(&exe_path)
        .output()
        .expect("run cold exe")
        .status
        .code();
    assert_eq!(cold_exit, Some(7), "cold exe returned wrong exit code");

    // --- (2) Delete the exe and rebuild: must hit the cache and emit a
    //     byte-identical executable that still returns 7. ---
    fs::remove_file(&exe_path).expect("remove cold exe");
    let warm = Command::new(&kryos)
        .args(["build", "--cache"])
        .arg(&src_path)
        .arg("-o")
        .arg(&exe_path)
        .env("KRYOS_CACHE_DIR", &cache_root)
        .current_dir(&project)
        .output()
        .expect("run kryos build (warm)");
    assert!(
        warm.status.success(),
        "warm build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&warm.stdout),
        String::from_utf8_lossy(&warm.stderr),
    );
    let warm_bytes = fs::read(&exe_path).expect("read warm exe");
    assert_eq!(
        warm_bytes, cold_bytes,
        "warm exe bytes diverge from cold exe — cache must be byte-exact"
    );
    let warm_exit = Command::new(&exe_path)
        .output()
        .expect("run warm exe")
        .status
        .code();
    assert_eq!(
        warm_exit,
        Some(7),
        "cache-restored exe must run correctly (return 7)"
    );

    // --- (3) Edit the source. The cache must miss and a new entry must
    //     appear alongside the original. ---
    let src_v2 = "fn main() -> i64 {\n    return 9\n}\n";
    fs::write(&src_path, src_v2).expect("write source v2");
    let edit = Command::new(&kryos)
        .args(["build", "--cache"])
        .arg(&src_path)
        .arg("-o")
        .arg(&exe_path)
        .env("KRYOS_CACHE_DIR", &cache_root)
        .current_dir(&project)
        .output()
        .expect("run kryos build (edit)");
    assert!(
        edit.status.success(),
        "edited build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&edit.stdout),
        String::from_utf8_lossy(&edit.stderr),
    );
    let edit_exit = Command::new(&exe_path)
        .output()
        .expect("run edited exe")
        .status
        .code();
    assert_eq!(edit_exit, Some(9), "edited program should return 9");

    assert_eq!(
        count_bin_entries(&cache_root),
        2,
        "expected two cache entries after source edit (old + new)"
    );

    // --- (4) `--no-cache` neither reads from nor writes to the cache.
    //     Revert the source and rebuild with --no-cache. The cache count
    //     stays at 2 (no new entry written), and the build still succeeds. ---
    fs::write(&src_path, "fn main() -> i64 {\n    return 11\n}\n").expect("write source v3");
    let no_cache = Command::new(&kryos)
        .args(["build", "--no-cache"])
        .arg(&src_path)
        .arg("-o")
        .arg(&exe_path)
        .env("KRYOS_CACHE_DIR", &cache_root)
        .current_dir(&project)
        .output()
        .expect("run kryos build (no-cache)");
    assert!(
        no_cache.status.success(),
        "--no-cache build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&no_cache.stdout),
        String::from_utf8_lossy(&no_cache.stderr),
    );
    assert_eq!(
        count_bin_entries(&cache_root),
        2,
        "--no-cache must not write a new cache entry"
    );

    // Cleanup.
    fs::remove_dir_all(&work).ok();
}
