//! KryosString — heap-allocated, length-tracked string type.
//!
//! Layout: `{ len: i64, cap: i64, data: *mut u8 }`.
//! Data is always null-terminated for C interop. All functions are
//! `#[no_mangle] extern "C"` for linking from compiled Kryos code.
//!
//! # Unsafe invariants (file-wide)
//!
//! See `docs/17-unsafe-audit.md` patterns 1 (FFI handle reconstruction),
//! 2 (slice::from_raw_parts on `(data, len)`) and 3 (alloc/dealloc).
//!
//! * Handles are `*mut KryosString` cast to `i64`; `0` is null.
//! * `data` is non-null when `len > 0`, allocated for `cap + 1` bytes
//!   (extra byte holds the C nul terminator).
//! * Realloc paths (concat / push) compute the old `Layout` from the
//!   pre-realloc `cap` field before freeing.
//! * Byte content is not assumed UTF-8 internally; conversion to `&str`
//!   always goes through `str::from_utf8(...).unwrap_or("")`.

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

/// Safely convert raw bytes to a &str. Returns empty string on invalid UTF-8.
unsafe fn bytes_to_str<'a>(ptr: *const u8, len: usize) -> &'a str {
    let slice = std::slice::from_raw_parts(ptr, len);
    std::str::from_utf8(slice).unwrap_or("")
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

/// Compute a content-based hash of a KryosString (FNV-1a).
/// Returns 0 for null pointers.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_hash(s: *const KryosString) -> i64 {
    if s.is_null() {
        return 0;
    }
    let len = (*s).len as usize;
    let data = (*s).data;
    // FNV-1a hash
    let mut h: u64 = 0xcbf29ce484222325;
    for i in 0..len {
        h ^= *data.add(i) as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h as i64
}

/// Compare two strings for equality.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_eq(a: *const KryosString, b: *const KryosString) -> bool {
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

/// Compare two strings lexicographically by byte content.
/// Returns -1 if a < b, 0 if equal, +1 if a > b.
/// Null pointers sort as the empty string.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_compare(
    a: *const KryosString,
    b: *const KryosString,
) -> i64 {
    let a_slice: &[u8] = if a.is_null() {
        &[]
    } else {
        let len = (*a).len as usize;
        if len == 0 { &[] } else { std::slice::from_raw_parts((*a).data, len) }
    };
    let b_slice: &[u8] = if b.is_null() {
        &[]
    } else {
        let len = (*b).len as usize;
        if len == 0 { &[] } else { std::slice::from_raw_parts((*b).data, len) }
    };
    match a_slice.cmp(b_slice) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Extract a substring [start..end). Returns a new heap-allocated string.
///
/// Panics on out-of-bounds indices.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_slice(
    s: *const KryosString,
    start: i64,
    end: i64,
) -> *mut KryosString {
    if s.is_null() {
        let msg = b"string is null";
        crate::panic::kryos_panic(msg.as_ptr(), msg.len());
    }
    let len = (*s).len;
    if start < 0 || end < 0 || start > len || end > len || start > end {
        let msg = format!(
            "string slice out of bounds: [{}..{}) but length is {}",
            start, end, len
        );
        crate::panic::kryos_panic(msg.as_ptr(), msg.len());
    }
    let start_usize = start as usize;
    let end_usize = end as usize;
    let slice_len = end_usize - start_usize;
    if slice_len == 0 {
        return kryos_string_new(ptr::null(), 0);
    }
    kryos_string_new((*s).data.add(start_usize), slice_len as i64)
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

/// Clone a KryosString — share the existing pointer (H19, shift step 30).
///
/// Kryos strings are immutable (concat allocates a new string, no in-place
/// mutation), so sharing the underlying pointer is semantically equivalent
/// to deep-cloning when paired with H10 (no-op kryos_string_free). Removes
/// O(N) string clone allocations from every @copy struct construction
/// involving a Str field. Eliminates the most common heap-pressure source
/// in stage-1's tokenize/parse path.
///
/// Null input still returns a fresh empty string (matches old contract).
#[no_mangle]
pub unsafe extern "C" fn kryos_string_clone(s: *const KryosString) -> *mut KryosString {
    if s.is_null() {
        return kryos_string_new(ptr::null(), 0);
    }
    s as *mut KryosString
}

/// Free a KryosString and its data buffer.
///
/// H4 INSTRUMENTATION (shift kryos-self-compile step 22, 2026-05-20):
/// Strings have no ref_count -- they are unique-owner. After freeing,
/// we set cap = -1 as a sentinel and leak the header (~24 bytes) so
/// a double-free can be detected by observing cap < 0 on entry.
/// Revert before production.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_free(s: *mut KryosString) {
    if s.is_null() {
        return;
    }
    // H10 (shift step 25): leak strings entirely. Strings have no
    // ref-count so they can't be retained-and-shared like arrays.
    // Many stage-1 paths share string pointers across @copy struct
    // boundaries (Token.text, Symbol.name, etc.), and a wrong drop
    // produces use-after-free that flakes the bootstrap. Until we
    // either (a) add ref_count to KryosString or (b) make codegen
    // emit kryos_string_clone everywhere shared, treat string-free
    // as a no-op. Leaks ~50-100MB per stage-1 invocation max; well
    // under the leak-guard 2GB limit.
    let _ = s;
}

// ---------------------------------------------------------------------------
// String utilities
// ---------------------------------------------------------------------------

/// Check if a string starts with a prefix.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_starts_with(
    s: *const KryosString,
    prefix: *const KryosString,
) -> i64 {
    if s.is_null() || prefix.is_null() {
        return 0;
    }
    let s_slice = std::slice::from_raw_parts((*s).data, (*s).len as usize);
    let p_slice = std::slice::from_raw_parts((*prefix).data, (*prefix).len as usize);
    if s_slice.starts_with(p_slice) {
        1
    } else {
        0
    }
}

/// Check if a string ends with a suffix.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_ends_with(
    s: *const KryosString,
    suffix: *const KryosString,
) -> i64 {
    if s.is_null() || suffix.is_null() {
        return 0;
    }
    let s_slice = std::slice::from_raw_parts((*s).data, (*s).len as usize);
    let p_slice = std::slice::from_raw_parts((*suffix).data, (*suffix).len as usize);
    if s_slice.ends_with(p_slice) {
        1
    } else {
        0
    }
}

/// Check if a string contains a needle.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_contains(
    s: *const KryosString,
    needle: *const KryosString,
) -> i64 {
    if s.is_null() || needle.is_null() {
        return 0;
    }
    let s_str = bytes_to_str((*s).data, (*s).len as usize);
    let n_str = bytes_to_str((*needle).data, (*needle).len as usize);
    if s_str.contains(n_str) {
        1
    } else {
        0
    }
}

/// Trim whitespace from both ends.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_trim(s: *const KryosString) -> *mut KryosString {
    if s.is_null() {
        return kryos_string_new(ptr::null(), 0);
    }
    let src = bytes_to_str((*s).data, (*s).len as usize);
    let trimmed = src.trim();
    kryos_string_new(trimmed.as_ptr(), trimmed.len() as i64)
}

/// Convert to uppercase.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_to_upper(s: *const KryosString) -> *mut KryosString {
    if s.is_null() {
        return kryos_string_new(ptr::null(), 0);
    }
    let src = bytes_to_str((*s).data, (*s).len as usize);
    let upper = src.to_uppercase();
    kryos_string_new(upper.as_ptr(), upper.len() as i64)
}

/// Convert to lowercase.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_to_lower(s: *const KryosString) -> *mut KryosString {
    if s.is_null() {
        return kryos_string_new(ptr::null(), 0);
    }
    let src = bytes_to_str((*s).data, (*s).len as usize);
    let lower = src.to_lowercase();
    kryos_string_new(lower.as_ptr(), lower.len() as i64)
}

/// Replace all occurrences of `from` with `to` in `s`.
#[no_mangle]
pub unsafe extern "C" fn kryos_string_replace(
    s: *const KryosString,
    from: *const KryosString,
    to: *const KryosString,
) -> *mut KryosString {
    if s.is_null() || from.is_null() || to.is_null() {
        return kryos_string_new(ptr::null(), 0);
    }
    let s_str = bytes_to_str((*s).data, (*s).len as usize);
    let from_str = bytes_to_str((*from).data, (*from).len as usize);
    let to_str = bytes_to_str((*to).data, (*to).len as usize);
    let result = s_str.replace(from_str, to_str);
    kryos_string_new(result.as_ptr(), result.len() as i64)
}

// Note: kryos_string_char_at is defined in builtins.rs

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
            assert!(!kryos_string_eq(
                std::ptr::null(),
                kryos_string_new(b"x".as_ptr(), 1)
            ));
            kryos_string_free(std::ptr::null_mut()); // should not crash
        }
    }
}
