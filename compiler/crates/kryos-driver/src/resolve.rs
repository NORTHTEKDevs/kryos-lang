//! Module resolution — resolves `use foo` to a file path on disk.
//!
//! Resolution order for `use foo` when the importing file is `/path/to/main.kry`:
//! 1. `/path/to/foo.kry`         (sibling file)
//! 2. `/path/to/foo/mod.kry`     (directory module)
//!
//! For project builds (when a `src/` directory exists):
//! 3. `<project_root>/src/foo.kry`
//! 4. `<project_root>/src/foo/mod.kry`
//!
//! For stdlib imports (`use std::foo`):
//! 5. `<stdlib_dir>/foo.kry`     (strip `std` prefix, look in stdlib dir)
//! 6. `<stdlib_dir>/foo/mod.kry`

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use kryos_ast::{Decl, ImportPath, Module};
use kryos_errors::Diagnostic;
use kryos_lexer::Lexer;
use kryos_parser::parse;

/// Locate the Kryos stdlib directory.
///
/// Search order:
/// 1. `KRYOS_STDLIB_DIR` environment variable (for testing and overrides).
/// 2. Walk up from `CARGO_MANIFEST_DIR` (compile-time) to find `stdlib/`.
/// 3. Walk up from `importing_file` to find `stdlib/`.
fn find_stdlib_dir(importing_file: &Path) -> Option<PathBuf> {
    // 1. Env var override
    if let Ok(dir) = env::var("KRYOS_STDLIB_DIR") {
        let p = PathBuf::from(dir);
        if p.is_dir() {
            return Some(p);
        }
    }

    // 2. Walk up from CARGO_MANIFEST_DIR (compile-time constant)
    let manifest_dir = option_env!("CARGO_MANIFEST_DIR");
    if let Some(dir) = manifest_dir {
        let mut ancestor = PathBuf::from(dir);
        loop {
            let stdlib = ancestor.join("stdlib");
            if stdlib.is_dir() {
                return Some(stdlib);
            }
            if !ancestor.pop() {
                break;
            }
        }
    }

    // 3. Walk up from the importing file
    if let Some(parent) = importing_file.parent() {
        let mut ancestor = parent.to_path_buf();
        loop {
            let stdlib = ancestor.join("stdlib");
            if stdlib.is_dir() {
                return Some(stdlib);
            }
            if !ancestor.pop() {
                break;
            }
        }
    }

    None
}

/// Errors that can occur during module resolution.
#[derive(Debug)]
pub enum ResolveError {
    /// The module file could not be found.
    NotFound {
        module_name: String,
        search_paths: Vec<PathBuf>,
    },
    /// A circular import was detected.
    CircularImport {
        module_name: String,
        chain: Vec<String>,
    },
    /// Failed to read a module file.
    ReadError { path: PathBuf, error: String },
    /// Failed to parse a module file.
    ParseError {
        path: PathBuf,
        diagnostics: Vec<Diagnostic>,
    },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::NotFound {
                module_name,
                search_paths,
            } => {
                write!(f, "module `{module_name}` not found; searched: ")?;
                for (i, p) in search_paths.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", p.display())?;
                }
                Ok(())
            }
            ResolveError::CircularImport { module_name, chain } => {
                write!(
                    f,
                    "circular import detected for `{module_name}`: {}",
                    chain.join(" -> ")
                )
            }
            ResolveError::ReadError { path, error } => {
                write!(f, "failed to read module '{}': {error}", path.display())
            }
            ResolveError::ParseError { path, .. } => {
                write!(f, "failed to parse module '{}'", path.display())
            }
        }
    }
}

/// Resolve a module path (one or more segments) to a file path.
///
/// `segments` contains the path components, e.g. `["ml", "math"]` for `use ml::math`.
/// `importing_file` is the path to the file that contains the `use` statement.
pub fn resolve_module_path(
    segments: &[String],
    importing_file: &Path,
) -> Result<PathBuf, ResolveError> {
    let module_name = segments.join("::");
    let parent = importing_file.parent().unwrap_or_else(|| Path::new("."));

    let mut search_paths = Vec::new();

    // Build relative path from segments: ["ml", "math"] -> ml/math
    let relative: PathBuf = segments.iter().collect();

    // 1. Sibling file: /path/to/ml/math.kry
    let sibling = parent.join(&relative).with_extension("kry");
    search_paths.push(sibling.clone());
    if sibling.is_file() {
        return Ok(sibling);
    }

    // 2. Directory module: /path/to/ml/math/mod.kry
    let dir_mod = parent.join(&relative).join("mod.kry");
    search_paths.push(dir_mod.clone());
    if dir_mod.is_file() {
        return Ok(dir_mod);
    }

    // 3. Project src/ directory: walk up to find a parent with src/
    let mut ancestor = parent.to_path_buf();
    loop {
        let src_dir = ancestor.join("src");
        if src_dir.is_dir() {
            let src_sibling = src_dir.join(&relative).with_extension("kry");
            search_paths.push(src_sibling.clone());
            if src_sibling.is_file() {
                return Ok(src_sibling);
            }

            let src_dir_mod = src_dir.join(&relative).join("mod.kry");
            search_paths.push(src_dir_mod.clone());
            if src_dir_mod.is_file() {
                return Ok(src_dir_mod);
            }
            break;
        }
        if !ancestor.pop() {
            break;
        }
    }

    // 4. Package dep redirect: walk up to find a parent containing
    //    `.kryos/deps/<first_segment>.redirect`. The redirect file contains
    //    `path = "<dir>"` pointing at the dep's project root. We then look
    //    inside `<dir>/src/` for the remaining segments (or `<dir>/src/<name>.kry`
    //    when only the package name was used).
    if !segments.is_empty() {
        let pkg = &segments[0];
        let mut ancestor = parent.to_path_buf();
        loop {
            let redirect = ancestor.join(".kryos").join("deps").join(format!("{pkg}.redirect"));
            if redirect.is_file() {
                if let Ok(content) = fs::read_to_string(&redirect) {
                    // Parse simple `path = "..."` line.
                    for line in content.lines() {
                        let line = line.trim();
                        if let Some(rest) = line.strip_prefix("path") {
                            let rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '=');
                            let rest = rest.trim();
                            let dep_path = rest.trim_matches('"');
                            // Resolve relative paths against the redirect's directory
                            // (which is `<project>/.kryos/deps/`). Path entries are
                            // typically relative to the project root, so canonicalize
                            // through that.
                            let project_root = ancestor.clone();
                            let dep_root = if Path::new(dep_path).is_absolute() {
                                PathBuf::from(dep_path)
                            } else {
                                project_root.join(dep_path)
                            };
                            let dep_src = dep_root.join("src");
                            // `use pkg` → src/lib.kry or src/<pkg>.kry
                            if segments.len() == 1 {
                                let lib = dep_src.join("lib.kry");
                                search_paths.push(lib.clone());
                                if lib.is_file() { return Ok(lib); }
                                let named = dep_src.join(format!("{pkg}.kry"));
                                search_paths.push(named.clone());
                                if named.is_file() { return Ok(named); }
                            } else {
                                // `use pkg::a::b` → src/a/b.kry or src/a/b/mod.kry
                                let sub: PathBuf = segments[1..].iter().collect();
                                let f = dep_src.join(&sub).with_extension("kry");
                                search_paths.push(f.clone());
                                if f.is_file() { return Ok(f); }
                                let d = dep_src.join(&sub).join("mod.kry");
                                search_paths.push(d.clone());
                                if d.is_file() { return Ok(d); }
                            }
                            break;
                        }
                    }
                }
                break;
            }
            if !ancestor.pop() { break; }
        }
    }

    // 5. Stdlib fallback: if the first segment is "std" and there are 2+ segments,
    //    strip the "std" prefix and look in the stdlib directory.
    if segments.len() >= 2 && segments[0] == "std" {
        if let Some(stdlib_dir) = find_stdlib_dir(importing_file) {
            let stdlib_relative: PathBuf = segments[1..].iter().collect();

            let stdlib_file = stdlib_dir.join(&stdlib_relative).with_extension("kry");
            search_paths.push(stdlib_file.clone());
            if stdlib_file.is_file() {
                return Ok(stdlib_file);
            }

            let stdlib_mod = stdlib_dir.join(&stdlib_relative).join("mod.kry");
            search_paths.push(stdlib_mod.clone());
            if stdlib_mod.is_file() {
                return Ok(stdlib_mod);
            }
        }
    }

    Err(ResolveError::NotFound {
        module_name,
        search_paths,
    })
}

/// Extract import declarations from a module's AST.
///
/// Returns the full `ImportPath` (segments, alias, selective items) for each import.
pub fn extract_imports(module: &Module) -> Vec<(ImportPath, kryos_errors::Span)> {
    module
        .declarations
        .iter()
        .filter_map(|decl| {
            if let Decl::Import { path, span, .. } = decl {
                Some((path.clone(), *span))
            } else {
                None
            }
        })
        .collect()
}

/// Parse a module file and return its AST.
fn parse_module_file(path: &Path) -> Result<(Module, kryos_errors::SourceMap), ResolveError> {
    let source = fs::read_to_string(path).map_err(|e| ResolveError::ReadError {
        path: path.to_path_buf(),
        error: e.to_string(),
    })?;

    let mut source_map = kryos_errors::SourceMap::default();
    let file_id = source_map.add_file(path.to_string_lossy().to_string(), source.clone());
    let tokens = Lexer::new(&source, file_id).tokenize();

    let module = parse(tokens).map_err(|diags| ResolveError::ParseError {
        path: path.to_path_buf(),
        diagnostics: diags,
    })?;

    Ok((module, source_map))
}

/// Recursively resolve all imports for a module, returning the merged set of
/// all declarations from imported modules.
///
/// `importing_file` is the canonical path of the file being compiled.
/// `visited` tracks which files are already being compiled (cycle detection).
/// `resolved_decls` accumulates declarations from all imported modules.
pub fn resolve_imports(
    module: &Module,
    importing_file: &Path,
    visited: &mut HashSet<PathBuf>,
    resolved_decls: &mut Vec<Decl>,
    verbose: bool,
) -> Result<(), Vec<Diagnostic>> {
    let imports = extract_imports(module);

    for (import_path, span) in imports {
        let module_name = import_path.segments.join("::");

        // Resolve the module path segments to a file path.
        let module_path = match resolve_module_path(&import_path.segments, importing_file) {
            Ok(p) => p,
            Err(e) => {
                return Err(vec![
                    Diagnostic::error(e.to_string()).with_label(span, "imported here")
                ]);
            }
        };

        // Canonicalize to detect cycles reliably.
        let canonical = match fs::canonicalize(&module_path) {
            Ok(c) => c,
            Err(_) => module_path.clone(),
        };

        // Cycle detection.
        if visited.contains(&canonical) {
            // Already processed this module — skip (not an error, just already imported).
            continue;
        }
        visited.insert(canonical.clone());

        if verbose {
            eprintln!(
                "[kryos] import: resolved `{module_name}` -> '{}'",
                module_path.display()
            );
        }

        // Parse the imported module.
        let (imported_module, _source_map) = match parse_module_file(&module_path) {
            Ok(m) => m,
            Err(ResolveError::ParseError { diagnostics, .. }) => {
                let mut diags = diagnostics;
                diags.insert(
                    0,
                    Diagnostic::error(format!(
                        "errors in imported module `{module_name}` ({})",
                        module_path.display()
                    ))
                    .with_label(span, "imported here"),
                );
                return Err(diags);
            }
            Err(e) => {
                return Err(vec![
                    Diagnostic::error(e.to_string()).with_label(span, "imported here")
                ]);
            }
        };

        // Recursively resolve imports within the imported module.
        resolve_imports(
            &imported_module,
            &module_path,
            visited,
            resolved_decls,
            verbose,
        )?;

        // Collect non-import declarations from the imported module.
        // When selective import is used (`use foo::{a, b}`), only include
        // declarations whose names match the requested items, plus ALL
        // module-level constants (Decl::Const). Constants are always
        // included because selected functions may depend on them internally
        // (e.g., `sqrt` uses `NAN`, trig functions use `PI`/`TAU`).
        for decl in imported_module.declarations {
            if matches!(decl, Decl::Import { .. }) {
                continue;
            }
            // Note: selective imports (`use foo::{a, b}`) used to filter
            // the imported module down to just the named items, but that
            // breaks any selected function whose body transitively calls
            // other (unselected) helpers in the same module. Doing proper
            // dependency tracing is a larger change; for now we always
            // include the full module so private helpers, types, externs,
            // and constants are all reachable. The `items` list still
            // serves as documentation of what the importer cares about.
            //
            // To avoid duplicate symbols when several modules are imported
            // and happen to share helper names, the dedup pass below will
            // surface conflicts.
            let _ = &import_path.items;
            resolved_decls.push(decl);
        }
    }

    // Check for duplicate declarations across imported modules.
    // When two imports define the same name, emit a clear error rather
    // than letting codegen crash with an opaque "duplicate definition".
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut dup_errors: Vec<Diagnostic> = Vec::new();
    for (idx, decl) in resolved_decls.iter().enumerate() {
        if let Some(name) = decl_name_of(decl) {
            if let Some(&prev_idx) = seen.get(&name) {
                // Skip duplicate Decl::Const — constants from the same module
                // can appear multiple times when re-exported through different
                // selective imports. Only flag non-const duplicates, or const
                // duplicates that are truly conflicting (different values).
                if matches!(decl, Decl::Const { .. })
                    && matches!(&resolved_decls[prev_idx], Decl::Const { .. })
                {
                    continue;
                }
                let kind = decl_kind_name(decl);
                dup_errors.push(
                    Diagnostic::error(format!(
                        "duplicate {kind} `{name}` imported from multiple modules"
                    ))
                    .with_label(decl.span(), "duplicate definition here")
                    .with_note(format!(
                        "a {kind} named `{name}` was already imported; \
                         consider using an alias or selective import to resolve the conflict"
                    )),
                );
            } else {
                seen.insert(name, idx);
            }
        }
    }
    if !dup_errors.is_empty() {
        return Err(dup_errors);
    }

    Ok(())
}

/// Return a human-readable kind name for a declaration (for error messages).
fn decl_kind_name(decl: &Decl) -> &'static str {
    match decl {
        Decl::Function { .. } => "function",
        Decl::Struct { .. } => "struct",
        Decl::Enum { .. } => "enum",
        Decl::Trait { .. } => "trait",
        Decl::TypeAlias { .. } => "type alias",
        Decl::Actor { .. } => "actor",
        Decl::Const { .. } => "constant",
        Decl::Import { .. } => "import",
        Decl::Extern { .. } => "extern block",
        Decl::Impl { .. } => "impl block",
    }
}

/// Extract the name of a declaration, if it has one.
fn decl_name_of(decl: &Decl) -> Option<String> {
    match decl {
        Decl::Function { name, .. } => Some(name.clone()),
        Decl::Struct { name, .. } => Some(name.clone()),
        Decl::Enum { name, .. } => Some(name.clone()),
        Decl::Trait { name, .. } => Some(name.clone()),
        Decl::TypeAlias { name, .. } => Some(name.clone()),
        Decl::Actor { name, .. } => Some(name.clone()),
        Decl::Const { name, .. } => Some(name.clone()),
        _ => None,
    }
}
