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

use crate::build_cache::{self, CacheContext, CacheKey};
use crate::config::{BuildConfig, BuildMode, OutputType};
use crate::resolve;

// ---------------------------------------------------------------------------
// Build-cache helpers (private)
// ---------------------------------------------------------------------------

/// Returns true if this output type produces a single artifact whose final
/// byte contents can be safely cached and restored by copying. Anything else
/// (MIR dump, LLVM IR text) is too small or text-shaped to be worth caching
/// and is intentionally skipped.
fn cacheable_output(t: OutputType) -> bool {
    matches!(t, OutputType::Binary | OutputType::Object | OutputType::Library)
}

fn build_mode_str(m: BuildMode) -> &'static str {
    match m {
        BuildMode::Debug => "debug",
        BuildMode::Release => "release",
    }
}

fn output_type_str(t: OutputType) -> &'static str {
    match t {
        OutputType::Binary => "binary",
        OutputType::Library => "library",
        OutputType::Object => "object",
        OutputType::LlvmIr => "llvm_ir",
        OutputType::Mir => "mir",
    }
}

/// Render the effective target string for caching. We deliberately do *not*
/// call `BuildConfig::effective_target` here because that pulls in
/// `kryos_linker` work the cache doesn't need; a plain host fallback is
/// sufficient and matches what the rest of the driver does when `target` is
/// `None`.
fn cache_target_str(config: &BuildConfig) -> String {
    if let Some(t) = &config.target {
        return t.clone();
    }
    format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)
}

/// Attempt to short-circuit compilation via the on-disk build cache. Returns
/// `Some(result)` only when the cache is enabled, the output type is
/// cacheable, and a matching artifact exists on disk. Any I/O or permission
/// problem silently returns `None`, falling back to a normal build.
fn try_cache_hit(
    source: &str,
    config: &BuildConfig,
) -> Option<(CacheKey, CompileResult)> {
    if !config.use_cache || !cacheable_output(config.output_type) {
        return None;
    }
    let target = cache_target_str(config);
    let ctx = CacheContext {
        target_triple: &target,
        build_mode: build_mode_str(config.mode),
        output_type: output_type_str(config.output_type),
        compiler_version: env!("CARGO_PKG_VERSION"),
    };
    let key = CacheKey::new(source, &ctx);
    let bytes = build_cache::lookup(&key)?;

    let out_path = config.derive_output_path();
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    if fs::write(&out_path, &bytes).is_err() {
        return None;
    }

    // Restore the executable bit on Unix so cached binaries actually run.
    #[cfg(unix)]
    if config.output_type == OutputType::Binary {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&out_path) {
            let mut perms = meta.permissions();
            perms.set_mode(perms.mode() | 0o111);
            let _ = fs::set_permissions(&out_path, perms);
        }
    }

    if config.verbose {
        eprintln!(
            "[kryos] cache: hit {} -> {}",
            key.as_filename(),
            out_path.display()
        );
    }

    Some((
        key,
        CompileResult {
            diagnostics: Vec::new(),
            source_map: SourceMap::default(),
            success: true,
            output_path: Some(out_path.to_string_lossy().to_string()),
            mir: None,
            object_bytes: None,
            llvm_ir: None,
        },
    ))
}

/// Best-effort: if the cache is enabled and the just-built artifact path is
/// readable, capture its bytes into the cache for future runs.
fn try_cache_store(source: &str, config: &BuildConfig, result: &CompileResult) {
    if !config.use_cache || !cacheable_output(config.output_type) || !result.success {
        return;
    }
    let Some(ref out) = result.output_path else {
        return;
    };
    let Ok(bytes) = fs::read(out) else {
        return;
    };
    let target = cache_target_str(config);
    let ctx = CacheContext {
        target_triple: &target,
        build_mode: build_mode_str(config.mode),
        output_type: output_type_str(config.output_type),
        compiler_version: env!("CARGO_PKG_VERSION"),
    };
    let key = CacheKey::new(source, &ctx);
    build_cache::store(&key, &bytes, &ctx);
    if config.verbose {
        eprintln!("[kryos] cache: stored {} ({} bytes)", key.as_filename(), bytes.len());
    }
}

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

    /// Whether this backend supports incremental per-function emission.
    /// When true, the pipeline will use `begin_module` / `emit_function_inc` /
    /// `finish_module` instead of `compile`, draining the MirFunction Vec one
    /// entry at a time to reduce peak memory.
    fn supports_incremental(&self) -> bool {
        false
    }

    /// Begin an incremental compilation session. The backend should prescan
    /// all functions for cross-function metadata (signatures, string constants,
    /// ARC usage) and write the module preamble to its output sink.
    ///
    /// `functions` is a shared slice used only for prescan — the caller will
    /// drain the Vec and pass each function to `emit_function_inc` afterwards.
    /// Backends use interior mutability (`RefCell`) to store session state.
    fn begin_module(
        &self,
        _header: &kryos_mir::ir::MirModuleHeader,
        _functions: &[kryos_mir::ir::MirFunction],
    ) -> Result<(), BackendError> {
        Err(BackendError::unsupported("incremental"))
    }

    /// Emit a single function. Takes ownership so the MIR for that function
    /// is freed immediately after emission. Backends use interior mutability.
    fn emit_function_inc(&self, _func: kryos_mir::ir::MirFunction) -> Result<(), BackendError> {
        Err(BackendError::unsupported("incremental"))
    }

    /// Finish the incremental session and return compiled object code bytes.
    /// Backends use interior mutability to finalize and clear session state.
    fn finish_module(&self) -> Result<Vec<u8>, BackendError> {
        Err(BackendError::unsupported("incremental"))
    }
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
            return CompileResult::from_error(format!("failed to read {}: {e}", path.display()));
        }
    };

    // 1a. Build-cache fast path. If the user opted in via `use_cache` and an
    //     identical (source, target, mode, output_type, compiler_version)
    //     entry is on disk, we restore the artifact directly and skip the
    //     entire pipeline. Misses fall through silently to a fresh build.
    if let Some((_key, hit)) = try_cache_hit(&source, config) {
        return hit;
    }

    let result = compile_file_impl(&source, path, config, backend);

    // 1b. On a successful build, capture the produced artifact for the next
    //     time. Errors here are non-fatal — the cache is purely an
    //     optimization.
    try_cache_store(&source, config, &result);

    result
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
    let (tokens, lex_diags) = Lexer::new(source, file_id).tokenize_with_diagnostics();
    diagnostics.extend(lex_diags);

    if config.verbose {
        eprintln!("[kryos] lexer: {} tokens from '{file_name}'", tokens.len());
    }

    // If the lexer emitted hard errors (e.g. unterminated string), stop
    // before the parser produces a confusing cascade.
    if diagnostics.iter().any(|d| d.is_error()) {
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

    // 4. Parse
    let mut module = match parse(tokens) {
        Ok(module) => module,
        Err(parse_errors) => {
            diagnostics.extend(parse_errors);
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
    // 4b. Expand @derive(...) annotations into concrete decls (e.g. add @copy
    //     for derive(Copy), synthesise inherent Debug methods, etc.) before
    //     type-checking so the rest of the pipeline sees a fully-elaborated
    //     module.
    let mut module = module;
    kryos_ast::expand_derives(&mut module);

    // 4c. Strip @cfg(...)-gated declarations that don't apply to the
    //     active target / build mode. Has to run before type-check so
    //     stripped functions don't generate stray "missing main" or
    //     "unused type" diagnostics.
    let cfg_ctx = kryos_ast::CfgContext::from_triple(
        config.target.as_deref(),
        matches!(config.mode, BuildMode::Release),
    );
    kryos_ast::strip_cfg(&mut module, &cfg_ctx);

    // 5. Type check (also collect resolved types for un-annotated lambda params
    //    so MIR can type closure params instead of defaulting them to i64).
    let (type_diags, lambda_param_types, let_types) =
        kryos_types::type_check_with_lambda_params(&module);
    let has_type_errors = type_diags.iter().any(|d| d.is_error());
    diagnostics.extend(type_diags);

    // 6. Ownership analysis (can be skipped for self-host bootstrap)
    let has_ownership_errors = if config.skip_ownership {
        if config.verbose {
            eprintln!("[kryos] ownership analysis skipped (--skip-ownership)");
        }
        false
    } else {
        let ownership = analyze_ownership(&module);
        let has_errors = ownership.errors.iter().any(|d| d.is_error());
        diagnostics.extend(ownership.errors);

        if config.verbose && !ownership.arc_insertions.is_empty() {
            eprintln!(
                "[kryos] ownership: {} ARC insertions",
                ownership.arc_insertions.len()
            );
        }
        has_errors
    };

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
    let mut mir =
        kryos_mir::lower_module_with_lambda_params(&module, &lambda_param_types, &let_types);

    // 9a. Populate source_file / source_line on each MIR function from AST spans.
    //     This is what drives accurate panic traces ("file.kry:42" instead of
    //     "<unknown>:0") at runtime.
    {
        // Look up the file from the source map. We assume file_id 0 (the
        // primary file) for single-file compilation.
        let (file_name_str, primary_file_id): (String, u32) = source_map
            .get_file(0)
            .map(|f| (f.name.clone(), 0u32))
            .unwrap_or_else(|| (String::new(), 0));
        // Build name -> start-offset map from top-level AST decls.
        let mut span_map: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();
        fn collect_decl_spans(
            decls: &[kryos_ast::decl::Decl],
            out: &mut std::collections::HashMap<String, u32>,
        ) {
            for decl in decls {
                match decl {
                    kryos_ast::decl::Decl::Function { name, span, .. } => {
                        out.entry(name.clone()).or_insert(span.start);
                    }
                    kryos_ast::decl::Decl::Impl {
                        target, methods, ..
                    } => {
                        for m in methods {
                            if let kryos_ast::decl::Decl::Function { name, span, .. } = m {
                                // Mirror name mangling used by lowering: `Target::method`.
                                let mangled = format!("{}::{}", target, name);
                                out.entry(mangled).or_insert(span.start);
                                out.entry(name.clone()).or_insert(span.start);
                            }
                        }
                    }
                    kryos_ast::decl::Decl::Trait { methods, .. } => {
                        collect_decl_spans(methods, out);
                    }
                    kryos_ast::decl::Decl::Extern { items, .. } => {
                        collect_decl_spans(items, out);
                    }
                    _ => {}
                }
            }
        }
        collect_decl_spans(&module.declarations, &mut span_map);

        for f in &mut mir.functions {
            f.source_file = Some(file_name_str.clone());
            // Try exact name first, then strip generic mangling suffix (`<T>` etc).
            let line = span_map
                .get(&f.name)
                .or_else(|| {
                    f.name
                        .find('<')
                        .and_then(|p| span_map.get(&f.name[..p]))
                })
                .or_else(|| {
                    // Try stripping `_kryos_<n>` mono suffix (generic instantiation).
                    f.name.rfind("__").and_then(|p| span_map.get(&f.name[..p]))
                })
                .map(|&offset| source_map.offset_to_line_col(primary_file_id, offset).0)
                .unwrap_or(0);
            f.source_line = line;
        }
    }

    drop(module); // AST no longer needed — free 0.5-1GB before codegen

    // 9b. Comptime evaluation: fold constant expressions in comptime blocks.
    kryos_mir::consteval::run_comptime_pass(&mut mir);

    // 9b'. Async lowering analysis: compute AsyncPlan for every `async fn`
    //      and stamp synthesised state-structs into the module. This is the
    //      non-destructive prelude to codegen-time poll-fn synthesis
    //      (kryos_mir::async_lower).
    {
        let report = kryos_mir::async_lower::run(&mir);
        if !report.errors.is_empty() {
            for e in &report.errors {
                diagnostics.push(Diagnostic::error(format!(
                    "async lowering error in `{}`: {}",
                    e.function, e.message
                )));
            }
            return CompileResult {
                diagnostics,
                source_map,
                success: false,
                output_path: None,
                mir: Some(mir),
                object_bytes: None,
                llvm_ir: None,
            };
        }
        let inserted = kryos_mir::async_lower::apply_state_structs(&mut mir, &report);
        kryos_mir::async_lower::stamp_attributes(&mut mir);
        let splits_applied = if config.split_async_awaits {
            kryos_mir::async_lower::apply_split_at_awaits(&mut mir, &report)
        } else {
            0
        };
        if config.verbose {
            eprintln!(
                "[kryos] async lowering: {} plan(s), {} state-struct(s) synthesised, {} fn(s) split at await",
                report.plans.len(),
                inserted,
                splits_applied
            );
        }
    }

    // 9c. Optimization passes (release builds): inlining, constant folding,
    //     dead code elimination, tail call optimization, strength reduction.
    if config.mode == crate::config::BuildMode::Release {
        kryos_mir::optimize::optimize(&mut mir);
    }

    if config.verbose {
        eprintln!("[kryos] MIR: {} functions lowered", mir.functions.len());
    }

    // 9d. Check for main() — binary targets require an entry point.
    if config.output_type == OutputType::Binary {
        let has_main = mir.functions.iter().any(|f| f.name == "main");
        if !has_main {
            diagnostics.push(Diagnostic::error(
                "no `main` function found — binary programs require a main() entry point",
            ));
            return CompileResult {
                diagnostics,
                source_map,
                success: false,
                output_path: None,
                mir: Some(mir),
                object_bytes: None,
                llvm_ir: None,
            };
        }
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
        eprintln!("[kryos] lexer: {} tokens from '{file_name}'", tokens.len());
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
                // Use per-function incremental path when the backend supports it.
                // Each MirFunction is freed immediately after its IR is written,
                // avoiding the peak memory spike of holding all functions + full IR simultaneously.
                let compile_result: Result<Vec<u8>, BackendError> = if be.supports_incremental() {
                    (|| -> Result<Vec<u8>, BackendError> {
                        let (header, functions) = mir.into_header_and_functions();
                        be.begin_module(&header, &functions)?;
                        drop(header);
                        for func in functions {
                            be.emit_function_inc(func)?;
                        }
                        be.finish_module()
                    })()
                } else {
                    be.compile(&mir)
                };
                match compile_result {
                    Ok(bytes) => {
                        let out_path = config.derive_output_path();

                        // WASM backends emit a self-contained `.wasm` module —
                        // no native linker, no runtime archive, no object file.
                        if be.name() == "wasm" {
                            let stem = out_path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("out");
                            let wasm_path = out_path
                                .parent()
                                .unwrap_or_else(|| Path::new("."))
                                .join(format!("{stem}.wasm"));
                            if let Err(e) = fs::write(&wasm_path, &bytes) {
                                diagnostics.push(Diagnostic::error(format!(
                                    "failed to write wasm file '{}': {e}",
                                    wasm_path.display()
                                )));
                                return CompileResult {
                                    diagnostics,
                                    source_map,
                                    success: false,
                                    output_path: None,
                                    mir: None,
                                    object_bytes: Some(bytes),
                                    llvm_ir: None,
                                };
                            }
                            return CompileResult {
                                diagnostics,
                                source_map,
                                success: true,
                                output_path: Some(wasm_path.to_string_lossy().to_string()),
                                mir: None,
                                object_bytes: Some(bytes),
                                llvm_ir: None,
                            };
                        }

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
                                    mir: None,
                                    object_bytes: Some(bytes),
                                    llvm_ir: None,
                                };
                            }

                            CompileResult {
                                diagnostics,
                                source_map,
                                success: true,
                                output_path: Some(out_path.to_string_lossy().to_string()),
                                mir: None,
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
                                    mir: None,
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
                                            mir: None,
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
                                    Some(p) => {
                                        eprintln!("[kryos] stdlib-native lib: {}", p.display())
                                    }
                                    None => eprintln!("[kryos] stdlib-native lib: not found"),
                                }
                            }

                            // System libraries required by the Rust-based runtime staticlib.
                            let (extra_libs, extra_frameworks) = crate::runtime::system_libs(&target);

                            // The LLVM backend suppresses `-flto=thin` when
                            // targeting Windows/MSVC because link.exe can't
                            // consume bitcode-bearing objects. Keep the
                            // linker's view of `lto` in sync so the linker
                            // doesn't try to invoke clang-as-driver for an
                            // object that's actually native COFF.
                            let is_msvc = target.os == kryos_linker::Os::Windows
                                && target.env == kryos_linker::Env::Msvc;
                            let effective_lto = config.lto && !is_msvc;

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
                                extra_frameworks,
                                lto: effective_lto,
                                debug_info: config.debug_info,
                            };

                            if let Err(e) = kryos_linker::link(&linker_config) {
                                let _ = fs::remove_file(&obj_path);
                                diagnostics.push(Diagnostic::error(format!("linking failed: {e}")));
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

                            let _ = fs::remove_file(&obj_path);

                            // mir and bytes no longer needed: object written to disk and linked.
                            // Dropping them here avoids holding MIR + binary simultaneously.
                            CompileResult {
                                diagnostics,
                                source_map,
                                success: true,
                                output_path: Some(out_path.to_string_lossy().to_string()),
                                mir: None,
                                object_bytes: None,
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
                            mir: None,
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
            return CompileResult::from_error(format!("failed to load project manifest: {e}"));
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

    // Compile only the entry point (src/main.kry by convention).
    // compile_file_with_backend already resolves and inlines all `use` imports,
    // so compiling every file independently would produce spurious "no main" errors
    // for every non-entry module.
    let entry_path = src_dir.join("main.kry");

    if config.verbose {
        eprintln!("[kryos] compiling entry: {}", entry_path.display());
    }

    if !entry_path.is_file() {
        return CompileResult::from_error(format!(
            "entry file not found: {}",
            entry_path.display()
        ));
    }

    // Apply output name from manifest if not already overridden by the caller.
    if config.output.is_none() {
        if let Some(ref name) = manifest.build.output {
            let mut config2 = config.clone();
            let out_name = if cfg!(windows) && !name.ends_with(".exe") {
                format!("{name}.exe")
            } else {
                name.clone()
            };
            config2.output = Some(out_name);
            return compile_file_with_backend(&entry_path, &config2, backend);
        }
    }

    compile_file_with_backend(&entry_path, config, backend)
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
    let file_id = source_map.add_file(path.to_string_lossy().to_string(), source.to_string());

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
    if let Err(import_diags) =
        resolve::resolve_imports(&module, path, &mut visited, &mut imported_decls, false)
    {
        diagnostics.extend(import_diags);
        return (diagnostics, source_map);
    }

    // Merge imported declarations before the main module's declarations.
    let mut merged_decls = imported_decls;
    merged_decls.append(&mut module.declarations);
    module.declarations = merged_decls;

    // Expand @derive(...) annotations.
    kryos_ast::expand_derives(&mut module);

    // Strip @cfg(...)-gated decls for the host (check_file targets the
    // host in debug mode by default — no BuildConfig is available here).
    kryos_ast::strip_cfg(&mut module, &kryos_ast::CfgContext::for_host(false));

    // Type check
    diagnostics.extend(type_check(&module));

    // Ownership
    let ownership = analyze_ownership(&module);
    diagnostics.extend(ownership.errors);

    // Capabilities
    diagnostics.extend(check_capabilities(&module));

    (diagnostics, source_map)
}

/// Check a single file with options (e.g. skip_ownership for self-host bootstrap).
///
/// Returns `(diagnostics, source_map)`.
pub fn check_file_with_options(path: &Path, skip_ownership: bool) -> (Vec<Diagnostic>, SourceMap) {
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
    let file_id = source_map.add_file(path.to_string_lossy().to_string(), source.to_string());

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
    if let Err(import_diags) =
        resolve::resolve_imports(&module, path, &mut visited, &mut imported_decls, false)
    {
        diagnostics.extend(import_diags);
        return (diagnostics, source_map);
    }

    // Merge imported declarations before the main module's declarations.
    let mut merged_decls = imported_decls;
    merged_decls.append(&mut module.declarations);
    module.declarations = merged_decls;

    // Expand @derive(...) annotations.
    kryos_ast::expand_derives(&mut module);

    // Strip @cfg(...)-gated decls for the host (debug mode).
    kryos_ast::strip_cfg(&mut module, &kryos_ast::CfgContext::for_host(false));

    // Type check
    diagnostics.extend(type_check(&module));

    // Ownership (skip for self-host bootstrap)
    if !skip_ownership {
        let ownership = analyze_ownership(&module);
        diagnostics.extend(ownership.errors);
    }

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
    let mut module = match parse(tokens) {
        Ok(module) => module,
        Err(parse_errors) => return (parse_errors, source_map),
    };

    // Expand @derive(...) annotations.
    kryos_ast::expand_derives(&mut module);

    // Strip @cfg(...)-gated decls for the host (debug mode).
    kryos_ast::strip_cfg(&mut module, &kryos_ast::CfgContext::for_host(false));

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
