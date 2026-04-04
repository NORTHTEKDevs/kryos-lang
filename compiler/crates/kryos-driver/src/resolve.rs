//! Module resolution — resolves `use foo` to a file path on disk.
//!
//! Resolution order for `use foo` when the importing file is `/path/to/main.kry`:
//! 1. `/path/to/foo.kry`         (sibling file)
//! 2. `/path/to/foo/mod.kry`     (directory module)
//!
//! For project builds (when a `src/` directory exists):
//! 3. `<project_root>/src/foo.kry`
//! 4. `<project_root>/src/foo/mod.kry`

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use kryos_ast::{Decl, Module};
use kryos_errors::Diagnostic;
use kryos_lexer::Lexer;
use kryos_parser::parse;

/// Errors that can occur during module resolution.
#[derive(Debug)]
pub enum ResolveError {
    /// The module file could not be found.
    NotFound { module_name: String, search_paths: Vec<PathBuf> },
    /// A circular import was detected.
    CircularImport { module_name: String, chain: Vec<String> },
    /// Failed to read a module file.
    ReadError { path: PathBuf, error: String },
    /// Failed to parse a module file.
    ParseError { path: PathBuf, diagnostics: Vec<Diagnostic> },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::NotFound { module_name, search_paths } => {
                write!(f, "module `{module_name}` not found; searched: ")?;
                for (i, p) in search_paths.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", p.display())?;
                }
                Ok(())
            }
            ResolveError::CircularImport { module_name, chain } => {
                write!(f, "circular import detected for `{module_name}`: {}", chain.join(" -> "))
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

/// Resolve a module name to a file path.
///
/// `importing_file` is the path to the file that contains the `use` statement.
pub fn resolve_module_path(module_name: &str, importing_file: &Path) -> Result<PathBuf, ResolveError> {
    let parent = importing_file
        .parent()
        .unwrap_or_else(|| Path::new("."));

    let mut search_paths = Vec::new();

    // 1. Sibling file: /path/to/<module_name>.kry
    let sibling = parent.join(format!("{module_name}.kry"));
    search_paths.push(sibling.clone());
    if sibling.is_file() {
        return Ok(sibling);
    }

    // 2. Directory module: /path/to/<module_name>/mod.kry
    let dir_mod = parent.join(module_name).join("mod.kry");
    search_paths.push(dir_mod.clone());
    if dir_mod.is_file() {
        return Ok(dir_mod);
    }

    // 3. Project src/ directory: walk up to find a parent with src/
    let mut ancestor = parent.to_path_buf();
    loop {
        let src_dir = ancestor.join("src");
        if src_dir.is_dir() {
            let src_sibling = src_dir.join(format!("{module_name}.kry"));
            search_paths.push(src_sibling.clone());
            if src_sibling.is_file() {
                return Ok(src_sibling);
            }

            let src_dir_mod = src_dir.join(module_name).join("mod.kry");
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

    Err(ResolveError::NotFound {
        module_name: module_name.to_string(),
        search_paths,
    })
}

/// Extract import declarations from a module's AST.
///
/// Returns the module name (first segment of the import path) for each import.
pub fn extract_imports(module: &Module) -> Vec<(String, kryos_errors::Span)> {
    module
        .declarations
        .iter()
        .filter_map(|decl| {
            if let Decl::Import { path, span } = decl {
                // Use the first segment as the module name.
                // e.g., `use math` -> "math", `use std::io` -> "std"
                path.segments.first().map(|name| (name.clone(), *span))
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

    for (module_name, span) in imports {
        // Resolve the module name to a file path.
        let module_path = match resolve_module_path(&module_name, importing_file) {
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
        for decl in imported_module.declarations {
            if !matches!(decl, Decl::Import { .. }) {
                resolved_decls.push(decl);
            }
        }
    }

    Ok(())
}
