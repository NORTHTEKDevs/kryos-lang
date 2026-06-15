//! LLVM IR text emitter.
//!
//! Translates MIR basic blocks, instructions, and terminators into valid
//! LLVM IR text. The output can be compiled by `llc` or `clang`.

use std::collections::{HashMap, HashSet};

use kryos_mir::ir::{
    BasicBlock, Constant, EnumVariantDef, Instruction, LocalId, MirAttributes, MirBinOp,
    MirFunction, MirModule, MirModuleHeader, MirType, MirUnOp, Operand, RValue, Terminator,
};

use crate::{CodegenError, EmitOptions};

// ---------------------------------------------------------------------------
// Codegen state
// ---------------------------------------------------------------------------

/// Container the host linker uses for debug-info records.
///
/// Selected by target OS: ELF + Mach-O use DWARF; Windows COFF uses
/// CodeView (`.pdb`). The choice changes the module-flag declaration
/// LLVM emits alongside the (otherwise format-agnostic) DI nodes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum DebugInfoFormat {
    Dwarf,
    CodeView,
}

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
    /// Whether the function currently being emitted contains MIR-level
    /// exception checks (try/catch). When it does, the MIR already routes
    /// pending exceptions and the auto post-call checks must stay out of
    /// the way (mirrors the Cranelift backend's gating).
    cur_fn_has_mir_exception_checks: bool,
    /// True if ANY function in the module calls `kryos_exception_throw` (the
    /// only thing that sets the catchable-exception flag; native panics abort
    /// instead). When false, no `throw` can ever fire, so the auto post-call
    /// exception check after every call is dead overhead and is elided —
    /// a large win for call-heavy throw-free code (e.g. recursion). Default
    /// true (conservative); set false only when a full module scan proves it.
    module_can_throw: bool,
    /// Closure capture types: func_name -> Vec of capture MIR types.
    /// Used to generate per-closure dropper functions that free heap captures.
    closure_cap_types: HashMap<String, Vec<Option<MirType>>>,
    /// Closure call signatures: func_name -> (user_param_count, ret_ty_llvm).
    /// Used to emit `{name}_env` thunks and to dispatch CallIndirect via env.
    closure_user_sig: HashMap<String, (usize, String)>,
    /// Trait vtable map from MIR: (concrete_type, trait_name) -> ordered list
    /// of mangled method names. Used to materialize trait objects and
    /// dispatch VtableCall.
    trait_vtables: HashMap<(String, String), Vec<String>>,
    /// Names of functions that have been emitted, in order — used when
    /// emitting DWARF debug metadata at module footer.
    emitted_function_names: Vec<String>,
    /// Per-emitted-function source-line tracking for DISubprogram emission.
    /// Same order as `emitted_function_names`. None means unknown line — we
    /// then synthesize line 1.
    emitted_function_lines: Vec<u32>,
    /// Metadata id currently assigned to the function being emitted, used
    /// to attach `!dbg !<n>` suffixes on the `define` line, `call`, and
    /// `ret` instructions. None outside any function or when DI is off.
    current_fn_dbg_md: Option<u32>,
    /// Metadata id for the !DILocation that's reused inside the current
    /// function. Re-emitted per function so the scope matches the DISubprogram.
    current_fn_loc_md: Option<u32>,
    /// Set of local IDs in the current function that are safe to stack-promote:
    /// fixed-size array literals that never escape (no pass-to-call, no return,
    /// no store-into-aggregate, no AddrOf, etc.).  Cleared and recomputed per
    /// function by `compute_stackable_locals`.
    stackable_locals: HashSet<u32>,
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
            cur_fn_has_mir_exception_checks: false,
            module_can_throw: true,
            closure_cap_types: HashMap::new(),
            closure_user_sig: HashMap::new(),
            trait_vtables: HashMap::new(),
            func_sig_aggs: HashMap::new(),
            emitted_function_names: Vec::new(),
            emitted_function_lines: Vec::new(),
            current_fn_dbg_md: None,
            current_fn_loc_md: None,
            stackable_locals: HashSet::new(),
        }
    }

    /// Returns `", !dbg !<n>"` if the current function has DI metadata
    /// attached, otherwise returns an empty string. Use as a suffix on
    /// every `call` and `ret` instruction inside a function with DI.
    ///
    /// Currently unused: with `emissionKind: LineTablesOnly` the verifier
    /// does not require per-instruction !dbg, so we keep this helper for
    /// future per-instruction DI emission without re-plumbing the codegen.
    #[allow(dead_code)]
    fn dbg_suffix(&self) -> String {
        match (self.options.debug_info, self.current_fn_loc_md) {
            (true, Some(n)) => format!(", !dbg !{}", n),
            _ => String::new(),
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
        self.trait_vtables = module.trait_vtables.clone();

        // Pre-scan: collect string constants, detect ARC usage, record
        // function signatures, and collect closure capture types.
        self.closure_cap_types.clear();
        self.closure_user_sig.clear();
        self.module_can_throw = module_has_throw(&module.functions);
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

        // Emit dyn-thunks for trait methods so VtableCall can dispatch via
        // an all-i64 indirect-call ABI regardless of whether the underlying
        // method uses byval/sret aggregate ABI.
        self.emit_vtable_thunks();

        // Emit type drop helpers for struct/enum types with heap-owning fields.
        // These enable array element drop to recursively clean up nested fields.
        self.emit_type_drop_helpers();
        self.emit_arc_drop_helpers();

        // Emit C-compatible main() wrapper if needed.
        if has_void_main {
            self.emit_main_wrapper();
        }

        // Emit DWARF compile-unit + DIFile metadata if `-g` is enabled and
        // the target supports DWARF (ELF/Mach-O). Skipped on Windows COFF.
        // Matches the incremental path in `emit_footer_section`.
        if self.should_emit_dwarf() {
            self.emit_dwarf_anchor_fn();
            self.emit_dwarf_metadata();
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
        self.trait_vtables = header.trait_vtables.clone();
        self.closure_cap_types.clear();
        self.closure_user_sig.clear();
        self.module_can_throw = module_has_throw(functions);

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
        self.emit_vtable_thunks();
        self.emit_type_drop_helpers();
        self.emit_arc_drop_helpers();
        if has_void_main {
            self.emit_main_wrapper();
        }
        if self.should_emit_dwarf() {
            self.emit_dwarf_anchor_fn();
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

        // !0 = compile unit. Use LineTablesOnly so we avoid the
        // expansive "every instruction needs !dbg" requirement of
        // FullDebug while still producing usable .debug_line for
        // addr2line / debugger backtraces.
        self.emit_line(&format!(
            "!0 = distinct !DICompileUnit(language: DW_LANG_C99, file: !1, producer: \"kryos {}\", isOptimized: {}, runtimeVersion: 0, emissionKind: LineTablesOnly)",
            env!("CARGO_PKG_VERSION"),
            !matches!(self.options.opt_level, crate::OptLevel::O0),
        ));
        // !1 = file
        self.emit_line(&format!(
            "!1 = !DIFile(filename: \"{}\", directory: \"{}\")",
            file, dir,
        ));
        // !2, !3 = required module flags. Module-flag !2 selects between
        // DWARF and CodeView; both are recognised by clang, and the value
        // tag matches what clang itself emits when invoked with `-g` (DWARF
        // version 4 -> matches `clang -gdwarf-4`) or `-gcodeview` (CodeView
        // -> value 1, present-or-not). !3 stays at Debug Info Version 3 for
        // both formats; that's the LLVM metadata-schema version, not the
        // container format version.
        match self.debug_info_format() {
            DebugInfoFormat::CodeView => {
                self.emit_line("!2 = !{i32 2, !\"CodeView\", i32 1}");
            }
            DebugInfoFormat::Dwarf => {
                self.emit_line("!2 = !{i32 7, !\"Dwarf Version\", i32 4}");
            }
        }
        self.emit_line("!3 = !{i32 2, !\"Debug Info Version\", i32 3}");

        // !4 = empty subroutine type (void()). LineTablesOnly mode does
        // not require parameter types, so we use a single null-typed
        // signature shared across all DISubprograms.
        self.emit_line("!4 = !DISubroutineType(types: !5)");
        self.emit_line("!5 = !{null}");

        // !6 = DISubprogram anchor attached to @__kryos_dwarf_anchor.
        // Kept for back-compat: previously the only DISubprogram.
        self.emit_line(
            "!6 = distinct !DISubprogram(name: \"__kryos_dwarf_anchor\", linkageName: \"__kryos_dwarf_anchor\", scope: !1, file: !1, line: 1, type: !4, scopeLine: 1, spFlags: DISPFlagDefinition, unit: !0)",
        );
        // !8 = a single DILocation pointing at the start of the user
        // source so LLVM emits a real .debug_line program.
        self.emit_line("!8 = !DILocation(line: 1, column: 1, scope: !6)");

        // Per-function DISubprograms + DILocations. Metadata id layout is
        // synchronised with `emit_function_as`:
        //   per function k (0-indexed): subprogram_id = 100 + 2*k, loc_id = +1
        let fn_meta: Vec<(String, u32)> = self
            .emitted_function_names
            .iter()
            .cloned()
            .zip(self.emitted_function_lines.iter().copied())
            .collect();
        for (k, (name, line)) in fn_meta.iter().enumerate() {
            let sub_id = 100 + 2 * k as u32;
            let loc_id = sub_id + 1;
            self.emit_line(&format!(
                "!{sub_id} = distinct !DISubprogram(name: \"{name}\", linkageName: \"{name}\", scope: !1, file: !1, line: {line}, type: !4, scopeLine: {line}, spFlags: DISPFlagDefinition, unit: !0)",
            ));
            self.emit_line(&format!(
                "!{loc_id} = !DILocation(line: {line}, column: 1, scope: !{sub_id})",
            ));
        }
    }

    // -----------------------------------------------------------------------
    // Module header
    // -----------------------------------------------------------------------

    fn emit_struct_type_decls(&mut self) {
        if self.struct_defs.is_empty() {
            return;
        }
        // Clone so we can call the &self helper sig_ty_to_llvm while mapping.
        let defs: Vec<(String, Vec<(String, MirType)>)> = self
            .struct_defs
            .iter()
            .filter(|(n, _)| n.as_str() != "Map")
            .map(|(n, fields)| (n.clone(), fields.clone()))
            .collect();
        let mut decls: Vec<(String, String)> = defs
            .iter()
            .map(|(n, fields)| {
                // sig_ty_to_llvm (not the free mir_type_to_llvm) so an ENUM-typed
                // field gets its real aggregate type `{ i64, <payloads> }` rather
                // than the bare-i64 tag fallback. With bare i64, extracting the
                // field yielded an i64 that mismatched the enum aggregate it is
                // used as (the json/mcp AOT errors). Safe now that StoreField uses
                // a struct-indexed GEP (step 184) -- field offsets honour the real
                // (variable-size) layout.
                let parts: Vec<String> =
                    fields.iter().map(|(_, ty)| self.sig_ty_to_llvm(ty)).collect();
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
        self.emit_line("declare ptr @calloc(i64, i64)");
        self.emit_line("declare ptr @kryos_calloc(i64, i64)");
        self.emit_line("declare void @kryos_free(ptr)");
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
        // libc exit declaration — suppressed when the program defines its
        // own `fn exit` (the std::process stdlib does), because the
        // user's `define internal void @exit(i32)` would otherwise clash
        // with this external declare AND with the libc-exported `exit`
        // at link time. The user's exit is reachable from the "exit"
        // builtin call site through user-shadow detection.
        if !self.func_param_types.contains_key("exit") {
            self.emit_line("declare void @exit(i32)");
        }
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
        // Cold panic helpers for the inlined array-read fast path.
        self.emit_line("declare void @kryos_array_oob_panic(i64, i64)");
        self.emit_line("declare void @kryos_array_null_panic()");
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
        self.emit_line("declare i64 @kryos_http_request_ks(i64, i64, i64, i64, i64)");
        self.emit_line("declare i64 @kryos_builtin_parse_int(i64)");
        self.emit_line("declare i64 @kryos_builtin_parse_float(i64)");
        self.emit_line("declare i64 @kryos_builtin_type_of(i64)");
        self.emit_line("declare i64 @kryos_builtin_assert(i64, i64)");
        self.emit_line("declare i64 @kryos_builtin_assert_eq(i64, i64)");
        self.emit_line("declare i64 @kryos_builtin_panic(i64)");
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
        // C math functions (used by sqrt, floor, ceil, sin, cos, etc. builtins).
        // Suppressed per-name when the program DEFINES a function of that name —
        // std::math provides pure-Kryos `floor`/`ceil`/`round`/`sqrt`/`sin`/...
        // and a `declare double @floor` (external) clashes with the user's
        // `define internal double @floor` ("invalid redefinition"). Same pattern
        // as the libc `exit` declaration above. The std::math implementations are
        // self-contained (Newton/Taylor), so they never need the libm symbol.
        for libm in [
            "sqrt", "floor", "ceil", "round", "sin", "cos", "tan", "log", "log2", "log10",
            "fabs",
        ] {
            if !self.func_param_types.contains_key(libm) {
                self.emit_line(&format!("declare double @{libm}(double)"));
            }
        }
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
        self.emit_line("declare i32 @kryos_fs_rename(ptr, i64, ptr, i64)");
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
        self.emit_line("declare i64 @kryos_hmac_sha256_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_ed25519_generate_ks()");
        self.emit_line("declare i64 @kryos_ed25519_public_ks(i64)");
        self.emit_line("declare i64 @kryos_ed25519_sign_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_ed25519_verify_ks(i64, i64, i64)");
        self.emit_line("declare i64 @kryos_pbkdf2_sha256_ks(i64, i64, i64)");
        self.emit_line("declare i64 @kryos_hex_to_b64url_ks(i64)");
        self.emit_line("declare i64 @kryos_b64url_to_hex_ks(i64)");
        self.emit_line("declare i64 @kryos_sha1_hex_ks(i64)");
        self.emit_line("declare i64 @kryos_sha1_base64_ks(i64)");
        self.emit_line("declare i64 @kryos_base64_encode_ks(i64)");
        self.emit_line("declare i64 @kryos_base64_decode_ks(i64)");
        self.emit_line("declare i64 @kryos_random_bytes_ks(i64)");
        self.emit_line("declare i64 @kryos_chr_ks(i64)");
        self.emit_line("declare i64 @kryos_byte_at_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_time_now_secs()");
        self.emit_line("declare i64 @kryos_time_now_millis()");
        self.emit_line("declare void @kryos_time_sleep_millis(i64)");
        self.emit_line("declare void @kryos_sleep_ms(i64)");
        // f64<->i64 bit reinterpret (i64-arg / f64-return form; the f64-arg
        // form is unused from Kryos because f64 args across the extern ABI
        // land in the wrong register -- stdlib does f64->bits array-side).
        self.emit_line("declare double @kryos_f64_from_bits(i64)");
        self.emit_line("declare i64 @kryos_regex_new_ks(i64)");
        self.emit_line("declare i64 @kryos_regex_is_match_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_mutex_new()");
        self.emit_line("declare void @kryos_mutex_lock(i64)");
        self.emit_line("declare void @kryos_mutex_unlock(i64)");
        self.emit_line("declare void @kryos_mutex_drop(i64)");
        self.emit_line("; Low-level FFI helpers (v2.3.4) — pointers carried as i64 in IR.");
        self.emit_line("declare i64 @kryos_str_to_ptr(i64)");
        self.emit_line("declare i64 @kryos_buf_to_str(i64, i64)");
        self.emit_line("declare i64 @kryos_alloc_bytes(i64)");
        self.emit_line("declare void @kryos_free_bytes(i64, i64)");
        self.emit_line("declare i64 @kryos_ptr_byte_at(i64, i64)");
        self.emit_line("declare void @kryos_ptr_set_byte(i64, i64, i64)");
        self.emit_line("declare i64 @kryos_ptr_read_i64(i64, i64)");
        self.emit_line("declare void @kryos_ptr_write_i64(i64, i64, i64)");
        self.emit_line("declare i64 @kryos_handle_to_str(i64)");
        self.emit_line("; Exception runtime (used by try/catch)");
        self.emit_line("declare void @kryos_exception_throw(i64)");
        self.emit_line("declare i64 @kryos_exception_check()");
        self.emit_line("declare i64 @kryos_exception_take()");
        self.emit_line("declare void @kryos_exception_report_uncaught_if_pending()");
        self.emit_line("declare i64 @kryos_budget_push(i64, i64)");
        self.emit_line("declare void @kryos_budget_pop_to(i64)");
        self.emit_line("declare i64 @kryos_budget_active()");
        self.emit_line("declare i64 @kryos_budget_try_call()");
        self.emit_line("declare i64 @kryos_budget_charge_tokens(i64)");
        self.emit_line("declare i64 @kryos_budget_remaining_tokens()");
        self.emit_line("declare i64 @kryos_budget_remaining_calls()");
        self.emit_line("declare i64 @kryos_string_compare(ptr, ptr)");
        // ---------------------------------------------------------------
        // Auto-generated runtime symbol declarations (Class A' fix).
        //
        // Mirrors every `pub (unsafe)? extern "C" fn kryos_*` exported by
        // `kryos-rt` and `kryos-stdlib-native`. The Cranelift JIT resolves
        // these symbols dynamically through its symbol map (`jit.rs`); the
        // LLVM AOT path needs explicit `declare` lines so clang knows the
        // signature when it links the emitted IR against the staticlibs.
        //
        // Generator: tests/parity/gen_decls.py. Regenerate when adding new
        // runtime exports.
        self.emit_line("; Auto-generated runtime symbol declarations (M3 Class A')");
        self.emit_line("declare i32 @kryos_actor_lock(i64)");
        self.emit_line("declare i32 @kryos_actor_recv(ptr, i64)");
        self.emit_line("declare i64 @kryos_actor_recv_timeout_i64(i64)");
        self.emit_line("declare i32 @kryos_actor_send(i64, ptr, i64)");
        self.emit_line("declare void @kryos_actor_spawn(i64)");
        self.emit_line("declare i32 @kryos_actor_unlock(i64)");
        self.emit_line("declare void @kryos_alert_ks(i64)");
        self.emit_line("declare ptr @kryos_alloc(i64, i64)");
        self.emit_line("declare i64 @kryos_arc_ref_count(ptr)");
        self.emit_line("declare void @kryos_arc_release_i64(i64)");
        self.emit_line("declare void @kryos_arc_retain_i64(i64)");
        self.emit_line("declare void @kryos_arc_set_drop_i64(i64, i64)");
        self.emit_line("declare ptr @kryos_array_retain(ptr)");
        self.emit_line("declare ptr @kryos_string_retain(ptr)");
        self.emit_line("declare i64 @kryos_map_retain(i64)");
        self.emit_line("declare i64 @kryos_async_current_task()");
        self.emit_line("declare i64 @kryos_async_park_current()");
        self.emit_line("declare void @kryos_async_run()");
        self.emit_line("declare void @kryos_async_set_result(i64)");
        self.emit_line("declare void @kryos_async_spawn(i64)");
        self.emit_line("declare i64 @kryos_async_take_result()");
        self.emit_line("declare void @kryos_async_wake(i64)");
        self.emit_line("declare void @kryos_async_yield_now()");
        self.emit_line("declare void @kryos_buf_free(i64)");
        self.emit_line("declare i64 @kryos_buf_get_byte(i64, i64)");
        self.emit_line("declare i64 @kryos_buf_len(i64)");
        self.emit_line("declare i64 @kryos_buf_new(i64)");
        self.emit_line("declare void @kryos_buf_patch_i32_le(i64, i64, i64)");
        self.emit_line("declare void @kryos_buf_patch_i64_le(i64, i64, i64)");
        self.emit_line("declare void @kryos_buf_set_byte(i64, i64, i64)");
        self.emit_line("declare void @kryos_buf_write_byte(i64, i64)");
        self.emit_line("declare void @kryos_buf_write_bytes(i64, i64, i64)");
        self.emit_line("declare void @kryos_buf_write_i16_le(i64, i64)");
        self.emit_line("declare void @kryos_buf_write_i32_le(i64, i64)");
        self.emit_line("declare void @kryos_buf_write_i64_le(i64, i64)");
        self.emit_line("declare void @kryos_buf_write_str(i64, i64)");
        self.emit_line("declare i64 @kryos_buf_write_to_file(i64, i64)");
        self.emit_line("declare void @kryos_buf_write_zeros(i64, i64)");
        self.emit_line("declare i64 @kryos_builtin_abs(i64)");
        self.emit_line("declare double @kryos_builtin_abs_f(double)");
        self.emit_line("declare double @kryos_builtin_ceil(double)");
        self.emit_line("declare double @kryos_builtin_cos(double)");
        self.emit_line("declare i64 @kryos_builtin_create_dir(i64)");
        self.emit_line("declare i64 @kryos_builtin_file_size(i64)");
        self.emit_line("declare i64 @kryos_builtin_float_from_float(double)");
        self.emit_line("declare double @kryos_builtin_floor(double)");
        self.emit_line("declare i64 @kryos_builtin_int_from_float(double)");
        self.emit_line("declare double @kryos_builtin_log(double)");
        self.emit_line("declare double @kryos_builtin_log10(double)");
        self.emit_line("declare double @kryos_builtin_log2(double)");
        self.emit_line("declare i64 @kryos_builtin_max(i64, i64)");
        self.emit_line("declare double @kryos_builtin_max_f(double, double)");
        self.emit_line("declare i64 @kryos_builtin_min(i64, i64)");
        self.emit_line("declare double @kryos_builtin_min_f(double, double)");
        self.emit_line("declare double @kryos_builtin_pow(double, double)");
        self.emit_line("declare i64 @kryos_builtin_push(i64, i64)");
        self.emit_line("declare double @kryos_builtin_sin(double)");
        self.emit_line("declare double @kryos_builtin_sqrt(double)");
        self.emit_line("declare double @kryos_builtin_tan(double)");
        self.emit_line("declare void @kryos_canvas_clear_ks(i64)");
        self.emit_line("declare ptr @kryos_chan_clone(ptr)");
        self.emit_line("declare void @kryos_chan_close(ptr)");
        self.emit_line("declare void @kryos_chan_close_i64(i64)");
        self.emit_line("declare void @kryos_chan_drop(ptr)");
        self.emit_line("declare void @kryos_chan_drop_i64(i64)");
        self.emit_line("declare i32 @kryos_chan_is_closed(ptr)");
        self.emit_line("declare ptr @kryos_chan_new(i64)");
        self.emit_line("declare i32 @kryos_chan_recv(ptr, ptr, i64)");
        self.emit_line("declare i64 @kryos_chan_recv_timeout_i64(i64, i64)");
        self.emit_line("declare i32 @kryos_chan_send(ptr, ptr, i64)");
        self.emit_line("declare i32 @kryos_chan_try_recv(ptr, ptr, i64)");
        self.emit_line("declare void @kryos_check_div_zero_f64(double)");
        self.emit_line("declare void @kryos_check_div_zero_i64(i64)");
        self.emit_line("declare i64 @kryos_checked_add_i64(i64, i64)");
        self.emit_line("declare i64 @kryos_checked_mul_i64(i64, i64)");
        self.emit_line("declare i64 @kryos_checked_sub_i64(i64, i64)");
        self.emit_line("declare i32 @kryos_db_close(i64)");
        self.emit_line("declare i32 @kryos_db_col_count(i64)");
        self.emit_line("declare i64 @kryos_db_col_int(i64, i32)");
        self.emit_line("declare i64 @kryos_db_col_text_len(i64, i32)");
        self.emit_line("declare i64 @kryos_db_exec(i64, ptr, i64)");
        self.emit_line("declare i32 @kryos_db_finalize(i64)");
        self.emit_line("declare i64 @kryos_db_open(ptr, i64)");
        self.emit_line("declare i64 @kryos_db_open_memory()");
        self.emit_line("declare i64 @kryos_db_prepare(i64, ptr, i64)");
        self.emit_line("declare i32 @kryos_db_step(i64)");
        self.emit_line("declare void @kryos_dealloc(ptr, i64, i64)");
        self.emit_line("declare i64 @kryos_dom_get_value_ks(i64)");
        self.emit_line("declare void @kryos_dom_set_text_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_env_args_count()");
        self.emit_line("declare ptr @kryos_env_cwd()");
        self.emit_line("declare ptr @kryos_env_home()");
        self.emit_line("declare ptr @kryos_env_platform()");
        self.emit_line("declare i32 @kryos_env_set(ptr, ptr)");
        self.emit_line("declare i32 @kryos_env_unset(ptr)");
        self.emit_line("declare i64 @kryos_fetch_text_ks(i64)");
        self.emit_line("declare i64 @kryos_ffi_cstr(ptr)");
        self.emit_line("declare void @kryos_ffi_dlcallv_3f32(i64, i64, i64, i64)");
        self.emit_line("declare void @kryos_ffi_dlcallv_4f(i64, double, double, double, double)");
        self.emit_line("declare void @kryos_ffi_dlcallv_4f32(i64, i64, i64, i64, i64)");
        // Fixed-arity i64-returning indirect calls (these were missing, so any
        // ffi.call0..8 call site emitted `call i64 @kryos_ffi_dlcallN` against an
        // undeclared symbol -> clang "use of undefined value").
        self.emit_line("declare i64 @kryos_ffi_dlcall0(i64)");
        self.emit_line("declare i64 @kryos_ffi_dlcall1(i64, i64)");
        self.emit_line("declare i64 @kryos_ffi_dlcall2(i64, i64, i64)");
        self.emit_line("declare i64 @kryos_ffi_dlcall3(i64, i64, i64, i64)");
        self.emit_line("declare i64 @kryos_ffi_dlcall4(i64, i64, i64, i64, i64)");
        self.emit_line("declare i64 @kryos_ffi_dlcall5(i64, i64, i64, i64, i64, i64)");
        self.emit_line("declare i64 @kryos_ffi_dlcall6(i64, i64, i64, i64, i64, i64, i64)");
        self.emit_line("declare i64 @kryos_ffi_dlcall7(i64, i64, i64, i64, i64, i64, i64, i64)");
        self.emit_line("declare i64 @kryos_ffi_dlcall8(i64, i64, i64, i64, i64, i64, i64, i64, i64)");
        // Raw pointer reads/writes (exposed by std::ffi).
        self.emit_line("declare i64 @kryos_ffi_read_i8(i64)");
        self.emit_line("declare i64 @kryos_ffi_read_i16(i64)");
        self.emit_line("declare i64 @kryos_ffi_read_i32(i64)");
        self.emit_line("declare i64 @kryos_ffi_read_i64(i64)");
        self.emit_line("declare double @kryos_ffi_read_f32(i64)");
        self.emit_line("declare double @kryos_ffi_read_f64(i64)");
        self.emit_line("declare void @kryos_ffi_write_i8(i64, i64)");
        self.emit_line("declare void @kryos_ffi_write_i16(i64, i64)");
        self.emit_line("declare void @kryos_ffi_write_i32(i64, i64)");
        self.emit_line("declare void @kryos_ffi_write_i64(i64, i64)");
        self.emit_line("declare void @kryos_ffi_write_f32(i64, double)");
        self.emit_line("declare void @kryos_ffi_write_f64(i64, double)");
        self.emit_line("declare void @kryos_ffi_dlcallv0(i64)");
        self.emit_line("declare void @kryos_ffi_dlcallv1(i64, i64)");
        self.emit_line("declare void @kryos_ffi_dlcallv2(i64, i64, i64)");
        self.emit_line("declare void @kryos_ffi_dlcallv3(i64, i64, i64, i64)");
        self.emit_line("declare void @kryos_ffi_dlcallv4(i64, i64, i64, i64, i64)");
        self.emit_line("declare void @kryos_ffi_dlcallv5(i64, i64, i64, i64, i64, i64)");
        self.emit_line("declare void @kryos_ffi_dlcallv6(i64, i64, i64, i64, i64, i64, i64)");
        self.emit_line("declare void @kryos_ffi_dlcallv7(i64, i64, i64, i64, i64, i64, i64, i64)");
        self.emit_line("declare i64 @kryos_ffi_dlclose(i64)");
        self.emit_line("declare i64 @kryos_ffi_dlopen(ptr)");
        self.emit_line("declare i64 @kryos_ffi_dlsym(i64, ptr)");
        self.emit_line("declare void @kryos_ffi_free(i64, i64)");
        self.emit_line("declare i64 @kryos_ffi_malloc(i64)");
        self.emit_line("declare i64 @kryos_ffi_string_from_ptr(i64, i64)");
        self.emit_line("declare i64 @kryos_ffi_strlen(ptr)");
        self.emit_line("declare i64 @kryos_fs_delete(ptr, i64)");
        self.emit_line("declare i64 @kryos_fs_exists(ptr, i64)");
        self.emit_line("declare i64 @kryos_global_get(i64)");
        self.emit_line("declare i64 @kryos_global_has(i64)");
        self.emit_line("declare i64 @kryos_global_set(i64, i64)");
        self.emit_line("declare i64 @kryos_http2_get_ks(i64)");
        self.emit_line("declare i64 @kryos_http2_post_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_http2_request_ks(i64, i64, i64, i64)");
        self.emit_line("declare i64 @kryos_https_get_ks(i64)");
        self.emit_line("declare i64 @kryos_map_delete(i64, i64)");
        self.emit_line("declare i64 @kryos_map_delete_str(i64, i64)");
        self.emit_line("declare i64 @kryos_map_has(i64, i64)");
        self.emit_line("declare i64 @kryos_map_has_str(i64, i64)");
        self.emit_line("declare i64 @kryos_map_keys(i64)");
        self.emit_line("declare i64 @kryos_map_keys_str(i64)");
        self.emit_line("declare double @kryos_math_abs_f64(double)");
        self.emit_line("declare i64 @kryos_math_abs_i64(i64)");
        self.emit_line("declare double @kryos_math_ceil(double)");
        self.emit_line("declare double @kryos_math_clamp_f64(double, double, double)");
        self.emit_line("declare double @kryos_math_cos(double)");
        self.emit_line("declare double @kryos_math_e()");
        self.emit_line("declare double @kryos_math_floor(double)");
        self.emit_line("declare double @kryos_math_log(double)");
        self.emit_line("declare double @kryos_math_log10(double)");
        self.emit_line("declare double @kryos_math_log2(double)");
        self.emit_line("declare double @kryos_math_max_f64(double, double)");
        self.emit_line("declare i64 @kryos_math_max_i64(i64, i64)");
        self.emit_line("declare double @kryos_math_min_f64(double, double)");
        self.emit_line("declare i64 @kryos_math_min_i64(i64, i64)");
        self.emit_line("declare double @kryos_math_pi()");
        self.emit_line("declare double @kryos_math_pow(double, double)");
        self.emit_line("declare double @kryos_math_round(double)");
        self.emit_line("declare double @kryos_math_sin(double)");
        self.emit_line("declare double @kryos_math_sqrt(double)");
        self.emit_line("declare double @kryos_math_tan(double)");
        self.emit_line("declare i64 @kryos_panic(ptr, i64)");
        self.emit_line("declare ptr @kryos_path_absolute(ptr)");
        self.emit_line("declare ptr @kryos_path_basename(ptr)");
        self.emit_line("declare ptr @kryos_path_dirname(ptr)");
        self.emit_line("declare ptr @kryos_path_extension(ptr)");
        self.emit_line("declare void @kryos_path_free(ptr)");
        self.emit_line("declare i32 @kryos_path_is_dir(ptr)");
        self.emit_line("declare i32 @kryos_path_is_file(ptr)");
        self.emit_line("declare ptr @kryos_path_join(ptr, ptr)");
        self.emit_line("declare i64 @kryos_pg_close(i64)");
        self.emit_line("declare i64 @kryos_pg_connect(ptr, i64)");
        self.emit_line("declare i64 @kryos_pg_exec(i64, ptr, i64)");
        self.emit_line("declare i64 @kryos_poll_readable(ptr, i64, i64)");
        self.emit_line("declare void @kryos_print_int(i64)");
        self.emit_line("declare void @kryos_println_int(i64)");
        self.emit_line("declare i32 @kryos_rand_bool()");
        self.emit_line("declare void @kryos_rand_bytes(ptr, i64)");
        self.emit_line("declare double @kryos_rand_f64()");
        self.emit_line("declare i64 @kryos_rand_i64(i64, i64)");
        self.emit_line("declare void @kryos_rand_seed(i64)");
        self.emit_line("declare i32 @kryos_random_bytes(ptr, i64)");
        self.emit_line("declare void @kryos_regex_drop(ptr)");
        self.emit_line("declare void @kryos_regex_drop_ks(i64)");
        self.emit_line("declare i64 @kryos_regex_find_end_ks(i64, i64, i64)");
        self.emit_line("declare i64 @kryos_regex_find_ks(i64, i64)");
        self.emit_line("declare i64 @kryos_regex_find_pos_ks(i64, i64, i64)");
        self.emit_line("declare i32 @kryos_regex_is_match(ptr, ptr, i64)");
        self.emit_line("declare ptr @kryos_regex_new(ptr, i64)");
        self.emit_line("declare void @kryos_rt_init()");
        self.emit_line("declare i64 @kryos_saturating_add_i64(i64, i64)");
        self.emit_line("declare i64 @kryos_saturating_mul_i64(i64, i64)");
        self.emit_line("declare i64 @kryos_saturating_sub_i64(i64, i64)");
        self.emit_line("declare i32 @kryos_sha1(ptr, i64, ptr)");
        self.emit_line("declare i32 @kryos_sha256(ptr, i64, ptr)");
        self.emit_line("declare i32 @kryos_sha512(ptr, i64, ptr)");
        self.emit_line("declare i32 @kryos_socket_close(i64)");
        self.emit_line("declare i64 @kryos_stdin_read(ptr, i64)");
        self.emit_line("declare i64 @kryos_stdout_write(ptr, i64)");
        self.emit_line("declare ptr @kryos_str_concat(ptr, ptr)");
        self.emit_line("declare i32 @kryos_str_contains(ptr, ptr)");
        self.emit_line("declare i32 @kryos_str_ends_with(ptr, ptr)");
        self.emit_line("declare void @kryos_str_free(ptr)");
        self.emit_line("declare i64 @kryos_str_len(ptr)");
        self.emit_line("declare i32 @kryos_str_parse_f64(ptr, ptr)");
        self.emit_line("declare i32 @kryos_str_parse_i64(ptr, ptr)");
        self.emit_line("declare ptr @kryos_str_repeat(ptr, i64)");
        self.emit_line("declare ptr @kryos_str_replace(ptr, ptr, ptr)");
        self.emit_line("declare i32 @kryos_str_starts_with(ptr, ptr)");
        self.emit_line("declare ptr @kryos_str_to_lower(ptr)");
        self.emit_line("declare ptr @kryos_str_to_upper(ptr)");
        self.emit_line("declare ptr @kryos_str_trim(ptr)");
        self.emit_line("declare i64 @kryos_string_char_at(i64, i64)");
        self.emit_line("declare i64 @kryos_string_hash(ptr)");
        self.emit_line("declare ptr @kryos_string_to_lower(ptr)");
        self.emit_line("declare ptr @kryos_string_to_upper(ptr)");
        self.emit_line("declare ptr @kryos_string_trim(ptr)");
        self.emit_line("declare i64 @kryos_tcp_bind(ptr, i64, i16)");
        self.emit_line("declare i64 @kryos_tcp_connect(ptr, i64, i16)");
        self.emit_line("declare i64 @kryos_tcp_recv(i64, ptr, i64)");
        self.emit_line("declare i64 @kryos_tcp_send(i64, ptr, i64)");
        self.emit_line("declare i32 @kryos_term_clear(i8)");
        self.emit_line("declare i32 @kryos_term_cursor_move(i16, i16)");
        self.emit_line("declare i32 @kryos_term_height()");
        self.emit_line("declare i32 @kryos_term_raw_disable()");
        self.emit_line("declare i32 @kryos_term_raw_enable()");
        self.emit_line("declare i32 @kryos_term_width()");
        self.emit_line("declare i32 @kryos_tls_close(i64)");
        self.emit_line("declare i64 @kryos_tls_connect(ptr, i64, i16)");
        self.emit_line("declare i64 @kryos_tls_recv(i64, ptr, i64)");
        self.emit_line("declare i64 @kryos_tls_send(i64, ptr, i64)");
        self.emit_line("declare void @kryos_trace_exit()");
        self.emit_line("declare i64 @kryos_uds_bind(ptr, i64)");
        self.emit_line("declare i64 @kryos_uds_connect(ptr, i64)");
        self.emit_line("declare i64 @kryos_uds_recv(i64, ptr, i64)");
        self.emit_line("declare i64 @kryos_uds_send(i64, ptr, i64)");
        self.emit_line("declare i64 @kryos_wrapping_add_i64(i64, i64)");
        self.emit_line("declare i64 @kryos_wrapping_mul_i64(i64, i64)");
        self.emit_line("declare i64 @kryos_wrapping_sub_i64(i64, i64)");
        // Multi-line runtime signatures the auto-generator skipped — added by
        // hand. Only entries NOT already declared above; types match the
        // actual extern "C" fn declarations in kryos-stdlib-native / kryos-rt.
        self.emit_line("declare i64 @kryos_db_col_text_copy(i64, i32, i64, i64)");
        self.emit_line("declare i64 @kryos_ws_decode_frame_ks(i64, i64, i64)");
        self.emit_line("declare ptr @kryos_realloc(ptr, i64, i64, i64)");
        self.emit_line("declare i64 @kryos_fs_read(ptr, i64, ptr, i64)");
        self.emit_line("declare i64 @kryos_fs_write(ptr, i64, ptr, i64)");
        self.emit_line("declare void @kryos_async_spawn_task(i64, i64)");
        self.emit_line("declare i64 @kryos_async_block_on(i64)");
        self.emit_line("declare void @kryos_panic_with_location(ptr, i64, ptr, i64, i64, i64)");

        // Inlined array-read fast path. `arr[i]` used to emit a runtime
        // `call @kryos_array_get`, so the hottest loops paid call overhead
        // and the optimizer could not hoist the bounds check or vectorize.
        // This `alwaysinline` helper open-codes the exact semantics (null
        // check, unsigned bounds check, load) so LLVM inlines it into the
        // loop, hoists the loop-invariant length load, and can elide
        // redundant checks. The cold panic branches call out-of-line runtime
        // helpers. KryosArray layout (frozen v4 ABI): { len@0, cap@8,
        // elem_size@16, ref_count@24, data@32 }, elements 8 bytes each.
        self.emit_line(
            "define internal i64 @__kryos_array_get_inline(ptr %a, i64 %i) alwaysinline {",
        );
        self.emit_line("entry:");
        self.emit_line("  %isnull = icmp eq ptr %a, null");
        self.emit_line("  br i1 %isnull, label %nullp, label %chk");
        self.emit_line("nullp:");
        self.emit_line("  call void @kryos_array_null_panic()");
        self.emit_line("  unreachable");
        self.emit_line("chk:");
        self.emit_line("  %len = load i64, ptr %a, !tbaa !103");
        self.emit_line("  %oob = icmp uge i64 %i, %len");
        self.emit_line("  br i1 %oob, label %oobp, label %ld");
        self.emit_line("oobp:");
        self.emit_line("  call void @kryos_array_oob_panic(i64 %i, i64 %len)");
        self.emit_line("  unreachable");
        self.emit_line("ld:");
        self.emit_line("  %dptr = getelementptr i64, ptr %a, i64 4");
        self.emit_line("  %data = load ptr, ptr %dptr, !tbaa !103");
        self.emit_line("  %elemp = getelementptr i64, ptr %data, i64 %i");
        self.emit_line("  %v = load i64, ptr %elemp, !tbaa !104");
        self.emit_line("  ret i64 %v");
        self.emit_line("}");

        // Inlined array-write fast path, mirroring the read helper. `arr[i] = v`
        // lowers to a call to kryos_array_set; the LLVM backend redirects it
        // here so the store is inlined (no COW -- array_set never copies; only
        // push does). The value is the raw i64 slot (floats already bitcast).
        self.emit_line(
            "define internal void @__kryos_array_set_inline(ptr %a, i64 %i, i64 %v) alwaysinline {",
        );
        self.emit_line("entry:");
        self.emit_line("  %isnull = icmp eq ptr %a, null");
        self.emit_line("  br i1 %isnull, label %nullp, label %chk");
        self.emit_line("nullp:");
        self.emit_line("  call void @kryos_array_null_panic()");
        self.emit_line("  unreachable");
        self.emit_line("chk:");
        self.emit_line("  %len = load i64, ptr %a, !tbaa !103");
        self.emit_line("  %oob = icmp uge i64 %i, %len");
        self.emit_line("  br i1 %oob, label %oobp, label %st");
        self.emit_line("oobp:");
        self.emit_line("  call void @kryos_array_oob_panic(i64 %i, i64 %len)");
        self.emit_line("  unreachable");
        self.emit_line("st:");
        self.emit_line("  %dptr = getelementptr i64, ptr %a, i64 4");
        self.emit_line("  %data = load ptr, ptr %dptr, !tbaa !103");
        self.emit_line("  %elemp = getelementptr i64, ptr %data, i64 %i");
        self.emit_line("  store i64 %v, ptr %elemp, !tbaa !104");
        self.emit_line("  ret void");
        self.emit_line("}");
        self.emit_blank();

        // TBAA metadata for array accesses. The element data buffer is ALWAYS a
        // separate allocation from the KryosArray header (header.data@32 points
        // outside the header), so a `!elem` store can never clobber a `!hdr`
        // load. Tagging them as sibling scalar types (neither an ancestor of the
        // other) lets LLVM prove no-alias and hoist the loop-invariant `len` and
        // `data` loads out of hot loops where an element store sits between them.
        // Fixed ids !100-!104 do not collide with the debug-info nodes (!0-!8).
        self.emit_line("!100 = !{!\"kryos_tbaa_root\"}");
        self.emit_line("!101 = !{!\"kryos_array_hdr\", !100, i64 0}");
        self.emit_line("!102 = !{!\"kryos_array_elem\", !100, i64 0}");
        self.emit_line("!103 = !{!101, !101, i64 0}");
        self.emit_line("!104 = !{!102, !102, i64 0}");
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

    /// Returns true if we should emit LLVM debug-info metadata for this
    /// build. The metadata itself is format-agnostic at the IR level
    /// (`DICompileUnit`, `DIFile`, `DISubprogram`, `DILocation`); what
    /// distinguishes DWARF from CodeView is the module-flag declaration
    /// emitted alongside it, plus the format-selection flag clang receives
    /// (`-g` for default-DWARF on Unix, `-gcodeview` for CodeView on
    /// Windows). See `debug_info_format()` for the selector.
    fn should_emit_dwarf(&self) -> bool {
        self.options.debug_info && self.options.source_file_path.is_some()
    }

    /// Which debug-info container the host linker will consume.
    ///
    /// On ELF + Mach-O targets, DWARF is the convention: `.debug_info` /
    /// `.debug_line` / `.debug_str` sections embedded directly in the
    /// object, resolved by `addr2line` / `dsymutil`.
    ///
    /// On Windows/COFF, the convention is CodeView: type and symbol
    /// records embedded in `.debug$S`/`.debug$T` sections of the object,
    /// which `link.exe /DEBUG` rolls up into a `.pdb` sidecar at link time.
    fn debug_info_format(&self) -> DebugInfoFormat {
        if self.is_windows_target() {
            DebugInfoFormat::CodeView
        } else {
            DebugInfoFormat::Dwarf
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
                        self.emit_line(&format!("  call void @kryos_free(ptr {cap_val})"));
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
                } else if expected_ty.starts_with('{')
                    || expected_ty.starts_with('%')
                    || expected_ty.starts_with('[')
                {
                    // Aggregate param (e.g. a closure `fn(Request) -> ...`): the
                    // caller boxed it into the i64 slot; unbox (inttoptr + load)
                    // and pass by value. Without this `call @fn(%Request <i64>)`
                    // mismatched.
                    let p = self.next_temp();
                    self.emit_line(&format!("  {p} = inttoptr i64 {raw} to ptr"));
                    let v = self.next_temp();
                    self.emit_line(&format!("  {v} = load {expected_ty}, ptr {p}"));
                    call_args.push(format!("{expected_ty} {v}"));
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
                } else if underlying_ret.starts_with('{')
                    || underlying_ret.starts_with('%')
                    || underlying_ret.starts_with('[')
                {
                    // Aggregate return (e.g. a closure `fn(Request) -> Response`):
                    // box on the heap and return the pointer as i64 through the
                    // uniform thunk ABI; the CallIndirect unboxes it. Without this
                    // `ret i64 %Response` mismatched the i64 thunk signature.
                    let size_ptr = self.next_temp();
                    self.emit_line(&format!(
                        "  {size_ptr} = getelementptr {underlying_ret}, ptr null, i32 1"
                    ));
                    let size_i64 = self.next_temp();
                    self.emit_line(&format!("  {size_i64} = ptrtoint ptr {size_ptr} to i64"));
                    let buf = self.next_temp();
                    self.emit_line(&format!(
                        "  {buf} = call ptr @kryos_arc_alloc(i64 {size_i64}, i64 8)"
                    ));
                    self.emit_line(&format!("  store {underlying_ret} {r}, ptr {buf}"));
                    let i = self.next_temp();
                    self.emit_line(&format!("  {i} = ptrtoint ptr {buf} to i64"));
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

    // -----------------------------------------------------------------------
    // Vtable dyn-thunks
    // -----------------------------------------------------------------------

    /// Emit `{method_name}_dyn(i64 self, i64 args...) -> i64` thunks for every
    /// trait method referenced from a vtable.  Each thunk:
    ///   1. inttoptr-converts the i64 self to a pointer to the concrete struct,
    ///   2. coerces each i64 user-arg to the underlying parameter's LLVM type,
    ///   3. calls the real method (forwarding aggregate self by `byval`),
    ///   4. widens the return value back to i64.
    /// This gives every method a uniform i64-only ABI suitable for indirect
    /// dispatch via the fat pointer, regardless of byval/sret usage.
    fn emit_vtable_thunks(&mut self) {
        // Collect unique method names across all vtables.
        let mut method_set: Vec<String> = Vec::new();
        for methods in self.trait_vtables.values() {
            for m in methods {
                if !method_set.iter().any(|x| x == m) {
                    method_set.push(m.clone());
                }
            }
        }

        for method_name in method_set {
            let param_types = match self.func_param_types.get(&method_name).cloned() {
                Some(v) => v,
                None => continue, // unknown method — skip
            };
            let ret_ty_llvm = self
                .func_ret_types
                .get(&method_name)
                .cloned()
                .unwrap_or_else(|| "void".to_string());
            let (ret_agg, param_aggs) = self
                .func_sig_aggs
                .get(&method_name)
                .cloned()
                .unwrap_or_else(|| (None, vec![None; param_types.len()]));

            let thunk_name = format!("{method_name}_dyn");
            self.emit_line(&format!("; Vtable dyn-thunk for {method_name}"));

            // Build parameter list: i64 self, i64 u1, i64 u2, ...
            let mut sig = String::new();
            for i in 0..param_types.len() {
                if !sig.is_empty() {
                    sig.push_str(", ");
                }
                sig.push_str(&format!("i64 %u{i}"));
            }
            self.emit_line(&format!(
                "define internal i64 @{thunk_name}({sig}) {{"
            ));
            self.emit_line("entry:");

            // Build the inner call's argument list, handling sret/byval ABI.
            let mut call_parts: Vec<String> = Vec::new();

            // If the underlying method returns an aggregate via sret, allocate
            // a slot and prepend a `ptr sret(%T) <slot>` arg.
            let sret_slot = if let Some(agg) = &ret_agg {
                let s = self.next_temp();
                self.emit_line(&format!("  {s} = alloca {agg}"));
                call_parts.push(format!("ptr sret({agg}) {s}"));
                Some((s, agg.clone()))
            } else {
                None
            };

            // Coerce each user-arg from i64 to its expected param ABI.
            for (i, p_ty) in param_types.iter().enumerate() {
                let u = format!("%u{i}");
                if let Some(agg) = param_aggs.get(i).and_then(|x| x.as_ref()) {
                    // Aggregate param: the i64 is a pointer to the aggregate.
                    let p = self.next_temp();
                    self.emit_line(&format!("  {p} = inttoptr i64 {u} to ptr"));
                    call_parts.push(format!("ptr byval({agg}) {p}"));
                } else {
                    // Scalar param: coerce i64 → p_ty.
                    let coerced = self.coerce_value(&u, "i64", p_ty);
                    call_parts.push(format!("{p_ty} {coerced}"));
                }
            }
            let arg_list = call_parts.join(", ");

            // Emit the call.  If the method uses sret, it returns void at the
            // ABI level — we load the aggregate, then widen its first slot to
            // i64 for the uniform dyn return.
            if let Some((slot, agg)) = sret_slot {
                self.emit_line(&format!(
                    "  call void @{method_name}({arg_list})"
                ));
                // Load the first i64 of the aggregate as a best-effort return.
                let loaded = self.next_temp();
                self.emit_line(&format!(
                    "  {loaded} = load {agg}, ptr {slot}"
                ));
                let first = self.next_temp();
                self.emit_line(&format!(
                    "  {first} = extractvalue {agg} {loaded}, 0"
                ));
                self.emit_line(&format!("  ret i64 {first}"));
            } else if ret_ty_llvm == "void" {
                self.emit_line(&format!(
                    "  call void @{method_name}({arg_list})"
                ));
                self.emit_line("  ret i64 0");
            } else {
                let r = self.next_temp();
                self.emit_line(&format!(
                    "  {r} = call {ret_ty_llvm} @{method_name}({arg_list})"
                ));
                let widened = self.coerce_value(&r, &ret_ty_llvm, "i64");
                self.emit_line(&format!("  ret i64 {widened}"));
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
            if name == "Map" {
                continue;
            }
            let drop_name = format!("__kryos_drop_{name}");
            // @copy structs are passed/copied by value with SHALLOW field
            // sharing (the LLVM aggregate copy duplicates the str/array handle
            // pointers, not the backing data). Their boxed bodies are also not
            // owned by any single copy. Freeing the fields OR the body in the
            // drop helper is therefore a use-after-free / invalid-free that a
            // sibling copy still references -- the dominant source of stage-1
            // codegen non-determinism (a freed Operand field is reused, so a
            // later read sees a garbage heap pointer). Matches the existing
            // "@copy structs share field pointers; the original owner frees"
            // design (drop_local for @copy is already a no-op). Emit an empty
            // helper so array/element drop call sites still link.
            if self.copy_structs.contains(name) {
                self.emit_line(&format!("define internal void @{drop_name}(ptr %ptr) {{"));
                self.emit_line("entry:");
                self.emit_line("  ret void");
                self.emit_line("}");
                self.emit_blank();
                continue;
            }
            if !has_heap_fields(fields) {
                // A struct with no heap fields needs no per-field drop work, but
                // it may still be used as an array/collection element whose drop
                // path calls `@__kryos_drop_<name>`. Emit an empty helper so that
                // reference links (the array buffer free reclaims the inline
                // element storage; matching the @copy no-op above).
                self.emit_line(&format!("define internal void @{drop_name}(ptr %ptr) {{"));
                self.emit_line("entry:");
                self.emit_line("  ret void");
                self.emit_line("}");
                self.emit_blank();
                continue;
            }
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
                            self.emit_line(&format!("  call void @kryos_free(ptr {fv})"));
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
                            self.emit_line(&format!("  call void @kryos_free(ptr {fv})"));
                        }
                    }
                    _ => {}
                }
            }

            self.emit_line("  call void @kryos_free(ptr %ptr)");
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
            self.emit_line("  call void @kryos_free(ptr %ptr)");
            self.emit_line("  ret void");
            self.emit_line("}");
            self.emit_blank();
        }
    }

    /// Emit per-struct "arc drop" helpers: `__kryos_arc_drop_<Struct>(ptr)`
    /// drops the struct's heap-owning fields (recursively releasing `Shared`
    /// children, dropping inline `Option<Shared>` payloads, etc.) WITHOUT
    /// freeing the struct allocation itself. These are registered as the
    /// `drop_fn` on `shared <struct>` arc blocks via `kryos_arc_set_drop`, so
    /// `kryos_arc_release` recurses: releasing a tree's root cascades through
    /// its children. (Distinct from `__kryos_drop_<Struct>`, which ALSO frees
    /// the struct box and is used for heap-boxed array elements -- reusing that
    /// here would double-free, since arc_release frees the block afterward.)
    /// Only emitted/used by code that uses `shared <struct>`; the self-host
    /// compiler does not, so this path cannot affect the bootstrap.
    fn emit_arc_drop_helpers(&mut self) {
        let dummy = MirFunction {
            name: String::new(),
            params: Vec::new(),
            ret_ty: MirType::I64,
            blocks: Vec::new(),
            locals: Vec::new(),
            attributes: MirAttributes::default(),
            source_file: None,
            source_line: 0,
        };
        let names: Vec<String> = self
            .struct_defs
            .keys()
            .filter(|n| n.as_str() != "Map")
            .cloned()
            .collect();
        for name in names {
            let drop_name = format!("__kryos_arc_drop_{name}");
            self.emit_line(&format!("define internal void @{drop_name}(ptr %ptr) {{"));
            self.emit_line("entry:");
            // emit_struct_drop drops the fields (no free); for @copy structs it
            // is a no-op-equivalent (shallow shared fields), which is correct
            // here since we never want to free the inline arc payload's box.
            if !self.copy_structs.contains(&name) {
                self.emit_struct_drop("%ptr", &name, &dummy);
            }
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

    /// Emit a tiny no-op function `@__kryos_dwarf_anchor` that carries
    /// a `DISubprogram` (!6) + `DILocation` (!8). LLVM strips orphan
    /// `!DICompileUnit` nodes — i.e. CUs not referenced by any
    /// instruction's `!dbg` — leaving only `.debug_frame`. Anchoring
    /// the CU on this dummy function preserves `.debug_info`,
    /// `.debug_line`, `.debug_str`, etc., so addr2line and gdb see
    /// the user's `.kry` source path.
    ///
    /// We retain the function via `@llvm.used` so opt/LTO cannot DCE it,
    /// which would also drop the metadata.
    fn emit_dwarf_anchor_fn(&mut self) {
        self.emit_blank();
        self.emit_line("; DWARF metadata anchor — keeps !DICompileUnit live.");
        self.emit_line("define internal void @__kryos_dwarf_anchor() !dbg !6 {");
        self.emit_line("entry:");
        self.emit_line("  ret void, !dbg !8");
        self.emit_line("}");
        self.emit_line(
            "@llvm.used = appending global [1 x ptr] [ptr @__kryos_dwarf_anchor], section \"llvm.metadata\"",
        );
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

    /// Walk an MIR Operand and append any referenced LocalId to `acc`.
    fn collect_operand(op: &Operand, acc: &mut HashSet<u32>) {
        if let Operand::Local(id) = op {
            acc.insert(id.0);
        }
    }

    /// Walk an MIR RValue's operands.
    fn collect_rvalue_locals(rv: &RValue, acc: &mut HashSet<u32>) {
        match rv {
            RValue::Use(op) => Self::collect_operand(op, acc),
            RValue::BinOp { left, right, .. } => {
                Self::collect_operand(left, acc);
                Self::collect_operand(right, acc);
            }
            RValue::UnOp { operand, .. } => Self::collect_operand(operand, acc),
            RValue::Call { args, .. } => {
                for a in args {
                    Self::collect_operand(a, acc);
                }
            }
            RValue::CallIndirect { callee, args } => {
                Self::collect_operand(callee, acc);
                for a in args {
                    Self::collect_operand(a, acc);
                }
            }
            RValue::Tuple(elems) | RValue::Array(elems) => {
                for e in elems {
                    Self::collect_operand(e, acc);
                }
            }
            RValue::Struct { fields, .. } => {
                for (_, op) in fields {
                    Self::collect_operand(op, acc);
                }
            }
            RValue::StringConcat(parts) => {
                for p in parts {
                    Self::collect_operand(p, acc);
                }
            }
            RValue::Field { object, .. } => Self::collect_operand(object, acc),
            RValue::Index { object, index } => {
                Self::collect_operand(object, acc);
                Self::collect_operand(index, acc);
            }
            RValue::Cast { operand, .. } => Self::collect_operand(operand, acc),
            // The variants below MUST be enumerated exhaustively: the escape
            // analysis in `compute_stackable_locals` treats "not referenced by
            // this rvalue" as "does not escape here". A missing operand-bearing
            // arm would let an array escape (into a map / enum / closure / &ref)
            // undetected and be wrongly stack-promoted -> use-after-return UB.
            // No blanket `_` arm: a future RValue variant must fail to compile
            // until it is handled here.
            RValue::Map(pairs) => {
                for (k, v) in pairs {
                    Self::collect_operand(k, acc);
                    Self::collect_operand(v, acc);
                }
            }
            RValue::EnumVariant { fields, .. } => {
                for op in fields {
                    Self::collect_operand(op, acc);
                }
            }
            RValue::EnumTag { operand } => Self::collect_operand(operand, acc),
            RValue::EnumPayload { operand, .. } => Self::collect_operand(operand, acc),
            RValue::Closure { captures, .. } => {
                for op in captures {
                    Self::collect_operand(op, acc);
                }
            }
            RValue::ArcAlloc { inner } => Self::collect_operand(inner, acc),
            RValue::Deref { operand } => Self::collect_operand(operand, acc),
            RValue::AddrOf { local, .. } => {
                acc.insert(local.0);
            }
            RValue::Range { start, end, .. } => {
                if let Some(s) = start {
                    Self::collect_operand(s, acc);
                }
                if let Some(e) = end {
                    Self::collect_operand(e, acc);
                }
            }
            RValue::Comptime(inner) => Self::collect_rvalue_locals(inner, acc),
            RValue::MakeTraitObject { value, .. } => Self::collect_operand(value, acc),
            RValue::VtableCall { object, args, .. } => {
                Self::collect_operand(object, acc);
                for op in args {
                    Self::collect_operand(op, acc);
                }
            }
            // Operand-free variants (no locals referenced).
            RValue::ConstInt(_)
            | RValue::ConstFloat(_)
            | RValue::ConstBool(_)
            | RValue::ConstString(_)
            | RValue::ConstNone => {}
        }
    }

    /// Walk an MIR Instruction's operands.
    fn collect_operand_locals(inst: &Instruction, acc: &mut HashSet<u32>) {
        match inst {
            Instruction::Assign { value, .. } => Self::collect_rvalue_locals(value, acc),
            Instruction::ArcRetain { ptr } | Instruction::ArcRelease { ptr } => {
                acc.insert(ptr.0);
            }
            Instruction::Drop { local } => {
                acc.insert(local.0);
            }
            Instruction::StoreField { object, value, .. } => {
                Self::collect_operand(object, acc);
                Self::collect_operand(value, acc);
            }
            Instruction::StoreDeref { ptr, value } => {
                Self::collect_operand(ptr, acc);
                Self::collect_operand(value, acc);
            }
            Instruction::Spawn { args, .. } => {
                for a in args {
                    Self::collect_operand(a, acc);
                }
            }
            Instruction::Send { channel, value } => {
                acc.insert(channel.0);
                acc.insert(value.0);
            }
            Instruction::Receive { channel, .. } => {
                acc.insert(channel.0);
            }
            Instruction::ActorSpawn { state, .. } => Self::collect_operand(state, acc),
            Instruction::ActorSend { actor, args, .. } => {
                acc.insert(actor.0);
                for a in args {
                    Self::collect_operand(a, acc);
                }
            }
            Instruction::ActorStateLoad { state_ptr, .. } => {
                acc.insert(state_ptr.0);
            }
            Instruction::ActorStateStore { state_ptr, value, .. } => {
                acc.insert(state_ptr.0);
                Self::collect_operand(value, acc);
            }
            Instruction::Nop => {}
        }
    }

    /// Walk an MIR Terminator's operands.
    fn collect_terminator_locals(term: &Terminator, acc: &mut HashSet<u32>) {
        match term {
            Terminator::Return(Some(op)) => Self::collect_operand(op, acc),
            Terminator::Branch { cond, .. } => Self::collect_operand(cond, acc),
            Terminator::Switch { value, .. } => {
                Self::collect_operand(value, acc);
            }
            _ => {}
        }
    }

    /// Compute the set of locals that are safe to stack-promote.
    ///
    /// A local L is stackable iff ALL of:
    ///   1. MirType::Array(_, Some(n)) with n <= 64.
    ///   2. Assigned exactly once via RValue::Array literal.
    ///   3. Every reference to L in any instruction is one of the strictly
    ///      allowed forms (see body). Any other reference = escape = not stackable.
    ///   4. No terminator references L.
    fn compute_stackable_locals(func: &MirFunction) -> HashSet<u32> {
        // Step 1: find candidates — fixed-size array locals with n <= 64
        // that are assigned exactly once with an Array literal.
        let mut candidates: HashSet<u32> = HashSet::new();
        // Track size n for each candidate.
        let mut cand_sizes: std::collections::HashMap<u32, u64> = std::collections::HashMap::new();

        // First pass: find singly-assigned Array-literal locals of fixed size.
        let mut assign_count: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        let mut is_array_literal: std::collections::HashMap<u32, bool> = std::collections::HashMap::new();

        for block in &func.blocks {
            for inst in &block.instructions {
                if let Instruction::Assign { dest, value: RValue::Array(_) } = inst {
                    *assign_count.entry(dest.0).or_insert(0) += 1;
                    is_array_literal.insert(dest.0, true);
                } else if let Instruction::Assign { dest, .. } = inst {
                    *assign_count.entry(dest.0).or_insert(0) += 1;
                }
            }
        }

        for local in &func.locals {
            let id = local.id.0;
            // Must be fixed-size array type.
            let n = match &local.ty {
                MirType::Array(_, Some(n)) if *n <= 64 => *n,
                _ => continue,
            };
            // Must be assigned exactly once with an Array literal.
            if assign_count.get(&id).copied() != Some(1) {
                continue;
            }
            if !is_array_literal.get(&id).copied().unwrap_or(false) {
                continue;
            }
            candidates.insert(id);
            cand_sizes.insert(id, n);
        }

        if candidates.is_empty() {
            return HashSet::new();
        }

        // Step 2: scan every instruction for references to each candidate.
        // For each reference, check it is in an allowed position; otherwise
        // remove the candidate from the stackable set.
        let mut disqualified: HashSet<u32> = HashSet::new();

        for block in &func.blocks {
            for inst in &block.instructions {
                // Collect ALL locals referenced by this instruction.
                let mut refs: HashSet<u32> = HashSet::new();
                Self::collect_operand_locals(inst, &mut refs);

                // For each candidate that appears in this instruction, verify
                // the instruction is in one of the strictly allowed forms.
                for &cand in refs.iter().filter(|id| candidates.contains(*id)) {
                    if disqualified.contains(&cand) {
                        continue;
                    }
                    let allowed = Self::is_allowed_array_use(inst, cand);
                    if !allowed {
                        disqualified.insert(cand);
                    }
                }
            }
            // No terminator may reference a candidate.
            let mut term_refs: HashSet<u32> = HashSet::new();
            Self::collect_terminator_locals(&block.terminator, &mut term_refs);
            for &cand in term_refs.iter().filter(|id| candidates.contains(*id)) {
                disqualified.insert(cand);
            }
        }

        candidates
            .into_iter()
            .filter(|id| !disqualified.contains(id))
            .collect()
    }

    /// Returns true if `inst` references local `cand` only in an allowed
    /// position for a stack-promoted array.  The allowed positions are:
    ///   - The defining `Assign { dest: cand, value: Array(_) }`
    ///   - Index read: `Assign { _, RValue::Index { object: Local(cand), index } }`
    ///     where index != cand.
    ///   - Array set: `Assign { _, RValue::Call { func: "kryos_array_set",
    ///     args: [Local(cand), idx, val] } }` where idx != cand and val != cand.
    ///   - Len: `Assign { _, RValue::Call { func: "len" | "kryos_array_len",
    ///     args: [Local(cand)] } }`
    ///   - Drop: `Instruction::Drop { local: cand }`
    ///   - ArcRetain/ArcRelease: `{ ptr: cand }`
    fn is_allowed_array_use(inst: &Instruction, cand: u32) -> bool {
        match inst {
            // Defining assignment: allowed.
            Instruction::Assign {
                dest,
                value: RValue::Array(_),
            } if dest.0 == cand => true,

            // Index read: allowed only if cand is the object, not the index.
            Instruction::Assign {
                value: RValue::Index { object, index },
                ..
            } => {
                // cand must appear only as object, not as index.
                let obj_is_cand = matches!(object, Operand::Local(id) if id.0 == cand);
                let idx_is_cand = matches!(index, Operand::Local(id) if id.0 == cand);
                // If cand is referenced here, it must be the object only.
                obj_is_cand && !idx_is_cand
            }

            // kryos_array_set: allowed only if cand is args[0].
            Instruction::Assign {
                value: RValue::Call { func, args },
                ..
            } if func == "kryos_array_set" => {
                // args[0] must be cand; args[1] and args[2] must not be cand.
                let arg0_is_cand = args
                    .first()
                    .map(|a| matches!(a, Operand::Local(id) if id.0 == cand))
                    .unwrap_or(false);
                let other_args_cand = args
                    .iter()
                    .skip(1)
                    .any(|a| matches!(a, Operand::Local(id) if id.0 == cand));
                arg0_is_cand && !other_args_cand
            }

            // len / kryos_array_len: allowed if the single arg is cand.
            Instruction::Assign {
                value: RValue::Call { func, args },
                ..
            } if func == "len" || func == "kryos_array_len" => {
                args.len() == 1
                    && matches!(args[0], Operand::Local(id) if id.0 == cand)
            }

            // Drop: allowed (we suppress the free).
            Instruction::Drop { local } if local.0 == cand => true,

            // ArcRetain/ArcRelease: allowed (we suppress).
            Instruction::ArcRetain { ptr } | Instruction::ArcRelease { ptr }
                if ptr.0 == cand =>
            {
                true
            }

            // Any other instruction that references cand = escape.
            _ => false,
        }
    }

    fn emit_function_as(&mut self, func: &MirFunction, name: &str) -> Result<(), CodegenError> {
        // Compute stack-promotable locals before any emission so we can gate
        // on `self.stackable_locals` inside emit_aggregate_array, Drop, etc.
        self.stackable_locals = Self::compute_stackable_locals(func);

        let fn_text_start = self.output.len();
        self.cur_fn_has_mir_exception_checks = func.blocks.iter().any(|bb| {
            bb.instructions.iter().any(|inst| {
                matches!(inst, Instruction::Assign { value: RValue::Call { func, .. }, .. }
                    if func == "kryos_exception_check")
            })
        });
        // Build the local type map for this function.
        self.local_types.clear();
        self.value_types.clear();
        for local in &func.locals {
            let llvm_ty = match &local.ty {
                MirType::Enum(name) => {
                    let max = self.enum_max_fields(name);
                    self.enum_llvm_type(name, max)
                }
                // A `Struct(name)` that actually names an ENUM (array-element and
                // some inferred locals carry an enum value as Struct) must render
                // as the enum aggregate `{ i64, <payloads> }`, not the bare named
                // `%name` -- which is never declared, so `load %E` / `extractvalue
                // %E` fail ("not a first class type"). enum_tag/enum_payload and
                // the array-element unbox all read this via operand_type.
                MirType::Struct(name) if self.enum_defs.contains_key(name) => {
                    let max = self.enum_max_fields(name);
                    self.enum_llvm_type(name, max)
                }
                other => mir_type_to_llvm(other),
            };
            self.local_types.insert(local.id.0, llvm_ty);
        }

        // Detect which locals need alloca/store/load: mutable, assigned >1 time,
        // OR used in a block other than the one they are defined in. The last
        // case is required for SSA validity -- a directly-named `%_N` value
        // defined in block A but used in block B that A does not dominate fails
        // LLVM's "instruction does not dominate all uses" (e.g. a value produced
        // in one match arm and consumed after the merge). Spilling it to an
        // `%_N.addr` alloca (store at def, load at each use) sidesteps dominance.
        self.mutable_locals.clear();
        let mut assign_counts: HashMap<u32, u32> = HashMap::new();
        let mut def_block: HashMap<u32, usize> = HashMap::new();
        let mut use_blocks: HashMap<u32, HashSet<usize>> = HashMap::new();
        for (bi, block) in func.blocks.iter().enumerate() {
            for inst in &block.instructions {
                if let Instruction::Assign { dest, .. } = inst {
                    *assign_counts.entry(dest.0).or_insert(0) += 1;
                    def_block.entry(dest.0).or_insert(bi);
                }
                let mut used = HashSet::new();
                Self::collect_operand_locals(inst, &mut used);
                for u in used {
                    use_blocks.entry(u).or_default().insert(bi);
                }
            }
            let mut tused = HashSet::new();
            Self::collect_terminator_locals(&block.terminator, &mut tused);
            for u in tused {
                use_blocks.entry(u).or_default().insert(bi);
            }
        }
        for local in &func.locals {
            let id = local.id.0;
            let count = assign_counts.get(&id).copied().unwrap_or(0);
            let cross_block = match (def_block.get(&id), use_blocks.get(&id)) {
                (Some(&db), Some(ubs)) => ubs.iter().any(|&ub| ub != db),
                _ => false,
            };
            if local.mutable || count > 1 || cross_block {
                self.mutable_locals.insert(id);
            }
        }
        // Aggregate (byval) parameters always get an alloca: params have no
        // body Assign so they never qualify above, and StoreField on a
        // non-mutable aggregate falls to the invalid `inttoptr %AggType`
        // path (param field mutation failed to compile). The alloca also
        // prevents an SSA double-definition when the body reassigns the
        // whole param (`p = S{..}` after the byval entry load).
        for p in &func.params {
            if self.aggregate_llvm_ty(&p.ty).is_some() {
                self.mutable_locals.insert(p.local.0);
            }
        }
        // Same for ANY aggregate-typed local that is the object of a
        // StoreField — e.g. the MIR inliner turns a callee's mutated param
        // into a single-assign temp copy, which the count-based rule above
        // leaves immutable; the store path then emits `inttoptr %AggType`.
        for block in &func.blocks {
            for inst in &block.instructions {
                if let Instruction::StoreField {
                    object: Operand::Local(id),
                    ..
                } = inst
                {
                    let is_agg = func
                        .locals
                        .iter()
                        .find(|l| l.id == *id)
                        .is_some_and(|l| self.aggregate_llvm_ty(&l.ty).is_some());
                    if is_agg {
                        self.mutable_locals.insert(id.0);
                    }
                }
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

        // Per-function DISubprogram emission. Each user function gets a
        // DISubprogram so addr2line and debugger backtraces resolve to
        // Kryos source file:line. We attach `!dbg !<dbg_md>` to the
        // `define` header and a matching `!dbg !<loc_md>` to every call
        // and ret inside via `dbg_suffix()`.
        let dbg_suffix_for_define: String =
            if self.should_emit_dwarf() {
                self.emitted_function_names.push(name.to_string());
                self.emitted_function_lines.push(func.source_line.max(1));
                // Reserve two consecutive metadata ids per function: one for
                // the DISubprogram and one for its DILocation. We compute
                // their final ids in `emit_dwarf_metadata`, but we need a
                // forward reference here. Encode as `subprogram_id`.
                // Layout (matches emit_dwarf_metadata):
                //   per function k (0-indexed in emitted_function_names):
                //     subprogram_md_id = 100 + 2*k
                //     location_md_id   = 100 + 2*k + 1
                let k = self.emitted_function_names.len() - 1;
                let sub_id = 100 + 2 * k as u32;
                let loc_id = sub_id + 1;
                self.current_fn_dbg_md = Some(sub_id);
                self.current_fn_loc_md = Some(loc_id);
                format!(" !dbg !{}", sub_id)
            } else {
                self.current_fn_dbg_md = None;
                self.current_fn_loc_md = None;
                String::new()
            };
        // User functions get `internal` linkage so their names cannot collide
        // with libc / system DLL symbols. Without this, Kryos stdlib names
        // like `connect`, `bind`, `exit`, `read`, `write` (all valid Kryos
        // function names) clash with the C runtime at link time:
        //   error LNK2005: connect already defined in foo.o
        //                  (ws2_32.dll already exports `connect`)
        // `main` is the program entry point and must stay external. Everything
        // else is module-local; the Kryos compiler emits the whole program
        // as a single LLVM module so internal linkage doesn't break cross-fn
        // references.
        let linkage = if name == "main" { "" } else { "internal " };
        self.emit_line(&format!(
            "define {linkage}{ret} @{name}({params}){dbg_suffix_for_define} {{"
        ));

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
        // Emit stack header + data allocas for stack-promotable array locals.
        // Layout mirrors the heap KryosArray header so the existing inline
        // get/set helpers work unchanged:
        //   { i64 len, i64 cap, i64 elem_size, i64 ref_count, ptr data }
        // elem_size is always 8 (all Kryos values are i64-sized slots).
        // ref_count is set to 1_000_000 so an accidental arc_release is a
        // no-op decrement; but we also suppress Drop/ArcRelease for these
        // locals, so the sentinel should never be hit in practice.
        {
            // Collect stackable locals in stable order for deterministic IR.
            let mut stack_locals: Vec<(u32, u64)> = Vec::new();
            for local in &func.locals {
                if let MirType::Array(_, Some(n)) = &local.ty {
                    if self.stackable_locals.contains(&local.id.0) {
                        stack_locals.push((local.id.0, *n));
                    }
                }
            }
            for (id, n) in stack_locals {
                // Emit: %_N.stk_hdr = alloca { i64, i64, i64, i64, ptr }
                //        %_N.stk_dat = alloca [n x i64]
                self.emit_line(&format!(
                    "  %_{id}.stk_hdr = alloca {{ i64, i64, i64, i64, ptr }}"
                ));
                self.emit_line(&format!(
                    "  %_{id}.stk_dat = alloca [{n} x i64]"
                ));
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

        // Defensive zero-init pass for locals referenced as Operands but
        // never bound via Instruction::Assign and not declared as a param.
        // The MIR layer occasionally elides a producing Cast (observed in
        // Command__arg / test_process: local 3 referenced as i64 form of
        // .arguments without an emit_assign generating it, and not present
        // in func.locals either). Without this pre-init, the LLVM IR uses
        // `%_N` undef and clang rejects the IR with "use of undefined
        // value '%_N'".
        //
        // Zero-init is the LEAST-WRONG behavior: it lets the IR link and
        // run; tests relying on the elided value will fail loudly at
        // runtime instead of a misleading link-time error that masks the
        // underlying MIR bug.
        let param_ids_set: HashSet<u32> =
            func.params.iter().map(|p| p.local.0).collect();
        let mut assigned: HashSet<u32> = HashSet::new();
        let mut referenced: HashSet<u32> = HashSet::new();
        for block in &func.blocks {
            for inst in &block.instructions {
                if let Instruction::Assign { dest, .. } = inst {
                    assigned.insert(dest.0);
                }
                Self::collect_operand_locals(inst, &mut referenced);
            }
            Self::collect_terminator_locals(&block.terminator, &mut referenced);
        }
        // Build the union of `referenced` and `func.locals` ids — local 3 in
        // the test_process bug is referenced but not in func.locals.
        let mut candidate_ids: Vec<u32> = referenced.iter().copied().collect();
        for local in &func.locals {
            candidate_ids.push(local.id.0);
        }
        candidate_ids.sort_unstable();
        candidate_ids.dedup();
        for id in candidate_ids {
            if param_ids_set.contains(&id) {
                continue;
            }
            if self.mutable_locals.contains(&id) {
                continue;
            }
            if assigned.contains(&id) {
                continue;
            }
            let ty = self
                .local_types
                .get(&id)
                .cloned()
                .unwrap_or_else(|| "i64".to_string());
            let init = match ty.as_str() {
                "void" => continue,
                "ptr" => format!("  %_{id} = inttoptr i64 0 to ptr"),
                "double" => format!("  %_{id} = fadd double 0.0, 0.0"),
                "float" => format!("  %_{id} = fadd float 0.0, 0.0"),
                "i1" => format!("  %_{id} = icmp ne i64 0, 0"),
                t if t.starts_with('{') => {
                    format!("  %_{id} = select i1 true, {t} undef, {t} undef")
                }
                t if t.starts_with('%') => {
                    format!("  %_{id} = select i1 true, {t} undef, {t} undef")
                }
                t => format!("  %_{id} = add {t} 0, 0"),
            };
            self.emit_line(&init);
            self.track_type(&format!("%_{id}"), &ty);
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

        // Clear DI tracking so subsequent module-level helpers (closure
        // thunks, drop helpers, etc.) emit without auto-tagging calls.
        self.current_fn_dbg_md = None;
        self.current_fn_loc_md = None;

        // LLVM only treats ENTRY-BLOCK allocas as static frame slots; an
        // alloca in a loop body bumps the stack pointer on every iteration
        // and releases only at function return, so a hot loop that spills an
        // aggregate (e.g. for a struct drop) grows the stack unboundedly --
        // a 65k-iteration loop was measured eating megabytes, and ~250k
        // iterations would overflow the 8 MB Windows stack outright. Hoist
        // every static alloca in this function's body into the entry block
        // (the slot is then reused per iteration; every spill stores before
        // it reads, so reuse is safe). Dynamically-sized allocas reference
        // SSA operands and cannot move.
        let hoisted = Self::hoist_static_allocas(&self.output[fn_text_start..]);
        self.output.truncate(fn_text_start);
        self.output.push_str(&hoisted);

        Ok(())
    }

    /// Move static `alloca`s that appear after the first basic-block label
    /// up into the function's entry block. `text` must contain exactly one
    /// `define ... {` function.
    fn hoist_static_allocas(text: &str) -> String {
        let mut entry: Vec<&str> = Vec::new();
        let mut body: Vec<&str> = Vec::new();
        let mut hoist: Vec<&str> = Vec::new();
        let mut seen_define = false;
        let mut seen_label = false;
        for line in text.lines() {
            if !seen_define {
                entry.push(line);
                if line.starts_with("define ") {
                    seen_define = true;
                }
                continue;
            }
            if !seen_label && !line.starts_with(' ') && line.trim_end().ends_with(':') {
                seen_label = true;
            }
            if seen_label {
                let trimmed = line.trim_start();
                if let Some(rest) = trimmed
                    .split_once(" = alloca ")
                    .map(|(_, r)| r)
                    .filter(|_| trimmed.starts_with('%'))
                {
                    // Static allocas only: a dynamic count (`alloca i8, i64 %n`)
                    // references an SSA value and must stay put.
                    let dynamic = rest
                        .rsplit_once(',')
                        .map(|(_, tail)| tail.contains('%'))
                        .unwrap_or(false)
                        && !rest.trim_start().starts_with('%');
                    // (`alloca %Struct` has '%' as the TYPE; only a trailing
                    // `, <ty> %reg` operand makes it dynamic.)
                    if !dynamic {
                        hoist.push(line);
                        continue;
                    }
                }
                body.push(line);
            } else {
                entry.push(line);
            }
        }
        // Place the hoisted allocas at the TOP of the entry block. When the
        // body opens with instructions (implicit entry block), that is right
        // after the `define` line. When it opens with a label (`bb0:` IS the
        // entry block), allocas must go after that label -- emitting them
        // between `define` and the label would create an implicit entry
        // block with no terminator, which the IR parser rejects.
        let entry_has_insts = entry
            .iter()
            .skip_while(|l| !l.starts_with("define "))
            .skip(1)
            .any(|l| !l.trim().is_empty());
        let mut out = String::with_capacity(text.len());
        let mut pending = !hoist.is_empty();
        for line in &entry {
            out.push_str(line);
            out.push('\n');
            if pending && entry_has_insts && line.starts_with("define ") {
                for h in &hoist {
                    out.push_str(h);
                    out.push('\n');
                }
                pending = false;
            }
        }
        for line in &body {
            out.push_str(line);
            out.push('\n');
            if pending && !line.starts_with(' ') && line.trim_end().ends_with(':') {
                for h in &hoist {
                    out.push_str(h);
                    out.push('\n');
                }
                pending = false;
            }
        }
        out
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
                // For functions WITHOUT MIR-level exception handling, check
                // the thread-local exception state after every user function
                // call and return early to propagate the unwind toward the
                // nearest try/catch up the call stack (mirrors the Cranelift
                // backend; without this an out-of-try `throw` in a callee is
                // silently ignored and execution continues).
                if self.module_can_throw
                    && !self.cur_fn_has_mir_exception_checks
                    && post_call_exception_check_applies(value)
                {
                    self.emit_post_call_exception_check(func);
                }
            }
            Instruction::ArcRetain { ptr } => {
                // Stack-promoted arrays must not be retain'd (they are not refcounted).
                if !self.stackable_locals.contains(&ptr.0) {
                    self.emit_line(&format!("  call void @kryos_arc_retain(ptr %_{})", ptr.0));
                }
            }
            Instruction::ArcRelease { ptr } => {
                // Stack-promoted arrays must not be release'd (they are stack memory).
                if !self.stackable_locals.contains(&ptr.0) {
                    self.emit_line(&format!("  call void @kryos_arc_release(ptr %_{})", ptr.0));
                }
            }
            Instruction::Drop { local } => {
                // Stack-promoted arrays are freed by unwinding the stack frame;
                // do NOT call kryos_array_free on them.
                if self.stackable_locals.contains(&local.0) {
                    return Ok(());
                }
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
                        // A map local is already an i64 handle (MirType::Map -> i64),
                        // so pass it straight to free; ptrtoint-ing it would treat an
                        // i64 as a ptr and fail LLVM verification.
                        self.emit_line(&format!("  call void @kryos_map_free(i64 {val})"));
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
                                self.emit_line(&format!("  call void @kryos_free(ptr {val})"));
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
                            Some(t @ (MirType::Struct(_) | MirType::Tuple(_) | MirType::Enum(_))) => {
                                // Box an aggregate capture: heap-copy the struct/
                                // tuple/enum and store the pointer as i64 in the
                                // spawn env. The wrapper unboxes it (inttoptr+load).
                                // Without this, `store i64 %Router` mismatched the
                                // %Router aggregate value.
                                let agg = self.sig_ty_to_llvm(t);
                                let size_ptr = self.next_temp();
                                self.emit_line(&format!(
                                    "  {size_ptr} = getelementptr {agg}, ptr null, i32 1"
                                ));
                                let size_i64 = self.next_temp();
                                self.emit_line(&format!(
                                    "  {size_i64} = ptrtoint ptr {size_ptr} to i64"
                                ));
                                let buf = self.next_temp();
                                self.emit_line(&format!(
                                    "  {buf} = call ptr @kryos_arc_alloc(i64 {size_i64}, i64 8)"
                                ));
                                self.emit_line(&format!("  store {agg} {val}, ptr {buf}"));
                                let t2 = self.next_temp();
                                self.emit_line(&format!("  {t2} = ptrtoint ptr {buf} to i64"));
                                t2
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
                let val = self.operand_to_llvm(value, func);
                let field_idx = self.resolve_field_index(object, field, func);

                // Compute the base pointer of the struct. A by-value aggregate
                // (struct/tuple) local lives in its own `%_N.addr` alloca; GEP
                // into that directly. `operand_to_llvm` would instead LOAD the
                // aggregate VALUE, which then cannot be inttoptr'd (it is not an
                // integer) — the cause of invalid `inttoptr %Struct ... to ptr`
                // IR that clang rejects. Heap structs (i64 handle) and ptr-typed
                // objects still go through the load + inttoptr path.
                let ptr_tmp = match object {
                    Operand::Local(id)
                        if self.mutable_locals.contains(&id.0) && {
                            let lt = self.local_type(*id);
                            lt.starts_with('%') || lt.starts_with('{')
                        } =>
                    {
                        format!("%_{}.addr", id.0)
                    }
                    _ => {
                        let obj_val = self.operand_to_llvm(object, func);
                        let obj_ty = self.operand_type(object, func);
                        // Coerce object to ptr if not already. Treat void-typed
                        // values as already-ptr (from runtime calls returning ptr).
                        if obj_ty == "ptr" || obj_ty == "void" {
                            obj_val
                        } else {
                            let tmp = self.next_temp();
                            self.emit_line(&format!(
                                "  {tmp} = inttoptr {obj_ty} {obj_val} to ptr"
                            ));
                            tmp
                        }
                    }
                };
                // Address the field. When we know the struct type, use a
                // struct-INDEXED GEP (`getelementptr %S, ptr, i32 0, i32 idx`)
                // so the byte offset honours the real field layout. The old
                // i64-stride GEP (`getelementptr i64, ptr, i32 idx`) assumed
                // every field is 8 bytes and silently mis-addressed any field
                // declared AFTER a >8-byte aggregate field (tuple/nested struct)
                // -- a real AOT miscompile. For all-8-byte structs the two GEPs
                // yield identical offsets, so this is a no-op there.
                let struct_name = self.resolve_struct_name(object, func);
                let field_llvm_ty: Option<String> = struct_name
                    .as_ref()
                    .and_then(|n| self.struct_defs.get(n))
                    .and_then(|fs| fs.get(field_idx))
                    .map(|(_, t)| t.clone())
                    .map(|t| self.sig_ty_to_llvm(&t));
                let field_ptr = self.next_temp();
                match (&struct_name, &field_llvm_ty) {
                    (Some(sn), Some(fty)) => {
                        self.emit_line(&format!(
                            "  {field_ptr} = getelementptr %{sn}, ptr {ptr_tmp}, i32 0, i32 {field_idx}"
                        ));
                        // Aggregate fields ({..}/%Name/[..]) are wider than 8 bytes,
                        // so store the full value with its real type. Scalar 8-byte
                        // fields (i64/ptr/double/...) keep the opaque `store i64`
                        // (same 8 bytes; avoids re-typing proven scalar stores).
                        if fty.starts_with('{') || fty.starts_with('%') || fty.starts_with('[') {
                            let val_ty = self.operand_type(value, func);
                            let coerced = self.coerce_value(&val, &val_ty, fty);
                            self.emit_line(&format!("  store {fty} {coerced}, ptr {field_ptr}"));
                        } else {
                            // The slot is stored as i64 but the value's SSA type
                            // may be ptr (str constant), double, or i1 — coerce
                            // to the slot first (`store i64 <ptr>` is invalid IR).
                            let val_ty = self
                                .actual_type(&val)
                                .unwrap_or_else(|| self.operand_type(value, func));
                            let val_i64 = if val_ty == "i64" {
                                val.clone()
                            } else {
                                self.coerce_value(&val, &val_ty, "i64")
                            };
                            self.emit_line(&format!("  store i64 {val_i64}, ptr {field_ptr}"));
                        }
                    }
                    _ => {
                        // Unknown struct (heap handle without a resolvable type):
                        // fall back to the legacy i64-stride store.
                        self.emit_line(&format!(
                            "  {field_ptr} = getelementptr i64, ptr {ptr_tmp}, i32 {field_idx}"
                        ));
                        let val_ty = self
                            .actual_type(&val)
                            .unwrap_or_else(|| self.operand_type(value, func));
                        let val_i64 = if val_ty == "i64" {
                            val.clone()
                        } else {
                            self.coerce_value(&val, &val_ty, "i64")
                        };
                        self.emit_line(&format!("  store i64 {val_i64}, ptr {field_ptr}"));
                    }
                }
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
                if actual_ty == agg_ty {
                    // Value is the aggregate by value: spill to a byval buffer.
                    let buf = self.next_temp();
                    self.emit_line(&format!("  {buf} = alloca {agg_ty}"));
                    self.emit_line(&format!("  store {agg_ty} {val}, ptr {buf}"));
                    arg_parts.push(format!("ptr byval({agg_ty}) {buf}"));
                } else if actual_ty.starts_with('%')
                    || actual_ty.starts_with('{')
                    || actual_ty.starts_with('[')
                {
                    // Aggregate VALUE under a different type spelling (named
                    // %Parser vs the literal body — layouts identical): spill
                    // with the value's own type; the byval annotation keeps
                    // the callee-declared type. The boxed-handle branch below
                    // would emit `inttoptr %Agg` (invalid IR).
                    let buf = self.next_temp();
                    self.emit_line(&format!("  {buf} = alloca {actual_ty}"));
                    self.emit_line(&format!("  store {actual_ty} {val}, ptr {buf}"));
                    arg_parts.push(format!("ptr byval({agg_ty}) {buf}"));
                } else {
                    // Value is a boxed handle (i64/ptr) -- e.g. a struct captured
                    // into a spawn env i64 slot. The box IS a pointer to the
                    // aggregate; pass it directly as byval (the callee copies).
                    // Without this, `store %Agg <i64>` mismatched (`i64 but
                    // expected %Router`).
                    let p = if actual_ty == "ptr" {
                        val
                    } else {
                        let t = self.next_temp();
                        self.emit_line(&format!("  {t} = inttoptr {actual_ty} {val} to ptr"));
                        t
                    };
                    arg_parts.push(format!("ptr byval({agg_ty}) {p}"));
                }
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
                let mut coerced = self.coerce_value(&val, &val_ty, &effective_dest_ty);
                // @copy struct with heap fields: clone the heap fields so each
                // copy owns its own data — same semantics as the Cranelift
                // backend's deep copy on `let c = b` (gotcha #23 unification).
                if let Operand::Local(src_id) = op {
                    coerced = self.maybe_deep_copy_struct_fields(
                        &coerced,
                        *src_id,
                        func,
                        &effective_dest_ty,
                    );
                }
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
                } else if is_string
                    && matches!(
                        op,
                        MirBinOp::Lt | MirBinOp::Gt | MirBinOp::LtEq | MirBinOp::GtEq
                    )
                {
                    // String ordering: kryos_string_compare(a, b) -> -1/0/+1,
                    // then an integer compare against 0. Without this arm the
                    // generic path compared the HANDLE POINTERS ("a" < "b"
                    // was whichever allocated lower — Cranelift has had the
                    // dispatch since v1).
                    let left_val = self.operand_to_llvm(left, func);
                    let left_ty = self.operand_type(left, func);
                    let right_val = self.operand_to_llvm(right, func);
                    let right_ty = self.operand_type(right, func);
                    let left_ptr = self.coerce_value(&left_val, &left_ty, "ptr");
                    let right_ptr = self.coerce_value(&right_val, &right_ty, "ptr");
                    let cmp = self.next_temp();
                    self.emit_line(&format!(
                        "  {cmp} = call i64 @kryos_string_compare(ptr {left_ptr}, ptr {right_ptr})"
                    ));
                    let pred = match op {
                        MirBinOp::Lt => "slt",
                        MirBinOp::Gt => "sgt",
                        MirBinOp::LtEq => "sle",
                        _ => "sge",
                    };
                    let res = if is_mutable {
                        self.next_temp()
                    } else {
                        format!("%_{}", dest.0)
                    };
                    self.emit_line(&format!("  {res} = icmp {pred} i64 {cmp}, 0"));
                    self.track_type(&res, "i1");
                    if is_mutable {
                        self.emit_line(&format!("  store i1 {res}, ptr %_{}.addr", dest.0));
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
                    let mut operand_ty = self.operand_type(left, func);
                    let mut right_ty = self.operand_type(right, func);
                    // Mixed integer widths widen to the WIDEST side (never
                    // truncate): coercing an i64 mask constant down to the
                    // i8 of a u8 operand made `x & 255` a no-op and defined
                    // the dest at the wrong width vs its declared i64 local.
                    let int_w = |t: &str| -> Option<u8> {
                        match t {
                            "i8" => Some(1),
                            "i16" => Some(2),
                            "i32" => Some(3),
                            "i64" => Some(4),
                            _ => None,
                        }
                    };
                    if !is_float {
                        if let (Some(lw), Some(rw)) = (int_w(&operand_ty), int_w(&right_ty)) {
                            if lw != rw {
                                let wide = if lw > rw { operand_ty.clone() } else { right_ty.clone() };
                                if operand_ty != wide {
                                    left_val = self.coerce_value(&left_val, &operand_ty, &wide);
                                    operand_ty = wide.clone();
                                }
                                if right_ty != wide {
                                    right_val = self.coerce_value(&right_val, &right_ty, &wide);
                                    right_ty = wide;
                                }
                            }
                        }
                    }
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
                            // Bring result to dest_ty before store; widening/narrowing
                            // mismatch (e.g. i64 -> i32 local) would otherwise emit
                            // a verifier error.
                            let coerced = self.coerce_value(&tmp, &operand_ty_w, &dest_ty);
                            self.emit_line(&format!("  store {dest_ty} {coerced}, ptr %_{}.addr", dest.0));
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
                        // Operand-type result may differ from dest_ty (e.g. an i64
                        // subtraction stored into an i32 local). Coerce before
                        // storing so the verifier accepts the IR.
                        let coerced = self.coerce_value(&tmp, &operand_ty, &dest_ty);
                        self.emit_line(&format!("  store {dest_ty} {coerced}, ptr %_{}.addr", dest.0));
                    } else {
                        // Immutable dest: when the (possibly widened) op type
                        // differs from the declared local type, compute into a
                        // temp and define %_N at dest_ty — otherwise call
                        // sites that pass %_N at its declared width emit
                        // mismatched IR. Comparisons always produce i1 and
                        // match a bool dest.
                        let is_cmp = matches!(
                            op,
                            MirBinOp::Eq
                                | MirBinOp::Neq
                                | MirBinOp::Lt
                                | MirBinOp::Gt
                                | MirBinOp::LtEq
                                | MirBinOp::GtEq
                        );
                        if !is_cmp && operand_ty != dest_ty {
                            let tmp = self.next_temp();
                            self.emit_binop_to(&tmp, *op, &left_val, &right_val, &operand_ty, is_float)?;
                            let coerced = self.coerce_value(&tmp, &operand_ty, &dest_ty);
                            let name = format!("%_{}", dest.0);
                            self.emit_identity_copy(&name, &dest_ty, &coerced);
                        } else {
                            self.emit_binop(dest, *op, &left_val, &right_val, &operand_ty, is_float)?;
                        }
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
                        // Inlined array write: redirect to the alwaysinline helper,
                        // reusing the already-coerced (ptr, i64, i64) arg_list (the
                        // value is the raw i64 slot -- floats already bitcast above).
                        "kryos_array_set" => {
                            self.emit_line(&format!(
                                "  call void @__kryos_array_set_inline({arg_list})"
                            ));
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
                                // Already a string handle. `to_string<T=str>`
                                // is the identity — clone the string so the
                                // caller can own + drop the result without
                                // touching the original. (kryos_string_clone
                                // makes a fresh KryosString with its own
                                // refcount.) Short-circuit out of the shared
                                // post-match logic — that path emits with
                                // i64 return type, but kryos_string_clone
                                // returns ptr.
                                let cloned = self.next_temp();
                                self.emit_line(&format!(
                                    "  {cloned} = call ptr @kryos_string_clone(ptr {val})"
                                ));
                                if is_mutable {
                                    if dest_ty == "ptr" {
                                        self.emit_line(&format!(
                                            "  store ptr {cloned}, ptr %_{}.addr",
                                            dest.0
                                        ));
                                    } else {
                                        let as_i64 = self.next_temp();
                                        self.emit_line(&format!(
                                            "  {as_i64} = ptrtoint ptr {cloned} to i64"
                                        ));
                                        self.emit_line(&format!(
                                            "  store i64 {as_i64}, ptr %_{}.addr",
                                            dest.0
                                        ));
                                    }
                                } else if dest_ty == "ptr" {
                                    self.emit_line(&format!(
                                        "  %_{} = getelementptr i8, ptr {cloned}, i64 0",
                                        dest.0
                                    ));
                                } else {
                                    self.emit_line(&format!(
                                        "  %_{} = ptrtoint ptr {cloned} to i64",
                                        dest.0
                                    ));
                                }
                                return Ok(());
                            } else {
                                // Integer types: coerce to i64 if needed.
                                // Unsigned narrow ints ZERO-extend — the
                                // coerce_value sext arm would print u8 200
                                // as -56. (U8/U16/U32 all render as iN type
                                // strings, so signedness must come from the
                                // MIR local type, not the LLVM type string.)
                                let is_unsigned_narrow = matches!(&args[0], Operand::Local(id)
                                    if func.locals.iter().find(|l| l.id == *id).is_some_and(|l| {
                                        matches!(
                                            l.ty,
                                            MirType::U8 | MirType::U16 | MirType::U32
                                        )
                                    }));
                                let coerced = if is_unsigned_narrow
                                    && matches!(arg_ty.as_str(), "i8" | "i16" | "i32")
                                {
                                    let tmp = self.next_temp();
                                    self.emit_line(&format!(
                                        "  {tmp} = zext {arg_ty} {val} to i64"
                                    ));
                                    self.track_type(&tmp, "i64");
                                    tmp
                                } else {
                                    self.coerce_value(&val, &arg_ty, "i64")
                                };
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
                            let dest_ty = self.local_type(dest);
                            if dest_ty.starts_with('{')
                                || dest_ty.starts_with('%')
                                || dest_ty.starts_with('[')
                            {
                                // Aggregate element: the array slot holds a boxed
                                // pointer (push boxes aggregates); unbox via
                                // inttoptr + load, mirroring the index path.
                                let raw = self.next_temp();
                                self.emit_line(&format!(
                                    "  {raw} = call i64 @kryos_builtin_pop(i64 {arr_val})"
                                ));
                                let p = self.next_temp();
                                self.emit_line(&format!("  {p} = inttoptr i64 {raw} to ptr"));
                                if is_mutable {
                                    let v = self.next_temp();
                                    self.emit_line(&format!("  {v} = load {dest_ty}, ptr {p}"));
                                    self.emit_line(&format!(
                                        "  store {dest_ty} {v}, ptr %_{}.addr",
                                        dest.0
                                    ));
                                } else {
                                    self.emit_line(&format!(
                                        "  %_{} = load {dest_ty}, ptr {p}",
                                        dest.0
                                    ));
                                }
                            } else if dest_ty == "double" || dest_ty == "ptr" {
                                // Scalar non-i64 element: the slot carries raw
                                // bits; reinterpret (bitcast / inttoptr).
                                let raw = self.next_temp();
                                self.emit_line(&format!(
                                    "  {raw} = call i64 @kryos_builtin_pop(i64 {arr_val})"
                                ));
                                let conv = if dest_ty == "double" {
                                    format!("bitcast i64 {raw} to double")
                                } else {
                                    format!("inttoptr i64 {raw} to ptr")
                                };
                                if is_mutable {
                                    let v = self.next_temp();
                                    self.emit_line(&format!("  {v} = {conv}"));
                                    self.emit_line(&format!(
                                        "  store {dest_ty} {v}, ptr %_{}.addr",
                                        dest.0
                                    ));
                                } else {
                                    self.emit_line(&format!("  %_{} = {conv}", dest.0));
                                }
                            } else if is_mutable {
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
                                // Prefer the SSA value's actual type if we tracked
                                // it (operand_type only knows the MIR-declared type;
                                // an extractvalue from a named struct can yield a
                                // ptr at the SSA layer even when MIR says i64,
                                // causing "'ptr' but expected 'i64'" at the call).
                                let actual = self
                                    .actual_type(&v)
                                    .unwrap_or_else(|| self.operand_type(&args[1], func));
                                if actual == "ptr" {
                                    let tmp = self.next_temp();
                                    self.emit_line(&format!("  {tmp} = ptrtoint ptr {v} to i64"));
                                    tmp
                                } else if actual.starts_with('{')
                                    || actual.starts_with('%')
                                    || actual.starts_with('[')
                                {
                                    // FIX: clone heap (str/array/map) fields of a @copy
                                    // struct element before boxing it into the array so the
                                    // array owns its own references. Without this a later
                                    // scope-exit free of the source local's strings dangles
                                    // the pushed struct's fields (e.g. a ToolResult.content
                                    // pushed in a loop -> empty `"content":` on the wire).
                                    let v = if actual.starts_with('%') {
                                        if let Operand::Local(eid) = &args[1] {
                                            self.maybe_deep_copy_struct_fields(&v, *eid, func, &actual)
                                        } else {
                                            v
                                        }
                                    } else {
                                        v
                                    };
                                    // Aggregate element (enum/tuple/struct): box on the
                                    // heap and push the pointer as i64, mirroring the
                                    // array-literal element path. The matching `a[i]`
                                    // index unboxes (inttoptr + load aggregate). Without
                                    // this, push emitted `kryos_array_push(ptr, i64
                                    // <aggregate>)` -- a type mismatch (e.g. pushing a
                                    // JsonValue {i64,i64,i64} into a [JsonValue]).
                                    let size_ptr = self.next_temp();
                                    self.emit_line(&format!(
                                        "  {size_ptr} = getelementptr {actual}, ptr null, i32 1"
                                    ));
                                    let size_i64 = self.next_temp();
                                    self.emit_line(&format!(
                                        "  {size_i64} = ptrtoint ptr {size_ptr} to i64"
                                    ));
                                    let buf = self.next_temp();
                                    self.emit_line(&format!(
                                        "  {buf} = call ptr @kryos_calloc(i64 1, i64 {size_i64})"
                                    ));
                                    self.emit_line(&format!("  store {actual} {v}, ptr {buf}"));
                                    let t = self.next_temp();
                                    self.emit_line(&format!("  {t} = ptrtoint ptr {buf} to i64"));
                                    t
                                } else {
                                    self.coerce_value(&v, &actual, "i64")
                                }
                            } else {
                                "0".to_string()
                            };
                            self.emit_line(&format!(
                                "  call void @kryos_array_push(ptr {arr_val}, i64 {elem_val})"
                            ));
                            // MIR binds push's result to the dest local (e.g.
                            // `_3 = call push(_2, _1)`) but the runtime
                            // `kryos_array_push` returns void — it mutates the
                            // array in-place. Alias dest to the same array so
                            // downstream uses of `_3` (struct rebuilds, drop
                            // tracking) see the right SSA value. Closes
                            // test_process Command__arg undef-`%_3`.
                            if is_mutable {
                                if dest_ty == "ptr" {
                                    self.emit_line(&format!(
                                        "  store ptr {arr_val}, ptr %_{}.addr",
                                        dest.0
                                    ));
                                } else if dest_ty == "i64" {
                                    let as_i64 = self.next_temp();
                                    self.emit_line(&format!(
                                        "  {as_i64} = ptrtoint ptr {arr_val} to i64"
                                    ));
                                    self.emit_line(&format!(
                                        "  store i64 {as_i64}, ptr %_{}.addr",
                                        dest.0
                                    ));
                                }
                            } else if dest_ty == "ptr" {
                                self.emit_line(&format!(
                                    "  %_{} = getelementptr i8, ptr {arr_val}, i64 0",
                                    dest.0
                                ));
                            } else if dest_ty == "i64" {
                                self.emit_line(&format!(
                                    "  %_{} = ptrtoint ptr {arr_val} to i64",
                                    dest.0
                                ));
                            }
                        }
                        "assert_eq" if args.len() == 2 => {
                            // assert_eq(left, right) -> void
                            // Runtime: kryos_builtin_assert_eq(i64 left_handle, i64 right_handle)
                            // Mirrors the Cranelift path: stringify both args using the
                            // type-appropriate runtime helper (i64/f64/bool/str), then call
                            // the runtime which prints both values on failure.
                            let mut handles = Vec::with_capacity(2);
                            for arg in args.iter() {
                                let v = self.operand_to_llvm(arg, func);
                                let ty = self.operand_type(arg, func);
                                let handle = if ty == "double" || ty == "float" {
                                    let coerced = if ty == "float" {
                                        let t = self.next_temp();
                                        self.emit_line(&format!(
                                            "  {t} = fpext float {v} to double"
                                        ));
                                        t
                                    } else {
                                        v.clone()
                                    };
                                    let h = self.next_temp();
                                    self.emit_line(&format!(
                                        "  {h} = call i64 @kryos_f64_to_string(double {coerced})"
                                    ));
                                    h
                                } else if ty == "i1" {
                                    let ext = self.next_temp();
                                    self.emit_line(&format!(
                                        "  {ext} = zext i1 {v} to i64"
                                    ));
                                    let h = self.next_temp();
                                    self.emit_line(&format!(
                                        "  {h} = call i64 @kryos_bool_to_string(i64 {ext})"
                                    ));
                                    h
                                } else if ty == "ptr" {
                                    // KryosString* — already a packed string handle. Convert
                                    // ptr -> i64 for the runtime's i64-handle ABI.
                                    let t = self.next_temp();
                                    self.emit_line(&format!(
                                        "  {t} = ptrtoint ptr {v} to i64"
                                    ));
                                    t
                                } else {
                                    let coerced = self.coerce_value(&v, &ty, "i64");
                                    let h = self.next_temp();
                                    self.emit_line(&format!(
                                        "  {h} = call i64 @kryos_i64_to_string(i64 {coerced})"
                                    ));
                                    h
                                };
                                handles.push(handle);
                            }
                            self.emit_line(&format!(
                                "  call i64 @kryos_builtin_assert_eq(i64 {}, i64 {})",
                                handles[0], handles[1]
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
                        "panic" => {
                            // panic(msg: str) -> void
                            // Runtime: kryos_builtin_panic(i64) -> i64 (never returns)
                            let msg_val = if !args.is_empty() {
                                let v = self.operand_to_llvm(&args[0], func);
                                let ty = self.operand_type(&args[0], func);
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
                                "  call i64 @kryos_builtin_panic(i64 {msg_val})"
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
                        "abs_f" if args.len() == 1 && !self.func_param_types.contains_key("abs_f") => {
                            // Float-specific abs builtin. Without this arm the call
                            // emitted a bare `@abs_f` (undefined). Mirrors the float
                            // branch of polymorphic `abs` (llvm.fabs.f64).
                            let arg_ty = self.operand_type(&args[0], func);
                            let arg_val = self.operand_to_llvm(&args[0], func);
                            let v = self.coerce_value(&arg_val, &arg_ty, "double");
                            if is_mutable {
                                let tmp = self.next_temp();
                                self.emit_line(&format!(
                                    "  {tmp} = call double @llvm.fabs.f64(double {v})"
                                ));
                                self.emit_line(&format!(
                                    "  store double {tmp}, ptr %_{}.addr",
                                    dest.0
                                ));
                            } else {
                                self.emit_line(&format!(
                                    "  %_{} = call double @llvm.fabs.f64(double {v})",
                                    dest.0
                                ));
                            }
                        }
                        "min_f" | "max_f"
                            if args.len() == 2
                                && !self.func_param_types.contains_key(fname.as_str()) =>
                        {
                            // Float-specific min/max builtins (mirror the float branch
                            // of polymorphic min/max). Were emitting bare `@min_f`/
                            // `@max_f` (undefined).
                            let a_ty = self.operand_type(&args[0], func);
                            let a_val = self.operand_to_llvm(&args[0], func);
                            let b_ty = self.operand_type(&args[1], func);
                            let b_val = self.operand_to_llvm(&args[1], func);
                            let a = self.coerce_value(&a_val, &a_ty, "double");
                            let b = self.coerce_value(&b_val, &b_ty, "double");
                            let intrin = if fname.as_str() == "min_f" {
                                "llvm.minnum.f64"
                            } else {
                                "llvm.maxnum.f64"
                            };
                            if is_mutable {
                                let tmp = self.next_temp();
                                self.emit_line(&format!(
                                    "  {tmp} = call double @{intrin}(double {a}, double {b})"
                                ));
                                self.emit_line(&format!(
                                    "  store double {tmp}, ptr %_{}.addr",
                                    dest.0
                                ));
                            } else {
                                self.emit_line(&format!(
                                    "  %_{} = call double @{intrin}(double {a}, double {b})",
                                    dest.0
                                ));
                            }
                        }
                        "sqrt"
                            if args.len() == 1
                                && !self.func_param_types.contains_key("sqrt") =>
                        {
                            // sqrt builtin -> the LLVM intrinsic, which lowers to a
                            // single hardware sqrt (vsqrtsd) instead of a libm call.
                            // Identical IEEE-754 semantics to libm sqrt (NOT
                            // fast-math), so it matches what rustc -O / clang -O2
                            // emit and keeps the benchmark comparison fair.
                            let a_ty = self.operand_type(&args[0], func);
                            let a_val = self.operand_to_llvm(&args[0], func);
                            let a = self.coerce_value(&a_val, &a_ty, "double");
                            if is_mutable {
                                let tmp = self.next_temp();
                                self.emit_line(&format!(
                                    "  {tmp} = call double @llvm.sqrt.f64(double {a})"
                                ));
                                self.emit_line(&format!(
                                    "  store double {tmp}, ptr %_{}.addr",
                                    dest.0
                                ));
                            } else {
                                self.emit_line(&format!(
                                    "  %_{} = call double @llvm.sqrt.f64(double {a})",
                                    dest.0
                                ));
                            }
                        }
                        "pow" if args.len() == 2 && !self.func_param_types.contains_key("pow") => {
                            // Float power builtin -> @kryos_fpow (declared; same
                            // runtime the `**` operator uses). Was emitting a bare
                            // `@pow` (undefined; no libm pow declared). Args are
                            // coerced to double (pow is registered F64).
                            let a_ty = self.operand_type(&args[0], func);
                            let a_val = self.operand_to_llvm(&args[0], func);
                            let b_ty = self.operand_type(&args[1], func);
                            let b_val = self.operand_to_llvm(&args[1], func);
                            let a = self.coerce_value(&a_val, &a_ty, "double");
                            let b = self.coerce_value(&b_val, &b_ty, "double");
                            if is_mutable {
                                let tmp = self.next_temp();
                                self.emit_line(&format!(
                                    "  {tmp} = call double @kryos_fpow(double {a}, double {b})"
                                ));
                                self.emit_line(&format!(
                                    "  store double {tmp}, ptr %_{}.addr",
                                    dest.0
                                ));
                            } else {
                                self.emit_line(&format!(
                                    "  %_{} = call double @kryos_fpow(double {a}, double {b})",
                                    dest.0
                                ));
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
                            // `int(x)` on a float: the generic name-map routes
                            // `int` -> kryos_builtin_int(i64), which would pass the
                            // float's bit pattern as i64 (garbage). Convert with the
                            // saturating intrinsic instead, mirroring the Cranelift
                            // int() special-case (its codegen.rs ~line 3494).
                            if fname == "int"
                                && args.len() == 1
                                && !self.func_param_types.contains_key("int")
                                && self.operand_is_float(&args[0], func)
                            {
                                let v = self.operand_to_llvm(&args[0], func);
                                let vt = self.operand_type(&args[0], func);
                                let vd = self.coerce_value(&v, &vt, "double");
                                if is_mutable {
                                    let tmp = self.next_temp();
                                    self.emit_line(&format!(
                                        "  {tmp} = call i64 @llvm.fptosi.sat.i64.f64(double {vd})"
                                    ));
                                    self.emit_line(&format!(
                                        "  store i64 {tmp}, ptr %_{}.addr",
                                        dest.0
                                    ));
                                } else {
                                    self.emit_line(&format!(
                                        "  %_{} = call i64 @llvm.fptosi.sat.i64.f64(double {vd})",
                                        dest.0
                                    ));
                                }
                                return Ok(());
                            }
                            // User-defined functions shadow same-named builtins
                            // (matches Cranelift's user_shadows_builtin behavior).
                            // Without this guard, `fn contains(arr, target) -> bool`
                            // would silently route to `kryos_builtin_contains(str, str)`
                            // — the test_user_fn_shadows_builtin regression.
                            let user_shadow =
                                self.func_param_types.contains_key(fname.as_str());
                            // Translate Kryos user-facing builtin names to runtime symbols.
                            let mapped: &str = match fname.as_str() {
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
                                "create_dir" => "kryos_builtin_create_dir",
                                "env_get" => "kryos_builtin_env_get",
                                "args" => "kryos_builtin_args",
                                "read_line" => "kryos_builtin_read_line",
                                "http_get" => "kryos_builtin_http_get",
                                "http_request" => "kryos_http_request_ks",
                                // HTTP/2 + HTTPS clients: declared as kryos_*_ks
                                // runtime symbols but previously absent from this
                                // call-site map, so the LLVM backend emitted bare
                                // @http2_get / @https_get -> "undefined value" at
                                // link time (Cranelift had them, AOT did not).
                                "https_get" => "kryos_https_get_ks",
                                "http2_get" => "kryos_http2_get_ks",
                                "http2_post" => "kryos_http2_post_ks",
                                "http2_request" => "kryos_http2_request_ks",
                                "parse_int" => "kryos_builtin_parse_int",
                                "parse_float" => "kryos_builtin_parse_float",
                                // int() / float() coercion builtins. Cranelift
                                // routes these to kryos_builtin_int / _float;
                                // LLVM previously left them as bare @int /
                                // @float, which clang rejected as undefined.
                                "int" => "kryos_builtin_int",
                                "float" => "kryos_builtin_float",
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
                                "hmac_sha256" => "kryos_hmac_sha256_ks",
                                "ed25519_generate" => "kryos_ed25519_generate_ks",
                                "ed25519_public" => "kryos_ed25519_public_ks",
                                "ed25519_sign" => "kryos_ed25519_sign_ks",
                                "ed25519_verify" => "kryos_ed25519_verify_ks",
                                "pbkdf2_sha256" => "kryos_pbkdf2_sha256_ks",
                                "hex_to_base64url" => "kryos_hex_to_b64url_ks",
                                "base64url_to_hex" => "kryos_b64url_to_hex_ks",
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
                                "sleep" => "kryos_sleep",
                                "close_chan" => "kryos_chan_close_i64",
                                // Regex
                                "regex_new" => "kryos_regex_new_ks",
                                "regex_match" => "kryos_regex_is_match_ks",
                                // Mutex
                                "mutex_new" => "kryos_mutex_new",
                                "mutex_lock" => "kryos_mutex_lock",
                                "mutex_unlock" => "kryos_mutex_unlock",
                                "mutex_drop" => "kryos_mutex_drop",
                                // Low-level FFI helpers (v2.3.4)
                                "str_to_ptr" => "kryos_str_to_ptr",
                                "buf_to_str" => "kryos_buf_to_str",
                                "alloc" => "kryos_alloc_bytes",
                                "free_bytes" => "kryos_free_bytes",
                                "ptr_byte_at" => "kryos_ptr_byte_at",
                                "ptr_set_byte" => "kryos_ptr_set_byte",
                                "ptr_read_i64" => "kryos_ptr_read_i64",
                                "ptr_write_i64" => "kryos_ptr_write_i64",
                                "handle_to_str" => "kryos_handle_to_str",
                                other => other,
                            };
                            // User-defined shadow wins over builtin mapping.
                            let runtime_fname: &str = if user_shadow {
                                fname.as_str()
                            } else {
                                mapped
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
                    let coerced = if val_ty.starts_with('{')
                        || val_ty.starts_with('%')
                        || val_ty.starts_with('[')
                    {
                        // Aggregate arg into the uniform i64 closure ABI: box on
                        // the heap and pass the pointer as i64; the thunk unboxes
                        // it (inttoptr + load). Mirrors the aggregate-return path.
                        let size_ptr = self.next_temp();
                        self.emit_line(&format!(
                            "  {size_ptr} = getelementptr {val_ty}, ptr null, i32 1"
                        ));
                        let size_i64 = self.next_temp();
                        self.emit_line(&format!("  {size_i64} = ptrtoint ptr {size_ptr} to i64"));
                        let buf = self.next_temp();
                        self.emit_line(&format!(
                            "  {buf} = call ptr @kryos_arc_alloc(i64 {size_i64}, i64 8)"
                        ));
                        self.emit_line(&format!("  store {val_ty} {val}, ptr {buf}"));
                        let t = self.next_temp();
                        self.emit_line(&format!("  {t} = ptrtoint ptr {buf} to i64"));
                        t
                    } else {
                        self.coerce_value(&val, &val_ty, "i64")
                    };
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
                } else if dest_ty.starts_with('{')
                    || dest_ty.starts_with('%')
                    || dest_ty.starts_with('[')
                {
                    // Aggregate return (e.g. closure `fn(Request) -> Response`):
                    // the thunk boxed it and returned an i64 pointer; unbox via
                    // inttoptr + load. Without this `add %Response <i64>` was
                    // emitted (i64 used where the aggregate was expected).
                    let raw = self.next_temp();
                    self.emit_line(&format!("  {raw} = call i64 {thunk_ptr}({arg_list})"));
                    let p = self.next_temp();
                    self.emit_line(&format!("  {p} = inttoptr i64 {raw} to ptr"));
                    if is_mutable {
                        let v = self.next_temp();
                        self.emit_line(&format!("  {v} = load {dest_ty}, ptr {p}"));
                        self.emit_line(&format!(
                            "  store {dest_ty} {v}, ptr %_{}.addr",
                            dest.0
                        ));
                    } else {
                        self.emit_line(&format!("  %_{} = load {dest_ty}, ptr {p}", dest.0));
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
                // Track the SSA value's actual LLVM type so downstream callers
                // (kryos_array_push, kryos_string_concat, ...) that look at
                // actual_type can coerce ptr→i64 / i64→ptr correctly. Without
                // this, a ptr-typed field extracted from a named struct flows
                // into an i64-arg slot at the call and clang errors with
                // "defined with type 'ptr' but expected 'i64'".
                if let Some(struct_name) = obj_ty
                    .strip_prefix('%')
                    .or_else(|| if obj_ty.starts_with('{') { None } else { Some(obj_ty.as_str()) })
                {
                    let field_mir = self
                        .struct_defs
                        .get(struct_name)
                        .and_then(|fs| fs.get(field_idx))
                        .map(|(_, t)| t.clone());
                    if let Some(fmt) = field_mir {
                        // sig_ty_to_llvm (enum-aware): an ENUM field's extractvalue
                        // result IS the `{i64,<payloads>}` aggregate, so track it as
                        // that -- not the bare-i64 mir_type_to_llvm fallback. Else
                        // downstream (push/concat/enum-construct) sees "i64" for an
                        // enum field and skips aggregate boxing.
                        let field_llvm_ty = self.sig_ty_to_llvm(&fmt);
                        self.track_type(&target_name, &field_llvm_ty);
                    }
                }
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
                        "  {raw} = call i64 @__kryos_array_get_inline(ptr {obj_val}, i64 {idx_i64})"
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
                            // Integer element: the slot is i64; truncate to a narrow
                            // element type (i8/i16/i32/i1) so `store {dest_ty}` gets a
                            // value of the right width (a `[i32]` element was stored
                            // as the raw i64, which clang rejects). i64 is a no-op.
                            self.coerce_value(&raw, "i64", &dest_ty)
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
                        // Integer element: truncate the i64 slot to a narrow element
                        // type (i8/i16/i32/i1) before the identity `add` — otherwise
                        // `add i32 <i64>` is emitted and clang rejects it. i64 is a
                        // no-op (coerce_value returns the value unchanged).
                        let coerced = self.coerce_value(&raw, "i64", &dest_ty);
                        self.emit_line(&format!("  %_{} = add {dest_ty} {coerced}, 0", dest.0));
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
                // Allocate ARC-managed memory and store the inner value at offset 0.
                // For aggregate types (struct/tuple/enum that are not ptr/i64), we
                // allocate sizeof(T) bytes so the value is stored inline in the arc
                // block.  Scalar types (i64, ptr, double, …) continue to use the
                // existing 8-byte-slot approach (ptr-to-data model).
                let inner_val = self.operand_to_llvm(inner, func);
                let inner_ty = self.operand_type(inner, func);

                let target_name = if is_mutable {
                    self.next_temp()
                } else {
                    format!("%_{}", dest.0)
                };

                // Detect aggregate inner types (named structs like %Tree, or
                // literal struct/array types { … } / [ … ]).
                let is_aggregate = inner_ty.starts_with('%')
                    || inner_ty.starts_with('{')
                    || inner_ty.starts_with('[');

                if is_aggregate {
                    // Allocate sizeof(T) via the getelementptr-null size-of trick.
                    let size_ptr = self.next_temp();
                    self.emit_line(&format!(
                        "  {size_ptr} = getelementptr {inner_ty}, ptr null, i32 1"
                    ));
                    let size_i64 = self.next_temp();
                    self.emit_line(&format!(
                        "  {size_i64} = ptrtoint ptr {size_ptr} to i64"
                    ));
                    self.emit_line(&format!(
                        "  {target_name} = call ptr @kryos_arc_alloc(i64 {size_i64}, i64 8)"
                    ));
                    self.track_type(&target_name, "ptr");
                    // Store the aggregate inline at offset 0 of the arc block.
                    self.emit_line(&format!(
                        "  store {inner_ty} {inner_val}, ptr {target_name}"
                    ));
                    // Register the per-type arc drop function so
                    // kryos_arc_release recurses into Shared children
                    // (recursive refcounted teardown of e.g. a tree).
                    if let Some(sname) = inner_ty.strip_prefix('%') {
                        if self.struct_defs.contains_key(sname) {
                            self.emit_line(&format!(
                                "  call void @kryos_arc_set_drop(ptr {target_name}, ptr @__kryos_arc_drop_{sname})"
                            ));
                        }
                    }
                } else {
                    // Scalar / pointer value: allocate an 8-byte slot.
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
                }

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
                        // Prefer the SSA value's tracked actual type over the MIR-
                        // declared type: an extractvalue/field result can be a ptr
                        // (e.g. a `[JsonValue]` array payload) while operand_type
                        // still says i64, leaving the ptr uncoerced -> `insertvalue
                        // ptr instead of i64`.
                        let val_ty = self
                            .actual_type(&val)
                            .unwrap_or_else(|| self.operand_type(field_op, func));
                        // Payload slot is i64; cast non-i64 values (e.g. ptr) first.
                        // void-typed operands (rare — result of a void-returning
                        // call cached into a local before the throw/catch
                        // rewrite) get replaced with a literal 0 to keep the
                        // emitted IR well-typed.
                        if val_ty == "void" {
                            val = "0".to_string();
                        } else if val_ty.starts_with('{') || val_ty.starts_with('%') {
                            // Aggregate-valued payload (recursive enum: an Expr
                            // child packed into another Expr's payload slot).
                            // bitcast { i64, i64, i64 } -> i64 is invalid; heap-
                            // allocate a copy and pass the pointer as i64.
                            // Use the GEP-sizeof trick to size the allocation.
                            let size_tmp = self.next_temp();
                            self.emit_line(&format!(
                                "  {size_tmp} = getelementptr {val_ty}, ptr null, i64 1"
                            ));
                            let size_int = self.next_temp();
                            self.emit_line(&format!(
                                "  {size_int} = ptrtoint ptr {size_tmp} to i64"
                            ));
                            let heap_i64 = self.next_temp();
                            self.emit_line(&format!(
                                "  {heap_i64} = call i64 @kryos_arc_alloc_i64(i64 {size_int})"
                            ));
                            let heap_ptr = self.next_temp();
                            self.emit_line(&format!(
                                "  {heap_ptr} = inttoptr i64 {heap_i64} to ptr"
                            ));
                            self.emit_line(&format!(
                                "  store {val_ty} {val}, ptr {heap_ptr}"
                            ));
                            val = heap_i64;
                        } else if val_ty != "i64" {
                            let casted = self.next_temp();
                            let op = if val_ty == "ptr" {
                                "ptrtoint"
                            } else if val_ty == "double" {
                                // 64-bit float: reinterpret as i64 via bitcast.
                                "bitcast"
                            } else if llvm_type_width(&val_ty) < 64 {
                                // narrow integer (i1/i8/i16/i32): zero-extend to i64.
                                // bitcast requires equal bit-width and would be rejected
                                // by LLVM ("invalid cast opcode for cast from 'i1' to 'i64'").
                                "zext"
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
                    // dest_ty pointing at a named struct or aggregate ({...} / %X)
                    // means the i64 payload slot is actually a heap-handle to the
                    // enum value (the recursive-Expr case). Materialise it via
                    // inttoptr+load instead of bitcast — bitcasting i64 to an
                    // opaque struct is invalid LLVM ("invalid cast opcode for
                    // cast from 'i64' to '%Expr = type opaque'").
                    if dest_ty.starts_with('%') || dest_ty.starts_with('{') {
                        // Resolve the bare named type (`%Expr`) into the full
                        // aggregate so the load operand is a first-class type
                        // (`{ i64, i64, i64 }`). Without this, `load %Expr` fails
                        // ("load operand must be a pointer to a first class type")
                        // because %Expr was only forward-declared.
                        let resolved_ty = if let Some(name) = dest_ty.strip_prefix('%') {
                            if self.struct_defs.contains_key(name) {
                                // A defined STRUCT type (e.g. Option<User>
                                // payload): load with the named type directly.
                                // The literal-body resolve below is only for
                                // enums (emitted as opaque forward decls);
                                // enum_max_fields on a struct name returns 0,
                                // producing "{ i64 }" which truncates a
                                // multi-field struct payload on load.
                                dest_ty.clone()
                            } else {
                                let max = self.enum_max_fields(name);
                                self.enum_llvm_type(name, max)
                            }
                        } else {
                            dest_ty.clone()
                        };
                        let ptr = self.next_temp();
                        self.emit_line(&format!(
                            "  {ptr} = inttoptr i64 {slot_tmp} to ptr"
                        ));
                        self.emit_line(&format!(
                            "  {t} = load {resolved_ty}, ptr {ptr}"
                        ));
                    } else {
                        let op = if dest_ty == "ptr" {
                            "inttoptr"
                        } else if dest_ty == "double" {
                            // 64-bit float: reinterpret from i64 via bitcast.
                            "bitcast"
                        } else if llvm_type_width(&dest_ty) < 64 {
                            // narrow integer (i1/i8/i16/i32): truncate from i64.
                            // bitcast requires equal bit-width.
                            "trunc"
                        } else {
                            "bitcast"
                        };
                        self.emit_line(&format!("  {t} = {op} i64 {slot_tmp} to {dest_ty}"));
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

            RValue::MakeTraitObject {
                value,
                concrete_type,
                trait_name,
            } => {
                // Materialize a fat pointer: [data: i64, fn_ptr_0: i64, fn_ptr_1: i64, ...]
                // Allocated via the ARC runtime so the trait object survives
                // the current scope and is freed when its refcount drops.
                let data_val = self.operand_to_llvm(value, func);
                let val_ty = self.operand_type(value, func);

                // For aggregate (struct/tuple) self values, spill to a heap
                // slot and store the pointer's integer representation.  The
                // matching `{method}_dyn` thunk loads via inttoptr and forwards
                // by `byval` to the underlying method.
                let is_aggregate =
                    val_ty.starts_with('%') || val_ty.starts_with('{');
                let data_i64 = if is_aggregate {
                    // Compute size: use sizeof via a GEP trick.
                    let size_tmp = self.next_temp();
                    self.emit_line(&format!(
                        "  {size_tmp} = getelementptr {val_ty}, ptr null, i64 1"
                    ));
                    let size_int = self.next_temp();
                    self.emit_line(&format!(
                        "  {size_int} = ptrtoint ptr {size_tmp} to i64"
                    ));
                    let slot_i64 = self.next_temp();
                    self.emit_line(&format!(
                        "  {slot_i64} = call i64 @kryos_arc_alloc_i64(i64 {size_int})"
                    ));
                    let slot_ptr = self.next_temp();
                    self.emit_line(&format!(
                        "  {slot_ptr} = inttoptr i64 {slot_i64} to ptr"
                    ));
                    self.emit_line(&format!(
                        "  store {val_ty} {data_val}, ptr {slot_ptr}"
                    ));
                    slot_i64
                } else {
                    self.coerce_value(&data_val, &val_ty, "i64")
                };

                let vtable_key = (concrete_type.clone(), trait_name.clone());
                let method_names = self
                    .trait_vtables
                    .get(&vtable_key)
                    .cloned()
                    .unwrap_or_default();
                let num_methods = method_names.len().max(1);
                let alloc_size = 8 + 8 * num_methods;

                let fat_i64 = self.next_temp();
                self.emit_line(&format!(
                    "  {fat_i64} = call i64 @kryos_arc_alloc_i64(i64 {alloc_size})"
                ));
                let fat_ptr = self.next_temp();
                self.emit_line(&format!("  {fat_ptr} = inttoptr i64 {fat_i64} to ptr"));
                // Store data at offset 0.
                self.emit_line(&format!("  store i64 {data_i64}, ptr {fat_ptr}"));
                // Store each method dyn-thunk pointer at offset (i+1)*8.
                // The dyn-thunk accepts uniform `(i64 self, i64 args...) -> i64`
                // and forwards to the real method (handling byval/sret).
                for (i, method_name) in method_names.iter().enumerate() {
                    let slot = self.next_temp();
                    self.emit_line(&format!(
                        "  {slot} = getelementptr i64, ptr {fat_ptr}, i64 {}",
                        i + 1
                    ));
                    let fn_int = self.next_temp();
                    self.emit_line(&format!(
                        "  {fn_int} = ptrtoint ptr @{method_name}_dyn to i64"
                    ));
                    self.emit_line(&format!("  store i64 {fn_int}, ptr {slot}"));
                }

                // Coerce fat_ptr to dest_ty (typically ptr).
                let coerced = self.coerce_value(&fat_ptr, "ptr", &dest_ty);
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
                method_index,
                args,
                return_ty,
            } => {
                // Dynamic dispatch: load self/data from fat[0], fn_ptr from
                // fat[1+method_index], then call fn_ptr(self, args...).
                let obj_raw = self.operand_to_llvm(object, func);
                let obj_ty = self.operand_type(object, func);
                // Treat the trait object as a pointer to the fat pointer.
                let fat_ptr = if obj_ty == "ptr" {
                    obj_raw
                } else {
                    self.coerce_value(&obj_raw, &obj_ty, "ptr")
                };

                // Load data (self) at offset 0.
                let data_val = self.next_temp();
                self.emit_line(&format!("  {data_val} = load i64, ptr {fat_ptr}"));

                // Load fn_ptr at offset (1 + method_index)*8.
                let fn_slot = self.next_temp();
                self.emit_line(&format!(
                    "  {fn_slot} = getelementptr i64, ptr {fat_ptr}, i64 {}",
                    method_index + 1
                ));
                let fn_i64 = self.next_temp();
                self.emit_line(&format!("  {fn_i64} = load i64, ptr {fn_slot}"));
                let fn_ptr = self.next_temp();
                self.emit_line(&format!("  {fn_ptr} = inttoptr i64 {fn_i64} to ptr"));

                // Build argument list: [self (i64), args (i64)...].
                let mut arg_vals: Vec<String> = Vec::with_capacity(1 + args.len());
                arg_vals.push(data_val.clone());
                for a in args {
                    let av = self.operand_to_llvm(a, func);
                    let aty = self.operand_type(a, func);
                    let coerced = self.coerce_value(&av, &aty, "i64");
                    arg_vals.push(coerced);
                }
                let arg_list = arg_vals
                    .iter()
                    .map(|v| format!("i64 {v}"))
                    .collect::<Vec<_>>()
                    .join(", ");

                let is_void = matches!(return_ty, MirType::Void);
                if is_void {
                    self.emit_line(&format!("  call void {fn_ptr}({arg_list})"));
                    // No value to bind; if dest is mutable, leave as-is.
                } else {
                    let raw = self.next_temp();
                    self.emit_line(&format!(
                        "  {raw} = call i64 {fn_ptr}({arg_list})"
                    ));
                    let coerced = self.coerce_value(&raw, "i64", &dest_ty);
                    if is_mutable {
                        self.emit_line(&format!(
                            "  store {dest_ty} {coerced}, ptr %_{}.addr",
                            dest.0
                        ));
                    } else if dest_ty == "ptr" {
                        self.emit_line(&format!(
                            "  %_{} = getelementptr i8, ptr {coerced}, i64 0",
                            dest.0
                        ));
                    } else if dest_ty == "double" || dest_ty == "float" {
                        self.emit_line(&format!(
                            "  %_{} = fadd {dest_ty} {coerced}, 0.0",
                            dest.0
                        ));
                    } else if dest_ty == "void" {
                        // dest already void — nothing to bind.
                    } else {
                        self.emit_line(&format!(
                            "  %_{} = add {dest_ty} {coerced}, 0",
                            dest.0
                        ));
                    }
                }
            }

            RValue::StringConcat(parts) => {
                // Chain kryos_string_concat calls: fold left across all parts.
                //
                // Every operand passed to @kryos_string_concat must be a `ptr`. A
                // string-typed local that was loaded from an i64-shaped alloca
                // (the common case for mutable string locals — see test_string_clobber)
                // arrives here as an i64 SSA value and must be `inttoptr`-cast first.
                // Without the cast, clang rejects the IR with
                //   "'%tN' defined with type 'i64' but expected 'ptr'"
                let load_part_as_ptr = |this: &mut Self, op: &Operand| -> String {
                    let val = this.operand_to_llvm(op, func);
                    let ty = this.operand_type(op, func);
                    // Non-string parts (bool / float / numeric int) used inside
                    // an interpolated string `"x={v}"` must be stringified before
                    // they can flow into kryos_string_concat. Without this, an i1
                    // (or double) is naively cast to ptr and clang rejects the
                    // call: "'%_N' defined with type 'i1' but expected 'ptr'".
                    if ty == "i1" {
                        let ext = this.next_temp();
                        this.emit_line(&format!("  {ext} = zext i1 {val} to i64"));
                        let h = this.next_temp();
                        this.emit_line(&format!(
                            "  {h} = call i64 @kryos_bool_to_string(i64 {ext})"
                        ));
                        let p = this.next_temp();
                        this.emit_line(&format!("  {p} = inttoptr i64 {h} to ptr"));
                        return p;
                    }
                    if ty == "double" || ty == "float" {
                        let coerced = if ty == "float" {
                            let t = this.next_temp();
                            this.emit_line(&format!("  {t} = fpext float {val} to double"));
                            t
                        } else {
                            val.clone()
                        };
                        let h = this.next_temp();
                        this.emit_line(&format!(
                            "  {h} = call i64 @kryos_f64_to_string(double {coerced})"
                        ));
                        let p = this.next_temp();
                        this.emit_line(&format!("  {p} = inttoptr i64 {h} to ptr"));
                        return p;
                    }
                    if ty == "i64"
                        || ty == "i32"
                        || ty == "i16"
                        || ty == "i8"
                    {
                        let widened = this.coerce_value(&val, &ty, "i64");
                        let h = this.next_temp();
                        this.emit_line(&format!(
                            "  {h} = call i64 @kryos_i64_to_string(i64 {widened})"
                        ));
                        let p = this.next_temp();
                        this.emit_line(&format!("  {p} = inttoptr i64 {h} to ptr"));
                        return p;
                    }
                    this.coerce_value(&val, &ty, "ptr")
                };

                if parts.is_empty() {
                    if is_mutable {
                        self.emit_line(&format!("  store ptr null, ptr %_{}.addr", dest.0));
                    } else {
                        self.emit_line(&format!("  %_{} = inttoptr i64 0 to ptr", dest.0));
                    }
                } else if parts.len() == 1 {
                    let val = load_part_as_ptr(self, &parts[0]);
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
                    let first = load_part_as_ptr(self, &parts[0]);
                    let second = load_part_as_ptr(self, &parts[1]);
                    let mut acc = self.next_temp();
                    self.emit_line(&format!(
                        "  {acc} = call ptr @kryos_string_concat(ptr {first}, ptr {second})"
                    ));
                    for part in &parts[2..] {
                        let next_val = load_part_as_ptr(self, part);
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
    /// The struct type name backing `object`, if it is a struct-typed local.
    fn resolve_struct_name(&self, object: &Operand, func: &MirFunction) -> Option<String> {
        if let Operand::Local(id) = object {
            func.locals.iter().find(|l| l.id == *id).and_then(|l| match &l.ty {
                MirType::Struct(name) => Some(name.clone()),
                _ => None,
            })
        } else {
            None
        }
    }

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
        // Operands whose tracked SSA type differs from the op's chosen type
        // (e.g. an i16 local compared against an i64 literal — the emitter
        // picks i64 but the load produced i16) must be coerced first or
        // clang rejects the module. coerce_value no-ops when types agree;
        // signed narrows sign-extend, matching Cranelift's i64 slots.
        let left_owned;
        let mut left = left;
        if let Some(actual) = self.actual_type(left) {
            if actual != ty && !is_float {
                left_owned = self.coerce_value(left, &actual, ty);
                left = &left_owned;
            }
        }
        let right_owned;
        let mut right = right;
        if let Some(actual) = self.actual_type(right) {
            if actual != ty && !is_float {
                right_owned = self.coerce_value(right, &actual, ty);
                right = &right_owned;
            }
        }
        // Runtime div-by-zero guard for integer division/modulo. A bare
        // sdiv/srem is undefined behaviour on a zero divisor under LLVM (it
        // silently produced garbage in release builds), whereas the Cranelift
        // JIT already panics. Call the same runtime check the JIT uses, which
        // panics with "integer division by zero" when the divisor is 0.
        if !is_float && matches!(op, MirBinOp::Div | MirBinOp::Mod) {
            let divisor = match ty {
                "i64" => Some(right.to_string()),
                "i8" | "i16" | "i32" => {
                    let w = self.next_temp();
                    self.emit_line(&format!("  {w} = sext {ty} {right} to i64"));
                    Some(w)
                }
                // i128 / other widths are rare; skip rather than emit invalid IR.
                _ => None,
            };
            if let Some(d) = divisor {
                self.emit_line(&format!("  call void @kryos_check_div_zero_i64(i64 {d})"));
            }
        }
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
        // Comparisons produce i1, not the operand type. Track it so later
        // reads of the value (stored bool locals used with `not`/`or`)
        // coerce correctly instead of emitting `icmp ne i64` on an i1.
        if matches!(
            op,
            MirBinOp::Eq
                | MirBinOp::Neq
                | MirBinOp::Lt
                | MirBinOp::Gt
                | MirBinOp::LtEq
                | MirBinOp::GtEq
        ) {
            self.track_type(target, "i1");
        }
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
        // Stack-promotion path: if this local is stackable, initialise the
        // pre-allocated header + data arrays on the stack instead of calling
        // kryos_array_new.  The allocas were emitted in the entry block above.
        //
        // Header layout (matches KryosArray ABI):
        //   field 0: i64 len      = n
        //   field 1: i64 cap      = n
        //   field 2: i64 elem_size = 8
        //   field 3: i64 ref_count = 1_000_000  (sentinel; frees are suppressed)
        //   field 4: ptr data     = ptr to %_N.stk_dat
        if dest_ty == "ptr" && self.stackable_locals.contains(&dest.0) {
            let n = elems.len() as i64;
            let hdr = format!("%_{}.stk_hdr", dest.0);
            let dat = format!("%_{}.stk_dat", dest.0);

            // Store header fields.
            // Field 0: len
            let p0 = self.next_temp();
            self.emit_line(&format!("  {p0} = getelementptr {{ i64, i64, i64, i64, ptr }}, ptr {hdr}, i32 0, i32 0"));
            self.emit_line(&format!("  store i64 {n}, ptr {p0}"));
            // Field 1: cap
            let p1 = self.next_temp();
            self.emit_line(&format!("  {p1} = getelementptr {{ i64, i64, i64, i64, ptr }}, ptr {hdr}, i32 0, i32 1"));
            self.emit_line(&format!("  store i64 {n}, ptr {p1}"));
            // Field 2: elem_size = 8
            let p2 = self.next_temp();
            self.emit_line(&format!("  {p2} = getelementptr {{ i64, i64, i64, i64, ptr }}, ptr {hdr}, i32 0, i32 2"));
            self.emit_line(&format!("  store i64 8, ptr {p2}"));
            // Field 3: ref_count = 1_000_000 (sentinel)
            let p3 = self.next_temp();
            self.emit_line(&format!("  {p3} = getelementptr {{ i64, i64, i64, i64, ptr }}, ptr {hdr}, i32 0, i32 3"));
            self.emit_line(&format!("  store i64 1000000, ptr {p3}"));
            // Field 4: data = ptr to data array
            let p4 = self.next_temp();
            self.emit_line(&format!("  {p4} = getelementptr {{ i64, i64, i64, i64, ptr }}, ptr {hdr}, i32 0, i32 4"));
            // Get a ptr to element 0 of the data array.
            let dat_ptr = self.next_temp();
            self.emit_line(&format!("  {dat_ptr} = getelementptr [{n} x i64], ptr {dat}, i32 0, i32 0"));
            self.emit_line(&format!("  store ptr {dat_ptr}, ptr {p4}"));

            // Store each element into the data array.
            for (i, elem) in elems.iter().enumerate() {
                let elem_val = self.operand_to_llvm(elem, func);
                let elem_ty = self.operand_type(elem, func);
                // Convert element to i64 (same logic as the heap path).
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
                } else if elem_ty == "i1" || elem_ty == "i8" || elem_ty == "i16" || elem_ty == "i32" {
                    let t = self.next_temp();
                    self.emit_line(&format!("  {t} = sext {elem_ty} {elem_val} to i64"));
                    t
                } else {
                    elem_val
                };
                let slot = self.next_temp();
                self.emit_line(&format!(
                    "  {slot} = getelementptr [{n} x i64], ptr {dat}, i32 0, i32 {i}"
                ));
                self.emit_line(&format!("  store i64 {as_i64}, ptr {slot}"));
            }

            // Produce the local value = pointer to the header.
            if is_mutable {
                self.emit_line(&format!("  store ptr {hdr}, ptr %_{}.addr", dest.0));
            } else {
                self.emit_line(&format!(
                    "  %_{} = getelementptr i8, ptr {hdr}, i64 0",
                    dest.0
                ));
            }
            return Ok(());
        }

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
                        "  {buf} = call ptr @kryos_calloc(i64 1, i64 {size_i64})"
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
        //
        // Resolve the aggregate type. `dest_ty` arrives from local_type(),
        // which defaults to `i64` if the local wasn't registered with its
        // proper aggregate shape. `insertvalue i64 undef, ...` is invalid
        // LLVM ("insertvalue operand must be aggregate type"). When dest_ty
        // is `i64`, synthesize `{ T1, T2, ... }` from each elem's actual
        // type so the chained insertvalue is well-typed.
        //
        // Use fresh next_temp() names for the intermediate insertvalue chain
        // instead of `_<dest>_tup_<i>` so a mutable local re-assigned to a
        // tuple in two blocks doesn't redefine the same SSA name (same fix
        // pattern as Class C struct field SSA collision).
        let elem_tys: Vec<String> = elems
            .iter()
            .map(|e| self.operand_type(e, func))
            .collect();
        let agg_ty = if dest_ty.starts_with('{') || dest_ty.starts_with('%') {
            dest_ty.to_string()
        } else if !elems.is_empty() {
            format!("{{ {} }}", elem_tys.join(", "))
        } else {
            dest_ty.to_string()
        };

        // The destination tuple type may disagree with an element's value type
        // (the MIR can leave a tuple element as an unresolved var -> i64 while the
        // value is a ptr/aggregate, e.g. a recursive-enum array). Coerce each
        // element to its declared SLOT type so the insertvalue is well-typed --
        // matching how the JIT treats all slots uniformly. Codegen-only.
        let slot_tys: Vec<String> = if agg_ty.starts_with('{') {
            split_aggregate_fields(&agg_ty)
        } else {
            Vec::new()
        };
        let mut prev = "undef".to_string();
        for (i, elem) in elems.iter().enumerate() {
            let elem_val = self.operand_to_llvm(elem, func);
            let elem_ty = elem_tys[i].clone();
            let slot_ty = slot_tys.get(i).cloned().unwrap_or_else(|| elem_ty.clone());
            let elem_val = if slot_ty != elem_ty {
                self.coerce_value(&elem_val, &elem_ty, &slot_ty)
            } else {
                elem_val
            };
            let elem_ty = &slot_ty;
            let this = if i + 1 == elems.len() {
                if is_mutable {
                    self.next_temp()
                } else {
                    format!("%_{}", dest.0)
                }
            } else {
                self.next_temp()
            };
            self.emit_line(&format!(
                "  {this} = insertvalue {agg_ty} {prev}, {elem_ty} {elem_val}, {i}"
            ));
            if i + 1 == elems.len() && is_mutable {
                self.emit_line(&format!("  store {agg_ty} {this}, ptr %_{}.addr", dest.0));
            }
            prev = this;
        }
        // Register the destination local's actual type so downstream readers
        // (terminator, sret-store) see the aggregate type instead of falling
        // back to `i64`. Without this, Return(Some(tuple)) emits
        //   store { i64, i64 } undef, ptr %_sret
        // because operand_type(%_<dest>) returned `i64`.
        if !elems.is_empty() {
            self.local_types.insert(dest.0, agg_ty.clone());
            self.track_type(&format!("%_{}", dest.0), &agg_ty);
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
        // MIR field types, to clone heap (array/str/map) fields at construction
        // (see clone note in the loop below).
        let field_mir_tys: Vec<MirType> = self
            .struct_defs
            .get(struct_name)
            .map(|fs| fs.iter().map(|(_, t)| t.clone()).collect())
            .unwrap_or_default();
        // Declared LLVM field types via sig_ty_to_llvm (enum-aware), matching
        // emit_struct_type_decls — so an enum field's initializer is inserted as
        // the `{ i64, <payloads> }` aggregate, not coerced to a bare i64.
        let declared_field_tys: Vec<String> =
            field_mir_tys.iter().map(|t| self.sig_ty_to_llvm(t)).collect();
        // Thread chained insertvalue temps through fresh SSA names from next_temp()
        // instead of dest-indexed names. Dest-indexed names ("%_3_fld_0", ...) collide
        // when the same mutable local is re-assigned in the same function, e.g. an
        // `if`-each-arm sets a Ctx struct: LLVM rejects the second insertvalue chain
        // as "multiple definition of local value named '_3_fld_0'".
        // Start from zeroinitializer (not undef) so any struct field NOT set
        // by this literal defaults to 0/null -- matching the Cranelift backend,
        // which calloc's struct bodies. With `undef`, omitted fields held
        // garbage that read non-deterministically (different bytes each run),
        // which made the LLVM-built stage-1 emit non-deterministic object code
        // (e.g. a bool/discriminant field read as 0 -> `xor`, nonzero -> `mov`).
        // This was the dominant remaining source of stage-1 codegen non-determinism.
        let mut prev = "zeroinitializer".to_string();
        for (i, (field_name, op)) in fields.iter().enumerate() {
            let val = self.operand_to_llvm(op, func);
            let actual_ty = self.operand_type(op, func);
            let expected_ty = declared_field_tys
                .get(i)
                .cloned()
                .unwrap_or_else(|| actual_ty.clone());
            let mut coerced_val = self.coerce_value(&val, &actual_ty, &expected_ty);
            // Clone heap (array/str/map) field VALUES so each field owns an
            // independent buffer. Without this, a constructor that assigns the
            // SAME array handle to two fields (e.g. cg_new's reused `empty_ints`
            // for func_positions + string_positions + block_offsets) makes those
            // fields share one buffer -- a push to one grows the others, which
            // desynced parallel arrays and corrupted function-offset lookups
            // (every internal call jumped to garbage). The Cranelift backend
            // clones these on @copy struct construction, which is why stage-0
            // was immune. Shallow clone (new buffer, shared elements), so no
            // per-element recursion and no O(N^2) blow-up; big collections live
            // in module-globals, not struct fields.
            if let Some(MirType::Array(_, _)) = field_mir_tys.get(i) {
                if coerced_val != "null" && coerced_val != "zeroinitializer" {
                    let cl = self.next_temp();
                    self.emit_line(&format!("  {cl} = call ptr @kryos_array_clone(ptr {coerced_val})"));
                    coerced_val = cl;
                }
            }
            let this = if i + 1 == fields.len() {
                if is_mutable {
                    // Use a temp name; we will store it to the alloca below.
                    self.next_temp()
                } else {
                    format!("%_{}", dest.0)
                }
            } else {
                // Fresh SSA name per step in the chain — never reuses dest.0.
                self.next_temp()
            };
            self.emit_line(&format!(
                "  {this} = insertvalue {dest_ty} {prev}, {expected_ty} {coerced_val}, {i} ; .{field_name}"
            ));
            // If this was the last field and the local is mutable, store to alloca.
            if i + 1 == fields.len() && is_mutable {
                self.emit_line(&format!("  store {dest_ty} {this}, ptr %_{}.addr", dest.0));
            }
            prev = this;
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
        // For enums use the proper aggregate type so recursive payloads
        // (an Expr-inside-Expr) get loaded as `{ i64, i64, ... }`, not
        // bitcast to an opaque `%Expr`. The bare named type emission
        // produced  `bitcast i64 X to %Expr`  which LLVM rejects because
        // `%Expr` is forward-declared opaque at the bitcast site.
        let dst_ty = self.sig_ty_to_llvm(target_ty);

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

        // i64 → enum aggregate (`{ i64, i64, ... }`). The i64 is a
        // heap-handle to the enum value; dereference it.
        if src_ty == "i64" && (dst_ty.starts_with('{') || dst_ty.starts_with('%')) {
            let ptr = self.next_temp();
            self.emit_line(&format!("  {ptr} = inttoptr i64 {src_val} to ptr"));
            if is_mutable {
                let tmp = self.next_temp();
                self.emit_line(&format!("  {tmp} = load {dst_ty}, ptr {ptr}"));
                self.emit_line(&format!("  store {dst_ty} {tmp}, ptr %_{}.addr", dest.0));
            } else {
                self.emit_line(&format!(
                    "  %_{} = load {dst_ty}, ptr {ptr}",
                    dest.0
                ));
            }
            return Ok(());
        }

        let src_is_float = is_float_type(&src_ty);
        let dst_is_float = is_float_type(&dst_ty);
        let src_is_ptr = src_ty == "ptr";
        let dst_is_ptr = dst_ty == "ptr";

        // float -> int: use the saturating intrinsic instead of bare `fptosi`.
        // `fptosi` is undefined (poison) for out-of-range / NaN inputs, which
        // silently produced garbage in release; `@llvm.fptosi.sat` saturates to
        // the target's min/max (and yields 0 for NaN), matching the Cranelift
        // JIT's `fcvt_to_sint_sat`. The intrinsic suffix uses `f64`/`f32`, not
        // the IR type names `double`/`float`.
        if src_is_float && !dst_is_float {
            let src_suffix = match src_ty.as_str() {
                "double" => "f64",
                "float" => "f32",
                other => other,
            };
            let intrinsic = format!("@llvm.fptosi.sat.{dst_ty}.{src_suffix}");
            if is_mutable {
                let tmp = self.next_temp();
                self.emit_line(&format!(
                    "  {tmp} = call {dst_ty} {intrinsic}({src_ty} {src_val})"
                ));
                self.emit_line(&format!("  store {dst_ty} {tmp}, ptr %_{}.addr", dest.0));
            } else {
                self.emit_line(&format!(
                    "  %_{} = call {dst_ty} {intrinsic}({src_ty} {src_val})",
                    dest.0
                ));
            }
            return Ok(());
        }

        let inst = if src_is_float && dst_is_float {
            // float -> float: fpext or fptrunc.
            if llvm_type_width(&dst_ty) > llvm_type_width(&src_ty) {
                "fpext"
            } else {
                "fptrunc"
            }
        } else if src_is_float && !dst_is_float {
            unreachable!("float->int is handled by the saturating intrinsic above")
        } else if !src_is_float && dst_is_float {
            "sitofp"
        } else if src_is_ptr && !dst_is_ptr {
            "ptrtoint"
        } else if !src_is_ptr && dst_is_ptr {
            "inttoptr"
        } else {
            // int -> int: sext or trunc. Booleans widen with ZERO extension —
            // `true as i64` must be 1, not -1 (sext i1 1 = -1).
            if llvm_type_width(&dst_ty) > llvm_type_width(&src_ty) {
                if src_ty == "i1" {
                    "zext"
                } else {
                    "sext"
                }
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
                if func.name == "main" {
                    self.emit_line(
                        "  call void @kryos_exception_report_uncaught_if_pending()",
                    );
                }
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
                if func.name == "main" {
                    self.emit_line(
                        "  call void @kryos_exception_report_uncaught_if_pending()",
                    );
                }
                if let Some(agg) = self.aggregate_llvm_ty(&func.ret_ty) {
                    let from_ty = self.operand_type(op, func);
                    let val = self.operand_to_llvm(op, func);
                    // Mirror the scalar-return coercion logic. The operand type may
                    // not match the aggregate return type after a `kryos_exception_throw`
                    // fallthrough (void/i64 result feeds a Return in an aggregate-returning
                    // function). The ret is dead code in that case — store undef to keep
                    // the IR well-typed instead of emitting `store %Parser 0, ptr ...`.
                    if from_ty == agg {
                        self.emit_line(&format!("  store {agg} {val}, ptr %_sret"));
                    } else {
                        self.emit_line(&format!("  store {agg} undef, ptr %_sret"));
                    }
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
                        // Track the load result's actual LLVM type so callers
                        // that inspect actual_type (e.g. kryos_array_push,
                        // kryos_string_concat) coerce ptr↔i64 correctly.
                        self.value_types.insert(tmp.clone(), ty);
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
            // An unresolved generic type parameter (e.g. the `T` left in a
            // partially monomorphized struct like `Probable___T`) is not a real
            // struct or enum. Erase it to the i64 ABI slot rather than emitting
            // `%T`, which LLVM treats as an opaque (unsized) named type and then
            // rejects as a GEP base ("base element of getelementptr must be
            // sized"). Generic payload slots are i64-sized under the erased ABI.
            MirType::Struct(name)
                if !self.struct_defs.contains_key(name)
                    && !self.enum_defs.contains_key(name) =>
            {
                "i64".to_string()
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
        // The caller's from_type comes from MIR, which carries booleans as
        // i64; the SSA value may really be an i1 (a stored comparison read
        // back from a bool local). Trust the tracked type across the
        // i1/i64 divide, or `not b` / `b or c` on a bool VARIABLE emits
        // `icmp ne i64 %v, 0` on an i1 and clang rejects the module.
        let mut from_type = from_type;
        if from_type == "i64" {
            if let Some(real) = self.actual_type(value) {
                if real == "i1" {
                    from_type = "i1";
                }
            }
        }
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
        // Generic integer width conversion: the pair table below misses some
        // combinations (i16<->i64 had none at all). Route every intN->intM
        // through sext/trunc here; i1 stays with its dedicated arms.
        let int_rank = |t: &str| -> Option<u8> {
            match t {
                "i8" => Some(1),
                "i16" => Some(2),
                "i32" => Some(3),
                "i64" => Some(4),
                _ => None,
            }
        };
        if let (Some(rf), Some(rt)) = (int_rank(from_type), int_rank(to_type)) {
            let tmp = self.next_temp();
            if rf < rt {
                self.emit_line(&format!("  {tmp} = sext {from_type} {value} to {to_type}"));
            } else {
                self.emit_line(&format!("  {tmp} = trunc {from_type} {value} to {to_type}"));
            }
            self.track_type(&tmp, to_type);
            return tmp;
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
                self.emit_line(&format!(
                    "  {tmp} = call i32 @llvm.fptosi.sat.i32.f64(double {value})"
                ));
                self.track_type(&tmp, "i32");
            }
            (from, "i64") if from.starts_with('%') || from.starts_with('{') => {
                // Struct/aggregate → i64. Two cases:
                //
                // MULTI-FIELD aggregate: this value is being stored into a
                // pointer-sized slot (array element, map value, etc.). The
                // read side (RValue::Index/Field on an aggregate dest) does
                // `inttoptr i64 raw; load %T, ptr` — i.e. it expects the i64
                // to be a POINTER to a heap copy of the struct. So we must BOX
                // it: heap-allocate, store the aggregate, return the pointer
                // as i64. The old code did `extractvalue ..., 0` (field 0
                // only), which stored a single field's value where a pointer
                // was expected -> `load` from a tiny integer address ->
                // segfault. This was THE stage-2 array-of-struct corruption.
                //
                // SINGLE-FIELD newtype: keep the by-value `extractvalue 0`
                // semantics (dyn-trait lowering depends on it).
                let nfields: Option<usize> = if let Some(name) = from.strip_prefix('%') {
                    self.struct_defs.get(name).map(|fields| fields.len())
                } else {
                    // inline aggregate `{a, b, ...}`: multi-field iff it has a
                    // top-level comma. Conservative: treat any comma as multi.
                    Some(if from.contains(',') { 2 } else { 1 })
                };
                if nfields.map_or(false, |n| n > 1) {
                    let size_ptr = self.next_temp();
                    self.emit_line(&format!(
                        "  {size_ptr} = getelementptr {from}, ptr null, i64 1"
                    ));
                    let size_int = self.next_temp();
                    self.emit_line(&format!(
                        "  {size_int} = ptrtoint ptr {size_ptr} to i64"
                    ));
                    let heap_i64 = self.next_temp();
                    self.emit_line(&format!(
                        "  {heap_i64} = call i64 @kryos_arc_alloc_i64(i64 {size_int})"
                    ));
                    let heap_ptr = self.next_temp();
                    self.emit_line(&format!(
                        "  {heap_ptr} = inttoptr i64 {heap_i64} to ptr"
                    ));
                    self.emit_line(&format!("  store {from} {value}, ptr {heap_ptr}"));
                    self.track_type(&heap_i64, "i64");
                    return heap_i64;
                }
                let field0_llvm_ty = if let Some(name) = from.strip_prefix('%') {
                    self.struct_defs
                        .get(name)
                        .and_then(|fields| fields.first())
                        .map(|(_, t)| mir_type_to_llvm(t))
                } else {
                    None
                };
                self.emit_line(&format!(
                    "  {tmp} = extractvalue {from} {value}, 0"
                ));
                match field0_llvm_ty.as_deref() {
                    Some("ptr") => {
                        self.track_type(&tmp, "ptr");
                        let i = self.next_temp();
                        self.emit_line(&format!("  {i} = ptrtoint ptr {tmp} to i64"));
                        self.track_type(&i, "i64");
                        return i;
                    }
                    Some(narrow) if narrow == "i8" || narrow == "i16" || narrow == "i32" => {
                        self.track_type(&tmp, narrow);
                        let i = self.next_temp();
                        self.emit_line(&format!("  {i} = sext {narrow} {tmp} to i64"));
                        self.track_type(&i, "i64");
                        return i;
                    }
                    Some("double") | Some("float") => {
                        // Float field stored as i64 via bitcast.
                        let ft = field0_llvm_ty.as_deref().unwrap();
                        self.track_type(&tmp, ft);
                        let i = self.next_temp();
                        self.emit_line(&format!("  {i} = bitcast {ft} {tmp} to i64"));
                        self.track_type(&i, "i64");
                        return i;
                    }
                    _ => {
                        // i64 or unknown: track as i64 and pass through.
                        self.track_type(&tmp, "i64");
                    }
                }
            }
            (from, "ptr") if from.starts_with('%') || from.starts_with('{') => {
                // Struct/aggregate → ptr. A MULTI-FIELD aggregate flowing into
                // a pointer slot must be BOXED (heap copy, pointer returned),
                // mirroring the struct→i64 case. The old `extractvalue 0 +
                // inttoptr` produced a pointer out of field 0's value -> wild
                // pointer -> segfault/garbage. Single-field newtypes keep the
                // field-0 unwrap.
                let nfields: Option<usize> = if let Some(name) = from.strip_prefix('%') {
                    self.struct_defs.get(name).map(|fields| fields.len())
                } else {
                    Some(if from.contains(',') { 2 } else { 1 })
                };
                if nfields.map_or(false, |n| n > 1) {
                    let size_ptr = self.next_temp();
                    self.emit_line(&format!(
                        "  {size_ptr} = getelementptr {from}, ptr null, i64 1"
                    ));
                    let size_int = self.next_temp();
                    self.emit_line(&format!(
                        "  {size_int} = ptrtoint ptr {size_ptr} to i64"
                    ));
                    let heap_i64 = self.next_temp();
                    self.emit_line(&format!(
                        "  {heap_i64} = call i64 @kryos_arc_alloc_i64(i64 {size_int})"
                    ));
                    self.emit_line(&format!("  {tmp} = inttoptr i64 {heap_i64} to ptr"));
                    self.emit_line(&format!("  store {from} {value}, ptr {tmp}"));
                    self.track_type(&tmp, "ptr");
                    return tmp;
                }
                let f0 = self.next_temp();
                self.emit_line(&format!(
                    "  {f0} = extractvalue {from} {value}, 0"
                ));
                self.emit_line(&format!("  {tmp} = inttoptr i64 {f0} to ptr"));
                self.track_type(&tmp, "ptr");
            }
            // General integer width change between LLVM iN types not covered by
            // the explicit arms above (e.g. i16<->i64, i128<->i64, i16->i32).
            // Without this, a narrow int (e.g. `x as i16`) passed where i64 is
            // expected — like `to_string(x as i16)` — was emitted unchanged,
            // producing `call ...(i64 %v)` with %v actually i16, which clang
            // rejects. Signedness is not tracked at the LLVM type level, so
            // widen with sext (matching the existing i8/i32 arms); narrow with
            // trunc. i1 (bool) is handled by the explicit arms above.
            (a, b)
                if a != "i1"
                    && b != "i1"
                    && a.strip_prefix('i').and_then(|n| n.parse::<u32>().ok()).is_some()
                    && b.strip_prefix('i').and_then(|n| n.parse::<u32>().ok()).is_some() =>
            {
                let aw = a[1..].parse::<u32>().unwrap_or(64);
                let bw = b[1..].parse::<u32>().unwrap_or(64);
                if aw < bw {
                    self.emit_line(&format!("  {tmp} = sext {a} {value} to {b}"));
                } else {
                    self.emit_line(&format!("  {tmp} = trunc {a} {value} to {b}"));
                }
                self.track_type(&tmp, b);
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

    /// If `src` is an `@copy` struct local with heap-backed fields, rebuild
    /// the aggregate value with cloned str/array/map fields (and retained
    /// fn/shared fields) so the copy owns its own data. Mirrors the Cranelift
    /// backend's `emit_struct_deep_copy` on assignment; nested struct fields
    /// stay shared (H21). Returns the (possibly new) SSA value name.
    fn maybe_deep_copy_struct_fields(
        &mut self,
        val: &str,
        src: LocalId,
        func: &MirFunction,
        dest_ty: &str,
    ) -> String {
        // Only aggregate-valued destinations (`%Name`) carry fields here.
        if !dest_ty.starts_with('%') {
            return val.to_string();
        }
        let Some(sname) = func.locals.iter().find(|l| l.id == src).and_then(|l| match &l.ty {
            MirType::Struct(n) => Some(n.clone()),
            _ => None,
        }) else {
            return val.to_string();
        };
        if !self.copy_structs.contains(&sname) {
            return val.to_string();
        }
        let Some(fields) = self.struct_defs.get(&sname).cloned() else {
            return val.to_string();
        };
        let needs_work = fields.iter().any(|(_, t)| {
            matches!(
                t,
                MirType::Str
                    | MirType::Array(_, _)
                    | MirType::Map { .. }
                    | MirType::Function { .. }
                    | MirType::Shared(_)
            )
        });
        if !needs_work {
            return val.to_string();
        }
        let mut cur = val.to_string();
        for (idx, (_, fty)) in fields.iter().enumerate() {
            let fty_ll = self.sig_ty_to_llvm(fty);
            match fty {
                MirType::Str | MirType::Array(_, _) => {
                    let clone_fn = if matches!(fty, MirType::Str) {
                        "kryos_string_clone"
                    } else {
                        "kryos_array_clone"
                    };
                    let fv = self.next_temp();
                    self.emit_line(&format!(
                        "  {fv} = extractvalue {dest_ty} {cur}, {idx}"
                    ));
                    let fp = self.coerce_value(&fv, &fty_ll, "ptr");
                    let cl = self.next_temp();
                    self.emit_line(&format!("  {cl} = call ptr @{clone_fn}(ptr {fp})"));
                    let back = self.coerce_value(&cl, "ptr", &fty_ll);
                    let nv = self.next_temp();
                    self.emit_line(&format!(
                        "  {nv} = insertvalue {dest_ty} {cur}, {fty_ll} {back}, {idx}"
                    ));
                    cur = nv;
                }
                MirType::Map { .. } => {
                    let fv = self.next_temp();
                    self.emit_line(&format!(
                        "  {fv} = extractvalue {dest_ty} {cur}, {idx}"
                    ));
                    let fh = self.coerce_value(&fv, &fty_ll, "i64");
                    let cl = self.next_temp();
                    self.emit_line(&format!("  {cl} = call i64 @kryos_map_clone(i64 {fh})"));
                    let back = self.coerce_value(&cl, "i64", &fty_ll);
                    let nv = self.next_temp();
                    self.emit_line(&format!(
                        "  {nv} = insertvalue {dest_ty} {cur}, {fty_ll} {back}, {idx}"
                    ));
                    cur = nv;
                }
                MirType::Function { .. } | MirType::Shared(_) => {
                    let fv = self.next_temp();
                    self.emit_line(&format!(
                        "  {fv} = extractvalue {dest_ty} {cur}, {idx}"
                    ));
                    let fp = self.coerce_value(&fv, &fty_ll, "ptr");
                    self.emit_line(&format!("  call void @kryos_arc_retain(ptr {fp})"));
                }
                _ => {}
            }
        }
        cur
    }

    /// Emit a pending-exception check after a user-function call: if the
    /// thread-local exception flag is set, return a default value so the
    /// exception keeps unwinding; `main`'s returns report it (see
    /// emit_terminator). Labels are derived from the temp counter so they
    /// are unique within the function.
    fn emit_post_call_exception_check(&mut self, func: &MirFunction) {
        let chk = self.next_temp();
        let pend = self.next_temp();
        let id = chk.trim_start_matches('%').to_string();
        let exc_lbl = format!("exc.ret.{id}");
        let cont_lbl = format!("exc.cont.{id}");
        self.emit_line(&format!("  {chk} = call i64 @kryos_exception_check()"));
        self.emit_line(&format!("  {pend} = icmp ne i64 {chk}, 0"));
        self.emit_line(&format!("  br i1 {pend}, label %{exc_lbl}, label %{cont_lbl}"));
        self.emit_line(&format!("{exc_lbl}:"));
        if func.name == "main" {
            self.emit_line("  call void @kryos_exception_report_uncaught_if_pending()");
        }
        if self.aggregate_llvm_ty(&func.ret_ty).is_some() {
            // sret aggregate: leave the out-param untouched (dead value) and exit.
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
        self.emit_line(&format!("{cont_lbl}:"));
    }

    // -----------------------------------------------------------------------
    // Output helpers
    // -----------------------------------------------------------------------

    fn emit_line(&mut self, line: &str) {
        // When DI is active inside a function body, LLVM's verifier requires
        // every inlinable call instruction in a function with !dbg to also
        // carry a !dbg location. To avoid touching all 55+ call-emission
        // sites, we centralize the suffix here: any line whose trimmed-left
        // text begins with `call `, `tail call `, `musttail call `, or
        // `notail call ` gets a `, !dbg !<loc>` suffix appended automatically.
        //
        // We deliberately do NOT auto-suffix `ret` or `br` because the
        // emission_kind=LineTablesOnly setting permits them to be bare, and
        // attaching !dbg to terminators in unreachable / synthetic blocks
        // can produce verifier errors.
        //
        // Sites that already include `!dbg` (rare) are passed through.
        if let Some(loc_id) = self.current_fn_loc_md {
            let trimmed = line.trim_start();
            let is_call = trimmed.starts_with("call ")
                || trimmed.starts_with("tail call ")
                || trimmed.starts_with("musttail call ")
                || trimmed.starts_with("notail call ")
                || (trimmed.contains(" = call ") && !trimmed.contains("!dbg"))
                || (trimmed.contains(" = tail call ") && !trimmed.contains("!dbg"));
            if is_call && !line.contains("!dbg") {
                self.output.push_str(line);
                self.output.push_str(&format!(", !dbg !{}", loc_id));
                self.output.push('\n');
                return;
            }
        }
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
    ///
    /// `val` is a `ptr` pointing to the struct in memory.
    /// We use struct-indexed GEP (`getelementptr %S, ptr val, i32 0, i32 idx`)
    /// so that multi-word fields (e.g. inline enums = 16 bytes) are addressed
    /// at the correct byte offset regardless of field size.
    #[allow(clippy::collapsible_match)]
    fn emit_struct_drop(&mut self, val: &str, struct_name: &str, _func: &MirFunction) {
        let struct_def = match self.struct_defs.get(struct_name).cloned() {
            Some(def) => def,
            None => return,
        };

        // Use struct-indexed GEP so that variable-size fields (enums, tuples)
        // are accessed at the correct byte offset rather than at an i64 stride.
        let llvm_struct_ty = format!("%{struct_name}");

        for (field_idx, (_field_name, field_ty)) in struct_def.iter().enumerate() {
            match field_ty {
                MirType::Str => {
                    let gep = self.next_temp();
                    let fv = self.next_temp();
                    self.emit_line(&format!(
                        "  {gep} = getelementptr {llvm_struct_ty}, ptr {val}, i32 0, i32 {field_idx}"
                    ));
                    self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                    self.emit_line(&format!("  call void @kryos_string_free(ptr {fv})"));
                }
                MirType::Array(ref inner_elem, _) => {
                    let gep = self.next_temp();
                    let fv = self.next_temp();
                    self.emit_line(&format!(
                        "  {gep} = getelementptr {llvm_struct_ty}, ptr {val}, i32 0, i32 {field_idx}"
                    ));
                    self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                    let et = inner_elem.as_ref().clone();
                    self.emit_array_drop(&fv, &et, _func);
                }
                MirType::Function { .. } => {
                    let gep = self.next_temp();
                    let fv = self.next_temp();
                    self.emit_line(&format!(
                        "  {gep} = getelementptr {llvm_struct_ty}, ptr {val}, i32 0, i32 {field_idx}"
                    ));
                    self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                    self.emit_line(&format!("  call void @kryos_arc_release(ptr {fv})"));
                }
                MirType::Map { .. } => {
                    let gep = self.next_temp();
                    let fv = self.next_temp();
                    self.emit_line(&format!(
                        "  {gep} = getelementptr {llvm_struct_ty}, ptr {val}, i32 0, i32 {field_idx}"
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
                            "  {gep} = getelementptr {llvm_struct_ty}, ptr {val}, i32 0, i32 {field_idx}"
                        ));
                        self.emit_line(&format!("  {fv} = load ptr, ptr {gep}"));
                        let inner = inner_name.clone();
                        self.emit_struct_drop(&fv, &inner, _func);
                        self.emit_line(&format!("  call void @kryos_free(ptr {fv})"));
                    }
                }
                MirType::Enum(inner_name) => {
                    // Enum fields in structs are INLINE (not heap-allocated).
                    // The struct-indexed GEP gives a ptr to the inline enum.
                    // Use emit_enum_drop_payload (which drops payload WITHOUT
                    // freeing the buffer, since it's stack-allocated).
                    let gep = self.next_temp();
                    self.emit_line(&format!(
                        "  {gep} = getelementptr {llvm_struct_ty}, ptr {val}, i32 0, i32 {field_idx}"
                    ));
                    let inner = inner_name.clone();
                    self.emit_enum_drop_payload(&gep, &inner, _func);
                }
                MirType::Shared(_) => {
                    // Shared<T> fields: the struct field holds a ptr to the arc block.
                    // Load that ptr and release the arc reference.
                    let gep = self.next_temp();
                    let fv = self.next_temp();
                    self.emit_line(&format!(
                        "  {gep} = getelementptr {llvm_struct_ty}, ptr {val}, i32 0, i32 {field_idx}"
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
            //
            // KryosArray (kryos-rt/src/array.rs) is #[repr(C)] with layout:
            //   offset  0: len: i64
            //   offset  8: cap: i64
            //   offset 16: elem_size: i64
            //   offset 24: ref_count: i64
            //   offset 32: data: *mut u8
            //
            // The drop loop previously read offset 24 expecting `data`, which
            // is actually `ref_count`. Iterating a ref_count-as-pointer
            // segfaulted on the very first element access. Closes the
            // 4-line minimum repro `let p = ["hello"]` segfault that
            // surfaced test_generics' cleanup crash.
            self.emit_line(&format!("{pre_label}:"));
            let len = self.next_temp();
            self.emit_line(&format!("  {len} = load i64, ptr {val}"));
            let data_gep = self.next_temp();
            self.emit_line(&format!(
                "  {data_gep} = getelementptr i8, ptr {val}, i64 32"
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
                    self.emit_line(&format!("  call void @kryos_free(ptr {val})"));
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
                            self.emit_line(&format!("  call void @kryos_free(ptr {fv})"));
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
            self.emit_line(&format!("  call void @kryos_free(ptr {val})"));
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
/// Split a top-level LLVM aggregate type string `{ T1, T2, ... }` into its
/// field type strings, respecting nesting (`{`/`[`/`(`). Returns empty for
/// non-aggregate inputs.
fn split_aggregate_fields(agg: &str) -> Vec<String> {
    let t = agg.trim();
    let inner = match t.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        Some(i) => i,
        None => return Vec::new(),
    };
    let mut fields = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in inner.chars() {
        match c {
            '{' | '[' | '(' => {
                depth += 1;
                cur.push(c);
            }
            '}' | ']' | ')' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                fields.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        fields.push(cur.trim().to_string());
    }
    fields
}


/// Mirror of the Cranelift backend's post-call exception-check filter:
/// check after user-function and indirect/vtable calls; skip the runtime's
/// own kryos_* helpers and pure builtins that can never throw.
fn post_call_exception_check_applies(value: &RValue) -> bool {
    match value {
        RValue::Call { func, .. } => {
            !func.starts_with("kryos_")
                && !matches!(
                    func.as_str(),
                    "println"
                        | "print"
                        | "eprintln"
                        | "sleep"
                        | "sleep_ms"
                        | "sqrt"
                        | "floor"
                        | "ceil"
                        | "round"
                        | "abs"
                        | "min"
                        | "max"
                        | "assert"
                        | "assert_eq"
                        | "panic"
                        | "len"
                        | "range"
                        | "to_string"
                        | "exit"
                )
        }
        RValue::CallIndirect { .. } | RValue::VtableCall { .. } => true,
        _ => false,
    }
}

/// True if any function in the module calls `kryos_exception_throw` (what
/// `throw` lowers to, and the only thing that sets the catchable-exception
/// flag — native panics abort the process instead). When this is false the
/// auto post-call exception check is provably dead and can be elided.
/// Conservative: a CallIndirect/VtableCall to an unknown target could in
/// principle reach a throw, so their presence also forces `true`.
fn module_has_throw(functions: &[MirFunction]) -> bool {
    functions.iter().any(|f| {
        f.blocks.iter().any(|bb| {
            bb.instructions.iter().any(|inst| {
                if let Instruction::Assign { value, .. } = inst {
                    match value {
                        RValue::Call { func, .. } => func == "kryos_exception_throw",
                        RValue::CallIndirect { .. } | RValue::VtableCall { .. } => true,
                        _ => false,
                    }
                } else {
                    false
                }
            })
        })
    })
}

fn default_value_for_type(ty: &str) -> &str {
    match ty {
        "float" | "double" => "0.0",
        "ptr" => "null",
        "void" => "void",
        // Aggregate types (struct %Name, tuple/struct {..}, array [..]) cannot
        // use the integer literal `0` — `ret {i64,i64,i64} 0` is malformed IR.
        // This fires on the implicit fallthrough return of an aggregate-returning
        // function (e.g. every std::json `-> JsonValue` helper's dead default
        // block emitted `ret {i64,i64,i64} 0`).
        _ if ty.starts_with('{') || ty.starts_with('%') || ty.starts_with('[') => {
            "zeroinitializer"
        }
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
        // str-key map variants: the string key is coerced to an i64 handle. Without
        // these the string key was passed as `ptr`, mismatching the i64 declares.
        "kryos_map_insert_str" => Some(vec!["i64".into(), "i64".into(), "i64".into()]),
        // http_request(method, url, headers, body, timeout_ms) — all str
        // handles + i64, passed as i64 slots to kryos_http_request_ks.
        "http_request" => Some(vec![
            "i64".into(),
            "i64".into(),
            "i64".into(),
            "i64".into(),
            "i64".into(),
        ]),
        "kryos_map_get_str" => Some(vec!["i64".into(), "i64".into()]),
        "kryos_map_delete_str" | "kryos_map_has_str" => {
            Some(vec!["i64".into(), "i64".into()])
        }
        "kryos_map_keys_str" => Some(vec!["i64".into()]),
        // C math functions — single double argument
        "sqrt" | "floor" | "ceil" | "round" | "sin" | "cos" | "tan"
        | "log" | "log2" | "log10" | "fabs" => Some(vec!["double".into()]),
        _ => None,
    }
}
