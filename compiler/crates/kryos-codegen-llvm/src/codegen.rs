//! LLVM IR text emitter.
//!
//! Translates MIR basic blocks, instructions, and terminators into valid
//! LLVM IR text. The output can be compiled by `llc` or `clang`.

use std::collections::{HashMap, HashSet};

use kryos_mir::ir::{
    BasicBlock, Constant, EnumVariantDef, Instruction, LocalId, MirBinOp, MirFunction, MirModule,
    MirType, MirUnOp, Operand, RValue, Terminator,
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
    /// Struct definitions from the MIR module (for field access resolution).
    struct_defs: HashMap<String, Vec<(String, MirType)>>,
    /// Enum definitions from the MIR module (for enum codegen).
    enum_defs: HashMap<String, Vec<EnumVariantDef>>,
    /// Set of local IDs that need alloca/store/load (mutable or multi-assigned).
    mutable_locals: HashSet<u32>,
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
            struct_defs: HashMap::new(),
            enum_defs: HashMap::new(),
            mutable_locals: HashSet::new(),
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
        self.struct_defs = module.struct_defs.clone();
        self.enum_defs = module.enum_defs.clone();

        // Pre-scan: collect string constants, detect ARC usage, and record
        // function signatures for type-correct call emission.
        for func in &module.functions {
            self.prescan_function(func);
            let param_types: Vec<String> = func
                .params
                .iter()
                .map(|p| mir_type_to_llvm(&p.ty))
                .collect();
            self.func_param_types
                .insert(func.name.clone(), param_types);
        }

        // Module header.
        self.emit_header();

        // String constant globals.
        self.emit_string_globals();

        // ARC runtime declarations (if needed).
        if self.needs_arc_runtime {
            self.emit_arc_declarations();
        }

        // External C function declarations used by builtins.
        self.emit_extern_declarations();

        // Functions.
        // Check if we need a main() wrapper: if MIR has a void-returning `main`,
        // rename it to `_kryos_main` and emit a C-compatible `main` wrapper.
        let has_void_main = module.functions.iter().any(|f| {
            f.name == "main" && f.ret_ty == MirType::Void
        });

        for func in &module.functions {
            if has_void_main && func.name == "main" {
                self.emit_function_as(func, "_kryos_main")?;
            } else {
                self.emit_function(func)?;
            }
        }

        // Emit C-compatible main() wrapper if needed.
        if has_void_main {
            self.emit_main_wrapper();
        }

        Ok(self.output.clone())
    }

    // -----------------------------------------------------------------------
    // Module header
    // -----------------------------------------------------------------------

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
        self.emit_line("declare ptr @kryos_arc_alloc(i64, ptr)");
        self.emit_line("declare void @kryos_arc_retain(ptr)");
        self.emit_line("declare void @kryos_arc_release(ptr)");
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
        self.emit_line("declare i64 @kryos_array_get(ptr, i64)");
        self.emit_line("declare void @kryos_array_set(ptr, i64, i64)");
        self.emit_line("declare i64 @kryos_array_len(ptr)");
        self.emit_line("declare void @kryos_array_free(ptr)");
        self.emit_line("; Map runtime");
        self.emit_line("declare i64 @kryos_map_new()");
        self.emit_line("declare void @kryos_map_insert(i64, i64, i64)");
        self.emit_line("declare void @kryos_map_insert_str(i64, i64, i64)");
        self.emit_line("declare i64 @kryos_map_get(i64, i64)");
        self.emit_line("declare i64 @kryos_map_get_str(i64, i64)");
        self.emit_line("declare i64 @kryos_map_len(i64)");
        self.emit_line("declare void @kryos_map_free(i64)");
        self.emit_line("; Builtin runtime");
        self.emit_line("declare i64 @kryos_builtin_len(i64)");
        self.emit_line("declare i64 @kryos_builtin_to_string(i64)");
        self.emit_line("declare i64 @kryos_ipow(i64, i64)");
        self.emit_line("declare double @kryos_fpow(double, double)");
        self.emit_line("declare double @kryos_fmod(double, double)");
        self.emit_line("declare i64 @kryos_i64_to_string(i64)");
        self.emit_line("declare i64 @kryos_f64_to_string(double)");
        self.emit_line("declare i64 @kryos_bool_to_string(i64)");
        // Channel runtime
        self.emit_line("declare i64 @kryos_chan_new_i64()");
        self.emit_line("declare i64 @kryos_chan_send_i64(i64, i64)");
        self.emit_line("declare i64 @kryos_chan_recv_i64(i64)");
        // Print runtime (for KryosString handles)
        self.emit_line("declare void @kryos_println_str(ptr)");
        self.emit_line("declare void @kryos_print_str(ptr)");
        self.emit_line("declare void @kryos_eprintln_str(ptr)");
        // Spawn runtime
        self.emit_line("declare i64 @kryos_spawn(i64, ptr, i64)");
        self.emit_line("declare void @kryos_spawn_wait_all()");
        self.emit_line("declare void @kryos_sleep(i64)");
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
    // Main wrapper
    // -----------------------------------------------------------------------

    fn emit_main_wrapper(&mut self) {
        self.emit_line("; C-compatible main() entry point");
        self.emit_line("define i32 @main() {");
        self.emit_line("entry:");
        self.emit_line("  call void @_kryos_main()");
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
                    Instruction::ArcRetain { .. }
                    | Instruction::ArcRelease { .. } => {
                        self.needs_arc_runtime = true;
                    }
                    Instruction::Drop { .. } | Instruction::Nop
                    | Instruction::Spawn { .. } | Instruction::Send { .. }
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
                if let Some(s) = start { self.prescan_operand(s); }
                if let Some(e) = end { self.prescan_operand(e); }
            }
            RValue::Comptime(inner) => self.prescan_rvalue(inner),
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

        let ret = mir_type_to_llvm(&func.ret_ty);
        let params = func
            .params
            .iter()
            .map(|p| format!("{} %_{}", mir_type_to_llvm(&p.ty), p.local.0))
            .collect::<Vec<_>>()
            .join(", ");

        self.emit_line(&format!("define {ret} @{name}({params}) {{"));

        // Emit entry block label and allocas for mutable locals.
        let first_block = &func.blocks[0];
        self.emit_line(&format!("bb{}:", first_block.id.0));

        // Emit allocas for all mutable locals at the top of the entry block.
        let _param_ids: HashSet<u32> = func.params.iter().map(|p| p.local.0).collect();
        for local in &func.locals {
            if self.mutable_locals.contains(&local.id.0) {
                let ty = mir_type_to_llvm(&local.ty);
                if ty != "void" {
                    self.emit_line(&format!("  %_{}.addr = alloca {ty}", local.id.0));
                }
            }
        }
        // Store parameter values into their allocas.
        for param in &func.params {
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
                self.emit_line(&format!(
                    "  call void @kryos_arc_retain(ptr %_{})",
                    ptr.0
                ));
            }
            Instruction::ArcRelease { ptr } => {
                self.emit_line(&format!(
                    "  call void @kryos_arc_release(ptr %_{})",
                    ptr.0
                ));
            }
            Instruction::Drop { local } => {
                // For string locals, call kryos_string_free to release the heap allocation.
                let is_str = func
                    .locals
                    .iter()
                    .find(|l| l.id == *local)
                    .map_or(false, |l| l.ty == MirType::Str);
                if is_str {
                    let val = if self.mutable_locals.contains(&local.0) {
                        let tmp = self.next_temp();
                        self.emit_line(&format!("  {tmp} = load ptr, ptr %_{}.addr", local.0));
                        tmp
                    } else {
                        format!("%_{}", local.0)
                    };
                    self.emit_line(&format!("  call void @kryos_string_free(ptr {val})"));
                } else {
                    self.emit_line("  ; drop (no-op)");
                }
            }
            Instruction::Nop => {}
            Instruction::Spawn { func: spawn_fn, args } => {
                // Get function pointer.
                let tmp_fptr = self.next_temp();
                self.emit_line(&format!(
                    "  {tmp_fptr} = ptrtoint ptr @{spawn_fn} to i64"
                ));
                if args.is_empty() {
                    // kryos_spawn(fn_ptr, null, 0)
                    self.emit_line(&format!(
                        "  call i64 @kryos_spawn(i64 {tmp_fptr}, ptr null, i64 0)"
                    ));
                } else {
                    // Alloca for args array.
                    let arr = self.next_temp();
                    self.emit_line(&format!(
                        "  {arr} = alloca i64, i32 {}", args.len()
                    ));
                    for (i, arg) in args.iter().enumerate() {
                        let val = self.operand_to_llvm(arg, func);
                        let gep = self.next_temp();
                        self.emit_line(&format!(
                            "  {gep} = getelementptr i64, ptr {arr}, i32 {i}"
                        ));
                        self.emit_line(&format!(
                            "  store i64 {val}, ptr {gep}"
                        ));
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
                self.emit_line(&format!(
                    "  call i64 @kryos_chan_send_i64(i64 {ch}, i64 {val})"
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
        }
        Ok(())
    }

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
                if dest_ty == "void" {
                    return Ok(());
                }
                if is_mutable {
                    // For mutable locals: compute value, store to alloca.
                    let tmp = self.next_temp();
                    self.emit_line(&format!("  {tmp} = add {dest_ty} {val}, 0"));
                    self.emit_line(&format!("  store {dest_ty} {tmp}, ptr %_{}.addr", dest.0));
                } else {
                    self.emit_line(&format!(
                        "  %_{} = add {dest_ty} {val}, 0",
                        dest.0
                    ));
                }
            }

            // ----- Binary ops -----
            RValue::BinOp { op, left, right } => {
                // String operations: dispatch to runtime instead of integer ops.
                let is_string = Self::operand_is_string(left, func)
                    || Self::operand_is_string(right, func);

                if is_string && *op == MirBinOp::Add {
                    let left_val = self.operand_to_llvm(left, func);
                    let right_val = self.operand_to_llvm(right, func);
                    if is_mutable {
                        let tmp = self.next_temp();
                        self.emit_line(&format!(
                            "  {tmp} = call ptr @kryos_string_concat(ptr {left_val}, ptr {right_val})"
                        ));
                        self.emit_line(&format!("  store ptr {tmp}, ptr %_{}.addr", dest.0));
                    } else {
                        self.emit_line(&format!(
                            "  %_{} = call ptr @kryos_string_concat(ptr {left_val}, ptr {right_val})",
                            dest.0
                        ));
                    }
                } else if is_string && (*op == MirBinOp::Eq || *op == MirBinOp::Neq) {
                    let left_val = self.operand_to_llvm(left, func);
                    let right_val = self.operand_to_llvm(right, func);
                    let eq_tmp = self.next_temp();
                    self.emit_line(&format!(
                        "  {eq_tmp} = call i1 @kryos_string_eq(ptr {left_val}, ptr {right_val})"
                    ));
                    if *op == MirBinOp::Neq {
                        let neq_tmp = if is_mutable { self.next_temp() } else { format!("%_{}", dest.0) };
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
                    let left_val = self.operand_to_llvm(left, func);
                    let right_val = self.operand_to_llvm(right, func);
                    let is_float = self.operand_is_float(left, func);
                    let operand_ty = self.operand_type(left, func);

                    if is_mutable {
                        let tmp = self.next_temp();
                        self.emit_binop_to(&tmp, *op, &left_val, &right_val, &operand_ty, is_float)?;
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
                        self.emit_line(&format!("  call void @{print_fn}(ptr {val})"));
                    } else {
                        // Non-string arg: convert to string first.
                        let val = self.operand_to_llvm(&args[0], func);
                        let handle_i64 = self.next_temp();
                        self.emit_line(&format!(
                            "  {handle_i64} = call i64 @kryos_builtin_to_string(i64 {val})"
                        ));
                        let handle_ptr = self.next_temp();
                        self.emit_line(&format!(
                            "  {handle_ptr} = inttoptr i64 {handle_i64} to ptr"
                        ));
                        self.emit_line(&format!("  call void @{print_fn}(ptr {handle_ptr})"));
                    }
                } else {

                // Look up the callee's parameter types for type-correct emission.
                let callee_param_types = self.func_param_types.get(fname.as_str()).cloned();

                let arg_list = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        let inferred_ty = self.operand_type(a, func);
                        let ty = callee_param_types
                            .as_ref()
                            .and_then(|pts| pts.get(i))
                            .cloned()
                            .unwrap_or(inferred_ty);
                        let val = self.operand_to_llvm(a, func);
                        format!("{ty} {val}")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");

                match fname.as_str() {
                    "exit" => {
                        self.emit_line(&format!("  call void @exit({arg_list})"));
                    }
                    "len" => {
                        let arg = if !args.is_empty() {
                            self.operand_to_llvm(&args[0], func)
                        } else {
                            "0".to_string()
                        };
                        if is_mutable {
                            let tmp = self.next_temp();
                            self.emit_line(&format!("  %t{tmp} = call i64 @kryos_builtin_len(i64 {arg})"));
                            self.emit_line(&format!("  store i64 %t{tmp}, ptr %_{}.addr", dest.0));
                        } else {
                            self.emit_line(&format!("  %_{} = call i64 @kryos_builtin_len(i64 {arg})", dest.0));
                        }
                    }
                    "to_string" => {
                        let val = if !args.is_empty() {
                            self.operand_to_llvm(&args[0], func)
                        } else {
                            "0".to_string()
                        };
                        if is_mutable {
                            let tmp = self.next_temp();
                            self.emit_line(&format!("  %t{tmp} = call i64 @kryos_builtin_to_string(i64 {val})"));
                            self.emit_line(&format!("  store i64 %t{tmp}, ptr %_{}.addr", dest.0));
                        } else {
                            self.emit_line(&format!("  %_{} = call i64 @kryos_builtin_to_string(i64 {val})", dest.0));
                        }
                    }
                    "chan" => {
                        if is_mutable {
                            let tmp = self.next_temp();
                            self.emit_line(&format!("  {tmp} = call i64 @kryos_chan_new_i64()"));
                            self.emit_line(&format!("  store i64 {tmp}, ptr %_{}.addr", dest.0));
                        } else {
                            self.emit_line(&format!("  %_{} = call i64 @kryos_chan_new_i64()", dest.0));
                        }
                    }
                    "send" => {
                        let ch = if !args.is_empty() { self.operand_to_llvm(&args[0], func) } else { "0".into() };
                        let val = if args.len() > 1 { self.operand_to_llvm(&args[1], func) } else { "0".into() };
                        self.emit_line(&format!("  call i64 @kryos_chan_send_i64(i64 {ch}, i64 {val})"));
                    }
                    "recv" => {
                        let ch = if !args.is_empty() { self.operand_to_llvm(&args[0], func) } else { "0".into() };
                        if is_mutable {
                            let tmp = self.next_temp();
                            self.emit_line(&format!("  {tmp} = call i64 @kryos_chan_recv_i64(i64 {ch})"));
                            self.emit_line(&format!("  store i64 {tmp}, ptr %_{}.addr", dest.0));
                        } else {
                            self.emit_line(&format!("  %_{} = call i64 @kryos_chan_recv_i64(i64 {ch})", dest.0));
                        }
                    }
                    _ => {
                        if dest_ty == "void" {
                            self.emit_line(&format!("  call void @{fname}({arg_list})"));
                        } else if is_mutable {
                            let tmp = self.next_temp();
                            self.emit_line(&format!(
                                "  {tmp} = call {dest_ty} @{fname}({arg_list})"
                            ));
                            self.emit_line(&format!("  store {dest_ty} {tmp}, ptr %_{}.addr", dest.0));
                        } else {
                            self.emit_line(&format!(
                                "  %_{} = call {dest_ty} @{fname}({arg_list})",
                                dest.0
                            ));
                        }
                    }
                }
                } // close else (non-print call path)
            }

            RValue::CallIndirect { callee, args } => {
                // Indirect call: callee is a function pointer stored as i64.
                let fn_ptr_val = self.operand_to_llvm(callee, func);

                // Build the argument list (all i64 in Kryos uniform slot model).
                let arg_list = args
                    .iter()
                    .map(|a| {
                        let val = self.operand_to_llvm(a, func);
                        format!("i64 {val}")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");

                // Cast i64 to function pointer, then call indirect.
                let fn_ptr = self.next_temp();
                self.emit_line(&format!(
                    "  {fn_ptr} = inttoptr i64 {fn_ptr_val} to ptr"
                ));

                if is_mutable {
                    let tmp = self.next_temp();
                    self.emit_line(&format!(
                        "  {tmp} = call {dest_ty} {fn_ptr}({arg_list})"
                    ));
                    self.emit_line(&format!("  store {dest_ty} {tmp}, ptr %_{}.addr", dest.0));
                } else {
                    self.emit_line(&format!(
                        "  %_{} = call {dest_ty} {fn_ptr}({arg_list})",
                        dest.0
                    ));
                }
            }

            // ----- Constants -----
            RValue::ConstInt(v) => {
                if is_mutable {
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
                    self.emit_line(&format!(
                        "  %_{} = fadd {dest_ty} {hex}, 0.0",
                        dest.0
                    ));
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
                    self.emit_line(&format!("  store {dest_ty} {target_name}, ptr %_{}.addr", dest.0));
                }
            }
            RValue::Index { object, index } => {
                // Array/tuple pointer + index: GEP to compute address, then load.
                let obj_val = self.operand_to_llvm(object, func);
                let idx_val = self.operand_to_llvm(index, func);
                let idx_ty = self.operand_type(index, func);
                let elem_ptr = self.next_temp();
                self.emit_line(&format!(
                    "  {elem_ptr} = getelementptr i64, ptr {obj_val}, {idx_ty} {idx_val}"
                ));
                let target_name = if is_mutable {
                    self.next_temp()
                } else {
                    format!("%_{}", dest.0)
                };
                self.emit_line(&format!(
                    "  {target_name} = load {dest_ty}, ptr {elem_ptr}"
                ));
                if is_mutable {
                    self.emit_line(&format!("  store {dest_ty} {target_name}, ptr %_{}.addr", dest.0));
                }
            }

            // ----- ARC alloc -----
            RValue::ArcAlloc { inner } => {
                let inner_val = self.operand_to_llvm(inner, func);
                let inner_ty = self.operand_type(inner, func);
                let tmp = self.next_temp();
                self.emit_line(&format!(
                    "  {tmp} = inttoptr {inner_ty} {inner_val} to ptr"
                ));
                let target_name = if is_mutable {
                    self.next_temp()
                } else {
                    format!("%_{}", dest.0)
                };
                self.emit_line(&format!(
                    "  {target_name} = call ptr @kryos_arc_alloc(i64 8, ptr {tmp})"
                ));
                if is_mutable {
                    self.emit_line(&format!("  store ptr {target_name}, ptr %_{}.addr", dest.0));
                }
            }

            // ----- Enums -----
            RValue::EnumVariant { enum_name, variant_idx, fields } => {
                let max_fields = self.enum_max_fields(enum_name);
                let llvm_ty = self.enum_llvm_type(enum_name, max_fields);

                if fields.is_empty() {
                    // Unit variant: just the tag.
                    let target = if is_mutable { self.next_temp() } else { format!("%_{}", dest.0) };
                    self.emit_line(&format!(
                        "  {target} = insertvalue {llvm_ty} undef, i64 {variant_idx}, 0"
                    ));
                    if is_mutable {
                        self.emit_line(&format!("  store {llvm_ty} {target}, ptr %_{}.addr", dest.0));
                    }
                } else {
                    // Tag + fields via chained insertvalue.
                    let tag_tmp = self.next_temp();
                    self.emit_line(&format!(
                        "  {tag_tmp} = insertvalue {llvm_ty} undef, i64 {variant_idx}, 0"
                    ));
                    let mut current = tag_tmp;

                    for (i, field_op) in fields.iter().enumerate() {
                        let val = self.operand_to_llvm(field_op, func);
                        let val_ty = self.operand_type(field_op, func);
                        let is_last = i + 1 == fields.len();
                        let target = if is_last && !is_mutable {
                            format!("%_{}", dest.0)
                        } else {
                            self.next_temp()
                        };
                        self.emit_line(&format!(
                            "  {target} = insertvalue {llvm_ty} {current}, {val_ty} {val}, {idx}",
                            idx = i + 1
                        ));
                        current = target;
                    }

                    if is_mutable {
                        self.emit_line(&format!("  store {llvm_ty} {current}, ptr %_{}.addr", dest.0));
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
                self.emit_line(&format!(
                    "  {target_name} = extractvalue {obj_ty} {val}, 0"
                ));
                if is_mutable {
                    self.emit_line(&format!("  store i64 {target_name}, ptr %_{}.addr", dest.0));
                }
            }
            RValue::EnumPayload { operand, field_idx, .. } => {
                let val = self.operand_to_llvm(operand, func);
                let obj_ty = self.operand_type(operand, func);
                let target_name = if is_mutable {
                    self.next_temp()
                } else {
                    format!("%_{}", dest.0)
                };
                self.emit_line(&format!(
                    "  {target_name} = extractvalue {obj_ty} {val}, {idx}",
                    idx = field_idx + 1
                ));
                if is_mutable {
                    self.emit_line(&format!("  store {dest_ty} {target_name}, ptr %_{}.addr", dest.0));
                }
            }

            // ----- Cast -----
            RValue::Cast { operand, ty } => {
                self.emit_cast(dest, operand, ty, func, is_mutable)?;
            }

            RValue::Closure { func_name, captures } => {
                // Closure: store function pointer as i64.
                // If captures exist, allocate env struct [func_ptr, cap0, cap1, ...].
                if captures.is_empty() {
                    let fptr = self.next_temp();
                    self.emit_line(&format!(
                        "  {fptr} = ptrtoint ptr @{func_name} to i64"
                    ));
                    if is_mutable {
                        self.emit_line(&format!("  store i64 {fptr}, ptr %_{}.addr", dest.0));
                    } else {
                        self.emit_line(&format!("  %_{} = add i64 {fptr}, 0", dest.0));
                    }
                } else {
                    // Allocate closure env: [func_ptr: i64, cap0: i64, cap1: i64, ...]
                    let env_size = (1 + captures.len()) * 8;
                    let env_ptr = self.next_temp();
                    self.emit_line(&format!(
                        "  {env_ptr} = call ptr @malloc(i64 {env_size})"
                    ));
                    // Store function pointer at offset 0.
                    let fptr = self.next_temp();
                    self.emit_line(&format!(
                        "  {fptr} = ptrtoint ptr @{func_name} to i64"
                    ));
                    self.emit_line(&format!("  store i64 {fptr}, ptr {env_ptr}"));
                    // Store each capture at offset (i+1)*8.
                    for (i, cap) in captures.iter().enumerate() {
                        let cap_val = self.operand_to_llvm(cap, func);
                        let cap_ptr = self.next_temp();
                        self.emit_line(&format!(
                            "  {cap_ptr} = getelementptr i64, ptr {env_ptr}, i64 {}",
                            i + 1
                        ));
                        self.emit_line(&format!("  store i64 {cap_val}, ptr {cap_ptr}"));
                    }
                    let env_int = self.next_temp();
                    self.emit_line(&format!("  {env_int} = ptrtoint ptr {env_ptr} to i64"));
                    if is_mutable {
                        self.emit_line(&format!("  store i64 {env_int}, ptr %_{}.addr", dest.0));
                    } else {
                        self.emit_line(&format!("  %_{} = add i64 {env_int}, 0", dest.0));
                    }
                }
            }

            RValue::Map(entries) => {
                // Create map via runtime, then insert each key-value pair.
                let map_handle = self.next_temp();
                self.emit_line(&format!("  {map_handle} = call i64 @kryos_map_new()"));
                for (k, v) in entries {
                    let key_val = self.operand_to_llvm(k, func);
                    let val_val = self.operand_to_llvm(v, func);
                    self.emit_line(&format!(
                        "  call void @kryos_map_insert(i64 {map_handle}, i64 {key_val}, i64 {val_val})"
                    ));
                }
                if is_mutable {
                    self.emit_line(&format!("  store i64 {map_handle}, ptr %_{}.addr", dest.0));
                } else {
                    self.emit_line(&format!("  %_{} = add i64 {map_handle}, 0", dest.0));
                }
            }

            RValue::Range { start, end, inclusive } => {
                // Range layout: { i64 start, i64 end, i64 inclusive } — alloca 3 x i64.
                let range_ptr = self.next_temp();
                self.emit_line(&format!("  {range_ptr} = alloca [3 x i64]"));
                // Store start.
                let start_val = match start {
                    Some(op) => self.operand_to_llvm(op, func),
                    None => "0".to_string(),
                };
                let start_ptr = self.next_temp();
                self.emit_line(&format!("  {start_ptr} = getelementptr i64, ptr {range_ptr}, i64 0"));
                self.emit_line(&format!("  store i64 {start_val}, ptr {start_ptr}"));
                // Store end.
                let end_val = match end {
                    Some(op) => self.operand_to_llvm(op, func),
                    None => format!("{}", i64::MAX),
                };
                let end_ptr = self.next_temp();
                self.emit_line(&format!("  {end_ptr} = getelementptr i64, ptr {range_ptr}, i64 1"));
                self.emit_line(&format!("  store i64 {end_val}, ptr {end_ptr}"));
                // Store inclusive flag.
                let incl_ptr = self.next_temp();
                self.emit_line(&format!("  {incl_ptr} = getelementptr i64, ptr {range_ptr}, i64 2"));
                self.emit_line(&format!("  store i64 {}, ptr {incl_ptr}", *inclusive as i64));
                // Assign pointer to dest.
                let ptr_val = self.next_temp();
                self.emit_line(&format!("  {ptr_val} = ptrtoint ptr {range_ptr} to i64"));
                if is_mutable {
                    self.emit_line(&format!("  store i64 {ptr_val}, ptr %_{}.addr", dest.0));
                } else {
                    self.emit_line(&format!("  %_{} = add i64 {ptr_val}, 0", dest.0));
                }
            }

            RValue::Comptime(inner) => {
                // Comptime: lower the inner RValue directly (const-eval at MIR level).
                self.emit_assign(dest, inner, func)?;
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
                        self.emit_line(&format!("  %_{} = getelementptr i8, ptr {val}, i64 0", dest.0));
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
                        acc = next_acc;
                    }
                    if is_mutable {
                        self.emit_line(&format!("  store ptr {acc}, ptr %_{}.addr", dest.0));
                    } else {
                        self.emit_line(&format!("  %_{} = getelementptr i8, ptr {acc}, i64 0", dest.0));
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
    /// index of `field` within the struct type of `object`, or 0 as fallback.
    fn resolve_field_index(&self, object: &Operand, field: &str, func: &MirFunction) -> usize {
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

        if let Some(name) = struct_name {
            if let Some(fields) = self.struct_defs.get(&name) {
                for (i, (fname, _)) in fields.iter().enumerate() {
                    if fname == field {
                        return i;
                    }
                }
            }
        }
        0 // Fallback
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
        _is_mutable: bool,
    ) -> Result<(), CodegenError> {
        // Build up with insertvalue.
        for (i, elem) in elems.iter().enumerate() {
            let elem_val = self.operand_to_llvm(elem, func);
            let elem_ty = self.operand_type(elem, func);
            let prev = if i == 0 {
                format!("undef")
            } else {
                format!("%_{}_arr_{}", dest.0, i - 1)
            };
            let this = if i + 1 == elems.len() {
                format!("%_{}", dest.0)
            } else {
                format!("%_{}_arr_{}", dest.0, i)
            };
            self.emit_line(&format!(
                "  {this} = insertvalue {dest_ty} {prev}, {elem_ty} {elem_val}, {i}"
            ));
        }

        if elems.is_empty() {
            // Empty array — just produce undef.
            self.emit_line(&format!(
                "  %_{} = insertvalue {dest_ty} undef, i8 0, 0",
                dest.0
            ));
        }

        Ok(())
    }

    fn emit_aggregate_tuple(
        &mut self,
        dest: LocalId,
        elems: &[Operand],
        dest_ty: &str,
        func: &MirFunction,
        _is_mutable: bool,
    ) -> Result<(), CodegenError> {
        // Same approach as arrays — insertvalue into a struct type.
        for (i, elem) in elems.iter().enumerate() {
            let elem_val = self.operand_to_llvm(elem, func);
            let elem_ty = self.operand_type(elem, func);
            let prev = if i == 0 {
                "undef".to_string()
            } else {
                format!("%_{}_tup_{}", dest.0, i - 1)
            };
            let this = if i + 1 == elems.len() {
                format!("%_{}", dest.0)
            } else {
                format!("%_{}_tup_{}", dest.0, i)
            };
            self.emit_line(&format!(
                "  {this} = insertvalue {dest_ty} {prev}, {elem_ty} {elem_val}, {i}"
            ));
        }

        if elems.is_empty() {
            self.emit_line(&format!("  %_{} = insertvalue {dest_ty} undef, i8 0, 0", dest.0));
        }

        Ok(())
    }

    fn emit_aggregate_struct(
        &mut self,
        dest: LocalId,
        fields: &[(String, Operand)],
        dest_ty: &str,
        func: &MirFunction,
        _is_mutable: bool,
    ) -> Result<(), CodegenError> {
        // Structs are lowered identically to tuples in LLVM IR (insertvalue by index).
        for (i, (field_name, op)) in fields.iter().enumerate() {
            let val = self.operand_to_llvm(op, func);
            let ty = self.operand_type(op, func);
            let prev = if i == 0 {
                "undef".to_string()
            } else {
                format!("%_{}_fld_{}", dest.0, i - 1)
            };
            let this = if i + 1 == fields.len() {
                format!("%_{}", dest.0)
            } else {
                format!("%_{}_fld_{}", dest.0, i)
            };
            self.emit_line(&format!(
                "  {this} = insertvalue {dest_ty} {prev}, {ty} {val}, {i} ; .{field_name}"
            ));
        }

        if fields.is_empty() {
            self.emit_line(&format!(
                "  %_{} = insertvalue {dest_ty} undef, i8 0, 0",
                dest.0
            ));
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
                self.emit_line(&format!("  {tmp} = add {dst_ty} {src_val}, 0"));
                self.emit_line(&format!("  store {dst_ty} {tmp}, ptr %_{}.addr", dest.0));
            } else {
                self.emit_line(&format!("  %_{} = add {dst_ty} {src_val}, 0", dest.0));
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
                let ret_ty = mir_type_to_llvm(&func.ret_ty);
                if ret_ty == "void" {
                    self.emit_line("  ret void");
                } else {
                    // Non-void function with bare return (e.g. cleanup block).
                    // Emit a zero-value return to keep LLVM IR valid.
                    let zero = default_value_for_type(&ret_ty);
                    self.emit_line(&format!("  ret {ret_ty} {zero}"));
                }
            }
            Terminator::Return(Some(op)) => {
                let ty = self.operand_type(op, func);
                let val = self.operand_to_llvm(op, func);
                self.emit_line(&format!("  ret {ty} {val}"));
            }
            Terminator::Goto(target) => {
                self.emit_line(&format!("  br label %bb{}", target.0));
            }
            Terminator::Branch {
                cond,
                then_block,
                else_block,
            } => {
                let cond_val = self.operand_to_llvm(cond, func);
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
                let cases = targets
                    .iter()
                    .map(|(v, b)| format!("    {ty} {v}, label %bb{}", b.0))
                    .collect::<Vec<_>>()
                    .join("\n");
                self.emit_line(&format!(
                    "  switch {ty} {val}, label %bb{} [\n{cases}\n  ]",
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
                .map_or(false, |l| l.ty == MirType::Str),
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
        MirType::Ptr(_) => "ptr".into(),
        MirType::Shared(_) => "ptr".into(),
        MirType::Array(elem, Some(n)) => {
            let inner = mir_type_to_llvm(elem);
            format!("[{n} x {inner}]")
        }
        MirType::Array(_elem, None) => {
            // Unsized array — represent as a pointer.
            "ptr".into()
        }
        MirType::Tuple(elems) => {
            let parts: Vec<String> = elems.iter().map(|e| mir_type_to_llvm(e)).collect();
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
