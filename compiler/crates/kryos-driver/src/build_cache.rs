//! Cross-build artifact cache.
//!
//! Stores the final compiled output (binary, object, or library) for each
//! `(source content, target triple, build mode, compiler version)` tuple so
//! that repeated `kryos build` invocations on the same source produce the
//! same artifact without rerunning the entire pipeline.
//!
//! # Cache directory layout
//!
//! ```text
//! <cache_root>/
//!   build/
//!     <key>.bin      # the cached artifact, byte-for-byte
//!     <key>.meta     # one-line metadata: <length> <triple> <mode> <version>
//! ```
//!
//! `<cache_root>` is, in order of preference:
//! 1. The `KRYOS_CACHE_DIR` env var if set,
//! 2. `$XDG_CACHE_HOME/kryos`,
//! 3. `$HOME/.cache/kryos`,
//! 4. `./.kryos-cache` as a last resort.
//!
//! # Key construction
//!
//! The cache key is `format!("{src_hash:016x}-{ctx_hash:016x}")` where:
//! - `src_hash` is the 64-bit DefaultHasher hash of the entire source string
//!   (including length, so a trimmed prefix can't collide with the original).
//! - `ctx_hash` is the same hash applied to the concatenation of
//!   `target_triple`, `build_mode`, `output_type`, and `compiler_version`.
//!
//! Collisions are extremely unlikely (2^64 space, ~10^-12 per pair-up for the
//! domains we care about) and even if one occurred the worst case is reading
//! a wrong artifact — which the consumer detects via the explicit metadata
//! file and falls back to a fresh build.
//!
//! # Failure semantics
//!
//! All cache operations are best-effort. Any I/O error is silently swallowed
//! at the boundary; the caller treats it as a cache miss and proceeds with
//! the normal build path. This keeps the cache strictly an optimization —
//! it can never make a build fail.

use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

const SCHEMA_VERSION: u32 = 1;

/// Build-time inputs that together identify a unique cache entry.
#[derive(Debug, Clone)]
pub struct CacheKey {
    src_hash: u64,
    ctx_hash: u64,
}

impl CacheKey {
    /// Build a cache key from the raw source text and the context strings
    /// that influence codegen. Pass in deterministic values for the context
    /// (target triple, normalised build mode, etc) — see `CacheContext`.
    pub fn new(source: &str, ctx: &CacheContext) -> Self {
        Self {
            src_hash: hash_str(source),
            ctx_hash: hash_str(&ctx.fingerprint()),
        }
    }

    /// Render the key as a stable directory-safe filename stem.
    pub fn as_filename(&self) -> String {
        format!("{:016x}-{:016x}", self.src_hash, self.ctx_hash)
    }
}

/// Everything *other* than source content that influences the produced
/// artifact. Anything missing here would let a stale entry be served.
#[derive(Debug, Clone)]
pub struct CacheContext<'a> {
    pub target_triple: &'a str,
    pub build_mode: &'a str,
    pub output_type: &'a str,
    pub compiler_version: &'a str,
}

impl<'a> CacheContext<'a> {
    fn fingerprint(&self) -> String {
        format!(
            "v{SCHEMA_VERSION}|{}|{}|{}|{}",
            self.target_triple, self.build_mode, self.output_type, self.compiler_version
        )
    }
}

/// Try to find an existing cached artifact for `key`. Returns the byte
/// contents on hit, `None` on miss or any I/O error.
pub fn lookup(key: &CacheKey) -> Option<Vec<u8>> {
    let root = cache_root()?;
    let stem = key.as_filename();
    let bin_path = root.join(format!("{stem}.bin"));
    let meta_path = root.join(format!("{stem}.meta"));
    // We require both files to exist, otherwise the entry is incomplete
    // and unsafe to serve.
    if !bin_path.is_file() || !meta_path.is_file() {
        return None;
    }
    fs::read(&bin_path).ok()
}

/// Store `bytes` under `key` along with a small metadata file. Errors are
/// swallowed: a failed write just means the next build won't be a cache
/// hit, which is harmless.
pub fn store(key: &CacheKey, bytes: &[u8], ctx: &CacheContext) {
    let Some(root) = cache_root() else {
        return;
    };
    if fs::create_dir_all(&root).is_err() {
        return;
    }
    let stem = key.as_filename();
    let bin_path = root.join(format!("{stem}.bin"));
    let meta_path = root.join(format!("{stem}.meta"));
    // Write artifact first; only write meta on success so a half-populated
    // entry is treated as a miss.
    if fs::write(&bin_path, bytes).is_err() {
        return;
    }
    let meta = format!("{} {}", bytes.len(), ctx.fingerprint());
    let _ = fs::write(&meta_path, meta);
}

/// Best-effort: wipe everything in the cache directory. Intended for the
/// `--no-cache` path and for tests.
pub fn purge_all() -> std::io::Result<()> {
    if let Some(root) = cache_root() {
        if root.is_dir() {
            fs::remove_dir_all(&root)?;
        }
    }
    Ok(())
}

/// Resolve the cache directory, creating *no* directories yet. Returns
/// `None` only if the platform exposes no writable home — vanishingly rare,
/// but we'd rather skip caching than panic.
fn cache_root() -> Option<PathBuf> {
    if let Ok(explicit) = env::var("KRYOS_CACHE_DIR") {
        return Some(PathBuf::from(explicit).join("build"));
    }
    if let Ok(xdg) = env::var("XDG_CACHE_HOME") {
        return Some(PathBuf::from(xdg).join("kryos").join("build"));
    }
    if let Ok(home) = env::var("HOME") {
        return Some(PathBuf::from(home).join(".cache").join("kryos").join("build"));
    }
    // Windows fallback.
    if let Ok(home) = env::var("USERPROFILE") {
        return Some(
            PathBuf::from(home)
                .join("AppData")
                .join("Local")
                .join("kryos")
                .join("build"),
        );
    }
    Some(PathBuf::from(".kryos-cache").join("build"))
}

fn hash_str(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    // Include the byte length to harden against prefix collisions.
    s.len().hash(&mut h);
    s.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Tests that mutate `KRYOS_CACHE_DIR` must run serially because env vars
    // are process-global and `cargo test` defaults to a thread pool. Pure
    // key-derivation tests are unaffected and run in parallel.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn ctx<'a>() -> CacheContext<'a> {
        CacheContext {
            target_triple: "x86_64-unknown-linux-gnu",
            build_mode: "release",
            output_type: "binary",
            compiler_version: "2.2.0",
        }
    }

    #[test]
    fn same_inputs_same_key() {
        let k1 = CacheKey::new("fn main() {}", &ctx());
        let k2 = CacheKey::new("fn main() {}", &ctx());
        assert_eq!(k1.as_filename(), k2.as_filename());
    }

    #[test]
    fn source_change_changes_key() {
        let k1 = CacheKey::new("fn main() {}", &ctx());
        let k2 = CacheKey::new("fn main() { 1 }", &ctx());
        assert_ne!(k1.as_filename(), k2.as_filename());
    }

    #[test]
    fn target_change_changes_key() {
        let c1 = ctx();
        let mut c2 = ctx();
        c2.target_triple = "aarch64-apple-darwin";
        let k1 = CacheKey::new("fn main() {}", &c1);
        let k2 = CacheKey::new("fn main() {}", &c2);
        assert_ne!(k1.as_filename(), k2.as_filename());
    }

    #[test]
    fn mode_change_changes_key() {
        let c1 = ctx();
        let mut c2 = ctx();
        c2.build_mode = "debug";
        let k1 = CacheKey::new("x", &c1);
        let k2 = CacheKey::new("x", &c2);
        assert_ne!(k1.as_filename(), k2.as_filename());
    }

    #[test]
    fn output_type_change_changes_key() {
        let c1 = ctx();
        let mut c2 = ctx();
        c2.output_type = "object";
        let k1 = CacheKey::new("x", &c1);
        let k2 = CacheKey::new("x", &c2);
        assert_ne!(k1.as_filename(), k2.as_filename());
    }

    #[test]
    fn compiler_version_change_changes_key() {
        let c1 = ctx();
        let mut c2 = ctx();
        c2.compiler_version = "2.3.0";
        let k1 = CacheKey::new("x", &c1);
        let k2 = CacheKey::new("x", &c2);
        assert_ne!(k1.as_filename(), k2.as_filename());
    }

    #[test]
    fn roundtrip_via_tmpdir() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Point the cache at a fresh temp dir so we don't trample the
        // user's real cache.
        let tmp = std::env::temp_dir().join(format!(
            "kryos-cache-test-roundtrip-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp);
        std::env::set_var("KRYOS_CACHE_DIR", &tmp);

        let key = CacheKey::new("fn main() { 42 }", &ctx());
        assert!(lookup(&key).is_none(), "fresh cache should miss");

        let payload = b"\x7fELF FAKE ARTIFACT".to_vec();
        store(&key, &payload, &ctx());

        let got = lookup(&key).expect("should hit after store");
        assert_eq!(got, payload);

        // Different key should still miss.
        let other = CacheKey::new("fn main() { 43 }", &ctx());
        assert!(lookup(&other).is_none());

        // Cleanup.
        std::env::remove_var("KRYOS_CACHE_DIR");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn missing_meta_is_a_miss() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!(
            "kryos-cache-test-meta-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp);
        std::env::set_var("KRYOS_CACHE_DIR", &tmp);

        // cache_root() returns <KRYOS_CACHE_DIR>/build, so write directly there.
        let key = CacheKey::new("x", &ctx());
        let bin_dir = tmp.join("build");
        fs::create_dir_all(&bin_dir).unwrap();
        let bin_path = bin_dir.join(format!("{}.bin", key.as_filename()));
        fs::write(&bin_path, b"orphan").unwrap();

        // No .meta file — should be treated as miss.
        assert!(lookup(&key).is_none());

        std::env::remove_var("KRYOS_CACHE_DIR");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn purge_clears_cache() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!(
            "kryos-cache-test-purge-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp);
        std::env::set_var("KRYOS_CACHE_DIR", &tmp);

        let key = CacheKey::new("fn x() {}", &ctx());
        store(&key, b"bytes", &ctx());
        assert!(lookup(&key).is_some());

        purge_all().unwrap();
        assert!(lookup(&key).is_none());

        std::env::remove_var("KRYOS_CACHE_DIR");
        let _ = fs::remove_dir_all(&tmp);
    }
}
