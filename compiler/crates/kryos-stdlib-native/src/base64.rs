//! Base64 (RFC 4648, standard alphabet) encode / decode for Kryos.
//!
//! No external dependency — small enough to ship inline. Both encoders write
//! into caller-provided buffers with a `needed` out parameter on overflow.

const ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode `input_len` bytes from `input` into `out` (capacity `out_cap`).
/// Returns the number of bytes written, or -1 on overflow with `*needed`
/// set to the required size.
#[no_mangle]
pub extern "C" fn kryos_base64_encode(
    input: *const u8,
    input_len: usize,
    out: *mut u8,
    out_cap: usize,
    needed: *mut i64,
) -> i64 {
    if input.is_null() || out.is_null() {
        return -1;
    }
    let req = ((input_len + 2) / 3) * 4;
    if !needed.is_null() {
        unsafe { *needed = req as i64 };
    }
    if req > out_cap {
        return -1;
    }
    let src = unsafe { std::slice::from_raw_parts(input, input_len) };
    let dst = unsafe { std::slice::from_raw_parts_mut(out, req) };
    let mut i = 0;
    let mut j = 0;
    while i + 3 <= src.len() {
        let b0 = src[i] as u32;
        let b1 = src[i + 1] as u32;
        let b2 = src[i + 2] as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        dst[j] = ALPHABET[((n >> 18) & 0x3f) as usize];
        dst[j + 1] = ALPHABET[((n >> 12) & 0x3f) as usize];
        dst[j + 2] = ALPHABET[((n >> 6) & 0x3f) as usize];
        dst[j + 3] = ALPHABET[(n & 0x3f) as usize];
        i += 3;
        j += 4;
    }
    let rem = src.len() - i;
    if rem == 1 {
        let b0 = src[i] as u32;
        let n = b0 << 16;
        dst[j] = ALPHABET[((n >> 18) & 0x3f) as usize];
        dst[j + 1] = ALPHABET[((n >> 12) & 0x3f) as usize];
        dst[j + 2] = b'=';
        dst[j + 3] = b'=';
    } else if rem == 2 {
        let b0 = src[i] as u32;
        let b1 = src[i + 1] as u32;
        let n = (b0 << 16) | (b1 << 8);
        dst[j] = ALPHABET[((n >> 18) & 0x3f) as usize];
        dst[j + 1] = ALPHABET[((n >> 12) & 0x3f) as usize];
        dst[j + 2] = ALPHABET[((n >> 6) & 0x3f) as usize];
        dst[j + 3] = b'=';
    }
    req as i64
}

fn decode_value(c: u8) -> i16 {
    match c {
        b'A'..=b'Z' => (c - b'A') as i16,
        b'a'..=b'z' => (c - b'a' + 26) as i16,
        b'0'..=b'9' => (c - b'0' + 52) as i16,
        b'+' => 62,
        b'/' => 63,
        b'=' => -1, // padding
        _ => -2,    // invalid
    }
}

/// Decode `input_len` bytes of base64 text from `input` into `out`. Returns the
/// number of bytes written. On a buffer that's too small returns -1 and sets
/// `*needed` to the actual byte count required. Returns -2 on invalid base64
/// input.
#[no_mangle]
pub extern "C" fn kryos_base64_decode(
    input: *const u8,
    input_len: usize,
    out: *mut u8,
    out_cap: usize,
    needed: *mut i64,
) -> i64 {
    if input.is_null() || out.is_null() {
        return -1;
    }
    let src = unsafe { std::slice::from_raw_parts(input, input_len) };

    let mut buf = [0i16; 4];
    let mut bi = 0;
    let mut written = 0usize;
    let dst = unsafe { std::slice::from_raw_parts_mut(out, out_cap) };

    for &c in src {
        if c == b'\n' || c == b'\r' || c == b' ' {
            continue;
        }
        let v = decode_value(c);
        if v == -2 {
            return -2;
        }
        buf[bi] = v;
        bi += 1;
        if bi == 4 {
            let pad = (buf[2] == -1) as usize + (buf[3] == -1) as usize;
            let b0 = buf[0].max(0) as u32;
            let b1 = buf[1].max(0) as u32;
            let b2 = buf[2].max(0) as u32;
            let b3 = buf[3].max(0) as u32;
            let n = (b0 << 18) | (b1 << 12) | (b2 << 6) | b3;
            let produce = 3 - pad;
            if written + produce > out_cap {
                if !needed.is_null() {
                    unsafe { *needed = (written + produce) as i64 };
                }
                return -1;
            }
            dst[written] = ((n >> 16) & 0xff) as u8;
            if pad < 2 {
                dst[written + 1] = ((n >> 8) & 0xff) as u8;
            }
            if pad < 1 {
                dst[written + 2] = (n & 0xff) as u8;
            }
            written += produce;
            bi = 0;
        }
    }
    if bi != 0 {
        return -2;
    }
    if !needed.is_null() {
        unsafe { *needed = written as i64 };
    }
    written as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(src: &[u8]) -> Vec<u8> {
        let mut buf = vec![0u8; ((src.len() + 2) / 3) * 4];
        let mut needed = 0i64;
        let n = kryos_base64_encode(
            src.as_ptr(),
            src.len(),
            buf.as_mut_ptr(),
            buf.len(),
            &mut needed,
        );
        assert!(n > 0);
        buf.truncate(n as usize);
        buf
    }

    #[test]
    fn known_vectors() {
        assert_eq!(round(b"f"), b"Zg==");
        assert_eq!(round(b"fo"), b"Zm8=");
        assert_eq!(round(b"foo"), b"Zm9v");
        assert_eq!(round(b"foob"), b"Zm9vYg==");
        assert_eq!(round(b"fooba"), b"Zm9vYmE=");
        assert_eq!(round(b"foobar"), b"Zm9vYmFy");
    }

    #[test]
    fn roundtrip() {
        let original = b"the quick brown fox jumps over 13 lazy dogs!";
        let enc = round(original);
        let mut dec_buf = vec![0u8; original.len()];
        let mut needed = 0i64;
        let n = kryos_base64_decode(
            enc.as_ptr(),
            enc.len(),
            dec_buf.as_mut_ptr(),
            dec_buf.len(),
            &mut needed,
        );
        assert_eq!(n as usize, original.len());
        assert_eq!(&dec_buf[..n as usize], original);
    }
}
