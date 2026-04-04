//! KryosString — heap-allocated, length-tracked string type.
//!
//! Layout: `{ len: i64, cap: i64, data: *mut u8 }`.
//! Data is always null-terminated for C interop. All functions are
//! `#[no_mangle] extern "C"` for linking from compiled Kryos code.

use std::alloc::{alloc, dealloc, Layout};
use std::ptr;

/// Heap-allocated string with explicit length and capacity.
#[repr(C)]
pub struct KryosString {
    pub len: i64,
    pub cap: i64,
    pub data: *mut u8,
}

impl KryosString {
    fn layout(cap: usize) -> Layout {
        // +1 for null terminator.
        Layout::from_size_align(cap + 1, 1).unwrap()
    }
}

/// Create a new KryosString by copying `len` bytes from `ptr`.
///
/// The caller retains ownership of `ptr`. The returned string is independently
/// heap-allocated.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_new(ptr: *const u8, len: i64) -> *mut KryosString {
    let len_usize = if len < 0 { 0 } else { len as usize };
    let cap = len_usize;
    let layout = KryosString::layout(cap);
    let data = alloc(layout);
    if data.is_null() {
        return ptr::null_mut();
    }
    if len_usize > 0 && !ptr.is_null() {
        ptr::copy_nonoverlapping(ptr, data, len_usize);
    }
    // Null-terminate.
    *data.add(len_usize) = 0;

    let s = alloc(Layout::new::<KryosString>()) as *mut KryosString;
    if s.is_null() {
        dealloc(data, layout);
        return ptr::null_mut();
    }
    (*s).len = len_usize as i64;
    (*s).cap = cap as i64;
    (*s).data = data;
    s
}

/// Concatenate two strings, returning a new heap-allocated string.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_concat(
    a: *const KryosString,
    b: *const KryosString,
) -> *mut KryosString {
    if a.is_null() && b.is_null() {
        return kryos_string_new(ptr::null(), 0);
    }
    if a.is_null() {
        return kryos_string_new((*b).data, (*b).len);
    }
    if b.is_null() {
        return kryos_string_new((*a).data, (*a).len);
    }

    let a_len = (*a).len as usize;
    let b_len = (*b).len as usize;
    let total = a_len + b_len;

    let layout = KryosString::layout(total);
    let data = alloc(layout);
    if data.is_null() {
        return ptr::null_mut();
    }
    ptr::copy_nonoverlapping((*a).data, data, a_len);
    ptr::copy_nonoverlapping((*b).data, data.add(a_len), b_len);
    *data.add(total) = 0;

    let s = alloc(Layout::new::<KryosString>()) as *mut KryosString;
    if s.is_null() {
        dealloc(data, layout);
        return ptr::null_mut();
    }
    (*s).len = total as i64;
    (*s).cap = total as i64;
    (*s).data = data;
    s
}

/// Return the length of a string.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_len(s: *const KryosString) -> i64 {
    if s.is_null() {
        return 0;
    }
    (*s).len
}

/// Compare two strings for equality.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_eq(
    a: *const KryosString,
    b: *const KryosString,
) -> bool {
    if a.is_null() && b.is_null() {
        return true;
    }
    if a.is_null() || b.is_null() {
        return false;
    }
    if (*a).len != (*b).len {
        return false;
    }
    let len = (*a).len as usize;
    if len == 0 {
        return true;
    }
    let a_slice = std::slice::from_raw_parts((*a).data, len);
    let b_slice = std::slice::from_raw_parts((*b).data, len);
    a_slice == b_slice
}

/// Extract a substring [start..end). Returns a new heap-allocated string.
///
/// Clamps indices to valid range.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_slice(
    s: *const KryosString,
    start: i64,
    end: i64,
) -> *mut KryosString {
    if s.is_null() {
        return kryos_string_new(ptr::null(), 0);
    }
    let len = (*s).len as usize;
    let start = (start.max(0) as usize).min(len);
    let end = (end.max(0) as usize).min(len);
    if start >= end {
        return kryos_string_new(ptr::null(), 0);
    }
    let slice_len = end - start;
    kryos_string_new((*s).data.add(start), slice_len as i64)
}

/// Find the first occurrence of `needle` in `s`. Returns the byte offset,
/// or -1 if not found.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_find(
    s: *const KryosString,
    needle: *const KryosString,
) -> i64 {
    if s.is_null() || needle.is_null() {
        return -1;
    }
    let s_len = (*s).len as usize;
    let n_len = (*needle).len as usize;
    if n_len == 0 {
        return 0;
    }
    if n_len > s_len {
        return -1;
    }
    let haystack = std::slice::from_raw_parts((*s).data, s_len);
    let needle_bytes = std::slice::from_raw_parts((*needle).data, n_len);
    for i in 0..=(s_len - n_len) {
        if &haystack[i..i + n_len] == needle_bytes {
            return i as i64;
        }
    }
    -1
}

/// Free a KryosString and its data buffer.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_free(s: *mut KryosString) {
    if s.is_null() {
        return;
    }
    let cap = (*s).cap as usize;
    if !(*s).data.is_null() && cap > 0 {
        dealloc((*s).data, KryosString::layout(cap));
    }
    dealloc(s as *mut u8, Layout::new::<KryosString>());
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_and_len() {
        unsafe {
            let data = b"hello";
            let s = kryos_string_new(data.as_ptr(), 5);
            assert!(!s.is_null());
            assert_eq!(kryos_string_len(s), 5);
            // Null-terminated.
            assert_eq!(*(*s).data.add(5), 0);
            kryos_string_free(s);
        }
    }

    #[test]
    fn empty_string() {
        unsafe {
            let s = kryos_string_new(std::ptr::null(), 0);
            assert!(!s.is_null());
            assert_eq!(kryos_string_len(s), 0);
            kryos_string_free(s);
        }
    }

    #[test]
    fn concat_two_strings() {
        unsafe {
            let a = kryos_string_new(b"hello".as_ptr(), 5);
            let b = kryos_string_new(b" world".as_ptr(), 6);
            let c = kryos_string_concat(a, b);
            assert!(!c.is_null());
            assert_eq!(kryos_string_len(c), 11);
            let slice = std::slice::from_raw_parts((*c).data, 11);
            assert_eq!(slice, b"hello world");
            kryos_string_free(a);
            kryos_string_free(b);
            kryos_string_free(c);
        }
    }

    #[test]
    fn equality() {
        unsafe {
            let a = kryos_string_new(b"same".as_ptr(), 4);
            let b = kryos_string_new(b"same".as_ptr(), 4);
            let c = kryos_string_new(b"diff".as_ptr(), 4);
            assert!(kryos_string_eq(a, b));
            assert!(!kryos_string_eq(a, c));
            kryos_string_free(a);
            kryos_string_free(b);
            kryos_string_free(c);
        }
    }

    #[test]
    fn slice_substring() {
        unsafe {
            let s = kryos_string_new(b"hello world".as_ptr(), 11);
            let sub = kryos_string_slice(s, 6, 11);
            assert!(!sub.is_null());
            assert_eq!(kryos_string_len(sub), 5);
            let slice = std::slice::from_raw_parts((*sub).data, 5);
            assert_eq!(slice, b"world");
            kryos_string_free(sub);
            kryos_string_free(s);
        }
    }

    #[test]
    fn find_needle() {
        unsafe {
            let s = kryos_string_new(b"hello world".as_ptr(), 11);
            let needle = kryos_string_new(b"world".as_ptr(), 5);
            assert_eq!(kryos_string_find(s, needle), 6);
            let missing = kryos_string_new(b"xyz".as_ptr(), 3);
            assert_eq!(kryos_string_find(s, missing), -1);
            kryos_string_free(s);
            kryos_string_free(needle);
            kryos_string_free(missing);
        }
    }

    #[test]
    fn null_safety() {
        unsafe {
            assert_eq!(kryos_string_len(std::ptr::null()), 0);
            assert!(kryos_string_eq(std::ptr::null(), std::ptr::null()));
            assert!(!kryos_string_eq(std::ptr::null(), kryos_string_new(b"x".as_ptr(), 1)));
            kryos_string_free(std::ptr::null_mut()); // should not crash
        }
    }
}
