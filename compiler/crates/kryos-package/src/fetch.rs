//! Package fetching — downloads remote dependencies to a local cache.
//!
//! For MVP, supports `github:org/repo` sources by cloning repositories
//! to `~/.kryos/packages/<name>-<version>/`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::manifest::{DepSpec, Manifest};
use crate::resolve::{PackageSource, ResolvedGraph};
use crate::semver::{Version, VersionReq};

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

/// Result of fetching a dependency from an EXPLICIT manifest source -- a
/// `git = "..."` table key, or the `github:org/repo@ver` CLI form -- i.e. a
/// source that is NOT looked up in the registry index at all (LEDGER item
/// 17).
#[derive(Debug)]
pub struct ExplicitFetch {
    /// The version discovered by reading the fetched content's own
    /// `kryos.toml` -- there is no registry index entry to read it from
    /// ahead of time, unlike a registry dependency.
    pub version: Version,
    /// Canonical cache location (`<cache>/<local_name>-<version>/`), same
    /// convention as a registry fetch, so everything downstream (dep
    /// redirects, `fetch_resolved`'s cache-hit check) works unchanged.
    pub dest: PathBuf,
    /// Content checksum computed over the freshly fetched directory. There
    /// is no registry-published checksum to compare an explicit source
    /// against, so this value is TRUSTED ON FIRST FETCH (the same trust
    /// model a `cargo` git dependency without a `rev` pin uses) -- the
    /// caller must record it into `kryos.lock` so a subsequent install
    /// re-verifies this exact content instead of re-trusting the source
    /// blindly every time (see LEDGER item 12's pinned-install path, which
    /// is what actually enforces this going forward).
    pub checksum: String,
    /// The fetched package's own declared dependencies, so the caller can
    /// thread them into the resolver the same way a registry entry's
    /// `dependencies` map is threaded in.
    pub dependencies: HashMap<String, DepSpec>,
}

/// Fetch a dependency directly from the EXPLICIT source a manifest (a
/// `git = "..."` table key) or `kryos pkg add github:org/repo@ver` declared,
/// bypassing the registry index entirely.
///
/// LEDGER item 17: previously `install()`/`update()` destructured
/// `DepSpec::Remote { .. }` with a wildcard, discarding `source` and
/// `version_req` completely, and did a pure by-NAME lookup against the
/// hardcoded official registry instead -- so a project declaring an
/// explicit source (to pin a private fork, a security-patched mirror, or
/// any code not published to the official registry) silently got whatever
/// the official registry happened to publish under the same NAME, with no
/// warning. This function is the fix: it clones/reads `source` itself,
/// never the registry, so the manifest's declared source is what actually
/// gets installed.
///
/// There is no registry index backing an explicit source, so there is no
/// pre-published checksum to verify the first fetch against (unlike a
/// registry package, LEDGER item 1b) -- trust is established the same way
/// a `cargo` git dependency with no `rev` pin, or SSH's `known_hosts`,
/// establishes it: on first fetch. The returned checksum MUST be recorded
/// into `kryos.lock` by the caller so a later install re-verifies this
/// exact content (LEDGER item 12) instead of re-trusting the source blindly
/// on every run.
pub fn fetch_explicit_source(
    local_name: &str,
    source: &str,
    version_req: &VersionReq,
) -> Result<ExplicitFetch, String> {
    // Clone into a scratch temp dir first: the canonical cache path is
    // keyed by `<local_name>-<version>`, and the version is only knowable
    // AFTER reading the fetched content's own kryos.toml.
    let tmp = std::env::temp_dir().join(format!(
        "kryos-explicit-fetch-{}-{}",
        local_name,
        std::process::id()
    ));
    if tmp.exists() {
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // `fetch_github` already understands `github:org/repo`, a bare
    // `https://`/`http://` URL, and (defense-in-depth, unused by this
    // caller) `github_subdir:` -- and its whole-repo clone path is already
    // symlink-guarded (LEDGER item 1b's follow-up hardening), so an
    // explicit source gets the exact same protection a registry fetch does.
    if let Err(e) = fetch_github(source, &tmp) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("failed to fetch explicit source `{source}`: {e}"));
    }

    finish_explicit_fetch(local_name, source, version_req, tmp)
}

/// Everything `fetch_explicit_source` does AFTER the clone: read the
/// fetched content's own version, check it against `version_req`, compute
/// its checksum, and move it into the canonical cache location. Split out
/// so it is directly testable against a REAL local git clone (via
/// `clone_and_guard`, exactly like the symlink-guard tests below) without
/// needing a live github.com/https:// network fetch in a unit test --
/// `fetch_github`/`clone_and_guard`'s own clone-and-symlink-guard behavior
/// is already covered by the tests above this one; this function is 100%
/// of what LEDGER item 17 actually adds.
fn finish_explicit_fetch(
    local_name: &str,
    source: &str,
    version_req: &VersionReq,
    tmp: PathBuf,
) -> Result<ExplicitFetch, String> {
    let cache = cache_dir();
    std::fs::create_dir_all(&cache).map_err(|e| format!("failed to create cache dir: {e}"))?;

    let manifest = match Manifest::from_file(&tmp.join("kryos.toml")) {
        Ok(m) => m,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(format!(
                "explicit source `{source}` for `{local_name}` does not contain a valid \
                 kryos.toml at its root: {e}"
            ));
        }
    };

    let version: Version = match manifest.package.version.parse() {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(format!(
                "explicit source `{source}` for `{local_name}` has an invalid version \
                 `{}`: {e}",
                manifest.package.version
            ));
        }
    };

    if !version_req.matches(&version) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!(
            "explicit source `{source}` for `{local_name}` resolved to v{version}, which does \
             not satisfy the declared requirement `{version_req}`"
        ));
    }

    let checksum = match crate::registry::content_checksum(&tmp) {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(e);
        }
    };

    let dest = cache.join(format!("{local_name}-{version}"));
    if dest.exists() {
        // Always re-trust the CURRENT fetch over a stale cache entry --
        // there is no version-indexed cache identity for an explicit
        // source the way there is for a registry package (the "version" is
        // whatever the source's HEAD currently declares, which can change
        // between runs even under the same local_name-version cache key).
        let _ = std::fs::remove_dir_all(&dest);
    }
    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::rename(&tmp, &dest).is_err() {
        // `rename` can fail across filesystem/volume boundaries (e.g. a
        // temp dir on a different drive than the cache) -- fall back to a
        // guarded copy, which also re-checks for symlinks defensively.
        if let Err(e) = copy_dir_all(&tmp, &dest) {
            let _ = std::fs::remove_dir_all(&tmp);
            let _ = std::fs::remove_dir_all(&dest);
            return Err(format!("failed to install fetched package `{local_name}`: {e}"));
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    Ok(ExplicitFetch {
        version,
        dest,
        checksum,
        dependencies: manifest.dependencies,
    })
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

    clone_and_guard(&url, dest)
}

/// Clone `url` directly into `dest` via `git clone --depth 1`, then refuse
/// the result if it contains any symlink entry.
///
/// Unlike `fetch_github_subdir` (which copies the cloned subdirectory into
/// `dest` through `copy_dir_all`'s symlink guard), this whole-repo clone
/// path writes straight to `dest` via `git clone` itself and never went
/// through that guard at all -- a malicious repo's history can commit a
/// symlink and `git clone` will happily materialize it on disk (confirmed
/// live on this machine: a real Windows symlink, `Get-Content` through it
/// reads the linked-to file's actual bytes) pointing anywhere the fetching
/// machine's filesystem permits. Split into its own function so the guard
/// is directly testable against a real local git clone, not just inferred.
fn clone_and_guard(url: &str, dest: &Path) -> Result<(), String> {
    eprintln!("  fetching {url} -> {}", dest.display());

    // Force real symlink materialization regardless of this machine's own
    // git config: if the local/global `core.symlinks` is `false` (a common
    // Windows default without Developer Mode), a committed symlink checks
    // out as an inert text file instead of a real symlink -- harmless, but
    // it would also make `reject_symlinks` below a no-op that LOOKS like
    // it's guarding something when it isn't. Forcing `true` here means the
    // guard sees the real artifact a POSIX default (`core.symlinks=true`)
    // checkout would produce, on every platform.
    let output = Command::new("git")
        .args([
            "-c",
            "core.symlinks=true",
            // A nonexistent/private/misconfigured repo can make git fall back
            // to an INTERACTIVE credential prompt or a GUI askpass/credential-
            // manager helper instead of erroring -- confirmed live on this
            // machine (LEDGER item 17 made a nonexistent-repo clone reachable
            // for the first time): `GIT_TERMINAL_PROMPT=0` ALONE was not
            // enough -- git still spawned a GUI `git-askpass` helper and hung
            // indefinitely with no terminal prompt ever printed. All THREE of
            // GIT_TERMINAL_PROMPT=0 + credential.helper= + core.askpass= are
            // required together to force a fast, honest failure instead of an
            // unkillable-by-this-process hang. This tool has no user to
            // authenticate as -- an inaccessible source must fail fast.
            "-c",
            "credential.helper=",
            "-c",
            "core.askpass=",
            "clone",
            "--depth",
            "1",
            url,
        ])
        .arg(dest)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("failed to run git clone: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git clone failed: {stderr}"));
    }

    if let Err(e) = reject_symlinks(dest, dest) {
        // Don't leave a partially-trusted, symlink-bearing clone sitting
        // in the cache for a later run to mistake for a good install.
        let _ = std::fs::remove_dir_all(dest);
        return Err(format!("refusing package: {e}"));
    }

    Ok(())
}

/// Recursively reject any symlink entry under `dir` (skipping `.git`,
/// which legitimately carries no package content and is never read by
/// `registry::content_checksum`). Mirrors `copy_dir_all`'s guard -- see
/// its doc comment for the full threat model.
fn reject_symlinks(dir: &Path, root: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name() == ".git" {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            let path = entry.path();
            let rel = path.strip_prefix(root).unwrap_or(&path);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "refusing to trust symlink entry `{}` in a cloned package -- a package \
                     must not contain symlinks (could point outside the package and pull \
                     unrelated files into the local cache)",
                    rel.display()
                ),
            ));
        }
        if file_type.is_dir() {
            reject_symlinks(&entry.path(), root)?;
        }
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

    // Force real symlink materialization regardless of local git config --
    // see `clone_and_guard`'s comment. `copy_dir_all` below only sees a
    // real symlink to reject if the checkout actually produced one.
    let output = Command::new("git")
        .args([
            "-c",
            "core.symlinks=true",
            "-c",
            "credential.helper=",
            "-c",
            "core.askpass=",
            "clone",
            "--depth",
            "1",
            &url,
        ])
        .arg(&tmp)
        // See clone_and_guard's comment: all three flags together are
        // required to fail fast instead of hanging on a GUI credential/
        // askpass prompt.
        .env("GIT_TERMINAL_PROMPT", "0")
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

    /// The whole-repo (non-`github_subdir:`) clone path never went through
    /// `copy_dir_all` at all -- `git clone` writes straight to `dest`, so a
    /// symlink committed in the source repo's history is a live escape
    /// distinct from (and unguarded by) the `copy_dir_all` tests above.
    /// This builds a REAL local git repo containing a symlink, clones it
    /// through `clone_and_guard` (the exact function `fetch_github`'s
    /// plain-clone branch calls), and requires the clone to be rejected.
    #[test]
    fn clone_and_guard_rejects_a_symlink_committed_in_the_source_repo() {
        let base = std::env::temp_dir().join(format!(
            "kryos-clone-guard-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let repo = base.join("repo");
        let outside = base.join("outside");
        let dest = base.join("dest");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(
            outside.join("secret.txt"),
            "not part of the package -- must never be copied in",
        )
        .unwrap();
        std::fs::write(
            repo.join("kryos.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        if make_symlink_file(&outside.join("secret.txt"), &repo.join("evil_link")).is_err() {
            eprintln!("skipping: cannot create a symlink in this environment");
            let _ = std::fs::remove_dir_all(&base);
            return;
        }

        let git = |args: &[&str]| -> std::process::Output {
            Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .expect("git must be on PATH for this test")
        };
        assert!(git(&["init", "-q"]).status.success());
        assert!(git(&[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=test",
            "add",
            "-A",
        ])
        .status
        .success());
        let commit = git(&[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=test",
            "commit",
            "-q",
            "-m",
            "initial",
        ]);
        if !commit.status.success() {
            // e.g. no git identity resolvable at all in this environment --
            // skip rather than fail on an unrelated capability gap.
            eprintln!(
                "skipping: git commit failed: {}",
                String::from_utf8_lossy(&commit.stderr)
            );
            let _ = std::fs::remove_dir_all(&base);
            return;
        }

        let repo_url = repo.to_string_lossy().to_string();
        let result = clone_and_guard(&repo_url, &dest);
        assert!(
            result.is_err(),
            "a symlink committed inside a cloned repo must be rejected, got Ok"
        );
        let msg = result.unwrap_err();
        assert!(
            msg.contains("evil_link"),
            "expected the guard's rejection to name the offending entry, got: {msg}"
        );
        assert!(
            !dest.exists(),
            "a rejected whole-repo clone must not be left in the cache -- found {}",
            dest.display()
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    // ─── LEDGER item 17: explicit-source fetch ────────────────────────────

    /// Build a real local git repo at `repo` containing a `kryos.toml`
    /// (name/version as given) and a trivial `src/main.kry`, committed.
    /// Returns `false` (caller should skip) if git/identity isn't usable in
    /// this environment, matching the existing tests' skip convention.
    fn make_versioned_repo(repo: &Path, name: &str, version: &str) -> bool {
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(
            repo.join("kryos.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n"),
        )
        .unwrap();
        std::fs::write(
            repo.join("src").join("main.kry"),
            "fn main() {\n    println(\"hi\")\n}\n",
        )
        .unwrap();

        let git = |args: &[&str]| -> std::process::Output {
            Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .expect("git must be on PATH for this test")
        };
        if !git(&["init", "-q"]).status.success() {
            return false;
        }
        if !git(&[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=test",
            "add",
            "-A",
        ])
        .status
        .success()
        {
            return false;
        }
        git(&[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=test",
            "commit",
            "-q",
            "-m",
            "initial",
        ])
        .status
        .success()
    }

    #[test]
    fn finish_explicit_fetch_honors_declared_source_and_computes_a_checksum() {
        // LEDGER item 17: an explicit source's OWN version (read from its
        // own kryos.toml, not a registry index) must be what gets recorded
        // -- and, since there is no registry checksum to compare against,
        // the content checksum computed here must be genuinely derived
        // from the fetched bytes (not a placeholder), matching what
        // `registry::content_checksum` would independently compute over
        // the same directory.
        let base = std::env::temp_dir().join(format!(
            "kryos-finish-explicit-ok-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let repo = base.join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        if !make_versioned_repo(&repo, "explicit-dep", "1.2.0") {
            eprintln!("skipping: git init/commit unusable in this environment");
            let _ = std::fs::remove_dir_all(&base);
            return;
        }

        let tmp = base.join("clone");
        let repo_url = repo.to_string_lossy().to_string();
        clone_and_guard(&repo_url, &tmp).expect("local clone must succeed");

        let req: VersionReq = "^1.0.0".parse().unwrap();
        let result = finish_explicit_fetch("explicit-dep", &repo_url, &req, tmp.clone());
        let fetched = result.expect("a version satisfying the requirement must be accepted");

        assert_eq!(fetched.version, Version::new(1, 2, 0));
        assert!(
            fetched.dest.join("kryos.toml").exists(),
            "the fetched package must land at the canonical cache dest"
        );
        assert!(
            !tmp.exists(),
            "the scratch temp clone must be moved, not left behind, at {}",
            tmp.display()
        );
        let recomputed = crate::registry::content_checksum(&fetched.dest).unwrap();
        assert_eq!(
            fetched.checksum, recomputed,
            "the returned checksum must match an independent recomputation over the same \
             fetched directory"
        );
        assert!(fetched.checksum.starts_with("sha256:"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn finish_explicit_fetch_rejects_a_version_not_satisfying_the_requirement() {
        // The manifest declared `^2.0.0`; the explicit source's own
        // kryos.toml says 1.0.0 -- this must be refused, not silently
        // accepted (an explicit source is not exempt from the version
        // requirement the manifest itself wrote down).
        let base = std::env::temp_dir().join(format!(
            "kryos-finish-explicit-mismatch-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let repo = base.join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        if !make_versioned_repo(&repo, "explicit-dep", "1.0.0") {
            eprintln!("skipping: git init/commit unusable in this environment");
            let _ = std::fs::remove_dir_all(&base);
            return;
        }

        let tmp = base.join("clone");
        let repo_url = repo.to_string_lossy().to_string();
        clone_and_guard(&repo_url, &tmp).expect("local clone must succeed");

        let req: VersionReq = "^2.0.0".parse().unwrap();
        let result = finish_explicit_fetch("explicit-dep", &repo_url, &req, tmp.clone());
        let err = result.expect_err("a version outside the requirement must be rejected");
        assert!(
            err.contains("does not satisfy"),
            "expected a version-requirement rejection message, got: {err}"
        );
        assert!(
            !tmp.exists(),
            "a rejected fetch must not leave the scratch temp clone behind"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn finish_explicit_fetch_rejects_a_source_with_no_kryos_toml() {
        // An explicit source that isn't actually a Kryos package at its
        // root (no kryos.toml) must be refused with a clear message, not
        // treated as version 0.0.0 or silently accepted.
        let base = std::env::temp_dir().join(format!(
            "kryos-finish-explicit-nomanifest-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let tmp = base.join("clone");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("README.md"), "not a kryos package\n").unwrap();

        let req: VersionReq = "*".parse().unwrap();
        let result = finish_explicit_fetch(
            "explicit-dep",
            "https://example.invalid/not-really-cloned",
            &req,
            tmp.clone(),
        );
        let err = result.expect_err("a source with no kryos.toml must be rejected");
        assert!(
            err.contains("does not contain a valid kryos.toml"),
            "expected a missing-manifest rejection message, got: {err}"
        );

        let _ = std::fs::remove_dir_all(&base);
    }
}
