//! Classification of every `kryos_*` native symbol the runtime exports
//! (`kryos-rt`, `kryos-stdlib-native`) -- bypass class B4.
//!
//! An `extern { fn kryos_X(..) }` block names a runtime native directly. What a
//! CALL to it demands is decided here, by exact name (the `kryos_` prefix
//! already stripped):
//!
//! * [`CAPABILITY_NATIVES`] -- the native exercises authority; the call needs
//!   the listed capability, the same one the language builtin or stdlib wrapper
//!   it backs needs.
//! * [`AMBIENT_NATIVES`] -- the native is genuinely authority-free (value
//!   plumbing on compiler-managed handles, pure compute, the process's own
//!   streams, self-scoped counters). The list is explicit, never a prefix rule.
//! * ANY OTHER name is unclassified and FAILS CLOSED: it requires `ffi`. Before
//!   this table the default arm returned "no capability", so a native the
//!   capability table had never heard of (`kryos_ffi_cstr`, `kryos_db_open`,
//!   `kryos_tls_connect`, `kryos_term_raw_enable`, `kryos_fs_read`, ...) was
//!   callable from a zero-authority `main` (tests/bypass/b4_*).
//!
//! `tests/native_symbols.rs` (in this crate) enumerates every `#[no_mangle] extern "C"` symbol
//! of both runtime crates from their source and fails if one is missing from
//! these two lists, so a future native has to be classified by whoever adds it
//! instead of silently being ambient.
//!
//! What "ambient" does NOT mean: the container, string and ARC natives take
//! handles (pointers). The raw-memory entry points are gated (`ffi`: `alloc`,
//! `str_to_ptr`, `arr_to_ptr`, the `kryos_ffi_*`, `kryos_ptr_*` and raw
//! allocator natives, the ARC natives that act on a caller-supplied address,
//! the `std::ffi` wrappers). The ARC allocators stay ambient because the repo
//! pins them so (tests/inferred_soundness.sh, `native_extern_pure_alloc`). The
//! ambient handle natives rest on the assumption that a handle cannot be
//! forged from an integer. That assumption fails where a stdlib struct
//! publishes a raw address in a public field (LEDGER bypass class B9); this
//! table does not claim otherwise.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::model::Capability;

/// What a call to a classified native demands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeClass {
    /// Authority-free; the call needs no capability.
    Ambient,
    /// The call needs this capability.
    Requires(Capability),
}

/// Natives that exercise authority, with the capability a call needs.
/// Names are the exported symbol without its `kryos_` prefix.
pub const CAPABILITY_NATIVES: &[(&str, Capability)] = &[
    // ffi: pointer sources and raw allocation (the natives behind alloc, free_bytes, str_to_ptr, arr_to_ptr)
    ("alloc", Capability::Ffi),
    ("alloc_bytes", Capability::Ffi),
    ("arr_to_ptr", Capability::Ffi),
    ("calloc", Capability::Ffi),
    ("dealloc", Capability::Ffi),
    ("free", Capability::Ffi),
    ("free_bytes", Capability::Ffi),
    ("realloc", Capability::Ffi),
    ("str_to_ptr", Capability::Ffi),
    // ffi: ARC / struct refcount and drop-hook natives that act on a caller-supplied address
    // (the ARC ALLOCATORS arc_alloc / arc_alloc_i64 are ambient, see AMBIENT_NATIVES)
    ("arc_ref_count", Capability::Ffi),
    ("arc_release", Capability::Ffi),
    ("arc_release_i64", Capability::Ffi),
    ("arc_retain", Capability::Ffi),
    ("arc_retain_i64", Capability::Ffi),
    ("arc_set_drop", Capability::Ffi),
    ("arc_set_drop_i64", Capability::Ffi),
    ("struct_release_shared", Capability::Ffi),
    ("struct_retain", Capability::Ffi),
    // fs:write
    ("buf_write_to_file", Capability::FsWrite),
    ("builtin_create_dir", Capability::FsWrite),
    ("builtin_file_append", Capability::FsWrite),
    ("builtin_file_write", Capability::FsWrite),
    ("dir_create", Capability::FsWrite),
    ("file_remove", Capability::FsWrite),
    ("file_write", Capability::FsWrite),
    ("fs_delete", Capability::FsWrite),
    ("fs_rename", Capability::FsWrite),
    ("fs_write", Capability::FsWrite),
    // process
    ("builtin_env_get", Capability::Process),
    ("env_cwd", Capability::Process),
    ("env_get", Capability::Process),
    ("env_home", Capability::Process),
    ("env_set", Capability::Process),
    ("env_unset", Capability::Process),
    ("process_exec", Capability::Process),
    ("process_exec_simple", Capability::Process),
    // fs:read
    ("builtin_file_exists", Capability::FsRead),
    ("builtin_file_read", Capability::FsRead),
    ("builtin_file_size", Capability::FsRead),
    ("dir_list", Capability::FsRead),
    ("dir_walk", Capability::FsRead),
    ("file_read", Capability::FsRead),
    ("fs_exists", Capability::FsRead),
    ("fs_read", Capability::FsRead),
    ("path_absolute", Capability::FsRead),
    ("path_exists", Capability::FsRead),
    ("path_is_dir", Capability::FsRead),
    ("path_is_file", Capability::FsRead),
    // net:http
    ("builtin_http_get", Capability::NetHttp),
    ("fetch_text", Capability::NetHttp),
    ("fetch_text_ks", Capability::NetHttp),
    ("http2_get_ks", Capability::NetHttp),
    ("http2_post_ks", Capability::NetHttp),
    ("http2_request_ks", Capability::NetHttp),
    ("http_fetch", Capability::NetHttp),
    ("http_request_ks", Capability::NetHttp),
    ("https_get_ks", Capability::NetHttp),
    // db (sqlite)
    ("db_bind_int", Capability::Db),
    ("db_bind_null", Capability::Db),
    ("db_bind_text", Capability::Db),
    ("db_close", Capability::Db),
    ("db_col_count", Capability::Db),
    ("db_col_int", Capability::Db),
    ("db_col_is_null", Capability::Db),
    ("db_col_text_copy", Capability::Db),
    ("db_col_text_len", Capability::Db),
    ("db_exec", Capability::Db),
    ("db_finalize", Capability::Db),
    ("db_open", Capability::Db),
    ("db_open_memory", Capability::Db),
    ("db_prepare", Capability::Db),
    ("db_step", Capability::Db),
    ("db_stmt_execute", Capability::Db),
    ("db_stmt_finalize", Capability::Db),
    ("db_stmt_prepare", Capability::Db),
    ("db_stmt_query", Capability::Db),
    // crypto
    ("ed25519_generate_ks", Capability::Crypto),
    ("ed25519_public_ks", Capability::Crypto),
    ("ed25519_sign_ks", Capability::Crypto),
    ("ed25519_verify_ks", Capability::Crypto),
    ("hmac_sha256_ks", Capability::Crypto),
    ("pbkdf2_sha256_ks", Capability::Crypto),
    ("random_bytes", Capability::Crypto),
    ("random_bytes_ks", Capability::Crypto),
    ("sha1", Capability::Crypto),
    ("sha1_base64_ks", Capability::Crypto),
    ("sha1_hex_ks", Capability::Crypto),
    ("sha256", Capability::Crypto),
    ("sha256_ks", Capability::Crypto),
    ("sha512", Capability::Crypto),
    ("sha512_ks", Capability::Crypto),
    // ffi: dlopen/dlsym/dlcall*, malloc/free, raw read/write, cstr (std::ffi's natives)
    ("ffi_cstr", Capability::Ffi),
    ("ffi_dlcall0", Capability::Ffi),
    ("ffi_dlcall1", Capability::Ffi),
    ("ffi_dlcall2", Capability::Ffi),
    ("ffi_dlcall3", Capability::Ffi),
    ("ffi_dlcall4", Capability::Ffi),
    ("ffi_dlcall5", Capability::Ffi),
    ("ffi_dlcall6", Capability::Ffi),
    ("ffi_dlcall7", Capability::Ffi),
    ("ffi_dlcall8", Capability::Ffi),
    ("ffi_dlcallv0", Capability::Ffi),
    ("ffi_dlcallv1", Capability::Ffi),
    ("ffi_dlcallv2", Capability::Ffi),
    ("ffi_dlcallv3", Capability::Ffi),
    ("ffi_dlcallv4", Capability::Ffi),
    ("ffi_dlcallv5", Capability::Ffi),
    ("ffi_dlcallv6", Capability::Ffi),
    ("ffi_dlcallv7", Capability::Ffi),
    ("ffi_dlcallv_3f32", Capability::Ffi),
    ("ffi_dlcallv_4f", Capability::Ffi),
    ("ffi_dlcallv_4f32", Capability::Ffi),
    ("ffi_dlclose", Capability::Ffi),
    ("ffi_dlopen", Capability::Ffi),
    ("ffi_dlsym", Capability::Ffi),
    ("ffi_free", Capability::Ffi),
    ("ffi_malloc", Capability::Ffi),
    ("ffi_read_f32", Capability::Ffi),
    ("ffi_read_f32_bits", Capability::Ffi),
    ("ffi_read_f64", Capability::Ffi),
    ("ffi_read_f64_bits", Capability::Ffi),
    ("ffi_read_i16", Capability::Ffi),
    ("ffi_read_i32", Capability::Ffi),
    ("ffi_read_i64", Capability::Ffi),
    ("ffi_read_i8", Capability::Ffi),
    ("ffi_string_from_ptr", Capability::Ffi),
    ("ffi_strlen", Capability::Ffi),
    ("ffi_write_f32", Capability::Ffi),
    ("ffi_write_f32_bits", Capability::Ffi),
    ("ffi_write_f64", Capability::Ffi),
    ("ffi_write_f64_bits", Capability::Ffi),
    ("ffi_write_i16", Capability::Ffi),
    ("ffi_write_i32", Capability::Ffi),
    ("ffi_write_i64", Capability::Ffi),
    ("ffi_write_i8", Capability::Ffi),
    // fs: file_open is mode dependent (0 read, 1 write+truncate, 2 append); the table says the strictest, a literal mode 0 at the call site narrows it to fs:read
    ("file_open", Capability::FsWrite),
    // net (postgres wire, websocket framing: coarse net, as the builtin table has them)
    ("pg_close", Capability::Net),
    ("pg_close_ks", Capability::Net),
    ("pg_connect", Capability::Net),
    ("pg_connect_ks", Capability::Net),
    ("pg_exec", Capability::Net),
    ("pg_exec_ks", Capability::Net),
    ("pg_query", Capability::Net),
    ("pg_query_ks", Capability::Net),
    ("ws_accept_key_ks", Capability::Net),
    ("ws_decode_frame_ks", Capability::Net),
    ("ws_encode_binary_ks", Capability::Net),
    ("ws_encode_close", Capability::Net),
    ("ws_encode_ping_ks", Capability::Net),
    ("ws_encode_pong_ks", Capability::Net),
    ("ws_encode_text_ks", Capability::Net),
    ("ws_read_frame_ks", Capability::Net),
    ("ws_unmask_ks", Capability::Net),
    // net:tcp
    ("poll_readable", Capability::NetTcp),
    ("socket_close", Capability::NetTcp),
    ("socket_close_ks", Capability::NetTcp),
    ("tcp_accept", Capability::NetTcp),
    ("tcp_bind", Capability::NetTcp),
    ("tcp_bind_ks", Capability::NetTcp),
    ("tcp_connect", Capability::NetTcp),
    ("tcp_connect_ks", Capability::NetTcp),
    ("tcp_recv", Capability::NetTcp),
    ("tcp_recv_ks", Capability::NetTcp),
    ("tcp_send", Capability::NetTcp),
    ("tcp_send_ks", Capability::NetTcp),
    ("tcp_set_nonblocking", Capability::NetTcp),
    ("tcp_try_accept", Capability::NetTcp),
    ("tcp_try_recv_ks", Capability::NetTcp),
    ("tls_accept", Capability::NetTcp),
    ("tls_close", Capability::NetTcp),
    ("tls_close_ks", Capability::NetTcp),
    ("tls_connect", Capability::NetTcp),
    ("tls_recv", Capability::NetTcp),
    ("tls_recv_ks", Capability::NetTcp),
    ("tls_send", Capability::NetTcp),
    ("tls_send_ks", Capability::NetTcp),
    ("tls_server_config", Capability::NetTcp),
    ("tls_server_config_ks", Capability::NetTcp),
    ("uds_accept", Capability::NetTcp),
    ("uds_bind", Capability::NetTcp),
    ("uds_bind_ks", Capability::NetTcp),
    ("uds_close", Capability::NetTcp),
    ("uds_connect", Capability::NetTcp),
    ("uds_connect_ks", Capability::NetTcp),
    ("uds_recv", Capability::NetTcp),
    ("uds_recv_ks", Capability::NetTcp),
    ("uds_send", Capability::NetTcp),
    ("uds_send_ks", Capability::NetTcp),
    // ffi: raw pointer deref primitives (the natives behind ptr_read_i64 ... ptr_set_byte)
    ("ptr_byte_at", Capability::Ffi),
    ("ptr_read_i64", Capability::Ffi),
    ("ptr_set_byte", Capability::Ffi),
    ("ptr_write_i64", Capability::Ffi),
    // term
    ("term_clear", Capability::Term),
    ("term_cursor_move", Capability::Term),
    ("term_height", Capability::Term),
    ("term_raw_disable", Capability::Term),
    ("term_raw_enable", Capability::Term),
    ("term_width", Capability::Term),
];

/// Natives with no authority. Explicit by name; adding a native here is a
/// review decision (see the module comment).
pub const AMBIENT_NATIVES: &[&str] = &[
    // in-process concurrency (actors, channels, coop executor, futures, spawn, mutex)
    "actor_lock", "actor_lock_i64", "actor_recv", "actor_recv_i64", "actor_recv_timeout_i64",
    "actor_report_exception", "actor_send", "actor_send_i64", "actor_spawn", "actor_spawn_i64",
    "actor_unlock", "actor_unlock_i64", "actor_wait_all", "async_block_on", "async_current_task",
    "async_park_current", "async_run", "async_set_result", "async_spawn", "async_spawn_task",
    "async_take_result", "async_wake", "async_yield_now", "chan_clone", "chan_close",
    "chan_close_i64", "chan_drop", "chan_drop_i64", "chan_is_closed", "chan_is_closed_i64",
    "chan_last_recv_i64", "chan_new", "chan_new_i64", "chan_recv", "chan_recv_i64",
    "chan_recv_timeout_i64", "chan_send", "chan_send_i64", "chan_try_recv",
    "chan_try_recv_status_i64", "closure_lock_acquire", "closure_lock_release", "coop_current",
    "coop_io_begin", "coop_io_end", "coop_order", "coop_record", "coop_reset", "coop_run",
    "coop_spawn", "coop_yield", "mutex_drop", "mutex_lock", "mutex_new", "mutex_unlock", "spawn",
    "spawn_wait_all",
    // wasm host imports declared by std::wasm (implemented by the JS host, pure plumbing)
    "alert", "array_length", "array_pop", "canvas_clear", "canvas_fill_rect", "dom_get_value",
    "dom_set_text", "json_get_int", "json_get_str", "print_f64", "print_i64", "regex_replace",
    "regex_test", "string_index_of", "string_length", "string_parse_float", "string_parse_int",
    // pure compute: json, regex, tensor, uuid, rand (PRNG), path string ops, dom/canvas/alert plumbing
    "alert_ks", "canvas_clear_ks", "canvas_fill_rect_ks", "dom_get_value_ks", "dom_set_text_ks",
    "json_array", "json_bool", "json_get", "json_get_index", "json_is_null", "json_length",
    "json_null", "json_number", "json_object", "json_parse", "json_string", "json_stringify",
    "json_to_float", "json_to_int", "json_to_str", "json_type", "path_basename", "path_dirname",
    "path_extension", "path_free", "path_join", "rand_bool", "rand_bytes", "rand_f64", "rand_i64",
    "rand_seed", "regex_capture", "regex_capture_at", "regex_capture_count", "regex_drop",
    "regex_drop_ks", "regex_find", "regex_find_at", "regex_find_end_ks", "regex_find_ks",
    "regex_find_pos_ks", "regex_is_match", "regex_is_match_ks", "regex_new", "regex_new_ks",
    "regex_replace_all", "regex_replace_all_ks", "tensor_add", "tensor_arange", "tensor_argmax",
    "tensor_argmin", "tensor_cross_entropy", "tensor_div", "tensor_exp", "tensor_eye",
    "tensor_flatten", "tensor_free", "tensor_from_data", "tensor_get", "tensor_log",
    "tensor_matmul", "tensor_max", "tensor_mean", "tensor_min", "tensor_mse_loss", "tensor_mul",
    "tensor_ndim", "tensor_neg", "tensor_numel", "tensor_ones", "tensor_pow", "tensor_rand",
    "tensor_randn", "tensor_relu", "tensor_reshape", "tensor_scale", "tensor_set",
    "tensor_shape_dim", "tensor_sigmoid", "tensor_softmax", "tensor_sqrt", "tensor_sub",
    "tensor_sum", "tensor_tanh", "tensor_to_string", "tensor_transpose", "tensor_zeros",
    "uuid_format", "uuid_parse", "uuid_v4_bytes",
    // containers: arrays, tuples, maps (compiler-managed value handles; bounds checked)
    "array_clone", "array_clone_deep", "array_concat", "array_contains", "array_contains_f64",
    "array_contains_str", "array_dup", "array_free", "array_free_chain", "array_free_elems",
    "array_free_tuples", "array_free_typed", "array_get", "array_len", "array_new",
    "array_null_panic", "array_oob_panic", "array_push", "array_release_if_ne", "array_retain",
    "array_retain_opt", "array_set", "array_typed_release_if_ne", "map_clone", "map_delete",
    "map_delete_str", "map_free", "map_free_tuple_values", "map_free_typed", "map_free_with",
    "map_get", "map_get_or_empty", "map_get_str", "map_get_str_or_empty", "map_has", "map_has_str",
    "map_insert", "map_insert_str", "map_keys", "map_keys_str", "map_len", "map_new",
    "map_release_if_ne", "map_retain", "map_retain_opt", "map_snapshot", "tuple_box_release_if_ne",
    "tuple_handle_release_if_ne", "tuple_release",
    // buffers (buf_*), base64 and hex codecs
    "b64url_to_hex_ks", "base64_decode", "base64_decode_ks", "base64_encode", "base64_encode_ks",
    "buf_contents_to_str", "buf_free", "buf_get_byte", "buf_len", "buf_new", "buf_patch_i32_le",
    "buf_patch_i64_le", "buf_set_byte", "buf_to_str", "buf_write_byte", "buf_write_bytes",
    "buf_write_i16_le", "buf_write_i32_le", "buf_write_i64_le", "buf_write_str", "buf_write_zeros",
    "hex_to_b64url_ks",
    // strings and string builders (compiler-managed value handles)
    "bool_to_string", "byte_at_ks", "chr_ks", "f64_to_string", "handle_to_str", "i64_to_string",
    "str_concat", "str_contains", "str_ends_with", "str_free", "str_len", "str_parse_f64",
    "str_parse_i64", "str_repeat", "str_replace", "str_starts_with", "str_to_lower",
    "str_to_upper", "str_trim", "string_char_at", "string_clone", "string_compare",
    "string_concat", "string_contains", "string_ends_with", "string_eq", "string_find",
    "string_free", "string_hash", "string_len", "string_new", "string_release_if_ne",
    "string_replace", "string_retain", "string_retain_opt", "string_slice", "string_starts_with",
    "string_to_lower", "string_to_upper", "string_trim", "u64_to_string",
    // ARC allocators: hand out a fresh compiler-managed refcounted block, the same kind every
    // str / array / struct creation allocates. Pinned ambient by tests/inferred_soundness.sh
    // (native_extern_pure_alloc); the natives that act on a caller-supplied address stay ffi.
    "arc_alloc", "arc_alloc_i64",
    // closures, dyn and struct plumbing, globals (compiler ABI)
    "box_or_zero", "dyn_drop", "field_set", "fn_release_if_ne", "fn_retain", "global_get",
    "global_has", "global_set",
    // @budget accounting (self-scoped counters)
    "budget_active", "budget_charge_tokens", "budget_charge_usd_micros", "budget_pop_to",
    "budget_push", "budget_push_usd", "budget_remaining_calls", "budget_remaining_tokens",
    "budget_remaining_usd_micros", "budget_try_call",
    // math, numeric and float helpers
    "builtin_abs", "builtin_abs_f", "builtin_ceil", "builtin_cos", "builtin_float",
    "builtin_float_from_float", "builtin_floor", "builtin_int", "builtin_int_from_float",
    "builtin_log", "builtin_log10", "builtin_log2", "builtin_max", "builtin_max_f", "builtin_min",
    "builtin_min_f", "builtin_pow", "builtin_sin", "builtin_sqrt", "builtin_tan",
    "check_div_zero_f64", "check_div_zero_i64", "check_sdiv_i64", "checked_add_i64",
    "checked_mul_i64", "checked_sub_i64", "f64_from_bits", "f64_to_bits", "fmod", "fpow", "ipow",
    "math_abs_f64", "math_abs_i64", "math_ceil", "math_clamp_f64", "math_cos", "math_e",
    "math_floor", "math_log", "math_log10", "math_log2", "math_max_f64", "math_max_i64",
    "math_min_f64", "math_min_i64", "math_pi", "math_pow", "math_round", "math_sin", "math_sqrt",
    "math_tan", "saturating_add_i64", "saturating_mul_i64", "saturating_sub_i64",
    "wrapping_add_i64", "wrapping_mul_i64", "wrapping_sub_i64",
    // process: exit and own argv (args() is ambient)
    "builtin_args", "builtin_exit", "env_args_count", "env_platform", "process_argc",
    "process_argv", "process_exit",
    // exceptions, panics, tracing, diagnostics, runtime init
    "builtin_assert", "builtin_assert_eq", "builtin_panic", "dbg_line", "diag_site",
    "exception_check", "exception_report_thread_fatal_if_pending",
    "exception_report_thread_if_pending", "exception_report_uncaught_if_pending", "exception_take",
    "exception_throw", "panic", "panic_with_location", "rt_init", "trace_enter", "trace_exit",
    // pure string / array / sort builtins (the natives behind len, split, join, sort, ...)
    "builtin_char_code", "builtin_char_from", "builtin_contains", "builtin_ends_with",
    "builtin_index_of", "builtin_join", "builtin_len", "builtin_parse_float", "builtin_parse_int",
    "builtin_pop", "builtin_push", "builtin_replace", "builtin_reverse", "builtin_sort",
    "builtin_sort_f64", "builtin_sort_str", "builtin_split", "builtin_starts_with",
    "builtin_substr", "builtin_to_lower", "builtin_to_string", "builtin_to_upper", "builtin_trim",
    "builtin_trim_end", "builtin_trim_start", "builtin_type_of",
    // stdout / stderr / stdin / file handle close: the process's own streams
    "builtin_read_line", "eprintln_str", "file_close", "print_int", "print_str", "println_int",
    "println_str", "stderr_write", "stdin_read", "stdout_write",
    // time and sleep (ambient: self-scoped, see required_capability_for_builtin)
    "builtin_time_now", "sleep", "sleep_ms", "time_day_utc", "time_format_rfc3339_utc",
    "time_from_ymdhms_utc", "time_hour_utc", "time_minute_utc", "time_month_utc",
    "time_now_millis", "time_now_nanos", "time_now_secs", "time_second_utc", "time_sleep_millis",
    "time_weekday_utc", "time_year_utc",
];

fn table() -> &'static HashMap<&'static str, NativeClass> {
    static TABLE: OnceLock<HashMap<&'static str, NativeClass>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut m = HashMap::with_capacity(CAPABILITY_NATIVES.len() + AMBIENT_NATIVES.len());
        for (name, cap) in CAPABILITY_NATIVES {
            let prev = m.insert(*name, NativeClass::Requires(*cap));
            assert!(prev.is_none(), "native `{name}` is classified twice");
        }
        for name in AMBIENT_NATIVES {
            let prev = m.insert(*name, NativeClass::Ambient);
            assert!(prev.is_none(), "native `{name}` is classified twice");
        }
        m
    })
}

/// The explicit classification of a native symbol (without its `kryos_`
/// prefix), or `None` if nobody classified it. `None` is not "ambient": the
/// caller must fail closed (see `required_capability_for_native_symbol`).
pub fn classify_native_symbol(sym: &str) -> Option<NativeClass> {
    table().get(sym).copied()
}

/// Every classified symbol, for tests and tooling.
pub fn classified_native_symbols() -> impl Iterator<Item = &'static str> {
    CAPABILITY_NATIVES
        .iter()
        .map(|(n, _)| *n)
        .chain(AMBIENT_NATIVES.iter().copied())
}
