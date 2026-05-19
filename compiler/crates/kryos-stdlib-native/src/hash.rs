//! Standalone hash + checksum helpers for Kryos programs.
//!
//! Provides:
//!  - FNV-1a 64-bit (fast non-cryptographic hash)
//!  - DJB2 (legacy string hash)
//!  - CRC32 (IEEE polynomial, used by zip / png / ethernet)
//!
//! For SHA-256 / blake3 etc. enable the `crypto` feature (`std::crypto`).

/// FNV-1a 64-bit hash. Fast and reasonably collision-resistant for
/// non-cryptographic uses (hash maps, bloom filters, content IDs).
#[no_mangle]
pub extern "C" fn kryos_hash_fnv1a64(ptr: *const u8, len: usize) -> i64 {
    if ptr.is_null() {
        return 0;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h as i64
}

/// DJB2 string hash. Slower than FNV-1a but well-known.
#[no_mangle]
pub extern "C" fn kryos_hash_djb2(ptr: *const u8, len: usize) -> i64 {
    if ptr.is_null() {
        return 0;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let mut h: u64 = 5381;
    for &b in bytes {
        h = h.wrapping_mul(33).wrapping_add(b as u64);
    }
    h as i64
}

/// CRC32 (IEEE polynomial). Slow-but-portable bit-by-bit implementation;
/// fast enough for small inputs (<1 MB). For large data, prefer the
/// `crypto` feature's hardware-accelerated variants.
#[no_mangle]
pub extern "C" fn kryos_hash_crc32(ptr: *const u8, len: usize) -> i64 {
    if ptr.is_null() {
        return 0;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let mut crc: u32 = 0xffffffff;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask: u32 = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb88320 & mask);
        }
    }
    (!crc) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a64_known_vectors() {
        // Empty string is the FNV offset basis itself.
        assert_eq!(kryos_hash_fnv1a64(b"".as_ptr(), 0) as u64, 0xcbf29ce484222325);
        // "a" — known fnv1a-64 reference value.
        assert_eq!(kryos_hash_fnv1a64(b"a".as_ptr(), 1) as u64, 0xaf63dc4c8601ec8c);
    }

    #[test]
    fn djb2_deterministic() {
        let a = kryos_hash_djb2(b"hello".as_ptr(), 5);
        let b = kryos_hash_djb2(b"hello".as_ptr(), 5);
        assert_eq!(a, b);
        let c = kryos_hash_djb2(b"world".as_ptr(), 5);
        assert_ne!(a, c);
    }

    #[test]
    fn crc32_known() {
        // CRC32 of "123456789" = 0xCBF43926 (standard checksum vector).
        assert_eq!(
            kryos_hash_crc32(b"123456789".as_ptr(), 9) as u32,
            0xCBF43926
        );
    }
}
