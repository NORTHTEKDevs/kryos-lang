//! Regular expression operations for the Kryos native stdlib.
//!
//! Uses the `regex` crate. Regex objects are heap-allocated and returned as
//! opaque pointers. The caller must call `kryos_regex_drop` to free them.

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
/// Returns 1 if it matches, 0 if it does not, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_regex_is_match(re: *mut u8, text_ptr: *const u8, text_len: usize) -> i32 {
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

/// Find the first match of the regex in `text`. Writes the byte-offset start
/// to `*out_start` and the byte length to `*out_len`. Returns 1 on match,
/// 0 if no match, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_regex_find(
    re: *mut u8,
    text_ptr: *const u8,
    text_len: usize,
    out_start: *mut i64,
    out_len: *mut i64,
) -> i32 {
    if re.is_null() || text_ptr.is_null() || out_start.is_null() || out_len.is_null() {
        return -1;
    }
    let re = unsafe { &*(re as *const Regex) };
    let text = unsafe { std::slice::from_raw_parts(text_ptr, text_len) };
    let text = match std::str::from_utf8(text) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    match re.find(text) {
        Some(m) => {
            unsafe {
                *out_start = m.start() as i64;
                *out_len = (m.end() - m.start()) as i64;
            }
            1
        }
        None => 0,
    }
}

/// Find the first match at or after byte offset `start`. Unlike re-slicing the
/// haystack and re-running `find`, anchors keep their meaning relative to the
/// TRUE start of `text` (`^` only matches at offset 0), which is what makes
/// iterated multi-match loops anchor-correct. A `start` that lands inside a
/// multi-byte codepoint (possible after a zero-width-match +1 skip) is rounded
/// up to the next char boundary instead of erroring. Returns 1 on match, 0 if
/// no match, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_regex_find_at(
    re: *mut u8,
    text_ptr: *const u8,
    text_len: usize,
    start: i64,
    out_start: *mut i64,
    out_len: *mut i64,
) -> i32 {
    if re.is_null() || text_ptr.is_null() || out_start.is_null() || out_len.is_null() || start < 0 {
        return -1;
    }
    let re = unsafe { &*(re as *const Regex) };
    let text = unsafe { std::slice::from_raw_parts(text_ptr, text_len) };
    let text = match std::str::from_utf8(text) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let mut s = start as usize;
    if s > text.len() {
        return 0;
    }
    while s < text.len() && !text.is_char_boundary(s) {
        s += 1;
    }
    match re.find_at(text, s) {
        Some(m) => {
            unsafe {
                *out_start = m.start() as i64;
                *out_len = (m.end() - m.start()) as i64;
            }
            1
        }
        None => 0,
    }
}

/// Offset-aware variant of `kryos_regex_capture`: captures of the first match
/// at or after byte offset `start`, anchor-correct like `kryos_regex_find_at`.
/// Writes the ABSOLUTE start (into `text`) and length of capture group
/// `group_idx` (0 = whole match). A group that did not participate writes
/// start=-1, len=0. Returns 1 on match, 0 if no match, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_regex_capture_at(
    re: *mut u8,
    text_ptr: *const u8,
    text_len: usize,
    start: i64,
    group_idx: i64,
    out_start: *mut i64,
    out_len: *mut i64,
) -> i32 {
    if re.is_null()
        || text_ptr.is_null()
        || out_start.is_null()
        || out_len.is_null()
        || start < 0
        || group_idx < 0
    {
        return -1;
    }
    let re = unsafe { &*(re as *const Regex) };
    let text = unsafe { std::slice::from_raw_parts(text_ptr, text_len) };
    let text = match std::str::from_utf8(text) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let mut s = start as usize;
    if s > text.len() {
        return 0;
    }
    while s < text.len() && !text.is_char_boundary(s) {
        s += 1;
    }
    match re.captures_at(text, s) {
        None => 0,
        Some(caps) => {
            if (group_idx as usize) >= caps.len() {
                return -1;
            }
            match caps.get(group_idx as usize) {
                Some(m) => {
                    unsafe {
                        *out_start = m.start() as i64;
                        *out_len = (m.end() - m.start()) as i64;
                    }
                    1
                }
                None => {
                    unsafe {
                        *out_start = -1;
                        *out_len = 0;
                    }
                    1
                }
            }
        }
    }
}

/// Replace all matches of `re` in `text` with `replacement`. Writes the
/// replaced bytes into `out` (caller-provided buffer of capacity `out_cap`).
/// Sets `*needed` to the required byte count. Returns the number of bytes
/// written on success, or -1 if the buffer was too small (consult `*needed`).
#[no_mangle]
pub extern "C" fn kryos_regex_replace_all(
    re: *mut u8,
    text_ptr: *const u8,
    text_len: usize,
    repl_ptr: *const u8,
    repl_len: usize,
    out: *mut u8,
    out_cap: usize,
    needed: *mut i64,
) -> i64 {
    if re.is_null() || text_ptr.is_null() || repl_ptr.is_null() || out.is_null() {
        return -1;
    }
    let re = unsafe { &*(re as *const Regex) };
    let text = unsafe { std::slice::from_raw_parts(text_ptr, text_len) };
    let text = match std::str::from_utf8(text) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let repl = unsafe { std::slice::from_raw_parts(repl_ptr, repl_len) };
    let repl = match std::str::from_utf8(repl) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let result = re.replace_all(text, repl);
    let bytes = result.as_bytes();
    if !needed.is_null() {
        unsafe { *needed = bytes.len() as i64 };
    }
    if bytes.len() > out_cap {
        return -1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len());
    }
    bytes.len() as i64
}

/// Count the number of capture groups defined by the regex, excluding the
/// implicit whole-match group at index 0.
#[no_mangle]
pub extern "C" fn kryos_regex_capture_count(re: *mut u8) -> i64 {
    if re.is_null() {
        return -1;
    }
    let re = unsafe { &*(re as *const Regex) };
    (re.captures_len() - 1) as i64
}

/// Find the first match in `text` and write the start/length of capture group
/// `group_idx` (0 = whole match, 1..N = explicit groups) to `*out_start` and
/// `*out_len`. Returns 1 on match, 0 if no match, -1 on error / out-of-range.
#[no_mangle]
pub extern "C" fn kryos_regex_capture(
    re: *mut u8,
    text_ptr: *const u8,
    text_len: usize,
    group_idx: i64,
    out_start: *mut i64,
    out_len: *mut i64,
) -> i32 {
    if re.is_null() || text_ptr.is_null() || out_start.is_null() || out_len.is_null() {
        return -1;
    }
    let re = unsafe { &*(re as *const Regex) };
    let text = unsafe { std::slice::from_raw_parts(text_ptr, text_len) };
    let text = match std::str::from_utf8(text) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    if group_idx < 0 {
        return -1;
    }
    match re.captures(text) {
        None => 0,
        Some(caps) => {
            if (group_idx as usize) >= caps.len() {
                return -1;
            }
            match caps.get(group_idx as usize) {
                Some(m) => {
                    unsafe {
                        *out_start = m.start() as i64;
                        *out_len = (m.end() - m.start()) as i64;
                    }
                    1
                }
                None => {
                    // Group didn't participate in this match.
                    unsafe {
                        *out_start = -1;
                        *out_len = 0;
                    }
                    1
                }
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn compile(pat: &str) -> *mut u8 {
        kryos_regex_new(pat.as_ptr(), pat.len())
    }

    #[test]
    fn find_first_match() {
        let re = compile(r"\d+");
        let text = "foo 42 bar 99";
        let mut start = 0i64;
        let mut len = 0i64;
        let r = kryos_regex_find(re, text.as_ptr(), text.len(), &mut start, &mut len);
        assert_eq!(r, 1);
        assert_eq!(start, 4);
        assert_eq!(len, 2);
        kryos_regex_drop(re);
    }

    #[test]
    fn replace_all_into_buffer() {
        let re = compile(r"\d+");
        let text = "a1b2c3";
        let repl = "X";
        let mut buf = [0u8; 32];
        let mut needed = 0i64;
        let written = kryos_regex_replace_all(
            re,
            text.as_ptr(),
            text.len(),
            repl.as_ptr(),
            repl.len(),
            buf.as_mut_ptr(),
            buf.len(),
            &mut needed,
        );
        assert!(written > 0);
        let out = std::str::from_utf8(&buf[..written as usize]).unwrap();
        assert_eq!(out, "aXbXcX");
        kryos_regex_drop(re);
    }

    #[test]
    fn find_at_respects_caret_anchor() {
        let re = compile("^a");
        let text = "aaa";
        let (mut s, mut l) = (0i64, 0i64);
        assert_eq!(
            kryos_regex_find_at(re, text.as_ptr(), text.len(), 0, &mut s, &mut l),
            1
        );
        assert_eq!((s, l), (0, 1));
        // ^ must NOT re-match at offset 1 of the same haystack.
        assert_eq!(
            kryos_regex_find_at(re, text.as_ptr(), text.len(), 1, &mut s, &mut l),
            0
        );
        kryos_regex_drop(re);
    }

    #[test]
    fn capture_at_absolute_offsets() {
        let re = compile(r"(\d+)");
        let text = "a1b22c333";
        let (mut s, mut l) = (0i64, 0i64);
        // Second match starting search after the first.
        assert_eq!(
            kryos_regex_capture_at(re, text.as_ptr(), text.len(), 2, 1, &mut s, &mut l),
            1
        );
        assert_eq!((s, l), (3, 2));
        kryos_regex_drop(re);
    }

    #[test]
    fn captures_group() {
        let re = compile(r"(\w+)=(\d+)");
        let text = "x=42";
        assert_eq!(kryos_regex_capture_count(re), 2);
        let (mut s, mut l) = (0i64, 0i64);
        kryos_regex_capture(re, text.as_ptr(), text.len(), 1, &mut s, &mut l);
        assert_eq!((s, l), (0, 1));
        kryos_regex_capture(re, text.as_ptr(), text.len(), 2, &mut s, &mut l);
        assert_eq!((s, l), (2, 2));
        kryos_regex_drop(re);
    }
}
