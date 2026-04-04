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
}
