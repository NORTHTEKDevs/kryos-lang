//! Package fetching — downloads remote dependencies to a local cache.
//!
//! For MVP, supports `github:org/repo` sources by cloning repositories
//! to `~/.kryos/packages/<name>-<version>/`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::resolve::{PackageSource, ResolvedGraph};

/// Get the Kryos package cache directory.
pub fn cache_dir() -> PathBuf {
    dirs_or_fallback().join("packages")
}

fn dirs_or_fallback() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        PathBuf::from(home).join(".kryos")
    } else {
        PathBuf::from(".kryos")
    }
}

/// Fetch all remote dependencies from a resolved graph to the local cache.
/// Returns the list of (name, local_path) pairs for fetched packages.
///
/// Every `Remote` package is checksum-verified against the registry-index
/// checksum recorded on its `ResolvedPackage` (`pkg.checksum`) BEFORE it is
/// trusted -- including on a cache hit, so a package tampered with (or
/// swapped out) on disk after a previous install can never be silently
/// reused. A missing checksum is rejected the same as a mismatch. See
/// `tools/loop/LEDGER.md` item 1b for the full history: previously nothing
/// on this path ever verified anything.
pub fn fetch_resolved(graph: &ResolvedGraph) -> Result<Vec<(String, PathBuf)>, String> {
    let cache = cache_dir();
    std::fs::create_dir_all(&cache).map_err(|e| format!("failed to create cache dir: {e}"))?;

    let mut fetched = Vec::new();

    for pkg in &graph.packages {
        match &pkg.source {
            PackageSource::Remote(source) => {
                let dest = cache.join(format!("{}-{}", pkg.name, pkg.version));
                if !dest.exists() {
                    if let Err(e) = fetch_github(source, &dest) {
                        // A failed fetch (including a rejected symlink
                        // entry) can leave a partially-copied directory
                        // behind; don't let a later run mistake it for a
                        // successfully cached package.
                        let _ = std::fs::remove_dir_all(&dest);
                        return Err(e);
                    }
                }

                if let Err(e) = verify_package_checksum(
                    &dest,
                    pkg.checksum.as_deref(),
                    &pkg.name,
                    &pkg.version.to_string(),
                ) {
                    // Never leave a rejected/tampered package sitting in the
                    // shared cache -- a later `install` (this project or any
                    // other) must re-fetch and re-verify from scratch rather
                    // than silently reuse it.
                    let _ = std::fs::remove_dir_all(&dest);
                    return Err(e);
                }

                fetched.push((pkg.name.clone(), dest));
            }
            PackageSource::Path(path) => {
                fetched.push((pkg.name.clone(), PathBuf::from(path)));
            }
        }
    }

    Ok(fetched)
}

/// Verify a fetched package directory's content against the checksum
/// recorded for it in the registry index. Fails closed: a missing or
/// empty checksum is rejected exactly like a mismatched one -- an
/// unverifiable package must never install silently (LEDGER item 1b: this
/// used to be the entire bug -- no comparison of any kind happened here).
pub fn verify_package_checksum(
    dest: &Path,
    expected: Option<&str>,
    name: &str,
    version: &str,
) -> Result<(), String> {
    let expected = match expected {
        Some(c) if !c.trim().is_empty() => c.trim(),
        _ => {
            return Err(format!(
                "refusing to install `{name}` v{version}: no checksum is recorded for this \
                 package in the registry index. An unverifiable package cannot be trusted -- \
                 run `kryos pkg sync` to refresh the index, or report this to the registry \
                 maintainer if the entry is genuinely missing a checksum."
            ));
        }
    };

    let actual = crate::registry::content_checksum(dest)?;

    if actual != expected {
        return Err(format!(
            "checksum mismatch for `{name}` v{version}: expected {expected}, got {actual}. \
             The fetched content does not match the registry index -- refusing to install. \
             (The tainted cache entry has been removed. This can mean registry tampering, a \
             force-pushed history, a corrupted download, or a non-reproducible publish.)"
        ));
    }

    Ok(())
}

/// Clone a GitHub repository to a local directory.
///
/// Accepts sources like:
/// - `github:org/repo` — clone whole repo
/// - `github_subdir:org/repo/path/to/subdir` — clone and extract subdirectory
/// - `https://github.com/org/repo` — clone whole repo
fn fetch_github(source: &str, dest: &Path) -> Result<(), String> {
    // NEW: subdirectory pattern
    if let Some(spec) = source.strip_prefix("github_subdir:") {
        return fetch_github_subdir(spec, dest);
    }

    let url = if let Some(gh) = source.strip_prefix("github:") {
        format!("https://github.com/{}.git", gh)
    } else if source.starts_with("https://") || source.starts_with("http://") {
        let url = source.trim_end_matches('/');
        if url.ends_with(".git") {
            url.to_string()
        } else {
            format!("{url}.git")
        }
    } else {
        return Err(format!("unsupported package source: {source}"));
    };

    eprintln!("  fetching {url} -> {}", dest.display());

    let output = Command::new("git")
        .args(["clone", "--depth", "1", &url])
        .arg(dest)
        .output()
        .map_err(|e| format!("failed to run git clone: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git clone failed: {stderr}"));
    }

    Ok(())
}

/// Fetch a package from a subdirectory of a git repository.
///
/// `spec` is `"org/repo/sub/path"` — clones the repo to a temp dir,
/// copies `<tmpdir>/<sub/path>/` to `dest`, then removes the temp dir.
fn fetch_github_subdir(spec: &str, dest: &Path) -> Result<(), String> {
    // Split into at most 3 parts: org, repo, subpath
    let parts: Vec<&str> = spec.splitn(3, '/').collect();
    if parts.len() < 3 {
        return Err(format!(
            "github_subdir source needs org/repo/subpath, got: {spec}"
        ));
    }
    let (org, repo, subpath) = (parts[0], parts[1], parts[2]);
    let url = format!("https://github.com/{org}/{repo}.git");

    let tmp = std::env::temp_dir().join(format!(
        "kryos-fetch-{}-{}",
        repo,
        std::process::id()
    ));
    if tmp.exists() {
        let _ = std::fs::remove_dir_all(&tmp);
    }

    eprintln!(
        "  cloning {url} (subdir: {subpath}) -> {}",
        dest.display()
    );

    let output = Command::new("git")
        .args(["clone", "--depth", "1", &url])
        .arg(&tmp)
        .output()
        .map_err(|e| format!("failed to run git clone: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("git clone failed: {stderr}"));
    }

    let src = tmp.join(subpath);
    if !src.exists() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("subdir not found in cloned repo: {subpath}"));
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("mkdir failed: {e}"))?;
    }

    let copy_result = copy_dir_all(&src, dest).map_err(|e| format!("copy failed: {e}"));
    // Always clean up the raw clone, whether the copy (e.g. a rejected
    // symlink entry) succeeded or failed -- a leaked tmp clone on every
    // rejected package would otherwise accumulate in the OS temp dir.
    let _ = std::fs::remove_dir_all(&tmp);
    copy_result
}

/// Recursively copy a directory tree from `src` to `dst`.
///
/// Refuses any symlink entry outright rather than following it. A cloned
/// registry repo is otherwise-untrusted content: `Path::is_dir()` follows
/// symlinks, so a malicious commit could plant a symlink inside a package
/// directory pointing at an arbitrary path on the fetching machine (e.g.
/// `evil -> /etc` or `..`/an absolute path) and have this function
/// recurse into it, silently copying unrelated files into the local
/// package cache where they would then be compiled/read as if they were
/// part of the package. `DirEntry::file_type()` (unlike `Path::is_dir()`)
/// does NOT follow symlinks, so it is used here to detect and reject them
/// before any recursion or copy happens.
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "refusing to copy symlink entry `{}` while fetching a package -- a \
                     package directory must not contain symlinks (could point outside the \
                     package and pull unrelated files into the local cache)",
                    entry.path().display()
                ),
            ));
        }
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

/// Get the source path for a package in the cache.
/// Returns `<cache_dir>/<name>-<version>/src/` if it exists.
pub fn package_src_path(name: &str, version: &crate::semver::Version) -> Option<PathBuf> {
    let dir = cache_dir().join(format!("{name}-{version}"));
    let src = dir.join("src");
    if src.is_dir() {
        Some(src)
    } else if dir.is_dir() {
        Some(dir)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn make_symlink_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(src, dst)
    }

    #[cfg(windows)]
    fn make_symlink_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_dir(src, dst)
    }

    #[cfg(unix)]
    fn make_symlink_file(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(src, dst)
    }

    #[cfg(windows)]
    fn make_symlink_file(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_file(src, dst)
    }

    // Asserting only `result.is_err()` is NOT load-bearing: on Windows,
    // `fs::copy` of a directory reparse point fails with a plain
    // `PermissionDenied` all on its own (confirmed live -- with the
    // `file_type().is_symlink()` guard deleted entirely, this exact test
    // still reported `ok`), so the assertion passed whether or not the
    // guard existed. Pinning the exact error kind + the guard's own
    // message ties the assertion to the guard actually firing, not to an
    // incidental OS refusal that happens to also return an `Err`.
    fn assert_is_guard_rejection(result: &std::io::Result<()>, entry_name: &str) {
        let err = result
            .as_ref()
            .expect_err("a symlink entry inside a package directory must be refused, not followed");
        assert_eq!(
            err.kind(),
            std::io::ErrorKind::InvalidData,
            "expected the copy_dir_all symlink guard's own error kind, got a different \
             error (possibly an incidental OS-level refusal unrelated to the guard): {err}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains("refusing to copy symlink entry") && msg.contains(entry_name),
            "expected the guard's own rejection message naming `{entry_name}`, got: {msg}"
        );
    }

    #[test]
    fn copy_dir_all_refuses_a_symlinked_directory_entry_pointing_outside_the_package() {
        // A malicious registry commit could plant a symlink inside a
        // package directory pointing OUTSIDE it (e.g. at another cached
        // package, or further up the filesystem). `Path::is_dir()` follows
        // symlinks, so without the file_type() guard this would make
        // copy_dir_all recurse into -- and copy -- content that was never
        // part of the package.
        let base = std::env::temp_dir().join(format!(
            "kryos-copy-dir-symlink-dir-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let src = base.join("src");
        let outside = base.join("outside");
        let dst = base.join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(
            outside.join("secret.txt"),
            "not part of the package -- must never be copied in",
        )
        .unwrap();
        // A legitimate file too, to prove the rejection doesn't silently
        // skip-and-continue -- the whole copy must fail.
        std::fs::write(src.join("real.kry"), "fn main() {}\n").unwrap();

        if make_symlink_dir(&outside, &src.join("evil_link")).is_err() {
            // No permission to create symlinks in this environment (e.g. a
            // locked-down CI runner without Developer Mode) -- skip rather
            // than fail on an unrelated capability gap.
            eprintln!("skipping: cannot create a symlink in this environment");
            let _ = std::fs::remove_dir_all(&base);
            return;
        }

        let result = copy_dir_all(&src, &dst);
        assert_is_guard_rejection(&result, "evil_link");
        assert!(
            !dst.join("evil_link").join("secret.txt").exists(),
            "content from outside the package must never be copied into the cache"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn copy_dir_all_refuses_a_symlinked_file_entry_pointing_outside_the_package() {
        // The file-symlink case is the one the guard genuinely has to stop:
        // `DirEntry::file_type()` on a symlink-to-a-file reports the
        // SYMLINK type (not a directory), so without the guard this entry
        // falls into the plain `std::fs::copy(&path, &target)` branch --
        // and `fs::copy` FOLLOWS a file symlink on every OS (unlike the
        // directory case, there is no incidental OS refusal to mask a
        // missing guard here: with the guard removed, the outside file's
        // real content is copied straight into the cache).
        let base = std::env::temp_dir().join(format!(
            "kryos-copy-dir-symlink-file-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let src = base.join("src");
        let outside = base.join("outside");
        let dst = base.join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let secret_content = "not part of the package -- must never be copied in";
        std::fs::write(outside.join("secret.txt"), secret_content).unwrap();
        std::fs::write(src.join("real.kry"), "fn main() {}\n").unwrap();

        if make_symlink_file(&outside.join("secret.txt"), &src.join("evil_link")).is_err() {
            eprintln!("skipping: cannot create a symlink in this environment");
            let _ = std::fs::remove_dir_all(&base);
            return;
        }

        let result = copy_dir_all(&src, &dst);
        assert_is_guard_rejection(&result, "evil_link");
        // The smoking-gun assertion: without the guard, `evil_link` would
        // be materialized in `dst` as a REGULAR FILE holding the outside
        // secret's actual bytes (fs::copy follows the symlink and reads
        // its target). With the guard, the entry is rejected before that
        // copy call ever runs, so it must not exist at all.
        let leaked = dst.join("evil_link");
        assert!(
            !leaked.exists(),
            "a file symlink pointing outside the package must never be materialized in the \
             cache -- found {}",
            leaked.display()
        );

        let _ = std::fs::remove_dir_all(&base);
    }
}
