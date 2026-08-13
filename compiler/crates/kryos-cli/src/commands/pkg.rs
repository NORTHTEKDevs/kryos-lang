//! `kryos pkg` — package management commands.

use std::path::{Path, PathBuf};

use kryos_package::Manifest;

/// `kryos pkg init [name]` — create a new kryos.toml.
///
/// When `name` is provided, creates a `<name>/` directory and writes all
/// project files inside it (like `cargo new`).  When `name` is `None`,
/// initialises the *current* directory (like `cargo init`).
pub fn init(name: Option<&str>) -> Result<(), String> {
    // Determine the base directory: either a new subdirectory or cwd.
    let base_dir: PathBuf = match name {
        Some(n) => {
            let dir = PathBuf::from(n);
            if dir.exists() {
                // If the directory already exists, only bail when a manifest
                // is already present (allows `init` into an empty folder).
                if dir.join("kryos.toml").exists() {
                    return Err(format!("kryos.toml already exists in `{}`", dir.display()));
                }
            } else {
                std::fs::create_dir_all(&dir)
                    .map_err(|e| format!("failed to create directory `{}`: {e}", dir.display()))?;
            }
            dir
        }
        None => {
            let dir = PathBuf::from(".");
            if dir.join("kryos.toml").exists() {
                return Err("kryos.toml already exists in this directory".to_string());
            }
            dir
        }
    };

    let project_name = match name {
        Some(n) => n.to_string(),
        None => {
            // Derive from the current directory name.
            std::env::current_dir()
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                .unwrap_or_else(|| "my-project".to_string())
        }
    };

    let toml_content = format!(
        r#"[package]
name = "{project_name}"
version = "0.1.0"
edition = "2026"

[dependencies]

[capabilities]
allowed = []

[build]
target = "native"
optimization = "dev"
"#
    );

    // Write kryos.toml inside the base directory.
    let manifest_path = base_dir.join("kryos.toml");
    std::fs::write(&manifest_path, &toml_content)
        .map_err(|e| format!("failed to write kryos.toml: {e}"))?;

    // Create a src/ directory with a main.kry stub if it doesn't exist.
    let src_dir = base_dir.join("src");
    if !src_dir.exists() {
        std::fs::create_dir_all(&src_dir).map_err(|e| format!("failed to create src/: {e}"))?;
    }

    let main_path = src_dir.join("main.kry");
    if !main_path.exists() {
        std::fs::write(
            &main_path,
            "// Welcome to Kryos!\n\nfn main() {\n    println(\"Hello, Kryos!\")\n}\n",
        )
        .map_err(|e| format!("failed to write src/main.kry: {e}"))?;
    }

    // Create .gitignore if it doesn't exist.
    let gitignore_path = base_dir.join(".gitignore");
    if !gitignore_path.exists() {
        std::fs::write(
            &gitignore_path,
            "# Build artifacts\ntarget/\n*.o\n*.obj\n*.exe\n\n# Dependencies\n.kryos/\n\n# Lock file (optional — commit if you want reproducible builds)\n# kryos.lock\n",
        )
        .map_err(|e| format!("failed to write .gitignore: {e}"))?;
    }

    // Create README.md if it doesn't exist.
    let readme_path = base_dir.join("README.md");
    if !readme_path.exists() {
        std::fs::write(
            &readme_path,
            format!(
                "# {project_name}\n\nA [Kryos](https://github.com/NORTHTEKDevs/kryos-lang) project.\n\n## Build\n\n```bash\nkryos build src/main.kry -o {project_name}\n```\n\n## Run\n\n```bash\nkryos run src/main.kry\n```\n"
            ),
        )
        .map_err(|e| format!("failed to write README.md: {e}"))?;
    }

    eprintln!("initialized Kryos project `{project_name}`");
    eprintln!("  created kryos.toml");
    eprintln!("  created src/main.kry");
    eprintln!("  created .gitignore");
    eprintln!("  created README.md");

    Ok(())
}

/// `kryos pkg add <dep>` — add a dependency to kryos.toml.
pub fn add(dependency: &str) -> Result<(), String> {
    let manifest_path = Path::new("kryos.toml");
    let mut manifest = load_manifest(manifest_path)?;

    // Parse the dependency specifier.
    let dep_spec = kryos_package::manifest::parse_dep_string(dependency)?;

    // Derive a name from the source URL.
    let dep_name = derive_dep_name(dependency);

    if manifest.dependencies.contains_key(&dep_name) {
        return Err(format!(
            "dependency `{dep_name}` already exists in kryos.toml"
        ));
    }

    manifest.dependencies.insert(dep_name.clone(), dep_spec);
    save_manifest(manifest_path, &manifest)?;

    eprintln!("added dependency `{dep_name}`");
    Ok(())
}

/// `kryos pkg remove <dep>` — remove a dependency from kryos.toml.
pub fn remove(dependency: &str) -> Result<(), String> {
    let manifest_path = Path::new("kryos.toml");
    let mut manifest = load_manifest(manifest_path)?;

    if manifest.dependencies.remove(dependency).is_none() {
        return Err(format!("dependency `{dependency}` not found in kryos.toml"));
    }

    save_manifest(manifest_path, &manifest)?;

    eprintln!("removed dependency `{dependency}`");
    Ok(())
}

/// Add every `Remote` dependency in `manifest.dependencies` to `registry` as
/// an available package for the resolver to pick from. Shared by
/// `install()`'s fresh-resolve path and `update()` so the two commands
/// cannot drift out of sync the way they did before LEDGER item 17 (both
/// `install()` and `update()` independently discarded an explicit source).
///
/// Two cases per dependency:
/// - `source` is EMPTY (the common case -- a plain version requirement like
///   `http-router = "*"` under the dependency's own name): resolve purely by
///   NAME against the registry index, unchanged from before.
/// - `source` is NON-EMPTY (a `git = "..."` table key, or the
///   `github:org/repo@ver` CLI form): LEDGER item 17 -- this is an EXPLICIT
///   override the manifest author wrote down. It must never be silently
///   replaced by an unrelated by-name registry match, so it is fetched
///   directly via `fetch::fetch_explicit_source` instead of ever calling
///   `registry_client.lookup`.
fn add_remote_deps_to_registry(
    manifest: &Manifest,
    registry: &mut kryos_package::resolve::PackageRegistry,
) -> Result<(), String> {
    let registry_client = kryos_package::RegistryClient::new();

    for (name, dep) in &manifest.dependencies {
        let kryos_package::DepSpec::Remote {
            source,
            version_req,
        } = dep
        else {
            continue; // Path deps are handled by the caller.
        };

        if !source.is_empty() {
            eprintln!(
                "  fetching `{name}` from its declared source `{source}` (not the registry \
                 index) ..."
            );
            let fetched = kryos_package::fetch::fetch_explicit_source(name, source, version_req)
                .map_err(|e| format!("failed to fetch explicit source for `{name}`: {e}"))?;
            eprintln!(
                "  warning: `{name}` v{} was fetched from an explicit source -- no registry \
                 checksum exists to verify it against, so this content is trusted on first \
                 fetch (checksum {}) and will be PINNED in kryos.lock so a future install \
                 re-verifies instead of re-trusting the source blindly",
                fetched.version, fetched.checksum
            );
            let transitive: std::collections::HashMap<String, kryos_package::semver::VersionReq> =
                fetched
                    .dependencies
                    .iter()
                    .filter_map(|(dep_name, dep_spec)| match dep_spec {
                        kryos_package::DepSpec::Remote { version_req, .. } => {
                            Some((dep_name.clone(), version_req.clone()))
                        }
                        kryos_package::DepSpec::Path { .. } => None,
                    })
                    .collect();
            registry.add(kryos_package::resolve::AvailablePackage {
                name: name.clone(),
                version: fetched.version,
                source: source.clone(),
                dependencies: transitive,
                checksum: Some(fetched.checksum),
            });
            continue;
        }

        // No explicit source: resolve purely by name against the registry
        // index, exactly as before item 17's fix.
        match registry_client.lookup(name) {
            Ok(Some(entries)) => {
                for entry in &entries {
                    let entry_source = format!(
                        "github_subdir:NORTHTEKDevs/kryos-registry/packages/{}/{}",
                        entry.name, entry.version
                    );
                    let transitive: std::collections::HashMap<
                        String,
                        kryos_package::semver::VersionReq,
                    > = entry
                        .dependencies
                        .iter()
                        .filter_map(|(dep_name, dep_ver)| {
                            dep_ver
                                .parse::<kryos_package::semver::VersionReq>()
                                .ok()
                                .map(|vr| (dep_name.clone(), vr))
                        })
                        .collect();
                    registry.add(kryos_package::resolve::AvailablePackage {
                        name: entry.name.clone(),
                        version: entry.version.clone(),
                        source: entry_source,
                        dependencies: transitive,
                        checksum: if entry.checksum.trim().is_empty() {
                            None
                        } else {
                            Some(entry.checksum.clone())
                        },
                    });
                }
            }
            Ok(None) => {
                eprintln!(
                    "  warning: `{name}` not found in registry index — \
                     try `kryos pkg sync` to refresh"
                );
            }
            Err(e) => {
                eprintln!("  warning: registry lookup for `{name}` failed: {e}");
            }
        }
    }

    Ok(())
}

/// Build a `PackageRegistry` covering both PATH and REMOTE dependencies
/// declared in `manifest`, ready to pass to `kryos_package::resolve::resolve`.
fn build_registry(manifest: &Manifest) -> Result<kryos_package::resolve::PackageRegistry, String> {
    let mut registry = kryos_package::resolve::PackageRegistry::new();
    for (name, dep) in &manifest.dependencies {
        if let kryos_package::DepSpec::Path { path } = dep {
            let dep_manifest_path = Path::new(path).join("kryos.toml");
            if dep_manifest_path.exists() {
                let dep_manifest = Manifest::from_file(&dep_manifest_path)?;
                let version: kryos_package::Version = dep_manifest
                    .package
                    .version
                    .parse()
                    .map_err(|e| format!("bad version in {}: {e}", name))?;
                registry.add(kryos_package::resolve::AvailablePackage {
                    name: name.clone(),
                    version,
                    source: path.clone(),
                    dependencies: std::collections::HashMap::new(),
                    // Local filesystem dep -- no remote content to pin by
                    // hash, trust is already implied by having the path.
                    checksum: None,
                });
            }
        }
    }
    add_remote_deps_to_registry(manifest, &mut registry)?;
    Ok(registry)
}

/// `kryos pkg update` — resolve dependencies and update lock file.
pub fn update() -> Result<(), String> {
    let manifest_path = Path::new("kryos.toml");
    let manifest = load_manifest(manifest_path)?;

    if manifest.dependencies.is_empty() {
        eprintln!("no dependencies to resolve");
        return Ok(());
    }

    eprintln!(
        "resolving dependencies for {} v{} ...",
        manifest.package.name, manifest.package.version
    );

    let registry = build_registry(&manifest)?;

    match kryos_package::resolve::resolve(&manifest.dependencies, &registry) {
        Ok(graph) => {
            let lock = kryos_package::LockFile::from_resolved(&graph);
            lock.write_to_file(Path::new("kryos.lock"))?;

            eprintln!(
                "resolved {} package{}",
                graph.packages.len(),
                if graph.packages.len() == 1 { "" } else { "s" }
            );
            for pkg in &graph.packages {
                eprintln!("  {} v{}", pkg.name, pkg.version);
            }
            eprintln!("wrote kryos.lock");
        }
        Err(e) => {
            return Err(format!("dependency resolution failed: {e}"));
        }
    }

    Ok(())
}

/// Names of manifest dependencies that `kryos.lock` does NOT pin, or pins
/// at a version that no longer satisfies the manifest's own declared
/// requirement (e.g. the requirement was hand-tightened without deleting
/// the lock). Empty means the lock is trustworthy as-is for a pinned
/// install (LEDGER item 12).
fn lock_coverage_gap(manifest: &Manifest, lock: &kryos_package::LockFile) -> Vec<String> {
    let mut gap = Vec::new();
    for (name, dep) in &manifest.dependencies {
        match lock.get(name) {
            None => gap.push(name.clone()),
            Some(entry) => {
                if let kryos_package::DepSpec::Remote { version_req, .. } = dep {
                    match entry.version.parse::<kryos_package::Version>() {
                        Ok(v) if version_req.matches(&v) => {}
                        _ => gap.push(name.clone()),
                    }
                }
                // Path deps: presence in the lock is enough coverage -- their
                // content is always read live from disk at build time
                // regardless of the version label recorded here.
            }
        }
    }
    gap.sort();
    gap
}

/// Convert an already-written `kryos.lock` into a `ResolvedGraph` without
/// touching the network or the registry at all -- LEDGER item 12's pinned-
/// install path. `install()` uses this whenever the lock already covers
/// every dependency the manifest declares, instead of re-resolving live.
fn lock_to_graph(
    lock: &kryos_package::LockFile,
) -> Result<kryos_package::resolve::ResolvedGraph, String> {
    let mut packages = Vec::with_capacity(lock.packages.len());
    for entry in &lock.packages {
        let version: kryos_package::Version = entry
            .version
            .parse()
            .map_err(|e| format!("kryos.lock: bad version for `{}`: {e}", entry.name))?;
        let source = if let Some(path) = entry.source.strip_prefix("path:") {
            kryos_package::PackageSource::Path(path.to_string())
        } else {
            kryos_package::PackageSource::Remote(entry.source.clone())
        };
        packages.push(kryos_package::ResolvedPackage {
            name: entry.name.clone(),
            version,
            source,
            dependencies: entry.dependencies.clone(),
            checksum: entry.checksum.clone(),
        });
    }
    Ok(kryos_package::resolve::ResolvedGraph { packages })
}

/// Create `.kryos/deps/<name>.redirect` for every fetched dependency.
fn write_dep_redirects(fetched: &[(String, PathBuf)]) -> Result<(), String> {
    let deps_dir = PathBuf::from(".kryos").join("deps");
    std::fs::create_dir_all(&deps_dir)
        .map_err(|e| format!("failed to create .kryos/deps/: {e}"))?;

    for (name, src_path) in fetched {
        let link_path = deps_dir.join(name);
        // Remove stale link/directory if present.
        if link_path.exists() {
            if link_path.is_dir() {
                let _ = std::fs::remove_dir_all(&link_path);
            } else {
                let _ = std::fs::remove_file(&link_path);
            }
        }
        let redirect_content = format!("# kryos dep redirect\npath = \"{}\"\n", src_path.display());
        std::fs::write(deps_dir.join(format!("{name}.redirect")), redirect_content)
            .map_err(|e| format!("failed to write dep redirect for {name}: {e}"))?;
    }
    Ok(())
}

/// Print the `installed N packages` summary shared by every install path.
fn print_install_summary(fetched: &[(String, PathBuf)]) {
    let path_count = fetched
        .iter()
        .filter(|(_, p)| !p.starts_with(kryos_package::fetch::cache_dir()))
        .count();
    let remote_count = fetched.len() - path_count;

    eprintln!();
    eprintln!(
        "installed {} package{}",
        fetched.len(),
        if fetched.len() == 1 { "" } else { "s" }
    );
    for (name, path) in fetched {
        let is_path = !path.starts_with(kryos_package::fetch::cache_dir());
        if is_path {
            eprintln!("  {name} (path: {})", path.display());
        } else {
            eprintln!("  {name} (cached: {})", path.display());
        }
    }
    if remote_count > 0 {
        eprintln!("  {} remote, {} local path", remote_count, path_count);
    }
}

/// `kryos pkg install` — resolve and fetch all dependencies.
///
/// LEDGER item 12: previously this ALWAYS re-resolved live against the
/// manifest and silently overwrote `kryos.lock` on every run, so a
/// committed lock provided no enforcement at all -- a newly published (or
/// force-pushed/compromised) registry version was adopted silently on the
/// very next install, with the lock re-signed to match. `install()` now
/// reads an existing `kryos.lock` first: if it already covers every
/// dependency the manifest declares (`lock_coverage_gap` is empty), the
/// install is PINNED -- it fetches exactly what the lock says, verifies
/// checksums, and does not touch the lock or the registry index at all,
/// matching `npm ci` / `cargo install --locked` semantics. `kryos pkg
/// update` remains the explicit, deliberate re-resolve operation.
pub fn install() -> Result<(), String> {
    let manifest_path = Path::new("kryos.toml");
    let manifest = load_manifest(manifest_path)?;

    if manifest.dependencies.is_empty() {
        eprintln!("no dependencies to install");
        eprintln!("  0 packages installed");
        return Ok(());
    }

    let lock_path = Path::new("kryos.lock");
    if lock_path.exists() {
        let lock = kryos_package::LockFile::from_file(lock_path)
            .map_err(|e| format!("failed to read kryos.lock: {e}"))?;

        let gap = lock_coverage_gap(&manifest, &lock);
        if gap.is_empty() {
            return install_pinned(&manifest, &lock);
        }

        eprintln!(
            "kryos.lock does not cover every dependency in kryos.toml ({}) -- re-resolving \
             live. Any ALREADY-locked package whose freshly resolved version differs from \
             kryos.lock will be reported below, not silently overwritten (run `kryos pkg \
             update` for a deliberate, reviewed re-resolve instead).",
            gap.join(", ")
        );
        return install_fresh(&manifest, Some(&lock));
    }

    install_fresh(&manifest, None)
}

/// Pinned install path (LEDGER item 12): fetch exactly what `kryos.lock`
/// already pins, checksum-verify (LEDGER item 1b), and stop -- never
/// re-resolve, never touch the registry index, never rewrite the lock.
fn install_pinned(manifest: &Manifest, lock: &kryos_package::LockFile) -> Result<(), String> {
    eprintln!(
        "kryos.lock covers every dependency in {} v{} -- installing PINNED versions ({} \
         package{}), not re-resolving (run `kryos pkg update` for a deliberate re-resolve)",
        manifest.package.name,
        manifest.package.version,
        lock.packages.len(),
        if lock.packages.len() == 1 { "" } else { "s" }
    );

    let graph = lock_to_graph(lock)?;

    eprintln!("fetching packages ...");
    let fetched = kryos_package::fetch::fetch_resolved(&graph)?;

    write_dep_redirects(&fetched)?;
    print_install_summary(&fetched);
    // Deliberately do NOT rewrite kryos.lock here -- see the function doc:
    // a pinned install must not silently touch the lock it is pinned to.
    eprintln!("kryos.lock unchanged (pinned install)");

    Ok(())
}

/// Fresh-resolve install path: the pre-item-12 behavior, now also carrying
/// item 17's fix (via `build_registry`/`add_remote_deps_to_registry`) and,
/// when `prior_lock` is `Some` (the manifest grew a dependency the lock
/// didn't cover yet), a loud per-package drift report instead of a silent
/// overwrite for any package that WAS already locked.
fn install_fresh(
    manifest: &Manifest,
    prior_lock: Option<&kryos_package::LockFile>,
) -> Result<(), String> {
    eprintln!(
        "resolving dependencies for {} v{} ...",
        manifest.package.name, manifest.package.version
    );

    let registry = build_registry(manifest)?;

    let graph = kryos_package::resolve::resolve(&manifest.dependencies, &registry)
        .map_err(|e| format!("dependency resolution failed: {e}"))?;

    if let Some(lock) = prior_lock {
        for pkg in &graph.packages {
            if let Some(entry) = lock.get(&pkg.name) {
                let fresh_version = pkg.version.to_string();
                if entry.version != fresh_version {
                    eprintln!(
                        "  warning: `{}` drifted from the committed kryos.lock: was v{}, now \
                         resolving to v{} -- review before trusting this install",
                        pkg.name, entry.version, fresh_version
                    );
                }
            }
        }
    }

    // Fetch all remote packages to the cache.
    eprintln!("fetching packages ...");
    let fetched = kryos_package::fetch::fetch_resolved(&graph)?;

    write_dep_redirects(&fetched)?;

    // Write lock file.
    let lock = kryos_package::LockFile::from_resolved(&graph);
    lock.write_to_file(Path::new("kryos.lock"))?;

    print_install_summary(&fetched);
    eprintln!("wrote kryos.lock");

    Ok(())
}

/// `kryos pkg lock` — regenerate the lock file (alias for update).
pub fn lock() -> Result<(), String> {
    update()
}

/// `kryos pkg publish` — package and publish to the registry.
pub fn publish() -> Result<(), String> {
    let project_dir =
        std::env::current_dir().map_err(|e| format!("cannot determine current directory: {e}"))?;

    eprintln!("packaging...");
    let pkg = kryos_package::registry::pack(&project_dir)?;

    let index_entry = kryos_package::registry::generate_index_entry(&pkg);

    eprintln!("packaged {} v{}", pkg.name, pkg.version);
    eprintln!("  tarball: {}", pkg.tarball_path.display());
    eprintln!();
    eprintln!("registry index entry:");
    eprintln!("{index_entry}");
    eprintln!();
    eprintln!("to publish, add the index entry to the registry at:");
    eprintln!("  {}", kryos_package::registry::DEFAULT_REGISTRY);
    eprintln!();
    eprintln!("then upload the tarball to your chosen hosting.");

    Ok(())
}

/// `kryos pkg search <query>` — search the registry for packages.
pub fn search(query: &str) -> Result<(), String> {
    let client = kryos_package::RegistryClient::new();

    let results = client.search(query)?;

    if results.is_empty() {
        eprintln!("no packages found matching `{query}`");
    } else {
        eprintln!("packages matching `{query}`:");
        for name in &results {
            // Try to get version info.
            if let Ok(Some(entries)) = client.lookup(name) {
                if let Some(latest) = entries.last() {
                    eprintln!("  {name} v{}", latest.version);
                } else {
                    eprintln!("  {name}");
                }
            } else {
                eprintln!("  {name}");
            }
        }
        eprintln!(
            "{} package{} found",
            results.len(),
            if results.len() == 1 { "" } else { "s" }
        );
    }

    Ok(())
}

/// `kryos pkg info <name>` — show package info from the registry.
pub fn info(name: &str) -> Result<(), String> {
    let client = kryos_package::RegistryClient::new();

    match client.lookup(name)? {
        Some(entries) => {
            eprintln!("{name}");
            eprintln!("  versions:");
            for entry in &entries {
                eprintln!("    v{}", entry.version);
            }
            if let Some(latest) = entries.last() {
                eprintln!("  latest: v{}", latest.version);
                eprintln!("  checksum: {}", latest.checksum);
            }
        }
        None => {
            eprintln!("package `{name}` not found in registry");
            eprintln!("  try `kryos pkg sync` to update the index");
        }
    }

    Ok(())
}

/// `kryos pkg outdated` — list locked packages that have newer versions
/// available in the registry index.
pub fn outdated() -> Result<(), String> {
    let lock_path = Path::new("kryos.lock");
    if !lock_path.exists() {
        return Err(
            "no kryos.lock found — run `kryos pkg update` or `kryos pkg install` first"
                .to_string(),
        );
    }

    let lock = kryos_package::LockFile::from_file(lock_path)
        .map_err(|e| format!("failed to read kryos.lock: {e}"))?;

    if lock.packages.is_empty() {
        eprintln!("no locked packages");
        return Ok(());
    }

    let client = kryos_package::RegistryClient::new();
    let mut outdated_count: usize = 0;
    let mut unknown_count: usize = 0;
    let mut current_count: usize = 0;

    eprintln!("checking {} locked package(s) against registry...", lock.packages.len());

    // Print header for the table.
    eprintln!("{:<28} {:<14} {:<14}", "PACKAGE", "INSTALLED", "LATEST");

    for entry in &lock.packages {
        // Skip path-source entries — they don't have a registry counterpart.
        if entry.source.starts_with("path:") {
            eprintln!("{:<28} {:<14} {:<14}", entry.name, entry.version, "(path)");
            continue;
        }

        match client.lookup(&entry.name) {
            Ok(Some(entries)) => {
                if let Some(latest) = entries.last() {
                    let latest_v = latest.version.to_string();
                    if latest_v != entry.version {
                        outdated_count += 1;
                        eprintln!(
                            "{:<28} {:<14} {:<14} (outdated)",
                            entry.name, entry.version, latest_v
                        );
                    } else {
                        current_count += 1;
                        eprintln!(
                            "{:<28} {:<14} {:<14}",
                            entry.name, entry.version, latest_v
                        );
                    }
                } else {
                    unknown_count += 1;
                    eprintln!(
                        "{:<28} {:<14} {:<14}",
                        entry.name, entry.version, "(no versions)"
                    );
                }
            }
            Ok(None) => {
                unknown_count += 1;
                eprintln!(
                    "{:<28} {:<14} {:<14}",
                    entry.name, entry.version, "(not in index)"
                );
            }
            Err(e) => {
                unknown_count += 1;
                eprintln!("{:<28} {:<14} (lookup error: {e})", entry.name, entry.version);
            }
        }
    }

    eprintln!();
    eprintln!(
        "summary: {current_count} up-to-date, {outdated_count} outdated, {unknown_count} unknown"
    );
    if outdated_count > 0 {
        eprintln!("run `kryos pkg update` to refresh kryos.lock");
    }

    Ok(())
}

/// `kryos pkg sync` — sync the registry index.
pub fn sync() -> Result<(), String> {
    eprintln!("syncing registry index...");
    let client = kryos_package::RegistryClient::new();
    client.sync()?;
    eprintln!("registry index up to date");
    Ok(())
}

// ─── helpers ────────────────────────────────────────────────────────────────

fn load_manifest(path: &Path) -> Result<Manifest, String> {
    if !path.exists() {
        return Err("no kryos.toml found — run `kryos pkg init` first".to_string());
    }
    Manifest::from_file(path)
}

fn save_manifest(path: &Path, manifest: &Manifest) -> Result<(), String> {
    let content = manifest.to_toml()?;
    std::fs::write(path, content).map_err(|e| format!("failed to write kryos.toml: {e}"))
}

/// Extract a short dependency name from a specifier like
/// `github:kryos-lang/serde@^1.0.0` -> `serde`.
fn derive_dep_name(spec: &str) -> String {
    // Strip everything after @
    let base = spec.split('@').next().unwrap_or(spec);
    // Take the last path segment
    base.rsplit('/').next().unwrap_or(base).to_string()
}

/// `kryos pkg list-local [--root PATH]` — discover packages on disk.
///
/// Walks `<root>/` (default `./packages`) one level deep, reads each
/// subdirectory's `kryos.toml`, and prints `(name, version, description)`.
pub fn list_local(root: Option<&str>) -> Result<(), String> {
    let root = match root {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from("packages"),
    };
    if !root.is_dir() {
        return Err(format!(
            "kryos pkg list-local: '{}' is not a directory",
            root.display()
        ));
    }

    let entries = std::fs::read_dir(&root)
        .map_err(|e| format!("kryos pkg list-local: read {}: {e}", root.display()))?;

    let mut found: Vec<(String, String, String)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest = path.join("kryos.toml");
        if !manifest.is_file() {
            continue;
        }
        let Ok(m) = Manifest::from_file(&manifest) else {
            continue;
        };
        found.push((
            m.package.name.clone(),
            m.package.version.clone(),
            m.package.description.clone().unwrap_or_default(),
        ));
    }

    found.sort_by(|a, b| a.0.cmp(&b.0));

    if found.is_empty() {
        println!("(no packages found under {})", root.display());
    } else {
        println!("\x1b[1mLocal packages under {}\x1b[0m", root.display());
        println!();
        let name_w = found.iter().map(|(n, _, _)| n.len()).max().unwrap_or(20).max(20);
        for (n, v, d) in &found {
            println!("  \x1b[36m{:<width$}\x1b[0m {}  {}", n, v, d, width = name_w);
        }
        println!();
        println!("{} package(s)", found.len());
    }
    Ok(())
}
