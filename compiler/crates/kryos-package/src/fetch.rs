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
    if let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
    {
        PathBuf::from(home).join(".kryos")
    } else {
        PathBuf::from(".kryos")
    }
}

/// Fetch all remote dependencies from a resolved graph to the local cache.
/// Returns the list of (name, local_path) pairs for fetched packages.
pub fn fetch_resolved(graph: &ResolvedGraph) -> Result<Vec<(String, PathBuf)>, String> {
    let cache = cache_dir();
    std::fs::create_dir_all(&cache)
        .map_err(|e| format!("failed to create cache dir: {e}"))?;

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
/// Accepts sources like `github:org/repo` or `https://github.com/org/repo`.
fn fetch_github(source: &str, dest: &Path) -> Result<(), String> {
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

/// Get the source path for a package in the cache.
/// Returns `<cache_dir>/<name>-<version>/src/` if it exists.
pub fn package_src_path(name: &str, version: &crate::semver::Version) -> Option<PathBuf> {
    let dir = cache_dir().join(format!("{name}-{version}"));
    let src = dir.join("src");
    if src.is_dir() { Some(src) } else if dir.is_dir() { Some(dir) } else { None }
}
