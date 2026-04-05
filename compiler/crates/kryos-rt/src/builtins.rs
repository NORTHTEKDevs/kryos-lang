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
}
