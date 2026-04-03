//! Regular expression operations for the Kryos native stdlib.
//!
//! Uses the `regex` crate. Regex objects are heap-allocated and returned as opaque
//! pointers. The caller must call `kryos_regex_drop` to free them.

use regex::Regex;

/// Compiles a regex pattern, returning an opaque pointer.
///
/// Returns null if the pattern is invalid UTF-8 or fails to compile.
#[no_mangle]
pub extern "C" fn kryos_regex_new(pattern_ptr: *const u8, pattern_len: usize) -> *mut u8 {
    if pattern_ptr.is_null() {
        return std::ptr::null_mut();
    }
    let pattern = unsafe { std::slice::from_raw_parts(pattern_ptr, pattern_len) };
    let pattern = match std::str::from_utf8(pattern) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    match Regex::new(pattern) {
        Ok(re) => {
            let boxed = Box::new(re);
            Box::into_raw(boxed) as *mut u8
        }
        Err(_) => std::ptr::null_mut(),
    }
}

/// Tests whether the regex matches the given text.
///
/// Returns 1 if it matches, 0 if it does not, -1 on error (null pointers or invalid UTF-8).
#[no_mangle]
pub extern "C" fn kryos_regex_is_match(
    re: *mut u8,
    text_ptr: *const u8,
    text_len: usize,
) -> i32 {
    if re.is_null() || text_ptr.is_null() {
        return -1;
    }
    let re = unsafe { &*(re as *const Regex) };
    let text = unsafe { std::slice::from_raw_parts(text_ptr, text_len) };
    let text = match std::str::from_utf8(text) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    if re.is_match(text) {
        1
    } else {
        0
    }
}

/// Drops (frees) the compiled regex object.
///
/// After this call, the pointer is invalid and must not be used.
#[no_mangle]
pub extern "C" fn kryos_regex_drop(re: *mut u8) {
    if re.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(re as *mut Regex);
    }
}
