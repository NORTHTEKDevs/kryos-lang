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

        // Register ARC runtime stubs so JIT linking doesn't fail when these
        // symbols are referenced but no runtime is linked.
        jit_builder.symbol("kryos_arc_retain", kryos_arc_retain_stub as *const u8);
        jit_builder.symbol("kryos_arc_release", kryos_arc_release_stub as *const u8);
        jit_builder.symbol("kryos_arc_alloc", kryos_arc_alloc_stub as *const u8);

        // Register string/array runtime functions for JIT linking.
        // These are the actual kryos-rt implementations when available,
        // or stubs when running without the runtime.
        jit_builder.symbol("kryos_string_new", kryos_string_new_stub as *const u8);
        jit_builder.symbol("kryos_string_concat", kryos_string_concat_stub as *const u8);
        jit_builder.symbol("kryos_string_len", kryos_string_len_stub as *const u8);
        jit_builder.symbol("kryos_string_eq", kryos_string_eq_stub as *const u8);
        jit_builder.symbol("kryos_string_slice", kryos_string_slice_stub as *const u8);
        jit_builder.symbol("kryos_string_find", kryos_string_find_stub as *const u8);
        jit_builder.symbol("kryos_string_free", kryos_string_free_stub as *const u8);
        jit_builder.symbol("kryos_array_new", kryos_array_new_stub as *const u8);
        jit_builder.symbol("kryos_array_push", kryos_array_push_stub as *const u8);
        jit_builder.symbol("kryos_array_get", kryos_array_get_stub as *const u8);
        jit_builder.symbol("kryos_array_set", kryos_array_set_stub as *const u8);
        jit_builder.symbol("kryos_array_len", kryos_array_len_stub as *const u8);
        jit_builder.symbol("kryos_array_free", kryos_array_free_stub as *const u8);
        jit_builder.symbol("kryos_builtin_len", kryos_builtin_len_stub as *const u8);
        jit_builder.symbol("kryos_builtin_to_string", kryos_builtin_to_string_stub as *const u8);
        jit_builder.symbol("kryos_ipow", kryos_ipow_stub as *const u8);

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
            let mut builder = FunctionBuilder::new(&mut cl_func, &mut self.fb_ctx);
            crate::codegen::translate_function(
                mir_func,
                &mut builder,
                &func_ids,
                &mut self.module,
                &empty_struct_defs,
                &empty_enum_defs,
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
// ARC runtime stubs (no-ops for JIT without a linked runtime)
// ---------------------------------------------------------------------------

extern "C" fn kryos_arc_retain_stub(_ptr: u64) {
    // No-op: ARC not active in JIT stub mode.
}

extern "C" fn kryos_arc_release_stub(_ptr: u64) {
    // No-op: ARC not active in JIT stub mode.
}

extern "C" fn kryos_arc_alloc_stub(_val: u64) -> u64 {
    // Returns the value itself — no real allocation.
    _val
}

// ---------------------------------------------------------------------------
// String/Array runtime stubs (no-ops for JIT without a linked runtime)
// ---------------------------------------------------------------------------

extern "C" fn kryos_string_new_stub(_ptr: u64, _len: u64) -> u64 { 0 }
extern "C" fn kryos_string_concat_stub(_a: u64, _b: u64) -> u64 { 0 }
extern "C" fn kryos_string_len_stub(_s: u64) -> u64 { 0 }
extern "C" fn kryos_string_eq_stub(_a: u64, _b: u64) -> u8 { 0 }
extern "C" fn kryos_string_slice_stub(_s: u64, _start: u64, _end: u64) -> u64 { 0 }
extern "C" fn kryos_string_find_stub(_s: u64, _needle: u64) -> u64 { u64::MAX } // -1 as unsigned
extern "C" fn kryos_string_free_stub(_s: u64) {}
extern "C" fn kryos_array_new_stub(_elem_size: u64, _cap: u64) -> u64 { 0 }
extern "C" fn kryos_array_push_stub(_arr: u64, _val: u64) {}
extern "C" fn kryos_array_get_stub(_arr: u64, _idx: u64) -> u64 { 0 }
extern "C" fn kryos_array_set_stub(_arr: u64, _idx: u64, _val: u64) {}
extern "C" fn kryos_array_len_stub(_arr: u64) -> u64 { 0 }
extern "C" fn kryos_array_free_stub(_arr: u64) {}
extern "C" fn kryos_builtin_len_stub(collection: u64) -> u64 {
    if collection == 0 { return 0; }
    unsafe { *(collection as *const u64) }
}
extern "C" fn kryos_builtin_to_string_stub(val: u64) -> u64 { val } // passthrough in JIT
extern "C" fn kryos_ipow_stub(mut base: u64, exp: u64) -> u64 {
    let mut result: u64 = 1;
    let mut e = exp;
    while e > 0 {
        if e & 1 == 1 { result = result.wrapping_mul(base); }
        base = base.wrapping_mul(base);
        e >>= 1;
    }
    result
}
