//! Byte-slice operations: search, compare, fill, copy. Works on raw
//! `*const u8` / `*mut u8` buffers; doesn't allocate.

/// Find the first byte in `buf` matching `needle`. Returns the index or -1.
#[no_mangle]
pub extern "C" fn kryos_bytes_find_byte(buf: *const u8, len: usize, needle: u8) -> i64 {
    if buf.is_null() {
        return -1;
    }
    let slice = unsafe { std::slice::from_raw_parts(buf, len) };
    match slice.iter().position(|&b| b == needle) {
        Some(i) => i as i64,
        None => -1,
    }
}

/// Find the first occurrence of the byte sequence `needle` in `haystack`.
/// Returns the byte offset or -1.
#[no_mangle]
pub extern "C" fn kryos_bytes_find_seq(
    haystack: *const u8,
    h_len: usize,
    needle: *const u8,
    n_len: usize,
) -> i64 {
    if haystack.is_null() || needle.is_null() || n_len == 0 || n_len > h_len {
        return -1;
    }
    let h = unsafe { std::slice::from_raw_parts(haystack, h_len) };
    let n = unsafe { std::slice::from_raw_parts(needle, n_len) };
    for i in 0..=(h.len() - n.len()) {
        if &h[i..i + n.len()] == n {
            return i as i64;
        }
    }
    -1
}

/// Compare two byte slices lexicographically. Returns -1 if a<b, 0 if equal, 1 if a>b.
#[no_mangle]
pub extern "C" fn kryos_bytes_compare(
    a: *const u8,
    a_len: usize,
    b: *const u8,
    b_len: usize,
) -> i32 {
    if a.is_null() || b.is_null() {
        return 0;
    }
    let sa = unsafe { std::slice::from_raw_parts(a, a_len) };
    let sb = unsafe { std::slice::from_raw_parts(b, b_len) };
    match sa.cmp(sb) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Fill `buf` with `value`.
#[no_mangle]
pub extern "C" fn kryos_bytes_fill(buf: *mut u8, len: usize, value: u8) {
    if buf.is_null() {
        return;
    }
    let slice = unsafe { std::slice::from_raw_parts_mut(buf, len) };
    slice.fill(value);
}

/// Returns 1 if all bytes in `buf` are ASCII (< 128), 0 otherwise.
#[no_mangle]
pub extern "C" fn kryos_bytes_is_ascii(buf: *const u8, len: usize) -> i32 {
    if buf.is_null() {
        return 1;
    }
    let slice = unsafe { std::slice::from_raw_parts(buf, len) };
    if slice.iter().all(|&b| b < 128) {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_byte_present() {
        assert_eq!(kryos_bytes_find_byte(b"hello".as_ptr(), 5, b'l'), 2);
        assert_eq!(kryos_bytes_find_byte(b"hello".as_ptr(), 5, b'x'), -1);
    }

    #[test]
    fn find_seq() {
        assert_eq!(
            kryos_bytes_find_seq(b"abcdefgh".as_ptr(), 8, b"def".as_ptr(), 3),
            3
        );
        assert_eq!(
            kryos_bytes_find_seq(b"abcdefgh".as_ptr(), 8, b"xyz".as_ptr(), 3),
            -1
        );
    }

    #[test]
    fn compare_lexicographic() {
        assert_eq!(
            kryos_bytes_compare(b"abc".as_ptr(), 3, b"abd".as_ptr(), 3),
            -1
        );
        assert_eq!(
            kryos_bytes_compare(b"abc".as_ptr(), 3, b"abc".as_ptr(), 3),
            0
        );
        assert_eq!(
            kryos_bytes_compare(b"abd".as_ptr(), 3, b"abc".as_ptr(), 3),
            1
        );
    }

    #[test]
    fn fill_writes_value() {
        let mut buf = [0u8; 4];
        kryos_bytes_fill(buf.as_mut_ptr(), buf.len(), b'X');
        assert_eq!(&buf, b"XXXX");
    }

    #[test]
    fn ascii_detection() {
        assert_eq!(kryos_bytes_is_ascii(b"hello".as_ptr(), 5), 1);
        let non_ascii = [b'h', b'i', 0xC3, 0xA9];
        assert_eq!(
            kryos_bytes_is_ascii(non_ascii.as_ptr(), non_ascii.len()),
            0
        );
    }
}
