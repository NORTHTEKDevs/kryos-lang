//! UTF-8 helpers that are awkward to write in Kryos directly.

/// Count Unicode code points in a UTF-8 buffer. Returns -1 on invalid UTF-8.
#[no_mangle]
pub extern "C" fn kryos_utf8_codepoint_count(buf: *const u8, len: usize) -> i64 {
    if buf.is_null() {
        return 0;
    }
    let slice = unsafe { std::slice::from_raw_parts(buf, len) };
    match std::str::from_utf8(slice) {
        Ok(s) => s.chars().count() as i64,
        Err(_) => -1,
    }
}

/// Validate UTF-8. Returns 1 if valid, 0 if not.
#[no_mangle]
pub extern "C" fn kryos_utf8_is_valid(buf: *const u8, len: usize) -> i32 {
    if buf.is_null() {
        return 0;
    }
    let slice = unsafe { std::slice::from_raw_parts(buf, len) };
    if std::str::from_utf8(slice).is_ok() {
        1
    } else {
        0
    }
}

/// Compute the byte length of the UTF-8 encoding of `codepoint` (1..=4).
/// Returns -1 if the codepoint is invalid (> 0x10FFFF or surrogate).
#[no_mangle]
pub extern "C" fn kryos_utf8_byte_len(codepoint: i64) -> i64 {
    let cp = codepoint as u32;
    if cp > 0x10FFFF || (0xD800..=0xDFFF).contains(&cp) {
        return -1;
    }
    if cp < 0x80 {
        1
    } else if cp < 0x800 {
        2
    } else if cp < 0x10000 {
        3
    } else {
        4
    }
}

/// Encode `codepoint` into `out` (capacity ≥ 4). Returns bytes written
/// or -1 on invalid codepoint / short buffer.
#[no_mangle]
pub extern "C" fn kryos_utf8_encode(codepoint: i64, out: *mut u8, out_cap: usize) -> i64 {
    let cp = codepoint as u32;
    if out.is_null() || cp > 0x10FFFF || (0xD800..=0xDFFF).contains(&cp) {
        return -1;
    }
    let c = match char::from_u32(cp) {
        Some(c) => c,
        None => return -1,
    };
    let mut tmp = [0u8; 4];
    let s = c.encode_utf8(&mut tmp);
    let bytes = s.as_bytes();
    if bytes.len() > out_cap {
        return -1;
    }
    let dst = unsafe { std::slice::from_raw_parts_mut(out, bytes.len()) };
    dst.copy_from_slice(bytes);
    bytes.len() as i64
}

/// Returns the byte offset of the Nth code point. -1 if out of range.
#[no_mangle]
pub extern "C" fn kryos_utf8_byte_offset(buf: *const u8, len: usize, codepoint_idx: i64) -> i64 {
    if buf.is_null() || codepoint_idx < 0 {
        return -1;
    }
    let slice = unsafe { std::slice::from_raw_parts(buf, len) };
    let s = match std::str::from_utf8(slice) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let target = codepoint_idx as usize;
    let mut count = 0;
    for (byte_idx, _) in s.char_indices() {
        if count == target {
            return byte_idx as i64;
        }
        count += 1;
    }
    if count == target {
        return s.len() as i64;
    }
    -1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codepoint_count_basic() {
        assert_eq!(kryos_utf8_codepoint_count(b"hello".as_ptr(), 5), 5);
        let s = "héllo"; // 5 chars, 6 bytes (é is 2 bytes)
        assert_eq!(kryos_utf8_codepoint_count(s.as_ptr(), s.len()), 5);
    }

    #[test]
    fn is_valid_rejects_garbage() {
        assert_eq!(kryos_utf8_is_valid(b"hello".as_ptr(), 5), 1);
        // 0xFF is not valid UTF-8 by itself.
        assert_eq!(kryos_utf8_is_valid([0xFFu8].as_ptr(), 1), 0);
    }

    #[test]
    fn byte_len_branches() {
        assert_eq!(kryos_utf8_byte_len(b'A' as i64), 1);
        assert_eq!(kryos_utf8_byte_len(0xE9), 2);     // é
        assert_eq!(kryos_utf8_byte_len(0x2603), 3);   // ☃
        assert_eq!(kryos_utf8_byte_len(0x1F600), 4);  // 😀
        assert_eq!(kryos_utf8_byte_len(0x110000), -1); // beyond max
        assert_eq!(kryos_utf8_byte_len(0xD800), -1);  // surrogate
    }

    #[test]
    fn encode_emoji() {
        let mut buf = [0u8; 4];
        let n = kryos_utf8_encode(0x1F600, buf.as_mut_ptr(), 4);
        assert_eq!(n, 4);
        assert_eq!(&buf, b"\xF0\x9F\x98\x80");
    }

    #[test]
    fn byte_offset_walks_chars() {
        let s = "héllo";
        // h=0, é=1 (2 bytes), l=3, l=4, o=5
        assert_eq!(kryos_utf8_byte_offset(s.as_ptr(), s.len(), 0), 0);
        assert_eq!(kryos_utf8_byte_offset(s.as_ptr(), s.len(), 1), 1);
        assert_eq!(kryos_utf8_byte_offset(s.as_ptr(), s.len(), 2), 3);
        assert_eq!(kryos_utf8_byte_offset(s.as_ptr(), s.len(), 5), 6);
    }
}
