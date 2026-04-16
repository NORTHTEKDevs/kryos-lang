//! String operations for the Kryos native stdlib.
//!
//! Provides string manipulation functions that operate on null-terminated C strings.
//! Functions that allocate and return strings place ownership on the caller, who must
//! free them via `kryos_str_free`.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// Returns the length of a null-terminated string.
#[no_mangle]
pub extern "C" fn kryos_str_len(ptr: *const u8) -> i64 {
    if ptr.is_null() {
        return -1;
    }
    unsafe {
        let cstr = CStr::from_ptr(ptr as *const c_char);
        match cstr.to_str() {
            Ok(s) => s.len() as i64,
            Err(_) => -1,
        }
    }
}

/// Concatenates two strings and returns a new allocated string.
/// Caller owns the returned pointer and must free it with kryos_str_free.
#[no_mangle]
pub extern "C" fn kryos_str_concat(a: *const u8, b: *const u8) -> *mut u8 {
    if a.is_null() || b.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        let str_a = match CStr::from_ptr(a as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let str_b = match CStr::from_ptr(b as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };

        let result = format!("{}{}", str_a, str_b);
        match CString::new(result) {
            Ok(cstr) => cstr.into_raw() as *mut u8,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Checks if haystack contains needle.
/// Returns 1 if found, 0 if not found or error.
#[no_mangle]
pub extern "C" fn kryos_str_contains(haystack: *const u8, needle: *const u8) -> i32 {
    if haystack.is_null() || needle.is_null() {
        return 0;
    }
    unsafe {
        let hay = match CStr::from_ptr(haystack as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return 0,
        };
        let need = match CStr::from_ptr(needle as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return 0,
        };
        if hay.contains(need) { 1 } else { 0 }
    }
}

/// Checks if string starts with prefix.
/// Returns 1 if true, 0 if false or error.
#[no_mangle]
pub extern "C" fn kryos_str_starts_with(s: *const u8, prefix: *const u8) -> i32 {
    if s.is_null() || prefix.is_null() {
        return 0;
    }
    unsafe {
        let str_s = match CStr::from_ptr(s as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return 0,
        };
        let str_prefix = match CStr::from_ptr(prefix as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return 0,
        };
        if str_s.starts_with(str_prefix) { 1 } else { 0 }
    }
}

/// Checks if string ends with suffix.
/// Returns 1 if true, 0 if false or error.
#[no_mangle]
pub extern "C" fn kryos_str_ends_with(s: *const u8, suffix: *const u8) -> i32 {
    if s.is_null() || suffix.is_null() {
        return 0;
    }
    unsafe {
        let str_s = match CStr::from_ptr(s as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return 0,
        };
        let str_suffix = match CStr::from_ptr(suffix as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return 0,
        };
        if str_s.ends_with(str_suffix) { 1 } else { 0 }
    }
}

/// Converts string to uppercase.
/// Returns a newly allocated string. Caller owns the pointer.
#[no_mangle]
pub extern "C" fn kryos_str_to_upper(s: *const u8) -> *mut u8 {
    if s.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        let str_s = match CStr::from_ptr(s as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let result = str_s.to_uppercase();
        match CString::new(result) {
            Ok(cstr) => cstr.into_raw() as *mut u8,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Converts string to lowercase.
/// Returns a newly allocated string. Caller owns the pointer.
#[no_mangle]
pub extern "C" fn kryos_str_to_lower(s: *const u8) -> *mut u8 {
    if s.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        let str_s = match CStr::from_ptr(s as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let result = str_s.to_lowercase();
        match CString::new(result) {
            Ok(cstr) => cstr.into_raw() as *mut u8,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Trims whitespace from both ends of string.
/// Returns a newly allocated string. Caller owns the pointer.
#[no_mangle]
pub extern "C" fn kryos_str_trim(s: *const u8) -> *mut u8 {
    if s.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        let str_s = match CStr::from_ptr(s as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let result = str_s.trim().to_string();
        match CString::new(result) {
            Ok(cstr) => cstr.into_raw() as *mut u8,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Parses a string to i64.
/// Returns 0 on success, -1 on parse error. Output written to out pointer.
#[no_mangle]
pub extern "C" fn kryos_str_parse_i64(s: *const u8, out: *mut i64) -> i32 {
    if s.is_null() || out.is_null() {
        return -1;
    }
    unsafe {
        let str_s = match CStr::from_ptr(s as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        };
        match str_s.trim().parse::<i64>() {
            Ok(val) => {
                *out = val;
                0
            }
            Err(_) => -1,
        }
    }
}

/// Parses a string to f64.
/// Returns 0 on success, -1 on parse error. Output written to out pointer.
#[no_mangle]
pub extern "C" fn kryos_str_parse_f64(s: *const u8, out: *mut f64) -> i32 {
    if s.is_null() || out.is_null() {
        return -1;
    }
    unsafe {
        let str_s = match CStr::from_ptr(s as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        };
        match str_s.trim().parse::<f64>() {
            Ok(val) => {
                *out = val;
                0
            }
            Err(_) => -1,
        }
    }
}

/// Frees a string allocated by string functions.
#[no_mangle]
pub extern "C" fn kryos_str_free(ptr: *mut u8) {
    if !ptr.is_null() {
        unsafe {
            drop(CString::from_raw(ptr as *mut c_char));
        }
    }
}

/// Repeats a string n times.
/// Returns a newly allocated string. Caller owns the pointer.
#[no_mangle]
pub extern "C" fn kryos_str_repeat(s: *const u8, n: i64) -> *mut u8 {
    if s.is_null() || n < 0 {
        return std::ptr::null_mut();
    }
    unsafe {
        let str_s = match CStr::from_ptr(s as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let result = str_s.repeat(n as usize);
        match CString::new(result) {
            Ok(cstr) => cstr.into_raw() as *mut u8,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Replaces all occurrences of `from` with `to` in string `s`.
/// Returns a newly allocated string. Caller owns the pointer.
#[no_mangle]
pub extern "C" fn kryos_str_replace(s: *const u8, from: *const u8, to: *const u8) -> *mut u8 {
    if s.is_null() || from.is_null() || to.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        let str_s = match CStr::from_ptr(s as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let str_from = match CStr::from_ptr(from as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let str_to = match CStr::from_ptr(to as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let result = str_s.replace(str_from, str_to);
        match CString::new(result) {
            Ok(cstr) => cstr.into_raw() as *mut u8,
            Err(_) => std::ptr::null_mut(),
        }
    }
}
