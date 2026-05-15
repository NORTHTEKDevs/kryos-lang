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
pub fn fetch_resolved(graph: &ResolvedGraph) -> Result<Vec<(String, PathBuf)>, String> {
    let cache = cache_dir();
    std::fs::create_dir_all(&cache).map_err(|e| format!("failed to create cache dir: {e}"))?;

    let mut fetched = Vec::new();

    for pkg in &graph.packages {
        match &pkg.source {
            PackageSource::Remote(source) => {
                let dest = cache.join(format!("{}-{}", pkg.name, pkg.version));
                if dest.exists() {
                    // Already cached.
                    fetched.push((pkg.name.clone(), dest));
                    continue;
                }
                fetch_github(source, &dest)?;
                fetched.push((pkg.name.clone(), dest));
            }
            PackageSource::Path(path) => {
                fetched.push((pkg.name.clone(), PathBuf::from(path)));
            }
        }
    }

    Ok(fetched)
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

    copy_dir_all(&src, dest).map_err(|e| format!("copy failed: {e}"))?;
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

/// Recursively copy a directory tree from `src` to `dst`.
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
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
