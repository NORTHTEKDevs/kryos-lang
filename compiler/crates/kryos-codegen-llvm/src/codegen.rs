//! LLVM IR text emitter.
//!
//! Translates MIR basic blocks, instructions, and terminators into valid
//! LLVM IR text. The output can be compiled by `llc` or `clang`.

use std::collections::{HashMap, HashSet};

use kryos_mir::ir::{
    BasicBlock, Constant, EnumVariantDef, Instruction, LocalId, MirBinOp, MirFunction, MirModule,
    MirModuleHeader, MirType, MirUnOp, Operand, RValue, Terminator,
};

use crate::{CodegenError, EmitOptions};

// ---------------------------------------------------------------------------
// Codegen state
// ---------------------------------------------------------------------------

/// LLVM IR text emitter.
pub struct LlvmCodegen {
    /// Accumulated LLVM IR output.
    output: String,
    /// Module-level string constants: content -> global name.
    string_constants: HashMap<String, String>,
    /// Counter for generating unique temporaries (`%t0`, `%t1`, ...).
    temp_counter: u32,
    /// Counter for generating unique string global names.
    string_counter: u32,
    /// Emission options (target triple, opt level, etc.).
    options: EmitOptions,
    /// Tracks whether any ARC operations are used — so we emit runtime decls.
    needs_arc_runtime: bool,
    /// Local type map for the current function (LocalId -> LLVM type string).
    local_types: HashMap<u32, String>,
    /// Function signatures: name -> list of LLVM parameter type strings.
    func_param_types: HashMap<String, Vec<String>>,
    /// Function return types: name -> LLVM return type string.
    func_ret_types: HashMap<String, String>,
    /// Aggregate-passing info: name -> (ret_agg_ty, per-param agg_ty).
    /// When ret_agg is Some, the function uses sret (returns void, takes ptr sret first).
    /// When a param entry is Some, that param is passed via ptr byval.
    func_sig_aggs: HashMap<String, (Option<String>, Vec<Option<String>>)>,
    /// Struct definitions from the MIR module (for field access resolution).
    struct_defs: HashMap<String, Vec<(String, MirType)>>,
    /// Enum definitions from the MIR module (for enum codegen).
    enum_defs: HashMap<String, Vec<EnumVariantDef>>,
    /// Set of local IDs that need alloca/store/load (mutable or multi-assigned).
    mutable_locals: HashSet<u32>,
    /// Tracks the actual LLVM type of each SSA temp (%tN -> type string).
    /// Used to know the real type of a value when coercing between ptr/i64/double.
    value_types: HashMap<String, String>,
    /// Structs annotated with `@copy` — assignment deep-copies the struct.
    copy_structs: HashSet<String>,
    /// Closure capture types: func_name -> Vec of capture MIR types.
    /// Used to generate per-closure dropper functions that free heap captures.
    closure_cap_types: HashMap<String, Vec<Option<MirType>>>,
    /// Closure call signatures: func_name -> (user_param_count, ret_ty_llvm).
    /// Used to emit `{name}_env` thunks and to dispatch CallIndirect via env.
    closure_user_sig: HashMap<String, (usize, String)>,
    /// Names of functions that have been emitted, in order — used when
    /// emitting DWARF debug metadata at module footer.
    emitted_function_names: Vec<String>,
}

impl LlvmCodegen {
    /// Create a new emitter with the given options.
    pub fn new(options: EmitOptions) -> Self {
        Self {
            output: String::with_capacity(4096),
            string_constants: HashMap::new(),
            temp_counter: 0,
            string_counter: 0,
            options,
            needs_arc_runtime: false,
            local_types: HashMap::new(),
            func_param_types: HashMap::new(),
            func_ret_types: HashMap::new(),
            struct_defs: HashMap::new(),
            enum_defs: HashMap::new(),
            mutable_locals: HashSet::new(),
            value_types: HashMap::new(),
            copy_structs: HashSet::new(),
            closure_cap_types: HashMap::new(),
            closure_user_sig: HashMap::new(),
            func_sig_aggs: HashMap::new(),
            emitted_function_names: Vec::new(),
        }
    }

    /// If the MIR type is an aggregate (Struct or Tuple), return its LLVM type
    /// string. These types must be passed via byval/sret instead of by value
    /// to satisfy LLVM ABI rules (especially for named struct types).
    fn aggregate_llvm_ty(&self, ty: &MirType) -> Option<String> {
        match ty {
            MirType::Struct(name) => Some(format!("%{name}")),
            MirType::Tuple(elems) => {
                let parts: Vec<String> = elems.iter().map(mir_type_to_llvm).collect();
                Some(format!("{{ {} }}", parts.join(", ")))
            }
            _ => None,
        }
    }

    // -----------------------------------------------------------------------
    // Public entry point
    // -----------------------------------------------------------------------

    /// Emit LLVM IR for an entire MIR module. Returns the IR text.
    pub fn emit_module(&mut self, module: &MirModule) -> Result<String, CodegenError> {
        // Reset state.
        self.output.clear();
        self.string_constants.clear();
        self.temp_counter = 0;
        self.string_counter = 0;
        self.needs_arc_runtime = false;
        self.func_param_types.clear();
        self.func_ret_types.clear();
        self.func_sig_aggs.clear();
        self.struct_defs = module.struct_defs.clone();
        self.enum_defs = module.enum_defs.clone();
        self.copy_structs = module.copy_structs.clone();

        // Pre-scan: collect string constants, detect ARC usage, record
        // function signatures, and collect closure capture types.
        self.closure_cap_types.clear();
        self.closure_user_sig.clear();
        for func in &module.functions {
            self.prescan_function(func);
            let param_types: Vec<String> = func
                .params
                .iter()
                .map(|p| self.sig_ty_to_llvm(&p.ty))
                .collect();
            self.func_param_types.insert(func.name.clone(), param_types);
            let ret_ty_str = self.sig_ty_to_llvm(&func.ret_ty);
            self.func_ret_types.insert(func.name.clone(), ret_ty_str);
            let ret_agg = self.aggregate_llvm_ty(&func.ret_ty);
            let param_aggs: Vec<Option<String>> = func
                .params
                .iter()
                .map(|p| self.aggregate_llvm_ty(&p.ty))
                .collect();
            self.func_sig_aggs
                .insert(func.name.clone(), (ret_agg, param_aggs));

            // Collect closure capture types for dropper generation.
            for bb in &func.blocks {
                for inst in &bb.instructions {
                    if let Instruction::Assign {
                        value:
                            RValue::Closure {
                                func_name,
                                captures,
                            },
                        ..
                    } = inst
                    {
                        if !self.closure_cap_types.contains_key(func_name.as_str()) {
                            let cap_types: Vec<Option<MirType>> = captures
                                .iter()
                                .map(|cap| match cap {
                                    Operand::Local(id) => func
                                        .locals
                                        .iter()
                                        .find(|l| l.id == *id)
                                        .map(|l| l.ty.clone()),
                                    _ => None,
                                })
                                .collect();
                            self.closure_cap_types.insert(func_name.clone(), cap_types);
                        }
                        // Record the underlying function's user-visible call
                        // shape so we can emit a `{name}_env` thunk later.
                        if !self.closure_user_sig.contains_key(func_name.as_str()) {
                            if let Some(mf) = module.functions.iter().find(|f| f.name == *func_name) {
                                let user_params =
                                    mf.params.len().saturating_sub(captures.len());
                                let ret_ty_llvm = self.sig_ty_to_llvm(&mf.ret_ty);
                                self.closure_user_sig.insert(
                                    func_name.clone(),
                                    (user_params, ret_ty_llvm),
                                );
                            }
                        }
                    }
                }
            }
        }

        // Module header.
        self.emit_header();

        // Named struct type declarations (must appear before any use).
        self.emit_struct_type_decls();

        // String constant globals.
        self.emit_string_globals();

        // ARC runtime declarations — always emit; some codegen paths use
        // ARC calls without going through ArcRetain/ArcRelease MIR.
        self.emit_arc_declarations();

        // External C function declarations used by builtins.
        self.emit_extern_declarations();

        // Functions.
        // Check if we need a main() wrapper: if MIR has a void-returning `main`,
        // rename it to `_kryos_main` and emit a C-compatible `main` wrapper.
        let has_void_main = module
            .functions
            .iter()
            .any(|f| f.name == "main" && f.ret_ty == MirType::Void);

        for func in &module.functions {
            if has_void_main && func.name == "main" {
                self.emit_function_as(func, "_kryos_main")?;
            } else {
                self.emit_function(func)?;
            }
        }

        // Emit dropper functions for closures with heap-typed captures.
        self.emit_closure_droppers();

        // Emit env-thunks so escaping closures can be invoked through a
        // uniform `(env, user_args...)` calling convention via CallIndirect.
        self.emit_closure_thunks();

        // Emit type drop helpers for struct/enum types with heap-owning fields.
        // These enable array element drop to recursively clean up nested fields.
        self.emit_type_drop_helpers();

        // Emit C-compatible main() wrapper if needed.
        if has_void_main {
            self.emit_main_wrapper();
        }

        Ok(self.output.clone())
    }

    // -----------------------------------------------------------------------
    // Incremental (per-function) emission helpers
    // -----------------------------------------------------------------------

    /// Take the current output buffer, leaving an empty String in its place.
    /// Used by the incremental path to drain the buffer after each section.
    pub fn take_output(&mut self) -> String {
        std::mem::take(&mut self.output)
    }

    /// Prescan all functions and initialize state from the module header.
    /// Must be called before `emit_header_section` or `emit_one_function_inc`.
    pub fn prescan_all(&mut self, header: &MirModuleHeader, functions: &[MirFunction]) {
        self.output.clear();
        self.string_constants.clear();
        self.temp_counter = 0;
        self.string_counter = 0;
        self.needs_arc_runtime = false;
        self.func_param_types.clear();
        self.func_ret_types.clear();
        self.struct_defs = header.struct_defs.clone();
        self.enum_defs = header.enum_defs.clone();
        self.copy_structs = header.copy_structs.clone();
        self.closure_cap_types.clear();
        self.closure_user_sig.clear();

        for func in functions {
            self.prescan_function(func);
            let param_types: Vec<String> = func
                .params
                .iter()
                .map(|p| self.sig_ty_to_llvm(&p.ty))
                .collect();
            self.func_param_types.insert(func.name.clone(), param_types);
            let ret_ty_str = self.sig_ty_to_llvm(&func.ret_ty);
            self.func_ret_types.insert(func.name.clone(), ret_ty_str);
            let ret_agg = self.aggregate_llvm_ty(&func.ret_ty);
            let param_aggs: Vec<Option<String>> = func
                .params
                .iter()
                .map(|p| self.aggregate_llvm_ty(&p.ty))
                .collect();
            self.func_sig_aggs
                .insert(func.name.clone(), (ret_agg, param_aggs));

            for bb in &func.blocks {
                for inst in &bb.instructions {
                    if let Instruction::Assign {
                        value:
                            RValue::Closure {
                                func_name,
                                captures,
                            },
                        ..
                    } = inst
                    {
                        if !self.closure_cap_types.contains_key(func_name.as_str()) {
                            let cap_types: Vec<Option<MirType>> = captures
                                .iter()
                                .map(|cap| match cap {
                                    Operand::Local(id) => func
                                        .locals
                                        .iter()
                                        .find(|l| l.id == *id)
                                        .map(|l| l.ty.clone()),
                                    _ => None,
                                })
                                .collect();
                            self.closure_cap_types.insert(func_name.clone(), cap_types);
                        }
                        if !self.closure_user_sig.contains_key(func_name.as_str()) {
                            if let Some(mf) = functions.iter().find(|f| f.name == *func_name) {
                                let user_params =
                                    mf.params.len().saturating_sub(captures.len());
                                let ret_ty_llvm = self.sig_ty_to_llvm(&mf.ret_ty);
                                self.closure_user_sig.insert(
                                    func_name.clone(),
                                    (user_params, ret_ty_llvm),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Emit the module-level preamble: target triple, data layout, string
    /// globals, ARC declarations, and extern C declarations. Call this once
    /// after `prescan_all`, before `emit_one_function_inc`.
    pub fn emit_header_section(&mut self) {
        self.emit_header();
        self.emit_struct_type_decls();
        self.emit_string_globals();
        // Always emit ARC declarations — some codegen paths (drop helpers,
        // aggregate dispatch) emit kryos_arc_release / kryos_arc_alloc_i64
        // calls without going through ArcRetain/ArcRelease MIR instructions,
        // so we cannot rely on `needs_arc_runtime` alone (was the root cause
        // of "undefined value '@kryos_arc_release'" linker errors).
        self.emit_arc_declarations();
        self.emit_extern_declarations();
    }

    /// Emit a single function in the incremental path. If `has_void_main` and
    /// the function is named `main`, it is emitted as `_kryos_main` instead.
    pub fn emit_one_function_inc(
        &mut self,
        func: &MirFunction,
        has_void_main: bool,
    ) -> Result<(), CodegenError> {
        if has_void_main && func.name == "main" {
            self.emit_function_as(func, "_kryos_main")
        } else {
            self.emit_function(func)
        }
    }

    /// Emit the module footer: closure droppers, type drop helpers, and the
    /// C-compatible `main` wrapper if `has_void_main`. Call once after all
    /// functions have been emitted via `emit_one_function_inc`.
    pub fn emit_footer_section(&mut self, has_void_main: bool) {
        self.emit_closure_droppers();
        self.emit_closure_thunks();
        self.emit_type_drop_helpers();
        if has_void_main {
            self.emit_main_wrapper();
        }
        if self.options.debug_info && self.options.source_file_path.is_some() {
            self.emit_dwarf_metadata();
        }
    }

    /// Emit a minimal `!llvm.dbg.cu` compile unit and `!DIFile` pointing
    /// at the user's `.kry` source. This is the lightweight tier of
    /// DWARF support: addr2line resolves the source filename for backtraces,
    /// but per-function DISubprograms and per-instruction !dbg locations
    /// require MIR-level source span plumbing (tracked for v2.3).
    fn emit_dwarf_metadata(&mut self) {
        let source_path = match self.options.source_file_path.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        // Split into directory and filename for DIFile.
        let path = std::path::Path::new(&source_path);
        let dir = path
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or(".")
            .replace('\\', "/");
        let file = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("<unknown>.kry");

        self.emit_blank();
        self.emit_line("; --- DWARF debug info ---");
        self.emit_line("!llvm.dbg.cu = !{!0}");
        self.emit_line("!llvm.module.flags = !{!2, !3}");
        self.emit_blank();

        // !0 = compile unit
        self.emit_line(&format!(
            "!0 = distinct !DICompileUnit(language: DW_LANG_C99, file: !1, producer: \"kryos {}\", isOptimized: {}, runtimeVersion: 0, emissionKind: FullDebug)",
            env!("CARGO_PKG_VERSION"),
            !matches!(self.options.opt_level, crate::OptLevel::O0),
        ));
        // !1 = file
        self.emit_line(&format!(
            "!1 = !DIFile(filename: \"{}\", directory: \"{}\")",
            file, dir,
        ));
        // !2, !3 = required module flags
        self.emit_line("!2 = !{i32 7, !\"Dwarf Version\", i32 4}");
        self.emit_line("!3 = !{i32 2, !\"Debug Info Version\", i32 3}");

        // !4 = empty subroutine type (void()). LineTablesOnly mode does
        // not require parameter types, so we use a single null-typed
        // signature shared across all DISubprograms.
        self.emit_line("!4 = !DISubroutineType(types: !5)");
        self.emit_line("!5 = !{null}");

        // DISubprograms intentionally omitted — see emit_function_as note.
    }

    // -----------------------------------------------------------------------
    // Module header
    // -----------------------------------------------------------------------

    fn emit_struct_type_decls(&mut self) {
        if self.struct_defs.is_empty() {
            return;
        }
        let mut decls: Vec<(String, String)> = self
            .struct_defs
            .iter()
            .filter(|(n, _)| n.as_str() != "Map")
            .map(|(n, fields)| {
                let parts: Vec<String> =
                    fields.iter().map(|(_, ty)| mir_type_to_llvm(ty)).collect();
                let body = if parts.is_empty() {
                    "{ i8 }".to_string()
                } else {
                    format!("{{ {} }}", parts.join(", "))
                };
                (n.clone(), body)
            })
            .collect();
        decls.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, body) in decls {
            self.emit_line(&format!("%{name} = type {body}"));
        }
        self.emit_blank();
    }

    fn emit_header(&mut self) {
        self.emit_line("; ModuleID = 'kryos_module'");
        self.emit_line("source_filename = \"kryos_module\"");

        if let Some(ref triple) = self.options.target_triple {
            self.emit_line(&format!("target triple = \"{triple}\""));
        }
        if let Some(ref layout) = self.options.target_datalayout {
            self.emit_line(&format!("target datalayout = \"{layout}\""));
        }

        self.emit_blank();
    }

    // -----------------------------------------------------------------------
    // String constant globals
    // -----------------------------------------------------------------------

    fn emit_string_globals(&mut self) {
        if self.string_constants.is_empty() {
            return;
        }

        // Sort by name for deterministic output.
        let mut entries: Vec<(String, String)> = self
            .string_constants
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        entries.sort_by(|a, b| a.1.cmp(&b.1));

        for (content, name) in &entries {
            let escaped = llvm_escape_string(content);
            // +1 for the null terminator.
            let len = content.len() + 1;
            self.emit_line(&format!(
                "{name} = private unnamed_addr constant [{len} x i8] c\"{escaped}\\00\""
            ));
        }
        self.emit_blank();
    }

    // -----------------------------------------------------------------------
    // ARC runtime declarations
    // -----------------------------------------------------------------------

    fn emit_arc_declarations(&mut self) {
        self.emit_line("; ARC runtime");
        self.emit_line("declare ptr @kryos_arc_alloc(i64, i64)");
        self.emit_line("declare void @kryos_arc_retain(ptr)");
        self.emit_line("declare void @kryos_arc_release(ptr)");
        self.emit_line("declare void @kryos_arc_set_drop(ptr, ptr)");
        self.emit_line("declare i64 @kryos_arc_alloc_i64(i64)");
        self.emit_blank();
    }

    // -----------------------------------------------------------------------
    // External C function declarations
    // -----------------------------------------------------------------------

    fn emit_extern_declarations(&mut self) {
        self.emit_line("; External C functions (used by Kryos builtins)");
        self.emit_line("declare i32 @puts(ptr)");
        self.emit_line("declare i32 @printf(ptr, ...)");
        self.emit_line("declare void @exit(i32)");
        self.emit_line("declare i32 @fputs(ptr, ptr)");
        self.emit_line("declare i32 @fputc(i32, ptr)");
        self.emit_line("declare ptr @malloc(i64)");
        self.emit_line("declare void @free(ptr)");
        self.emit_line("declare ptr @realloc(ptr, i64)");
        if self.is_windows_target() {
            self.emit_line("declare ptr @__acrt_iob_func(i32)");
        } else {
            self.emit_line("@stderr = external global ptr");
        }
        self.emit_blank();
        self.emit_line("; Kryos runtime functions");
        self.emit_line("declare ptr @kryos_string_new(ptr, i64)");
        self.emit_line("declare ptr @kryos_string_concat(ptr, ptr)");
        self.emit_line("declare i64 @kryos_string_len(ptr)");
        self.emit_line("declare i1 @kryos_string_eq(ptr, ptr)");
        self.emit_line("declare ptr @kryos_string_slice(ptr, i64, i64)");
        self.emit_line("declare i64 @kryos_string_find(ptr, ptr)");
        self.emit_line("declare void @kryos_string_free(ptr)");
        self.emit_line("declare ptr @kryos_array_new(i64, i64)");
        self.emit_line("declare void @kryos_array_push(ptr, i64)");
        self.emit_line("declare i64 @kryos_builtin_pop(i64)");
        self.emit_line("declare i64 @kryos_array_get(ptr, i64)");
        self.emit_line("declare void @kryos_array_set(ptr, i64, i64)");
        self.emit_line("declare i64 @kryos_array_len(ptr)");
        self.emit_line("declare void @kryos_array_free(ptr)");
        self.emit_line("declare ptr @kryos_array_concat(ptr, ptr)");
        self.emit_line("; Map runtime");
        self.emit_line("declare i64 @kryos_map_new()");
        self.emit_line("declare void @kryos_map_insert(i64, i64, i64)");
        self.emit_line("declare void @kryos_map_insert_str(i64, i64, i64)");
        self.emit_line("declare i64 @kryos_map_get(i64, i64)");
        self.emit_line("declare i64 @kryos_map_get_str(i64, i64)");
        self.emit_line("declare i64 @kryos_map_len(i64)");
        self.emit_line("declare void @kryos_map_free(i64)");
        self.emit_line("declare i64 @kryos_map_clone(i64)");
        self.emit_line("declare ptr @kryos_string_clone(ptr)");
        self.emit_line("declare ptr @kryos_array_clone(ptr)");
        self.emit_line("; Builtin runtime");
        self.emit_line("declare i64 @kryos_builtin_len(i64)");
        self.emit_line("declare i64 @kryos_builtin_to_string(i64)");
        self.emit_line("declare i64 @kryos_builtin_trim(i64)");
        self.emit_line("declare i64 @kryos_builtin_trim_start(i64)");
        self.emit_line("declare i64 @kryos_builtin_trim_end(i64)");
        self.emit_line("declare i64 @kryos_builtin_to_upper(i64)");
        self.emit_line("declare i64 @kryos_builtin_to_lower(i64)");
        self.emit_line("declare i64 @kryos_builtin_index_of(i64, i64)");
        self.emit_line("declare i64 @kryos_builtin_contains(i64, i64)");
        self.emit_line("declare i64 @kryos_builtin_starts_with(i64, i64)");
        self.emit_line("declare i64 @kryos_builtin_ends_with(i64, i64)");
        self.emit_line("declare i64 @kryos_builtin_replace(i64, i64, i64)");
        self.emit_line("declare i64 @kryos_builtin_split(i64, i64)");
        self.emit_line("declare i64 @kryos_builtin_join(i64, i64)");
        self.emit_line("declare void @kryos_builtin_sort(i64)");
        self.emit_line("declare void @kryos_builtin_reverse(i64)");
        self.emit_line("declare i64 @kryos_builtin_file_read(i64)");
        self.emit_line("declare void @kryos_builtin_file_write(i64, i64)");
        self.emit_line("declare i64 @kryos_builtin_file_append(i64, i64)");
        self.emit_line("declare i64 @kryos_builtin_file_exists(i64)");
        self.emit_line("declare i64 @kryos_builtin_env_get(i64)");
        self.emit_line("declare void @kryos_builtin_exit(i64)");
        self.emit_line("declare i64 @kryos_builtin_args()");
        self.emit_line("declare i64 @kryos_builtin_read_line()");
        self.emit_line("declare i64 @kryos_builtin_http_get(i64)");
        self.emit_line("declare i64 @kryos_builtin_parse_int(i64)");
        self.emit_line("declare i64 @kryos_builtin_parse_float(i64)");
        self.emit_line("declare i64 @kryos_builtin_type_of(i64)");
        self.emit_line("declare i64 @kryos_builtin_assert(i64, i64)");
        self.emit_line("declare i64 @kryos_builtin_char_code(i64)");
        self.emit_line("declare i64 @kryos_builtin_char_from(i64)");
        self.emit_line("declare i64 @kryos_builtin_substr(i64, i64, i64)");
        self.emit_line("declare i64 @kryos_builtin_time_now()");
        self.emit_line("declare i64 @kryos_builtin_int(i64)");
        self.emit_line("declare i64 @kryos_builtin_float(i64)");
        self.emit_line("declare i64 @kryos_builtin_string_clone(i64)");
        self.emit_line("declare i64 @kryos_builtin_array_clone(i64)");
        self.emit_line("declare i64 @kryos_ipow(i64, i64)");
        self.emit_line("declare double @kryos_fpow(double, double)");
        self.emit_line("declare double @kryos_fmod(double, double)");
        // C math functions (used by sqrt, floor, ceil, sin, cos, etc. builtins)
        self.emit_line("declare double @sqrt(double)");
        self.emit_line("declare double @floor(double)");
        self.emit_line("declare double @ceil(double)");
        self.emit_line("declare double @round(double)");
        self.emit_line("declare double @sin(double)");
        self.emit_line("declare double @cos(double)");
        self.emit_line("declare double @tan(double)");
        self.emit_line("declare double @log(double)");
        self.emit_line("declare double @log2(double)");
        self.emit_line("declare double @log10(double)");
        self.emit_line("declare double @fabs(double)");
        self.emit_line("declare i64 @kryos_i64_to_string(i64)");
        self.emit_line("declare i64 @kryos_f64_to_string(double)");
        self.emit_line("declare i64 @kryos_bool_to_string(i64)");
        // Channel runtime
        self.emit_line("declare i64 @kryos_chan_new_i64()");
        self.emit_line("declare i64 @kryos_chan_send_i64(i64, i64)");
        self.emit_line("declare i64 @kryos_chan_recv_i64(i64)");
        self.emit_line("declare i64 @kryos_chan_try_recv_status_i64(i64)");
        self.emit_line("declare i64 @kryos_chan_last_recv_i64()");
        self.emit_line("declare i64 @kryos_chan_is_closed_i64(i64)");
        // Print runtime (for KryosString handles)
        self.emit_line("declare void @kryos_println_str(ptr)");
        self.emit_line("declare void @kryos_print_str(ptr)");
        self.emit_line("declare void @kryos_eprintln_str(ptr)");
        // Spawn runtime
        self.emit_line("declare i64 @kryos_spawn(i64, ptr, i64)");
        self.emit_line("declare void @kryos_spawn_wait_all()");
        self.emit_line("declare void @kryos_sleep(i64)");
        // Actor runtime
        self.emit_line("declare i64 @kryos_actor_spawn_i64(i64, i64)");
        self.emit_line("declare i64 @kryos_actor_send_i64(i64, i64)");
        self.emit_line("declare i64 @kryos_actor_recv_i64()");
        self.emit_line("declare i64 @kryos_actor_lock_i64(i64)");
        self.emit_line("declare i64 @kryos_actor_unlock_i64(i64)");
        // Tensor runtime
        self.emit_line("declare i64 @kryos_tensor_zeros(ptr, i64)");
        self.emit_line("declare i64 @kryos_tensor_ones(ptr, i64)");
        self.emit_line("declare i64 @kryos_tensor_rand(ptr, i64)");
        self.emit_line("declare i64 @kryos_tensor_randn(ptr, i64)");
        self.emit_line("declare i64 @kryos_tensor_from_data(ptr, i64, ptr, i64)");
        self.emit_line("declare i64 @kryos_tensor_eye(i64)");
        self.emit_line("declare i64 @kryos_tensor_arange(i64, i64, i64)");
        self.emit_line("declare i64 @kryos_tensor_ndim(i64)");
        self.emit_line("declare i64 @kryos_tensor_numel(i64)");
        self.emit_line("declare i64 @kryos_tensor_shape_dim(i64, i64)");
        self.emit_line("declare i64 @kryos_tensor_get(i64, i64)");
        self.emit_line("declare void @kryos_tensor_set(i64, i64, i64)");
        self.emit_line("declare i64 @kryos_tensor_add(i64, i64)");
        self.emit_line("declare i64 @kryos_tensor_sub(i64, i64)");
        self.emit_line("declare i64 @kryos_tensor_mul(i64, i64)");
        self.emit_line("declare i64 @kryos_tensor_div(i64, i64)");
        self.emit_line("declare i64 @kryos_tensor_pow(i64, i64)");
        self.emit_line("declare i64 @kryos_tensor_scale(i64, i64)");
        self.emit_line("declare i64 @kryos_tensor_exp(i64)");
        self.emit_line("declare i64 @kryos_tensor_log(i64)");
        self.emit_line("declare i64 @kryos_tensor_sqrt(i64)");
        self.emit_line("declare i64 @kryos_tensor_tanh(i64)");
        self.emit_line("declare i64 @kryos_tensor_sigmoid(i64)");
        self.emit_line("declare i64 @kryos_tensor_relu(i64)");
        self.emit_line("declare i64 @kryos_tensor_neg(i64)");
        self.emit_line("declare i64 @kryos_tensor_sum(i64)");
        self.emit_line("declare i64 @kryos_tensor_mean(i64)");
        self.emit_line("declare i64 @kryos_tensor_max(i64)");
        self.emit_line("declare i64 @kryos_tensor_min(i64)");
        self.emit_line("declare i64 @kryos_tensor_argmax(i64)");
        self.emit_line("declare i64 @kryos_tensor_argmin(i64)");
        self.emit_line("declare i64 @kryos_tensor_matmul(i64, i64)");
        self.emit_line("declare i64 @kryos_tensor_transpose(i64)");
        self.emit_line("declare i64 @kryos_tensor_reshape(i64, ptr, i64)");
        self.emit_line("declare i64 @kryos_tensor_flatten(i64)");
        self.emit_line("declare i64 @kryos_tensor_softmax(i64, i64)");
        self.emit_line("declare i64 @kryos_tensor_cross_entropy(i64, i64)");
        self.emit_line("declare i64 @kryos_tensor_mse_loss(i64, i64)");
        self.emit_line("declare i64 @kryos_tensor_to_string(i64)");
        self.emit_line("declare void @kryos_tensor_free(i64)");
        // Process management (stdlib-native)
        self.emit_line("declare i64 @kryos_env_get(ptr, i64, ptr, i64)");
        self.emit_line("declare void @kryos_process_exit(i32)");
        self.emit_line("declare i64 @kryos_process_exec(ptr, i64, ptr, i64)");
        self.emit_line("declare i64 @kryos_process_exec_simple(ptr, i64)");
        self.emit_line("declare i64 @kryos_process_argc()");
        self.emit_line("declare i64 @kryos_process_argv(i64)");
        // Filesystem (stdlib-native)
        self.emit_line("declare i32 @kryos_path_exists(ptr, i64)");
        self.emit_line("declare i32 @kryos_dir_create(ptr, i64)");
        self.emit_line("declare i32 @kryos_file_remove(ptr, i64)");
        self.emit_line("declare i64 @kryos_dir_list(ptr, i64, ptr, i64)");
        self.emit_line("declare i64 @kryos_dir_walk(ptr, i64, ptr, i64)");
        // File I/O (stdlib-native)
        self.emit_line("declare i64 @kryos_file_open(ptr, i64, i8)");
        self.emit_line("declare i64 @kryos_file_read(i64, ptr, i64)");
        self.emit_line("declare i64 @kryos_file_write(i64, ptr, i64)");
        self.emit_line("declare i32 @kryos_file_close(i64)");
        self.emit_line("declare i64 @kryos_stderr_write(ptr, i64)");
        // TCP / TLS / PostgreSQL (handle-ABI: i64 in, i64 out)
        self.emit_line("declare i64 @kryos_tcp_connect_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_tcp_bind_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_tcp_accept(i64)");
        self.emit_line("declare i64 @kryos_tcp_send_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_tcp_recv_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_socket_close_ks(i64)");
        self.emit_line("declare i64 @kryos_tcp_set_nonblocking(i64, i64)");
        self.emit_line("declare i64 @kryos_tcp_try_accept(i64)");
        self.emit_line("declare i64 @kryos_tcp_try_recv_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_tls_server_config_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_tls_accept(i64, i64)");
        self.emit_line("declare i64 @kryos_tls_send_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_tls_recv_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_tls_close_ks(i64)");
        self.emit_line("declare i64 @kryos_pg_connect_ks(i64)");
        self.emit_line("declare i64 @kryos_pg_exec_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_pg_query_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_pg_close_ks(i64)");
        // Unix domain sockets (v2.0)
        self.emit_line("declare i64 @kryos_uds_connect_ks(i64)");
        self.emit_line("declare i64 @kryos_uds_bind_ks(i64)");
        self.emit_line("declare i64 @kryos_uds_accept(i64)");
        self.emit_line("declare i64 @kryos_uds_send_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_uds_recv_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_uds_close(i64)");
        // WebSocket (RFC 6455) helpers (v2.0)
        self.emit_line("declare i64 @kryos_ws_accept_key_ks(i64)");
        self.emit_line("declare i64 @kryos_ws_encode_text_ks(i64)");
        self.emit_line("declare i64 @kryos_ws_encode_binary_ks(i64)");
        self.emit_line("declare i64 @kryos_ws_encode_close(i64)");
        self.emit_line("declare i64 @kryos_ws_encode_ping_ks(i64)");
        self.emit_line("declare i64 @kryos_ws_encode_pong_ks(i64)");
        self.emit_line("declare i64 @kryos_ws_unmask_ks(i64, i64, i64, i64)");
        self.emit_line("declare i64 @kryos_ws_read_frame_ks(i64)");
        // JSON / Crypto / Regex / Time (handle ABI)
        self.emit_line("declare i64 @kryos_json_parse(i64)");
        self.emit_line("declare i64 @kryos_json_stringify(i64)");
        self.emit_line("declare i64 @kryos_json_get(i64, i64)");
        self.emit_line("declare i64 @kryos_json_get_index(i64, i64)");
        self.emit_line("declare i64 @kryos_json_to_str(i64)");
        self.emit_line("declare i64 @kryos_json_to_int(i64)");
        self.emit_line("declare double @kryos_json_to_float(i64)");
        self.emit_line("declare i64 @kryos_json_is_null(i64)");
        self.emit_line("declare i64 @kryos_json_length(i64)");
        self.emit_line("declare i64 @kryos_json_type(i64)");
        self.emit_line("declare i64 @kryos_json_string(i64)");
        self.emit_line("declare i64 @kryos_json_number(double)");
        self.emit_line("declare i64 @kryos_json_bool(i64)");
        self.emit_line("declare i64 @kryos_json_null()");
        self.emit_line("declare i64 @kryos_json_object(i64, i64)");
        self.emit_line("declare i64 @kryos_json_array(i64)");
        self.emit_line("declare i64 @kryos_sha256_ks(i64)");
        self.emit_line("declare i64 @kryos_sha512_ks(i64)");
        self.emit_line("declare i64 @kryos_sha1_hex_ks(i64)");
        self.emit_line("declare i64 @kryos_sha1_base64_ks(i64)");
        self.emit_line("declare i64 @kryos_base64_encode_ks(i64)");
        self.emit_line("declare i64 @kryos_base64_decode_ks(i64)");
        self.emit_line("declare i64 @kryos_random_bytes_ks(i64)");
        self.emit_line("declare i64 @kryos_chr_ks(i64)");
        self.emit_line("declare i64 @kryos_byte_at_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_time_now_secs()");
        self.emit_line("declare i64 @kryos_time_now_millis()");
        self.emit_line("declare void @kryos_sleep_ms(i64)");
        self.emit_line("declare i64 @kryos_regex_new_ks(i64)");
        self.emit_line("declare i64 @kryos_regex_is_match_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_mutex_new()");
        self.emit_line("declare void @kryos_mutex_lock(i64)");
        self.emit_line("declare void @kryos_mutex_unlock(i64)");
        self.emit_line("declare void @kryos_mutex_drop(i64)");
        self.emit_line("; Exception runtime (used by try/catch)");
        self.emit_line("declare void @kryos_exception_throw(i64)");
        self.emit_line("declare i64 @kryos_exception_check()");
        self.emit_line("declare i64 @kryos_exception_take()");
        self.emit_blank();
    }

    /// Returns true if the target is Windows (for platform-specific codegen).
    fn is_windows_target(&self) -> bool {
        if let Some(ref triple) = self.options.target_triple {
            triple.contains("windows")
        } else {
            cfg!(target_os = "windows")
        }
    }

    // -----------------------------------------------------------------------
    // Closure dropper functions
    // -----------------------------------------------------------------------

    /// Emit dropper functions for closures with heap-typed captures.
    /// Each dropper has signature `void(ptr env)` and frees captured heap
    /// values before the ARC system frees the env buffer itself.
    fn emit_closure_droppers(&mut self) {
        let cap_types_snapshot: Vec<(String, Vec<Option<MirType>>)> = self
            .closure_cap_types
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        for (func_name, cap_types) in &cap_types_snapshot {
            let has_heap_caps = cap_types.iter().any(|ct| {
                matches!(
                    ct,
                    Some(MirType::Str)
                        | Some(MirType::Array(_, _))
                        | Some(MirType::Function { .. })
                        | Some(MirType::Shared(_))
                        | Some(MirType::Struct(_))
                        | Some(MirType::Enum(_))
                        | Some(MirType::Map { .. })
                )
            });
            if !has_heap_caps {
                continue;
            }

            let dropper_name = format!("{func_name}_drop");
            self.emit_line(&format!("; Closure dropper for {func_name}"));
            self.emit_line(&format!(
                "define internal void @{dropper_name}(ptr %env) {{"
            ));
            self.emit_line("entry:");

            for (i, cap_ty) in cap_types.iter().enumerate() {
                let offset = i + 1;
                let needs_free = matches!(
                    cap_ty,
                    Some(MirType::Str)
                        | Some(MirType::Array(_, _))
                        | Some(MirType::Function { .. })
                        | Some(MirType::Shared(_))
                        | Some(MirType::Struct(_))
                        | Some(MirType::Enum(_))
                        | Some(MirType::Map { .. })
                );
                if !needs_free {
                    continue;
                }

                let cap_ptr = self.next_temp();
                self.emit_line(&format!(
                    "  {cap_ptr} = getelementptr i64, ptr %env, i64 {offset}"
                ));
                let cap_val = self.next_temp();

                match cap_ty {
                    Some(MirType::Str) => {
                        self.emit_line(&format!("  {cap_val} = load ptr, ptr {cap_ptr}"));
                        self.emit_line(&format!("  call void @kryos_string_free(ptr {cap_val})"));
                    }
                    Some(MirType::Array(_, _)) => {
                        self.emit_line(&format!("  {cap_val} = load ptr, ptr {cap_ptr}"));
                        self.emit_line(&format!("  call void @kryos_array_free(ptr {cap_val})"));
                    }
                    Some(MirType::Map { .. }) => {
                        self.emit_line(&format!("  {cap_val} = load i64, ptr {cap_ptr}"));
                        self.emit_line(&format!("  call void @kryos_map_free(i64 {cap_val})"));
                    }
                    Some(MirType::Function { .. }) | Some(MirType::Shared(_)) => {
                        self.emit_line(&format!("  {cap_val} = load ptr, ptr {cap_ptr}"));
                        self.emit_line(&format!("  call void @kryos_arc_release(ptr {cap_val})"));
                    }
                    Some(MirType::Struct(_)) | Some(MirType::Enum(_)) => {
                        self.emit_line(&format!("  {cap_val} = load ptr, ptr {cap_ptr}"));
                        self.emit_line(&format!("  call void @free(ptr {cap_val})"));
                    }
                    _ => {}
                }
            }

            self.emit_line("  ret void");
            self.emit_line("}");
            self.emit_blank();
        }
    }

    // -----------------------------------------------------------------------
    // Closure env-thunk functions
    // -----------------------------------------------------------------------

    /// Emit `{func_name}_env(ptr env, i64 arg0, ...)` thunks for each
    /// lambda used as a value.  The thunk loads captures from the env
    /// (offsets 1..=N) and forwards to the underlying function with the
    /// captures prepended to the user args.  This gives all function
    /// values a uniform env-based calling convention so CallIndirect can
    /// dispatch through `env[0]` regardless of how many captures the
    /// closure has.
    fn emit_closure_thunks(&mut self) {
        let sig_snapshot: Vec<(String, usize, String, Vec<Option<MirType>>)> = self
            .closure_user_sig
            .iter()
            .map(|(name, (n, ret))| {
                let caps = self
                    .closure_cap_types
                    .get(name)
                    .cloned()
                    .unwrap_or_default();
                (name.clone(), *n, ret.clone(), caps)
            })
            .collect();

        for (func_name, user_params, ret_ty, cap_types) in &sig_snapshot {
            let thunk_name = format!("{func_name}_env");
            self.emit_line(&format!("; Closure env-thunk for {func_name}"));

            // Build parameter list: ptr env, i64 arg0, i64 arg1, ...
            let mut params = String::from("ptr %env");
            for i in 0..*user_params {
                params.push_str(&format!(", i64 %u{i}"));
            }
            // The thunk always returns i64 (uniform slot).  Functions with
            // non-i64 returns are widened/coerced on the way out.
            self.emit_line(&format!(
                "define internal i64 @{thunk_name}({params}) {{"
            ));
            self.emit_line("entry:");

            // Load each capture from env[i+1] (i64-typed slots).
            let mut call_args: Vec<String> = Vec::new();
            let underlying_params = self
                .func_param_types
                .get(func_name.as_str())
                .cloned()
                .unwrap_or_default();

            for (i, cap_ty) in cap_types.iter().enumerate() {
                let cap_ptr = self.next_temp();
                self.emit_line(&format!(
                    "  {cap_ptr} = getelementptr i64, ptr %env, i64 {}",
                    i + 1
                ));
                let expected_ty = underlying_params
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| "i64".to_string());
                // Load using the storage type matching the capture kind.
                let raw = self.next_temp();
                match cap_ty {
                    Some(MirType::Str)
                    | Some(MirType::Array(_, _))
                    | Some(MirType::Function { .. })
                    | Some(MirType::Shared(_))
                    | Some(MirType::Struct(_))
                    | Some(MirType::Enum(_)) => {
                        self.emit_line(&format!("  {raw} = load ptr, ptr {cap_ptr}"));
                        // If the underlying fn expects a pointer-shaped
                        // param, pass as-is; otherwise coerce to i64.
                        if expected_ty == "ptr" {
                            call_args.push(format!("ptr {raw}"));
                        } else {
                            let i = self.next_temp();
                            self.emit_line(&format!(
                                "  {i} = ptrtoint ptr {raw} to i64"
                            ));
                            let coerced = self.coerce_value(&i, "i64", &expected_ty);
                            call_args.push(format!("{expected_ty} {coerced}"));
                        }
                    }
                    _ => {
                        self.emit_line(&format!("  {raw} = load i64, ptr {cap_ptr}"));
                        if expected_ty == "i64" {
                            call_args.push(format!("i64 {raw}"));
                        } else if expected_ty == "ptr" {
                            let p = self.next_temp();
                            self.emit_line(&format!(
                                "  {p} = inttoptr i64 {raw} to ptr"
                            ));
                            call_args.push(format!("ptr {p}"));
                        } else {
                            let coerced = self.coerce_value(&raw, "i64", &expected_ty);
                            call_args.push(format!("{expected_ty} {coerced}"));
                        }
                    }
                }
            }

            // Append user args (already i64 in the thunk's parameter list).
            for i in 0..*user_params {
                let idx = cap_types.len() + i;
                let expected_ty = underlying_params
                    .get(idx)
                    .cloned()
                    .unwrap_or_else(|| "i64".to_string());
                let raw = format!("%u{i}");
                if expected_ty == "i64" {
                    call_args.push(format!("i64 {raw}"));
                } else if expected_ty == "ptr" {
                    let p = self.next_temp();
                    self.emit_line(&format!(
                        "  {p} = inttoptr i64 {raw} to ptr"
                    ));
                    call_args.push(format!("ptr {p}"));
                } else {
                    let coerced = self.coerce_value(&raw, "i64", &expected_ty);
                    call_args.push(format!("{expected_ty} {coerced}"));
                }
            }

            let arg_list = call_args.join(", ");
            let underlying_ret = self
                .func_ret_types
                .get(func_name.as_str())
                .cloned()
                .unwrap_or_else(|| ret_ty.clone());

            if underlying_ret == "void" {
                self.emit_line(&format!(
                    "  call void @{func_name}({arg_list})"
                ));
                self.emit_line("  ret i64 0");
            } else {
                let r = self.next_temp();
                self.emit_line(&format!(
                    "  {r} = call {underlying_ret} @{func_name}({arg_list})"
                ));
                if underlying_ret == "i64" {
                    self.emit_line(&format!("  ret i64 {r}"));
                } else if underlying_ret == "ptr" {
                    let i = self.next_temp();
                    self.emit_line(&format!("  {i} = ptrtoint ptr {r} to i64"));
                    self.emit_line(&format!("  ret i64 {i}"));
                } else {
                    let coerced = self.coerce_value(&r, &underlying_ret, "i64");
                    self.emit_line(&format!("  ret i64 {coerced}"));
                }
            }
            self.emit_line("}");
            self.emit_blank();
        }
    }

    // Type drop helpers
    // -----------------------------------------------------------------------

    /// Emit named drop helper functions for struct/enum types with heap fields.
    /// `__kryos_drop_MyStruct(ptr)` recursively frees nested heap fields then
    /// frees the struct/enum allocation. Used by array element drop loops.
    fn emit_type_drop_helpers(&mut self) {
        let struct_defs: Vec<(String, Vec<(String, MirType)>)> = self
            .struct_defs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let enum_defs: Vec<(String, Vec<EnumVariantDef>)> = self
            .enum_defs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let has_heap_fields = |fields: &[(String, MirType)]| -> bool {
            fields.iter().any(|(_, ty)| {
                matches!(
                    ty,
                    MirType::Str
                        | MirType::Array(_, _)
                        | MirType::Struct(_)
                        | MirType::Function { .. }
                        | MirType::Enum(_)
                        | MirType::Shared(_)
                        | MirType::Map { .. }
                )
            })
        };

        // Struct drop helpers.
        for (name, fields) in &struct_defs {
            if name == "Map" || !has_heap_fields(fields) {
                continue;
            }
            let drop_name = format!("__kryos_drop_{name}");
            self.emit_line(&format!("; Type drop helper for struct {name}"));
            self.emit_line(&format!("define internal void @{drop_name}(ptr %ptr) {{"));
            self.emit_line("entry:");

            for (field_idx, (_, field_ty)) in fields.iter().enumerate() {
                let needs_drop = matches!(
                    field_ty,
                    MirType::Str
                        | MirType::Array(_, _)
                        | MirType::Struct(_)
                        | MirType::Function { .. }
                        | MirType::Enum(_)
                        | MirType::Shared(_)
                        | MirType::Map { .. }
                );
                if !needs_drop {
                    continue;
                }

                let gep = self.next_temp();
                self.emit_line(&format!(
                    "  {gep} = getelementptr i64, ptr %ptr, i32 {field_idx}"
                ));
                let fv = self.next_temp();

                match field_ty {
                    MirType::Str => {
                        self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                        self.emit_line(&format!("  call void @kryos_string_free(ptr {fv})"));
                    }
                    MirType::Array(_, _) => {
                        self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                        self.emit_line(&format!("  call void @kryos_array_free(ptr {fv})"));
                    }
                    MirType::Map { .. } => {
                        self.emit_line(&format!("  {fv} = load i64, ptr {gep}"));
                        self.emit_line(&format!("  call void @kryos_map_free(i64 {fv})"));
                    }
                    MirType::Function { .. } | MirType::Shared(_) => {
                        self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                        self.emit_line(&format!("  call void @kryos_arc_release(ptr {fv})"));
                    }
                    MirType::Struct(n) => {
                        let nested_drop = format!("__kryos_drop_{n}");
                        self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                        // Check if nested struct has a drop helper; fall back to free.
                        let has_nested = struct_defs
                            .iter()
                            .any(|(sn, sf)| sn == n && sn != "Map" && has_heap_fields(sf));
                        if has_nested {
                            self.emit_line(&format!("  call void @{nested_drop}(ptr {fv})"));
                        } else {
                            self.emit_line(&format!("  call void @free(ptr {fv})"));
                        }
                    }
                    MirType::Enum(n) => {
                        let nested_drop = format!("__kryos_drop_{n}");
                        self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                        let has_nested = enum_defs.iter().any(|(en, evs)| {
                            en == n
                                && evs.iter().any(|v| {
                                    v.fields.iter().any(|f| {
                                        matches!(
                                            f,
                                            MirType::Str
                                                | MirType::Array(_, _)
                                                | MirType::Struct(_)
                                                | MirType::Function { .. }
                                                | MirType::Enum(_)
                                                | MirType::Shared(_)
                                                | MirType::Map { .. }
                                        )
                                    })
                                })
                        });
                        if has_nested {
                            self.emit_line(&format!("  call void @{nested_drop}(ptr {fv})"));
                        } else {
                            self.emit_line(&format!("  call void @free(ptr {fv})"));
                        }
                    }
                    _ => {}
                }
            }

            self.emit_line("  call void @free(ptr %ptr)");
            self.emit_line("  ret void");
            self.emit_line("}");
            self.emit_blank();
        }

        // Enum drop helpers.
        for (name, variants) in &enum_defs {
            let has_droppable = variants.iter().any(|v| {
                v.fields.iter().any(|f| {
                    matches!(
                        f,
                        MirType::Str
                            | MirType::Array(_, _)
                            | MirType::Struct(_)
                            | MirType::Function { .. }
                            | MirType::Enum(_)
                            | MirType::Shared(_)
                            | MirType::Map { .. }
                    )
                })
            });
            // Always emit a drop helper for every enum, even when no fields
            // need recursive cleanup. Array-of-enum drop loops call this
            // unconditionally; a missing symbol would break linking.
            let drop_name = format!("__kryos_drop_{name}");
            self.emit_line(&format!("; Type drop helper for enum {name}"));
            self.emit_line(&format!("define internal void @{drop_name}(ptr %ptr) {{"));
            self.emit_line("entry:");

            if !has_droppable {
                // Stub: enum has no droppable fields. Release the arc-allocated
                // buffer (matches the path used by `array_push` for aggregate
                // elements, which routes through `kryos_arc_alloc`).
                let nck = self.next_temp();
                self.emit_line(&format!("  {nck} = icmp eq ptr %ptr, null"));
                self.emit_line(&format!(
                    "  br i1 {nck}, label %stub_ret_{name}, label %stub_rel_{name}"
                ));
                self.emit_line(&format!("stub_rel_{name}:"));
                self.emit_line("  call void @kryos_arc_release(ptr %ptr)");
                self.emit_line(&format!("  br label %stub_ret_{name}"));
                self.emit_line(&format!("stub_ret_{name}:"));
                self.emit_line("  ret void");
                self.emit_line("}");
                self.emit_blank();
                continue;
            }

            let uid = self.temp_counter;
            self.temp_counter += 1;
            let tag_tmp = self.next_temp();
            self.emit_line(&format!("  {tag_tmp} = load i64, ptr %ptr"));

            let merge_label = format!("edrop_merge_{uid}");
            let mut prev_skip_label = String::new();

            for (idx, variant) in variants.iter().enumerate() {
                let droppable: Vec<(usize, &MirType)> = variant
                    .fields
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| {
                        matches!(
                            f,
                            MirType::Str
                                | MirType::Array(_, _)
                                | MirType::Struct(_)
                                | MirType::Function { .. }
                                | MirType::Enum(_)
                                | MirType::Shared(_)
                                | MirType::Map { .. }
                        )
                    })
                    .collect();
                if droppable.is_empty() {
                    continue;
                }

                let var_label = format!("edrop_v{idx}_{uid}");
                let skip_label = format!("edrop_skip{idx}_{uid}");

                if !prev_skip_label.is_empty() {
                    // We're already in the previous skip block.
                } else {
                    // First variant check — we're in entry.
                }

                let cmp = self.next_temp();
                self.emit_line(&format!("  {cmp} = icmp eq i64 {tag_tmp}, {idx}"));
                self.emit_line(&format!(
                    "  br i1 {cmp}, label %{var_label}, label %{skip_label}"
                ));

                self.emit_line(&format!("{var_label}:"));
                for (fi, fty) in &droppable {
                    let offset = fi + 1;
                    let fgep = self.next_temp();
                    self.emit_line(&format!(
                        "  {fgep} = getelementptr i64, ptr %ptr, i64 {offset}"
                    ));
                    let fval = self.next_temp();
                    match *fty {
                        MirType::Str => {
                            self.emit_line(&format!("  {fval} = load ptr, ptr {fgep}"));
                            self.emit_line(&format!("  call void @kryos_string_free(ptr {fval})"));
                        }
                        MirType::Array(_, _) => {
                            self.emit_line(&format!("  {fval} = load ptr, ptr {fgep}"));
                            self.emit_line(&format!("  call void @kryos_array_free(ptr {fval})"));
                        }
                        MirType::Map { .. } => {
                            self.emit_line(&format!("  {fval} = load i64, ptr {fgep}"));
                            self.emit_line(&format!("  call void @kryos_map_free(i64 {fval})"));
                        }
                        MirType::Function { .. } | MirType::Shared(_) => {
                            self.emit_line(&format!("  {fval} = load ptr, ptr {fgep}"));
                            self.emit_line(&format!("  call void @kryos_arc_release(ptr {fval})"));
                        }
                        MirType::Struct(ref n) | MirType::Enum(ref n) => {
                            let nested = format!("__kryos_drop_{n}");
                            self.emit_line(&format!("  {fval} = load ptr, ptr {fgep}"));
                            self.emit_line(&format!("  call void @{nested}(ptr {fval})"));
                        }
                        _ => {}
                    }
                }
                self.emit_line(&format!("  br label %{merge_label}"));

                self.emit_line(&format!("{skip_label}:"));
                prev_skip_label = skip_label;
            }

            self.emit_line(&format!("  br label %{merge_label}"));
            self.emit_line(&format!("{merge_label}:"));
            self.emit_line("  call void @free(ptr %ptr)");
            self.emit_line("  ret void");
            self.emit_line("}");
            self.emit_blank();
        }
    }

    // Main wrapper
    // -----------------------------------------------------------------------

    fn emit_main_wrapper(&mut self) {
        self.emit_line("; C-compatible main() entry point");
        self.emit_line("define i32 @main() {");
        self.emit_line("entry:");
        self.emit_line("  call void @_kryos_main()");
        // Wait for any spawned threads to complete before exit (no-op if none).
        self.emit_line("  call void @kryos_spawn_wait_all()");
        self.emit_line("  ret i32 0");
        self.emit_line("}");
        self.emit_blank();
    }

    // -----------------------------------------------------------------------
    // Pre-scan
    // -----------------------------------------------------------------------

    /// Walk a function to discover string constants and ARC usage.
    fn prescan_function(&mut self, func: &MirFunction) {
        for block in &func.blocks {
            for inst in &block.instructions {
                match inst {
                    Instruction::Assign { value, .. } => self.prescan_rvalue(value),
                    Instruction::ArcRetain { .. } | Instruction::ArcRelease { .. } => {
                        self.needs_arc_runtime = true;
                    }
                    Instruction::Spawn { args, .. } => {
                        for a in args {
                            self.prescan_operand(a);
                        }
                    }
                    Instruction::ActorSpawn { state, .. } => {
                        self.prescan_operand(state);
                    }
                    Instruction::ActorSend { args, .. } => {
                        for a in args {
                            self.prescan_operand(a);
                        }
                    }
                    Instruction::ActorStateStore { value, .. } => {
                        self.prescan_operand(value);
                    }
                    Instruction::ActorStateLoad { .. } => {}
                    Instruction::StoreDeref { ptr, value } => {
                        self.prescan_operand(ptr);
                        self.prescan_operand(value);
                    }
                    Instruction::StoreField { object, value, .. } => {
                        self.prescan_operand(object);
                        self.prescan_operand(value);
                    }
                    Instruction::Drop { .. }
                    | Instruction::Nop
                    | Instruction::Send { .. }
                    | Instruction::Receive { .. } => {}
                }
            }
            // Also scan terminator operands for constants (rare, but possible).
            self.prescan_terminator(&block.terminator);
        }
    }

    fn prescan_rvalue(&mut self, rv: &RValue) {
        match rv {
            RValue::ConstString(s) => {
                self.intern_string(s);
            }
            RValue::ArcAlloc { .. } => {
                self.needs_arc_runtime = true;
            }
            RValue::BinOp { left, right, .. } => {
                self.prescan_operand(left);
                self.prescan_operand(right);
            }
            RValue::UnOp { operand, .. } => self.prescan_operand(operand),
            RValue::Use(op) => self.prescan_operand(op),
            RValue::Call { args, .. } => {
                for arg in args {
                    self.prescan_operand(arg);
                }
            }
            RValue::CallIndirect { callee, args } => {
                self.prescan_operand(callee);
                for arg in args {
                    self.prescan_operand(arg);
                }
            }
            RValue::Array(ops) | RValue::Tuple(ops) => {
                for op in ops {
                    self.prescan_operand(op);
                }
            }
            RValue::Struct { fields, .. } => {
                for (_, op) in fields {
                    self.prescan_operand(op);
                }
            }
            RValue::Field { object, .. } => self.prescan_operand(object),
            RValue::Index { object, index } => {
                self.prescan_operand(object);
                self.prescan_operand(index);
            }
            RValue::Cast { operand, .. } => self.prescan_operand(operand),
            RValue::EnumVariant { fields, .. } => {
                for op in fields {
                    self.prescan_operand(op);
                }
            }
            RValue::EnumTag { operand } => self.prescan_operand(operand),
            RValue::EnumPayload { operand, .. } => self.prescan_operand(operand),
            RValue::Closure { captures, .. } => {
                for cap in captures {
                    self.prescan_operand(cap);
                }
            }
            RValue::Map(entries) => {
                for (k, v) in entries {
                    self.prescan_operand(k);
                    self.prescan_operand(v);
                }
            }
            RValue::StringConcat(parts) => {
                for p in parts {
                    self.prescan_operand(p);
                }
            }
            RValue::Range { start, end, .. } => {
                if let Some(s) = start {
                    self.prescan_operand(s);
                }
                if let Some(e) = end {
                    self.prescan_operand(e);
                }
            }
            RValue::AddrOf { .. } => {}
            RValue::Deref { operand } => self.prescan_operand(operand),
            RValue::Comptime(inner) => self.prescan_rvalue(inner),
            RValue::MakeTraitObject { value, .. } => self.prescan_operand(value),
            RValue::VtableCall { object, args, .. } => {
                self.prescan_operand(object);
                for arg in args {
                    self.prescan_operand(arg);
                }
            }
            RValue::ConstInt(_)
            | RValue::ConstFloat(_)
            | RValue::ConstBool(_)
            | RValue::ConstNone => {}
        }
    }

    fn prescan_operand(&mut self, op: &Operand) {
        if let Operand::Constant(Constant::Str(s)) = op {
            self.intern_string(s);
        }
    }

    fn prescan_terminator(&mut self, term: &Terminator) {
        match term {
            Terminator::Return(Some(op)) => self.prescan_operand(op),
            Terminator::Branch { cond, .. } => self.prescan_operand(cond),
            Terminator::Switch { value, .. } => self.prescan_operand(value),
            _ => {}
        }
    }

    /// Intern a string constant, returning its global name (e.g. `@.str.0`).
    fn intern_string(&mut self, s: &str) -> String {
        if let Some(name) = self.string_constants.get(s) {
            return name.clone();
        }
        let name = format!("@.str.{}", self.string_counter);
        self.string_counter += 1;
        self.string_constants.insert(s.to_string(), name.clone());
        name
    }

    // -----------------------------------------------------------------------
    // Function emission
    // -----------------------------------------------------------------------

    fn emit_function(&mut self, func: &MirFunction) -> Result<(), CodegenError> {
        self.emit_function_as(func, &func.name.clone())
    }

    fn emit_function_as(&mut self, func: &MirFunction, name: &str) -> Result<(), CodegenError> {
        // Build the local type map for this function.
        self.local_types.clear();
        self.value_types.clear();
        for local in &func.locals {
            let llvm_ty = match &local.ty {
                MirType::Enum(name) => {
                    let max = self.enum_max_fields(name);
                    self.enum_llvm_type(name, max)
                }
                other => mir_type_to_llvm(other),
            };
            self.local_types.insert(local.id.0, llvm_ty);
        }

        // Detect which locals need alloca/store/load (mutable or assigned >1 time).
        self.mutable_locals.clear();
        let mut assign_counts: HashMap<u32, u32> = HashMap::new();
        for block in &func.blocks {
            for inst in &block.instructions {
                if let Instruction::Assign { dest, .. } = inst {
                    *assign_counts.entry(dest.0).or_insert(0) += 1;
                }
            }
        }
        for local in &func.locals {
            let count = assign_counts.get(&local.id.0).copied().unwrap_or(0);
            if local.mutable || count > 1 {
                self.mutable_locals.insert(local.id.0);
            }
        }

        let ret_agg = self.aggregate_llvm_ty(&func.ret_ty);
        let ret = if ret_agg.is_some() {
            "void".to_string()
        } else {
            self.sig_ty_to_llvm(&func.ret_ty)
        };
        let mut param_strs: Vec<String> = Vec::new();
        if let Some(ref agg) = ret_agg {
            param_strs.push(format!("ptr sret({agg}) %_sret"));
        }
        for p in &func.params {
            if let Some(agg) = self.aggregate_llvm_ty(&p.ty) {
                param_strs.push(format!("ptr byval({agg}) %_{}_arg", p.local.0));
            } else {
                param_strs.push(format!("{} %_{}", self.sig_ty_to_llvm(&p.ty), p.local.0));
            }
        }
        let params = param_strs.join(", ");

        // Note: we intentionally do NOT attach `!dbg` to function `define`
        // headers. Doing so requires every `call` instruction inside the
        // function to carry a matching `!dbg !DILocation`, otherwise the
        // LLVM verifier strips all debug info as invalid. Since Kryos
        // does not yet plumb MIR spans through codegen, we emit only the
        // module-level !DICompileUnit + !DIFile so addr2line resolves
        // file/dir names. Per-function DISubprogram + per-call !dbg is
        // tracked for v2.3 once MIR carries source locations.
        if self.options.debug_info && self.options.source_file_path.is_some() {
            self.emitted_function_names.push(name.to_string());
        }
        self.emit_line(&format!("define {ret} @{name}({params}) {{"));

        // Detect TCO loops: if any other block branches back to bb0, we must
        // emit the param-spill + alloca init in a separate `entry:` block
        // (which falls through to bb0). Otherwise the back-edge re-executes
        // the `store %_0, ptr %_0.addr` and overwrites the updated value.
        let entry_id = func.blocks[0].id;
        let has_back_edge_to_entry = func.blocks.iter().skip(1).any(|b| match &b.terminator {
            Terminator::Goto(t) => *t == entry_id,
            Terminator::Branch { then_block, else_block, .. } => {
                *then_block == entry_id || *else_block == entry_id
            }
            _ => false,
        });

        let first_block = &func.blocks[0];
        if has_back_edge_to_entry {
            self.emit_line("entry:");
        } else {
            self.emit_line(&format!("bb{}:", first_block.id.0));
        }

        // Emit allocas for all mutable locals at the top of the entry block.
        // Use the resolved local_types so enums/aggregates get the correct
        // backing storage size (e.g. `alloca { i64, i64 }`, not `alloca i64`).
        let _param_ids: HashSet<u32> = func.params.iter().map(|p| p.local.0).collect();
        for local in &func.locals {
            if self.mutable_locals.contains(&local.id.0) {
                let ty = self.local_type(local.id);
                if ty != "void" {
                    self.emit_line(&format!("  %_{}.addr = alloca {ty}", local.id.0));
                }
            }
        }
        // For aggregate (byval) params: load the aggregate from the byval ptr
        // into the SSA value `%_N`, or into the alloca if mutable.
        for p in &func.params {
            if let Some(agg) = self.aggregate_llvm_ty(&p.ty) {
                if self.mutable_locals.contains(&p.local.0) {
                    let tmp = self.next_temp();
                    self.emit_line(&format!("  {tmp} = load {agg}, ptr %_{}_arg", p.local.0));
                    self.emit_line(&format!("  store {agg} {tmp}, ptr %_{}.addr", p.local.0));
                } else {
                    self.emit_line(&format!(
                        "  %_{} = load {agg}, ptr %_{}_arg",
                        p.local.0, p.local.0
                    ));
                }
            }
        }
        // Store parameter values into their allocas (non-aggregate params only).
        for param in &func.params {
            if self.aggregate_llvm_ty(&param.ty).is_some() {
                continue;
            }
            if self.mutable_locals.contains(&param.local.0) {
                let ty = mir_type_to_llvm(&param.ty);
                if ty != "void" {
                    self.emit_line(&format!(
                        "  store {ty} %_{}, ptr %_{}.addr",
                        param.local.0, param.local.0
                    ));
                }
            }
        }

        // If we emitted a separate `entry:` block for TCO, branch into bb0 now
        // so the param-spill happens exactly once.
        if has_back_edge_to_entry {
            self.emit_line(&format!("  br label %bb{}", first_block.id.0));
            self.emit_line(&format!("bb{}:", first_block.id.0));
        }

        // Emit the entry block's instructions and terminator (label already emitted).
        for inst in &first_block.instructions {
            self.emit_instruction(inst, func)?;
        }
        self.emit_terminator(&first_block.terminator, func)?;

        // Emit remaining blocks.
        for (i, block) in func.blocks.iter().enumerate() {
            if i == 0 {
                continue; // Already emitted above.
            }
            if i > 0 {
                self.emit_blank();
            }
            self.emit_block(block, func)?;
        }

        self.emit_line("}");
        self.emit_blank();

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Block emission
    // -----------------------------------------------------------------------

    fn emit_block(&mut self, block: &BasicBlock, func: &MirFunction) -> Result<(), CodegenError> {
        self.emit_line(&format!("bb{}:", block.id.0));

        for inst in &block.instructions {
            self.emit_instruction(inst, func)?;
        }

        self.emit_terminator(&block.terminator, func)?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Instruction emission
    // -----------------------------------------------------------------------

    fn emit_instruction(
        &mut self,
        inst: &Instruction,
        func: &MirFunction,
    ) -> Result<(), CodegenError> {
        match inst {
            Instruction::Assign { dest, value } => {
                self.emit_assign(*dest, value, func)?;
            }
            Instruction::ArcRetain { ptr } => {
                self.emit_line(&format!("  call void @kryos_arc_retain(ptr %_{})", ptr.0));
            }
            Instruction::ArcRelease { ptr } => {
                self.emit_line(&format!("  call void @kryos_arc_release(ptr %_{})", ptr.0));
            }
            Instruction::Drop { local } => {
                let local_ty = func
                    .locals
                    .iter()
                    .find(|l| l.id == *local)
                    .map(|l| l.ty.clone());
                let val = if self.mutable_locals.contains(&local.0) {
                    // Load using the local's actual LLVM type, not always `ptr`.
                    // Aggregate locals (enum/struct stored in alloca) must be
                    // loaded as the aggregate type so subsequent emit_*_drop
                    // calls operate on the correct shape.
                    let load_ty = self.local_type(*local);
                    let tmp = self.next_temp();
                    self.emit_line(&format!(
                        "  {tmp} = load {load_ty}, ptr %_{}.addr",
                        local.0
                    ));
                    tmp
                } else {
                    format!("%_{}", local.0)
                };
                match local_ty.as_ref() {
                    Some(MirType::Str) => {
                        self.emit_line(&format!("  call void @kryos_string_free(ptr {val})"));
                    }
                    Some(MirType::Array(ref elem_ty, _)) => {
                        let et = elem_ty.as_ref().clone();
                        self.emit_array_drop(&val, &et, func);
                    }
                    Some(MirType::Function { .. }) | Some(MirType::Shared(_)) => {
                        self.emit_line(&format!("  call void @kryos_arc_release(ptr {val})"));
                    }
                    Some(MirType::Map { .. }) => {
                        // Map uses i64 handle, coerce ptr to i64 for the call.
                        let i64_val = self.next_temp();
                        self.emit_line(&format!("  {i64_val} = ptrtoint ptr {val} to i64"));
                        self.emit_line(&format!("  call void @kryos_map_free(i64 {i64_val})"));
                    }
                    Some(MirType::Struct(name)) => {
                        // @copy structs are passed by value and share field pointers;
                        // do not free them here -- the original owner will free.
                        if !self.copy_structs.contains(name) {
                            let local_llvm = self.local_type(*local);
                            if local_llvm.starts_with('%') {
                                // SSA aggregate struct value — spill to alloca to
                                // give emit_struct_drop a ptr; do not free the alloca.
                                let agg = local_llvm;
                                let buf = self.next_temp();
                                self.emit_line(&format!("  {buf} = alloca {agg}"));
                                self.emit_line(&format!("  store {agg} {val}, ptr {buf}"));
                                self.emit_struct_drop(&buf, name, func);
                            } else {
                                self.emit_struct_drop(&val, name, func);
                                self.emit_line(&format!("  call void @free(ptr {val})"));
                            }
                        }
                    }
                    Some(MirType::Enum(name)) => {
                        // emit_enum_drop expects a ptr to enum data.
                        // - If the local is held as an SSA aggregate (or as
                        //   storage backing an alloca, i.e. mutable), the
                        //   buffer is stack-allocated — drop the payload but
                        //   do NOT free the buffer.
                        // - Otherwise the enum is heap-allocated via malloc
                        //   and the buffer itself must be freed.
                        let local_llvm = self.local_type(*local);
                        if local_llvm.starts_with('{') {
                            // Aggregate value. If this is a mutable local, the
                            // buffer is already %_N.addr (stack); otherwise spill.
                            if self.mutable_locals.contains(&local.0) {
                                let buf = format!("%_{}.addr", local.0);
                                self.emit_enum_drop_payload(&buf, name, func);
                            } else {
                                let max = self.enum_max_fields(name);
                                let agg = self.enum_llvm_type(name, max);
                                let buf = self.next_temp();
                                self.emit_line(&format!("  {buf} = alloca {agg}"));
                                self.emit_line(&format!("  store {agg} {val}, ptr {buf}"));
                                self.emit_enum_drop_payload(&buf, name, func);
                            }
                        } else {
                            // Heap-allocated enum via malloc — free after drop.
                            self.emit_enum_drop(&val, name, func);
                        }
                    }
                    _ => {
                        self.emit_line("  ; drop (no-op)");
                    }
                }
            }
            Instruction::StoreDeref { ptr, value } => {
                // Store through a reference/pointer.
                let ptr_val = self.operand_to_llvm(ptr, func);
                let ptr_ty = self.operand_type(ptr, func);
                let val = self.operand_to_llvm(value, func);
                let val_ty = self.operand_type(value, func);
                // Coerce to ptr if not already. Void-typed sources (e.g. ARC ptrs
                // whose MIR type was lost) are already raw ptrs at the LLVM level.
                let real_ptr = if ptr_ty == "ptr" || ptr_ty == "void" {
                    ptr_val
                } else {
                    let tmp = self.next_temp();
                    self.emit_line(&format!("  {tmp} = inttoptr {ptr_ty} {ptr_val} to ptr"));
                    tmp
                };
                let (store_val_ty, store_val) = if val_ty == "void" {
                    ("i64".to_string(), "0".to_string())
                } else {
                    (val_ty, val)
                };
                self.emit_line(&format!("  store {store_val_ty} {store_val}, ptr {real_ptr}"));
            }
            Instruction::Nop => {}
            Instruction::Spawn {
                func: spawn_fn,
                args,
            } => {
                // Get function pointer.
                let tmp_fptr = self.next_temp();
                self.emit_line(&format!("  {tmp_fptr} = ptrtoint ptr @{spawn_fn} to i64"));
                if args.is_empty() {
                    // kryos_spawn(fn_ptr, null, 0)
                    self.emit_line(&format!(
                        "  call i64 @kryos_spawn(i64 {tmp_fptr}, ptr null, i64 0)"
                    ));
                } else {
                    // Alloca for args array.
                    let arr = self.next_temp();
                    self.emit_line(&format!("  {arr} = alloca i64, i32 {}", args.len()));
                    for (i, arg) in args.iter().enumerate() {
                        let val = self.operand_to_llvm(arg, func);
                        let gep = self.next_temp();
                        self.emit_line(&format!("  {gep} = getelementptr i64, ptr {arr}, i32 {i}"));
                        // Clone heap-typed args for thread ownership.
                        let arg_ty = match arg {
                            Operand::Local(id) => func
                                .locals
                                .iter()
                                .find(|l| l.id == *id)
                                .map(|l| l.ty.clone()),
                            _ => None,
                        };
                        let store_val = match arg_ty.as_ref() {
                            Some(MirType::Str) => {
                                let cl = self.next_temp();
                                self.emit_line(&format!(
                                    "  {cl} = call ptr @kryos_string_clone(ptr {val})"
                                ));
                                cl
                            }
                            Some(MirType::Array(_, _)) => {
                                let cl = self.next_temp();
                                self.emit_line(&format!(
                                    "  {cl} = call ptr @kryos_array_clone(ptr {val})"
                                ));
                                cl
                            }
                            Some(MirType::Map { .. }) => {
                                let cl = self.next_temp();
                                self.emit_line(&format!(
                                    "  {cl} = call i64 @kryos_map_clone(i64 {val})"
                                ));
                                cl
                            }
                            Some(MirType::Function { .. }) | Some(MirType::Shared(_)) => {
                                self.emit_line(&format!(
                                    "  call void @kryos_arc_retain(ptr {val})"
                                ));
                                val
                            }
                            _ => val,
                        };
                        self.emit_line(&format!("  store i64 {store_val}, ptr {gep}"));
                    }
                    self.emit_line(&format!(
                        "  call i64 @kryos_spawn(i64 {tmp_fptr}, ptr {arr}, i64 {})",
                        args.len()
                    ));
                }
            }
            Instruction::Send { channel, value } => {
                let ch_op = Operand::Local(*channel);
                let val_op = Operand::Local(*value);
                let ch = self.operand_to_llvm(&ch_op, func);
                let val = self.operand_to_llvm(&val_op, func);

                // Clone heap-typed values before sending to prevent double-free.
                let val_ty = func
                    .locals
                    .iter()
                    .find(|l| l.id == *value)
                    .map(|l| l.ty.clone());

                let send_val = match val_ty.as_ref() {
                    Some(MirType::Str) => {
                        let cloned = self.next_temp();
                        self.emit_line(&format!(
                            "  {cloned} = call ptr @kryos_string_clone(ptr {val})"
                        ));
                        cloned
                    }
                    Some(MirType::Array(_, _)) => {
                        let cloned = self.next_temp();
                        self.emit_line(&format!(
                            "  {cloned} = call ptr @kryos_array_clone(ptr {val})"
                        ));
                        cloned
                    }
                    Some(MirType::Map { .. }) => {
                        let cloned = self.next_temp();
                        self.emit_line(&format!(
                            "  {cloned} = call i64 @kryos_map_clone(i64 {val})"
                        ));
                        cloned
                    }
                    Some(MirType::Function { .. }) | Some(MirType::Shared(_)) => {
                        self.emit_line(&format!("  call void @kryos_arc_retain(ptr {val})"));
                        val
                    }
                    _ => val,
                };

                self.emit_line(&format!(
                    "  call i64 @kryos_chan_send_i64(i64 {ch}, i64 {send_val})"
                ));
            }
            Instruction::Receive { dest, channel } => {
                let ch_op = Operand::Local(*channel);
                let ch = self.operand_to_llvm(&ch_op, func);
                let is_mutable = self.mutable_locals.contains(&dest.0);
                if is_mutable {
                    let tmp = self.next_temp();
                    self.emit_line(&format!(
                        "  {tmp} = call i64 @kryos_chan_recv_i64(i64 {ch})"
                    ));
                    self.emit_line(&format!("  store i64 {tmp}, ptr %_{}.addr", dest.0));
                } else {
                    self.emit_line(&format!(
                        "  %_{} = call i64 @kryos_chan_recv_i64(i64 {ch})",
                        dest.0
                    ));
                }
            }
            Instruction::ActorSpawn {
                dest,
                dispatch_fn,
                state,
            } => {
                // Get dispatch function pointer.
                let fptr = self.next_temp();
                self.emit_line(&format!("  {fptr} = ptrtoint ptr @{dispatch_fn} to i64"));
                let state_val = self.operand_to_llvm(state, func);
                let is_mutable = self.mutable_locals.contains(&dest.0);
                if is_mutable {
                    let tmp = self.next_temp();
                    self.emit_line(&format!(
                        "  {tmp} = call i64 @kryos_actor_spawn_i64(i64 {fptr}, i64 {state_val})"
                    ));
                    self.emit_line(&format!("  store i64 {tmp}, ptr %_{}.addr", dest.0));
                } else {
                    self.emit_line(&format!(
                        "  %_{} = call i64 @kryos_actor_spawn_i64(i64 {fptr}, i64 {state_val})",
                        dest.0
                    ));
                }
            }
            Instruction::ActorSend {
                actor,
                handler_tag,
                args,
            } => {
                let actor_op = Operand::Local(*actor);
                let actor_val = self.operand_to_llvm(&actor_op, func);
                // Lock to prevent message interleaving.
                self.emit_line(&format!(
                    "  call i64 @kryos_actor_lock_i64(i64 {actor_val})"
                ));
                // Send handler tag.
                self.emit_line(&format!(
                    "  call i64 @kryos_actor_send_i64(i64 {actor_val}, i64 {handler_tag})"
                ));
                // Send each argument (clone heap-typed values to prevent double-free).
                for arg in args {
                    let val = self.operand_to_llvm(arg, func);
                    let arg_ty = match arg {
                        Operand::Local(id) => func
                            .locals
                            .iter()
                            .find(|l| l.id == *id)
                            .map(|l| l.ty.clone()),
                        _ => None,
                    };
                    let send_val = match arg_ty.as_ref() {
                        Some(MirType::Str) => {
                            let cloned = self.next_temp();
                            self.emit_line(&format!(
                                "  {cloned} = call ptr @kryos_string_clone(ptr {val})"
                            ));
                            cloned
                        }
                        Some(MirType::Array(_, _)) => {
                            let cloned = self.next_temp();
                            self.emit_line(&format!(
                                "  {cloned} = call ptr @kryos_array_clone(ptr {val})"
                            ));
                            cloned
                        }
                        Some(MirType::Map { .. }) => {
                            let cloned = self.next_temp();
                            self.emit_line(&format!(
                                "  {cloned} = call i64 @kryos_map_clone(i64 {val})"
                            ));
                            cloned
                        }
                        Some(MirType::Function { .. }) | Some(MirType::Shared(_)) => {
                            self.emit_line(&format!("  call void @kryos_arc_retain(ptr {val})"));
                            val
                        }
                        _ => val,
                    };
                    self.emit_line(&format!(
                        "  call i64 @kryos_actor_send_i64(i64 {actor_val}, i64 {send_val})"
                    ));
                }
                // Unlock.
                self.emit_line(&format!(
                    "  call i64 @kryos_actor_unlock_i64(i64 {actor_val})"
                ));
            }
            Instruction::ActorStateLoad {
                dest,
                state_ptr,
                field_offset,
            } => {
                // Load from state_ptr + field_offset * 8.
                // Convert i64 state_ptr to a real pointer, GEP to field, then load.
                let ptr_local = self.operand_to_llvm(&Operand::Local(*state_ptr), func);
                let ptr_tmp = self.next_temp();
                self.emit_line(&format!("  {ptr_tmp} = inttoptr i64 {ptr_local} to ptr"));
                let field_ptr = self.next_temp();
                self.emit_line(&format!(
                    "  {field_ptr} = getelementptr i64, ptr {ptr_tmp}, i32 {field_offset}"
                ));
                let is_mutable = self.mutable_locals.contains(&dest.0);
                if is_mutable {
                    let tmp = self.next_temp();
                    self.emit_line(&format!("  {tmp} = load i64, ptr {field_ptr}"));
                    self.emit_line(&format!("  store i64 {tmp}, ptr %_{}.addr", dest.0));
                } else {
                    self.emit_line(&format!("  %_{} = load i64, ptr {field_ptr}", dest.0));
                }
            }
            Instruction::ActorStateStore {
                state_ptr,
                field_offset,
                value,
            } => {
                // Store value to state_ptr + field_offset * 8.
                let ptr_local = self.operand_to_llvm(&Operand::Local(*state_ptr), func);
                let val = self.operand_to_llvm(value, func);
                let ptr_tmp = self.next_temp();
                self.emit_line(&format!("  {ptr_tmp} = inttoptr i64 {ptr_local} to ptr"));
                let field_ptr = self.next_temp();
                self.emit_line(&format!(
                    "  {field_ptr} = getelementptr i64, ptr {ptr_tmp}, i32 {field_offset}"
                ));
                self.emit_line(&format!("  store i64 {val}, ptr {field_ptr}"));
            }
            Instruction::StoreField {
                object,
                field,
                value,
            } => {
                // Store a value into a struct field at its computed offset.
                // The object is a pointer to the struct; we GEP to the field
                // index and store the value.
                let obj_val = self.operand_to_llvm(object, func);
                let obj_ty = self.operand_type(object, func);
                let val = self.operand_to_llvm(value, func);
                let field_idx = self.resolve_field_index(object, field, func);

                // Coerce object to ptr if not already. Treat void-typed values
                // as already-ptr (they originate from runtime calls returning ptr).
                let ptr_tmp = if obj_ty == "ptr" || obj_ty == "void" {
                    obj_val
                } else {
                    let tmp = self.next_temp();
                    self.emit_line(&format!("  {tmp} = inttoptr {obj_ty} {obj_val} to ptr"));
                    tmp
                };
                let field_ptr = self.next_temp();
                self.emit_line(&format!(
                    "  {field_ptr} = getelementptr i64, ptr {ptr_tmp}, i32 {field_idx}"
                ));
                self.emit_line(&format!("  store i64 {val}, ptr {field_ptr}"));
            }
        }
        Ok(())
    }

    /// Emit a call to a user-defined function that uses byval/sret aggregate ABI.
    /// `ret_agg`: Some(llvm_ty) if return is aggregate (callee uses sret).
    /// `param_aggs[i]`: Some(llvm_ty) if arg i should be passed byval.
    #[allow(clippy::too_many_arguments)]
    fn emit_aggregate_call(
        &mut self,
        fname: &str,
        args: &[Operand],
        dest: LocalId,
        dest_ty: &str,
        is_mutable: bool,
        ret_agg: Option<String>,
        param_aggs: Vec<Option<String>>,
        func: &MirFunction,
    ) {
        let mut arg_parts: Vec<String> = Vec::new();

        // Allocate sret buffer if returning aggregate.
        let sret_buf = if let Some(ref agg) = ret_agg {
            let buf = self.next_temp();
            self.emit_line(&format!("  {buf} = alloca {agg}"));
            arg_parts.push(format!("ptr sret({agg}) {buf}"));
            Some((buf, agg.clone()))
        } else {
            None
        };

        // Emit each arg, allocating + storing byval aggregates.
        let callee_param_types = self.func_param_types.get(fname).cloned();
        for (i, a) in args.iter().enumerate() {
            let actual_ty = self.operand_type(a, func);
            let val = self.operand_to_llvm(a, func);
            let agg = param_aggs.get(i).cloned().flatten();
            if let Some(agg_ty) = agg {
                let buf = self.next_temp();
                self.emit_line(&format!("  {buf} = alloca {agg_ty}"));
                self.emit_line(&format!("  store {agg_ty} {val}, ptr {buf}"));
                arg_parts.push(format!("ptr byval({agg_ty}) {buf}"));
            } else {
                let expected_ty = callee_param_types
                    .as_ref()
                    .and_then(|pts| pts.get(i))
                    .cloned()
                    .unwrap_or_else(|| actual_ty.clone());
                let coerced = self.coerce_value(&val, &actual_ty, &expected_ty);
                arg_parts.push(format!("{expected_ty} {coerced}"));
            }
        }
        let arg_list = arg_parts.join(", ");

        if let Some((buf, agg)) = sret_buf {
            self.emit_line(&format!("  call void @{fname}({arg_list})"));
            // Load result into %_dest (or store into alloca for mutable dest).
            if is_mutable {
                let tmp = self.next_temp();
                self.emit_line(&format!("  {tmp} = load {agg}, ptr {buf}"));
                self.emit_line(&format!("  store {agg} {tmp}, ptr %_{}.addr", dest.0));
            } else {
                self.emit_line(&format!("  %_{} = load {agg}, ptr {buf}", dest.0));
            }
        } else if dest_ty == "void" {
            self.emit_line(&format!("  call void @{fname}({arg_list})"));
        } else if is_mutable {
            let tmp = self.next_temp();
            self.emit_line(&format!("  {tmp} = call {dest_ty} @{fname}({arg_list})"));
            self.emit_line(&format!("  store {dest_ty} {tmp}, ptr %_{}.addr", dest.0));
        } else {
            self.emit_line(&format!(
                "  %_{} = call {dest_ty} @{fname}({arg_list})",
                dest.0
            ));
        }
    }

    #[allow(clippy::if_same_then_else)]
    fn emit_assign(
        &mut self,
        dest: LocalId,
        value: &RValue,
        func: &MirFunction,
    ) -> Result<(), CodegenError> {
        let dest_ty = self.local_type(dest);
        let is_mutable = self.mutable_locals.contains(&dest.0);

        match value {
            // ----- Simple use / copy -----
            RValue::Use(op) => {
                let val = self.operand_to_llvm(op, func);
                let mut val_ty = self.operand_type(op, func);
                // If MIR reports void but we have a real SSA value, recover the
                // actual LLVM type from the value tracker. This avoids substituting
                // null/0 for legitimate pointer/integer values whose MIR type was
                // lost (e.g. ARC handles assigned to `let b = a`).
                if val_ty == "void" {
                    if let Some(real_ty) = self.actual_type(&val) {
                        if real_ty != "void" {
                            val_ty = real_ty;
                        }
                    }
                }
                // If the destination MIR type is void, fall back to the source's
                // LLVM type (or ptr) so we still produce a usable SSA value.
                let effective_dest_ty = if dest_ty == "void" {
                    if val_ty == "void" {
                        "ptr".to_string()
                    } else {
                        val_ty.clone()
                    }
                } else {
                    dest_ty.clone()
                };
                // Coerce value to the destination type if they differ.
                let coerced = self.coerce_value(&val, &val_ty, &effective_dest_ty);
                if is_mutable {
                    // For mutable locals: compute value, store to alloca.
                    let tmp = self.next_temp();
                    self.emit_identity_copy(&tmp, &effective_dest_ty, &coerced);
                    self.emit_line(&format!(
                        "  store {effective_dest_ty} {tmp}, ptr %_{}.addr",
                        dest.0
                    ));
                } else {
                    let name = format!("%_{}", dest.0);
                    self.emit_identity_copy(&name, &effective_dest_ty, &coerced);
                }
            }

            // ----- Binary ops -----
            RValue::BinOp { op, left, right } => {
                // String operations: dispatch to runtime instead of integer ops.
                let is_string =
                    Self::operand_is_string(left, func) || Self::operand_is_string(right, func);

                if is_string && *op == MirBinOp::Add {
                    let left_val = self.operand_to_llvm(left, func);
                    let left_ty = self.operand_type(left, func);
                    let right_val = self.operand_to_llvm(right, func);
                    let right_ty = self.operand_type(right, func);
                    // Coerce to ptr if operands are i64 (string handles).
                    let left_ptr = self.coerce_value(&left_val, &left_ty, "ptr");
                    let right_ptr = self.coerce_value(&right_val, &right_ty, "ptr");
                    if is_mutable {
                        let tmp = self.next_temp();
                        self.emit_line(&format!(
                            "  {tmp} = call ptr @kryos_string_concat(ptr {left_ptr}, ptr {right_ptr})"
                        ));
                        self.emit_line(&format!("  store ptr {tmp}, ptr %_{}.addr", dest.0));
                    } else {
                        self.emit_line(&format!(
                            "  %_{} = call ptr @kryos_string_concat(ptr {left_ptr}, ptr {right_ptr})",
                            dest.0
                        ));
                    }
                } else if is_string && (*op == MirBinOp::Eq || *op == MirBinOp::Neq) {
                    let left_val = self.operand_to_llvm(left, func);
                    let left_ty = self.operand_type(left, func);
                    let right_val = self.operand_to_llvm(right, func);
                    let right_ty = self.operand_type(right, func);
                    // Coerce to ptr if operands are i64 (string handles).
                    let left_ptr = self.coerce_value(&left_val, &left_ty, "ptr");
                    let right_ptr = self.coerce_value(&right_val, &right_ty, "ptr");
                    let eq_tmp = self.next_temp();
                    self.emit_line(&format!(
                        "  {eq_tmp} = call i1 @kryos_string_eq(ptr {left_ptr}, ptr {right_ptr})"
                    ));
                    if *op == MirBinOp::Neq {
                        let neq_tmp = if is_mutable {
                            self.next_temp()
                        } else {
                            format!("%_{}", dest.0)
                        };
                        self.emit_line(&format!("  {neq_tmp} = xor i1 {eq_tmp}, 1"));
                        if is_mutable {
                            self.emit_line(&format!("  store i1 {neq_tmp}, ptr %_{}.addr", dest.0));
                        }
                    } else if is_mutable {
                        self.emit_line(&format!("  store i1 {eq_tmp}, ptr %_{}.addr", dest.0));
                    } else {
                        // eq_tmp is a temp, need to assign to dest
                        self.emit_line(&format!("  %_{} = xor i1 {eq_tmp}, 0", dest.0));
                    }
                } else {
                    let mut left_val = self.operand_to_llvm(left, func);
                    let mut right_val = self.operand_to_llvm(right, func);
                    let is_float = self.operand_is_float(left, func);
                    let operand_ty = self.operand_type(left, func);
                    let right_ty = self.operand_type(right, func);
                    // Both operands must share LLVM type for `add`/`sub`/etc.
                    // The left operand defines the canonical type; coerce
                    // right to match. This catches the common case of mixing
                    // i64 locals with i32 function returns (e.g. score + bool_to_int(...)).
                    if right_ty != operand_ty {
                        right_val = self.coerce_value(&right_val, &right_ty, &operand_ty);
                    }
                    // If left is bool (i1) and we are doing an arithmetic op,
                    // widen both to i64 so the add/sub is well-typed.
                    if operand_ty == "i1" && matches!(op,
                        MirBinOp::Add | MirBinOp::Sub | MirBinOp::Mul | MirBinOp::Div |
                        MirBinOp::Mod | MirBinOp::BitAnd | MirBinOp::BitOr | MirBinOp::BitXor |
                        MirBinOp::Shl | MirBinOp::Shr
                    ) {
                        left_val = self.coerce_value(&left_val, "i1", "i64");
                        right_val = self.coerce_value(&right_val, "i1", "i64");
                        let operand_ty_w = "i64".to_string();
                        if is_mutable {
                            let tmp = self.next_temp();
                            self.emit_binop_to(&tmp, *op, &left_val, &right_val, &operand_ty_w, is_float)?;
                            self.emit_line(&format!("  store {dest_ty} {tmp}, ptr %_{}.addr", dest.0));
                        } else {
                            self.emit_binop(dest, *op, &left_val, &right_val, &operand_ty_w, is_float)?;
                        }
                    } else if is_mutable {
                        let tmp = self.next_temp();
                        self.emit_binop_to(
                            &tmp,
                            *op,
                            &left_val,
                            &right_val,
                            &operand_ty,
                            is_float,
                        )?;
                        self.emit_line(&format!("  store {dest_ty} {tmp}, ptr %_{}.addr", dest.0));
                    } else {
                        self.emit_binop(dest, *op, &left_val, &right_val, &operand_ty, is_float)?;
                    }
                }
            }

            // ----- Unary ops -----
            RValue::UnOp { op, operand } => {
                let val = self.operand_to_llvm(operand, func);
                let operand_ty = self.operand_type(operand, func);
                let is_float = self.operand_is_float(operand, func);

                if is_mutable {
                    let tmp = self.next_temp();
                    self.emit_unop_to(&tmp, *op, &val, &operand_ty, is_float)?;
                    self.emit_line(&format!("  store {dest_ty} {tmp}, ptr %_{}.addr", dest.0));
                } else {
                    self.emit_unop(dest, *op, &val, &operand_ty, is_float)?;
                }
            }

            // ----- Function call -----
            RValue::Call { func: fname, args } => {
                // Handle print/println/eprintln before building arg_list to
                // avoid double-evaluating operands (which would emit duplicate
                // kryos_string_new calls for string constants).
                if matches!(fname.as_str(), "println" | "print" | "eprintln") {
                    let print_fn = match fname.as_str() {
                        "println" => "kryos_println_str",
                        "print" => "kryos_print_str",
                        _ => "kryos_eprintln_str",
                    };
                    if args.is_empty() {
                        self.emit_line(&format!("  call void @{print_fn}(ptr null)"));
                    } else if Self::operand_is_string(&args[0], func) {
                        let val = self.operand_to_llvm(&args[0], func);
                        let val_ty = self.operand_type(&args[0], func);
                        let val_ptr = self.coerce_value(&val, &val_ty, "ptr");
                        self.emit_line(&format!("  call void @{print_fn}(ptr {val_ptr})"));
                    } else {
                        // Non-string arg: convert to string first, dispatching on type
                        // (bool/float/int produce different formats; previously all i64).
                        let val = self.operand_to_llvm(&args[0], func);
                        let val_ty = self.operand_type(&args[0], func);
                        let (runtime_fn, call_arg) = if val_ty == "double" || val_ty == "float" {
                            let coerced = if val_ty == "float" {
                                let tmp = self.next_temp();
                                self.emit_line(&format!(
                                    "  {tmp} = fpext float {val} to double"
                                ));
                                tmp
                            } else {
                                val.clone()
                            };
                            ("kryos_f64_to_string", format!("double {coerced}"))
                        } else if val_ty == "i1" {
                            let ext = self.next_temp();
                            self.emit_line(&format!("  {ext} = zext i1 {val} to i64"));
                            ("kryos_bool_to_string", format!("i64 {ext}"))
                        } else if val_ty == "ptr" {
                            let as_i64 = self.next_temp();
                            self.emit_line(&format!("  {as_i64} = ptrtoint ptr {val} to i64"));
                            ("kryos_builtin_to_string", format!("i64 {as_i64}"))
                        } else {
                            let coerced = self.coerce_value(&val, &val_ty, "i64");
                            ("kryos_builtin_to_string", format!("i64 {coerced}"))
                        };
                        let handle_i64 = self.next_temp();
                        self.emit_line(&format!(
                            "  {handle_i64} = call i64 @{runtime_fn}({call_arg})"
                        ));
                        let handle_ptr = self.next_temp();
                        self.emit_line(&format!(
                            "  {handle_ptr} = inttoptr i64 {handle_i64} to ptr"
                        ));
                        self.emit_line(&format!("  call void @{print_fn}(ptr {handle_ptr})"));
                    }
                } else {
                    // If the callee uses aggregate (byval/sret) ABI, emit specialized call.
                    if let Some((ret_agg, param_aggs)) =
                        self.func_sig_aggs.get(fname.as_str()).cloned()
                    {
                        if ret_agg.is_some() || param_aggs.iter().any(|p| p.is_some()) {
                            self.emit_aggregate_call(
                                fname, args, dest, &dest_ty, is_mutable, ret_agg, param_aggs, func,
                            );
                            return Ok(());
                        }
                    }

                    // Look up the callee's parameter types for type-correct emission.
                    // Fall back to known runtime function signatures when not in the user-defined table.
                    let callee_param_types = self.func_param_types.get(fname.as_str()).cloned()
                        .or_else(|| runtime_param_types(fname.as_str()));

                    let mut arg_parts = Vec::new();
                    for (i, a) in args.iter().enumerate() {
                        let actual_ty = self.operand_type(a, func);
                        let expected_ty = callee_param_types
                            .as_ref()
                            .and_then(|pts| pts.get(i))
                            .cloned()
                            .unwrap_or_else(|| actual_ty.clone());
                        let val = self.operand_to_llvm(a, func);
                        // Coerce value type to match callee's expected parameter type.
                        let coerced = self.coerce_value(&val, &actual_ty, &expected_ty);
                        arg_parts.push(format!("{expected_ty} {coerced}"));
                    }
                    let arg_list = arg_parts.join(", ");

                    match fname.as_str() {
                        "exit" => {
                            self.emit_line(&format!("  call void @exit({arg_list})"));
                        }
                        "len" => {
                            let arg = if !args.is_empty() {
                                let val = self.operand_to_llvm(&args[0], func);
                                let val_ty = self.operand_type(&args[0], func);
                                self.coerce_value(&val, &val_ty, "i64")
                            } else {
                                "0".to_string()
                            };
                            if is_mutable {
                                let tmp = self.next_temp();
                                self.emit_line(&format!(
                                    "  {tmp} = call i64 @kryos_builtin_len(i64 {arg})"
                                ));
                                self.emit_line(&format!(
                                    "  store i64 {tmp}, ptr %_{}.addr",
                                    dest.0
                                ));
                            } else {
                                self.emit_line(&format!(
                                    "  %_{} = call i64 @kryos_builtin_len(i64 {arg})",
                                    dest.0
                                ));
                            }
                        }
                        "to_string" => {
                            let val = if !args.is_empty() {
                                self.operand_to_llvm(&args[0], func)
                            } else {
                                "0".to_string()
                            };
                            let arg_ty = if !args.is_empty() {
                                self.operand_type(&args[0], func)
                            } else {
                                "i64".to_string()
                            };
                            // Choose the correct runtime function and coerce argument type.
                            let (runtime_fn, call_arg) = if arg_ty == "double" || arg_ty == "float"
                            {
                                // Float: use kryos_f64_to_string(double)
                                let coerced = if arg_ty == "float" {
                                    let tmp = self.next_temp();
                                    self.emit_line(&format!(
                                        "  {tmp} = fpext float {val} to double"
                                    ));
                                    tmp
                                } else {
                                    val
                                };
                                ("kryos_f64_to_string", format!("double {coerced}"))
                            } else if arg_ty == "i1" {
                                // Bool: use kryos_bool_to_string, but need to zext to i64 first.
                                let ext = self.next_temp();
                                self.emit_line(&format!("  {ext} = zext i1 {val} to i64"));
                                ("kryos_bool_to_string", format!("i64 {ext}"))
                            } else if arg_ty == "ptr" {
                                // Already a string handle -- ptrtoint to i64 for the runtime call.
                                let as_i64 = self.next_temp();
                                self.emit_line(&format!("  {as_i64} = ptrtoint ptr {val} to i64"));
                                ("kryos_builtin_to_string", format!("i64 {as_i64}"))
                            } else {
                                // Integer types: coerce to i64 if needed.
                                let coerced = self.coerce_value(&val, &arg_ty, "i64");
                                ("kryos_builtin_to_string", format!("i64 {coerced}"))
                            };
                            // The runtime returns i64 (a handle). If dest expects ptr, convert.
                            if dest_ty == "ptr" {
                                let handle = self.next_temp();
                                self.emit_line(&format!(
                                    "  {handle} = call i64 @{runtime_fn}({call_arg})"
                                ));
                                let as_ptr = self.next_temp();
                                self.emit_line(&format!(
                                    "  {as_ptr} = inttoptr i64 {handle} to ptr"
                                ));
                                if is_mutable {
                                    self.emit_line(&format!(
                                        "  store ptr {as_ptr}, ptr %_{}.addr",
                                        dest.0
                                    ));
                                } else {
                                    self.emit_line(&format!(
                                        "  %_{} = getelementptr i8, ptr {as_ptr}, i64 0",
                                        dest.0
                                    ));
                                }
                            } else if is_mutable {
                                let tmp = self.next_temp();
                                self.emit_line(&format!(
                                    "  {tmp} = call i64 @{runtime_fn}({call_arg})"
                                ));
                                self.emit_line(&format!(
                                    "  store i64 {tmp}, ptr %_{}.addr",
                                    dest.0
                                ));
                            } else {
                                self.emit_line(&format!(
                                    "  %_{} = call i64 @{runtime_fn}({call_arg})",
                                    dest.0
                                ));
                            }
                        }
                        "chan" => {
                            if is_mutable {
                                let tmp = self.next_temp();
                                self.emit_line(&format!(
                                    "  {tmp} = call i64 @kryos_chan_new_i64()"
                                ));
                                self.emit_line(&format!(
                                    "  store i64 {tmp}, ptr %_{}.addr",
                                    dest.0
                                ));
                            } else {
                                self.emit_line(&format!(
                                    "  %_{} = call i64 @kryos_chan_new_i64()",
                                    dest.0
                                ));
                            }
                        }
                        "kryos_sleep"
                        | "kryos_spawn_wait_all"
                        | "kryos_chan_close_i64"
                        | "kryos_chan_drop_i64" => {
                            self.emit_line(&format!("  call void @{fname}({arg_list})"));
                        }
                        "send" => {
                            let ch = if !args.is_empty() {
                                self.operand_to_llvm(&args[0], func)
                            } else {
                                "0".into()
                            };
                            let val = if args.len() > 1 {
                                self.operand_to_llvm(&args[1], func)
                            } else {
                                "0".into()
                            };
                            self.emit_line(&format!(
                                "  call i64 @kryos_chan_send_i64(i64 {ch}, i64 {val})"
                            ));
                        }
                        "recv" => {
                            let ch = if !args.is_empty() {
                                self.operand_to_llvm(&args[0], func)
                            } else {
                                "0".into()
                            };
                            if is_mutable {
                                let tmp = self.next_temp();
                                self.emit_line(&format!(
                                    "  {tmp} = call i64 @kryos_chan_recv_i64(i64 {ch})"
                                ));
                                self.emit_line(&format!(
                                    "  store i64 {tmp}, ptr %_{}.addr",
                                    dest.0
                                ));
                            } else {
                                self.emit_line(&format!(
                                    "  %_{} = call i64 @kryos_chan_recv_i64(i64 {ch})",
                                    dest.0
                                ));
                            }
                        }
                        "pop" => {
                            // pop(arr: [T]) -> T
                            // Runtime: kryos_builtin_pop(arr: i64) -> i64
                            let arr_val = if !args.is_empty() {
                                let v = self.operand_to_llvm(&args[0], func);
                                let ty = self.operand_type(&args[0], func);
                                self.coerce_value(&v, &ty, "i64")
                            } else {
                                "0".to_string()
                            };
                            if is_mutable {
                                let tmp = self.next_temp();
                                self.emit_line(&format!(
                                    "  {tmp} = call i64 @kryos_builtin_pop(i64 {arr_val})"
                                ));
                                self.emit_line(&format!(
                                    "  store i64 {tmp}, ptr %_{}.addr",
                                    dest.0
                                ));
                            } else {
                                self.emit_line(&format!(
                                    "  %_{} = call i64 @kryos_builtin_pop(i64 {arr_val})",
                                    dest.0
                                ));
                            }
                        }
                        "push" => {
                            // push(arr: [T], val: T) -> void
                            // Runtime: kryos_array_push(arr: ptr, val: i64) -> void
                            // arg0 stays ptr, arg1 (value, possibly ptr) -> i64 via ptrtoint.
                            let arr_val = if !args.is_empty() {
                                let v = self.operand_to_llvm(&args[0], func);
                                let ty = self.operand_type(&args[0], func);
                                self.coerce_value(&v, &ty, "ptr")
                            } else {
                                "null".to_string()
                            };
                            let elem_val = if args.len() >= 2 {
                                let v = self.operand_to_llvm(&args[1], func);
                                let ty = self.operand_type(&args[1], func);
                                if ty == "ptr" {
                                    let tmp = self.next_temp();
                                    self.emit_line(&format!("  {tmp} = ptrtoint ptr {v} to i64"));
                                    tmp
                                } else {
                                    self.coerce_value(&v, &ty, "i64")
                                }
                            } else {
                                "0".to_string()
                            };
                            self.emit_line(&format!(
                                "  call void @kryos_array_push(ptr {arr_val}, i64 {elem_val})"
                            ));
                        }
                        "assert" => {
                            // assert(condition: bool, message: str) -> void
                            // Runtime: kryos_builtin_assert(i64, i64) -> i64
                            // Coerce condition (i1) -> i64 via zext, message (ptr) -> i64 via ptrtoint.
                            let cond_val = if !args.is_empty() {
                                let v = self.operand_to_llvm(&args[0], func);
                                let ty = self.operand_type(&args[0], func);
                                if ty == "i1" {
                                    let tmp = self.next_temp();
                                    self.emit_line(&format!("  {tmp} = zext i1 {v} to i64"));
                                    tmp
                                } else {
                                    self.coerce_value(&v, &ty, "i64")
                                }
                            } else {
                                "1".to_string()
                            };
                            let msg_val = if args.len() >= 2 {
                                let v = self.operand_to_llvm(&args[1], func);
                                let ty = self.operand_type(&args[1], func);
                                if ty == "ptr" {
                                    let tmp = self.next_temp();
                                    self.emit_line(&format!("  {tmp} = ptrtoint ptr {v} to i64"));
                                    tmp
                                } else {
                                    self.coerce_value(&v, &ty, "i64")
                                }
                            } else {
                                "0".to_string()
                            };
                            // Discard the return value — assert is void in Kryos.
                            self.emit_line(&format!(
                                "  call i64 @kryos_builtin_assert(i64 {cond_val}, i64 {msg_val})"
                            ));
                        }
                        "abs" if args.len() == 1 && !self.func_param_types.contains_key("abs") => {
                            // Polymorphic abs: dispatch to llvm.fabs.f64 for floats,
                            // branchless integer abs for ints. Mirrors Cranelift behavior.
                            // Skipped when the user has defined their own `abs`.
                            let arg_ty = self.operand_type(&args[0], func);
                            let arg_val = self.operand_to_llvm(&args[0], func);
                            let is_f = self.operand_is_float(&args[0], func) || arg_ty == "double";
                            if is_f {
                                let v = self.coerce_value(&arg_val, &arg_ty, "double");
                                if is_mutable {
                                    let tmp = self.next_temp();
                                    self.emit_line(&format!(
                                        "  {tmp} = call double @llvm.fabs.f64(double {v})"
                                    ));
                                    self.emit_line(&format!("  store double {tmp}, ptr %_{}.addr", dest.0));
                                } else {
                                    self.emit_line(&format!(
                                        "  %_{} = call double @llvm.fabs.f64(double {v})",
                                        dest.0
                                    ));
                                }
                            } else {
                                let v = self.coerce_value(&arg_val, &arg_ty, "i64");
                                // Branchless: (v ^ (v >> 63)) - (v >> 63)
                                let sm = self.next_temp();
                                self.emit_line(&format!("  {sm} = ashr i64 {v}, 63"));
                                let xr = self.next_temp();
                                self.emit_line(&format!("  {xr} = xor i64 {v}, {sm}"));
                                if is_mutable {
                                    let tmp = self.next_temp();
                                    self.emit_line(&format!("  {tmp} = sub i64 {xr}, {sm}"));
                                    self.emit_line(&format!("  store i64 {tmp}, ptr %_{}.addr", dest.0));
                                } else {
                                    self.emit_line(&format!(
                                        "  %_{} = sub i64 {xr}, {sm}",
                                        dest.0
                                    ));
                                }
                            }
                        }
                        "min" | "max" if args.len() == 2 && !self.func_param_types.contains_key(fname.as_str()) => {
                            // Polymorphic min/max: dispatch by operand type.
                            // Skipped when the user has defined their own min/max.
                            let a_ty = self.operand_type(&args[0], func);
                            let a_val = self.operand_to_llvm(&args[0], func);
                            let b_val = self.operand_to_llvm(&args[1], func);
                            let b_ty = self.operand_type(&args[1], func);
                            let is_f = self.operand_is_float(&args[0], func) || a_ty == "double";
                            let is_min = fname.as_str() == "min";
                            if is_f {
                                let a = self.coerce_value(&a_val, &a_ty, "double");
                                let b = self.coerce_value(&b_val, &b_ty, "double");
                                let intrin = if is_min { "llvm.minnum.f64" } else { "llvm.maxnum.f64" };
                                if is_mutable {
                                    let tmp = self.next_temp();
                                    self.emit_line(&format!(
                                        "  {tmp} = call double @{intrin}(double {a}, double {b})"
                                    ));
                                    self.emit_line(&format!("  store double {tmp}, ptr %_{}.addr", dest.0));
                                } else {
                                    self.emit_line(&format!(
                                        "  %_{} = call double @{intrin}(double {a}, double {b})",
                                        dest.0
                                    ));
                                }
                            } else {
                                let a = self.coerce_value(&a_val, &a_ty, "i64");
                                let b = self.coerce_value(&b_val, &b_ty, "i64");
                                let cmp_op = if is_min { "slt" } else { "sgt" };
                                let cmp = self.next_temp();
                                self.emit_line(&format!("  {cmp} = icmp {cmp_op} i64 {a}, {b}"));
                                if is_mutable {
                                    let tmp = self.next_temp();
                                    self.emit_line(&format!("  {tmp} = select i1 {cmp}, i64 {a}, i64 {b}"));
                                    self.emit_line(&format!("  store i64 {tmp}, ptr %_{}.addr", dest.0));
                                } else {
                                    self.emit_line(&format!(
                                        "  %_{} = select i1 {cmp}, i64 {a}, i64 {b}",
                                        dest.0
                                    ));
                                }
                            }
                        }
                        _ => {
                            // Translate Kryos user-facing builtin names to runtime symbols.
                            let runtime_fname: &str = match fname.as_str() {
                                "trim" => "kryos_builtin_trim",
                                "trim_start" => "kryos_builtin_trim_start",
                                "trim_end" => "kryos_builtin_trim_end",
                                "to_upper" => "kryos_builtin_to_upper",
                                "to_lower" => "kryos_builtin_to_lower",
                                "index_of" => "kryos_builtin_index_of",
                                "contains" => "kryos_builtin_contains",
                                "starts_with" => "kryos_builtin_starts_with",
                                "ends_with" => "kryos_builtin_ends_with",
                                "replace" => "kryos_builtin_replace",
                                "split" => "kryos_builtin_split",
                                "join" => "kryos_builtin_join",
                                "sort" => "kryos_builtin_sort",
                                "reverse" => "kryos_builtin_reverse",
                                // Canonical names (match Cranelift + typechecker + runtime).
                                "file_read" => "kryos_builtin_file_read",
                                "file_write" => "kryos_builtin_file_write",
                                "file_append" => "kryos_builtin_file_append",
                                // Legacy aliases kept for source-compatibility; will
                                // be retired once stdlib examples are migrated.
                                "read_file" => "kryos_builtin_file_read",
                                "write_file" => "kryos_builtin_file_write",
                                "append_file" => "kryos_builtin_file_append",
                                "file_exists" => "kryos_builtin_file_exists",
                                "env_get" => "kryos_builtin_env_get",
                                "args" => "kryos_builtin_args",
                                "read_line" => "kryos_builtin_read_line",
                                "http_get" => "kryos_builtin_http_get",
                                "parse_int" => "kryos_builtin_parse_int",
                                "parse_float" => "kryos_builtin_parse_float",
                                "type_of" => "kryos_builtin_type_of",
                                "char_code" => "kryos_builtin_char_code",
                                "char_from" => "kryos_builtin_char_from",
                                "substr" => "kryos_builtin_substr",
                                "time_now" => "kryos_builtin_time_now",
                                // TCP
                                "tcp_connect" => "kryos_tcp_connect_ks",
                                "tcp_listen" => "kryos_tcp_bind_ks",
                                "tcp_accept" => "kryos_tcp_accept",
                                "tcp_send" => "kryos_tcp_send_ks",
                                "tcp_recv" => "kryos_tcp_recv_ks",
                                "tcp_close" => "kryos_socket_close_ks",
                                "tcp_set_nonblocking" => "kryos_tcp_set_nonblocking",
                                "tcp_try_accept" => "kryos_tcp_try_accept",
                                "tcp_try_recv" => "kryos_tcp_try_recv_ks",
                                // TLS server
                                "tls_server_config" => "kryos_tls_server_config_ks",
                                "tls_accept" => "kryos_tls_accept",
                                "tls_send" => "kryos_tls_send_ks",
                                "tls_recv" => "kryos_tls_recv_ks",
                                "tls_close" => "kryos_tls_close_ks",
                                // PostgreSQL
                                "pg_connect" => "kryos_pg_connect_ks",
                                "pg_exec" => "kryos_pg_exec_ks",
                                "pg_query" => "kryos_pg_query_ks",
                                "pg_close" => "kryos_pg_close_ks",
                                // Unix domain sockets (v2.0)
                                "uds_connect" => "kryos_uds_connect_ks",
                                "uds_bind" => "kryos_uds_bind_ks",
                                "uds_accept" => "kryos_uds_accept",
                                "uds_send" => "kryos_uds_send_ks",
                                "uds_recv" => "kryos_uds_recv_ks",
                                "uds_close" => "kryos_uds_close",
                                // WebSocket (RFC 6455) (v2.0)
                                "ws_accept_key" => "kryos_ws_accept_key_ks",
                                "ws_encode_text" => "kryos_ws_encode_text_ks",
                                "ws_encode_binary" => "kryos_ws_encode_binary_ks",
                                "ws_encode_close" => "kryos_ws_encode_close",
                                "ws_encode_ping" => "kryos_ws_encode_ping_ks",
                                "ws_encode_pong" => "kryos_ws_encode_pong_ks",
                                "ws_unmask" => "kryos_ws_unmask_ks",
                                "ws_read_frame" => "kryos_ws_read_frame_ks",
                                // JSON
                                "json_parse" => "kryos_json_parse",
                                "json_stringify" => "kryos_json_stringify",
                                "json_get" => "kryos_json_get",
                                "json_get_index" => "kryos_json_get_index",
                                "json_to_str" => "kryos_json_to_str",
                                "json_to_int" => "kryos_json_to_int",
                                "json_to_float" => "kryos_json_to_float",
                                "json_is_null" => "kryos_json_is_null",
                                "json_length" => "kryos_json_length",
                                "json_type" => "kryos_json_type",
                                "json_string" => "kryos_json_string",
                                "json_number" => "kryos_json_number",
                                "json_bool" => "kryos_json_bool",
                                "json_null" => "kryos_json_null",
                                "json_object" => "kryos_json_object",
                                "json_array" => "kryos_json_array",
                                // Crypto
                                "sha256" => "kryos_sha256_ks",
                                "sha512" => "kryos_sha512_ks",
                                "sha1_hex" => "kryos_sha1_hex_ks",
                                "sha1_base64" => "kryos_sha1_base64_ks",
                                "base64_encode" => "kryos_base64_encode_ks",
                                "base64_decode" => "kryos_base64_decode_ks",
                                "random_bytes" => "kryos_random_bytes_ks",
                                "chr" => "kryos_chr_ks",
                                "byte_at" => "kryos_byte_at_ks",
                                // Time / sleep
                                "time_now_secs" => "kryos_time_now_secs",
                                "time_now_millis" => "kryos_time_now_millis",
                                "sleep_ms" => "kryos_sleep_ms",
                                // Regex
                                "regex_new" => "kryos_regex_new_ks",
                                "regex_match" => "kryos_regex_is_match_ks",
                                // Mutex
                                "mutex_new" => "kryos_mutex_new",
                                "mutex_lock" => "kryos_mutex_lock",
                                "mutex_unlock" => "kryos_mutex_unlock",
                                "mutex_drop" => "kryos_mutex_drop",
                                other => other,
                            };
                            // Use callee's *actual* return type for the call instruction,
                            // then coerce into dest_ty if they differ. Without this the
                            // emitter would write e.g. `call ptr @add_one(i32 %x)` when
                            // add_one actually returns i32, causing a type mismatch when
                            // the result is later passed to another callee.
                            let actual_ret_ty = self
                                .func_ret_types
                                .get(runtime_fname)
                                .cloned()
                                .unwrap_or_else(|| dest_ty.to_string());
                            if dest_ty == "void" || actual_ret_ty == "void" {
                                self.emit_line(&format!(
                                    "  call void @{runtime_fname}({arg_list})"
                                ));
                            } else if actual_ret_ty == dest_ty {
                                if is_mutable {
                                    let tmp = self.next_temp();
                                    self.emit_line(&format!(
                                        "  {tmp} = call {dest_ty} @{runtime_fname}({arg_list})"
                                    ));
                                    self.emit_line(&format!(
                                        "  store {dest_ty} {tmp}, ptr %_{}.addr",
                                        dest.0
                                    ));
                                } else {
                                    self.emit_line(&format!(
                                        "  %_{} = call {dest_ty} @{runtime_fname}({arg_list})",
                                        dest.0
                                    ));
                                }
                            } else {
                                // Call with actual return type, then coerce to dest_ty.
                                let raw = self.next_temp();
                                self.emit_line(&format!(
                                    "  {raw} = call {actual_ret_ty} @{runtime_fname}({arg_list})"
                                ));
                                let coerced = self.coerce_value(&raw, &actual_ret_ty, dest_ty.as_str());
                                if is_mutable {
                                    self.emit_line(&format!(
                                        "  store {dest_ty} {coerced}, ptr %_{}.addr",
                                        dest.0
                                    ));
                                } else {
                                    // Need to materialize as %_N. Use an emit_identity_copy.
                                    self.emit_identity_copy(
                                        &format!("%_{}", dest.0),
                                        dest_ty.as_str(),
                                        &coerced,
                                    );
                                }
                            }
                        }
                    }
                } // close else (non-print call path)
            }

            RValue::CallIndirect { callee, args } => {
                // Indirect call via env-based calling convention.
                // The callee operand holds an env pointer:
                //   [thunk_fn_ptr: i64, cap0: i64, cap1: i64, ...]
                // We load the thunk pointer from env[0] and call it with
                // (env_ptr, user_arg0, user_arg1, ...).  The thunk unpacks
                // captures and forwards to the underlying function.
                //
                // When a bare function pointer (no captures) is passed -- the
                // env_size=0 case stores the function address directly in the
                // local -- we'd dereference an undefined pointer.  The MIR
                // lowering only takes the indirect path for fn-typed locals,
                // and the captures-less Closure RValue stores the bare
                // function address.  To keep both shapes working we still
                // emit `load i64, ptr env`, which for a bare function ptr
                // reads the first 8 bytes of the function's instruction
                // stream -- definitely wrong.  Bare function pointers as
                // indirect callees are not produced by the current MIR for
                // escaping closures, so this is safe for the closure-escape
                // case we are fixing here.
                let env_val = self.operand_to_llvm(callee, func);
                let callee_ty = self.operand_type(callee, func);

                let env_ptr = if callee_ty == "ptr" {
                    env_val
                } else {
                    let tmp = self.next_temp();
                    self.emit_line(&format!(
                        "  {tmp} = inttoptr {callee_ty} {env_val} to ptr"
                    ));
                    tmp
                };

                // Load thunk pointer from env[0].
                let thunk_i64 = self.next_temp();
                self.emit_line(&format!(
                    "  {thunk_i64} = load i64, ptr {env_ptr}"
                ));
                let thunk_ptr = self.next_temp();
                self.emit_line(&format!(
                    "  {thunk_ptr} = inttoptr i64 {thunk_i64} to ptr"
                ));

                // Build the argument list: env_ptr first, then user args
                // (all i64 in uniform slot model).
                let mut arg_parts = vec![format!("ptr {env_ptr}")];
                for a in args {
                    let val = self.operand_to_llvm(a, func);
                    let val_ty = self.operand_type(a, func);
                    let coerced = self.coerce_value(&val, &val_ty, "i64");
                    arg_parts.push(format!("i64 {coerced}"));
                }
                let arg_list = arg_parts.join(", ");

                // The thunk always returns i64; if our dest expects a
                // different LLVM type we coerce.
                if dest_ty == "void" {
                    let _r = self.next_temp();
                    self.emit_line(&format!(
                        "  {_r} = call i64 {thunk_ptr}({arg_list})"
                    ));
                } else if dest_ty == "i64" {
                    if is_mutable {
                        let tmp = self.next_temp();
                        self.emit_line(&format!(
                            "  {tmp} = call i64 {thunk_ptr}({arg_list})"
                        ));
                        self.emit_line(&format!(
                            "  store i64 {tmp}, ptr %_{}.addr", dest.0
                        ));
                    } else {
                        self.emit_line(&format!(
                            "  %_{} = call i64 {thunk_ptr}({arg_list})",
                            dest.0
                        ));
                    }
                } else {
                    let raw = self.next_temp();
                    self.emit_line(&format!(
                        "  {raw} = call i64 {thunk_ptr}({arg_list})"
                    ));
                    let coerced = self.coerce_value(&raw, "i64", &dest_ty);
                    if is_mutable {
                        self.emit_line(&format!(
                            "  store {dest_ty} {coerced}, ptr %_{}.addr", dest.0
                        ));
                    } else if dest_ty == "ptr" {
                        // `add ptr X, 0` is invalid LLVM; use a no-op GEP.
                        self.emit_line(&format!(
                            "  %_{} = getelementptr i8, ptr {coerced}, i64 0",
                            dest.0
                        ));
                    } else if dest_ty == "double" || dest_ty == "float" {
                        self.emit_line(&format!(
                            "  %_{} = fadd {dest_ty} {coerced}, 0.0",
                            dest.0
                        ));
                    } else {
                        self.emit_line(&format!(
                            "  %_{} = add {dest_ty} {coerced}, 0",
                            dest.0
                        ));
                    }
                }
            }

            // ----- Constants -----
            RValue::ConstInt(v) => {
                if dest_ty == "ptr" {
                    // Integer constant going into a ptr-typed local: inttoptr.
                    if is_mutable {
                        let tmp = self.next_temp();
                        self.emit_line(&format!("  {tmp} = inttoptr i64 {v} to ptr"));
                        self.emit_line(&format!("  store ptr {tmp}, ptr %_{}.addr", dest.0));
                    } else {
                        self.emit_line(&format!("  %_{} = inttoptr i64 {v} to ptr", dest.0));
                    }
                } else if is_mutable {
                    self.emit_line(&format!("  store {dest_ty} {v}, ptr %_{}.addr", dest.0));
                } else {
                    self.emit_line(&format!("  %_{} = add {dest_ty} {v}, 0", dest.0));
                }
            }
            RValue::ConstFloat(v) => {
                let hex = float_to_llvm_hex(*v);
                if is_mutable {
                    let tmp = self.next_temp();
                    self.emit_line(&format!("  {tmp} = fadd {dest_ty} {hex}, 0.0"));
                    self.emit_line(&format!("  store {dest_ty} {tmp}, ptr %_{}.addr", dest.0));
                } else {
                    self.emit_line(&format!("  %_{} = fadd {dest_ty} {hex}, 0.0", dest.0));
                }
            }
            RValue::ConstBool(b) => {
                let v: i32 = if *b { 1 } else { 0 };
                if is_mutable {
                    self.emit_line(&format!("  store i1 {v}, ptr %_{}.addr", dest.0));
                } else {
                    self.emit_line(&format!("  %_{} = add i1 {v}, 0", dest.0));
                }
            }
            RValue::ConstString(s) => {
                let global_name = self
                    .string_constants
                    .get(s)
                    .cloned()
                    .unwrap_or_else(|| self.intern_string(s));
                let byte_len = s.len();
                let arr_len = byte_len + 1;
                // Create a KryosString handle from the raw data section bytes.
                let gep_tmp = self.next_temp();
                self.emit_line(&format!(
                    "  {gep_tmp} = getelementptr [{arr_len} x i8], ptr {global_name}, i64 0, i64 0"
                ));
                if is_mutable {
                    let handle = self.next_temp();
                    self.emit_line(&format!(
                        "  {handle} = call ptr @kryos_string_new(ptr {gep_tmp}, i64 {byte_len})"
                    ));
                    self.emit_line(&format!("  store ptr {handle}, ptr %_{}.addr", dest.0));
                } else {
                    self.emit_line(&format!(
                        "  %_{} = call ptr @kryos_string_new(ptr {gep_tmp}, i64 {byte_len})",
                        dest.0
                    ));
                }
            }
            RValue::ConstNone => {
                if is_mutable {
                    let tmp = self.next_temp();
                    self.emit_line(&format!("  {tmp} = inttoptr i64 0 to ptr"));
                    self.emit_line(&format!("  store ptr {tmp}, ptr %_{}.addr", dest.0));
                } else {
                    self.emit_line(&format!("  %_{} = inttoptr i64 0 to ptr", dest.0));
                }
            }

            // ----- Aggregates -----
            RValue::Array(elems) => {
                self.emit_aggregate_array(dest, elems, &dest_ty, func, is_mutable)?;
            }
            RValue::Tuple(elems) => {
                self.emit_aggregate_tuple(dest, elems, &dest_ty, func, is_mutable)?;
            }
            RValue::Struct { name: _, fields } => {
                self.emit_aggregate_struct(dest, fields, &dest_ty, func, is_mutable)?;
            }

            // ----- Field / Index access -----
            RValue::Field { object, field } => {
                let obj_val = self.operand_to_llvm(object, func);
                let obj_ty = self.operand_type(object, func);

                // Resolve field index from struct definitions.
                let field_idx = self.resolve_field_index(object, field, func);
                let target_name = if is_mutable {
                    self.next_temp()
                } else {
                    format!("%_{}", dest.0)
                };
                self.emit_line(&format!(
                    "  {target_name} = extractvalue {obj_ty} {obj_val}, {field_idx} ; .{field}"
                ));
                if is_mutable {
                    self.emit_line(&format!(
                        "  store {dest_ty} {target_name}, ptr %_{}.addr",
                        dest.0
                    ));
                }
            }
            RValue::Index { object, index } => {
                let obj_val = self.operand_to_llvm(object, func);
                let obj_ty = self.operand_type(object, func);
                let idx_val = self.operand_to_llvm(index, func);
                let idx_ty = self.operand_type(index, func);

                if obj_ty == "ptr" {
                    // Dynamic KryosArray — use kryos_array_get(ptr, i64) -> i64.
                    // The element is stored as i64; convert back to ptr if dest is ptr.
                    let idx_i64 = if idx_ty != "i64" {
                        let t = self.next_temp();
                        self.emit_line(&format!("  {t} = sext {idx_ty} {idx_val} to i64"));
                        t
                    } else {
                        idx_val
                    };
                    let raw = self.next_temp();
                    self.emit_line(&format!(
                        "  {raw} = call i64 @kryos_array_get(ptr {obj_val}, i64 {idx_i64})"
                    ));
                    // Convert i64 -> dest_ty, naming the result %_N for non-mutable.
                    let is_aggregate = dest_ty.starts_with('{')
                        || dest_ty.starts_with('[')
                        || dest_ty.starts_with('%');
                    if is_mutable {
                        let coerced = if dest_ty == "ptr" {
                            let t = self.next_temp();
                            self.emit_line(&format!("  {t} = inttoptr i64 {raw} to ptr"));
                            t
                        } else if dest_ty == "double" {
                            let t = self.next_temp();
                            self.emit_line(&format!("  {t} = bitcast i64 {raw} to double"));
                            t
                        } else if is_aggregate {
                            // Aggregate stored as ptr-as-i64: inttoptr, then load.
                            let p = self.next_temp();
                            self.emit_line(&format!("  {p} = inttoptr i64 {raw} to ptr"));
                            let v = self.next_temp();
                            self.emit_line(&format!("  {v} = load {dest_ty}, ptr {p}"));
                            v
                        } else {
                            raw
                        };
                        self.emit_line(&format!(
                            "  store {dest_ty} {coerced}, ptr %_{}.addr",
                            dest.0
                        ));
                    } else if dest_ty == "ptr" {
                        self.emit_line(&format!("  %_{} = inttoptr i64 {raw} to ptr", dest.0));
                    } else if dest_ty == "double" {
                        // Float array element: stored as i64 bits, must bitcast back to double.
                        self.emit_line(&format!("  %_{} = bitcast i64 {raw} to double", dest.0));
                    } else if is_aggregate {
                        let p = self.next_temp();
                        self.emit_line(&format!("  {p} = inttoptr i64 {raw} to ptr"));
                        self.emit_line(&format!(
                            "  %_{} = load {dest_ty}, ptr {p}",
                            dest.0
                        ));
                    } else {
                        // Identity: use add 0 for integer types.
                        self.emit_line(&format!("  %_{} = add {dest_ty} {raw}, 0", dest.0));
                    }
                } else {
                    // Fixed-size array or tuple aggregate — direct GEP + load.
                    // Coerce obj to ptr if it is carried as an i64 handle in the
                    // local type map (was the cause of "defined with type 'i64'
                    // but expected 'ptr'" in for-loop iteration over fixed-size
                    // arrays).
                    let obj_ptr = if obj_ty == "ptr" {
                        obj_val.clone()
                    } else {
                        self.coerce_value(&obj_val, &obj_ty, "ptr")
                    };
                    let elem_ptr = self.next_temp();
                    self.emit_line(&format!(
                        "  {elem_ptr} = getelementptr i64, ptr {obj_ptr}, {idx_ty} {idx_val}"
                    ));
                    let target_name = if is_mutable {
                        self.next_temp()
                    } else {
                        format!("%_{}", dest.0)
                    };
                    self.emit_line(&format!("  {target_name} = load {dest_ty}, ptr {elem_ptr}"));
                    if is_mutable {
                        self.emit_line(&format!(
                            "  store {dest_ty} {target_name}, ptr %_{}.addr",
                            dest.0
                        ));
                    }
                }
            }

            // ----- ARC alloc -----
            RValue::ArcAlloc { inner } => {
                // Matches the Cranelift backend semantics: allocate 8 bytes of
                // ARC-managed memory and store the inner value at offset 0.
                let inner_val = self.operand_to_llvm(inner, func);
                let inner_ty = self.operand_type(inner, func);

                // Allocate via kryos_arc_alloc(size, align) -> ptr.
                let target_name = if is_mutable {
                    self.next_temp()
                } else {
                    format!("%_{}", dest.0)
                };
                self.emit_line(&format!(
                    "  {target_name} = call ptr @kryos_arc_alloc(i64 8, i64 8)"
                ));
                self.track_type(&target_name, "ptr");

                // Store the inner value at offset 0.
                // - i64/i32/i8/bool: store directly as i64
                // - double: bitcast to i64 then store
                // - ptr: store as ptr
                // - void/unknown: store i64 0 (best-effort)
                let (store_ty, store_val) = match inner_ty.as_str() {
                    "i64" => ("i64".to_string(), inner_val.clone()),
                    "i32" => {
                        let t = self.next_temp();
                        self.emit_line(&format!(
                            "  {t} = sext i32 {inner_val} to i64"
                        ));
                        ("i64".to_string(), t)
                    }
                    "i8" => {
                        let t = self.next_temp();
                        self.emit_line(&format!(
                            "  {t} = sext i8 {inner_val} to i64"
                        ));
                        ("i64".to_string(), t)
                    }
                    "i1" => {
                        let t = self.next_temp();
                        self.emit_line(&format!(
                            "  {t} = zext i1 {inner_val} to i64"
                        ));
                        ("i64".to_string(), t)
                    }
                    "double" => {
                        let t = self.next_temp();
                        self.emit_line(&format!(
                            "  {t} = bitcast double {inner_val} to i64"
                        ));
                        ("i64".to_string(), t)
                    }
                    "ptr" => ("ptr".to_string(), inner_val.clone()),
                    "void" => ("i64".to_string(), "0".to_string()),
                    other => (other.to_string(), inner_val.clone()),
                };
                self.emit_line(&format!(
                    "  store {store_ty} {store_val}, ptr {target_name}"
                ));

                if is_mutable {
                    self.emit_line(&format!("  store ptr {target_name}, ptr %_{}.addr", dest.0));
                }
            }

            // ----- Enums -----
            RValue::EnumVariant {
                enum_name,
                variant_idx,
                fields,
            } => {
                let max_fields = self.enum_max_fields(enum_name);
                let llvm_ty = self.enum_llvm_type(enum_name, max_fields);

                if fields.is_empty() {
                    // Unit variant: just the tag.
                    let target = if is_mutable {
                        self.next_temp()
                    } else {
                        format!("%_{}", dest.0)
                    };
                    self.emit_line(&format!(
                        "  {target} = insertvalue {llvm_ty} undef, i64 {variant_idx}, 0"
                    ));
                    if is_mutable {
                        self.emit_line(&format!(
                            "  store {llvm_ty} {target}, ptr %_{}.addr",
                            dest.0
                        ));
                    }
                } else {
                    // Tag + fields via chained insertvalue.
                    let tag_tmp = self.next_temp();
                    self.emit_line(&format!(
                        "  {tag_tmp} = insertvalue {llvm_ty} undef, i64 {variant_idx}, 0"
                    ));
                    let mut current = tag_tmp;

                    for (i, field_op) in fields.iter().enumerate() {
                        let mut val = self.operand_to_llvm(field_op, func);
                        let val_ty = self.operand_type(field_op, func);
                        // Payload slot is i64; cast non-i64 values (e.g. ptr) first.
                        // void-typed operands (rare — result of a void-returning
                        // call cached into a local before the throw/catch
                        // rewrite) get replaced with a literal 0 to keep the
                        // emitted IR well-typed.
                        if val_ty == "void" {
                            val = "0".to_string();
                        } else if val_ty != "i64" {
                            let casted = self.next_temp();
                            let op = if val_ty == "ptr" {
                                "ptrtoint"
                            } else {
                                "bitcast"
                            };
                            self.emit_line(&format!("  {casted} = {op} {val_ty} {val} to i64"));
                            val = casted;
                        }
                        let is_last = i + 1 == fields.len();
                        let target = if is_last && !is_mutable {
                            format!("%_{}", dest.0)
                        } else {
                            self.next_temp()
                        };
                        self.emit_line(&format!(
                            "  {target} = insertvalue {llvm_ty} {current}, i64 {val}, {idx}",
                            idx = i + 1
                        ));
                        current = target;
                    }

                    if is_mutable {
                        self.emit_line(&format!(
                            "  store {llvm_ty} {current}, ptr %_{}.addr",
                            dest.0
                        ));
                    }
                }
            }
            RValue::EnumTag { operand } => {
                let val = self.operand_to_llvm(operand, func);
                let obj_ty = self.operand_type(operand, func);
                let target_name = if is_mutable {
                    self.next_temp()
                } else {
                    format!("%_{}", dest.0)
                };
                self.emit_line(&format!("  {target_name} = extractvalue {obj_ty} {val}, 0"));
                if is_mutable {
                    self.emit_line(&format!("  store i64 {target_name}, ptr %_{}.addr", dest.0));
                }
            }
            RValue::EnumPayload {
                operand, field_idx, ..
            } => {
                let val = self.operand_to_llvm(operand, func);
                let obj_ty = self.operand_type(operand, func);
                // Payload slot is always i64 in the enum aggregate; cast to
                // the dest's actual LLVM type if needed (e.g. ptr for arrays).
                let needs_cast = dest_ty != "i64" && dest_ty != "void";
                let slot_tmp = if needs_cast {
                    self.next_temp()
                } else if is_mutable {
                    self.next_temp()
                } else {
                    format!("%_{}", dest.0)
                };
                self.emit_line(&format!(
                    "  {slot_tmp} = extractvalue {obj_ty} {val}, {idx}",
                    idx = field_idx + 1
                ));
                let final_name = if needs_cast {
                    let t = if is_mutable {
                        self.next_temp()
                    } else {
                        format!("%_{}", dest.0)
                    };
                    let op = if dest_ty == "ptr" {
                        "inttoptr"
                    } else {
                        "bitcast"
                    };
                    if op == "inttoptr" {
                        self.emit_line(&format!("  {t} = inttoptr i64 {slot_tmp} to {dest_ty}"));
                    } else {
                        self.emit_line(&format!("  {t} = bitcast i64 {slot_tmp} to {dest_ty}"));
                    }
                    t
                } else {
                    slot_tmp.clone()
                };
                if is_mutable {
                    self.emit_line(&format!(
                        "  store {dest_ty} {final_name}, ptr %_{}.addr",
                        dest.0
                    ));
                }
            }

            // ----- Cast -----
            RValue::Cast { operand, ty } => {
                self.emit_cast(dest, operand, ty, func, is_mutable)?;
            }

            RValue::Closure {
                func_name,
                captures,
            } => {
                // Closure: uniform env-thunk calling convention for ALL function values.
                // Env layout: [thunk_fn_ptr: i64, cap0: i64, cap1: i64, ...]
                // CallIndirect always loads fn from env[0] and calls thunk(env, user_args...).
                {
                    // Allocate closure env via ARC: [thunk_fn_ptr: i64, cap0: i64, cap1: i64, ...]
                    // Uniform calling convention regardless of capture count.
                    let env_size = (1 + captures.len()) * 8;
                    let env_i64 = self.next_temp();
                    self.emit_line(&format!(
                        "  {env_i64} = call i64 @kryos_arc_alloc_i64(i64 {env_size})"
                    ));
                    let env_ptr = self.next_temp();
                    self.emit_line(&format!("  {env_ptr} = inttoptr i64 {env_i64} to ptr"));
                    // Store thunk pointer at offset 0.
                    let fptr = self.next_temp();
                    self.emit_line(&format!(
                        "  {fptr} = ptrtoint ptr @{func_name}_env to i64"
                    ));
                    self.emit_line(&format!("  store i64 {fptr}, ptr {env_ptr}"));
                    // Store each capture at offset (i+1)*8.
                    // Clone/retain heap-typed captures so the closure owns them
                    // independently of the original local's lifetime.
                    for (i, cap) in captures.iter().enumerate() {
                        let cap_val = self.operand_to_llvm(cap, func);
                        let cap_ptr = self.next_temp();
                        self.emit_line(&format!(
                            "  {cap_ptr} = getelementptr i64, ptr {env_ptr}, i64 {}",
                            i + 1
                        ));

                        let cap_mir_ty = match cap {
                            Operand::Local(id) => func
                                .locals
                                .iter()
                                .find(|l| l.id == *id)
                                .map(|l| l.ty.clone()),
                            _ => None,
                        };

                        match cap_mir_ty.as_ref() {
                            Some(MirType::Str) => {
                                let cloned = self.next_temp();
                                self.emit_line(&format!(
                                    "  {cloned} = call ptr @kryos_string_clone(ptr {cap_val})"
                                ));
                                self.emit_line(&format!("  store ptr {cloned}, ptr {cap_ptr}"));
                            }
                            Some(MirType::Array(_, _)) => {
                                let cloned = self.next_temp();
                                self.emit_line(&format!(
                                    "  {cloned} = call ptr @kryos_array_clone(ptr {cap_val})"
                                ));
                                self.emit_line(&format!("  store ptr {cloned}, ptr {cap_ptr}"));
                            }
                            Some(MirType::Map { .. }) => {
                                let cloned = self.next_temp();
                                self.emit_line(&format!(
                                    "  {cloned} = call i64 @kryos_map_clone(i64 {cap_val})"
                                ));
                                self.emit_line(&format!("  store i64 {cloned}, ptr {cap_ptr}"));
                            }
                            Some(MirType::Function { .. }) | Some(MirType::Shared(_)) => {
                                self.emit_line(&format!(
                                    "  call void @kryos_arc_retain(ptr {cap_val})"
                                ));
                                // cap_val is a ptr -- store as ptr to avoid type mismatch
                                self.emit_line(&format!("  store ptr {cap_val}, ptr {cap_ptr}"));
                            }
                            _ => {
                                // If cap_val is a ptr, coerce to i64; otherwise store as-is.
                                let actual = self
                                    .actual_type(&cap_val)
                                    .unwrap_or_else(|| "i64".to_string());
                                if actual == "i64" {
                                    self.emit_line(&format!(
                                        "  store i64 {cap_val}, ptr {cap_ptr}"
                                    ));
                                } else {
                                    let coerced = self.coerce_value(&cap_val, &actual, "i64");
                                    self.emit_line(&format!(
                                        "  store i64 {coerced}, ptr {cap_ptr}"
                                    ));
                                }
                            }
                        }
                    }

                    // Register dropper so captured heap values are freed when
                    // the closure's ARC ref count reaches zero.
                    let dropper_name = format!("{func_name}_drop");
                    let has_dropper = self
                        .closure_cap_types
                        .get(func_name.as_str())
                        .map(|cts| {
                            cts.iter().any(|ct| {
                                matches!(
                                    ct,
                                    Some(MirType::Str)
                                        | Some(MirType::Array(_, _))
                                        | Some(MirType::Function { .. })
                                        | Some(MirType::Shared(_))
                                        | Some(MirType::Struct(_))
                                        | Some(MirType::Enum(_))
                                        | Some(MirType::Map { .. })
                                )
                            })
                        })
                        .unwrap_or(false);
                    if has_dropper {
                        self.emit_line(&format!(
                            "  call void @kryos_arc_set_drop(ptr {env_ptr}, ptr @{dropper_name})"
                        ));
                    }

                    if dest_ty == "ptr" {
                        // Dest expects ptr -- use env_ptr directly.
                        if is_mutable {
                            self.emit_line(&format!(
                                "  store ptr {env_ptr}, ptr %_{}.addr",
                                dest.0
                            ));
                        } else {
                            self.emit_line(&format!(
                                "  %_{} = getelementptr i8, ptr {env_ptr}, i64 0",
                                dest.0
                            ));
                        }
                    } else {
                        let env_int = self.next_temp();
                        self.emit_line(&format!("  {env_int} = ptrtoint ptr {env_ptr} to i64"));
                        if is_mutable {
                            self.emit_line(&format!(
                                "  store i64 {env_int}, ptr %_{}.addr",
                                dest.0
                            ));
                        } else {
                            self.emit_line(&format!("  %_{} = add i64 {env_int}, 0", dest.0));
                        }
                    }
                }
            }

            RValue::Map(entries) => {
                // Create map via runtime, then insert each key-value pair.
                let map_handle = self.next_temp();
                self.emit_line(&format!("  {map_handle} = call i64 @kryos_map_new()"));
                for (k, v) in entries {
                    let key_val = self.operand_to_llvm(k, func);
                    let key_ty = self.operand_type(k, func);
                    let val_val = self.operand_to_llvm(v, func);
                    let val_ty = self.operand_type(v, func);
                    let key_i64 = self.coerce_value(&key_val, &key_ty, "i64");
                    let val_i64 = self.coerce_value(&val_val, &val_ty, "i64");
                    // Use string-aware insert for string keys (content hashing).
                    let is_string_key = Self::operand_is_string(k, func);
                    let insert_fn = if is_string_key {
                        "kryos_map_insert_str"
                    } else {
                        "kryos_map_insert"
                    };
                    self.emit_line(&format!(
                        "  call void @{insert_fn}(i64 {map_handle}, i64 {key_i64}, i64 {val_i64})"
                    ));
                }
                if is_mutable {
                    self.emit_line(&format!("  store i64 {map_handle}, ptr %_{}.addr", dest.0));
                } else {
                    let name = format!("%_{}", dest.0);
                    self.emit_identity_copy(&name, "i64", &map_handle);
                }
            }

            RValue::Range {
                start,
                end,
                inclusive,
            } => {
                // Range layout: { i64 start, i64 end, i64 inclusive } — alloca 3 x i64.
                let range_ptr = self.next_temp();
                self.emit_line(&format!("  {range_ptr} = alloca [3 x i64]"));
                // Store start.
                let start_val = match start {
                    Some(op) => self.operand_to_llvm(op, func),
                    None => "0".to_string(),
                };
                let start_ptr = self.next_temp();
                self.emit_line(&format!(
                    "  {start_ptr} = getelementptr i64, ptr {range_ptr}, i64 0"
                ));
                self.emit_line(&format!("  store i64 {start_val}, ptr {start_ptr}"));
                // Store end.
                let end_val = match end {
                    Some(op) => self.operand_to_llvm(op, func),
                    None => format!("{}", i64::MAX),
                };
                let end_ptr = self.next_temp();
                self.emit_line(&format!(
                    "  {end_ptr} = getelementptr i64, ptr {range_ptr}, i64 1"
                ));
                self.emit_line(&format!("  store i64 {end_val}, ptr {end_ptr}"));
                // Store inclusive flag.
                let incl_ptr = self.next_temp();
                self.emit_line(&format!(
                    "  {incl_ptr} = getelementptr i64, ptr {range_ptr}, i64 2"
                ));
                self.emit_line(&format!(
                    "  store i64 {}, ptr {incl_ptr}",
                    *inclusive as i64
                ));
                // Assign pointer to dest.
                if dest_ty == "ptr" {
                    if is_mutable {
                        self.emit_line(&format!("  store ptr {range_ptr}, ptr %_{}.addr", dest.0));
                    } else {
                        self.emit_line(&format!(
                            "  %_{} = getelementptr i8, ptr {range_ptr}, i64 0",
                            dest.0
                        ));
                    }
                } else {
                    let ptr_val = self.next_temp();
                    self.emit_line(&format!("  {ptr_val} = ptrtoint ptr {range_ptr} to i64"));
                    if is_mutable {
                        self.emit_line(&format!("  store i64 {ptr_val}, ptr %_{}.addr", dest.0));
                    } else {
                        self.emit_line(&format!("  %_{} = add i64 {ptr_val}, 0", dest.0));
                    }
                }
            }

            RValue::AddrOf { local, mutable: _ } => {
                // Take the address of a local.
                // If the local is mutable (has an alloca), use its alloca address.
                // Otherwise, create a temp alloca, store the value, return address.
                if self.mutable_locals.contains(&local.0) {
                    // The alloca already exists as %_N.addr — return its pointer.
                    if dest_ty == "ptr" {
                        if is_mutable {
                            self.emit_line(&format!(
                                "  store ptr %_{}.addr, ptr %_{}.addr",
                                local.0, dest.0
                            ));
                        } else {
                            self.emit_line(&format!(
                                "  %_{} = getelementptr i8, ptr %_{}.addr, i64 0",
                                dest.0, local.0
                            ));
                        }
                    } else {
                        let addr_tmp = self.next_temp();
                        self.emit_line(&format!(
                            "  {addr_tmp} = ptrtoint ptr %_{}.addr to i64",
                            local.0
                        ));
                        if is_mutable {
                            self.emit_line(&format!(
                                "  store i64 {addr_tmp}, ptr %_{}.addr",
                                dest.0
                            ));
                        } else {
                            self.emit_line(&format!("  %_{} = add i64 {addr_tmp}, 0", dest.0));
                        }
                    }
                } else {
                    // Create a temporary alloca for the value.
                    let local_ty = self
                        .local_types
                        .get(&local.0)
                        .cloned()
                        .unwrap_or_else(|| "i64".to_string());
                    let alloca_tmp = self.next_temp();
                    self.emit_line(&format!("  {alloca_tmp} = alloca {local_ty}"));
                    self.emit_line(&format!(
                        "  store {local_ty} %_{}, ptr {alloca_tmp}",
                        local.0
                    ));
                    if dest_ty == "ptr" {
                        if is_mutable {
                            self.emit_line(&format!(
                                "  store ptr {alloca_tmp}, ptr %_{}.addr",
                                dest.0
                            ));
                        } else {
                            self.emit_line(&format!(
                                "  %_{} = getelementptr i8, ptr {alloca_tmp}, i64 0",
                                dest.0
                            ));
                        }
                    } else {
                        let addr_tmp = self.next_temp();
                        self.emit_line(&format!("  {addr_tmp} = ptrtoint ptr {alloca_tmp} to i64"));
                        if is_mutable {
                            self.emit_line(&format!(
                                "  store i64 {addr_tmp}, ptr %_{}.addr",
                                dest.0
                            ));
                        } else {
                            self.emit_line(&format!("  %_{} = add i64 {addr_tmp}, 0", dest.0));
                        }
                    }
                }
            }

            RValue::Deref { operand } => {
                // Load from a reference/pointer.
                let ptr_val = self.operand_to_llvm(operand, func);
                let ptr_ty = self.operand_type(operand, func);
                // Coerce to ptr if not already. If the MIR type is unknown/void,
                // the operand value (e.g. from kryos_arc_alloc) is already a ptr,
                // so use it directly rather than emitting `inttoptr void ...`.
                let real_ptr = if ptr_ty == "ptr" || ptr_ty == "void" {
                    ptr_val
                } else {
                    let tmp = self.next_temp();
                    self.emit_line(&format!("  {tmp} = inttoptr {ptr_ty} {ptr_val} to ptr"));
                    tmp
                };
                let load_tmp = self.next_temp();
                self.emit_line(&format!("  {load_tmp} = load {dest_ty}, ptr {real_ptr}"));
                if is_mutable {
                    self.emit_line(&format!(
                        "  store {dest_ty} {load_tmp}, ptr %_{}.addr",
                        dest.0
                    ));
                } else {
                    let name = format!("%_{}", dest.0);
                    self.emit_identity_copy(&name, &dest_ty, &load_tmp);
                }
            }

            RValue::Comptime(inner) => {
                // Comptime: lower the inner RValue directly (const-eval at MIR level).
                self.emit_assign(dest, inner, func)?;
            }

            RValue::MakeTraitObject { value, .. } => {
                // LLVM release mode: create fat pointer. For now, pass data through directly.
                // Full vtable codegen for LLVM deferred to Ring 3.
                let data_val = self.operand_to_llvm(value, func);
                let val_ty = self.operand_type(value, func);
                let coerced = self.coerce_value(&data_val, &val_ty, &dest_ty);
                if is_mutable {
                    self.emit_line(&format!(
                        "  store {dest_ty} {coerced}, ptr %_{}.addr",
                        dest.0
                    ));
                } else {
                    let name = format!("%_{}", dest.0);
                    self.emit_identity_copy(&name, &dest_ty, &coerced);
                }
            }

            RValue::VtableCall {
                object,
                method_index: _,
                args,
                ..
            } => {
                // LLVM release mode: for now, emit a placeholder.
                // Full vtable dispatch for LLVM deferred to Ring 3.
                let obj_val = self.operand_to_llvm(object, func);
                let _ = obj_val;
                for arg in args {
                    let _ = self.operand_to_llvm(arg, func);
                }
                let zero = default_value_for_type(&dest_ty);
                if is_mutable {
                    self.emit_line(&format!("  store {dest_ty} {zero}, ptr %_{}.addr", dest.0));
                } else {
                    let name = format!("%_{}", dest.0);
                    self.emit_identity_copy(&name, &dest_ty, zero);
                }
            }

            RValue::StringConcat(parts) => {
                // Chain kryos_string_concat calls: fold left across all parts.
                if parts.is_empty() {
                    if is_mutable {
                        self.emit_line(&format!("  store ptr null, ptr %_{}.addr", dest.0));
                    } else {
                        self.emit_line(&format!("  %_{} = inttoptr i64 0 to ptr", dest.0));
                    }
                } else if parts.len() == 1 {
                    let val = self.operand_to_llvm(&parts[0], func);
                    if is_mutable {
                        self.emit_line(&format!("  store ptr {val}, ptr %_{}.addr", dest.0));
                    } else {
                        // Copy the pointer value to the dest.
                        self.emit_line(&format!(
                            "  %_{} = getelementptr i8, ptr {val}, i64 0",
                            dest.0
                        ));
                    }
                } else {
                    // Fold: acc = concat(parts[0], parts[1]), acc = concat(acc, parts[2]), ...
                    let first = self.operand_to_llvm(&parts[0], func);
                    let second = self.operand_to_llvm(&parts[1], func);
                    let mut acc = self.next_temp();
                    self.emit_line(&format!(
                        "  {acc} = call ptr @kryos_string_concat(ptr {first}, ptr {second})"
                    ));
                    for part in &parts[2..] {
                        let next_val = self.operand_to_llvm(part, func);
                        let next_acc = self.next_temp();
                        self.emit_line(&format!(
                            "  {next_acc} = call ptr @kryos_string_concat(ptr {acc}, ptr {next_val})"
                        ));
                        // Free the intermediate concat result that was just replaced.
                        self.emit_line(&format!("  call void @kryos_string_free(ptr {acc})"));
                        acc = next_acc;
                    }
                    if is_mutable {
                        self.emit_line(&format!("  store ptr {acc}, ptr %_{}.addr", dest.0));
                    } else {
                        self.emit_line(&format!(
                            "  %_{} = getelementptr i8, ptr {acc}, i64 0",
                            dest.0
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Binary operations
    // -----------------------------------------------------------------------

    /// Resolve the field index for a struct field access. Returns the 0-based
    /// index of `field` within the struct type of `object`.
    ///
    /// Falls back to index 0 with a warning if the struct or field cannot be
    /// resolved — this indicates a gap in the type checker or MIR lowering that
    /// should be investigated.
    fn resolve_field_index(&self, object: &Operand, field: &str, func: &MirFunction) -> usize {
        // Numeric field names are tuple element indices (from tuple destructuring).
        if let Ok(idx) = field.parse::<usize>() {
            return idx;
        }

        // Determine the struct type name from the operand.
        let struct_name = match object {
            Operand::Local(id) => {
                func.locals
                    .iter()
                    .find(|l| l.id == *id)
                    .and_then(|l| match &l.ty {
                        MirType::Struct(name) => Some(name.clone()),
                        _ => None,
                    })
            }
            _ => None,
        };

        if let Some(ref name) = struct_name {
            if let Some(fields) = self.struct_defs.get(name) {
                for (i, (fname, _)) in fields.iter().enumerate() {
                    if fname == field {
                        return i;
                    }
                }
                // Field not found in known struct — emit a warning.
                eprintln!(
                    "warning: LLVM codegen: field `{}` not found in struct `{}` (known fields: {:?}) — defaulting to index 0",
                    field,
                    name,
                    fields.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
                );
            } else {
                // Struct definition not registered — emit a warning.
                eprintln!(
                    "warning: LLVM codegen: struct `{}` not found in struct_defs — defaulting to index 0 for field `{}`",
                    name, field
                );
            }
        } else {
            // Could not determine struct type from operand — emit a warning.
            eprintln!(
                "warning: LLVM codegen: could not determine struct type for field access `.{}` — defaulting to index 0",
                field
            );
        }
        0 // Fallback — should not be reached in well-typed programs.
    }

    fn emit_binop(
        &mut self,
        dest: LocalId,
        op: MirBinOp,
        left: &str,
        right: &str,
        ty: &str,
        is_float: bool,
    ) -> Result<(), CodegenError> {
        let name = format!("%_{}", dest.0);
        self.emit_binop_to(&name, op, left, right, ty, is_float)
    }

    /// Emit a binary op to a named target (used for both direct SSA and mutable temp names).
    fn emit_binop_to(
        &mut self,
        target: &str,
        op: MirBinOp,
        left: &str,
        right: &str,
        ty: &str,
        is_float: bool,
    ) -> Result<(), CodegenError> {
        let line = match op {
            MirBinOp::Add if is_float => format!("  {target} = fadd {ty} {left}, {right}"),
            MirBinOp::Add => format!("  {target} = add {ty} {left}, {right}"),
            MirBinOp::Sub if is_float => format!("  {target} = fsub {ty} {left}, {right}"),
            MirBinOp::Sub => format!("  {target} = sub {ty} {left}, {right}"),
            MirBinOp::Mul if is_float => format!("  {target} = fmul {ty} {left}, {right}"),
            MirBinOp::Mul => format!("  {target} = mul {ty} {left}, {right}"),
            MirBinOp::Div if is_float => format!("  {target} = fdiv {ty} {left}, {right}"),
            MirBinOp::Div => format!("  {target} = sdiv {ty} {left}, {right}"),
            MirBinOp::Mod if is_float => format!("  {target} = frem {ty} {left}, {right}"),
            MirBinOp::Mod => format!("  {target} = srem {ty} {left}, {right}"),
            MirBinOp::Pow if is_float => {
                format!("  {target} = call {ty} @llvm.pow.f64({ty} {left}, {ty} {right})")
            }
            MirBinOp::Pow => {
                format!("  {target} = call {ty} @kryos_ipow({ty} {left}, {ty} {right})")
            }
            MirBinOp::Eq if is_float => format!("  {target} = fcmp oeq {ty} {left}, {right}"),
            MirBinOp::Eq => format!("  {target} = icmp eq {ty} {left}, {right}"),
            MirBinOp::Neq if is_float => format!("  {target} = fcmp one {ty} {left}, {right}"),
            MirBinOp::Neq => format!("  {target} = icmp ne {ty} {left}, {right}"),
            MirBinOp::Lt if is_float => format!("  {target} = fcmp olt {ty} {left}, {right}"),
            MirBinOp::Lt => format!("  {target} = icmp slt {ty} {left}, {right}"),
            MirBinOp::Gt if is_float => format!("  {target} = fcmp ogt {ty} {left}, {right}"),
            MirBinOp::Gt => format!("  {target} = icmp sgt {ty} {left}, {right}"),
            MirBinOp::LtEq if is_float => format!("  {target} = fcmp ole {ty} {left}, {right}"),
            MirBinOp::LtEq => format!("  {target} = icmp sle {ty} {left}, {right}"),
            MirBinOp::GtEq if is_float => format!("  {target} = fcmp oge {ty} {left}, {right}"),
            MirBinOp::GtEq => format!("  {target} = icmp sge {ty} {left}, {right}"),
            MirBinOp::And | MirBinOp::BitAnd => format!("  {target} = and {ty} {left}, {right}"),
            MirBinOp::Or | MirBinOp::BitOr => format!("  {target} = or {ty} {left}, {right}"),
            MirBinOp::BitXor => format!("  {target} = xor {ty} {left}, {right}"),
            MirBinOp::Shl => format!("  {target} = shl {ty} {left}, {right}"),
            MirBinOp::Shr => format!("  {target} = ashr {ty} {left}, {right}"),
        };

        self.emit_line(&line);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Unary operations
    // -----------------------------------------------------------------------

    fn emit_unop(
        &mut self,
        dest: LocalId,
        op: MirUnOp,
        val: &str,
        ty: &str,
        is_float: bool,
    ) -> Result<(), CodegenError> {
        let name = format!("%_{}", dest.0);
        self.emit_unop_to(&name, op, val, ty, is_float)
    }

    fn emit_unop_to(
        &mut self,
        target: &str,
        op: MirUnOp,
        val: &str,
        ty: &str,
        is_float: bool,
    ) -> Result<(), CodegenError> {
        let line = match op {
            MirUnOp::Neg if is_float => format!("  {target} = fneg {ty} {val}"),
            MirUnOp::Neg => format!("  {target} = sub {ty} 0, {val}"),
            MirUnOp::Not => format!("  {target} = xor {ty} {val}, 1"),
            MirUnOp::BitNot => format!("  {target} = xor {ty} {val}, -1"),
        };

        self.emit_line(&line);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Aggregates
    // -----------------------------------------------------------------------

    fn emit_aggregate_array(
        &mut self,
        dest: LocalId,
        elems: &[Operand],
        dest_ty: &str,
        func: &MirFunction,
        is_mutable: bool,
    ) -> Result<(), CodegenError> {
        // Heap arrays (dest_ty == "ptr"): allocate via kryos_array_new + push each elem.
        if dest_ty == "ptr" && !elems.is_empty() {
            let arr_tmp = self.next_temp();
            self.emit_line(&format!(
                "  {arr_tmp} = call ptr @kryos_array_new(i64 8, i64 {})",
                elems.len()
            ));
            for elem in elems {
                let elem_val = self.operand_to_llvm(elem, func);
                let elem_ty = self.operand_type(elem, func);
                let as_i64 = if elem_ty == "i64" {
                    elem_val
                } else if elem_ty == "ptr" {
                    let t = self.next_temp();
                    self.emit_line(&format!("  {t} = ptrtoint ptr {elem_val} to i64"));
                    t
                } else if elem_ty == "double" {
                    let t = self.next_temp();
                    self.emit_line(&format!("  {t} = bitcast double {elem_val} to i64"));
                    t
                } else if elem_ty.starts_with('{') || elem_ty.starts_with('[') || elem_ty.starts_with('%') {
                    // Aggregate (struct/array/named): heap-allocate enough bytes,
                    // store the value, and pass the pointer as i64.
                    // Use the LLVM `getelementptr null, i32 1` size-of trick to
                    // compute the aggregate's size at the IR level.
                    let size_ptr = self.next_temp();
                    self.emit_line(&format!(
                        "  {size_ptr} = getelementptr {elem_ty}, ptr null, i32 1"
                    ));
                    let size_i64 = self.next_temp();
                    self.emit_line(&format!(
                        "  {size_i64} = ptrtoint ptr {size_ptr} to i64"
                    ));
                    let buf = self.next_temp();
                    self.emit_line(&format!(
                        "  {buf} = call ptr @kryos_arc_alloc(i64 {size_i64}, i64 8)"
                    ));
                    self.emit_line(&format!(
                        "  store {elem_ty} {elem_val}, ptr {buf}"
                    ));
                    let t = self.next_temp();
                    self.emit_line(&format!("  {t} = ptrtoint ptr {buf} to i64"));
                    t
                } else if elem_ty == "i1" || elem_ty == "i8" || elem_ty == "i16" || elem_ty == "i32" {
                    let t = self.next_temp();
                    self.emit_line(&format!("  {t} = sext {elem_ty} {elem_val} to i64"));
                    t
                } else {
                    // Unknown type: emit identity and let the verifier flag it.
                    let t = self.next_temp();
                    self.emit_line(&format!("  {t} = bitcast {elem_ty} {elem_val} to i64"));
                    t
                };
                self.emit_line(&format!(
                    "  call void @kryos_array_push(ptr {arr_tmp}, i64 {as_i64})"
                ));
            }
            if is_mutable {
                self.emit_line(&format!("  store ptr {arr_tmp}, ptr %_{}.addr", dest.0));
            } else {
                self.emit_line(&format!(
                    "  %_{} = getelementptr i8, ptr {arr_tmp}, i64 0",
                    dest.0
                ));
            }
            return Ok(());
        }

        // Build up with insertvalue (fixed-size local aggregate).
        for (i, elem) in elems.iter().enumerate() {
            let elem_val = self.operand_to_llvm(elem, func);
            let elem_ty = self.operand_type(elem, func);
            let prev = if i == 0 {
                "undef".to_string()
            } else {
                format!("%_{}_arr_{}", dest.0, i - 1)
            };
            let this = if i + 1 == elems.len() {
                if is_mutable {
                    self.next_temp()
                } else {
                    format!("%_{}", dest.0)
                }
            } else {
                format!("%_{}_arr_{}", dest.0, i)
            };
            self.emit_line(&format!(
                "  {this} = insertvalue {dest_ty} {prev}, {elem_ty} {elem_val}, {i}"
            ));
            if i + 1 == elems.len() && is_mutable {
                self.emit_line(&format!("  store {dest_ty} {this}, ptr %_{}.addr", dest.0));
            }
        }

        if elems.is_empty() {
            if dest_ty == "ptr" {
                // Dynamic (unsized) array — allocate an empty array via kryos_array_new.
                // elem_size=8 (all Kryos values are i64/ptr sized), initial cap=0 (runtime min=4).
                let arr_tmp = self.next_temp();
                self.emit_line(&format!(
                    "  {arr_tmp} = call ptr @kryos_array_new(i64 8, i64 0)"
                ));
                if is_mutable {
                    self.emit_line(&format!("  store ptr {arr_tmp}, ptr %_{}.addr", dest.0));
                } else {
                    self.emit_line(&format!(
                        "  %_{} = getelementptr i8, ptr {arr_tmp}, i64 0",
                        dest.0
                    ));
                }
            } else if is_mutable {
                let tmp = self.next_temp();
                self.emit_line(&format!("  {tmp} = insertvalue {dest_ty} undef, i8 0, 0"));
                self.emit_line(&format!("  store {dest_ty} {tmp}, ptr %_{}.addr", dest.0));
            } else {
                // Empty fixed-size array — just produce undef.
                self.emit_line(&format!(
                    "  %_{} = insertvalue {dest_ty} undef, i8 0, 0",
                    dest.0
                ));
            }
        }

        Ok(())
    }

    fn emit_aggregate_tuple(
        &mut self,
        dest: LocalId,
        elems: &[Operand],
        dest_ty: &str,
        func: &MirFunction,
        is_mutable: bool,
    ) -> Result<(), CodegenError> {
        // Same approach as arrays — insertvalue into a struct type.
        // When the destination is mutable, the final value is stored to its alloca.
        for (i, elem) in elems.iter().enumerate() {
            let elem_val = self.operand_to_llvm(elem, func);
            let elem_ty = self.operand_type(elem, func);
            let prev = if i == 0 {
                "undef".to_string()
            } else {
                format!("%_{}_tup_{}", dest.0, i - 1)
            };
            let this = if i + 1 == elems.len() {
                if is_mutable {
                    self.next_temp()
                } else {
                    format!("%_{}", dest.0)
                }
            } else {
                format!("%_{}_tup_{}", dest.0, i)
            };
            self.emit_line(&format!(
                "  {this} = insertvalue {dest_ty} {prev}, {elem_ty} {elem_val}, {i}"
            ));
            if i + 1 == elems.len() && is_mutable {
                self.emit_line(&format!("  store {dest_ty} {this}, ptr %_{}.addr", dest.0));
            }
        }

        if elems.is_empty() {
            if is_mutable {
                let tmp = self.next_temp();
                self.emit_line(&format!("  {tmp} = insertvalue {dest_ty} undef, i8 0, 0"));
                self.emit_line(&format!("  store {dest_ty} {tmp}, ptr %_{}.addr", dest.0));
            } else {
                self.emit_line(&format!(
                    "  %_{} = insertvalue {dest_ty} undef, i8 0, 0",
                    dest.0
                ));
            }
        }

        Ok(())
    }

    fn emit_aggregate_struct(
        &mut self,
        dest: LocalId,
        fields: &[(String, Operand)],
        dest_ty: &str,
        func: &MirFunction,
        is_mutable: bool,
    ) -> Result<(), CodegenError> {
        // Structs are lowered identically to tuples in LLVM IR (insertvalue by index).
        // When the destination is mutable, the final value is stored to its alloca
        // via a temporary so subsequent loads from %_X.addr see the correct value.
        //
        // Look up the declared struct field types so we can coerce the
        // initializer value to match (e.g. literal `10: i64` into an `i32` field).
        let struct_name = dest_ty.strip_prefix('%').unwrap_or(dest_ty);
        let declared_field_tys: Vec<String> = self
            .struct_defs
            .get(struct_name)
            .map(|fs| fs.iter().map(|(_, t)| mir_type_to_llvm(t)).collect())
            .unwrap_or_default();
        for (i, (field_name, op)) in fields.iter().enumerate() {
            let val = self.operand_to_llvm(op, func);
            let actual_ty = self.operand_type(op, func);
            let expected_ty = declared_field_tys
                .get(i)
                .cloned()
                .unwrap_or_else(|| actual_ty.clone());
            let coerced_val = self.coerce_value(&val, &actual_ty, &expected_ty);
            let prev = if i == 0 {
                "undef".to_string()
            } else {
                format!("%_{}_fld_{}", dest.0, i - 1)
            };
            let this = if i + 1 == fields.len() {
                if is_mutable {
                    // Use a temp name; we will store it to the alloca below.
                    self.next_temp()
                } else {
                    format!("%_{}", dest.0)
                }
            } else {
                format!("%_{}_fld_{}", dest.0, i)
            };
            self.emit_line(&format!(
                "  {this} = insertvalue {dest_ty} {prev}, {expected_ty} {coerced_val}, {i} ; .{field_name}"
            ));
            // If this was the last field and the local is mutable, store to alloca.
            if i + 1 == fields.len() && is_mutable {
                self.emit_line(&format!("  store {dest_ty} {this}, ptr %_{}.addr", dest.0));
            }
        }

        if fields.is_empty() {
            if is_mutable {
                let tmp = self.next_temp();
                self.emit_line(&format!("  {tmp} = insertvalue {dest_ty} undef, i8 0, 0"));
                self.emit_line(&format!("  store {dest_ty} {tmp}, ptr %_{}.addr", dest.0));
            } else {
                self.emit_line(&format!(
                    "  %_{} = insertvalue {dest_ty} undef, i8 0, 0",
                    dest.0
                ));
            }
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Cast
    // -----------------------------------------------------------------------

    fn emit_cast(
        &mut self,
        dest: LocalId,
        operand: &Operand,
        target_ty: &MirType,
        func: &MirFunction,
        is_mutable: bool,
    ) -> Result<(), CodegenError> {
        let src_val = self.operand_to_llvm(operand, func);
        let src_ty = self.operand_type(operand, func);
        let dst_ty = mir_type_to_llvm(target_ty);

        if src_ty == dst_ty {
            if is_mutable {
                let tmp = self.next_temp();
                self.emit_identity_copy(&tmp, &dst_ty, &src_val);
                self.emit_line(&format!("  store {dst_ty} {tmp}, ptr %_{}.addr", dest.0));
            } else {
                let name = format!("%_{}", dest.0);
                self.emit_identity_copy(&name, &dst_ty, &src_val);
            }
            return Ok(());
        }

        let src_is_float = is_float_type(&src_ty);
        let dst_is_float = is_float_type(&dst_ty);
        let src_is_ptr = src_ty == "ptr";
        let dst_is_ptr = dst_ty == "ptr";

        let inst = if src_is_float && dst_is_float {
            // float -> float: fpext or fptrunc.
            if llvm_type_width(&dst_ty) > llvm_type_width(&src_ty) {
                "fpext"
            } else {
                "fptrunc"
            }
        } else if src_is_float && !dst_is_float {
            "fptosi"
        } else if !src_is_float && dst_is_float {
            "sitofp"
        } else if src_is_ptr && !dst_is_ptr {
            "ptrtoint"
        } else if !src_is_ptr && dst_is_ptr {
            "inttoptr"
        } else {
            // int -> int: sext or trunc.
            if llvm_type_width(&dst_ty) > llvm_type_width(&src_ty) {
                "sext"
            } else if llvm_type_width(&dst_ty) < llvm_type_width(&src_ty) {
                "trunc"
            } else {
                "bitcast"
            }
        };

        if is_mutable {
            let tmp = self.next_temp();
            self.emit_line(&format!("  {tmp} = {inst} {src_ty} {src_val} to {dst_ty}"));
            self.emit_line(&format!("  store {dst_ty} {tmp}, ptr %_{}.addr", dest.0));
        } else {
            self.emit_line(&format!(
                "  %_{} = {inst} {src_ty} {src_val} to {dst_ty}",
                dest.0
            ));
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Terminator emission
    // -----------------------------------------------------------------------

    fn emit_terminator(
        &mut self,
        term: &Terminator,
        func: &MirFunction,
    ) -> Result<(), CodegenError> {
        match term {
            Terminator::Return(None) => {
                if self.aggregate_llvm_ty(&func.ret_ty).is_some() {
                    // sret return — nothing to store, just exit.
                    self.emit_line("  ret void");
                } else {
                    let ret_ty = self.sig_ty_to_llvm(&func.ret_ty);
                    if ret_ty == "void" {
                        self.emit_line("  ret void");
                    } else {
                        let zero = default_value_for_type(&ret_ty);
                        self.emit_line(&format!("  ret {ret_ty} {zero}"));
                    }
                }
            }
            Terminator::Return(Some(op)) => {
                if let Some(agg) = self.aggregate_llvm_ty(&func.ret_ty) {
                    let val = self.operand_to_llvm(op, func);
                    self.emit_line(&format!("  store {agg} {val}, ptr %_sret"));
                    self.emit_line("  ret void");
                } else {
                    let from_ty = self.operand_type(op, func);
                    let mut val = self.operand_to_llvm(op, func);
                    // Use sig_ty_to_llvm so enum returns get the correct
                    // aggregate type (`{ i64, i64 }`) rather than the bare
                    // tag (`i64`).
                    let want_ty = self.sig_ty_to_llvm(&func.ret_ty);
                    // Coerce when the operand's LLVM type differs from the
                    // function's declared return type.
                    if from_ty != want_ty {
                        // If the destination is an aggregate but the source
                        // is a scalar, we need a different strategy. For
                        // enums, the source is usually already the aggregate
                        // SSA value — trust the operand and bypass coerce.
                        if (want_ty.starts_with('{') || want_ty.starts_with('%'))
                            && !(from_ty.starts_with('{') || from_ty.starts_with('%'))
                        {
                            // Scalar → aggregate: synthesize the aggregate by
                            // inserting the scalar into the tag slot. Best-effort.
                            let agg_tmp = self.next_temp();
                            self.emit_line(&format!(
                                "  {agg_tmp} = insertvalue {want_ty} undef, {from_ty} {val}, 0"
                            ));
                            self.emit_line(&format!("  ret {want_ty} {agg_tmp}"));
                            return Ok(());
                        }
                        val = self.coerce_value(&val, &from_ty, &want_ty);
                    }
                    self.emit_line(&format!("  ret {want_ty} {val}"));
                }
            }
            Terminator::Goto(target) => {
                self.emit_line(&format!("  br label %bb{}", target.0));
            }
            Terminator::Branch {
                cond,
                then_block,
                else_block,
            } => {
                let from_ty = self.operand_type(cond, func);
                let mut cond_val = self.operand_to_llvm(cond, func);
                if from_ty != "i1" {
                    cond_val = self.coerce_value(&cond_val, &from_ty, "i1");
                }
                self.emit_line(&format!(
                    "  br i1 {cond_val}, label %bb{}, label %bb{}",
                    then_block.0, else_block.0
                ));
            }
            Terminator::Switch {
                value,
                targets,
                default,
            } => {
                let ty = self.operand_type(value, func);
                let val = self.operand_to_llvm(value, func);
                // Coerce subject to i64 if needed — LLVM `switch` requires
                // all case constants to share the subject's integer type, and
                // MIR lowering emits case constants sized for i64.
                // For enum aggregates ({ i64, i64 } etc.), extract the tag field.
                let (switch_ty, switch_val) = if ty.starts_with('{') || ty.starts_with('%') {
                    let tag = self.next_temp();
                    self.emit_line(&format!(
                        "  {tag} = extractvalue {ty} {val}, 0"
                    ));
                    ("i64".to_string(), tag)
                } else if ty != "i64" {
                    let coerced = self.coerce_value(&val, &ty, "i64");
                    ("i64".to_string(), coerced)
                } else {
                    (ty, val)
                };
                let cases = targets
                    .iter()
                    .map(|(v, b)| format!("    {switch_ty} {v}, label %bb{}", b.0))
                    .collect::<Vec<_>>()
                    .join("\n");
                // Use the MIR-supplied default block as the LLVM switch default.
                // This is where wildcard `_` arms live.
                self.emit_line(&format!(
                    "  switch {switch_ty} {switch_val}, label %bb{} [\n{cases}\n  ]",
                    default.0
                ));
            }
            Terminator::Unreachable => {
                self.emit_line("  unreachable");
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Operand helpers
    // -----------------------------------------------------------------------

    /// Convert an MIR Operand to its LLVM IR textual representation (the value part).
    /// For mutable locals, this emits a `load` instruction and returns the temp name.
    fn operand_to_llvm(&mut self, op: &Operand, _func: &MirFunction) -> String {
        match op {
            Operand::Local(id) => {
                if self.mutable_locals.contains(&id.0) {
                    let ty = self.local_type(*id);
                    if ty != "void" {
                        let tmp = self.next_temp();
                        self.emit_line(&format!("  {tmp} = load {ty}, ptr %_{}.addr", id.0));
                        return tmp;
                    }
                }
                format!("%_{}", id.0)
            }
            Operand::Constant(Constant::Str(s)) => {
                // String constants: get raw data pointer then wrap in KryosString.
                if let Some(global_name) = self.string_constants.get(s.as_str()) {
                    let byte_len = s.len();
                    let arr_len = byte_len + 1;
                    let gep = format!(
                        "getelementptr ([{arr_len} x i8], ptr {global_name}, i64 0, i64 0)"
                    );
                    let tmp = self.next_temp();
                    self.emit_line(&format!(
                        "  {tmp} = call ptr @kryos_string_new(ptr {gep}, i64 {byte_len})"
                    ));
                    tmp
                } else {
                    "null".into()
                }
            }
            Operand::Constant(c) => constant_to_llvm(c),
        }
    }

    /// Get the LLVM type string for an operand.
    /// Returns the max number of payload fields across all variants of an enum.
    fn enum_max_fields(&self, enum_name: &str) -> usize {
        self.enum_defs
            .get(enum_name)
            .map(|variants| variants.iter().map(|v| v.fields.len()).max().unwrap_or(0))
            .unwrap_or(0)
    }

    /// Build the LLVM struct type for an enum: `{ i64, <payload fields> }`.
    /// All payload slots use i64 for uniform layout.
    /// Resolve an MIR type to its LLVM representation for function signatures
    /// and locals — uses the proper enum aggregate instead of the i64 fallback.
    fn sig_ty_to_llvm(&self, ty: &MirType) -> String {
        match ty {
            MirType::Enum(name) => {
                let max = self.enum_max_fields(name);
                self.enum_llvm_type(name, max)
            }
            _ => mir_type_to_llvm(ty),
        }
    }

    fn enum_llvm_type(&self, _enum_name: &str, max_fields: usize) -> String {
        if max_fields == 0 {
            "{ i64 }".to_string()
        } else {
            let fields: Vec<&str> = (0..max_fields).map(|_| "i64").collect();
            format!("{{ i64, {} }}", fields.join(", "))
        }
    }

    fn operand_type(&self, op: &Operand, _func: &MirFunction) -> String {
        match op {
            Operand::Local(id) => self.local_type(*id),
            Operand::Constant(c) => constant_type(c),
        }
    }

    /// Check if an operand has a floating-point type.
    fn operand_is_float(&self, op: &Operand, func: &MirFunction) -> bool {
        let ty = self.operand_type(op, func);
        is_float_type(&ty)
    }

    /// Check if an operand has string type at the MIR level.
    fn operand_is_string(op: &Operand, func: &MirFunction) -> bool {
        match op {
            Operand::Local(id) => func
                .locals
                .iter()
                .find(|l| l.id == *id)
                .is_some_and(|l| l.ty == MirType::Str),
            Operand::Constant(Constant::Str(_)) => true,
            _ => false,
        }
    }

    /// Get the LLVM type for a local from the cached map.
    fn local_type(&self, id: LocalId) -> String {
        self.local_types
            .get(&id.0)
            .cloned()
            .unwrap_or_else(|| "i64".to_string())
    }

    // -----------------------------------------------------------------------
    // Type coercion helpers
    // -----------------------------------------------------------------------

    /// Track the actual LLVM type produced by a temp value.
    fn track_type(&mut self, name: &str, ty: &str) {
        self.value_types.insert(name.to_string(), ty.to_string());
    }

    /// Look up the actual LLVM type of an SSA value.
    /// For `%_N` locals, returns their declared type. For `%tN` temps,
    /// returns the tracked type. For anything else, returns `None`.
    #[allow(dead_code)]
    fn actual_type(&self, value: &str) -> Option<String> {
        // Check tracked temp types first.
        if let Some(ty) = self.value_types.get(value) {
            return Some(ty.clone());
        }
        // Check local types (%_N pattern).
        if let Some(suffix) = value.strip_prefix("%_") {
            if let Ok(id) = suffix.parse::<u32>() {
                return self.local_types.get(&id).cloned();
            }
        }
        None
    }

    /// Coerce a value from one LLVM type to another, emitting the necessary
    /// conversion instruction. Returns the (possibly new) value name.
    fn coerce_value(&mut self, value: &str, from_type: &str, to_type: &str) -> String {
        if from_type == to_type {
            return value.to_string();
        }
        // void operands cannot be coerced — substitute a typed zero/null.
        // This arises when a MIR local was assigned the result of a
        // void-returning call (try/catch lowering pre-allocates payload
        // slots) and is later read in an expression position.
        if from_type == "void" {
            return match to_type {
                "ptr" => "null".to_string(),
                "double" | "float" => "0.0".to_string(),
                _ => "0".to_string(),
            };
        }
        let tmp = self.next_temp();
        match (from_type, to_type) {
            ("i64", "ptr") => {
                self.emit_line(&format!("  {tmp} = inttoptr i64 {value} to ptr"));
                self.track_type(&tmp, "ptr");
            }
            ("ptr", "i64") => {
                self.emit_line(&format!("  {tmp} = ptrtoint ptr {value} to i64"));
                self.track_type(&tmp, "i64");
            }
            ("i64", "double") => {
                self.emit_line(&format!("  {tmp} = bitcast i64 {value} to double"));
                self.track_type(&tmp, "double");
            }
            ("double", "i64") => {
                self.emit_line(&format!("  {tmp} = bitcast double {value} to i64"));
                self.track_type(&tmp, "i64");
            }
            // Integer width changes — used by `fn -> i32` returns and i32
            // arithmetic that the rest of the codegen carries as i64.
            ("i64", "i32") => {
                self.emit_line(&format!("  {tmp} = trunc i64 {value} to i32"));
                self.track_type(&tmp, "i32");
            }
            ("i32", "i64") => {
                self.emit_line(&format!("  {tmp} = sext i32 {value} to i64"));
                self.track_type(&tmp, "i64");
            }
            ("i64", "i8") => {
                self.emit_line(&format!("  {tmp} = trunc i64 {value} to i8"));
                self.track_type(&tmp, "i8");
            }
            ("i8", "i64") => {
                self.emit_line(&format!("  {tmp} = sext i8 {value} to i64"));
                self.track_type(&tmp, "i64");
            }
            // Bool conversions — Kryos carries booleans as i64; LLVM `br` and
            // `select` require i1, and the typechecker carries booleans as i64
            // when read from variables.
            ("i64", "i1") => {
                self.emit_line(&format!("  {tmp} = icmp ne i64 {value}, 0"));
                self.track_type(&tmp, "i1");
            }
            ("i1", "i64") => {
                self.emit_line(&format!("  {tmp} = zext i1 {value} to i64"));
                self.track_type(&tmp, "i64");
            }
            ("i32", "i1") => {
                self.emit_line(&format!("  {tmp} = icmp ne i32 {value}, 0"));
                self.track_type(&tmp, "i1");
            }
            ("i1", "i32") => {
                self.emit_line(&format!("  {tmp} = zext i1 {value} to i32"));
                self.track_type(&tmp, "i32");
            }
            // i32 <-> ptr: widen/narrow via i64.
            ("i32", "ptr") => {
                let wide = self.next_temp();
                self.emit_line(&format!("  {wide} = sext i32 {value} to i64"));
                self.emit_line(&format!("  {tmp} = inttoptr i64 {wide} to ptr"));
                self.track_type(&tmp, "ptr");
            }
            ("ptr", "i32") => {
                let wide = self.next_temp();
                self.emit_line(&format!("  {wide} = ptrtoint ptr {value} to i64"));
                self.emit_line(&format!("  {tmp} = trunc i64 {wide} to i32"));
                self.track_type(&tmp, "i32");
            }
            // i32 <-> double.
            ("i32", "double") => {
                self.emit_line(&format!("  {tmp} = sitofp i32 {value} to double"));
                self.track_type(&tmp, "double");
            }
            ("double", "i32") => {
                self.emit_line(&format!("  {tmp} = fptosi double {value} to i32"));
                self.track_type(&tmp, "i32");
            }
            (from, "i64") if from.starts_with('%') || from.starts_with('{') => {
                // Struct/aggregate → i64: extract the first field.
                // This matches Cranelift's by-value-as-i64 semantics for
                // single-field passes (e.g. dyn-trait lowering) and is a
                // conservative fallback for multi-field aggregates.
                self.emit_line(&format!(
                    "  {tmp} = extractvalue {from} {value}, 0"
                ));
                self.track_type(&tmp, "i64");
            }
            (from, "ptr") if from.starts_with('%') || from.starts_with('{') => {
                // Struct/aggregate → ptr: extract first field as i64, then inttoptr.
                let f0 = self.next_temp();
                self.emit_line(&format!(
                    "  {f0} = extractvalue {from} {value}, 0"
                ));
                self.emit_line(&format!("  {tmp} = inttoptr i64 {f0} to ptr"));
                self.track_type(&tmp, "ptr");
            }
            _ => return value.to_string(), // no conversion needed/possible
        }
        tmp
    }

    /// Emit an identity copy that works for any LLVM type (not just integer types).
    /// For `ptr` types, uses `getelementptr i8, ptr {val}, i64 0` instead of
    /// the illegal `add ptr {val}, 0`.
    /// For `double` types, uses `fadd double {val}, 0.0`.
    /// For integer types, uses `add {ty} {val}, 0`.
    fn emit_identity_copy(&mut self, target: &str, ty: &str, val: &str) {
        if ty == "ptr" {
            self.emit_line(&format!("  {target} = getelementptr i8, ptr {val}, i64 0"));
            self.track_type(target, "ptr");
        } else if ty == "void" {
            // Void-typed locals are unrepresentable in LLVM as first-class values.
            // The source value usually came from a runtime call returning ptr.
            // Treat as ptr identity copy. Track as ptr so subsequent uses are sound.
            self.emit_line(&format!("  {target} = getelementptr i8, ptr {val}, i64 0"));
            self.track_type(target, "ptr");
        } else if is_float_type(ty) {
            self.emit_line(&format!("  {target} = fadd {ty} {val}, 0.0"));
            self.track_type(target, ty);
        } else if ty.starts_with('%') || ty.starts_with('{') {
            // Named struct types and inline aggregate types do not support
            // `add ty x, 0`. Use `select` on a true constant — LLVM optimizes
            // this away, but it is type-correct for any first-class value.
            self.emit_line(&format!(
                "  {target} = select i1 true, {ty} {val}, {ty} {val}"
            ));
            self.track_type(target, ty);
        } else {
            self.emit_line(&format!("  {target} = add {ty} {val}, 0"));
            self.track_type(target, ty);
        }
    }

    // -----------------------------------------------------------------------
    // Temp counter
    // -----------------------------------------------------------------------

    fn next_temp(&mut self) -> String {
        let t = format!("%t{}", self.temp_counter);
        self.temp_counter += 1;
        t
    }

    // -----------------------------------------------------------------------
    // Output helpers
    // -----------------------------------------------------------------------

    fn emit_line(&mut self, line: &str) {
        self.output.push_str(line);
        self.output.push('\n');
    }

    fn emit_blank(&mut self) {
        self.output.push('\n');
    }

    // -----------------------------------------------------------------------
    // Drop helpers
    // -----------------------------------------------------------------------

    /// Emit drop logic for a struct value: recursively free heap-allocated
    /// fields (strings, arrays, maps, enums, nested structs), then free the
    /// struct pointer itself.
    #[allow(clippy::collapsible_match)]
    fn emit_struct_drop(&mut self, val: &str, struct_name: &str, _func: &MirFunction) {
        let struct_def = match self.struct_defs.get(struct_name).cloned() {
            Some(def) => def,
            None => return,
        };

        for (field_idx, (_field_name, field_ty)) in struct_def.iter().enumerate() {
            match field_ty {
                MirType::Str => {
                    let gep = self.next_temp();
                    let fv = self.next_temp();
                    self.emit_line(&format!(
                        "  {gep} = getelementptr i64, ptr {val}, i32 {field_idx}"
                    ));
                    self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                    self.emit_line(&format!("  call void @kryos_string_free(ptr {fv})"));
                }
                MirType::Array(ref inner_elem, _) => {
                    let gep = self.next_temp();
                    let fv = self.next_temp();
                    self.emit_line(&format!(
                        "  {gep} = getelementptr i64, ptr {val}, i32 {field_idx}"
                    ));
                    self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                    let et = inner_elem.as_ref().clone();
                    self.emit_array_drop(&fv, &et, _func);
                }
                MirType::Function { .. } => {
                    let gep = self.next_temp();
                    let fv = self.next_temp();
                    self.emit_line(&format!(
                        "  {gep} = getelementptr i64, ptr {val}, i32 {field_idx}"
                    ));
                    self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                    self.emit_line(&format!("  call void @kryos_arc_release(ptr {fv})"));
                }
                MirType::Map { .. } => {
                    let gep = self.next_temp();
                    let fv = self.next_temp();
                    self.emit_line(&format!(
                        "  {gep} = getelementptr i64, ptr {val}, i32 {field_idx}"
                    ));
                    self.emit_line(&format!("  {fv} = load i64, ptr {gep}"));
                    self.emit_line(&format!("  call void @kryos_map_free(i64 {fv})"));
                }
                MirType::Struct(inner_name) => {
                    // @copy structs embedded in a containing struct share field
                    // pointers with their source; skip recursive drop to avoid
                    // double-free. The original owner handles cleanup.
                    if !self.copy_structs.contains(inner_name) {
                        let gep = self.next_temp();
                        let fv = self.next_temp();
                        self.emit_line(&format!(
                            "  {gep} = getelementptr i64, ptr {val}, i32 {field_idx}"
                        ));
                        self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                        let inner = inner_name.clone();
                        self.emit_struct_drop(&fv, &inner, _func);
                        self.emit_line(&format!("  call void @free(ptr {fv})"));
                    }
                }
                MirType::Enum(inner_name) => {
                    let gep = self.next_temp();
                    let fv = self.next_temp();
                    self.emit_line(&format!(
                        "  {gep} = getelementptr i64, ptr {val}, i32 {field_idx}"
                    ));
                    self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                    let inner = inner_name.clone();
                    self.emit_enum_drop(&fv, &inner, _func);
                }
                MirType::Shared(_) => {
                    let gep = self.next_temp();
                    let fv = self.next_temp();
                    self.emit_line(&format!(
                        "  {gep} = getelementptr i64, ptr {val}, i32 {field_idx}"
                    ));
                    self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                    self.emit_line(&format!("  call void @kryos_arc_release(ptr {fv})"));
                }
                _ => {}
            }
        }
    }

    /// Emit drop logic for an array value: iterate elements and drop each
    /// heap-allocated element, then call kryos_array_free.
    fn emit_array_drop(&mut self, val: &str, elem_ty: &MirType, _func: &MirFunction) {
        let is_droppable = matches!(
            elem_ty,
            MirType::Str
                | MirType::Array(_, _)
                | MirType::Struct(_)
                | MirType::Function { .. }
                | MirType::Enum(_)
                | MirType::Shared(_)
                | MirType::Map { .. }
        );

        if is_droppable {
            let uid = self.temp_counter;
            self.temp_counter += 1;

            let null_ck_label = format!("arr_nullck_{uid}");
            let pre_label = format!("arr_pre_{uid}");
            let hdr_label = format!("arr_hdr_{uid}");
            let body_label = format!("arr_body_{uid}");
            let tail_label = format!("arr_tail_{uid}");
            let exit_label = format!("arr_exit_{uid}");
            let skip_label = format!("arr_skip_{uid}");
            let i_name = format!("%arr_i_{uid}");
            let i_next_name = format!("%arr_inext_{uid}");

            // Null guard: skip element cleanup for null arrays.
            self.emit_line(&format!("  br label %{null_ck_label}"));
            self.emit_line(&format!("{null_ck_label}:"));
            let null_cmp = self.next_temp();
            self.emit_line(&format!("  {null_cmp} = icmp ne ptr {val}, null"));
            self.emit_line(&format!(
                "  br i1 {null_cmp}, label %{pre_label}, label %{skip_label}"
            ));

            // Pre-block: load array length and data pointer.
            self.emit_line(&format!("{pre_label}:"));
            let len = self.next_temp();
            self.emit_line(&format!("  {len} = load i64, ptr {val}"));
            let data_gep = self.next_temp();
            self.emit_line(&format!(
                "  {data_gep} = getelementptr i8, ptr {val}, i64 24"
            ));
            let data = self.next_temp();
            self.emit_line(&format!("  {data} = load ptr, ptr {data_gep}"));
            self.emit_line(&format!("  br label %{hdr_label}"));

            // Header: loop counter phi, compare against length.
            self.emit_line(&format!("{hdr_label}:"));
            self.emit_line(&format!(
                "  {i_name} = phi i64 [0, %{pre_label}], [{i_next_name}, %{tail_label}]"
            ));
            let cmp = self.next_temp();
            self.emit_line(&format!("  {cmp} = icmp sge i64 {i_name}, {len}"));
            self.emit_line(&format!(
                "  br i1 {cmp}, label %{exit_label}, label %{body_label}"
            ));

            // Body: load element, drop based on type.
            self.emit_line(&format!("{body_label}:"));
            let elem_gep = self.next_temp();
            self.emit_line(&format!(
                "  {elem_gep} = getelementptr i64, ptr {data}, i64 {i_name}"
            ));

            // For struct/enum elements, use named drop helpers that recursively
            // free nested heap fields. This breaks compile-time recursion since
            // the helpers are standalone functions that can call each other.
            let (load_ty, free_call) = match elem_ty {
                MirType::Str => (
                    "ptr".to_string(),
                    "call void @kryos_string_free(ptr {fv})".to_string(),
                ),
                MirType::Array(_, _) => (
                    "ptr".to_string(),
                    "call void @kryos_array_free(ptr {fv})".to_string(),
                ),
                MirType::Function { .. } | MirType::Shared(_) => (
                    "ptr".to_string(),
                    "call void @kryos_arc_release(ptr {fv})".to_string(),
                ),
                MirType::Map { .. } => (
                    "i64".to_string(),
                    "call void @kryos_map_free(i64 {fv})".to_string(),
                ),
                MirType::Struct(n) => {
                    let drop_name = format!("__kryos_drop_{n}");
                    (
                        "ptr".to_string(),
                        format!("call void @{drop_name}(ptr {{fv}})"),
                    )
                }
                MirType::Enum(n) => {
                    let drop_name = format!("__kryos_drop_{n}");
                    (
                        "ptr".to_string(),
                        format!("call void @{drop_name}(ptr {{fv}})"),
                    )
                }
                _ => ("i64".to_string(), String::new()),
            };
            if !free_call.is_empty() {
                let fv = self.next_temp();
                self.emit_line(&format!("  {fv} = load {load_ty}, ptr {elem_gep}"));
                self.emit_line(&format!("  {}", free_call.replace("{fv}", &fv)));
            }

            // Tail: increment counter, loop back.
            self.emit_line(&format!("  br label %{tail_label}"));
            self.emit_line(&format!("{tail_label}:"));
            self.emit_line(&format!("  {i_next_name} = add i64 {i_name}, 1"));
            self.emit_line(&format!("  br label %{hdr_label}"));

            self.emit_line(&format!("{exit_label}:"));
            self.emit_line(&format!("  br label %{skip_label}"));
            self.emit_line(&format!("{skip_label}:"));
        }

        self.emit_line(&format!("  call void @kryos_array_free(ptr {val})"));
    }

    /// Emit drop logic for an enum value: load the tag, dispatch on it to
    /// drop heap-owning payload fields for the active variant, then free
    /// the enum pointer.
    /// Drop the heap-managed payload fields of an enum (strings, arrays, etc.)
    /// without freeing the enum's backing storage itself. Use this for
    /// stack-allocated enum locals (alloca).
    fn emit_enum_drop_payload(&mut self, val: &str, enum_name: &str, func: &MirFunction) {
        self.emit_enum_drop_inner(val, enum_name, func, /*free_buf=*/ false);
    }

    fn emit_enum_drop(&mut self, val: &str, enum_name: &str, func: &MirFunction) {
        self.emit_enum_drop_inner(val, enum_name, func, /*free_buf=*/ true);
    }

    fn emit_enum_drop_inner(
        &mut self,
        val: &str,
        enum_name: &str,
        func: &MirFunction,
        free_buf: bool,
    ) {
        let variants = match self.enum_defs.get(enum_name).cloned() {
            Some(v) => v,
            None => {
                // Unknown enum — free the pointer if requested.
                if free_buf {
                    self.emit_line(&format!("  call void @free(ptr {val})"));
                }
                return;
            }
        };

        let has_droppable = variants.iter().any(|v| {
            v.fields.iter().any(|f| {
                matches!(
                    f,
                    MirType::Str
                        | MirType::Array(_, _)
                        | MirType::Struct(_)
                        | MirType::Function { .. }
                        | MirType::Enum(_)
                        | MirType::Shared(_)
                        | MirType::Map { .. }
                )
            })
        });

        if has_droppable {
            // Load the tag (i64 at offset 0).
            let tag = self.next_temp();
            self.emit_line(&format!("  {tag} = load i64, ptr {val}"));

            let merge_label = format!("enum_drop_merge_{}", self.temp_counter);
            self.temp_counter += 1;

            for (idx, variant) in variants.iter().enumerate() {
                let droppable_fields: Vec<(usize, &MirType)> = variant
                    .fields
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| {
                        matches!(
                            f,
                            MirType::Str
                                | MirType::Array(_, _)
                                | MirType::Struct(_)
                                | MirType::Function { .. }
                                | MirType::Enum(_)
                                | MirType::Shared(_)
                                | MirType::Map { .. }
                        )
                    })
                    .collect();

                if droppable_fields.is_empty() {
                    continue;
                }

                let cmp = self.next_temp();
                let variant_label = format!("enum_drop_v{}_{}", idx, self.temp_counter);
                let skip_label = format!("enum_drop_skip{}_{}", idx, self.temp_counter);
                self.temp_counter += 1;

                self.emit_line(&format!("  {cmp} = icmp eq i64 {tag}, {idx}"));
                self.emit_line(&format!(
                    "  br i1 {cmp}, label %{variant_label}, label %{skip_label}"
                ));
                self.emit_line(&format!("{variant_label}:"));

                for (field_idx, field_ty) in &droppable_fields {
                    let offset = (*field_idx + 1) as u32; // +1 to skip tag
                    let gep = self.next_temp();
                    let fv = self.next_temp();
                    self.emit_line(&format!(
                        "  {gep} = getelementptr i64, ptr {val}, i32 {offset}"
                    ));

                    match field_ty {
                        MirType::Str => {
                            self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                            self.emit_line(&format!("  call void @kryos_string_free(ptr {fv})"));
                        }
                        MirType::Array(inner_elem, _) => {
                            self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                            let et = inner_elem.as_ref().clone();
                            self.emit_array_drop(&fv, &et, func);
                        }
                        MirType::Function { .. } => {
                            self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                            self.emit_line(&format!("  call void @kryos_arc_release(ptr {fv})"));
                        }
                        MirType::Map { .. } => {
                            self.emit_line(&format!("  {fv} = load i64, ptr {gep}"));
                            self.emit_line(&format!("  call void @kryos_map_free(i64 {fv})"));
                        }
                        MirType::Struct(name) => {
                            self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                            let inner = name.clone();
                            self.emit_struct_drop(&fv, &inner, func);
                            self.emit_line(&format!("  call void @free(ptr {fv})"));
                        }
                        MirType::Enum(name) => {
                            self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                            let inner = name.clone();
                            self.emit_enum_drop(&fv, &inner, func);
                        }
                        MirType::Shared(_) => {
                            self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                            self.emit_line(&format!("  call void @kryos_arc_release(ptr {fv})"));
                        }
                        _ => {}
                    }
                }

                self.emit_line(&format!("  br label %{merge_label}"));
                self.emit_line(&format!("{skip_label}:"));
            }

            // Final skip block falls through to merge.
            self.emit_line(&format!("  br label %{merge_label}"));
            self.emit_line(&format!("{merge_label}:"));
        }

        // Free the heap-allocated enum itself (only when free_buf is true).
        if free_buf {
            self.emit_line(&format!("  call void @free(ptr {val})"));
        }
    }
}

// ===========================================================================
// Free functions — type mapping and utilities
// ===========================================================================

/// Map a MIR type to its LLVM IR textual representation.
pub fn mir_type_to_llvm(ty: &MirType) -> String {
    match ty {
        MirType::I8 => "i8".into(),
        MirType::I16 => "i16".into(),
        MirType::I32 => "i32".into(),
        MirType::I64 => "i64".into(),
        MirType::I128 => "i128".into(),
        // LLVM has no unsigned integer types — signedness is on the operations.
        MirType::U8 => "i8".into(),
        MirType::U16 => "i16".into(),
        MirType::U32 => "i32".into(),
        MirType::U64 => "i64".into(),
        MirType::U128 => "i128".into(),
        MirType::F32 => "float".into(),
        MirType::F64 => "double".into(),
        MirType::Bool => "i1".into(),
        MirType::Char => "i32".into(),
        MirType::Str => "ptr".into(),
        MirType::Void => "void".into(),
        // Opaque pointers since LLVM 15+.
        MirType::Ptr(_) | MirType::Ref { .. } => "ptr".into(),
        MirType::Shared(_) => "ptr".into(),
        MirType::Array(_, _) => {
            // All arrays are heap-allocated runtime objects (KryosArray*).
            // Sized arrays `[T; N]` lower to the same ptr as `[T]`; the size
            // hint is preserved only at the MIR/type-check level.
            "ptr".into()
        }
        MirType::Tuple(elems) => {
            let parts: Vec<String> = elems.iter().map(mir_type_to_llvm).collect();
            format!("{{ {} }}", parts.join(", "))
        }
        MirType::Struct(name) => format!("%{name}"),
        MirType::Enum(_) => {
            // Enum type is resolved dynamically by enum_llvm_type().
            // Fallback: treat as i64 (just the tag).
            "i64".into()
        }
        MirType::Function { params: _, ret: _ } => {
            // Function types in LLVM IR: ret_ty (param_tys)
            // But in most contexts we use `ptr` for function pointers.
            "ptr".into()
        }
        MirType::DynTrait(_) => {
            // Fat pointer: (data_ptr, vtable_ptr) — represented as i64.
            "i64".into()
        }
        MirType::Map { .. } => {
            // Heap-allocated map: runtime handle stored as i64.
            "i64".into()
        }
    }
}

/// Map an inline constant to its LLVM IR value string.
fn constant_to_llvm(c: &Constant) -> String {
    match c {
        Constant::Int(v) => v.to_string(),
        Constant::Float(v) => float_to_llvm_hex(*v),
        Constant::Bool(true) => "1".into(),
        Constant::Bool(false) => "0".into(),
        Constant::Str(_) => {
            // String constants are referenced via their global pointer;
            // this path is for operands, which shouldn't normally appear
            // as bare Constant::Str in non-assign positions.
            "null".into()
        }
        Constant::None => "null".into(),
    }
}

/// Infer the LLVM type for an inline constant.
fn constant_type(c: &Constant) -> String {
    match c {
        Constant::Int(_) => "i64".into(),
        Constant::Float(_) => "double".into(),
        Constant::Bool(_) => "i1".into(),
        Constant::Str(_) => "ptr".into(),
        Constant::None => "ptr".into(),
    }
}

/// Check if an LLVM type string represents a floating-point type.
fn is_float_type(ty: &str) -> bool {
    ty == "float" || ty == "double"
}

/// Get a rough bit-width for an LLVM type (for deciding sext vs trunc, etc.).
fn llvm_type_width(ty: &str) -> u32 {
    match ty {
        "i1" => 1,
        "i8" => 8,
        "i16" => 16,
        "i32" => 32,
        "i64" => 64,
        "i128" => 128,
        "float" => 32,
        "double" => 64,
        "ptr" => 64,
        _ => 64, // Default
    }
}

/// Convert an f64 to LLVM's hexadecimal floating-point representation.
/// LLVM uses IEEE 754 double in hex: `0xHHHHHHHHHHHHHHHH`.
fn float_to_llvm_hex(v: f64) -> String {
    if v == 0.0 && !v.is_sign_negative() {
        return "0.0".to_string();
    }
    let bits = v.to_bits();
    format!("0x{:016X}", bits)
}

/// Return a suitable zero/default value for an LLVM type string.
fn default_value_for_type(ty: &str) -> &str {
    match ty {
        "float" | "double" => "0.0",
        "ptr" => "null",
        "void" => "void",
        _ => "0", // i1, i8, i16, i32, i64, i128
    }
}

/// Escape a string for use in an LLVM IR constant array.
/// Non-printable and special characters become `\xx`.
fn llvm_escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'\\' => out.push_str("\\5C"),
            b'"' => out.push_str("\\22"),
            b'\n' => out.push_str("\\0A"),
            b'\r' => out.push_str("\\0D"),
            b'\t' => out.push_str("\\09"),
            0x20..=0x7E => out.push(b as char),
            _ => out.push_str(&format!("\\{:02X}", b)),
        }
    }
    out
}

/// Return the known LLVM parameter types for Kryos runtime functions.
/// These are used to coerce arguments correctly when calling runtime functions
/// that are not in the user-defined function table (func_param_types).
fn runtime_param_types(fname: &str) -> Option<Vec<String>> {
    match fname {
        // kryos_array_set(ptr arr, i64 idx, i64 val) -> void
        // Values are stored as raw i64 bits (floats bitcast to i64).
        "kryos_array_set" => Some(vec!["ptr".into(), "i64".into(), "i64".into()]),
        // kryos_array_push(ptr arr, i64 val) -> void
        "kryos_array_push" => Some(vec!["ptr".into(), "i64".into()]),
        // kryos_array_get(ptr arr, i64 idx) -> i64
        "kryos_array_get" => Some(vec!["ptr".into(), "i64".into()]),
        // kryos_map_insert(i64 map, i64 key, i64 val) -> void
        "kryos_map_insert" => Some(vec!["i64".into(), "i64".into(), "i64".into()]),
        // kryos_map_get(i64 map, i64 key) -> i64
        "kryos_map_get" => Some(vec!["i64".into(), "i64".into()]),
        // C math functions — single double argument
        "sqrt" | "floor" | "ceil" | "round" | "sin" | "cos" | "tan"
        | "log" | "log2" | "log10" | "fabs" => Some(vec!["double".into()]),
        _ => None,
    }
}
