//! JIT compilation: MIR -> Cranelift IR -> executable function pointer.
//!
//! Used for `kryos run` (JIT the whole program) and REPL (JIT individual
//! expressions). The JIT module allocates executable memory and returns
//! raw function pointers that can be called directly.

use std::collections::HashMap;

use cranelift_codegen::ir::{types, AbiParam, Function, InstBuilder, MemFlags, Signature, UserFuncName};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_frontend::FunctionBuilder;
use cranelift_frontend::FunctionBuilderContext;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{DataId, Linkage, Module};

use kryos_mir::ir::{Instruction, MirFunction, MirModule, RValue};

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
///
/// All functions are declared first so cross-function calls resolve correctly.
pub fn jit_compile_module(module: &MirModule) -> Result<HashMap<String, *const u8>, CodegenError> {
    let mut jit = JitCompiler::new()?;
    jit.compile_all_with_module(module)
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
        // Enable unwind info so panics can propagate through JIT-compiled
        // frames (required for @test assertion failure reporting).
        flag_builder
            .set("unwind_info", "true")
            .map_err(|e| CodegenError::Target(e.to_string()))?;

        let isa_builder =
            cranelift_native::builder().map_err(|e| CodegenError::Target(e.to_string()))?;

        let mut jit_builder = JITBuilder::with_isa(
            isa_builder
                .finish(settings::Flags::new(flag_builder))
                .map_err(|e| CodegenError::Target(e.to_string()))?,
            cranelift_module::default_libcall_names(),
        );

        // Register real ARC runtime functions (i64 wrappers from kryos-rt).
        jit_builder.symbol(
            "kryos_arc_retain_i64",
            kryos_rt::builtins::kryos_arc_retain_i64 as *const u8,
        );
        jit_builder.symbol(
            "kryos_arc_release_i64",
            kryos_rt::builtins::kryos_arc_release_i64 as *const u8,
        );
        jit_builder.symbol(
            "kryos_arc_alloc_i64",
            kryos_rt::builtins::kryos_arc_alloc_i64 as *const u8,
        );

        // Register real kryos-rt function implementations. Since the JIT
        // runs in-process, we pass the actual runtime function addresses
        // instead of stubs. This gives `kryos run` full runtime support.

        // String operations
        jit_builder.symbol(
            "kryos_string_new",
            kryos_rt::string::kryos_string_new as *const u8,
        );
        jit_builder.symbol(
            "kryos_string_concat",
            kryos_rt::string::kryos_string_concat as *const u8,
        );
        jit_builder.symbol(
            "kryos_string_len",
            kryos_rt::string::kryos_string_len as *const u8,
        );
        jit_builder.symbol(
            "kryos_string_eq",
            kryos_rt::string::kryos_string_eq as *const u8,
        );
        jit_builder.symbol(
            "kryos_string_compare",
            kryos_rt::string::kryos_string_compare as *const u8,
        );
        jit_builder.symbol(
            "kryos_string_slice",
            kryos_rt::string::kryos_string_slice as *const u8,
        );
        jit_builder.symbol(
            "kryos_string_find",
            kryos_rt::string::kryos_string_find as *const u8,
        );
        jit_builder.symbol(
            "kryos_string_free",
            kryos_rt::string::kryos_string_free as *const u8,
        );
        jit_builder.symbol(
            "kryos_string_release_if_ne",
            kryos_rt::string::kryos_string_release_if_ne as *const u8,
        );
        jit_builder.symbol(
            "kryos_array_release_if_ne",
            kryos_rt::array::kryos_array_release_if_ne as *const u8,
        );
        jit_builder.symbol(
            "kryos_map_release_if_ne",
            kryos_rt::map::kryos_map_release_if_ne as *const u8,
        );
        jit_builder.symbol(
            "kryos_string_clone",
            kryos_rt::string::kryos_string_clone as *const u8,
        );
        jit_builder.symbol(
            "kryos_string_retain",
            kryos_rt::string::kryos_string_retain as *const u8,
        );
        jit_builder.symbol(
            "kryos_string_retain_opt",
            kryos_rt::string::kryos_string_retain_opt as *const u8,
        );
        jit_builder.symbol(
            "kryos_array_retain_opt",
            kryos_rt::array::kryos_array_retain_opt as *const u8,
        );
        jit_builder.symbol(
            "kryos_array_dup",
            kryos_rt::array::kryos_array_dup as *const u8,
        );
        jit_builder.symbol(
            "kryos_map_retain_opt",
            kryos_rt::map::kryos_map_retain_opt as *const u8,
        );
        jit_builder.symbol(
            "kryos_diag_site",
            kryos_rt::kryos_diag_site as *const u8,
        );

        // Array operations
        jit_builder.symbol(
            "kryos_array_new",
            kryos_rt::array::kryos_array_new as *const u8,
        );
        jit_builder.symbol(
            "kryos_array_push",
            kryos_rt::array::kryos_array_push as *const u8,
        );
        jit_builder.symbol(
            "kryos_array_get",
            kryos_rt::array::kryos_array_get as *const u8,
        );
        jit_builder.symbol(
            "kryos_array_set",
            kryos_rt::array::kryos_array_set as *const u8,
        );
        jit_builder.symbol(
            "kryos_array_len",
            kryos_rt::array::kryos_array_len as *const u8,
        );
        jit_builder.symbol(
            "kryos_array_free",
            kryos_rt::array::kryos_array_free as *const u8,
        );
        jit_builder.symbol(
            "kryos_array_concat",
            kryos_rt::array::kryos_array_concat as *const u8,
        );
        jit_builder.symbol(
            "kryos_array_clone",
            kryos_rt::array::kryos_array_clone as *const u8,
        );
        jit_builder.symbol(
            "kryos_array_retain",
            kryos_rt::array::kryos_array_retain as *const u8,
        );

        // Map operations
        jit_builder.symbol("kryos_map_new", kryos_rt::map::kryos_map_new as *const u8);
        jit_builder.symbol(
            "kryos_map_insert",
            kryos_rt::map::kryos_map_insert as *const u8,
        );
        jit_builder.symbol(
            "kryos_map_insert_str",
            kryos_rt::map::kryos_map_insert_str as *const u8,
        );
        jit_builder.symbol("kryos_map_get", kryos_rt::map::kryos_map_get as *const u8);
        jit_builder.symbol(
            "kryos_map_get_str",
            kryos_rt::map::kryos_map_get_str as *const u8,
        );
        jit_builder.symbol("kryos_map_len", kryos_rt::map::kryos_map_len as *const u8);
        jit_builder.symbol("kryos_map_has", kryos_rt::map::kryos_map_has as *const u8);
        jit_builder.symbol(
            "kryos_map_has_str",
            kryos_rt::map::kryos_map_has_str as *const u8,
        );
        jit_builder.symbol(
            "kryos_map_delete",
            kryos_rt::map::kryos_map_delete as *const u8,
        );
        jit_builder.symbol(
            "kryos_map_delete_str",
            kryos_rt::map::kryos_map_delete_str as *const u8,
        );
        jit_builder.symbol("kryos_map_keys", kryos_rt::map::kryos_map_keys as *const u8);
        jit_builder.symbol(
            "kryos_map_keys_str",
            kryos_rt::map::kryos_map_keys_str as *const u8,
        );
        jit_builder.symbol("kryos_map_free", kryos_rt::map::kryos_map_free as *const u8);
        jit_builder.symbol(
            "kryos_map_clone",
            kryos_rt::map::kryos_map_clone as *const u8,
        );
        jit_builder.symbol(
            "kryos_map_snapshot",
            kryos_rt::map::kryos_map_snapshot as *const u8,
        );
        jit_builder.symbol(
            "kryos_map_retain",
            kryos_rt::map::kryos_map_retain as *const u8,
        );

        // Exception handling
        jit_builder.symbol(
            "kryos_exception_check",
            kryos_rt::exception::kryos_exception_check as *const u8,
        );
        jit_builder.symbol(
            "kryos_exception_throw",
            kryos_rt::exception::kryos_exception_throw as *const u8,
        );
        jit_builder.symbol(
            "kryos_exception_take",
            kryos_rt::exception::kryos_exception_take as *const u8,
        );
        jit_builder.symbol(
            "kryos_exception_report_uncaught_if_pending",
            kryos_rt::exception::kryos_exception_report_uncaught_if_pending as *const u8,
        );
        jit_builder.symbol(
            "kryos_budget_push",
            kryos_rt::budget::kryos_budget_push as *const u8,
        );
        jit_builder.symbol(
            "kryos_budget_pop_to",
            kryos_rt::budget::kryos_budget_pop_to as *const u8,
        );
        jit_builder.symbol(
            "kryos_budget_active",
            kryos_rt::budget::kryos_budget_active as *const u8,
        );
        jit_builder.symbol(
            "kryos_budget_try_call",
            kryos_rt::budget::kryos_budget_try_call as *const u8,
        );
        jit_builder.symbol(
            "kryos_budget_charge_tokens",
            kryos_rt::budget::kryos_budget_charge_tokens as *const u8,
        );
        jit_builder.symbol(
            "kryos_budget_remaining_tokens",
            kryos_rt::budget::kryos_budget_remaining_tokens as *const u8,
        );
        jit_builder.symbol(
            "kryos_budget_remaining_calls",
            kryos_rt::budget::kryos_budget_remaining_calls as *const u8,
        );
        jit_builder.symbol(
            "kryos_budget_push_usd",
            kryos_rt::budget::kryos_budget_push_usd as *const u8,
        );
        jit_builder.symbol(
            "kryos_budget_charge_usd_micros",
            kryos_rt::budget::kryos_budget_charge_usd_micros as *const u8,
        );
        jit_builder.symbol(
            "kryos_budget_remaining_usd_micros",
            kryos_rt::budget::kryos_budget_remaining_usd_micros as *const u8,
        );

        // ARC drop function registration
        jit_builder.symbol(
            "kryos_arc_set_drop_i64",
            kryos_rt::builtins::kryos_arc_set_drop_i64 as *const u8,
        );

        // Panic handler and runtime checks
        jit_builder.symbol("kryos_panic", kryos_rt::panic::kryos_panic as *const u8);
        jit_builder.symbol(
            "kryos_panic_with_location",
            kryos_rt::panic::kryos_panic_with_location as *const u8,
        );
        jit_builder.symbol(
            "kryos_check_div_zero_i64",
            kryos_rt::builtins::kryos_check_div_zero_i64 as *const u8,
        );
        jit_builder.symbol(
            "kryos_check_sdiv_i64",
            kryos_rt::builtins::kryos_check_sdiv_i64 as *const u8,
        );

        // Builtins and type conversions
        jit_builder.symbol(
            "kryos_builtin_len",
            kryos_rt::builtins::kryos_builtin_len as *const u8,
        );
        jit_builder.symbol(
            "kryos_builtin_to_string",
            kryos_rt::builtins::kryos_builtin_to_string as *const u8,
        );
        jit_builder.symbol(
            "kryos_i64_to_string",
            kryos_rt::builtins::kryos_i64_to_string as *const u8,
        );
        jit_builder.symbol(
            "kryos_u64_to_string",
            kryos_rt::builtins::kryos_u64_to_string as *const u8,
        );
        jit_builder.symbol(
            "kryos_f64_to_string",
            kryos_rt::builtins::kryos_f64_to_string as *const u8,
        );
        jit_builder.symbol(
            "kryos_bool_to_string",
            kryos_rt::builtins::kryos_bool_to_string as *const u8,
        );
        jit_builder.symbol("kryos_ipow", kryos_rt::builtins::kryos_ipow as *const u8);
        jit_builder.symbol("kryos_fpow", kryos_rt::builtins::kryos_fpow as *const u8);
        jit_builder.symbol("kryos_fmod", kryos_rt::builtins::kryos_fmod as *const u8);

        // Ergonomic builtins (file I/O, env, assertions, parsing, introspection)
        jit_builder.symbol(
            "kryos_builtin_file_read",
            kryos_rt::builtins::kryos_builtin_file_read as *const u8,
        );
        jit_builder.symbol(
            "kryos_builtin_file_write",
            kryos_rt::builtins::kryos_builtin_file_write as *const u8,
        );
        jit_builder.symbol(
            "kryos_builtin_env_get",
            kryos_rt::builtins::kryos_builtin_env_get as *const u8,
        );
        jit_builder.symbol(
            "kryos_builtin_time_now",
            kryos_rt::builtins::kryos_builtin_time_now as *const u8,
        );
        jit_builder.symbol(
            "kryos_builtin_assert",
            kryos_rt::builtins::kryos_builtin_assert as *const u8,
        );
        jit_builder.symbol(
            "kryos_builtin_assert_eq",
            kryos_rt::builtins::kryos_builtin_assert_eq as *const u8,
        );
        jit_builder.symbol(
            "kryos_builtin_panic",
            kryos_rt::builtins::kryos_builtin_panic as *const u8,
        );
        jit_builder.symbol(
            "kryos_builtin_parse_int",
            kryos_rt::builtins::kryos_builtin_parse_int as *const u8,
        );
        jit_builder.symbol(
            "kryos_builtin_parse_float",
            kryos_rt::builtins::kryos_builtin_parse_float as *const u8,
        );
        jit_builder.symbol(
            "kryos_builtin_type_of",
            kryos_rt::builtins::kryos_builtin_type_of as *const u8,
        );

        // Print operations
        jit_builder.symbol(
            "kryos_println_str",
            kryos_rt::builtins::kryos_println_str as *const u8,
        );
        jit_builder.symbol(
            "kryos_print_str",
            kryos_rt::builtins::kryos_print_str as *const u8,
        );
        jit_builder.symbol(
            "kryos_eprintln_str",
            kryos_rt::builtins::kryos_eprintln_str as *const u8,
        );

        // Channel operations
        jit_builder.symbol(
            "kryos_chan_new_i64",
            kryos_rt::builtins::kryos_chan_new_i64 as *const u8,
        );
        jit_builder.symbol(
            "kryos_chan_send_i64",
            kryos_rt::builtins::kryos_chan_send_i64 as *const u8,
        );
        jit_builder.symbol(
            "kryos_chan_recv_i64",
            kryos_rt::builtins::kryos_chan_recv_i64 as *const u8,
        );
        jit_builder.symbol(
            "kryos_chan_try_recv_status_i64",
            kryos_rt::builtins::kryos_chan_try_recv_status_i64 as *const u8,
        );
        jit_builder.symbol(
            "kryos_chan_last_recv_i64",
            kryos_rt::builtins::kryos_chan_last_recv_i64 as *const u8,
        );
        jit_builder.symbol(
            "kryos_chan_try_recv_status_i64",
            kryos_rt::builtins::kryos_chan_try_recv_status_i64 as *const u8,
        );
        jit_builder.symbol(
            "kryos_chan_last_recv_i64",
            kryos_rt::builtins::kryos_chan_last_recv_i64 as *const u8,
        );
        jit_builder.symbol(
            "kryos_chan_is_closed_i64",
            kryos_rt::builtins::kryos_chan_is_closed_i64 as *const u8,
        );
        jit_builder.symbol(
            "kryos_chan_close_i64",
            kryos_rt::builtins::kryos_chan_close_i64 as *const u8,
        );

        // Actor runtime
        jit_builder.symbol(
            "kryos_actor_spawn_i64",
            kryos_rt::builtins::kryos_actor_spawn_i64 as *const u8,
        );
        jit_builder.symbol(
            "kryos_actor_send_i64",
            kryos_rt::builtins::kryos_actor_send_i64 as *const u8,
        );
        jit_builder.symbol(
            "kryos_actor_recv_i64",
            kryos_rt::builtins::kryos_actor_recv_i64 as *const u8,
        );
        jit_builder.symbol(
            "kryos_actor_lock_i64",
            kryos_rt::builtins::kryos_actor_lock_i64 as *const u8,
        );
        jit_builder.symbol(
            "kryos_actor_unlock_i64",
            kryos_rt::builtins::kryos_actor_unlock_i64 as *const u8,
        );

        // Spawn runtime
        jit_builder.symbol("kryos_spawn", kryos_rt::spawn::kryos_spawn as *const u8);
        jit_builder.symbol(
            "kryos_spawn_wait_all",
            kryos_rt::spawn::kryos_spawn_wait_all as *const u8,
        );
        jit_builder.symbol(
            "kryos_actor_wait_all",
            kryos_rt::actor::kryos_actor_wait_all as *const u8,
        );
        jit_builder.symbol("kryos_sleep", kryos_rt::spawn::kryos_sleep as *const u8);
        jit_builder.symbol(
            "kryos_sleep_ms",
            kryos_rt::spawn::kryos_sleep_ms as *const u8,
        );

        // Cooperative async executor (real interleaving via await/yield).
        jit_builder.symbol(
            "kryos_coop_spawn",
            kryos_rt::executor::kryos_coop_spawn as *const u8,
        );
        jit_builder.symbol(
            "kryos_coop_yield",
            kryos_rt::executor::kryos_coop_yield as *const u8,
        );
        jit_builder.symbol(
            "kryos_coop_run",
            kryos_rt::executor::kryos_coop_run as *const u8,
        );
        jit_builder.symbol(
            "kryos_coop_record",
            kryos_rt::executor::kryos_coop_record as *const u8,
        );
        jit_builder.symbol(
            "kryos_coop_order",
            kryos_rt::executor::kryos_coop_order as *const u8,
        );
        jit_builder.symbol(
            "kryos_coop_reset",
            kryos_rt::executor::kryos_coop_reset as *const u8,
        );

        // Tensor runtime
        jit_builder.symbol(
            "kryos_tensor_zeros",
            kryos_rt::tensor::kryos_tensor_zeros as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_ones",
            kryos_rt::tensor::kryos_tensor_ones as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_rand",
            kryos_rt::tensor::kryos_tensor_rand as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_randn",
            kryos_rt::tensor::kryos_tensor_randn as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_from_data",
            kryos_rt::tensor::kryos_tensor_from_data as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_eye",
            kryos_rt::tensor::kryos_tensor_eye as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_arange",
            kryos_rt::tensor::kryos_tensor_arange as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_ndim",
            kryos_rt::tensor::kryos_tensor_ndim as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_numel",
            kryos_rt::tensor::kryos_tensor_numel as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_shape_dim",
            kryos_rt::tensor::kryos_tensor_shape_dim as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_get",
            kryos_rt::tensor::kryos_tensor_get as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_set",
            kryos_rt::tensor::kryos_tensor_set as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_add",
            kryos_rt::tensor::kryos_tensor_add as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_sub",
            kryos_rt::tensor::kryos_tensor_sub as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_mul",
            kryos_rt::tensor::kryos_tensor_mul as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_div",
            kryos_rt::tensor::kryos_tensor_div as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_pow",
            kryos_rt::tensor::kryos_tensor_pow as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_scale",
            kryos_rt::tensor::kryos_tensor_scale as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_exp",
            kryos_rt::tensor::kryos_tensor_exp as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_log",
            kryos_rt::tensor::kryos_tensor_log as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_sqrt",
            kryos_rt::tensor::kryos_tensor_sqrt as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_tanh",
            kryos_rt::tensor::kryos_tensor_tanh as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_sigmoid",
            kryos_rt::tensor::kryos_tensor_sigmoid as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_relu",
            kryos_rt::tensor::kryos_tensor_relu as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_neg",
            kryos_rt::tensor::kryos_tensor_neg as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_sum",
            kryos_rt::tensor::kryos_tensor_sum as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_mean",
            kryos_rt::tensor::kryos_tensor_mean as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_max",
            kryos_rt::tensor::kryos_tensor_max as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_min",
            kryos_rt::tensor::kryos_tensor_min as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_argmax",
            kryos_rt::tensor::kryos_tensor_argmax as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_argmin",
            kryos_rt::tensor::kryos_tensor_argmin as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_matmul",
            kryos_rt::tensor::kryos_tensor_matmul as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_transpose",
            kryos_rt::tensor::kryos_tensor_transpose as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_reshape",
            kryos_rt::tensor::kryos_tensor_reshape as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_flatten",
            kryos_rt::tensor::kryos_tensor_flatten as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_softmax",
            kryos_rt::tensor::kryos_tensor_softmax as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_cross_entropy",
            kryos_rt::tensor::kryos_tensor_cross_entropy as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_mse_loss",
            kryos_rt::tensor::kryos_tensor_mse_loss as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_to_string",
            kryos_rt::tensor::kryos_tensor_to_string as *const u8,
        );
        jit_builder.symbol(
            "kryos_tensor_free",
            kryos_rt::tensor::kryos_tensor_free as *const u8,
        );

        // Stdlib-native: process management
        jit_builder.symbol(
            "kryos_env_get",
            kryos_stdlib_native::process::kryos_env_get as *const u8,
        );
        jit_builder.symbol(
            "kryos_process_exit",
            kryos_stdlib_native::process::kryos_process_exit as *const u8,
        );
        jit_builder.symbol(
            "kryos_process_exec",
            kryos_stdlib_native::process::kryos_process_exec as *const u8,
        );
        jit_builder.symbol(
            "kryos_process_exec_simple",
            kryos_stdlib_native::process::kryos_process_exec_simple as *const u8,
        );
        jit_builder.symbol(
            "kryos_process_argc",
            kryos_stdlib_native::process::kryos_process_argc as *const u8,
        );
        jit_builder.symbol(
            "kryos_process_argv",
            kryos_stdlib_native::process::kryos_process_argv as *const u8,
        );

        // Stdlib-native: filesystem
        jit_builder.symbol(
            "kryos_path_exists",
            kryos_stdlib_native::fs::kryos_path_exists as *const u8,
        );
        jit_builder.symbol(
            "kryos_dir_create",
            kryos_stdlib_native::fs::kryos_dir_create as *const u8,
        );
        jit_builder.symbol(
            "kryos_file_remove",
            kryos_stdlib_native::fs::kryos_file_remove as *const u8,
        );
        jit_builder.symbol(
            "kryos_dir_list",
            kryos_stdlib_native::process::kryos_dir_list as *const u8,
        );
        jit_builder.symbol(
            "kryos_dir_walk",
            kryos_stdlib_native::process::kryos_dir_walk as *const u8,
        );

        // Stdlib-native: file I/O
        jit_builder.symbol(
            "kryos_file_open",
            kryos_stdlib_native::io::kryos_file_open as *const u8,
        );
        jit_builder.symbol(
            "kryos_file_read",
            kryos_stdlib_native::io::kryos_file_read as *const u8,
        );
        jit_builder.symbol(
            "kryos_file_write",
            kryos_stdlib_native::io::kryos_file_write as *const u8,
        );
        jit_builder.symbol(
            "kryos_file_close",
            kryos_stdlib_native::io::kryos_file_close as *const u8,
        );
        jit_builder.symbol(
            "kryos_stderr_write",
            kryos_stdlib_native::io::kryos_stderr_write as *const u8,
        );
        jit_builder.symbol(
            "kryos_stdout_write",
            kryos_stdlib_native::io::kryos_stdout_write as *const u8,
        );
        jit_builder.symbol(
            "kryos_stdin_read",
            kryos_stdlib_native::io::kryos_stdin_read as *const u8,
        );

        // Stdlib-native: networking
        jit_builder.symbol(
            "kryos_tcp_connect",
            kryos_stdlib_native::net::kryos_tcp_connect as *const u8,
        );
        jit_builder.symbol(
            "kryos_tcp_bind",
            kryos_stdlib_native::net::kryos_tcp_bind as *const u8,
        );
        jit_builder.symbol(
            "kryos_tcp_accept",
            kryos_stdlib_native::net::kryos_tcp_accept as *const u8,
        );
        jit_builder.symbol(
            "kryos_tcp_send",
            kryos_stdlib_native::net::kryos_tcp_send as *const u8,
        );
        jit_builder.symbol(
            "kryos_tcp_recv",
            kryos_stdlib_native::net::kryos_tcp_recv as *const u8,
        );
        jit_builder.symbol(
            "kryos_socket_close",
            kryos_stdlib_native::net::kryos_socket_close as *const u8,
        );
        jit_builder.symbol(
            "kryos_tcp_connect_ks",
            kryos_stdlib_native::net::kryos_tcp_connect_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_tcp_bind_ks",
            kryos_stdlib_native::net::kryos_tcp_bind_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_tcp_send_ks",
            kryos_stdlib_native::net::kryos_tcp_send_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_tcp_recv_ks",
            kryos_stdlib_native::net::kryos_tcp_recv_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_socket_close_ks",
            kryos_stdlib_native::net::kryos_socket_close_ks as *const u8,
        );
        // Async / non-blocking primitives
        jit_builder.symbol(
            "kryos_tcp_set_nonblocking",
            kryos_stdlib_native::net::kryos_tcp_set_nonblocking as *const u8,
        );
        jit_builder.symbol(
            "kryos_tcp_try_accept",
            kryos_stdlib_native::net::kryos_tcp_try_accept as *const u8,
        );
        jit_builder.symbol(
            "kryos_tcp_try_recv_ks",
            kryos_stdlib_native::net::kryos_tcp_try_recv_ks as *const u8,
        );
        // kryos_sleep_ms is provided by kryos-rt (see above).

        // TLS server (Gap A)
        jit_builder.symbol(
            "kryos_tls_server_config_ks",
            kryos_stdlib_native::tls::kryos_tls_server_config_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_tls_accept",
            kryos_stdlib_native::tls::kryos_tls_accept as *const u8,
        );
        jit_builder.symbol(
            "kryos_tls_send_ks",
            kryos_stdlib_native::tls::kryos_tls_send_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_tls_recv_ks",
            kryos_stdlib_native::tls::kryos_tls_recv_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_tls_close_ks",
            kryos_stdlib_native::tls::kryos_tls_close_ks as *const u8,
        );

        // Unix domain sockets (v2.0)
        jit_builder.symbol(
            "kryos_uds_connect_ks",
            kryos_stdlib_native::unix_socket::kryos_uds_connect_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_uds_bind_ks",
            kryos_stdlib_native::unix_socket::kryos_uds_bind_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_uds_accept",
            kryos_stdlib_native::unix_socket::kryos_uds_accept as *const u8,
        );
        jit_builder.symbol(
            "kryos_uds_send_ks",
            kryos_stdlib_native::unix_socket::kryos_uds_send_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_uds_recv_ks",
            kryos_stdlib_native::unix_socket::kryos_uds_recv_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_uds_close",
            kryos_stdlib_native::unix_socket::kryos_uds_close as *const u8,
        );

        // WebSocket helpers (RFC 6455) (v2.0)
        jit_builder.symbol(
            "kryos_ws_accept_key_ks",
            kryos_stdlib_native::websocket::kryos_ws_accept_key_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_ws_encode_text_ks",
            kryos_stdlib_native::websocket::kryos_ws_encode_text_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_ws_encode_binary_ks",
            kryos_stdlib_native::websocket::kryos_ws_encode_binary_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_ws_encode_close",
            kryos_stdlib_native::websocket::kryos_ws_encode_close as *const u8,
        );
        jit_builder.symbol(
            "kryos_ws_encode_ping_ks",
            kryos_stdlib_native::websocket::kryos_ws_encode_ping_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_ws_encode_pong_ks",
            kryos_stdlib_native::websocket::kryos_ws_encode_pong_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_ws_unmask_ks",
            kryos_stdlib_native::websocket::kryos_ws_unmask_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_ws_read_frame_ks",
            kryos_stdlib_native::websocket::kryos_ws_read_frame_ks as *const u8,
        );

        // PostgreSQL (Gap B)
        jit_builder.symbol(
            "kryos_pg_connect_ks",
            kryos_stdlib_native::postgres::kryos_pg_connect_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_pg_exec_ks",
            kryos_stdlib_native::postgres::kryos_pg_exec_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_pg_query_ks",
            kryos_stdlib_native::postgres::kryos_pg_query_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_pg_close_ks",
            kryos_stdlib_native::postgres::kryos_pg_close_ks as *const u8,
        );

        // Stdlib-native: JSON (all handles are i64)
        jit_builder.symbol(
            "kryos_json_parse",
            kryos_stdlib_native::json::kryos_json_parse as *const u8,
        );
        jit_builder.symbol(
            "kryos_json_stringify",
            kryos_stdlib_native::json::kryos_json_stringify as *const u8,
        );
        jit_builder.symbol(
            "kryos_json_object",
            kryos_stdlib_native::json::kryos_json_object as *const u8,
        );
        jit_builder.symbol(
            "kryos_json_array",
            kryos_stdlib_native::json::kryos_json_array as *const u8,
        );
        jit_builder.symbol(
            "kryos_json_string",
            kryos_stdlib_native::json::kryos_json_string as *const u8,
        );
        jit_builder.symbol(
            "kryos_json_number",
            kryos_stdlib_native::json::kryos_json_number as *const u8,
        );
        jit_builder.symbol(
            "kryos_json_bool",
            kryos_stdlib_native::json::kryos_json_bool as *const u8,
        );
        jit_builder.symbol(
            "kryos_json_null",
            kryos_stdlib_native::json::kryos_json_null as *const u8,
        );
        jit_builder.symbol(
            "kryos_json_get",
            kryos_stdlib_native::json::kryos_json_get as *const u8,
        );
        jit_builder.symbol(
            "kryos_json_get_index",
            kryos_stdlib_native::json::kryos_json_get_index as *const u8,
        );
        jit_builder.symbol(
            "kryos_json_to_str",
            kryos_stdlib_native::json::kryos_json_to_str as *const u8,
        );
        jit_builder.symbol(
            "kryos_json_to_int",
            kryos_stdlib_native::json::kryos_json_to_int as *const u8,
        );
        jit_builder.symbol(
            "kryos_json_to_float",
            kryos_stdlib_native::json::kryos_json_to_float as *const u8,
        );
        jit_builder.symbol(
            "kryos_json_is_null",
            kryos_stdlib_native::json::kryos_json_is_null as *const u8,
        );
        jit_builder.symbol(
            "kryos_json_length",
            kryos_stdlib_native::json::kryos_json_length as *const u8,
        );
        jit_builder.symbol(
            "kryos_json_type",
            kryos_stdlib_native::json::kryos_json_type as *const u8,
        );

        // Stdlib-native: bindings (Kryos-handle wrappers)
        jit_builder.symbol(
            "kryos_sha256_ks",
            kryos_stdlib_native::bindings::kryos_sha256_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_sha512_ks",
            kryos_stdlib_native::bindings::kryos_sha512_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_hmac_sha256_ks",
            kryos_stdlib_native::bindings::kryos_hmac_sha256_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_ed25519_generate_ks",
            kryos_stdlib_native::bindings::kryos_ed25519_generate_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_ed25519_public_ks",
            kryos_stdlib_native::bindings::kryos_ed25519_public_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_ed25519_sign_ks",
            kryos_stdlib_native::bindings::kryos_ed25519_sign_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_ed25519_verify_ks",
            kryos_stdlib_native::bindings::kryos_ed25519_verify_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_pbkdf2_sha256_ks",
            kryos_stdlib_native::bindings::kryos_pbkdf2_sha256_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_hex_to_b64url_ks",
            kryos_stdlib_native::bindings::kryos_hex_to_b64url_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_b64url_to_hex_ks",
            kryos_stdlib_native::bindings::kryos_b64url_to_hex_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_random_bytes_ks",
            kryos_stdlib_native::bindings::kryos_random_bytes_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_sha1_hex_ks",
            kryos_stdlib_native::bindings::kryos_sha1_hex_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_sha1_base64_ks",
            kryos_stdlib_native::bindings::kryos_sha1_base64_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_base64_encode_ks",
            kryos_stdlib_native::bindings::kryos_base64_encode_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_base64_decode_ks",
            kryos_stdlib_native::bindings::kryos_base64_decode_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_chr_ks",
            kryos_stdlib_native::bindings::kryos_chr_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_byte_at_ks",
            kryos_stdlib_native::bindings::kryos_byte_at_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_regex_new_ks",
            kryos_stdlib_native::bindings::kryos_regex_new_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_regex_is_match_ks",
            kryos_stdlib_native::bindings::kryos_regex_is_match_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_regex_find_ks",
            kryos_stdlib_native::bindings::kryos_regex_find_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_regex_find_pos_ks",
            kryos_stdlib_native::bindings::kryos_regex_find_pos_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_regex_find_end_ks",
            kryos_stdlib_native::bindings::kryos_regex_find_end_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_regex_replace_all_ks",
            kryos_stdlib_native::bindings::kryos_regex_replace_all_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_regex_drop_ks",
            kryos_stdlib_native::bindings::kryos_regex_drop_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_http_request_ks",
            kryos_stdlib_native::bindings::kryos_http_request_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_https_get_ks",
            kryos_stdlib_native::bindings::kryos_https_get_ks as *const u8,
        );
        // HTTP/2 client (Gap C)
        jit_builder.symbol(
            "kryos_http2_get_ks",
            kryos_stdlib_native::bindings::kryos_http2_get_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_http2_post_ks",
            kryos_stdlib_native::bindings::kryos_http2_post_ks as *const u8,
        );
        jit_builder.symbol(
            "kryos_http2_request_ks",
            kryos_stdlib_native::bindings::kryos_http2_request_ks as *const u8,
        );
        // WASM v0.4 web builtins (native fallbacks).
        jit_builder.symbol("kryos_dom_set_text_ks",
            kryos_stdlib_native::bindings::kryos_dom_set_text_ks as *const u8);
        jit_builder.symbol("kryos_dom_get_value_ks",
            kryos_stdlib_native::bindings::kryos_dom_get_value_ks as *const u8);
        jit_builder.symbol("kryos_alert_ks",
            kryos_stdlib_native::bindings::kryos_alert_ks as *const u8);
        jit_builder.symbol("kryos_canvas_fill_rect_ks",
            kryos_stdlib_native::bindings::kryos_canvas_fill_rect_ks as *const u8);
        jit_builder.symbol("kryos_canvas_clear_ks",
            kryos_stdlib_native::bindings::kryos_canvas_clear_ks as *const u8);
        jit_builder.symbol("kryos_fetch_text_ks",
            kryos_stdlib_native::bindings::kryos_fetch_text_ks as *const u8);

        // Runtime: mutable module-level globals
        jit_builder.symbol(
            "kryos_global_get",
            kryos_rt::globals::kryos_global_get as *const u8,
        );
        jit_builder.symbol(
            "kryos_global_set",
            kryos_rt::globals::kryos_global_set as *const u8,
        );
        jit_builder.symbol(
            "kryos_global_has",
            kryos_rt::globals::kryos_global_has as *const u8,
        );

        // Stdlib-native: datetime
        jit_builder.symbol(
            "kryos_time_now_secs",
            kryos_stdlib_native::datetime::kryos_time_now_secs as *const u8,
        );
        jit_builder.symbol(
            "kryos_time_now_millis",
            kryos_stdlib_native::datetime::kryos_time_now_millis as *const u8,
        );
        jit_builder.symbol(
            "kryos_time_sleep_millis",
            kryos_stdlib_native::datetime::kryos_time_sleep_millis as *const u8,
        );

        // Runtime: f64<->i64 bit reinterpretation (stdlib value transport)
        jit_builder.symbol(
            "kryos_f64_from_bits",
            kryos_rt::floatbits::kryos_f64_from_bits as *const u8,
        );
        jit_builder.symbol(
            "kryos_f64_to_bits",
            kryos_rt::floatbits::kryos_f64_to_bits as *const u8,
        );
        // Stdlib-native: filesystem rename (atomic move)
        jit_builder.symbol(
            "kryos_fs_rename",
            kryos_stdlib_native::fs::kryos_fs_rename as *const u8,
        );

        // Stdlib-native: crypto
        jit_builder.symbol(
            "kryos_sha256",
            kryos_stdlib_native::crypto::kryos_sha256 as *const u8,
        );
        jit_builder.symbol(
            "kryos_sha512",
            kryos_stdlib_native::crypto::kryos_sha512 as *const u8,
        );
        jit_builder.symbol(
            "kryos_random_bytes",
            kryos_stdlib_native::crypto::kryos_random_bytes as *const u8,
        );

        // Stdlib-native: regex
        jit_builder.symbol(
            "kryos_regex_new",
            kryos_stdlib_native::re::kryos_regex_new as *const u8,
        );
        jit_builder.symbol(
            "kryos_regex_is_match",
            kryos_stdlib_native::re::kryos_regex_is_match as *const u8,
        );
        jit_builder.symbol(
            "kryos_regex_drop",
            kryos_stdlib_native::re::kryos_regex_drop as *const u8,
        );

        // Stdlib-native: synchronization primitives
        jit_builder.symbol(
            "kryos_mutex_new",
            kryos_stdlib_native::sync_prims::kryos_mutex_new as *const u8,
        );
        jit_builder.symbol(
            "kryos_mutex_lock",
            kryos_stdlib_native::sync_prims::kryos_mutex_lock as *const u8,
        );
        jit_builder.symbol(
            "kryos_mutex_unlock",
            kryos_stdlib_native::sync_prims::kryos_mutex_unlock as *const u8,
        );
        jit_builder.symbol(
            "kryos_mutex_drop",
            kryos_stdlib_native::sync_prims::kryos_mutex_drop as *const u8,
        );

        // FFI/pointer-helper builtins (str_to_ptr, alloc, buf_to_str, ...).
        // These were registered only on the AOT path, so the in-process JIT
        // (the `kryos test` runner -- `kryos run` is Cranelift-AOT plus a
        // subprocess, NOT this JIT) crashed with "can't resolve symbol
        // str_to_ptr" the moment a test imported any stdlib module using
        // them (std::re, std::db, std::http, ...).
        jit_builder.symbol(
            "kryos_str_to_ptr",
            kryos_rt::builtins::kryos_str_to_ptr as *const u8,
        );
        jit_builder.symbol(
            "kryos_alloc_bytes",
            kryos_rt::builtins::kryos_alloc_bytes as *const u8,
        );
        jit_builder.symbol(
            "kryos_handle_to_str",
            kryos_rt::builtins::kryos_handle_to_str as *const u8,
        );
        jit_builder.symbol(
            "kryos_buf_to_str",
            kryos_rt::builtins::kryos_buf_to_str as *const u8,
        );
        jit_builder.symbol(
            "kryos_ptr_byte_at",
            kryos_rt::builtins::kryos_ptr_byte_at as *const u8,
        );
        jit_builder.symbol(
            "kryos_ptr_read_i64",
            kryos_rt::builtins::kryos_ptr_read_i64 as *const u8,
        );
        jit_builder.symbol(
            "kryos_free_bytes",
            kryos_rt::builtins::kryos_free_bytes as *const u8,
        );
        jit_builder.symbol(
            "kryos_ptr_set_byte",
            kryos_rt::builtins::kryos_ptr_set_byte as *const u8,
        );
        jit_builder.symbol(
            "kryos_ptr_write_i64",
            kryos_rt::builtins::kryos_ptr_write_i64 as *const u8,
        );
        jit_builder.symbol(
            "kryos_array_clone_deep",
            kryos_rt::array::kryos_array_clone_deep as *const u8,
        );
        jit_builder.symbol(
            "kryos_string_char_at",
            kryos_rt::builtins::kryos_string_char_at as *const u8,
        );

        // Trace runtime (call stack tracking for panic stack traces)
        jit_builder.symbol(
            "kryos_trace_enter",
            kryos_rt::trace::kryos_trace_enter as *const u8,
        );
        jit_builder.symbol(
            "kryos_trace_exit",
            kryos_rt::trace::kryos_trace_exit as *const u8,
        );

        // Source-level debugger line hook (only referenced by debug-instrumented
        // code produced under `kryos dap`; harmless to register unconditionally).
        jit_builder.symbol(
            "kryos_dbg_line",
            kryos_rt::debug::kryos_dbg_line as *const u8,
        );

        // Stdlib-native: terminal
        jit_builder.symbol(
            "kryos_term_raw_enable",
            kryos_stdlib_native::term::kryos_term_raw_enable as *const u8,
        );
        jit_builder.symbol(
            "kryos_term_raw_disable",
            kryos_stdlib_native::term::kryos_term_raw_disable as *const u8,
        );
        jit_builder.symbol(
            "kryos_term_width",
            kryos_stdlib_native::term::kryos_term_width as *const u8,
        );
        jit_builder.symbol(
            "kryos_term_height",
            kryos_stdlib_native::term::kryos_term_height as *const u8,
        );
        jit_builder.symbol(
            "kryos_term_cursor_move",
            kryos_stdlib_native::term::kryos_term_cursor_move as *const u8,
        );
        jit_builder.symbol(
            "kryos_term_clear",
            kryos_stdlib_native::term::kryos_term_clear as *const u8,
        );

        // Byte buffer operations (for self-hosted native code emission)
        jit_builder.symbol(
            "kryos_buf_new",
            kryos_rt::builtins::kryos_buf_new as *const u8,
        );
        jit_builder.symbol(
            "kryos_buf_write_byte",
            kryos_rt::builtins::kryos_buf_write_byte as *const u8,
        );
        jit_builder.symbol(
            "kryos_buf_write_i16_le",
            kryos_rt::builtins::kryos_buf_write_i16_le as *const u8,
        );
        jit_builder.symbol(
            "kryos_buf_write_i32_le",
            kryos_rt::builtins::kryos_buf_write_i32_le as *const u8,
        );
        jit_builder.symbol(
            "kryos_buf_write_i64_le",
            kryos_rt::builtins::kryos_buf_write_i64_le as *const u8,
        );
        jit_builder.symbol(
            "kryos_buf_write_bytes",
            kryos_rt::builtins::kryos_buf_write_bytes as *const u8,
        );
        jit_builder.symbol(
            "kryos_buf_write_str",
            kryos_rt::builtins::kryos_buf_write_str as *const u8,
        );
        jit_builder.symbol(
            "kryos_buf_write_zeros",
            kryos_rt::builtins::kryos_buf_write_zeros as *const u8,
        );
        jit_builder.symbol(
            "kryos_buf_len",
            kryos_rt::builtins::kryos_buf_len as *const u8,
        );
        jit_builder.symbol(
            "kryos_buf_contents_to_str",
            kryos_rt::builtins::kryos_buf_contents_to_str as *const u8,
        );
        jit_builder.symbol(
            "kryos_buf_get_byte",
            kryos_rt::builtins::kryos_buf_get_byte as *const u8,
        );
        jit_builder.symbol(
            "kryos_buf_set_byte",
            kryos_rt::builtins::kryos_buf_set_byte as *const u8,
        );
        jit_builder.symbol(
            "kryos_buf_patch_i32_le",
            kryos_rt::builtins::kryos_buf_patch_i32_le as *const u8,
        );
        jit_builder.symbol(
            "kryos_buf_patch_i64_le",
            kryos_rt::builtins::kryos_buf_patch_i64_le as *const u8,
        );
        jit_builder.symbol(
            "kryos_buf_write_to_file",
            kryos_rt::builtins::kryos_buf_write_to_file as *const u8,
        );
        jit_builder.symbol(
            "kryos_buf_free",
            kryos_rt::builtins::kryos_buf_free as *const u8,
        );
        jit_builder.symbol(
            "kryos_builtin_exit",
            kryos_rt::builtins::kryos_builtin_exit as *const u8,
        );
        jit_builder.symbol(
            "kryos_builtin_args",
            kryos_rt::builtins::kryos_builtin_args as *const u8,
        );

        // --- Bulk registration of additional builtins (math, strings,
        //     filesystem helpers, etc) that the AOT pipeline already
        //     declares via auto-declare in `ensure_func_ref_with_args`.
        //     Without these the JIT cannot resolve the symbols at runtime.
        jit_builder.symbol("kryos_builtin_abs", kryos_rt::builtins::kryos_builtin_abs as *const u8);
        jit_builder.symbol("kryos_builtin_abs_f", kryos_rt::builtins::kryos_builtin_abs_f as *const u8);
        jit_builder.symbol("kryos_builtin_ceil", kryos_rt::builtins::kryos_builtin_ceil as *const u8);
        jit_builder.symbol("kryos_builtin_char_code", kryos_rt::builtins::kryos_builtin_char_code as *const u8);
        jit_builder.symbol("kryos_builtin_char_from", kryos_rt::builtins::kryos_builtin_char_from as *const u8);
        jit_builder.symbol("kryos_builtin_contains", kryos_rt::builtins::kryos_builtin_contains as *const u8);
        jit_builder.symbol("kryos_builtin_cos", kryos_rt::builtins::kryos_builtin_cos as *const u8);
        jit_builder.symbol("kryos_builtin_create_dir", kryos_rt::builtins::kryos_builtin_create_dir as *const u8);
        jit_builder.symbol("kryos_builtin_ends_with", kryos_rt::builtins::kryos_builtin_ends_with as *const u8);
        jit_builder.symbol("kryos_builtin_file_append", kryos_rt::builtins::kryos_builtin_file_append as *const u8);
        jit_builder.symbol("kryos_builtin_file_exists", kryos_rt::builtins::kryos_builtin_file_exists as *const u8);
        jit_builder.symbol("kryos_builtin_file_size", kryos_rt::builtins::kryos_builtin_file_size as *const u8);
        jit_builder.symbol("kryos_builtin_float", kryos_rt::builtins::kryos_builtin_float as *const u8);
        jit_builder.symbol("kryos_builtin_float_from_float", kryos_rt::builtins::kryos_builtin_float_from_float as *const u8);
        jit_builder.symbol("kryos_builtin_floor", kryos_rt::builtins::kryos_builtin_floor as *const u8);
        jit_builder.symbol("kryos_builtin_http_get", kryos_rt::builtins::kryos_builtin_http_get as *const u8);
        jit_builder.symbol("kryos_builtin_index_of", kryos_rt::builtins::kryos_builtin_index_of as *const u8);
        jit_builder.symbol("kryos_builtin_int", kryos_rt::builtins::kryos_builtin_int as *const u8);
        jit_builder.symbol("kryos_builtin_int_from_float", kryos_rt::builtins::kryos_builtin_int_from_float as *const u8);
        jit_builder.symbol("kryos_builtin_join", kryos_rt::builtins::kryos_builtin_join as *const u8);
        jit_builder.symbol("kryos_builtin_log", kryos_rt::builtins::kryos_builtin_log as *const u8);
        jit_builder.symbol("kryos_builtin_log10", kryos_rt::builtins::kryos_builtin_log10 as *const u8);
        jit_builder.symbol("kryos_builtin_log2", kryos_rt::builtins::kryos_builtin_log2 as *const u8);
        jit_builder.symbol("kryos_builtin_max", kryos_rt::builtins::kryos_builtin_max as *const u8);
        jit_builder.symbol("kryos_builtin_max_f", kryos_rt::builtins::kryos_builtin_max_f as *const u8);
        jit_builder.symbol("kryos_builtin_min", kryos_rt::builtins::kryos_builtin_min as *const u8);
        jit_builder.symbol("kryos_builtin_min_f", kryos_rt::builtins::kryos_builtin_min_f as *const u8);
        jit_builder.symbol("kryos_builtin_pop", kryos_rt::builtins::kryos_builtin_pop as *const u8);
        jit_builder.symbol("kryos_builtin_pow", kryos_rt::builtins::kryos_builtin_pow as *const u8);
        jit_builder.symbol("kryos_builtin_push", kryos_rt::builtins::kryos_builtin_push as *const u8);
        jit_builder.symbol("kryos_builtin_read_line", kryos_rt::builtins::kryos_builtin_read_line as *const u8);
        jit_builder.symbol("kryos_builtin_replace", kryos_rt::builtins::kryos_builtin_replace as *const u8);
        jit_builder.symbol("kryos_builtin_reverse", kryos_rt::builtins::kryos_builtin_reverse as *const u8);
        jit_builder.symbol("kryos_builtin_sin", kryos_rt::builtins::kryos_builtin_sin as *const u8);
        jit_builder.symbol("kryos_builtin_sort", kryos_rt::builtins::kryos_builtin_sort as *const u8);
        jit_builder.symbol("kryos_builtin_sort_str", kryos_rt::builtins::kryos_builtin_sort_str as *const u8);
        jit_builder.symbol("kryos_builtin_sort_f64", kryos_rt::builtins::kryos_builtin_sort_f64 as *const u8);
        jit_builder.symbol("kryos_builtin_split", kryos_rt::builtins::kryos_builtin_split as *const u8);
        jit_builder.symbol("kryos_builtin_sqrt", kryos_rt::builtins::kryos_builtin_sqrt as *const u8);
        jit_builder.symbol("kryos_builtin_starts_with", kryos_rt::builtins::kryos_builtin_starts_with as *const u8);
        jit_builder.symbol("kryos_builtin_substr", kryos_rt::builtins::kryos_builtin_substr as *const u8);
        jit_builder.symbol("kryos_builtin_tan", kryos_rt::builtins::kryos_builtin_tan as *const u8);
        jit_builder.symbol("kryos_builtin_to_lower", kryos_rt::builtins::kryos_builtin_to_lower as *const u8);
        jit_builder.symbol("kryos_builtin_to_upper", kryos_rt::builtins::kryos_builtin_to_upper as *const u8);
        jit_builder.symbol("kryos_builtin_trim", kryos_rt::builtins::kryos_builtin_trim as *const u8);
        jit_builder.symbol("kryos_builtin_trim_end", kryos_rt::builtins::kryos_builtin_trim_end as *const u8);
        jit_builder.symbol("kryos_builtin_trim_start", kryos_rt::builtins::kryos_builtin_trim_start as *const u8);
        jit_builder.symbol("kryos_string_char_at", kryos_rt::builtins::kryos_string_char_at as *const u8);
        jit_builder.symbol("kryos_string_contains", kryos_rt::string::kryos_string_contains as *const u8);
        jit_builder.symbol("kryos_string_ends_with", kryos_rt::string::kryos_string_ends_with as *const u8);
        jit_builder.symbol("kryos_string_hash", kryos_rt::string::kryos_string_hash as *const u8);
        jit_builder.symbol("kryos_string_replace", kryos_rt::string::kryos_string_replace as *const u8);
        jit_builder.symbol("kryos_string_starts_with", kryos_rt::string::kryos_string_starts_with as *const u8);
        jit_builder.symbol("kryos_string_to_lower", kryos_rt::string::kryos_string_to_lower as *const u8);
        jit_builder.symbol("kryos_string_to_upper", kryos_rt::string::kryos_string_to_upper as *const u8);
        jit_builder.symbol("kryos_string_trim", kryos_rt::string::kryos_string_trim as *const u8);

        // Stdlib-native: FFI (dlopen/dlsym/dlcall*, ptr interop, raw read/write).
        // These extern "C" symbols are implemented in kryos-stdlib-native::ffi and
        // statically linked on the AOT path, but the JIT must register the function
        // pointers itself or calls from `kryos run` fail to resolve at link time.
        use kryos_stdlib_native::ffi as kffi;
        jit_builder.symbol("kryos_ffi_dlopen", kffi::kryos_ffi_dlopen as *const u8);
        jit_builder.symbol("kryos_ffi_dlsym", kffi::kryos_ffi_dlsym as *const u8);
        jit_builder.symbol("kryos_ffi_dlclose", kffi::kryos_ffi_dlclose as *const u8);
        jit_builder.symbol("kryos_ffi_dlcall0", kffi::kryos_ffi_dlcall0 as *const u8);
        jit_builder.symbol("kryos_ffi_dlcall1", kffi::kryos_ffi_dlcall1 as *const u8);
        jit_builder.symbol("kryos_ffi_dlcall2", kffi::kryos_ffi_dlcall2 as *const u8);
        jit_builder.symbol("kryos_ffi_dlcall3", kffi::kryos_ffi_dlcall3 as *const u8);
        jit_builder.symbol("kryos_ffi_dlcall4", kffi::kryos_ffi_dlcall4 as *const u8);
        jit_builder.symbol("kryos_ffi_dlcall5", kffi::kryos_ffi_dlcall5 as *const u8);
        jit_builder.symbol("kryos_ffi_dlcall6", kffi::kryos_ffi_dlcall6 as *const u8);
        jit_builder.symbol("kryos_ffi_dlcall7", kffi::kryos_ffi_dlcall7 as *const u8);
        jit_builder.symbol("kryos_ffi_dlcall8", kffi::kryos_ffi_dlcall8 as *const u8);
        jit_builder.symbol("kryos_ffi_dlcallv0", kffi::kryos_ffi_dlcallv0 as *const u8);
        jit_builder.symbol("kryos_ffi_dlcallv1", kffi::kryos_ffi_dlcallv1 as *const u8);
        jit_builder.symbol("kryos_ffi_dlcallv2", kffi::kryos_ffi_dlcallv2 as *const u8);
        jit_builder.symbol("kryos_ffi_dlcallv3", kffi::kryos_ffi_dlcallv3 as *const u8);
        jit_builder.symbol("kryos_ffi_dlcallv4", kffi::kryos_ffi_dlcallv4 as *const u8);
        jit_builder.symbol("kryos_ffi_dlcallv5", kffi::kryos_ffi_dlcallv5 as *const u8);
        jit_builder.symbol("kryos_ffi_dlcallv6", kffi::kryos_ffi_dlcallv6 as *const u8);
        jit_builder.symbol("kryos_ffi_dlcallv7", kffi::kryos_ffi_dlcallv7 as *const u8);
        jit_builder.symbol("kryos_ffi_dlcallv_4f", kffi::kryos_ffi_dlcallv_4f as *const u8);
        jit_builder.symbol("kryos_ffi_dlcallv_4f32", kffi::kryos_ffi_dlcallv_4f32 as *const u8);
        jit_builder.symbol("kryos_ffi_dlcallv_3f32", kffi::kryos_ffi_dlcallv_3f32 as *const u8);
        jit_builder.symbol("kryos_ffi_cstr", kffi::kryos_ffi_cstr as *const u8);
        jit_builder.symbol("kryos_ffi_strlen", kffi::kryos_ffi_strlen as *const u8);
        jit_builder.symbol("kryos_ffi_string_from_ptr", kffi::kryos_ffi_string_from_ptr as *const u8);
        jit_builder.symbol("kryos_ffi_malloc", kffi::kryos_ffi_malloc as *const u8);
        jit_builder.symbol("kryos_ffi_free", kffi::kryos_ffi_free as *const u8);
        jit_builder.symbol("kryos_ffi_read_i8", kffi::kryos_ffi_read_i8 as *const u8);
        jit_builder.symbol("kryos_ffi_read_i16", kffi::kryos_ffi_read_i16 as *const u8);
        jit_builder.symbol("kryos_ffi_read_i32", kffi::kryos_ffi_read_i32 as *const u8);
        jit_builder.symbol("kryos_ffi_read_i64", kffi::kryos_ffi_read_i64 as *const u8);
        jit_builder.symbol("kryos_ffi_read_f32", kffi::kryos_ffi_read_f32 as *const u8);
        jit_builder.symbol("kryos_ffi_read_f64", kffi::kryos_ffi_read_f64 as *const u8);
        jit_builder.symbol("kryos_ffi_read_f32_bits", kffi::kryos_ffi_read_f32_bits as *const u8);
        jit_builder.symbol("kryos_ffi_read_f64_bits", kffi::kryos_ffi_read_f64_bits as *const u8);
        jit_builder.symbol("kryos_ffi_write_i8", kffi::kryos_ffi_write_i8 as *const u8);
        jit_builder.symbol("kryos_ffi_write_i16", kffi::kryos_ffi_write_i16 as *const u8);
        jit_builder.symbol("kryos_ffi_write_i32", kffi::kryos_ffi_write_i32 as *const u8);
        jit_builder.symbol("kryos_ffi_write_i64", kffi::kryos_ffi_write_i64 as *const u8);
        jit_builder.symbol("kryos_ffi_write_f32", kffi::kryos_ffi_write_f32 as *const u8);
        jit_builder.symbol("kryos_ffi_write_f64", kffi::kryos_ffi_write_f64 as *const u8);
        jit_builder.symbol("kryos_ffi_write_f32_bits", kffi::kryos_ffi_write_f32_bits as *const u8);
        jit_builder.symbol("kryos_ffi_write_f64_bits", kffi::kryos_ffi_write_f64_bits as *const u8);

        // Bulk-register EVERY kryos-stdlib-native symbol from the generated
        // inventory. The hand-maintained registrations above cover kryos-rt
        // and a historical subset of stdlib-native; the inventory closes the
        // rest (sqlite, regex find/captures, http2, crypto, ...) so a stdlib
        // extern that works on the AOT path can never again panic the test
        // runner's JIT with "can't resolve symbol". Later registrations of an
        // already-registered name are identical pointers, so overlap with the
        // explicit list is harmless.
        for (name, ptr) in kryos_stdlib_native::native_symbols() {
            jit_builder.symbol(name, ptr);
        }

        let module = JITModule::new(jit_builder);

        Ok(Self {
            module,
            fb_ctx: FunctionBuilderContext::new(),
            func_counter: 0,
        })
    }

    /// Compile all MIR functions together so cross-function calls resolve.
    ///
    /// Declares every function first, then translates each body, then
    /// finalizes once. Returns a map of function name -> executable pointer.
    ///
    /// Deprecated thin wrapper. Use `compile_all_with_module` so that
    /// struct/enum definitions are available to the translator. Without
    /// them, struct field access falls back to returning zero, silently
    /// producing wrong results.
    pub fn compile_all(
        &mut self,
        functions: &[MirFunction],
    ) -> Result<HashMap<String, *const u8>, CodegenError> {
        self.compile_all_inner(
            functions,
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
            &std::collections::HashSet::new(),
        )
    }

    /// Like `compile_all` but takes the full MIR module so struct/enum
    /// definitions are passed to the function translator. This is what
    /// `kryos test` and `kryos run` use to get correct behaviour for
    /// programs that touch structs or enums.
    pub fn compile_all_with_module(
        &mut self,
        module: &MirModule,
    ) -> Result<HashMap<String, *const u8>, CodegenError> {
        self.compile_all_inner(
            &module.functions,
            &module.struct_defs,
            &module.enum_defs,
            &module.trait_vtables,
            &module.copy_structs,
        )
    }

    fn compile_all_inner(
        &mut self,
        functions: &[MirFunction],
        struct_defs: &std::collections::HashMap<String, Vec<(String, kryos_mir::ir::MirType)>>,
        enum_defs: &std::collections::HashMap<String, Vec<kryos_mir::ir::EnumVariantDef>>,
        trait_vtables: &std::collections::HashMap<(String, String), Vec<String>>,
        copy_structs: &std::collections::HashSet<String>,
    ) -> Result<HashMap<String, *const u8>, CodegenError> {
        use cranelift_module::FuncId;

        let call_conv = self.module.isa().default_call_conv();

        // Declare ARC runtime (same as compile_function).
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

        let mut func_ids: HashMap<String, FuncId> = HashMap::new();

        let arc_retain_id = self.module.declare_function(
            "kryos_arc_retain_i64",
            Linkage::Import,
            &arc_retain_sig,
        )?;
        let arc_release_id = self.module.declare_function(
            "kryos_arc_release_i64",
            Linkage::Import,
            &arc_release_sig,
        )?;
        let arc_alloc_id =
            self.module
                .declare_function("kryos_arc_alloc_i64", Linkage::Import, &arc_alloc_sig)?;

        func_ids.insert("kryos_arc_retain".to_string(), arc_retain_id);
        func_ids.insert("kryos_arc_release".to_string(), arc_release_id);
        func_ids.insert("kryos_arc_alloc".to_string(), arc_alloc_id);
        func_ids.insert("kryos_arc_alloc_i64".to_string(), arc_alloc_id);

        // Declare runtime builtin functions with the same name mapping the
        // AOT codegen uses, so translate_function() can look them up. Each
        // symbol was already registered with the JIT builder in new().
        let user_func_names: std::collections::HashSet<String> =
            functions.iter().map(|f| f.name.clone()).collect();
        declare_runtime_builtins(&mut self.module, call_conv, &mut func_ids, &user_func_names)?;

        // Phase 1: Declare ALL user functions so cross-calls resolve.
        let mut declared: Vec<FuncId> = Vec::new();
        for mir_func in functions {
            let sig = build_signature(mir_func, call_conv);
            let func_id = self
                .module
                .declare_function(&mir_func.name, Linkage::Export, &sig)?;
            func_ids.insert(mir_func.name.clone(), func_id);
            declared.push(func_id);
        }

        // Phase 1.5: Scan for `RValue::Closure` and declare an env-thunk
        // `{func_name}_env(env, user_args...) -> i64` for each function used
        // as a function-pointer value. This is required for the env-based
        // CallIndirect calling convention (env[0] = thunk fn ptr). The AOT
        // path does the same scan/declare/define dance — without it, a
        // `let f: fn(...) = bare_fn; f(arg)` lowers to a CallIndirect that
        // dereferences the raw function address as if it were an env block,
        // causing a segfault (regression test: tests/smoke/test_fn_pointer).
        let mir_func_map: HashMap<&str, &MirFunction> =
            functions.iter().map(|f| (f.name.as_str(), f)).collect();
        // closure_name -> (num_captures, user_param_count)
        let mut closure_info: HashMap<String, (usize, usize)> = HashMap::new();
        for mir_func in functions {
            for bb in &mir_func.blocks {
                for inst in &bb.instructions {
                    if let Instruction::Assign {
                        value: RValue::Closure { func_name, captures },
                        ..
                    } = inst
                    {
                        if !closure_info.contains_key(func_name.as_str()) {
                            let user_params = if let Some(f) = mir_func_map.get(func_name.as_str()) {
                                f.params.len().saturating_sub(captures.len())
                            } else {
                                0
                            };
                            closure_info.insert(func_name.clone(), (captures.len(), user_params));
                        }
                    }
                }
            }
        }

        let mut thunk_ids: HashMap<String, cranelift_module::FuncId> = HashMap::new();
        for (func_name, (_, user_param_count)) in &closure_info {
            let env_thunk_name = format!("{func_name}_env");
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // env pointer
            for _ in 0..*user_param_count {
                sig.params.push(AbiParam::new(types::I64));
            }
            sig.returns.push(AbiParam::new(types::I64));
            let id = self
                .module
                .declare_function(&env_thunk_name, Linkage::Export, &sig)?;
            thunk_ids.insert(func_name.clone(), id);
            func_ids.insert(env_thunk_name, id);
        }

        // Phase 2: Translate each function body.
        // string_counter is module-wide so data section names are unique
        // across all functions (e.g. `.trace_name.0`, `.trace_name.1`, ...).
        let mut str_counter = 0u32;
        // Interned string-literal cache for this compilation pass -- see the
        // doc comment on `global_string_constants` in codegen.rs's
        // `compile_module_with_options` (leak fix: a literal used repeatedly,
        // e.g. a map key inside a loop, used to allocate a fresh heap string
        // on every evaluation).
        let mut string_constants: HashMap<String, DataId> = HashMap::new();
        for (mir_func, &func_id) in functions.iter().zip(declared.iter()) {
            let sig = build_signature(mir_func, call_conv);
            let func_index = self.func_counter;
            self.func_counter += 1;

            let mut cl_func = Function::with_name_signature(UserFuncName::user(0, func_index), sig);

            {
                let mut builder = FunctionBuilder::new(&mut cl_func, &mut self.fb_ctx);
                crate::codegen::translate_function(
                    mir_func,
                    &mut builder,
                    &func_ids,
                    &mut self.module,
                    struct_defs,
                    enum_defs,
                    &mut str_counter,
                    &mut string_constants,
                    trait_vtables,
                    false,
                    copy_structs,
                    &user_func_names,
                )?;
                builder.seal_all_blocks();
                builder.finalize();
            }

            let mut ctx = Context::for_function(cl_func);
            self.module.define_function(func_id, &mut ctx).map_err(|e| {
                // Include the function name AND a debug-formatted error so
                // verifier reports are actionable. `Debug` for ModuleError
                // includes the underlying CompileError variant details.
                if std::env::var("KRYOS_JIT_DUMP_IR").is_ok() {
                    eprintln!("[kryos-jit] IR for '{}':\n{}", mir_func.name, ctx.func.display());
                }
                CodegenError::Internal(format!(
                    "function `{}`: {e:?}",
                    mir_func.name
                ))
            })?;
        }

        // Phase 2.5: Generate the env-thunk function bodies. Each thunk loads
        // captures from env at offsets 8.. (count=num_captures), appends user
        // args from its own parameters, and tail-calls the original function.
        for (func_name, (num_captures, user_param_count)) in &closure_info {
            let env_thunk_id = thunk_ids[func_name.as_str()];

            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64)); // env
            for _ in 0..*user_param_count {
                sig.params.push(AbiParam::new(types::I64));
            }
            sig.returns.push(AbiParam::new(types::I64));

            let func_index = self.func_counter;
            self.func_counter += 1;
            let mut cl_func =
                Function::with_name_signature(UserFuncName::user(0, func_index), sig);

            {
                let mut builder = FunctionBuilder::new(&mut cl_func, &mut self.fb_ctx);
                let entry = builder.create_block();
                builder.append_block_params_for_function_params(entry);
                builder.switch_to_block(entry);

                let block_params: Vec<_> = builder.block_params(entry).to_vec();
                let env_val = block_params[0];

                // Load captures from env at offsets 8, 16, ...
                let mut call_args: Vec<cranelift_codegen::ir::Value> = Vec::new();
                for i in 0..*num_captures {
                    let offset = ((i + 1) * 8) as i32;
                    let cap = builder
                        .ins()
                        .load(types::I64, MemFlags::new(), env_val, offset);
                    call_args.push(cap);
                }

                // Append user args from thunk parameters (indices 1..).
                for i in 0..*user_param_count {
                    call_args.push(block_params[1 + i]);
                }

                // Coerce args to match the original function's signature types.
                let orig_sig = if let Some(f) = mir_func_map.get(func_name.as_str()) {
                    build_signature(f, call_conv)
                } else {
                    let mut s = Signature::new(call_conv);
                    for _ in 0..call_args.len() {
                        s.params.push(AbiParam::new(types::I64));
                    }
                    s.returns.push(AbiParam::new(types::I64));
                    s
                };
                for (i, arg) in call_args.iter_mut().enumerate() {
                    if i < orig_sig.params.len() {
                        let expected = orig_sig.params[i].value_type;
                        let actual = builder.func.dfg.value_type(*arg);
                        if expected != actual {
                            if expected.is_int() && actual.is_int() {
                                if expected.bits() < actual.bits() {
                                    *arg = builder.ins().ireduce(expected, *arg);
                                } else {
                                    *arg = builder.ins().sextend(expected, *arg);
                                }
                            } else if expected.is_float() && actual.is_int() {
                                *arg = builder.ins().bitcast(expected, MemFlags::new(), *arg);
                            } else if expected.is_int() && actual.is_float() {
                                *arg = builder.ins().bitcast(expected, MemFlags::new(), *arg);
                            }
                        }
                    }
                }

                // Call the original function and widen its return to i64.
                let orig_id = func_ids[func_name.as_str()];
                let orig_ref = self.module.declare_func_in_func(orig_id, builder.func);
                let call_inst = builder.ins().call(orig_ref, &call_args);

                let results = builder.inst_results(call_inst).to_vec();
                let ret_val = if results.is_empty() {
                    builder.ins().iconst(types::I64, 0)
                } else {
                    let result = results[0];
                    let result_ty = builder.func.dfg.value_type(result);
                    if result_ty == types::I64 {
                        result
                    } else if result_ty.is_int() && result_ty.bits() < 64 {
                        builder.ins().sextend(types::I64, result)
                    } else if result_ty.is_float() {
                        builder.ins().bitcast(types::I64, MemFlags::new(), result)
                    } else {
                        result
                    }
                };

                builder.ins().return_(&[ret_val]);
                builder.seal_all_blocks();
                builder.finalize();
            }

            let mut ctx = Context::for_function(cl_func);
            self.module
                .define_function(env_thunk_id, &mut ctx)
                .map_err(|e| {
                    CodegenError::Internal(format!("env thunk `{}_env`: {e:?}", func_name))
                })?;
        }

        // Phase 3: Finalize once.
        self.module
            .finalize_definitions()
            .map_err(|e| CodegenError::Internal(e.to_string()))?;

        let mut result = HashMap::new();
        for (mir_func, &func_id) in functions.iter().zip(declared.iter()) {
            let ptr = self.module.get_finalized_function(func_id);
            result.insert(mir_func.name.clone(), ptr);
        }
        Ok(result)
    }

    /// Compile a single MIR function and return its executable pointer.
    pub fn compile_function(&mut self, mir_func: &MirFunction) -> Result<*const u8, CodegenError> {
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
            "kryos_arc_retain_i64",
            Linkage::Import,
            &arc_retain_sig,
        )?;
        let arc_release_id = self.module.declare_function(
            "kryos_arc_release_i64",
            Linkage::Import,
            &arc_release_sig,
        )?;
        let arc_alloc_id =
            self.module
                .declare_function("kryos_arc_alloc_i64", Linkage::Import, &arc_alloc_sig)?;

        func_ids.insert("kryos_arc_retain".to_string(), arc_retain_id);
        func_ids.insert("kryos_arc_release".to_string(), arc_release_id);
        func_ids.insert("kryos_arc_alloc".to_string(), arc_alloc_id);
        func_ids.insert("kryos_arc_alloc_i64".to_string(), arc_alloc_id);

        // Declare the function itself.
        let func_id = self
            .module
            .declare_function(&mir_func.name, Linkage::Export, &sig)?;
        func_ids.insert(mir_func.name.clone(), func_id);

        // Build the Cranelift IR.
        let func_index = self.func_counter;
        self.func_counter += 1;

        let mut cl_func = Function::with_name_signature(UserFuncName::user(0, func_index), sig);

        {
            let empty_struct_defs = std::collections::HashMap::new();
            let empty_enum_defs = std::collections::HashMap::new();
            let empty_trait_vtables = std::collections::HashMap::new();
            let empty_copy_structs = std::collections::HashSet::new();
            let single_user_func_names: std::collections::HashSet<String> =
                std::iter::once(mir_func.name.clone()).collect();
            let mut str_counter = 0u32;
            let mut string_constants: HashMap<String, DataId> = HashMap::new();
            let mut builder = FunctionBuilder::new(&mut cl_func, &mut self.fb_ctx);
            crate::codegen::translate_function(
                mir_func,
                &mut builder,
                &func_ids,
                &mut self.module,
                &empty_struct_defs,
                &empty_enum_defs,
                &mut str_counter,
                &mut string_constants,
                &empty_trait_vtables,
                false, // no checked arithmetic in JIT/REPL
                &empty_copy_structs,
                &single_user_func_names,
            )?;
            builder.seal_all_blocks();
            builder.finalize();
        }

        let mut ctx = Context::for_function(cl_func);
        self.module
            .define_function(func_id, &mut ctx)
            .map_err(CodegenError::Module)?;

        // Finalize: patch relocations and make code executable.
        self.module
            .finalize_definitions()
            .map_err(|e| CodegenError::Internal(e.to_string()))?;

        let code_ptr = self.module.get_finalized_function(func_id);
        Ok(code_ptr)
    }
}

// ARC runtime functions are now provided by kryos-rt (no stubs needed).

/// Declare all runtime builtin functions in the JIT module so the codegen's
/// `ensure_func_ref_with_args` can find them by the names it expects (e.g.,
/// `"len"` → `kryos_builtin_len`, `"println"` → `kryos_println_str`).
///
/// This mirrors the declarations in `codegen::emit_module` for the AOT path.
fn declare_runtime_builtins<M: Module>(
    module: &mut M,
    call_conv: cranelift_codegen::isa::CallConv,
    func_ids: &mut HashMap<String, cranelift_module::FuncId>,
    user_shadowed: &std::collections::HashSet<String>,
) -> Result<(), CodegenError> {
    // All signatures use i64 params and i64 return (matching the generic
    // signature that `ensure_func_ref_with_args` creates). This prevents
    // signature conflicts when the codegen dynamically re-declares a function.
    // Functions that actually return void will have an unused return value,
    // which is harmless at the ABI level.
    let sig = |n: usize| -> Signature {
        let mut s = Signature::new(call_conv);
        for _ in 0..n {
            s.params.push(AbiParam::new(types::I64));
        }
        s.returns.push(AbiParam::new(types::I64));
        s
    };
    // f64 → i64 (e.g. kryos_json_number).
    let sig_f64_i64 = {
        let mut s = Signature::new(call_conv);
        s.params.push(AbiParam::new(types::F64));
        s.returns.push(AbiParam::new(types::I64));
        s
    };
    // i64 → f64 (e.g. kryos_json_to_float).
    let sig_i64_f64 = {
        let mut s = Signature::new(call_conv);
        s.params.push(AbiParam::new(types::I64));
        s.returns.push(AbiParam::new(types::F64));
        s
    };

    // Declare a function and insert name mapping(s). The codegen_name is
    // what translate_function looks up in func_ids; the symbol_name is the
    // actual runtime symbol registered in the JIT builder.
    macro_rules! decl {
        ($codegen_name:expr, $symbol_name:expr, $sig:expr) => {
            let id = module.declare_function($symbol_name, Linkage::Import, &$sig)?;
            // A USER function of the same bare name SHADOWS the builtin -- the
            // documented contract, and what the LLVM backend already does via
            // its `user_shadow` check. Binding the bare name to this Import
            // meant the user's own definition could not be defined at all:
            // `fn alloc(n: i64) -> i64` failed the whole compile with
            // "Invalid to define identifier declared as an import:
            // kryos_alloc_bytes", while AOT compiled it and printed the right
            // answer. Leave the bare name free so Phase 1 can bind it to the
            // user's function; the raw kryos_* symbol stays available for the
            // codegen paths that call it directly.
            if !user_shadowed.contains($codegen_name) {
                func_ids.insert($codegen_name.to_string(), id);
            }
            // Also store under symbol name if different, so the codegen can
            // find it regardless of which name path it uses.
            if $codegen_name != $symbol_name {
                func_ids.insert($symbol_name.to_string(), id);
            }
        };
    }

    // --- I/O builtins (codegen uses runtime name directly) ---
    decl!("println", "kryos_println_str", sig(1));
    decl!("print", "kryos_print_str", sig(1));
    decl!("eprintln", "kryos_eprintln_str", sig(1));

    // --- Utility builtins ---
    decl!("exit", "kryos_builtin_exit", sig(1));
    decl!("len", "kryos_builtin_len", sig(1));
    decl!("to_string", "kryos_builtin_to_string", sig(1));
    decl!("kryos_f64_to_string", "kryos_f64_to_string", sig(1));
    decl!("kryos_bool_to_string", "kryos_bool_to_string", sig(1));
    decl!("kryos_i64_to_string", "kryos_i64_to_string", sig(1));
    decl!("kryos_u64_to_string", "kryos_u64_to_string", sig(1));
    decl!("kryos_ipow", "kryos_ipow", sig(2));
    decl!("kryos_builtin_assert", "kryos_builtin_assert", sig(2));
    decl!("kryos_builtin_assert_eq", "kryos_builtin_assert_eq", sig(2));
    decl!("kryos_builtin_panic", "kryos_builtin_panic", sig(1));
    decl!("kryos_builtin_type_of", "kryos_builtin_type_of", sig(1));
    decl!("kryos_builtin_parse_int", "kryos_builtin_parse_int", sig(1));
    decl!(
        "kryos_builtin_parse_float",
        "kryos_builtin_parse_float",
        sig(1)
    );

    // --- Memory management ---
    decl!("malloc", "malloc", sig(1));
    decl!("free", "free", sig(1));
    decl!("realloc", "realloc", sig(2));

    // --- String runtime ---
    decl!("kryos_string_new", "kryos_string_new", sig(2));
    // FFI/pointer-helper builtins -- mirror the AOT builtin table in
    // codegen.rs (compile_module_with_options). Without these the JIT
    // declared the callee under its bare MIR name ("str_to_ptr") and
    // finalization aborted with "can't resolve symbol str_to_ptr".
    decl!("str_to_ptr", "kryos_str_to_ptr", sig(1));
    decl!("alloc", "kryos_alloc_bytes", sig(1));
    decl!("handle_to_str", "kryos_handle_to_str", sig(1));
    decl!("buf_to_str", "kryos_buf_to_str", sig(2));
    decl!("ptr_byte_at", "kryos_ptr_byte_at", sig(2));
    decl!("ptr_read_i64", "kryos_ptr_read_i64", sig(2));
    decl!("free_bytes", "kryos_free_bytes", sig(2));
    decl!("ptr_set_byte", "kryos_ptr_set_byte", sig(3));
    decl!("ptr_write_i64", "kryos_ptr_write_i64", sig(3));
    decl!("kryos_array_clone_deep", "kryos_array_clone_deep", sig(2));
    decl!("kryos_string_char_at", "kryos_string_char_at", sig(2));
    decl!("calloc", "calloc", sig(2));
    decl!("kryos_string_concat", "kryos_string_concat", sig(2));
    decl!("kryos_string_len", "kryos_string_len", sig(1));
    decl!("kryos_string_eq", "kryos_string_eq", sig(2));
    decl!("kryos_string_compare", "kryos_string_compare", sig(2));
    decl!("kryos_string_slice", "kryos_string_slice", sig(3));
    decl!("kryos_string_find", "kryos_string_find", sig(2));
    decl!("kryos_string_free", "kryos_string_free", sig(1));
    decl!("kryos_string_clone", "kryos_string_clone", sig(1));
    decl!("kryos_string_retain", "kryos_string_retain", sig(1));
    decl!("kryos_string_retain_opt", "kryos_string_retain_opt", sig(1));
    decl!("kryos_array_retain_opt", "kryos_array_retain_opt", sig(1));
    decl!("kryos_array_dup", "kryos_array_dup", sig(2));
    decl!("kryos_map_retain_opt", "kryos_map_retain_opt", sig(1));
    decl!("kryos_diag_site", "kryos_diag_site", sig(1));

    // --- Array runtime ---
    decl!("kryos_array_new", "kryos_array_new", sig(2));
    decl!("kryos_array_push", "kryos_array_push", sig(2));
    decl!("kryos_array_get", "kryos_array_get", sig(2));
    decl!("kryos_array_set", "kryos_array_set", sig(3));
    decl!("kryos_array_len", "kryos_array_len", sig(1));
    decl!("kryos_array_free", "kryos_array_free", sig(1));
    decl!("kryos_array_concat", "kryos_array_concat", sig(2));
    decl!("kryos_array_clone", "kryos_array_clone", sig(1));
    decl!("kryos_array_retain", "kryos_array_retain", sig(1));

    // --- Map runtime ---
    decl!("kryos_map_new", "kryos_map_new", sig(0));
    decl!("kryos_map_insert", "kryos_map_insert", sig(3));
    decl!("kryos_map_insert_str", "kryos_map_insert_str", sig(3));
    decl!("kryos_map_get", "kryos_map_get", sig(2));
    decl!("kryos_map_get_str", "kryos_map_get_str", sig(2));
    decl!("kryos_map_len", "kryos_map_len", sig(1));
    decl!("kryos_map_free", "kryos_map_free", sig(1));
    decl!("kryos_map_clone", "kryos_map_clone", sig(1));
    decl!("kryos_map_snapshot", "kryos_map_snapshot", sig(3));
    decl!("kryos_map_retain", "kryos_map_retain", sig(1));
    decl!("kryos_map_has", "kryos_map_has", sig(2));
    decl!("kryos_map_has_str", "kryos_map_has_str", sig(2));
    decl!("kryos_map_delete", "kryos_map_delete", sig(2));
    decl!("kryos_map_delete_str", "kryos_map_delete_str", sig(2));
    decl!("kryos_map_keys", "kryos_map_keys", sig(1));
    decl!("kryos_map_keys_str", "kryos_map_keys_str", sig(1));

    // --- Trace runtime (uses ensure_func_ref_void in codegen, but we
    //     declare with i64 return for consistency — unused return is fine) ---
    decl!("kryos_trace_enter", "kryos_trace_enter", sig(5));
    decl!("kryos_trace_exit", "kryos_trace_exit", sig(0));

    // --- Panic ---
    decl!("kryos_panic", "kryos_panic", sig(1));
    decl!(
        "kryos_panic_with_location",
        "kryos_panic_with_location",
        sig(3)
    );
    decl!(
        "kryos_check_div_zero_i64",
        "kryos_check_div_zero_i64",
        sig(1)
    );
    decl!("kryos_check_sdiv_i64", "kryos_check_sdiv_i64", sig(3));

    // --- Channel / Actor / Spawn ---
    decl!("kryos_chan_new_i64", "kryos_chan_new_i64", sig(0));
    decl!("kryos_chan_send_i64", "kryos_chan_send_i64", sig(2));
    decl!("kryos_chan_recv_i64", "kryos_chan_recv_i64", sig(1));
    decl!("kryos_chan_try_recv_status_i64", "kryos_chan_try_recv_status_i64", sig(1));
    decl!("kryos_chan_last_recv_i64", "kryos_chan_last_recv_i64", sig(0));
    decl!("kryos_spawn", "kryos_spawn", sig(2));
    decl!("kryos_spawn_wait_all", "kryos_spawn_wait_all", sig(0));
    decl!("kryos_actor_wait_all", "kryos_actor_wait_all", sig(0));
    decl!("kryos_sleep", "kryos_sleep", sig(1));
    decl!("kryos_sleep_ms", "kryos_sleep_ms", sig(1));

    // --- Cooperative async executor (await/yield interleaving) ---
    decl!("kryos_coop_spawn", "kryos_coop_spawn", sig(2));
    decl!("kryos_coop_yield", "kryos_coop_yield", sig(0));
    decl!("kryos_coop_run", "kryos_coop_run", sig(0));
    decl!("kryos_coop_record", "kryos_coop_record", sig(1));
    decl!("kryos_coop_order", "kryos_coop_order", sig(0));
    decl!("kryos_coop_reset", "kryos_coop_reset", sig(0));

    // --- ARC set_drop ---
    decl!("kryos_arc_set_drop", "kryos_arc_set_drop_i64", sig(2));

    // --- Exception check ---
    decl!("kryos_exception_check", "kryos_exception_check", sig(0));
    decl!(
        "kryos_exception_report_uncaught_if_pending",
        "kryos_exception_report_uncaught_if_pending",
        sig(0)
    );
    decl!("kryos_budget_push", "kryos_budget_push", sig(2));
    decl!("kryos_budget_pop_to", "kryos_budget_pop_to", sig(1));
    decl!("kryos_budget_active", "kryos_budget_active", sig(0));
    decl!("kryos_budget_try_call", "kryos_budget_try_call", sig(0));
    decl!("kryos_budget_charge_tokens", "kryos_budget_charge_tokens", sig(1));
    decl!("kryos_budget_remaining_tokens", "kryos_budget_remaining_tokens", sig(0));
    decl!("kryos_budget_remaining_calls", "kryos_budget_remaining_calls", sig(0));
    decl!("kryos_budget_push_usd", "kryos_budget_push_usd", sig(3));
    decl!("kryos_budget_charge_usd_micros", "kryos_budget_charge_usd_micros", sig(1));
    decl!("kryos_budget_remaining_usd_micros", "kryos_budget_remaining_usd_micros", sig(0));

    // --- Networking (Kryos-string-handle wrappers) ---
    decl!("kryos_tcp_connect_ks", "kryos_tcp_connect_ks", sig(2));
    decl!("kryos_tcp_bind_ks", "kryos_tcp_bind_ks", sig(2));
    decl!("kryos_tcp_accept", "kryos_tcp_accept", sig(1));
    decl!("kryos_tcp_send_ks", "kryos_tcp_send_ks", sig(2));
    decl!("kryos_tcp_recv_ks", "kryos_tcp_recv_ks", sig(2));
    decl!("kryos_socket_close_ks", "kryos_socket_close_ks", sig(1));
    decl!("kryos_tcp_set_nonblocking", "kryos_tcp_set_nonblocking", sig(2));
    decl!("kryos_tcp_try_accept", "kryos_tcp_try_accept", sig(1));
    decl!("kryos_tcp_try_recv_ks", "kryos_tcp_try_recv_ks", sig(2));
    // TLS server (Gap A)
    decl!("kryos_tls_server_config_ks", "kryos_tls_server_config_ks", sig(2));
    decl!("kryos_tls_accept", "kryos_tls_accept", sig(2));
    decl!("kryos_tls_send_ks", "kryos_tls_send_ks", sig(2));
    decl!("kryos_tls_recv_ks", "kryos_tls_recv_ks", sig(2));
    decl!("kryos_tls_close_ks", "kryos_tls_close_ks", sig(1));
    // Unix domain sockets (v2.0)
    decl!("kryos_uds_connect_ks", "kryos_uds_connect_ks", sig(1));
    decl!("kryos_uds_bind_ks", "kryos_uds_bind_ks", sig(1));
    decl!("kryos_uds_accept", "kryos_uds_accept", sig(1));
    decl!("kryos_uds_send_ks", "kryos_uds_send_ks", sig(2));
    decl!("kryos_uds_recv_ks", "kryos_uds_recv_ks", sig(2));
    decl!("kryos_uds_close", "kryos_uds_close", sig(1));
    // WebSocket helpers (v2.0)
    decl!("kryos_ws_accept_key_ks", "kryos_ws_accept_key_ks", sig(1));
    decl!("kryos_ws_encode_text_ks", "kryos_ws_encode_text_ks", sig(1));
    decl!("kryos_ws_encode_binary_ks", "kryos_ws_encode_binary_ks", sig(1));
    decl!("kryos_ws_encode_close", "kryos_ws_encode_close", sig(1));
    decl!("kryos_ws_encode_ping_ks", "kryos_ws_encode_ping_ks", sig(1));
    decl!("kryos_ws_encode_pong_ks", "kryos_ws_encode_pong_ks", sig(1));
    decl!("kryos_ws_unmask_ks", "kryos_ws_unmask_ks", sig(4));
    decl!("kryos_ws_read_frame_ks", "kryos_ws_read_frame_ks", sig(1));
    // kryos_sleep_ms is declared above in the spawn block.
    // PostgreSQL (Gap B)
    decl!("kryos_pg_connect_ks", "kryos_pg_connect_ks", sig(1));
    decl!("kryos_pg_exec_ks", "kryos_pg_exec_ks", sig(2));
    decl!("kryos_pg_query_ks", "kryos_pg_query_ks", sig(2));
    decl!("kryos_pg_close_ks", "kryos_pg_close_ks", sig(1));

    // --- JSON ---
    decl!("kryos_json_parse", "kryos_json_parse", sig(1));
    decl!("kryos_json_stringify", "kryos_json_stringify", sig(1));
    decl!("kryos_json_object", "kryos_json_object", sig(2));
    decl!("kryos_json_array", "kryos_json_array", sig(1));
    decl!("kryos_json_string", "kryos_json_string", sig(1));
    decl!("kryos_json_number", "kryos_json_number", sig_f64_i64);
    decl!("kryos_json_bool", "kryos_json_bool", sig(1));
    decl!("kryos_json_null", "kryos_json_null", sig(0));
    decl!("kryos_json_get", "kryos_json_get", sig(2));
    decl!("kryos_json_get_index", "kryos_json_get_index", sig(2));
    decl!("kryos_json_to_str", "kryos_json_to_str", sig(1));
    decl!("kryos_json_to_int", "kryos_json_to_int", sig(1));
    decl!("kryos_json_to_float", "kryos_json_to_float", sig_i64_f64);
    decl!("kryos_json_is_null", "kryos_json_is_null", sig(1));
    decl!("kryos_json_length", "kryos_json_length", sig(1));
    decl!("kryos_json_type", "kryos_json_type", sig(1));

    // --- Crypto / Regex / HTTP (handle-based wrappers in `bindings`) ---
    decl!("kryos_sha256_ks", "kryos_sha256_ks", sig(1));
    decl!("kryos_sha1_hex_ks", "kryos_sha1_hex_ks", sig(1));
    decl!("kryos_sha1_base64_ks", "kryos_sha1_base64_ks", sig(1));
    decl!("kryos_base64_encode_ks", "kryos_base64_encode_ks", sig(1));
    decl!("kryos_base64_decode_ks", "kryos_base64_decode_ks", sig(1));
    decl!("kryos_chr_ks", "kryos_chr_ks", sig(1));
    decl!("kryos_byte_at_ks", "kryos_byte_at_ks", sig(2));
    decl!("kryos_sha512_ks", "kryos_sha512_ks", sig(1));
    decl!("kryos_hmac_sha256_ks", "kryos_hmac_sha256_ks", sig(2));
    decl!("kryos_ed25519_generate_ks", "kryos_ed25519_generate_ks", sig(0));
    decl!("kryos_ed25519_public_ks", "kryos_ed25519_public_ks", sig(1));
    decl!("kryos_ed25519_sign_ks", "kryos_ed25519_sign_ks", sig(2));
    decl!("kryos_ed25519_verify_ks", "kryos_ed25519_verify_ks", sig(3));
    decl!("kryos_pbkdf2_sha256_ks", "kryos_pbkdf2_sha256_ks", sig(3));
    decl!("kryos_hex_to_b64url_ks", "kryos_hex_to_b64url_ks", sig(1));
    decl!("kryos_b64url_to_hex_ks", "kryos_b64url_to_hex_ks", sig(1));
    decl!("kryos_random_bytes_ks", "kryos_random_bytes_ks", sig(1));
    decl!("kryos_regex_new_ks", "kryos_regex_new_ks", sig(1));
    decl!("kryos_regex_is_match_ks", "kryos_regex_is_match_ks", sig(2));
    decl!("kryos_regex_find_ks", "kryos_regex_find_ks", sig(2));
    decl!("kryos_regex_find_pos_ks", "kryos_regex_find_pos_ks", sig(3));
    decl!("kryos_regex_find_end_ks", "kryos_regex_find_end_ks", sig(3));
    decl!("kryos_regex_replace_all_ks", "kryos_regex_replace_all_ks", sig(3));
    decl!("kryos_regex_drop_ks", "kryos_regex_drop_ks", sig(1));
    decl!("kryos_http_request_ks", "kryos_http_request_ks", sig(5));
    decl!("kryos_https_get_ks", "kryos_https_get_ks", sig(1));
    // HTTP/2 client (Gap C)
    decl!("kryos_http2_get_ks", "kryos_http2_get_ks", sig(1));
    decl!("kryos_http2_post_ks", "kryos_http2_post_ks", sig(2));
    decl!("kryos_http2_request_ks", "kryos_http2_request_ks", sig(4));
    decl!("kryos_dom_set_text_ks", "kryos_dom_set_text_ks", sig(2));
    decl!("kryos_dom_get_value_ks", "kryos_dom_get_value_ks", sig(1));
    decl!("kryos_alert_ks", "kryos_alert_ks", sig(1));
    decl!("kryos_canvas_fill_rect_ks", "kryos_canvas_fill_rect_ks", sig(6));
    decl!("kryos_canvas_clear_ks", "kryos_canvas_clear_ks", sig(1));
    decl!("kryos_fetch_text_ks", "kryos_fetch_text_ks", sig(1));

    // --- Time / Mutex (no string args) ---
    decl!("kryos_time_now_secs", "kryos_time_now_secs", sig(0));
    decl!("kryos_time_now_millis", "kryos_time_now_millis", sig(0));
    decl!("kryos_mutex_new", "kryos_mutex_new", sig(0));
    decl!("kryos_mutex_lock", "kryos_mutex_lock", sig(1));
    decl!("kryos_mutex_unlock", "kryos_mutex_unlock", sig(1));
    decl!("kryos_mutex_drop", "kryos_mutex_drop", sig(1));

    // --- Mutable globals ---
    decl!("kryos_global_get", "kryos_global_get", sig(1));
    // NOTE: kryos_global_set returns `()` in Rust, but we declare it with an
    // i64 return in the JIT module so that callers can uniformly use
    // `builder.inst_results(call)[0]` (the MIR Call instruction always has a
    // destination). The garbage i64 in the return register is discarded by
    // the assigned-but-unused throwaway temp.
    decl!("kryos_global_set", "kryos_global_set", sig(2));
    decl!("kryos_global_has", "kryos_global_has", sig(1));

    Ok(())
}
