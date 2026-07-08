//! Built-in runtime functions for compiled Kryos programs.
//!
//! These are the implementations behind Kryos built-in functions like
//! `to_string()`, `len()`, and the `**` power operator.
//!
//! # Unsafe invariants (file-wide)
//!
//! Every `extern "C"` entry point in this file follows the Kryos opaque-handle
//! ABI documented in `docs/17-unsafe-audit.md` (pattern 1, "FFI Handle
//! Reconstruction").
//!
//! * Inputs typed `i64` are either a raw integer value (when the Kryos type is
//!   `i64`, `bool`, etc.) or an opaque handle to a heap object
//!   (`*const KryosString`, `*const KryosArray`, `*const MapHeader`, ...).
//!   The generated code from `kryos-codegen-*` is responsible for emitting the
//!   correct handle type per the Kryos type checker.
//! * `0` is the universal sentinel for "null handle"; every entry point that
//!   dereferences a handle must check `handle != 0` first.
//! * Returned `i64` handles convey one logical strong refcount to the caller,
//!   to be released via the appropriate `kryos_*_release` function.
//!
//! Inner `unsafe { ... }` blocks in this file rely on one of:
//!   - **Pattern 1**: cast `i64` -> `*const KryosXxx` + deref (validated by
//!     `!= 0` check above and by the Kryos type system upstream).
//!   - **Pattern 2**: `slice::from_raw_parts(data, len)` on a `(data, len)`
//!     pair freshly loaded from a validated `KryosString` / `KryosArray`,
//!     followed by `str::from_utf8(...).unwrap_or("")`.
//!   - Call into another `unsafe extern "C"` runtime function whose own
//!     contract is documented in its home module (`string.rs`, `array.rs`,
//!     `map.rs`, `tensor.rs`).
//!
//! Reviewers: when adding a new `unsafe` block here, either fit one of the
//! patterns above or extend `docs/17-unsafe-audit.md`.

use crate::string::kryos_string_new;

/// Safely convert raw bytes to a &str. Returns empty string on invalid UTF-8.
unsafe fn bytes_to_str<'a>(ptr: *const u8, len: usize) -> &'a str {
    let slice = std::slice::from_raw_parts(ptr, len);
    std::str::from_utf8(slice).unwrap_or("")
}

/// Generic `len()` for any Kryos collection.
///
/// Works because KryosString, KryosArray, and MapHeader all have `len: i64`
/// as their first field (at offset 0). The argument is an opaque handle (pointer as i64).
#[no_mangle]
pub extern "C" fn kryos_builtin_len(collection: i64) -> i64 {
    crate::fault::hang_tick();
    if collection == 0 {
        return 0;
    }
    unsafe { *(collection as *const i64) }
}

/// Generic `to_string()` — converts an i64 value to a KryosString.
/// This is the default; callers that know the type should use the
/// specific variants (kryos_f64_to_string, kryos_bool_to_string).
#[no_mangle]
pub extern "C" fn kryos_builtin_to_string(value: i64) -> i64 {
    kryos_i64_to_string(value)
}

/// Integer exponentiation: `base ** exp`.
/// Returns 1 for exp == 0, handles negative exponents as 0 (integer truncation).
#[no_mangle]
pub extern "C" fn kryos_ipow(base: i64, exp: i64) -> i64 {
    if exp < 0 {
        // Integer division: base^(-n) rounds to 0 for |base| > 1.
        return if base == 1 {
            1
        } else if base == -1 {
            if exp % 2 == 0 {
                1
            } else {
                -1
            }
        } else {
            0
        };
    }
    let mut result: i64 = 1;
    let mut b = base;
    let mut e = exp as u64;
    while e > 0 {
        if e & 1 == 1 {
            result = result.wrapping_mul(b);
        }
        b = b.wrapping_mul(b);
        e >>= 1;
    }
    result
}

/// Float power: base ** exp.
#[no_mangle]
pub extern "C" fn kryos_fpow(base: f64, exp: f64) -> f64 {
    base.powf(exp)
}

/// Float modulo: lhs % rhs.
#[no_mangle]
pub extern "C" fn kryos_fmod(lhs: f64, rhs: f64) -> f64 {
    lhs % rhs
}

/// `kryos_field_set(obj, field_name, value)` -- runtime field store by name.
///
/// Emitted by the self-host (stage-1) lower pass for `obj.field = value`.
/// The stage-0 (Rust) lower compiles field stores directly to memory writes
/// at the known field offset, so this entry point never gets called by
/// stage-0-built binaries; it exists solely so multi-.obj stage-2 link
/// resolves the symbol.
///
/// Real implementation requires runtime type info (per-struct field layout
/// keyed by name). That metadata does not exist yet at the ABI boundary, so
/// for now this is a no-op stub. A stage-2 binary that calls this will not
/// store the field but will not crash either. Replacing the stub with a
/// proper RTTI-driven implementation is the natural next step.
#[no_mangle]
pub extern "C" fn kryos_field_set(_obj: i64, _field_name: i64, _value: i64) -> i64 {
    0
}

/// Convert an i64 to a KryosString. Returns an opaque handle (pointer as i64).
#[no_mangle]
pub extern "C" fn kryos_i64_to_string(value: i64) -> i64 {
    crate::fault::hang_tick();
    let s = value.to_string();
    let bytes = s.as_bytes();
    unsafe { kryos_string_new(bytes.as_ptr(), bytes.len() as i64) as i64 }
}

/// Convert an unsigned 64-bit value (stored in an i64 slot) to a KryosString.
/// The codegen routes to_string of a u64 here so that values above i64::MAX
/// print as their true unsigned decimal (e.g. u64::MAX -> 18446744073709551615)
/// instead of the signed reinterpretation (-1). Narrow unsigned types
/// (u8/u16/u32) are zero-extended before the call, so this is also correct for
/// them.
#[no_mangle]
pub extern "C" fn kryos_u64_to_string(value: i64) -> i64 {
    crate::fault::hang_tick();
    let s = (value as u64).to_string();
    let bytes = s.as_bytes();
    unsafe { kryos_string_new(bytes.as_ptr(), bytes.len() as i64) as i64 }
}

/// Convert an f64 to a KryosString. Returns an opaque handle (pointer as i64).
#[no_mangle]
pub extern "C" fn kryos_f64_to_string(value: f64) -> i64 {
    let s = value.to_string();
    let bytes = s.as_bytes();
    unsafe { kryos_string_new(bytes.as_ptr(), bytes.len() as i64) as i64 }
}

/// Convert a bool (i64: 0=false, nonzero=true) to a KryosString.
#[no_mangle]
pub extern "C" fn kryos_bool_to_string(value: i64) -> i64 {
    let s = if value != 0 { "true" } else { "false" };
    let bytes = s.as_bytes();
    unsafe { kryos_string_new(bytes.as_ptr(), bytes.len() as i64) as i64 }
}

// ---------------------------------------------------------------------------
// Print functions — handle both KryosString pointers and integers
// ---------------------------------------------------------------------------

/// Print a KryosString followed by a newline.
/// Takes the KryosString pointer as i64.
#[no_mangle]
pub extern "C" fn kryos_println_str(handle: i64) {
    let body = kryos_str_text(handle).unwrap_or_default();
    let out = format!("{body}\n");
    // Under the debugger, route to the output sink so the program's stdout
    // does not corrupt the DAP protocol stream framed on the real stdout.
    if crate::debug::sink_write(&out) {
        return;
    }
    print!("{out}");
}

/// Print a KryosString without a newline.
#[no_mangle]
pub extern "C" fn kryos_print_str(handle: i64) {
    let Some(text) = kryos_str_text(handle) else {
        return;
    };
    if text.is_empty() {
        return;
    }
    if crate::debug::sink_write(&text) {
        return;
    }
    print!("{text}");
    // No newline means the line-buffered stdout may never flush this —
    // and neither exit path (JIT trampoline return, AOT exit()) runs the
    // Rust stdout destructor, so unflushed output would be silently lost.
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

/// Read a `KryosString` handle into an owned `String`. Returns `None` for a
/// null handle; an empty string for an empty/null buffer; `"<invalid utf-8>"`
/// for non-UTF-8 content (matching the historical print behaviour).
fn kryos_str_text(handle: i64) -> Option<String> {
    if handle == 0 {
        return None;
    }
    let s = handle as *const crate::string::KryosString;
    unsafe {
        let len = (*s).len as usize;
        let data = (*s).data;
        if data.is_null() || len == 0 {
            return Some(String::new());
        }
        let slice = std::slice::from_raw_parts(data, len);
        match std::str::from_utf8(slice) {
            Ok(text) => Some(text.to_string()),
            Err(_) => Some("<invalid utf-8>".to_string()),
        }
    }
}

/// Print a KryosString to stderr followed by a newline.
#[no_mangle]
pub extern "C" fn kryos_eprintln_str(handle: i64) {
    if handle == 0 {
        eprintln!();
        return;
    }
    let s = handle as *const crate::string::KryosString;
    unsafe {
        let len = (*s).len as usize;
        let data = (*s).data;
        if !data.is_null() && len > 0 {
            let slice = std::slice::from_raw_parts(data, len);
            if let Ok(text) = std::str::from_utf8(slice) {
                eprintln!("{}", text);
            }
        }
    }
}

/// Print an i64 value followed by a newline.
#[no_mangle]
pub extern "C" fn kryos_println_int(value: i64) {
    let out = format!("{value}\n");
    if crate::debug::sink_write(&out) {
        return;
    }
    print!("{out}");
}

/// Print an i64 value without a newline.
#[no_mangle]
pub extern "C" fn kryos_print_int(value: i64) {
    let out = value.to_string();
    if crate::debug::sink_write(&out) {
        return;
    }
    print!("{out}");
    // See kryos_print_str: unflushed no-newline output is lost at exit.
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

// ---------------------------------------------------------------------------
// Channel wrappers — i64-based API for codegen simplicity
// ---------------------------------------------------------------------------

/// Create a new channel for i64-sized elements.
/// Returns the channel handle as i64 (pointer).
#[no_mangle]
pub extern "C" fn kryos_chan_new_i64() -> i64 {
    crate::channel::kryos_chan_new(8) as i64
}

/// Send an i64 value through a channel.
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_chan_send_i64(handle: i64, value: i64) -> i64 {
    let result =
        crate::channel::kryos_chan_send(handle as *mut u8, &value as *const i64 as *const u8, 8);
    result as i64
}

/// Receive an i64 value from a channel (blocking).
/// Returns the received value, or 0 if the channel is closed.
#[no_mangle]
pub extern "C" fn kryos_chan_recv_i64(handle: i64) -> i64 {
    let mut buf: i64 = 0;
    crate::channel::kryos_chan_recv(handle as *mut u8, &mut buf as *mut i64 as *mut u8, 8);
    buf
}

/// Close a channel.
#[no_mangle]
pub extern "C" fn kryos_chan_close_i64(handle: i64) {
    crate::channel::kryos_chan_close(handle as *mut u8);
}

/// Drop a channel handle (decrement ref count).
#[no_mangle]
pub extern "C" fn kryos_chan_drop_i64(handle: i64) {
    crate::channel::kryos_chan_drop(handle as *mut u8);
}

/// Non-blocking receive — status only.
/// Returns: 1 = data received (retrieve with `kryos_chan_last_recv_i64`),
///          0 = no data available, -1 = channel closed or error.
/// The received value is stored in thread-local storage.
#[no_mangle]
pub extern "C" fn kryos_chan_try_recv_status_i64(handle: i64) -> i64 {
    let mut buf: i64 = 0;
    let result =
        crate::channel::kryos_chan_try_recv(handle as *mut u8, &mut buf as *mut i64 as *mut u8, 8);
    if result > 0 {
        LAST_RECV_I64.with(|cell| cell.set(buf));
        1
    } else if result == 0 {
        0
    } else {
        -1
    }
}

/// Retrieve the value from the last successful `kryos_chan_try_recv_status_i64` call.
/// Must be called on the same thread, immediately after a status == 1 return.
#[no_mangle]
pub extern "C" fn kryos_chan_last_recv_i64() -> i64 {
    LAST_RECV_I64.with(|cell| cell.get())
}

/// Check if a channel is closed.
/// Returns 1 if closed, 0 if open.
#[no_mangle]
pub extern "C" fn kryos_chan_is_closed_i64(handle: i64) -> i64 {
    crate::channel::kryos_chan_is_closed(handle as *mut u8) as i64
}

std::thread_local! {
    static LAST_RECV_I64: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
}

// ---------------------------------------------------------------------------
// Actor runtime wrappers — i64-based API for codegen simplicity
// ---------------------------------------------------------------------------

/// Spawn an actor. `dispatch_fn_ptr` is the actor's message dispatch loop
/// function (cast to i64). `state_ptr` is the actor's initial state (i64 handle).
/// Returns actor ID (always > 0).
#[no_mangle]
pub extern "C" fn kryos_actor_spawn_i64(dispatch_fn_ptr: i64, state_ptr: i64) -> i64 {
    let fn_ptr: extern "C" fn(*mut u8) = unsafe { std::mem::transmute(dispatch_fn_ptr as usize) };
    crate::actor::kryos_actor_spawn(fn_ptr, state_ptr as *mut u8) as i64
}

/// Send a single i64 message to an actor.
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_actor_send_i64(actor_id: i64, message: i64) -> i64 {
    crate::actor::kryos_actor_send(actor_id as u64, &message as *const i64 as *const u8, 8) as i64
}

/// Receive a single i64 message from the current actor's mailbox. Blocks.
/// Returns the message value. Returns 0 if mailbox closed.
#[no_mangle]
pub extern "C" fn kryos_actor_recv_i64() -> i64 {
    let mut buf: i64 = 0;
    let result = crate::actor::kryos_actor_recv(&mut buf as *mut i64 as *mut u8, 8);
    if result > 0 {
        buf
    } else {
        0
    }
}

/// Acquire the send lock for an actor (prevents message interleaving).
#[no_mangle]
pub extern "C" fn kryos_actor_lock_i64(actor_id: i64) -> i64 {
    crate::actor::kryos_actor_lock(actor_id as u64) as i64
}

/// Release the send lock for an actor.
#[no_mangle]
pub extern "C" fn kryos_actor_unlock_i64(actor_id: i64) -> i64 {
    crate::actor::kryos_actor_unlock(actor_id as u64) as i64
}

// ---------------------------------------------------------------------------
// Ergonomic builtins — high-level operations on KryosString handles
// ---------------------------------------------------------------------------

/// Read an entire file to a KryosString.
/// `path_handle` is a KryosString handle containing the file path.
/// Returns a KryosString handle with the file contents, or an empty string on error.
#[no_mangle]
pub extern "C" fn kryos_builtin_file_read(path_handle: i64) -> i64 {
    let contents = if path_handle == 0 {
        String::new()
    } else {
        let ks = path_handle as *const crate::string::KryosString;
        let path_str = unsafe {
            let len = (*ks).len as usize;
            let data = (*ks).data;
            if data.is_null() || len == 0 {
                ""
            } else {
                let slice = std::slice::from_raw_parts(data, len);
                std::str::from_utf8(slice).unwrap_or("")
            }
        };
        match std::fs::read_to_string(path_str) {
            Ok(c) => c,
            Err(e) => {
                // A silent "" made a missing file indistinguishable from an
                // empty one. Fail loudly like the other checked builtins
                // (substr OOB, div by zero); the recoverable path is
                // std::fs::read_file (throws) or a file_exists() probe.
                let msg = format!("file_read: {path_str}: {e}");
                crate::panic::kryos_panic(msg.as_ptr(), msg.len())
            }
        }
    };
    let bytes = contents.as_bytes();
    unsafe { kryos_string_new(bytes.as_ptr(), bytes.len() as i64) as i64 }
}

/// Write a string to a file.
/// `path_handle` is a KryosString handle containing the file path.
/// `content_handle` is a KryosString handle containing the data to write.
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_builtin_file_write(path_handle: i64, content_handle: i64) -> i64 {
    if path_handle == 0 {
        return -1;
    }
    let ks_path = path_handle as *const crate::string::KryosString;
    let path_str = unsafe {
        let len = (*ks_path).len as usize;
        let data = (*ks_path).data;
        if data.is_null() || len == 0 {
            return -1;
        }
        let slice = std::slice::from_raw_parts(data, len);
        match std::str::from_utf8(slice) {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };
    let content = if content_handle == 0 {
        &[] as &[u8]
    } else {
        let ks_content = content_handle as *const crate::string::KryosString;
        unsafe {
            let len = (*ks_content).len as usize;
            let data = (*ks_content).data;
            if data.is_null() || len == 0 {
                &[] as &[u8]
            } else {
                std::slice::from_raw_parts(data, len)
            }
        }
    };
    // Create parent directories so writing to a nested path succeeds.
    // No-op when the parent already exists; mirrors create_dir semantics.
    if let Some(parent) = std::path::Path::new(path_str).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    match std::fs::write(path_str, content) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Get an environment variable as a KryosString.
/// `key_handle` is a KryosString handle containing the variable name.
/// Returns a KryosString handle with the value, or an empty string if not found.
#[no_mangle]
pub extern "C" fn kryos_builtin_env_get(key_handle: i64) -> i64 {
    let value = if key_handle == 0 {
        String::new()
    } else {
        let ks = key_handle as *const crate::string::KryosString;
        let key_str = unsafe {
            let len = (*ks).len as usize;
            let data = (*ks).data;
            if data.is_null() || len == 0 {
                ""
            } else {
                let slice = std::slice::from_raw_parts(data, len);
                std::str::from_utf8(slice).unwrap_or("")
            }
        };
        std::env::var(key_str).unwrap_or_default()
    };
    let bytes = value.as_bytes();
    unsafe { kryos_string_new(bytes.as_ptr(), bytes.len() as i64) as i64 }
}

/// Return the current Unix timestamp in seconds.
#[no_mangle]
pub extern "C" fn kryos_builtin_time_now() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(_) => -1,
    }
}

/// Abort the process with a user-supplied message. `msg_handle` is a
/// KryosString handle. Prints "panic: <msg>" to stderr (or records it
/// as a test failure when running under the test harness) and exits.
/// Returns i64 only so codegen can treat it as a normal extern call;
/// the call never actually returns.
#[no_mangle]
pub extern "C" fn kryos_builtin_panic(msg_handle: i64) -> i64 {
    let msg = if msg_handle == 0 {
        "<no message>".to_string()
    } else {
        let ks = msg_handle as *const crate::string::KryosString;
        unsafe {
            let len = (*ks).len as usize;
            let data = (*ks).data;
            if data.is_null() || len == 0 {
                "<no message>".to_string()
            } else {
                let slice = std::slice::from_raw_parts(data, len);
                std::str::from_utf8(slice)
                    .unwrap_or("<invalid utf-8>")
                    .to_string()
            }
        }
    };
    if crate::is_test_mode() {
        crate::set_test_failure(format!("panic: {}", msg));
        return 0;
    }
    eprintln!("panic: {}", msg);
    std::process::exit(101);
}

/// Assert that two values are equal by comparing their stringified forms.
/// Both args are KryosString handles produced at the call site via the
/// type-aware `to_string` codegen path (so ints, bools, floats, and
/// strings all print correctly). If the contents differ, prints a
/// multi-line diff to stderr and aborts (or records a test failure when
/// running under the test harness). Returns 0 (ignored).
#[no_mangle]
pub extern "C" fn kryos_builtin_assert_eq(a_handle: i64, b_handle: i64) -> i64 {
    fn read_str(handle: i64) -> String {
        if handle == 0 {
            return String::new();
        }
        let ks = handle as *const crate::string::KryosString;
        unsafe {
            let len = (*ks).len as usize;
            let data = (*ks).data;
            if data.is_null() || len == 0 {
                String::new()
            } else {
                let slice = std::slice::from_raw_parts(data, len);
                std::str::from_utf8(slice)
                    .unwrap_or("<invalid utf-8>")
                    .to_string()
            }
        }
    }
    let a = read_str(a_handle);
    let b = read_str(b_handle);
    if a == b {
        return 0;
    }
    let msg = format!("assertion failed: left != right\n  left:  {}\n  right: {}", a, b);
    if crate::is_test_mode() {
        crate::set_test_failure(msg);
        return 0;
    }
    eprintln!("{}", msg);
    std::process::abort();
}

/// Assert that `condition` is non-zero (truthy). If it is zero, print
/// "assertion failed: <msg>" to stderr and abort the process.
/// `msg_handle` is a KryosString handle. Returns 0 (ignored).
#[no_mangle]
pub extern "C" fn kryos_builtin_assert(condition: i64, msg_handle: i64) -> i64 {
    if condition != 0 {
        return 0;
    }
    let msg = if msg_handle == 0 {
        "<no message>".to_string()
    } else {
        let ks = msg_handle as *const crate::string::KryosString;
        unsafe {
            let len = (*ks).len as usize;
            let data = (*ks).data;
            if data.is_null() || len == 0 {
                "<no message>".to_string()
            } else {
                let slice = std::slice::from_raw_parts(data, len);
                std::str::from_utf8(slice)
                    .unwrap_or("<invalid utf-8>")
                    .to_string()
            }
        }
    };
    if crate::is_test_mode() {
        crate::set_test_failure(format!("assertion failed: {}", msg));
        return 0;
    }
    eprintln!("assertion failed: {}", msg);
    std::process::abort();
}

/// Parse a KryosString as an integer. PANICS on malformed input, matching the
/// documented contract (docs/learn/cookbook: "parse_int panics on non-numeric,
/// pre-check"). It previously returned 0 silently, so code that followed the
/// docs and relied on the panic to reject bad input got a valid-looking 0
/// instead (backlog #41/#71). Pre-check or use std::result for graceful paths.
#[no_mangle]
pub extern "C" fn kryos_builtin_parse_int(s_handle: i64) -> i64 {
    let text = parse_str_arg(s_handle);
    match text.trim().parse::<i64>() {
        Ok(v) => v,
        Err(_) => parse_panic("parse_int", &text),
    }
}

/// Parse a KryosString as an f64, returning the bits as i64. PANICS on
/// malformed input (see `kryos_builtin_parse_int`; backlog #41/#71).
#[no_mangle]
pub extern "C" fn kryos_builtin_parse_float(s_handle: i64) -> i64 {
    let text = parse_str_arg(s_handle);
    match text.trim().parse::<f64>() {
        Ok(v) => v.to_bits() as i64,
        Err(_) => parse_panic("parse_float", &text),
    }
}

/// Extract the UTF-8 text of a KryosString handle (empty string for a null
/// handle / invalid UTF-8, which then fails the numeric parse and panics).
fn parse_str_arg(s_handle: i64) -> String {
    if s_handle == 0 {
        return String::new();
    }
    let ks = s_handle as *const crate::string::KryosString;
    unsafe {
        let len = (*ks).len as usize;
        let data = (*ks).data;
        if data.is_null() || len == 0 {
            return String::new();
        }
        let slice = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(slice).unwrap_or("").to_string()
    }
}

fn parse_panic(func: &str, text: &str) -> ! {
    let msg = format!("{func}: invalid numeric input: '{text}'");
    crate::panic::kryos_panic(msg.as_ptr(), msg.len())
}

/// Return the type name of a value. Since everything is i64 at runtime,
/// this always returns "i64".
#[no_mangle]
pub extern "C" fn kryos_builtin_type_of(_value: i64) -> i64 {
    let s = b"i64";
    unsafe { kryos_string_new(s.as_ptr(), s.len() as i64) as i64 }
}

// ---------------------------------------------------------------------------
// String character builtins — char_code, char_from, substr
// ---------------------------------------------------------------------------

/// `char_code(s)` — Return the Unicode code point of the first character
/// of a KryosString. Returns 0 for an empty or null string.
#[no_mangle]
pub extern "C" fn kryos_builtin_char_code(s_handle: i64) -> i64 {
    if s_handle == 0 {
        return 0;
    }
    let ks = s_handle as *const crate::string::KryosString;
    unsafe {
        let len = (*ks).len as usize;
        let data = (*ks).data;
        if data.is_null() || len == 0 {
            return 0;
        }
        let slice = std::slice::from_raw_parts(data, len);
        match std::str::from_utf8(slice) {
            Ok(s) => s.chars().next().map_or(0, |c| c as i64),
            Err(_) => slice[0] as i64,
        }
    }
}

/// `char_from(n)` — Create a single-character KryosString from a Unicode code
/// point. Returns an empty string if the code point is invalid.
#[no_mangle]
pub extern "C" fn kryos_builtin_char_from(code: i64) -> i64 {
    let ch = if (0..=0x10FFFF).contains(&code) {
        char::from_u32(code as u32)
    } else {
        None
    };
    match ch {
        Some(c) => {
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            let bytes = s.as_bytes();
            unsafe { kryos_string_new(bytes.as_ptr(), bytes.len() as i64) as i64 }
        }
        None => unsafe { kryos_string_new(std::ptr::null(), 0) as i64 },
    }
}

/// `substr(s, start, end)` — Extract a substring [start..end).
/// Delegates to `kryos_string_slice`.
#[no_mangle]
pub extern "C" fn kryos_builtin_substr(s_handle: i64, start: i64, end: i64) -> i64 {
    crate::fault::hang_tick();
    if s_handle == 0 {
        return unsafe { kryos_string_new(std::ptr::null(), 0) as i64 };
    }
    let ks = s_handle as *const crate::string::KryosString;
    unsafe { crate::string::kryos_string_slice(ks, start, end) as i64 }
}

// ---------------------------------------------------------------------------
// String utility builtins — contains, starts_with, ends_with, trim,
// to_upper, to_lower, replace
// ---------------------------------------------------------------------------

/// `contains(haystack, needle)` — Check if haystack contains needle.
/// Returns 1 (true) or 0 (false).
#[no_mangle]
pub extern "C" fn kryos_builtin_contains(haystack: i64, needle: i64) -> i64 {
    if haystack == 0 || needle == 0 {
        return 0;
    }
    let ks_h = haystack as *const crate::string::KryosString;
    let ks_n = needle as *const crate::string::KryosString;
    unsafe { crate::string::kryos_string_contains(ks_h, ks_n) }
}

/// `starts_with(s, prefix)` — Check if s starts with prefix.
/// Returns 1 (true) or 0 (false).
#[no_mangle]
pub extern "C" fn kryos_builtin_starts_with(s: i64, prefix: i64) -> i64 {
    if s == 0 || prefix == 0 {
        return 0;
    }
    let ks_s = s as *const crate::string::KryosString;
    let ks_p = prefix as *const crate::string::KryosString;
    unsafe { crate::string::kryos_string_starts_with(ks_s, ks_p) }
}

/// `ends_with(s, suffix)` — Check if s ends with suffix.
/// Returns 1 (true) or 0 (false).
#[no_mangle]
pub extern "C" fn kryos_builtin_ends_with(s: i64, suffix: i64) -> i64 {
    if s == 0 || suffix == 0 {
        return 0;
    }
    let ks_s = s as *const crate::string::KryosString;
    let ks_sf = suffix as *const crate::string::KryosString;
    unsafe { crate::string::kryos_string_ends_with(ks_s, ks_sf) }
}

/// `trim(s)` — Trim whitespace from both ends, returning a new string.
#[no_mangle]
pub extern "C" fn kryos_builtin_trim(s: i64) -> i64 {
    if s == 0 {
        return unsafe { kryos_string_new(std::ptr::null(), 0) as i64 };
    }
    let ks = s as *const crate::string::KryosString;
    unsafe { crate::string::kryos_string_trim(ks) as i64 }
}

/// `to_upper(s)` — Convert string to uppercase, returning a new string.
#[no_mangle]
pub extern "C" fn kryos_builtin_to_upper(s: i64) -> i64 {
    if s == 0 {
        return unsafe { kryos_string_new(std::ptr::null(), 0) as i64 };
    }
    let ks = s as *const crate::string::KryosString;
    unsafe { crate::string::kryos_string_to_upper(ks) as i64 }
}

/// `to_lower(s)` — Convert string to lowercase, returning a new string.
#[no_mangle]
pub extern "C" fn kryos_builtin_to_lower(s: i64) -> i64 {
    if s == 0 {
        return unsafe { kryos_string_new(std::ptr::null(), 0) as i64 };
    }
    let ks = s as *const crate::string::KryosString;
    unsafe { crate::string::kryos_string_to_lower(ks) as i64 }
}

/// `replace(s, from, to)` — Replace all occurrences of `from` with `to`,
/// returning a new string.
#[no_mangle]
pub extern "C" fn kryos_builtin_replace(s: i64, from: i64, to_str: i64) -> i64 {
    if s == 0 || from == 0 || to_str == 0 {
        return unsafe { kryos_string_new(std::ptr::null(), 0) as i64 };
    }
    let ks_s = s as *const crate::string::KryosString;
    let ks_f = from as *const crate::string::KryosString;
    let ks_t = to_str as *const crate::string::KryosString;
    unsafe { crate::string::kryos_string_replace(ks_s, ks_f, ks_t) as i64 }
}

// ---------------------------------------------------------------------------
// String split / join builtins
// ---------------------------------------------------------------------------

/// `split(s, delimiter)` — Split a string by delimiter, returning a [str] array.
#[no_mangle]
pub extern "C" fn kryos_builtin_split(s: i64, delimiter: i64) -> i64 {
    if s == 0 || delimiter == 0 {
        // Return an empty array.
        return unsafe { crate::array::kryos_array_new(8, 0) as i64 };
    }
    let ks_s = s as *const crate::string::KryosString;
    let ks_d = delimiter as *const crate::string::KryosString;
    unsafe {
        let s_str = bytes_to_str((*ks_s).data, (*ks_s).len as usize);
        let d_str = bytes_to_str((*ks_d).data, (*ks_d).len as usize);
        let parts: Vec<&str> = s_str.split(d_str).collect();
        let arr = crate::array::kryos_array_new(8, parts.len() as i64);
        for part in &parts {
            let ks = crate::string::kryos_string_new(part.as_ptr(), part.len() as i64);
            crate::array::kryos_array_push(arr, ks as i64);
        }
        arr as i64
    }
}

/// `join(arr, separator)` — Join an array of strings with a separator, returning a new string.
#[no_mangle]
pub extern "C" fn kryos_builtin_join(arr_handle: i64, sep: i64) -> i64 {
    if arr_handle == 0 {
        return unsafe { kryos_string_new(std::ptr::null(), 0) as i64 };
    }
    let arr = arr_handle as *const crate::array::KryosArray;
    let sep_str = if sep == 0 {
        ""
    } else {
        let ks_sep = sep as *const crate::string::KryosString;
        unsafe { bytes_to_str((*ks_sep).data, (*ks_sep).len as usize) }
    };
    unsafe {
        let len = crate::array::kryos_array_len(arr);
        let mut parts: Vec<&str> = Vec::new();
        for i in 0..len {
            let elem = crate::array::kryos_array_get(arr, i);
            if elem == 0 {
                parts.push("");
            } else {
                let ks = elem as *const crate::string::KryosString;
                parts.push(bytes_to_str((*ks).data, (*ks).len as usize));
            }
        }
        let joined = parts.join(sep_str);
        kryos_string_new(joined.as_ptr(), joined.len() as i64) as i64
    }
}

// ---------------------------------------------------------------------------
// Math builtins — sin, cos, tan, log, log2, log10
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn kryos_builtin_sin(x: f64) -> f64 {
    x.sin()
}

#[no_mangle]
pub extern "C" fn kryos_builtin_cos(x: f64) -> f64 {
    x.cos()
}

#[no_mangle]
pub extern "C" fn kryos_builtin_tan(x: f64) -> f64 {
    x.tan()
}

#[no_mangle]
pub extern "C" fn kryos_builtin_log(x: f64) -> f64 {
    x.ln()
}

#[no_mangle]
pub extern "C" fn kryos_builtin_log2(x: f64) -> f64 {
    x.log2()
}

#[no_mangle]
pub extern "C" fn kryos_builtin_log10(x: f64) -> f64 {
    x.log10()
}

// ---------------------------------------------------------------------------
// Numeric conversion builtins — int, float
// ---------------------------------------------------------------------------

/// `int(x)` — Identity on integers (everything is i64 at runtime).
/// When called on a float bit-pattern, reinterprets as f64 and truncates.
/// The type checker will dispatch the correct variant based on the source type.
#[no_mangle]
pub extern "C" fn kryos_builtin_int(value: i64) -> i64 {
    value
}

/// `int` for float arguments — truncate f64 to i64.
#[no_mangle]
pub extern "C" fn kryos_builtin_int_from_float(value: f64) -> i64 {
    value as i64
}

/// `float(x)` — Convert an integer to f64, return as bit-pattern i64.
#[no_mangle]
pub extern "C" fn kryos_builtin_float(value: i64) -> i64 {
    (value as f64).to_bits() as i64
}

/// `float` for float arguments — identity (already f64 bits).
#[no_mangle]
pub extern "C" fn kryos_builtin_float_from_float(value: f64) -> i64 {
    value.to_bits() as i64
}

// ---------------------------------------------------------------------------
// Array builtins — push, pop
// ---------------------------------------------------------------------------

/// `push(arr, val)` — Append a value to an array. Returns the array handle.
#[no_mangle]
pub extern "C" fn kryos_builtin_push(arr_handle: i64, val: i64) -> i64 {
    if arr_handle == 0 {
        return arr_handle;
    }
    let arr = arr_handle as *mut crate::array::KryosArray;
    unsafe { crate::array::kryos_array_push(arr, val) };
    arr_handle
}

/// `pop(arr)` — Remove and return the last element of an array.
/// Returns 0 if the array is empty or null.
#[no_mangle]
pub extern "C" fn kryos_builtin_pop(arr_handle: i64) -> i64 {
    if arr_handle == 0 {
        return 0;
    }
    let arr = arr_handle as *mut crate::array::KryosArray;
    unsafe {
        let len = (*arr).len;
        if len <= 0 {
            return 0;
        }
        let last_idx = len - 1;
        let val = crate::array::kryos_array_get(arr, last_idx);
        (*arr).len = last_idx;
        val
    }
}

// ---------------------------------------------------------------------------
// String indexing — get single character as a new KryosString
// ---------------------------------------------------------------------------

/// `kryos_string_char_at(s, idx)` — Return a single-character substring at
/// the given byte index. Used by `s[i]` on strings.
#[no_mangle]
pub extern "C" fn kryos_string_char_at(s_handle: i64, idx: i64) -> i64 {
    kryos_builtin_substr(s_handle, idx, idx + 1)
}

// ---------------------------------------------------------------------------
// ARC runtime wrappers — i64-based API for codegen simplicity
// ---------------------------------------------------------------------------

/// Allocate an ARC-managed object. `size` is the payload size in bytes.
/// Uses default alignment (8 bytes, matching i64). Returns user-data pointer as i64.
#[no_mangle]
pub extern "C" fn kryos_arc_alloc_i64(size: i64) -> i64 {
    crate::arc::kryos_arc_alloc(size as usize, 8) as i64
}

/// Increment the reference count of an ARC-managed object.
/// `ptr` is the user-data pointer (as i64) returned by `kryos_arc_alloc_i64`.
#[no_mangle]
pub extern "C" fn kryos_arc_retain_i64(ptr: i64) {
    crate::arc::kryos_arc_retain(ptr as *mut u8);
}

/// Decrement the reference count of an ARC-managed object.
/// When count reaches zero, calls the drop function and deallocates.
#[no_mangle]
pub extern "C" fn kryos_arc_release_i64(ptr: i64) {
    crate::arc::kryos_arc_release(ptr as *mut u8);
}

/// Set the drop function for an ARC-managed object (i64 wrapper).
/// `drop_fn` is a function pointer cast to i64 with signature `fn(*mut u8)`.
/// Called by codegen after allocating closure env buffers so that captured
/// heap values (strings, arrays, maps, closures) are freed when the closure
/// reference count reaches zero.
#[no_mangle]
pub extern "C" fn kryos_arc_set_drop_i64(ptr: i64, drop_fn: i64) {
    if ptr == 0 || drop_fn == 0 {
        return;
    }
    let fn_ptr: extern "C" fn(*mut u8) = unsafe { core::mem::transmute(drop_fn as usize) };
    crate::arc::kryos_arc_set_drop(ptr as *mut u8, fn_ptr);
}

// ---------------------------------------------------------------------------
// Runtime checks — division by zero
// ---------------------------------------------------------------------------

/// Check for integer division by zero at runtime.
/// Called by codegen before `sdiv` and `srem` instructions.
/// If the divisor is zero, prints a panic message and aborts.
#[no_mangle]
pub extern "C" fn kryos_check_div_zero_i64(divisor: i64) {
    if divisor == 0 {
        let msg = b"integer division by zero";
        crate::panic::kryos_panic(msg.as_ptr(), msg.len());
    }
}

/// Guard for signed integer division/modulo: panics on a zero divisor and on
/// the one overflowing case (`MIN(bits) / -1`), which sdiv/srem cannot
/// represent — without this it raises a hardware exception (silent crash,
/// no diagnostic) on both backends. `bits` is the operand width (8..64);
/// operands arrive sign-extended to i64.
#[no_mangle]
pub extern "C" fn kryos_check_sdiv_i64(dividend: i64, divisor: i64, bits: i64) {
    if divisor == 0 {
        let msg = b"integer division by zero";
        crate::panic::kryos_panic(msg.as_ptr(), msg.len());
    }
    let min = match bits {
        8 => i8::MIN as i64,
        16 => i16::MIN as i64,
        32 => i32::MIN as i64,
        _ => i64::MIN,
    };
    if divisor == -1 && dividend == min {
        let msg = b"integer division overflow";
        crate::panic::kryos_panic(msg.as_ptr(), msg.len());
    }
}

/// Check for float division by zero — intentional no-op.
/// IEEE 754 float division by zero produces inf/nan, which is expected behavior.
#[no_mangle]
pub extern "C" fn kryos_check_div_zero_f64(_divisor: f64) {
    // No-op: float division by zero produces inf/nan per IEEE 754.
}

// ── Overflow-aware integer arithmetic ─────────────────────────────
//
// Kryos default behaviour: 2's-complement wrap on overflow for all
// integer operations (`a + b`, `a * b`, etc). Matches C, Rust release,
// Go, and Java semantics.
//
// These builtins give the programmer explicit control:
//   wrapping_*    : same as default — explicit wrap on overflow
//   checked_*     : panic with a clear message on overflow
//   saturating_*  : clamp to INT64_MIN / INT64_MAX on overflow
//
// All currently operate on i64. Smaller integer types can use these
// via widening + range-check at the call site if needed; richer
// support will land alongside generics over integer width.

#[inline]
fn panic_overflow(op: &str) -> ! {
    let mut buf = [0u8; 64];
    let prefix = b"integer overflow in ";
    let mut n = 0;
    for &b in prefix {
        if n < buf.len() {
            buf[n] = b;
            n += 1;
        }
    }
    for &b in op.as_bytes() {
        if n < buf.len() {
            buf[n] = b;
            n += 1;
        }
    }
    crate::panic::kryos_panic(buf.as_ptr(), n);
}

// --- wrapping_* (explicit wrap; identical to default operators) ---

#[no_mangle]
pub extern "C" fn kryos_wrapping_add_i64(a: i64, b: i64) -> i64 {
    a.wrapping_add(b)
}

#[no_mangle]
pub extern "C" fn kryos_wrapping_sub_i64(a: i64, b: i64) -> i64 {
    a.wrapping_sub(b)
}

#[no_mangle]
pub extern "C" fn kryos_wrapping_mul_i64(a: i64, b: i64) -> i64 {
    a.wrapping_mul(b)
}

// --- checked_* (panic on overflow) ---

#[no_mangle]
pub extern "C" fn kryos_checked_add_i64(a: i64, b: i64) -> i64 {
    a.checked_add(b)
        .unwrap_or_else(|| panic_overflow("checked_add"))
}

#[no_mangle]
pub extern "C" fn kryos_checked_sub_i64(a: i64, b: i64) -> i64 {
    a.checked_sub(b)
        .unwrap_or_else(|| panic_overflow("checked_sub"))
}

#[no_mangle]
pub extern "C" fn kryos_checked_mul_i64(a: i64, b: i64) -> i64 {
    a.checked_mul(b)
        .unwrap_or_else(|| panic_overflow("checked_mul"))
}

// --- saturating_* (clamp on overflow) ---

#[no_mangle]
pub extern "C" fn kryos_saturating_add_i64(a: i64, b: i64) -> i64 {
    a.saturating_add(b)
}

#[no_mangle]
pub extern "C" fn kryos_saturating_sub_i64(a: i64, b: i64) -> i64 {
    a.saturating_sub(b)
}

#[no_mangle]
pub extern "C" fn kryos_saturating_mul_i64(a: i64, b: i64) -> i64 {
    a.saturating_mul(b)
}

// ── Byte buffer for native code emission ──────────────────────────

struct KryosBuf {
    data: Vec<u8>,
}

#[no_mangle]
pub unsafe extern "C" fn kryos_buf_new(capacity: i64) -> i64 {
    let buf = Box::new(KryosBuf {
        data: Vec::with_capacity(capacity.max(0) as usize),
    });
    Box::into_raw(buf) as i64
}

#[no_mangle]
pub unsafe extern "C" fn kryos_buf_write_byte(handle: i64, byte: i64) {
    let buf = &mut *(handle as *mut KryosBuf);
    buf.data.push(byte as u8);
}

#[no_mangle]
pub unsafe extern "C" fn kryos_buf_write_i16_le(handle: i64, val: i64) {
    let buf = &mut *(handle as *mut KryosBuf);
    buf.data.extend_from_slice(&(val as i16).to_le_bytes());
}

#[no_mangle]
pub unsafe extern "C" fn kryos_buf_write_i32_le(handle: i64, val: i64) {
    let buf = &mut *(handle as *mut KryosBuf);
    buf.data.extend_from_slice(&(val as i32).to_le_bytes());
}

#[no_mangle]
pub unsafe extern "C" fn kryos_buf_write_i64_le(handle: i64, val: i64) {
    let buf = &mut *(handle as *mut KryosBuf);
    buf.data.extend_from_slice(&val.to_le_bytes());
}

#[no_mangle]
pub unsafe extern "C" fn kryos_buf_write_bytes(dst_handle: i64, src_handle: i64, len: i64) {
    let src = &*(src_handle as *const KryosBuf);
    let dst = &mut *(dst_handle as *mut KryosBuf);
    let n = (len as usize).min(src.data.len());
    dst.data.extend_from_slice(&src.data[..n]);
}

#[no_mangle]
pub unsafe extern "C" fn kryos_buf_write_str(handle: i64, s_handle: i64) {
    let buf = &mut *(handle as *mut KryosBuf);
    let s = &*(s_handle as *const crate::string::KryosString);
    let slice = std::slice::from_raw_parts(s.data, s.len as usize);
    buf.data.extend_from_slice(slice);
}

#[no_mangle]
pub unsafe extern "C" fn kryos_buf_write_zeros(handle: i64, count: i64) {
    let buf = &mut *(handle as *mut KryosBuf);
    buf.data.resize(buf.data.len() + count.max(0) as usize, 0);
}

#[no_mangle]
pub unsafe extern "C" fn kryos_buf_len(handle: i64) -> i64 {
    let buf = &*(handle as *const KryosBuf);
    buf.data.len() as i64
}

#[no_mangle]
pub unsafe extern "C" fn kryos_buf_get_byte(handle: i64, offset: i64) -> i64 {
    let buf = &*(handle as *const KryosBuf);
    let idx = offset as usize;
    if idx < buf.data.len() {
        buf.data[idx] as i64
    } else {
        -1
    }
}

#[no_mangle]
pub unsafe extern "C" fn kryos_buf_set_byte(handle: i64, offset: i64, byte: i64) {
    let buf = &mut *(handle as *mut KryosBuf);
    let idx = offset as usize;
    if idx < buf.data.len() {
        buf.data[idx] = byte as u8;
    }
}

#[no_mangle]
pub unsafe extern "C" fn kryos_buf_patch_i32_le(handle: i64, offset: i64, val: i64) {
    let buf = &mut *(handle as *mut KryosBuf);
    let idx = offset as usize;
    let bytes = (val as i32).to_le_bytes();
    if idx + 4 <= buf.data.len() {
        buf.data[idx..idx + 4].copy_from_slice(&bytes);
    }
}

#[no_mangle]
pub unsafe extern "C" fn kryos_buf_patch_i64_le(handle: i64, offset: i64, val: i64) {
    let buf = &mut *(handle as *mut KryosBuf);
    let idx = offset as usize;
    let bytes = val.to_le_bytes();
    if idx + 8 <= buf.data.len() {
        buf.data[idx..idx + 8].copy_from_slice(&bytes);
    }
}

#[no_mangle]
pub unsafe extern "C" fn kryos_buf_write_to_file(handle: i64, path_handle: i64) -> i64 {
    let buf = &*(handle as *const KryosBuf);
    let path_str = &*(path_handle as *const crate::string::KryosString);
    let path_slice = std::slice::from_raw_parts(path_str.data, path_str.len as usize);
    let path = match std::str::from_utf8(path_slice) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    match std::fs::write(path, &buf.data) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn kryos_buf_free(handle: i64) {
    if handle != 0 {
        let _ = Box::from_raw(handle as *mut KryosBuf);
    }
}

// ── Process control ───────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn kryos_builtin_exit(code: i64) {
    std::process::exit(code as i32);
}

// ── Command-line arguments ────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn kryos_builtin_args() -> i64 {
    let args: Vec<String> = std::env::args().collect();
    let arr = crate::array::kryos_array_new(8, args.len() as i64);
    for arg in &args {
        let s = crate::string::kryos_string_new(arg.as_ptr(), arg.len() as i64);
        crate::array::kryos_array_push(arr, s as i64);
    }
    arr as i64
}

// ── Stdin read ────────────────────────────────────────────────────

/// Read one line from stdin (blocking). Strips the trailing `\n` (and `\r\n`).
/// Returns a KryosString handle. Returns an empty string on EOF or error.
#[no_mangle]
pub extern "C" fn kryos_builtin_read_line() -> i64 {
    use std::io::BufRead;
    let stdin = std::io::stdin();
    let mut line = String::new();
    match stdin.lock().read_line(&mut line) {
        Ok(0) | Err(_) => {}
        Ok(_) => {
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
        }
    }
    let bytes = line.as_bytes();
    unsafe { kryos_string_new(bytes.as_ptr(), bytes.len() as i64) as i64 }
}

// ── Filesystem helpers ────────────────────────────────────────────

/// Check whether a path exists on disk.
/// `path_handle` is a KryosString handle.
/// Returns 1 if the path exists, 0 otherwise.
#[no_mangle]
pub extern "C" fn kryos_builtin_file_exists(path_handle: i64) -> i64 {
    if path_handle == 0 {
        return 0;
    }
    let ks = path_handle as *const crate::string::KryosString;
    let path_str = unsafe {
        let len = (*ks).len as usize;
        let data = (*ks).data;
        if data.is_null() || len == 0 {
            return 0;
        }
        let slice = std::slice::from_raw_parts(data, len);
        match std::str::from_utf8(slice) {
            Ok(s) => s,
            Err(_) => return 0,
        }
    };
    if std::path::Path::new(path_str).exists() {
        1
    } else {
        0
    }
}

/// Return the size in bytes of a file.
/// `path_handle` is a KryosString handle.
/// Returns the byte count on success, -1 on error or if not a regular file.
#[no_mangle]
pub extern "C" fn kryos_builtin_file_size(path_handle: i64) -> i64 {
    if path_handle == 0 {
        return -1;
    }
    let ks = path_handle as *const crate::string::KryosString;
    let path_str = unsafe {
        let len = (*ks).len as usize;
        let data = (*ks).data;
        if data.is_null() || len == 0 {
            return -1;
        }
        let slice = std::slice::from_raw_parts(data, len);
        match std::str::from_utf8(slice) {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };
    match std::fs::metadata(path_str) {
        Ok(m) => m.len() as i64,
        Err(_) => -1,
    }
}

/// Create a directory (and all parent directories) at `path_handle`.
/// `path_handle` is a KryosString handle.
/// Returns 0 on success (including if the directory already exists), -1 on error.
#[no_mangle]
pub extern "C" fn kryos_builtin_create_dir(path_handle: i64) -> i64 {
    if path_handle == 0 {
        return -1;
    }
    let ks = path_handle as *const crate::string::KryosString;
    let path_str = unsafe {
        let len = (*ks).len as usize;
        let data = (*ks).data;
        if data.is_null() || len == 0 {
            return -1;
        }
        let slice = std::slice::from_raw_parts(data, len);
        match std::str::from_utf8(slice) {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };
    match std::fs::create_dir_all(path_str) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

// ---------------------------------------------------------------------------
// Additional string builtins
// ---------------------------------------------------------------------------

/// `trim_start(s)` — Trim leading whitespace, returning a new string.
#[no_mangle]
pub extern "C" fn kryos_builtin_trim_start(s: i64) -> i64 {
    if s == 0 {
        return unsafe { kryos_string_new(std::ptr::null(), 0) as i64 };
    }
    let ks = s as *const crate::string::KryosString;
    unsafe {
        let trimmed = bytes_to_str((*ks).data, (*ks).len as usize).trim_start();
        kryos_string_new(trimmed.as_ptr(), trimmed.len() as i64) as i64
    }
}

/// `trim_end(s)` — Trim trailing whitespace, returning a new string.
#[no_mangle]
pub extern "C" fn kryos_builtin_trim_end(s: i64) -> i64 {
    if s == 0 {
        return unsafe { kryos_string_new(std::ptr::null(), 0) as i64 };
    }
    let ks = s as *const crate::string::KryosString;
    unsafe {
        let trimmed = bytes_to_str((*ks).data, (*ks).len as usize).trim_end();
        kryos_string_new(trimmed.as_ptr(), trimmed.len() as i64) as i64
    }
}

/// `index_of(s, sub)` — Return the first byte offset of `sub` in `s`, or -1.
#[no_mangle]
pub extern "C" fn kryos_builtin_index_of(s: i64, sub: i64) -> i64 {
    if s == 0 || sub == 0 {
        return -1;
    }
    let ks_s = s as *const crate::string::KryosString;
    let ks_sub = sub as *const crate::string::KryosString;
    unsafe { crate::string::kryos_string_find(ks_s, ks_sub) }
}

// ---------------------------------------------------------------------------
// Array builtins
// ---------------------------------------------------------------------------

/// `sort(arr)` — Sort a numeric array in-place (ascending i64 order).
#[no_mangle]
pub unsafe extern "C" fn kryos_builtin_sort(arr_handle: i64) {
    if arr_handle == 0 {
        return;
    }
    let arr = arr_handle as *mut crate::array::KryosArray;
    let len = (*arr).len as usize;
    if len <= 1 {
        return;
    }
    let elems = std::slice::from_raw_parts_mut((*arr).data as *mut i64, len);
    elems.sort_unstable();
}

/// Raw byte view of a `KryosString` handle (empty for null/empty handles).
unsafe fn kryos_str_bytes<'a>(handle: i64) -> &'a [u8] {
    if handle == 0 {
        return &[];
    }
    let s = handle as *const crate::string::KryosString;
    let len = (*s).len as usize;
    let data = (*s).data;
    if data.is_null() || len == 0 {
        return &[];
    }
    std::slice::from_raw_parts(data, len)
}

/// `sort(arr)` on `[str]` — sort in-place by string content (byte-wise
/// lexicographic). Sorting the raw handles would order by heap address.
#[no_mangle]
pub unsafe extern "C" fn kryos_builtin_sort_str(arr_handle: i64) {
    if arr_handle == 0 {
        return;
    }
    let arr = arr_handle as *mut crate::array::KryosArray;
    let len = (*arr).len as usize;
    if len <= 1 {
        return;
    }
    let elems = std::slice::from_raw_parts_mut((*arr).data as *mut i64, len);
    elems.sort_by(|a, b| kryos_str_bytes(*a).cmp(kryos_str_bytes(*b)));
}

/// `sort(arr)` on `[f64]` — sort in-place by float value (slots hold the f64
/// bit pattern; IEEE total order). Sorting the bits as signed i64 reverses
/// the negative range.
#[no_mangle]
pub unsafe extern "C" fn kryos_builtin_sort_f64(arr_handle: i64) {
    if arr_handle == 0 {
        return;
    }
    let arr = arr_handle as *mut crate::array::KryosArray;
    let len = (*arr).len as usize;
    if len <= 1 {
        return;
    }
    let elems = std::slice::from_raw_parts_mut((*arr).data as *mut i64, len);
    elems.sort_by(|a, b| f64::from_bits(*a as u64).total_cmp(&f64::from_bits(*b as u64)));
}

/// `reverse(arr)` — Reverse an array in-place.
#[no_mangle]
pub unsafe extern "C" fn kryos_builtin_reverse(arr_handle: i64) {
    if arr_handle == 0 {
        return;
    }
    let arr = arr_handle as *mut crate::array::KryosArray;
    let len = (*arr).len as usize;
    if len <= 1 {
        return;
    }
    let elems = std::slice::from_raw_parts_mut((*arr).data as *mut i64, len);
    elems.reverse();
}

// ---------------------------------------------------------------------------
// Filesystem — append
// ---------------------------------------------------------------------------

/// `append_file(path, content)` — Append `content` to a file, creating it if
/// it does not exist. Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_builtin_file_append(path_handle: i64, content_handle: i64) -> i64 {
    if path_handle == 0 || content_handle == 0 {
        return -1;
    }
    let ks_path = path_handle as *const crate::string::KryosString;
    let ks_content = content_handle as *const crate::string::KryosString;
    unsafe {
        let path_bytes = std::slice::from_raw_parts((*ks_path).data, (*ks_path).len as usize);
        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => return -1,
        };
        let content_bytes =
            std::slice::from_raw_parts((*ks_content).data, (*ks_content).len as usize);
        use std::io::Write;
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path_str)
        {
            Ok(mut f) => match f.write_all(content_bytes) {
                Ok(()) => 0,
                Err(_) => -1,
            },
            Err(_) => -1,
        }
    }
}

// ---------------------------------------------------------------------------
// Networking — blocking HTTP GET (no TLS, v1)
// ---------------------------------------------------------------------------

/// `http_get(url)` — Blocking HTTP GET. Returns the response body as a new
/// KryosString. Returns an empty string on error. No TLS support in v1.
#[no_mangle]
pub extern "C" fn kryos_builtin_http_get(url_handle: i64) -> i64 {
    if url_handle == 0 {
        return unsafe { kryos_string_new(std::ptr::null(), 0) as i64 };
    }
    let ks_url = url_handle as *const crate::string::KryosString;
    let url_str = unsafe {
        let bytes = std::slice::from_raw_parts((*ks_url).data, (*ks_url).len as usize);
        match std::str::from_utf8(bytes) {
            Ok(s) => s.to_owned(),
            Err(_) => {
                return kryos_string_new(std::ptr::null(), 0) as i64;
            }
        }
    };

    // Off the coop baton: on an async task, sibling tasks run while the HTTP
    // round-trip (connect + request + response) blocks.
    let body = crate::executor::io_offload(|| http_get_impl(&url_str)).unwrap_or_default();
    let bytes = body.as_bytes();
    unsafe { kryos_string_new(bytes.as_ptr(), bytes.len() as i64) as i64 }
}

/// Minimal HTTP/1.0 GET over TCP. No TLS, no redirects.
fn http_get_impl(url: &str) -> Option<String> {
    use std::io::{Read, Write};

    // Strip "http://" prefix if present.
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);

    // Split host+port from path.
    let (host_port, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };

    // Determine host and port.
    let (host, port) = if let Some(idx) = host_port.rfind(':') {
        let p: u16 = host_port[idx + 1..].parse().unwrap_or(80);
        (&host_port[..idx], p)
    } else {
        (host_port, 80u16)
    };

    let addr = format!("{host}:{port}");
    let mut stream = std::net::TcpStream::connect(&addr).ok()?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .ok();

    let request = format!("GET {path} HTTP/1.0\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).ok()?;

    // Strip HTTP headers (end of headers is "\r\n\r\n").
    let body_start = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .unwrap_or(0);

    String::from_utf8(response[body_start..].to_vec()).ok()
}

// ---------------------------------------------------------------------------
// Math functions — abs, sqrt, floor, ceil, pow, min, max
// ---------------------------------------------------------------------------

/// `abs(x)` — Absolute value for i64.
#[no_mangle]
pub extern "C" fn kryos_builtin_abs(x: i64) -> i64 {
    x.abs()
}

/// `abs_f(x)` — Absolute value for f64.
#[no_mangle]
pub extern "C" fn kryos_builtin_abs_f(x: f64) -> f64 {
    x.abs()
}

/// `sqrt(x)` — Square root.
#[no_mangle]
pub extern "C" fn kryos_builtin_sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// `floor(x)` — Floor function.
#[no_mangle]
pub extern "C" fn kryos_builtin_floor(x: f64) -> f64 {
    x.floor()
}

/// `ceil(x)` — Ceiling function.
#[no_mangle]
pub extern "C" fn kryos_builtin_ceil(x: f64) -> f64 {
    x.ceil()
}

/// `pow(base, exp)` — Power function.
#[no_mangle]
pub extern "C" fn kryos_builtin_pow(base: f64, exp: f64) -> f64 {
    base.powf(exp)
}

/// `min(a, b)` — Minimum of two i64 values.
#[no_mangle]
pub extern "C" fn kryos_builtin_min(a: i64, b: i64) -> i64 {
    std::cmp::min(a, b)
}

/// `max(a, b)` — Maximum of two i64 values.
#[no_mangle]
pub extern "C" fn kryos_builtin_max(a: i64, b: i64) -> i64 {
    std::cmp::max(a, b)
}

/// `min_f(a, b)` — Minimum of two f64 values.
#[no_mangle]
pub extern "C" fn kryos_builtin_min_f(a: f64, b: f64) -> f64 {
    a.min(b)
}

/// `max_f(a, b)` — Maximum of two f64 values.
#[no_mangle]
pub extern "C" fn kryos_builtin_max_f(a: f64, b: f64) -> f64 {
    a.max(b)
}

// ---------------------------------------------------------------------------
// FFI helpers — used by stdlib `extern { ... }` blocks that need raw byte
// buffers and raw pointers into KryosString contents.
//
// These accept a KryosString handle (cast to i64; `0` is null) and return a
// raw `*mut u8` pointer into the heap-allocated byte buffer. The caller must
// NOT free the returned pointer — it is owned by the KryosString.
// ---------------------------------------------------------------------------

use crate::string::KryosString;

/// `str_to_ptr(s) -> i64` — raw data pointer (as i64) of a KryosString.
/// Returns 0 for a null handle.
#[no_mangle]
pub unsafe extern "C" fn kryos_str_to_ptr(handle: i64) -> i64 {
    if handle == 0 {
        return 0;
    }
    let s = handle as *const KryosString;
    (*s).data as i64
}

/// `buf_to_str(ptr, len) -> str` — build a KryosString by copying `len` bytes
/// from `ptr`. Returns 0 (null) if `ptr` is null and `len > 0`.
#[no_mangle]
pub unsafe extern "C" fn kryos_buf_to_str(ptr: i64, len: i64) -> i64 {
    if ptr == 0 && len > 0 {
        return 0;
    }
    crate::string::kryos_string_new(ptr as *const u8, len) as i64
}

/// `ptr_byte_at(ptr, i) -> i64` — read a single byte at offset `i`.
#[no_mangle]
pub unsafe extern "C" fn kryos_ptr_byte_at(ptr: i64, i: i64) -> i64 {
    if ptr == 0 || i < 0 {
        return 0;
    }
    let p = ptr as *const u8;
    *p.add(i as usize) as i64
}

/// `ptr_set_byte(ptr, i, b) -> void` — write a single byte at offset `i`.
#[no_mangle]
pub unsafe extern "C" fn kryos_ptr_set_byte(ptr: i64, i: i64, b: i64) {
    if ptr == 0 || i < 0 {
        return;
    }
    let p = ptr as *mut u8;
    *p.add(i as usize) = b as u8;
}

/// `alloc(size) -> i64` — heap allocate `size` bytes, zero-initialized.
/// Use `free_bytes` to release. Returns 0 on failure.
#[no_mangle]
pub unsafe extern "C" fn kryos_alloc_bytes(size: i64) -> i64 {
    if size <= 0 {
        return 0;
    }
    let layout = match std::alloc::Layout::from_size_align(size as usize, 1) {
        Ok(l) => l,
        Err(_) => return 0,
    };
    std::alloc::alloc_zeroed(layout) as i64
}

/// `free_bytes(ptr, size) -> void` — release memory from `alloc`.
#[no_mangle]
pub unsafe extern "C" fn kryos_free_bytes(ptr: i64, size: i64) {
    if ptr == 0 || size <= 0 {
        return;
    }
    let layout = match std::alloc::Layout::from_size_align(size as usize, 1) {
        Ok(l) => l,
        Err(_) => return,
    };
    std::alloc::dealloc(ptr as *mut u8, layout);
}

/// `handle_to_str(handle) -> str` — identity for KryosString handle; lets the
/// stdlib convert opaque i64 handles returned from native fns into typed
/// strings without a no-op call. Returns 0 for null.
#[no_mangle]
pub unsafe extern "C" fn kryos_handle_to_str(handle: i64) -> i64 {
    handle
}

/// `ptr_read_i64(p, i) -> i64` — read 8 bytes at offset `i*8`.
#[no_mangle]
pub unsafe extern "C" fn kryos_ptr_read_i64(ptr: i64, i: i64) -> i64 {
    if ptr == 0 || i < 0 {
        return 0;
    }
    let slot = (ptr as *const i64).add(i as usize);
    *slot
}

/// `ptr_write_i64(p, i, v) -> void` — write 8 bytes at offset `i*8`.
#[no_mangle]
pub unsafe extern "C" fn kryos_ptr_write_i64(ptr: i64, i: i64, v: i64) {
    if ptr == 0 || i < 0 {
        return;
    }
    let slot = (ptr as *mut i64).add(i as usize);
    *slot = v;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string::{kryos_string_free, kryos_string_len};

    #[test]
    fn ipow_basic() {
        assert_eq!(kryos_ipow(2, 0), 1);
        assert_eq!(kryos_ipow(2, 1), 2);
        assert_eq!(kryos_ipow(2, 10), 1024);
        assert_eq!(kryos_ipow(3, 3), 27);
        assert_eq!(kryos_ipow(-2, 3), -8);
        assert_eq!(kryos_ipow(-2, 4), 16);
    }

    #[test]
    fn ipow_negative_exp() {
        assert_eq!(kryos_ipow(2, -1), 0);
        assert_eq!(kryos_ipow(1, -5), 1);
        assert_eq!(kryos_ipow(-1, -3), -1);
    }

    #[test]
    fn i64_to_string_basic() {
        let handle = kryos_i64_to_string(42);
        assert_ne!(handle, 0);
        unsafe {
            let ptr = handle as *const crate::string::KryosString;
            assert_eq!(kryos_string_len(ptr), 2); // "42"
            kryos_string_free(handle as *mut crate::string::KryosString);
        }
    }

    #[test]
    fn i64_to_string_negative() {
        let handle = kryos_i64_to_string(-123);
        assert_ne!(handle, 0);
        unsafe {
            let ptr = handle as *const crate::string::KryosString;
            assert_eq!(kryos_string_len(ptr), 4); // "-123"
            kryos_string_free(handle as *mut crate::string::KryosString);
        }
    }

    #[test]
    fn bool_to_string_values() {
        let t = kryos_bool_to_string(1);
        let f = kryos_bool_to_string(0);
        unsafe {
            assert_eq!(kryos_string_len(t as *const crate::string::KryosString), 4); // "true"
            assert_eq!(kryos_string_len(f as *const crate::string::KryosString), 5); // "false"
            kryos_string_free(t as *mut crate::string::KryosString);
            kryos_string_free(f as *mut crate::string::KryosString);
        }
    }

    #[test]
    fn chan_send_recv_i64() {
        let ch = kryos_chan_new_i64();
        assert_ne!(ch, 0);
        let send_result = kryos_chan_send_i64(ch, 42);
        assert_eq!(send_result, 0);
        let received = kryos_chan_recv_i64(ch);
        assert_eq!(received, 42);
        kryos_chan_drop_i64(ch);
    }

    #[test]
    fn chan_multiple_values() {
        let ch = kryos_chan_new_i64();
        kryos_chan_send_i64(ch, 10);
        kryos_chan_send_i64(ch, 20);
        kryos_chan_send_i64(ch, 30);
        assert_eq!(kryos_chan_recv_i64(ch), 10);
        assert_eq!(kryos_chan_recv_i64(ch), 20);
        assert_eq!(kryos_chan_recv_i64(ch), 30);
        kryos_chan_drop_i64(ch);
    }

    #[test]
    fn try_recv_status_no_data() {
        let ch = kryos_chan_new_i64();
        let status = kryos_chan_try_recv_status_i64(ch);
        assert_eq!(status, 0, "empty channel should return status 0");
        kryos_chan_drop_i64(ch);
    }

    #[test]
    fn try_recv_status_with_data() {
        let ch = kryos_chan_new_i64();
        kryos_chan_send_i64(ch, 42);
        let status = kryos_chan_try_recv_status_i64(ch);
        assert_eq!(status, 1, "channel with data should return status 1");
        let value = kryos_chan_last_recv_i64();
        assert_eq!(value, 42);
        kryos_chan_drop_i64(ch);
    }

    #[test]
    fn try_recv_status_closed() {
        let ch = kryos_chan_new_i64();
        kryos_chan_close_i64(ch);
        let status = kryos_chan_try_recv_status_i64(ch);
        assert_eq!(status, -1, "closed channel should return status -1");
        kryos_chan_drop_i64(ch);
    }

    #[test]
    fn try_recv_i64_min_value() {
        // Verify i64::MIN can be sent and received correctly (no sentinel collision).
        let ch = kryos_chan_new_i64();
        kryos_chan_send_i64(ch, i64::MIN);
        let status = kryos_chan_try_recv_status_i64(ch);
        assert_eq!(status, 1);
        let value = kryos_chan_last_recv_i64();
        assert_eq!(value, i64::MIN, "i64::MIN must be received correctly");
        kryos_chan_drop_i64(ch);
    }

    #[test]
    fn is_closed_open_channel() {
        let ch = kryos_chan_new_i64();
        assert_eq!(kryos_chan_is_closed_i64(ch), 0);
        kryos_chan_drop_i64(ch);
    }

    #[test]
    fn is_closed_after_close() {
        let ch = kryos_chan_new_i64();
        kryos_chan_close_i64(ch);
        assert_eq!(kryos_chan_is_closed_i64(ch), 1);
        kryos_chan_drop_i64(ch);
    }

    #[test]
    fn actor_spawn_send_recv() {
        use std::sync::atomic::{AtomicI64, Ordering};
        static RECEIVED: AtomicI64 = AtomicI64::new(0);

        extern "C" fn actor_main(_state: *mut u8) {
            let msg = kryos_actor_recv_i64();
            RECEIVED.store(msg, Ordering::SeqCst);
        }

        let actor_id = kryos_actor_spawn_i64(actor_main as *const () as i64, 0);
        assert!(actor_id > 0);
        let send_result = kryos_actor_send_i64(actor_id, 42);
        assert_eq!(send_result, 0);
        // Give the actor thread time to process
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(RECEIVED.load(Ordering::SeqCst), 42);
    }
}
