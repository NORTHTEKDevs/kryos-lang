//! Compilation pipeline — orchestrates the full source-to-binary flow.
//!
//! The pipeline runs each compiler pass in sequence, collecting diagnostics
//! at every stage and bailing out early if errors are found.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use kryos_capabilities::check_capabilities;
use kryos_errors::{Diagnostic, SourceMap};
use kryos_lexer::Lexer;
use kryos_mir::MirModule;
use kryos_ownership::analyze_ownership;
use kryos_parser::parse;
use kryos_types::type_check;

use crate::config::{BuildConfig, OutputType};
use crate::resolve;

// ---------------------------------------------------------------------------
// Backend trait — codegen backends implement this
// ---------------------------------------------------------------------------

/// Trait for codegen backends.
///
/// Backends (`kryos-codegen-cranelift`, `kryos-codegen-llvm`) are not compiled
/// in as direct dependencies. Instead they implement this trait and are provided
/// by the CLI or embedding application at call time.
pub trait Backend {
    /// Compile a MIR module to object code bytes.
    fn compile(&self, module: &MirModule) -> Result<Vec<u8>, BackendError>;

    /// Emit LLVM IR text (only meaningful for the LLVM backend; others
    /// should return `Err(BackendError::unsupported(...))`).
    fn emit_ir(&self, module: &MirModule) -> Result<String, BackendError>;

    /// Human-readable name for this backend (e.g. "cranelift", "llvm").
    fn name(&self) -> &str;
}

/// Errors that a codegen backend can produce.
#[derive(Debug, Clone)]
pub struct BackendError {
    pub message: String,
}

impl BackendError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }

    pub fn unsupported(feature: &str) -> Self {
        Self {
            message: format!("unsupported: {feature}"),
        }
    }
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "backend error: {}", self.message)
    }
}

impl std::error::Error for BackendError {}

// ---------------------------------------------------------------------------
// CompileResult
// ---------------------------------------------------------------------------

/// The result of running the compilation pipeline.
#[derive(Debug)]
pub struct CompileResult {
    /// All diagnostics accumulated across every compiler pass.
    pub diagnostics: Vec<Diagnostic>,
    /// Source map used during compilation (for rendering diagnostics).
    pub source_map: SourceMap,
    /// Whether the compilation completed without any errors.
    pub success: bool,
    /// Path to the produced artifact, if any.
    pub output_path: Option<String>,
    /// The lowered MIR module, if compilation reached the MIR stage.
    pub mir: Option<MirModule>,
    /// Object code bytes from the codegen backend, if a backend was used.
    pub object_bytes: Option<Vec<u8>>,
    /// LLVM IR text, if `OutputType::LlvmIr` was requested and a backend provided it.
    pub llvm_ir: Option<String>,
}

impl CompileResult {
    /// Create an error result from a single message (used for I/O failures, etc.).
    fn from_error(msg: impl Into<String>) -> Self {
        Self {
            diagnostics: vec![Diagnostic::error(msg)],
            source_map: SourceMap::default(),
            success: false,
            output_path: None,
            mir: None,
            object_bytes: None,
            llvm_ir: None,
        }
    }

    /// Return only the error-level diagnostics.
    pub fn errors(&self) -> Vec<&Diagnostic> {
        self.diagnostics.iter().filter(|d| d.is_error()).collect()
    }

    /// Return the count of error-level diagnostics.
    pub fn error_count(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.is_error()).count()
    }
}

// ---------------------------------------------------------------------------
// compile_file
// ---------------------------------------------------------------------------

/// Compile a single `.kry` source file through the full pipeline.
///
/// Runs: lex -> parse -> type check -> ownership -> capabilities -> MIR lower.
/// If a `backend` is provided and the output type requires codegen, it will
/// also invoke the backend. Otherwise, compilation stops after MIR.
///
/// When called without a backend (the common case during early development),
/// the pipeline still validates the source and produces MIR — it just cannot
/// emit object code or LLVM IR.
pub fn compile_file(path: &Path, config: &BuildConfig) -> CompileResult {
    compile_file_with_backend(path, config, None)
}

/// Same as [`compile_file`] but with an explicit backend.
///
/// This is the primary entry point for compiling a `.kry` file. It handles
/// module imports by resolving `use` declarations, parsing imported modules,
/// and merging their declarations into the main module before type checking.
pub fn compile_file_with_backend(
    path: &Path,
    config: &BuildConfig,
    backend: Option<&dyn Backend>,
) -> CompileResult {
    // 1. Read source
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return CompileResult::from_error(format!(
                "failed to read {}: {e}",
                path.display()
            ));
        }
    };

    compile_file_impl(&source, path, config, backend)
}

/// Internal: compile a file with full import resolution support.
///
/// Unlike `compile_source_impl` (which works on bare strings), this function
/// knows the file path and can resolve `use` declarations to sibling files.
fn compile_file_impl(
    source: &str,
    path: &Path,
    config: &BuildConfig,
    backend: Option<&dyn Backend>,
) -> CompileResult {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // 2. Source map registration
    let mut source_map = SourceMap::default();
    let file_name = path.to_string_lossy();
    let file_id = source_map.add_file(file_name.to_string(), source.to_string());

    // 3. Lex
    let tokens = Lexer::new(source, file_id).tokenize();

    if config.verbose {
        eprintln!(
            "[kryos] lexer: {} tokens from '{file_name}'",
            tokens.len()
        );
    }

    // 4. Parse
    let mut module = match parse(tokens) {
        Ok(module) => module,
        Err(parse_errors) => {
            return CompileResult {
                diagnostics: parse_errors,
                source_map,
                success: false,
                output_path: None,
                mir: None,
                object_bytes: None,
                llvm_ir: None,
            };
        }
    };

    // 5. Module import resolution — resolve `use` declarations and merge
    //    imported declarations into the main module's AST.
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut visited = HashSet::new();
    visited.insert(canonical);

    let mut imported_decls = Vec::new();
    if let Err(import_diags) = resolve::resolve_imports(
        &module,
        path,
        &mut visited,
        &mut imported_decls,
        config.verbose,
    ) {
        diagnostics.extend(import_diags);
        return CompileResult {
            diagnostics,
            source_map,
            success: false,
            output_path: None,
            mir: None,
            object_bytes: None,
            llvm_ir: None,
        };
    }

    if config.verbose && !imported_decls.is_empty() {
        eprintln!(
            "[kryos] imports: merged {} declarations from imported modules",
            imported_decls.len()
        );
    }

    // Prepend imported declarations so they are visible during type checking.
    // We insert them before the main module's declarations.
    let mut merged_decls = imported_decls;
    merged_decls.append(&mut module.declarations);
    module.declarations = merged_decls;

    // From here on, the pipeline is the same as compile_source_impl but uses
    // the merged module.
    compile_module_impl(module, diagnostics, source_map, config, backend)
}

/// Internal: run the analysis + codegen pipeline on an already-parsed module.
///
/// This is the shared tail of both `compile_source_impl` (no imports) and
/// `compile_file_impl` (with imports merged).
fn compile_module_impl(
    module: kryos_ast::Module,
    mut diagnostics: Vec<Diagnostic>,
    source_map: SourceMap,
    config: &BuildConfig,
    backend: Option<&dyn Backend>,
) -> CompileResult {
    // 5. Type check
    let type_diags = type_check(&module);
    let has_type_errors = type_diags.iter().any(|d| d.is_error());
    diagnostics.extend(type_diags);

    // 6. Ownership analysis
    let ownership = analyze_ownership(&module);
    let has_ownership_errors = ownership.errors.iter().any(|d| d.is_error());
    diagnostics.extend(ownership.errors);

    if config.verbose && !ownership.arc_insertions.is_empty() {
        eprintln!(
            "[kryos] ownership: {} ARC insertions",
            ownership.arc_insertions.len()
        );
    }

    // 7. Capability checking
    let cap_diags = check_capabilities(&module);
    let has_cap_errors = cap_diags.iter().any(|d| d.is_error());
    diagnostics.extend(cap_diags);

    // 8. Bail if any analysis pass produced errors
    if has_type_errors || has_ownership_errors || has_cap_errors {
        return CompileResult {
            diagnostics,
            source_map,
            success: false,
            output_path: None,
            mir: None,
            object_bytes: None,
            llvm_ir: None,
        };
    }

    // 9. MIR lowering
    let mut mir = kryos_mir::lower_module(&module);

    // 9b. Comptime evaluation: fold constant expressions in comptime blocks.
    kryos_mir::consteval::run_comptime_pass(&mut mir);

    if config.verbose {
        eprintln!(
            "[kryos] MIR: {} functions lowered",
            mir.functions.len()
        );
    }

    // 10. If only MIR dump is requested, we are done
    if config.output_type == OutputType::Mir {
        return CompileResult {
            diagnostics,
            source_map,
            success: true,
            output_path: None,
            mir: Some(mir),
            object_bytes: None,
            llvm_ir: None,
        };
    }

    // 11. Codegen (requires a backend)
    codegen_and_link(mir, diagnostics, source_map, config, backend)
}

/// Compile source code provided as a string.
///
/// This is the core pipeline entry point. `file_name` is used for
/// diagnostic messages (it does not need to correspond to a real file).
pub fn compile_source(source: &str, file_name: &str, config: &BuildConfig) -> CompileResult {
    compile_source_impl(source, file_name, config, None)
}

/// Internal: the full compilation pipeline for string-based compilation (no imports).
fn compile_source_impl(
    source: &str,
    file_name: &str,
    config: &BuildConfig,
    backend: Option<&dyn Backend>,
) -> CompileResult {
    let diagnostics: Vec<Diagnostic> = Vec::new();

    // 2. Source map registration
    let mut source_map = SourceMap::default();
    let file_id = source_map.add_file(file_name.to_string(), source.to_string());

    // 3. Lex
    let tokens = Lexer::new(source, file_id).tokenize();

    if config.verbose {
        eprintln!(
            "[kryos] lexer: {} tokens from '{file_name}'",
            tokens.len()
        );
    }

    // 4. Parse
    let module = match parse(tokens) {
        Ok(module) => module,
        Err(parse_errors) => {
            return CompileResult {
                diagnostics: parse_errors,
                source_map,
                success: false,
                output_path: None,
                mir: None,
                object_bytes: None,
                llvm_ir: None,
            };
        }
    };

    // Note: compile_source does not support `use` imports (no file path context).
    // For file-based compilation with import support, use compile_file_with_backend.
    compile_module_impl(module, diagnostics, source_map, config, backend)
}

/// Codegen + linking stage, extracted for reuse.
fn codegen_and_link(
    mir: MirModule,
    mut diagnostics: Vec<Diagnostic>,
    source_map: SourceMap,
    config: &BuildConfig,
    backend: Option<&dyn Backend>,
) -> CompileResult {
    match config.output_type {
        OutputType::LlvmIr => {
            if let Some(be) = backend {
                match be.emit_ir(&mir) {
                    Ok(ir) => CompileResult {
                        diagnostics,
                        source_map,
                        success: true,
                        output_path: None,
                        mir: Some(mir),
                        object_bytes: None,
                        llvm_ir: Some(ir),
                    },
                    Err(e) => {
                        diagnostics.push(Diagnostic::error(format!(
                            "codegen ({}) failed: {}",
                            be.name(),
                            e.message
                        )));
                        CompileResult {
                            diagnostics,
                            source_map,
                            success: false,
                            output_path: None,
                            mir: Some(mir),
                            object_bytes: None,
                            llvm_ir: None,
                        }
                    }
                }
            } else {
                diagnostics.push(Diagnostic::warning(
                    "no codegen backend available; stopping after MIR",
                ));
                CompileResult {
                    diagnostics,
                    source_map,
                    success: true,
                    output_path: None,
                    mir: Some(mir),
                    object_bytes: None,
                    llvm_ir: None,
                }
            }
        }
        OutputType::Binary | OutputType::Library | OutputType::Object => {
            if let Some(be) = backend {
                match be.compile(&mir) {
                    Ok(bytes) => {
                        let out_path = config.derive_output_path();

                        if config.output_type == OutputType::Object {
                            if let Err(e) = fs::write(&out_path, &bytes) {
                                diagnostics.push(Diagnostic::error(format!(
                                    "failed to write object file '{}': {e}",
                                    out_path.display()
                                )));
                                return CompileResult {
                                    diagnostics,
                                    source_map,
                                    success: false,
                                    output_path: None,
                                    mir: Some(mir),
                                    object_bytes: Some(bytes),
                                    llvm_ir: None,
                                };
                            }

                            CompileResult {
                                diagnostics,
                                source_map,
                                success: true,
                                output_path: Some(out_path.to_string_lossy().to_string()),
                                mir: Some(mir),
                                object_bytes: Some(bytes),
                                llvm_ir: None,
                            }
                        } else {
                            let stem = out_path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("out");
                            let obj_path = out_path
                                .parent()
                                .unwrap_or_else(|| Path::new("."))
                                .join(format!("{stem}.o"));

                            if let Err(e) = fs::write(&obj_path, &bytes) {
                                diagnostics.push(Diagnostic::error(format!(
                                    "failed to write temp object file '{}': {e}",
                                    obj_path.display()
                                )));
                                return CompileResult {
                                    diagnostics,
                                    source_map,
                                    success: false,
                                    output_path: None,
                                    mir: Some(mir),
                                    object_bytes: Some(bytes),
                                    llvm_ir: None,
                                };
                            }

                            let target = if let Some(ref triple) = config.target {
                                match kryos_linker::Target::from_triple(triple) {
                                    Ok(t) => t,
                                    Err(e) => {
                                        let _ = fs::remove_file(&obj_path);
                                        diagnostics.push(Diagnostic::error(format!(
                                            "invalid target triple: {e}"
                                        )));
                                        return CompileResult {
                                            diagnostics,
                                            source_map,
                                            success: false,
                                            output_path: None,
                                            mir: Some(mir),
                                            object_bytes: Some(bytes),
                                            llvm_ir: None,
                                        };
                                    }
                                }
                            } else {
                                kryos_linker::Target::host()
                            };

                            let rt_lib = crate::runtime::find_runtime_lib();
                            let stdlib_native_lib = crate::runtime::find_stdlib_native_lib();
                            if config.verbose {
                                match &rt_lib {
                                    Some(p) => eprintln!("[kryos] runtime lib: {}", p.display()),
                                    None => eprintln!("[kryos] runtime lib: not found (runtime symbols will be unresolved)"),
                                }
                                match &stdlib_native_lib {
                                    Some(p) => eprintln!("[kryos] stdlib-native lib: {}", p.display()),
                                    None => eprintln!("[kryos] stdlib-native lib: not found"),
                                }
                            }

                            // System libraries required by the Rust-based runtime staticlib.
                            let extra_libs = crate::runtime::system_libs(&target);

                            let linker_config = kryos_linker::LinkerConfig {
                                target,
                                object_files: vec![obj_path.clone()],
                                runtime_lib: rt_lib,
                                stdlib_native: stdlib_native_lib,
                                output: out_path.clone(),
                                link_type: if config.output_type == OutputType::Library {
                                    kryos_linker::LinkType::SharedLib
                                } else {
                                    kryos_linker::LinkType::Dynamic
                                },
                                extra_libs,
                                extra_lib_dirs: vec![],
                            };

                            if let Err(e) = kryos_linker::link(&linker_config) {
                                let _ = fs::remove_file(&obj_path);
                                diagnostics.push(Diagnostic::error(format!(
                                    "linking failed: {e}"
                                )));
                                return CompileResult {
                                    diagnostics,
                                    source_map,
                                    success: false,
                                    output_path: None,
                                    mir: Some(mir),
                                    object_bytes: Some(bytes),
                                    llvm_ir: None,
                                };
                            }

                            let _ = fs::remove_file(&obj_path);

                            CompileResult {
                                diagnostics,
                                source_map,
                                success: true,
                                output_path: Some(out_path.to_string_lossy().to_string()),
                                mir: Some(mir),
                                object_bytes: Some(bytes),
                                llvm_ir: None,
                            }
                        }
                    }
                    Err(e) => {
                        diagnostics.push(Diagnostic::error(format!(
                            "codegen ({}) failed: {}",
                            be.name(),
                            e.message
                        )));
                        CompileResult {
                            diagnostics,
                            source_map,
                            success: false,
                            output_path: None,
                            mir: Some(mir),
                            object_bytes: None,
                            llvm_ir: None,
                        }
                    }
                }
            } else {
                diagnostics.push(Diagnostic::warning(
                    "no codegen backend available; stopping after MIR",
                ));
                CompileResult {
                    diagnostics,
                    source_map,
                    success: true,
                    output_path: None,
                    mir: Some(mir),
                    object_bytes: None,
                    llvm_ir: None,
                }
            }
        }
        OutputType::Mir => unreachable!("handled above"),
    }
}

// ---------------------------------------------------------------------------
// compile_project
// ---------------------------------------------------------------------------

/// Compile a project directory by reading `kryos.toml` and compiling all
/// `.kry` source files under `src/`.
///
/// Aggregates diagnostics from every file and returns a combined result.
pub fn compile_project(dir: &Path, config: &BuildConfig) -> CompileResult {
    compile_project_with_backend(dir, config, None)
}

/// Same as [`compile_project`] but with an explicit backend.
pub fn compile_project_with_backend(
    dir: &Path,
    config: &BuildConfig,
    backend: Option<&dyn Backend>,
) -> CompileResult {
    let manifest_path = dir.join("kryos.toml");
    let manifest = match kryos_package::Manifest::from_file(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            return CompileResult::from_error(format!(
                "failed to load project manifest: {e}"
            ));
        }
    };

    if config.verbose {
        eprintln!(
            "[kryos] project: {} v{}",
            manifest.package.name, manifest.package.version
        );
    }

    let src_dir = dir.join("src");
    if !src_dir.is_dir() {
        return CompileResult::from_error(format!(
            "project has no src/ directory: {}",
            src_dir.display()
        ));
    }

    // Collect all .kry files
    let source_files = collect_kry_files(&src_dir);
    if source_files.is_empty() {
        return CompileResult::from_error(format!(
            "no .kry source files found in {}",
            src_dir.display()
        ));
    }

    if config.verbose {
        eprintln!(
            "[kryos] found {} source file(s)",
            source_files.len()
        );
    }

    // Compile each file, aggregating results
    let combined_source_map = SourceMap::default();
    let mut all_diagnostics: Vec<Diagnostic> = Vec::new();
    let mut last_mir: Option<MirModule> = None;
    let mut all_success = true;

    for file_path in &source_files {
        let result = compile_file_with_backend(file_path, config, backend);
        all_diagnostics.extend(result.diagnostics);
        // Note: each file gets its own source_map during compilation.
        // For the combined result we keep a fresh one (diagnostics already
        // carry their file_id from the per-file source map).
        if !result.success {
            all_success = false;
        }
        if result.mir.is_some() {
            last_mir = result.mir;
        }
    }

    CompileResult {
        diagnostics: all_diagnostics,
        source_map: combined_source_map,
        success: all_success,
        output_path: None,
        mir: last_mir,
        object_bytes: None,
        llvm_ir: None,
    }
}

// ---------------------------------------------------------------------------
// check_file / check_source
// ---------------------------------------------------------------------------

/// Check a file without producing any output — equivalent to `kryos check`.
///
/// Runs the full analysis pipeline (lex, parse, type check, ownership,
/// capabilities) but skips MIR lowering and codegen.
/// Resolves `use` imports from sibling files.
///
/// Returns `(diagnostics, source_map)` so the caller can render errors.
pub fn check_file(path: &Path) -> (Vec<Diagnostic>, SourceMap) {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return (
                vec![Diagnostic::error(format!(
                    "failed to read {}: {e}",
                    path.display()
                ))],
                SourceMap::default(),
            );
        }
    };

    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // Source map
    let mut source_map = SourceMap::default();
    let file_id = source_map.add_file(
        path.to_string_lossy().to_string(),
        source.to_string(),
    );

    // Lex
    let tokens = Lexer::new(&source, file_id).tokenize();

    // Parse
    let mut module = match parse(tokens) {
        Ok(module) => module,
        Err(parse_errors) => return (parse_errors, source_map),
    };

    // Resolve imports
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut visited = HashSet::new();
    visited.insert(canonical);

    let mut imported_decls = Vec::new();
    if let Err(import_diags) = resolve::resolve_imports(
        &module,
        path,
        &mut visited,
        &mut imported_decls,
        false,
    ) {
        diagnostics.extend(import_diags);
        return (diagnostics, source_map);
    }

    // Merge imported declarations before the main module's declarations.
    let mut merged_decls = imported_decls;
    merged_decls.append(&mut module.declarations);
    module.declarations = merged_decls;

    // Type check
    diagnostics.extend(type_check(&module));

    // Ownership
    let ownership = analyze_ownership(&module);
    diagnostics.extend(ownership.errors);

    // Capabilities
    diagnostics.extend(check_capabilities(&module));

    (diagnostics, source_map)
}

/// Check source code provided as a string, without producing output.
///
/// Returns `(diagnostics, source_map)`.
pub fn check_source(source: &str, file_name: &str) -> (Vec<Diagnostic>, SourceMap) {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // Source map
    let mut source_map = SourceMap::default();
    let file_id = source_map.add_file(file_name.to_string(), source.to_string());

    // Lex
    let tokens = Lexer::new(source, file_id).tokenize();

    // Parse
    let module = match parse(tokens) {
        Ok(module) => module,
        Err(parse_errors) => return (parse_errors, source_map),
    };

    // Type check
    diagnostics.extend(type_check(&module));

    // Ownership
    let ownership = analyze_ownership(&module);
    diagnostics.extend(ownership.errors);

    // Capabilities
    diagnostics.extend(check_capabilities(&module));

    (diagnostics, source_map)
}

/// Check a project directory without producing output.
pub fn check_project(dir: &Path) -> (Vec<Diagnostic>, SourceMap) {
    let manifest_path = dir.join("kryos.toml");
    if let Err(e) = kryos_package::Manifest::from_file(&manifest_path) {
        return (
            vec![Diagnostic::error(format!(
                "failed to load project manifest: {e}"
            ))],
            SourceMap::default(),
        );
    }

    let src_dir = dir.join("src");
    if !src_dir.is_dir() {
        return (
            vec![Diagnostic::error(format!(
                "project has no src/ directory: {}",
                src_dir.display()
            ))],
            SourceMap::default(),
        );
    }

    let source_files = collect_kry_files(&src_dir);
    if source_files.is_empty() {
        return (
            vec![Diagnostic::error(format!(
                "no .kry source files found in {}",
                src_dir.display()
            ))],
            SourceMap::default(),
        );
    }

    let mut all_diagnostics = Vec::new();
    let combined_source_map = SourceMap::default();

    for file_path in &source_files {
        let (diags, _sm) = check_file(file_path);
        all_diagnostics.extend(diags);
    }

    (all_diagnostics, combined_source_map)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Render all diagnostics in a `CompileResult` to a human-readable string.
pub fn render_diagnostics(result: &CompileResult) -> String {
    let mut out = String::new();
    for diag in &result.diagnostics {
        out.push_str(&kryos_errors::render_diagnostic(diag, &result.source_map));
    }
    out
}

/// Recursively collect all `.kry` files under a directory.
fn collect_kry_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_kry_files(&path));
            } else if path.extension().and_then(|e| e.to_str()) == Some("kry") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}
