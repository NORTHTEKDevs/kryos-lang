//! JIT compilation: MIR -> Cranelift IR -> executable function pointer.
//!
//! Used for `kryos run` (JIT the whole program) and REPL (JIT individual
//! expressions). The JIT module allocates executable memory and returns
//! raw function pointers that can be called directly.

use std::collections::HashMap;

use cranelift_codegen::ir::{types, AbiParam, Function, Signature, UserFuncName};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_frontend::FunctionBuilderContext;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};
use cranelift_frontend::FunctionBuilder;

use kryos_mir::ir::{MirFunction, MirModule};

use crate::codegen::build_signature;
use crate::CodegenError;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// JIT compile a single MIR function, returning a pointer to executable memory.
///
/// # Safety
///
/// The returned pointer is valid as long as the internal JIT module is alive.
/// Callers must cast it to the appropriate function pointer type before calling.
pub fn jit_compile_function(func: &MirFunction) -> Result<*const u8, CodegenError> {
    let mut jit = JitCompiler::new()?;
    jit.compile_function(func)
}

/// JIT compile all functions in a MIR module, returning a map of function
/// names to executable pointers.
pub fn jit_compile_module(
    module: &MirModule,
) -> Result<HashMap<String, *const u8>, CodegenError> {
    let mut jit = JitCompiler::new()?;
    let mut result = HashMap::new();
    for func in &module.functions {
        let ptr = jit.compile_function(func)?;
        result.insert(func.name.clone(), ptr);
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// JIT compiler
// ---------------------------------------------------------------------------

/// Wraps a Cranelift `JITModule` with helper state for incremental compilation.
pub struct JitCompiler {
    module: JITModule,
    fb_ctx: FunctionBuilderContext,
    func_counter: u32,
}

impl JitCompiler {
    /// Create a new JIT compiler targeting the host machine.
    pub fn new() -> Result<Self, CodegenError> {
        let mut flag_builder = settings::builder();
        flag_builder
            .set("opt_level", "speed")
            .map_err(|e| CodegenError::Target(e.to_string()))?;

        let isa_builder = cranelift_native::builder()
            .map_err(|e| CodegenError::Target(e.to_string()))?;

        let mut jit_builder = JITBuilder::with_isa(
            isa_builder
                .finish(settings::Flags::new(flag_builder))
                .map_err(|e| CodegenError::Target(e.to_string()))?,
            cranelift_module::default_libcall_names(),
        );

        // Register ARC runtime stubs — ARC is not active in JIT mode.
        jit_builder.symbol("kryos_arc_retain", kryos_arc_retain_stub as *const u8);
        jit_builder.symbol("kryos_arc_release", kryos_arc_release_stub as *const u8);
        jit_builder.symbol("kryos_arc_alloc", kryos_arc_alloc_stub as *const u8);

        // Register real kryos-rt function implementations. Since the JIT
        // runs in-process, we pass the actual runtime function addresses
        // instead of stubs. This gives `kryos run` full runtime support.

        // String operations
        jit_builder.symbol("kryos_string_new", kryos_rt::string::kryos_string_new as *const u8);
        jit_builder.symbol("kryos_string_concat", kryos_rt::string::kryos_string_concat as *const u8);
        jit_builder.symbol("kryos_string_len", kryos_rt::string::kryos_string_len as *const u8);
        jit_builder.symbol("kryos_string_eq", kryos_rt::string::kryos_string_eq as *const u8);
        jit_builder.symbol("kryos_string_slice", kryos_rt::string::kryos_string_slice as *const u8);
        jit_builder.symbol("kryos_string_find", kryos_rt::string::kryos_string_find as *const u8);
        jit_builder.symbol("kryos_string_free", kryos_rt::string::kryos_string_free as *const u8);

        // Array operations
        jit_builder.symbol("kryos_array_new", kryos_rt::array::kryos_array_new as *const u8);
        jit_builder.symbol("kryos_array_push", kryos_rt::array::kryos_array_push as *const u8);
        jit_builder.symbol("kryos_array_get", kryos_rt::array::kryos_array_get as *const u8);
        jit_builder.symbol("kryos_array_set", kryos_rt::array::kryos_array_set as *const u8);
        jit_builder.symbol("kryos_array_len", kryos_rt::array::kryos_array_len as *const u8);
        jit_builder.symbol("kryos_array_free", kryos_rt::array::kryos_array_free as *const u8);

        // Map operations
        jit_builder.symbol("kryos_map_new", kryos_rt::map::kryos_map_new as *const u8);
        jit_builder.symbol("kryos_map_insert", kryos_rt::map::kryos_map_insert as *const u8);
        jit_builder.symbol("kryos_map_insert_str", kryos_rt::map::kryos_map_insert_str as *const u8);
        jit_builder.symbol("kryos_map_get", kryos_rt::map::kryos_map_get as *const u8);
        jit_builder.symbol("kryos_map_get_str", kryos_rt::map::kryos_map_get_str as *const u8);
        jit_builder.symbol("kryos_map_len", kryos_rt::map::kryos_map_len as *const u8);
        jit_builder.symbol("kryos_map_free", kryos_rt::map::kryos_map_free as *const u8);

        // Builtins and type conversions
        jit_builder.symbol("kryos_builtin_len", kryos_rt::builtins::kryos_builtin_len as *const u8);
        jit_builder.symbol("kryos_builtin_to_string", kryos_rt::builtins::kryos_builtin_to_string as *const u8);
        jit_builder.symbol("kryos_i64_to_string", kryos_rt::builtins::kryos_i64_to_string as *const u8);
        jit_builder.symbol("kryos_f64_to_string", kryos_rt::builtins::kryos_f64_to_string as *const u8);
        jit_builder.symbol("kryos_bool_to_string", kryos_rt::builtins::kryos_bool_to_string as *const u8);
        jit_builder.symbol("kryos_ipow", kryos_rt::builtins::kryos_ipow as *const u8);
        jit_builder.symbol("kryos_fpow", kryos_rt::builtins::kryos_fpow as *const u8);
        jit_builder.symbol("kryos_fmod", kryos_rt::builtins::kryos_fmod as *const u8);

        // Print operations
        jit_builder.symbol("kryos_println_str", kryos_rt::builtins::kryos_println_str as *const u8);
        jit_builder.symbol("kryos_print_str", kryos_rt::builtins::kryos_print_str as *const u8);
        jit_builder.symbol("kryos_eprintln_str", kryos_rt::builtins::kryos_eprintln_str as *const u8);

        // Channel operations
        jit_builder.symbol("kryos_chan_new_i64", kryos_rt::builtins::kryos_chan_new_i64 as *const u8);
        jit_builder.symbol("kryos_chan_send_i64", kryos_rt::builtins::kryos_chan_send_i64 as *const u8);
        jit_builder.symbol("kryos_chan_recv_i64", kryos_rt::builtins::kryos_chan_recv_i64 as *const u8);

        // Spawn runtime
        jit_builder.symbol("kryos_spawn", kryos_rt::spawn::kryos_spawn as *const u8);
        jit_builder.symbol("kryos_spawn_wait_all", kryos_rt::spawn::kryos_spawn_wait_all as *const u8);
        jit_builder.symbol("kryos_sleep", kryos_rt::spawn::kryos_sleep as *const u8);

        let module = JITModule::new(jit_builder);

        Ok(Self {
            module,
            fb_ctx: FunctionBuilderContext::new(),
            func_counter: 0,
        })
    }

    /// Compile a single MIR function and return its executable pointer.
    pub fn compile_function(
        &mut self,
        mir_func: &MirFunction,
    ) -> Result<*const u8, CodegenError> {
        let call_conv = self.module.isa().default_call_conv();
        let sig = build_signature(mir_func, call_conv);

        // Declare ARC runtime functions.
        let arc_retain_sig = {
            let mut s = Signature::new(call_conv);
            s.params.push(AbiParam::new(types::I64));
            s
        };
        let arc_release_sig = arc_retain_sig.clone();
        let arc_alloc_sig = {
            let mut s = Signature::new(call_conv);
            s.params.push(AbiParam::new(types::I64));
            s.returns.push(AbiParam::new(types::I64));
            s
        };

        let mut func_ids = HashMap::new();

        let arc_retain_id = self.module.declare_function(
            "kryos_arc_retain",
            Linkage::Import,
            &arc_retain_sig,
        )?;
        let arc_release_id = self.module.declare_function(
            "kryos_arc_release",
            Linkage::Import,
            &arc_release_sig,
        )?;
        let arc_alloc_id = self.module.declare_function(
            "kryos_arc_alloc",
            Linkage::Import,
            &arc_alloc_sig,
        )?;

        func_ids.insert("kryos_arc_retain".to_string(), arc_retain_id);
        func_ids.insert("kryos_arc_release".to_string(), arc_release_id);
        func_ids.insert("kryos_arc_alloc".to_string(), arc_alloc_id);

        // Declare the function itself.
        let func_id = self.module.declare_function(
            &mir_func.name,
            Linkage::Export,
            &sig,
        )?;
        func_ids.insert(mir_func.name.clone(), func_id);

        // Build the Cranelift IR.
        let func_index = self.func_counter;
        self.func_counter += 1;

        let mut cl_func = Function::with_name_signature(
            UserFuncName::user(0, func_index),
            sig,
        );

        {
            let empty_struct_defs = std::collections::HashMap::new();
            let empty_enum_defs = std::collections::HashMap::new();
            let mut str_counter = 0u32;
            let mut builder = FunctionBuilder::new(&mut cl_func, &mut self.fb_ctx);
            crate::codegen::translate_function(
                mir_func,
                &mut builder,
                &func_ids,
                &mut self.module,
                &empty_struct_defs,
                &empty_enum_defs,
                &mut str_counter,
            )?;
            builder.seal_all_blocks();
            builder.finalize();
        }

        let mut ctx = Context::for_function(cl_func);
        self.module
            .define_function(func_id, &mut ctx)
            .map_err(CodegenError::Module)?;

        // Finalize: patch relocations and make code executable.
        self.module.finalize_definitions()
            .map_err(|e| CodegenError::Internal(e.to_string()))?;

        let code_ptr = self.module.get_finalized_function(func_id);
        Ok(code_ptr)
    }
}

// ---------------------------------------------------------------------------
// ARC runtime stubs — ARC is not active in JIT mode, so these remain no-ops.
// All other runtime functions use real kryos-rt implementations.
// ---------------------------------------------------------------------------

extern "C" fn kryos_arc_retain_stub(_ptr: u64) {}

extern "C" fn kryos_arc_release_stub(_ptr: u64) {}

extern "C" fn kryos_arc_alloc_stub(_val: u64) -> u64 {
    _val
}
