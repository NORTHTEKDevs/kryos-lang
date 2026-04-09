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
                    return Err(format!(
                        "kryos.toml already exists in `{}`",
                        dir.display()
                    ));
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
        std::fs::create_dir_all(&src_dir)
            .map_err(|e| format!("failed to create src/: {e}"))?;
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
                "# {project_name}\n\nA [Kryos](https://github.com/FrostbyteDevTeam/kryos-lang) project.\n\n## Build\n\n```bash\nkryos build src/main.kry -o {project_name}\n```\n\n## Run\n\n```bash\nkryos run src/main.kry\n```\n"
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
        return Err(format!("dependency `{dep_name}` already exists in kryos.toml"));
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

/// `kryos pkg update` — resolve dependencies and update lock file.
pub fn update() -> Result<(), String> {
    let manifest_path = Path::new("kryos.toml");
    let manifest = load_manifest(manifest_path)?;

    if manifest.dependencies.is_empty() {
        eprintln!("no dependencies to resolve");
        return Ok(());
    }

    // Resolve dependencies using the package resolver.
    let mut registry = kryos_package::resolve::PackageRegistry::new();

    // For path dependencies, register them in the registry so the resolver can find them.
    for (name, dep) in &manifest.dependencies {
        if let kryos_package::DepSpec::Path { path } = dep {
            let dep_manifest_path = Path::new(path).join("kryos.toml");
            if dep_manifest_path.exists() {
                let dep_manifest = Manifest::from_file(&dep_manifest_path)?;
                let version: kryos_package::Version = dep_manifest.package.version.parse()
                    .map_err(|e| format!("bad version in {}: {e}", name))?;
                registry.add(kryos_package::resolve::AvailablePackage {
                    name: name.clone(),
                    version,
                    source: path.clone(),
                    dependencies: std::collections::HashMap::new(),
                });
            }
        }
    }

    // resolve() accepts HashMap<String, DepSpec> directly.
    match kryos_package::resolve::resolve(&manifest.dependencies, &registry) {
        Ok(graph) => {
            // Write lock file.
            let lock = kryos_package::LockFile::from_resolved(&graph);
            lock.write_to_file(Path::new("kryos.lock"))?;

            eprintln!("resolved {} dependencies", graph.packages.len());
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

/// `kryos pkg install` — resolve and fetch all dependencies.
pub fn install() -> Result<(), String> {
    let manifest_path = Path::new("kryos.toml");
    let manifest = load_manifest(manifest_path)?;

    if manifest.dependencies.is_empty() {
        eprintln!("no dependencies to install");
        return Ok(());
    }

    // First resolve.
    let mut registry = kryos_package::resolve::PackageRegistry::new();
    for (name, dep) in &manifest.dependencies {
        if let kryos_package::DepSpec::Path { path } = dep {
            let dep_manifest_path = Path::new(path).join("kryos.toml");
            if dep_manifest_path.exists() {
                let dep_manifest = Manifest::from_file(&dep_manifest_path)?;
                let version: kryos_package::Version = dep_manifest.package.version.parse()
                    .map_err(|e| format!("bad version in {}: {e}", name))?;
                registry.add(kryos_package::resolve::AvailablePackage {
                    name: name.clone(),
                    version,
                    source: path.clone(),
                    dependencies: std::collections::HashMap::new(),
                });
            }
        }
    }

    let graph = kryos_package::resolve::resolve(&manifest.dependencies, &registry)
        .map_err(|e| format!("dependency resolution failed: {e}"))?;

    // Fetch all remote packages.
    let fetched = kryos_package::fetch::fetch_resolved(&graph)?;

    // Write lock file.
    let lock = kryos_package::LockFile::from_resolved(&graph);
    lock.write_to_file(Path::new("kryos.lock"))?;

    eprintln!("installed {} dependencies", fetched.len());
    for (name, path) in &fetched {
        eprintln!("  {} -> {}", name, path.display());
    }
    eprintln!("wrote kryos.lock");

    Ok(())
}

/// `kryos pkg lock` — regenerate the lock file (alias for update).
pub fn lock() -> Result<(), String> {
    update()
}

/// `kryos pkg publish` — package and publish to the registry.
pub fn publish() -> Result<(), String> {
    let project_dir = std::env::current_dir()
        .map_err(|e| format!("cannot determine current directory: {e}"))?;

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
    base.rsplit('/')
        .next()
        .unwrap_or(base)
        .to_string()
}
