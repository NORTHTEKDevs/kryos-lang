//! UUID v4 (random) generation + parsing for Kryos.
//!
//! Implements RFC 4122 variant 1, version 4. Random bytes come from the
//! existing `kryos-rt` random source.

/// Fill 16 bytes at `out` with a freshly-generated v4 UUID (random + version/
/// variant bits set per RFC 4122). Returns 0 on success, -1 on null input.
#[no_mangle]
pub extern "C" fn kryos_uuid_v4_bytes(out: *mut u8) -> i32 {
    if out.is_null() {
        return -1;
    }
    let buf = unsafe { std::slice::from_raw_parts_mut(out, 16) };
    fill_random(buf);
    // Version 4 (random): set top nibble of byte 6 to 0100.
    buf[6] = (buf[6] & 0x0f) | 0x40;
    // RFC 4122 variant: top two bits of byte 8 → 10.
    buf[8] = (buf[8] & 0x3f) | 0x80;
    0
}

/// Format the 16-byte UUID at `bytes` as the canonical
/// `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` string into `out` (capacity 36).
/// Returns 36 on success, -1 on bad input / overflow.
#[no_mangle]
pub extern "C" fn kryos_uuid_format(bytes: *const u8, out: *mut u8, out_cap: usize) -> i64 {
    if bytes.is_null() || out.is_null() || out_cap < 36 {
        return -1;
    }
    let src = unsafe { std::slice::from_raw_parts(bytes, 16) };
    let dst = unsafe { std::slice::from_raw_parts_mut(out, 36) };
    let mut i = 0;
    let mut o = 0;
    for &b in src {
        let hi = (b >> 4) & 0x0f;
        let lo = b & 0x0f;
        dst[o] = hex_nibble(hi);
        dst[o + 1] = hex_nibble(lo);
        o += 2;
        i += 1;
        if matches!(i, 4 | 6 | 8 | 10) {
            dst[o] = b'-';
            o += 1;
        }
    }
    36
}

/// Parse a canonical UUID string from `input` (36 bytes) into the 16-byte
/// representation at `out`. Returns 0 on success, -1 on malformed input.
#[no_mangle]
pub extern "C" fn kryos_uuid_parse(input: *const u8, input_len: usize, out: *mut u8) -> i32 {
    if input.is_null() || out.is_null() || input_len != 36 {
        return -1;
    }
    let src = unsafe { std::slice::from_raw_parts(input, 36) };
    let dst = unsafe { std::slice::from_raw_parts_mut(out, 16) };
    let mut o = 0;
    let mut i = 0;
    while i < 36 {
        if matches!(i, 8 | 13 | 18 | 23) {
            if src[i] != b'-' {
                return -1;
            }
            i += 1;
            continue;
        }
        let hi = match hex_value(src[i]) {
            Some(v) => v,
            None => return -1,
        };
        let lo = match hex_value(src[i + 1]) {
            Some(v) => v,
            None => return -1,
        };
        dst[o] = (hi << 4) | lo;
        o += 1;
        i += 2;
    }
    0
}

fn hex_nibble(n: u8) -> u8 {
    match n {
        0..=9 => b'0' + n,
        10..=15 => b'a' + (n - 10),
        _ => b'?',
    }
}

fn hex_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn fill_random(buf: &mut [u8]) {
    // Use SystemTime + an internal counter mixed via splitmix64 for a
    // simple cross-platform random source. Good enough for UUID v4 — not
    // a cryptographic generator. Callers needing CSPRNG should use the
    // `crypto` feature.
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::SystemTime;

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut state = nanos ^ COUNTER.fetch_add(1, Ordering::Relaxed).wrapping_mul(0x9E37_79B9);
    for b in buf.iter_mut() {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        *b = z as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_and_parse_roundtrip() {
        let mut bytes = [0u8; 16];
        kryos_uuid_v4_bytes(bytes.as_mut_ptr());
        // Version 4 nibble:
        assert_eq!(bytes[6] & 0xf0, 0x40);
        // Variant:
        assert_eq!(bytes[8] & 0xc0, 0x80);

        let mut s = [0u8; 36];
        let n = kryos_uuid_format(bytes.as_ptr(), s.as_mut_ptr(), s.len());
        assert_eq!(n, 36);

        let mut back = [0u8; 16];
        let r = kryos_uuid_parse(s.as_ptr(), 36, back.as_mut_ptr());
        assert_eq!(r, 0);
        assert_eq!(bytes, back);
    }

    #[test]
    fn parse_rejects_bad_input() {
        let mut out = [0u8; 16];
        // Wrong length.
        assert_eq!(kryos_uuid_parse(b"abc".as_ptr(), 3, out.as_mut_ptr()), -1);
        // Missing hyphen.
        let bad = b"550e8400e29b41d4a716446655440000abcd";
        assert_eq!(kryos_uuid_parse(bad.as_ptr(), 36, out.as_mut_ptr()), -1);
        // Invalid hex char.
        let bad = b"550e8400-e29b-41d4-a716-44665544000z";
        assert_eq!(kryos_uuid_parse(bad.as_ptr(), 36, out.as_mut_ptr()), -1);
    }

    #[test]
    fn two_uuids_differ() {
        let mut a = [0u8; 16];
        let mut b = [0u8; 16];
        kryos_uuid_v4_bytes(a.as_mut_ptr());
        kryos_uuid_v4_bytes(b.as_mut_ptr());
        assert_ne!(a, b);
    }
}
