//! Extended string operations that aren't already in the core string module.

/// In-place case-fold a byte buffer to lowercase. ASCII only.
#[no_mangle]
pub extern "C" fn kryos_str_ascii_lower(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    let slice = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
    for b in slice.iter_mut() {
        if (b'A'..=b'Z').contains(b) {
            *b += 32;
        }
    }
}

/// In-place case-fold a byte buffer to uppercase. ASCII only.
#[no_mangle]
pub extern "C" fn kryos_str_ascii_upper(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    let slice = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
    for b in slice.iter_mut() {
        if (b'a'..=b'z').contains(b) {
            *b -= 32;
        }
    }
}

/// Trim ASCII whitespace from both ends of a slice. Writes the trimmed
/// length back into `*out_len` and the new start offset into `*out_start`.
/// Caller can read `bytes[*out_start .. *out_start + *out_len]`.
#[no_mangle]
pub extern "C" fn kryos_str_trim_ascii(
    ptr: *const u8,
    len: usize,
    out_start: *mut i64,
    out_len: *mut i64,
) {
    if ptr.is_null() || out_start.is_null() || out_len.is_null() {
        return;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let mut start = 0;
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    let mut end = bytes.len();
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    unsafe {
        *out_start = start as i64;
        *out_len = (end - start) as i64;
    }
}

/// Count occurrences of `needle` in `haystack`. Returns -1 on null input.
#[no_mangle]
pub extern "C" fn kryos_str_count(
    haystack: *const u8,
    h_len: usize,
    needle: *const u8,
    n_len: usize,
) -> i64 {
    if haystack.is_null() || needle.is_null() || n_len == 0 {
        return -1;
    }
    let h = unsafe { std::slice::from_raw_parts(haystack, h_len) };
    let n = unsafe { std::slice::from_raw_parts(needle, n_len) };
    if n_len > h_len {
        return 0;
    }
    let mut count = 0i64;
    let mut i = 0;
    while i + n.len() <= h.len() {
        if &h[i..i + n.len()] == n {
            count += 1;
            i += n.len();
        } else {
            i += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lower_inplace() {
        let mut s = b"Hello World".to_vec();
        kryos_str_ascii_lower(s.as_mut_ptr(), s.len());
        assert_eq!(&s, b"hello world");
    }

    #[test]
    fn upper_inplace() {
        let mut s = b"Hello World".to_vec();
        kryos_str_ascii_upper(s.as_mut_ptr(), s.len());
        assert_eq!(&s, b"HELLO WORLD");
    }

    #[test]
    fn trim_strips_both_ends() {
        let s = b"   hello   ";
        let mut start = 0i64;
        let mut len = 0i64;
        kryos_str_trim_ascii(s.as_ptr(), s.len(), &mut start, &mut len);
        assert_eq!(start, 3);
        assert_eq!(len, 5);
    }

    #[test]
    fn count_substring() {
        assert_eq!(kryos_str_count(b"abcabcabc".as_ptr(), 9, b"abc".as_ptr(), 3), 3);
        assert_eq!(kryos_str_count(b"abc".as_ptr(), 3, b"xyz".as_ptr(), 3), 0);
        assert_eq!(kryos_str_count(b"aaaa".as_ptr(), 4, b"aa".as_ptr(), 2), 2);
    }
}
