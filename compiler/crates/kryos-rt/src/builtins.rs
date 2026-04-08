//! Built-in runtime functions for compiled Kryos programs.
//!
//! These are the implementations behind Kryos built-in functions like
//! `to_string()`, `len()`, and the `**` power operator.

use crate::string::kryos_string_new;

/// Generic `len()` for any Kryos collection.
///
/// Works because KryosString, KryosArray, and MapHeader all have `len: i64`
/// as their first field (at offset 0). The argument is an opaque handle (pointer as i64).
#[no_mangle]
pub extern "C" fn kryos_builtin_len(collection: i64) -> i64 {
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
        return if base == 1 { 1 } else if base == -1 { if exp % 2 == 0 { 1 } else { -1 } } else { 0 };
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

/// Convert an i64 to a KryosString. Returns an opaque handle (pointer as i64).
#[no_mangle]
pub extern "C" fn kryos_i64_to_string(value: i64) -> i64 {
    let s = value.to_string();
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
    if handle == 0 {
        println!();
        return;
    }
    let s = handle as *const crate::string::KryosString;
    unsafe {
        let len = (*s).len as usize;
        let data = (*s).data;
        if !data.is_null() && len > 0 {
            let slice = std::slice::from_raw_parts(data, len);
            if let Ok(text) = std::str::from_utf8(slice) {
                println!("{}", text);
            } else {
                println!("<invalid utf-8>");
            }
        } else {
            println!();
        }
    }
}

/// Print a KryosString without a newline.
#[no_mangle]
pub extern "C" fn kryos_print_str(handle: i64) {
    if handle == 0 {
        return;
    }
    let s = handle as *const crate::string::KryosString;
    unsafe {
        let len = (*s).len as usize;
        let data = (*s).data;
        if !data.is_null() && len > 0 {
            let slice = std::slice::from_raw_parts(data, len);
            if let Ok(text) = std::str::from_utf8(slice) {
                print!("{}", text);
            }
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
    println!("{}", value);
}

/// Print an i64 value without a newline.
#[no_mangle]
pub extern "C" fn kryos_print_int(value: i64) {
    print!("{}", value);
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
    let result = crate::channel::kryos_chan_send(
        handle as *mut u8,
        &value as *const i64 as *const u8,
        8,
    );
    result as i64
}

/// Receive an i64 value from a channel (blocking).
/// Returns the received value, or 0 if the channel is closed.
#[no_mangle]
pub extern "C" fn kryos_chan_recv_i64(handle: i64) -> i64 {
    let mut buf: i64 = 0;
    crate::channel::kryos_chan_recv(
        handle as *mut u8,
        &mut buf as *mut i64 as *mut u8,
        8,
    );
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
    let result = crate::channel::kryos_chan_try_recv(
        handle as *mut u8,
        &mut buf as *mut i64 as *mut u8,
        8,
    );
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
    let fn_ptr: extern "C" fn(*mut u8) = unsafe {
        std::mem::transmute(dispatch_fn_ptr as usize)
    };
    crate::actor::kryos_actor_spawn(fn_ptr, state_ptr as *mut u8) as i64
}

/// Send a single i64 message to an actor.
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_actor_send_i64(actor_id: i64, message: i64) -> i64 {
    crate::actor::kryos_actor_send(
        actor_id as u64,
        &message as *const i64 as *const u8,
        8,
    ) as i64
}

/// Receive a single i64 message from the current actor's mailbox. Blocks.
/// Returns the message value. Returns 0 if mailbox closed.
#[no_mangle]
pub extern "C" fn kryos_actor_recv_i64() -> i64 {
    let mut buf: i64 = 0;
    let result = crate::actor::kryos_actor_recv(
        &mut buf as *mut i64 as *mut u8,
        8,
    );
    if result > 0 { buf } else { 0 }
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
        std::fs::read_to_string(path_str).unwrap_or_default()
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
    eprintln!("assertion failed: {}", msg);
    std::process::abort();
}

/// Parse a KryosString as an integer. Returns the parsed value, or 0 on failure.
#[no_mangle]
pub extern "C" fn kryos_builtin_parse_int(s_handle: i64) -> i64 {
    if s_handle == 0 {
        return 0;
    }
    let ks = s_handle as *const crate::string::KryosString;
    let text = unsafe {
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
    text.trim().parse::<i64>().unwrap_or(0)
}

/// Parse a KryosString as an f64, returning the bits as i64. Returns 0 on failure.
#[no_mangle]
pub extern "C" fn kryos_builtin_parse_float(s_handle: i64) -> i64 {
    if s_handle == 0 {
        return 0;
    }
    let ks = s_handle as *const crate::string::KryosString;
    let text = unsafe {
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
    match text.trim().parse::<f64>() {
        Ok(v) => v.to_bits() as i64,
        Err(_) => 0,
    }
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
    let ch = if code >= 0 && code <= 0x10FFFF {
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
    if s_handle == 0 {
        return unsafe { kryos_string_new(std::ptr::null(), 0) as i64 };
    }
    let ks = s_handle as *const crate::string::KryosString;
    unsafe { crate::string::kryos_string_slice(ks, start, end) as i64 }
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

// ---------------------------------------------------------------------------
// Runtime checks — division by zero
// ---------------------------------------------------------------------------

/// Check for integer division by zero at runtime.
/// Called by codegen before `sdiv` and `srem` instructions.
/// If the divisor is zero, prints a panic message and aborts.
#[no_mangle]
pub extern "C" fn kryos_check_div_zero_i64(divisor: i64) {
    if divisor == 0 {
        eprintln!("kryos panic: integer division by zero");
        std::process::abort();
    }
}

/// Check for float division by zero — intentional no-op.
/// IEEE 754 float division by zero produces inf/nan, which is expected behavior.
#[no_mangle]
pub extern "C" fn kryos_check_div_zero_f64(_divisor: f64) {
    // No-op: float division by zero produces inf/nan per IEEE 754.
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
    if idx < buf.data.len() { buf.data[idx] as i64 } else { -1 }
}

#[no_mangle]
pub unsafe extern "C" fn kryos_buf_set_byte(handle: i64, offset: i64, byte: i64) {
    let buf = &mut *(handle as *mut KryosBuf);
    let idx = offset as usize;
    if idx < buf.data.len() { buf.data[idx] = byte as u8; }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string::{kryos_string_len, kryos_string_free};

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
