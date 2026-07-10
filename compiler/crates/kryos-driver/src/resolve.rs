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

use kryos_ast::{Block, Decl, Expr, ImportPath, Module, Stmt, StringPart};
use kryos_errors::Diagnostic;
use kryos_lexer::Lexer;
use kryos_parser::parse;

/// Locate the Kryos stdlib directory.
///
/// Search order:
/// 1. `KRYOS_STDLIB_DIR` environment variable (for testing and overrides).
/// 2. `<exe_dir>/stdlib` or `<exe_dir>/../stdlib` (distribution layouts).
/// 3. Walk up from `CARGO_MANIFEST_DIR` (compile-time) to find `stdlib/`.
/// 4. Walk up from `importing_file` to find `stdlib/`.
/// Diagnostics helper (`kryos doctor`): where the stdlib would resolve for a
/// file in the current working directory. Same order as real imports.
pub fn resolve_stdlib_dir_for_diagnostics() -> Option<PathBuf> {
    find_stdlib_dir(Path::new("."))
}

fn find_stdlib_dir(importing_file: &Path) -> Option<PathBuf> {
    // 1. Env var override
    if let Ok(dir) = env::var("KRYOS_STDLIB_DIR") {
        let p = PathBuf::from(dir);
        if p.is_dir() {
            return Some(p);
        }
    }

    // 2. Relative to the running executable (distribution layouts).
    //    Release archives ship `stdlib/` next to the binary (`kryos.exe` at
    //    the root) or one level up (`bin/kryos.exe`). Without this, a
    //    downloaded release can only find its own stdlib via the env var --
    //    the compile-time CARGO_MANIFEST_DIR below points at the build
    //    machine's checkout, which does not exist on user machines.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            for candidate in [exe_dir.join("stdlib"), exe_dir.join("..").join("stdlib")] {
                if candidate.is_dir() {
                    return Some(candidate);
                }
            }
        }
    }

    // 3. Walk up from CARGO_MANIFEST_DIR (compile-time constant)
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

    // 4. Walk up from the importing file
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
use std::cell::RefCell;

thread_local! {
    /// fn-name -> module last-segment it was imported from, recorded during
    /// the current resolve pass. Used to validate module-qualified calls:
    /// `json::parse(..)` is pure sugar for the flat name `parse`, so without
    /// this check it silently bound to WHATEVER `parse` was in scope (e.g.
    /// std::csv's) -- a wrong-binding with no diagnostic.
    static IMPORT_ORIGINS: RefCell<std::collections::HashMap<String, String>> =
        RefCell::new(std::collections::HashMap::new());
}

/// Reset the per-compile import-origin table. Call before a fresh resolve.
pub fn reset_import_origins() {
    IMPORT_ORIGINS.with(|o| o.borrow_mut().clear());
}

fn record_origin(fn_name: &str, module_last: &str) {
    IMPORT_ORIGINS.with(|o| {
        o.borrow_mut()
            .insert(fn_name.to_string(), module_last.to_string());
    });
}

/// Validate every module-qualified call (`mod::fn(..)`) in the ROOT module
/// against the recorded import origins. Returns diagnostics for calls whose
/// qualifier names an imported module but whose function either was not
/// imported from it or came from a DIFFERENT module (the silent-misbinding
/// case). Method calls on values are unaffected: the check only fires when
/// the receiver identifier exactly matches an imported module's last segment.
pub fn validate_qualified_calls(module: &Module) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let (origins, modules): (std::collections::HashMap<String, String>, std::collections::HashSet<String>) =
        IMPORT_ORIGINS.with(|o| {
            let m = o.borrow();
            let mods = m.values().cloned().collect();
            (m.clone(), mods)
        });
    if modules.is_empty() {
        return diags;
    }
    let mut calls: Vec<(String, String, kryos_errors::Span)> = Vec::new();
    for decl in &module.declarations {
        collect_qualified_calls_in_decl(decl, &mut calls);
    }
    for (recv, method, span) in calls {
        if !modules.contains(&recv) {
            continue; // not a module qualifier (a value/static-type receiver)
        }
        match origins.get(&method) {
            Some(origin) if origin == &recv => {}
            Some(origin) => {
                diags.push(
                    Diagnostic::error(format!(
                        "`{recv}::{method}` refers to `{method}` imported from `{origin}`, not `{recv}`"
                    ))
                    .with_label(span, "qualified call here")
                    .with_note(format!(
                        "`{method}` in scope came from `{origin}`; import `{method}` from `{recv}` instead (and drop the conflicting import) -- Kryos has no import aliasing"
                    )),
                );
            }
            None => {
                diags.push(
                    Diagnostic::error(format!(
                        "`{recv}::{method}` is not imported: add `{method}` to the `use` list for `{recv}`"
                    ))
                    .with_label(span, "qualified call here"),
                );
            }
        }
    }
    diags
}

fn collect_qualified_calls_in_decl(d: &Decl, out: &mut Vec<(String, String, kryos_errors::Span)>) {
    match d {
        Decl::Function { body: Some(b), .. } => collect_qualified_calls_in_block(b, out),
        Decl::Impl { methods, .. } => {
            for m in methods {
                collect_qualified_calls_in_decl(m, out);
            }
        }
        _ => {}
    }
}

fn collect_qualified_calls_in_block(b: &Block, out: &mut Vec<(String, String, kryos_errors::Span)>) {
    for st in &b.stmts {
        collect_qualified_calls_in_stmt(st, out);
    }
}

fn collect_qualified_calls_in_stmt(s: &Stmt, out: &mut Vec<(String, String, kryos_errors::Span)>) {
    match s {
        Stmt::Let { value: Some(e), .. } => collect_qualified_calls_in_expr(e, out),
        Stmt::Assign { target, value, .. } => {
            collect_qualified_calls_in_expr(target, out);
            collect_qualified_calls_in_expr(value, out);
        }
        Stmt::Expr { expr, .. } => collect_qualified_calls_in_expr(expr, out),
        Stmt::Return { value: Some(e), .. } => collect_qualified_calls_in_expr(e, out),
        Stmt::Throw { expr, .. } => collect_qualified_calls_in_expr(expr, out),
        Stmt::Spawn { expr, .. } => collect_qualified_calls_in_expr(expr, out),
        Stmt::If { condition, then_block, elif_clauses, else_block, .. } => {
            collect_qualified_calls_in_expr(condition, out);
            collect_qualified_calls_in_block(then_block, out);
            for (c, b) in elif_clauses {
                collect_qualified_calls_in_expr(c, out);
                collect_qualified_calls_in_block(b, out);
            }
            if let Some(b) = else_block {
                collect_qualified_calls_in_block(b, out);
            }
        }
        Stmt::While { condition, body, .. } => {
            collect_qualified_calls_in_expr(condition, out);
            collect_qualified_calls_in_block(body, out);
        }
        Stmt::For { iterable, body, .. } => {
            collect_qualified_calls_in_expr(iterable, out);
            collect_qualified_calls_in_block(body, out);
        }
        Stmt::TryCatch { try_block, catch_block, .. } => {
            collect_qualified_calls_in_block(try_block, out);
            collect_qualified_calls_in_block(catch_block, out);
        }
        _ => {}
    }
}

fn collect_qualified_calls_in_expr(e: &Expr, out: &mut Vec<(String, String, kryos_errors::Span)>) {
    match e {
        Expr::MethodCall { object, method, args, span, .. } => {
            if let Expr::Identifier { name, .. } = object.as_ref() {
                out.push((name.clone(), method.clone(), *span));
            } else {
                collect_qualified_calls_in_expr(object, out);
            }
            for a in args {
                collect_qualified_calls_in_expr(a, out);
            }
        }
        // `mod::fn(..)` parses as StaticMethodCall (the `::` spelling);
        // `mod.fn(..)` as MethodCall. Both are module-qualified sugar.
        Expr::StaticMethodCall { type_name, method, args, span, .. } => {
            out.push((type_name.clone(), method.clone(), *span));
            for a in args {
                collect_qualified_calls_in_expr(a, out);
            }
        }
        Expr::FnCall { callee, args, .. } => {
            collect_qualified_calls_in_expr(callee, out);
            for a in args {
                collect_qualified_calls_in_expr(a, out);
            }
        }
        Expr::BinaryOp { left, right, .. } => {
            collect_qualified_calls_in_expr(left, out);
            collect_qualified_calls_in_expr(right, out);
        }
        Expr::UnaryOp { operand, .. } => collect_qualified_calls_in_expr(operand, out),
        Expr::FieldAccess { object, .. } => collect_qualified_calls_in_expr(object, out),
        Expr::IndexAccess { object, index, .. } => {
            collect_qualified_calls_in_expr(object, out);
            collect_qualified_calls_in_expr(index, out);
        }
        Expr::IfExpr { condition, then_branch, else_branch, .. } => {
            collect_qualified_calls_in_expr(condition, out);
            collect_qualified_calls_in_block(then_branch, out);
            if let Some(b) = else_branch {
                collect_qualified_calls_in_block(b, out);
            }
        }
        _ => {}
    }
}

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
        // When selective import is used (`use foo::{a, b}`), include:
        //   1. All non-Function decls (types, traits, impls, constants,
        //      externs) — these are cheap and often referenced silently
        //      by selected functions' parameter / return types.
        //   2. Functions whose names are in the transitive closure of
        //      `import_path.items` over identifier / FnCall references.
        //      So `use math::{clamp}` pulls in `clamp` + (transitively)
        //      `min` and `max` because clamp() calls them.
        // Empty `items` (`use foo`) means "import everything".
        let selected: HashSet<String> = import_path
            .items
            .iter()
            .cloned()
            .collect();

        // Validate every selected name against the module's exportable
        // declarations. Without this, `use std::datetime::{from_unix}`
        // for a name the module does not define succeeded silently and the
        // failure surfaced later as a misleading "undefined variable
        // `from_unix`" at the CALL site instead of at the import.
        if !selected.is_empty() {
            let mut exported: HashSet<String> = imported_module
                .declarations
                .iter()
                .filter_map(decl_name_of)
                .collect();
            // Enum VARIANTS are importable by name (`use std::result::{Ok,
            // Err}` is the documented pattern), and extern items are callable
            // exports; neither is a top-level named decl, so include both.
            for decl in &imported_module.declarations {
                match decl {
                    Decl::Enum { variants, .. } => {
                        for v in variants {
                            exported.insert(v.name.clone());
                        }
                    }
                    Decl::Extern { items, .. } => {
                        for item in items {
                            if let Some(n) = decl_name_of(item) {
                                exported.insert(n);
                            }
                        }
                    }
                    _ => {}
                }
            }
            let mut missing: Vec<Diagnostic> = Vec::new();
            for name in &selected {
                if !exported.contains(name) {
                    // Suggest a close match to catch typos.
                    let suggestion = exported
                        .iter()
                        .filter(|e| {
                            let a = e.to_lowercase();
                            let b = name.to_lowercase();
                            a.contains(&b) || b.contains(&a)
                        })
                        .min_by_key(|e| e.len());
                    let mut d = Diagnostic::error(format!(
                        "module `{module_name}` has no export `{name}`"
                    ))
                    .with_label(span, "imported here");
                    if let Some(sug) = suggestion {
                        d = d.with_note(format!("did you mean `{sug}`?"));
                    }
                    missing.push(d);
                }
            }
            if !missing.is_empty() {
                return Err(missing);
            }
        }

        if selected.is_empty() {
            for decl in imported_module.declarations {
                // An imported module's `main` is an ENTRY POINT, not an
                // export: pulling it in alongside the importer's own main
                // produced "Duplicate definition of _kryos_main" (a runnable
                // library like examples/mylib.kry ships a main() so it can be
                // run directly, and `use mylib` re-imported it).
                if let Decl::Function { ref name, .. } = decl {
                    if name == "main" {
                        continue;
                    }
                    if let Some(last) = import_path.segments.last() {
                        record_origin(name, last);
                    }
                }
                if !matches!(decl, Decl::Import { .. }) {
                    resolved_decls.push(decl);
                }
            }
        } else {
            // Build name → Decl-index for fast lookup during the closure.
            let fn_by_name: HashMap<String, usize> = imported_module
                .declarations
                .iter()
                .enumerate()
                .filter_map(|(i, d)| match d {
                    Decl::Function { name, .. } => Some((name.clone(), i)),
                    _ => None,
                })
                .collect();

            // Transitive closure of identifier references over selected
            // function bodies. Seeded with `selected`. New names that
            // resolve to functions in this module get added; missing names
            // (builtins, types, helpers from other modules) are ignored —
            // the type-checker will resolve / diagnose them later.
            let mut needed: HashSet<String> = selected.clone();
            let mut worklist: Vec<String> = selected.into_iter().collect();
            // Impl blocks (and actors/consts) are ALWAYS included below, so
            // every module-local function their bodies reference must come
            // along too. Without this, `use m::{SomeStruct}` imported the
            // struct's methods but not the helper fns they call -- consumers
            // got "undefined variable `cost_add`" from inside Budget.charge
            // unless they imported the helper themselves.
            for decl in imported_module.declarations.iter() {
                if !matches!(decl, Decl::Function { .. } | Decl::Import { .. }) {
                    let mut refs: HashSet<String> = HashSet::new();
                    collect_idents_in_decl(decl, &mut refs);
                    for r in refs {
                        if !needed.contains(&r) {
                            needed.insert(r.clone());
                            worklist.push(r);
                        }
                    }
                }
            }
            while let Some(name) = worklist.pop() {
                if let Some(&idx) = fn_by_name.get(&name) {
                    let mut refs: HashSet<String> = HashSet::new();
                    collect_idents_in_decl(&imported_module.declarations[idx], &mut refs);
                    for r in refs {
                        if !needed.contains(&r) {
                            needed.insert(r.clone());
                            worklist.push(r);
                        }
                    }
                }
            }

            for decl in imported_module.declarations {
                match &decl {
                    Decl::Import { .. } => continue,
                    Decl::Function { name, .. } => {
                        if name != "main" && needed.contains(name) {
                            if let Some(last) = import_path.segments.last() {
                                record_origin(name, last);
                            }
                            resolved_decls.push(decl);
                        }
                    }
                    // Always include types, constants, externs, impls,
                    // traits, actors, type aliases. Selecting `{a, b}`
                    // shouldn't hide the struct definitions they need.
                    _ => resolved_decls.push(decl),
                }
            }
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
                                        .with_note(format!(
                        "a {kind} named `{name}` was already imported; \
                         Kryos has no import aliasing -- import disjoint names selectively so only one is in scope"
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

// ─── Identifier collection (selective-import transitive closure) ───────────

/// Collect every identifier name appearing inside a decl's body. Used by
/// selective-import resolution: a function selected via `use foo::{bar}`
/// transitively pulls in any other in-module function whose name appears
/// in its body. Conservative — we collect ALL identifier names, including
/// local variables. Non-function names get filtered out at the call site.
fn collect_idents_in_decl(d: &Decl, out: &mut HashSet<String>) {
    match d {
        Decl::Function { body: Some(b), .. } => collect_idents_in_block(b, out),
        Decl::Impl { methods, .. } | Decl::Trait { methods, .. } => {
            for m in methods {
                collect_idents_in_decl(m, out);
            }
        }
        Decl::Const { value, .. } => collect_idents_in_expr(value, out),
        _ => {}
    }
}

fn collect_idents_in_block(b: &Block, out: &mut HashSet<String>) {
    for stmt in &b.stmts {
        collect_idents_in_stmt(stmt, out);
    }
}

fn collect_idents_in_stmt(s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::Let { value, .. } => {
            if let Some(v) = value {
                collect_idents_in_expr(v, out);
            }
        }
        Stmt::Assign { target, value, .. } => {
            collect_idents_in_expr(target, out);
            collect_idents_in_expr(value, out);
        }
        Stmt::Return { value: Some(v), .. } => collect_idents_in_expr(v, out),
        Stmt::Return { value: None, .. } => {}
        Stmt::If {
            condition,
            then_block,
            elif_clauses,
            else_block,
            ..
        } => {
            collect_idents_in_expr(condition, out);
            collect_idents_in_block(then_block, out);
            for (c, b) in elif_clauses {
                collect_idents_in_expr(c, out);
                collect_idents_in_block(b, out);
            }
            if let Some(b) = else_block {
                collect_idents_in_block(b, out);
            }
        }
        Stmt::For {
            iterable, body, ..
        } => {
            collect_idents_in_expr(iterable, out);
            collect_idents_in_block(body, out);
        }
        Stmt::While {
            condition, body, ..
        } => {
            collect_idents_in_expr(condition, out);
            collect_idents_in_block(body, out);
        }
        Stmt::Expr { expr, .. }
        | Stmt::Spawn { expr, .. }
        | Stmt::Throw { expr, .. } => collect_idents_in_expr(expr, out),
        Stmt::Select {
            branches, timeout, ..
        } => {
            for br in branches {
                collect_idents_in_expr(&br.channel, out);
                collect_idents_in_block(&br.body, out);
            }
            if let Some(t) = timeout {
                collect_idents_in_expr(&t.duration_ms, out);
                collect_idents_in_block(&t.body, out);
            }
        }
        Stmt::TryCatch {
            try_block,
            catch_block,
            ..
        } => {
            collect_idents_in_block(try_block, out);
            collect_idents_in_block(catch_block, out);
        }
        Stmt::DenyBlock { body, .. } => collect_idents_in_block(body, out),
        Stmt::Break { .. } | Stmt::Continue { .. } => {}
    }
}

fn collect_idents_in_expr(e: &Expr, out: &mut HashSet<String>) {
    match e {
        Expr::Identifier { name, .. } => {
            out.insert(name.clone());
        }
        Expr::FieldAccess { object, .. } => collect_idents_in_expr(object, out),
        Expr::IndexAccess { object, index, .. } => {
            collect_idents_in_expr(object, out);
            collect_idents_in_expr(index, out);
        }
        Expr::BinaryOp { left, right, .. } => {
            collect_idents_in_expr(left, out);
            collect_idents_in_expr(right, out);
        }
        Expr::UnaryOp { operand, .. } => collect_idents_in_expr(operand, out),
        Expr::FnCall { callee, args, .. } => {
            collect_idents_in_expr(callee, out);
            for a in args {
                collect_idents_in_expr(a, out);
            }
        }
        Expr::MethodCall { object, args, .. } => {
            collect_idents_in_expr(object, out);
            for a in args {
                collect_idents_in_expr(a, out);
            }
        }
        Expr::StaticMethodCall { type_name, args, .. } => {
            out.insert(type_name.clone());
            for a in args {
                collect_idents_in_expr(a, out);
            }
        }
        Expr::ArrayLiteral { elements, .. }
        | Expr::TupleLiteral { elements, .. } => {
            for el in elements {
                collect_idents_in_expr(el, out);
            }
        }
        Expr::MapLiteral { entries, .. } => {
            for (k, v) in entries {
                collect_idents_in_expr(k, out);
                collect_idents_in_expr(v, out);
            }
        }
        Expr::StructLiteral { name, fields, .. } => {
            out.insert(name.clone());
            for (_, v) in fields {
                collect_idents_in_expr(v, out);
            }
        }
        Expr::Lambda { body, .. } => collect_idents_in_expr(body, out),
        // Control-flow / wrapper expressions: recurse into sub-expressions so a
        // selectively-imported fn that references a module-local helper ONLY
        // inside (e.g.) a match arm, if-expression, pipe, cast, or interpolation
        // still drags that helper into the import closure. Previously the
        // catch-all `_ => {}` skipped these -> silent E0102 on valid selective
        // imports (match/Result/Option helpers are pervasive in the stdlib).
        Expr::IfExpr { condition, then_branch, else_branch, .. } => {
            collect_idents_in_expr(condition, out);
            collect_idents_in_block(then_branch, out);
            if let Some(eb) = else_branch {
                collect_idents_in_block(eb, out);
            }
        }
        Expr::MatchExpr { subject, arms, .. } => {
            collect_idents_in_expr(subject, out);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    collect_idents_in_expr(g, out);
                }
                collect_idents_in_expr(&arm.body, out);
            }
        }
        Expr::RangeExpr { start, end, .. } => {
            if let Some(s) = start {
                collect_idents_in_expr(s, out);
            }
            if let Some(en) = end {
                collect_idents_in_expr(en, out);
            }
        }
        Expr::PipeExpr { left, right, .. } => {
            collect_idents_in_expr(left, out);
            collect_idents_in_expr(right, out);
        }
        Expr::Borrow { inner, .. }
        | Expr::Deref { inner, .. }
        | Expr::SharedExpr { inner, .. }
        | Expr::MoveExpr { inner, .. }
        | Expr::WeakExpr { inner, .. } => collect_idents_in_expr(inner, out),
        Expr::Cast { expr, .. } => collect_idents_in_expr(expr, out),
        Expr::Await { value, .. } => collect_idents_in_expr(value, out),
        Expr::Block { block, .. }
        | Expr::ComptimeBlock { body: block, .. }
        | Expr::QuantumBlock { body: block, .. } => collect_idents_in_block(block, out),
        Expr::InterpolatedString { parts, .. } => {
            for p in parts {
                if let StringPart::Expr(pe) = p {
                    collect_idents_in_expr(pe, out);
                }
            }
        }
        // True leaves (int/float/string/bool literals, etc.) contribute no idents.
        _ => {}
    }
}
