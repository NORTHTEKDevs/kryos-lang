//! `kryos pkg` — package management commands.

use std::path::Path;

use kryos_package::Manifest;

/// `kryos pkg init [name]` — create a new kryos.toml.
pub fn init(name: Option<&str>) -> Result<(), String> {
    let manifest_path = Path::new("kryos.toml");

    if manifest_path.exists() {
        return Err("kryos.toml already exists in this directory".to_string());
    }

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

    std::fs::write(manifest_path, &toml_content)
        .map_err(|e| format!("failed to write kryos.toml: {e}"))?;

    // Also create a src/ directory with a main.kry stub if it doesn't exist.
    let src_dir = Path::new("src");
    if !src_dir.exists() {
        std::fs::create_dir_all(src_dir)
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

    eprintln!("initialized Kryos project `{project_name}`");
    eprintln!("  created kryos.toml");
    eprintln!("  created src/main.kry");

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

/// `kryos pkg update` — update all dependencies.
pub fn update() -> Result<(), String> {
    let manifest_path = Path::new("kryos.toml");
    if !manifest_path.exists() {
        return Err("no kryos.toml found".to_string());
    }

    // TODO: Once the package resolver supports fetching from registries,
    // resolve all dependencies to their latest compatible versions and
    // update kryos.lock.
    eprintln!("kryos pkg update: dependency resolution not yet connected");
    Ok(())
}

/// `kryos pkg lock` — regenerate the lock file.
pub fn lock() -> Result<(), String> {
    let manifest_path = Path::new("kryos.toml");
    if !manifest_path.exists() {
        return Err("no kryos.toml found".to_string());
    }

    // TODO: Resolve dependency graph and write kryos.lock.
    eprintln!("kryos pkg lock: lock file generation not yet connected");
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
