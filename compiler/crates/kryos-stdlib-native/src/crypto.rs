//! Cryptographic operations for the Kryos native stdlib.
//!
//! Uses `ring::digest` for SHA-256/SHA-512 and `ring::rand` for secure random bytes.

use ring::digest;
use ring::rand::{SecureRandom, SystemRandom};

/// Computes the SHA-256 hash of `data[0..len]` and writes the 32-byte result to `out`.
///
/// `out` must point to at least 32 bytes of writable memory.
/// Returns 0 on success, -1 on error (null pointers).
#[no_mangle]
pub extern "C" fn kryos_sha256(data: *const u8, len: usize, out: *mut u8) -> i32 {
    if data.is_null() || out.is_null() {
        return -1;
    }
    let input = unsafe { std::slice::from_raw_parts(data, len) };
    let hash = digest::digest(&digest::SHA256, input);
    let result = hash.as_ref();
    let out_slice = unsafe { std::slice::from_raw_parts_mut(out, 32) };
    out_slice.copy_from_slice(result);
    0
}

/// Computes the SHA-512 hash of `data[0..len]` and writes the 64-byte result to `out`.
///
/// `out` must point to at least 64 bytes of writable memory.
/// Returns 0 on success, -1 on error (null pointers).
#[no_mangle]
pub extern "C" fn kryos_sha512(data: *const u8, len: usize, out: *mut u8) -> i32 {
    if data.is_null() || out.is_null() {
        return -1;
    }
    let input = unsafe { std::slice::from_raw_parts(data, len) };
    let hash = digest::digest(&digest::SHA512, input);
    let result = hash.as_ref();
    let out_slice = unsafe { std::slice::from_raw_parts_mut(out, 64) };
    out_slice.copy_from_slice(result);
    0
}

/// Computes the SHA-1 hash of `data[0..len]` and writes the 20-byte result to `out`.
///
/// SHA-1 is included for legacy protocols (WebSocket handshake, etc.); it is
/// not cryptographically secure for new constructions. `out` must point to at
/// least 20 bytes of writable memory.
#[no_mangle]
pub extern "C" fn kryos_sha1(data: *const u8, len: usize, out: *mut u8) -> i32 {
    if data.is_null() || out.is_null() {
        return -1;
    }
    let input = unsafe { std::slice::from_raw_parts(data, len) };
    let digest = sha1_compute(input);
    let out_slice = unsafe { std::slice::from_raw_parts_mut(out, 20) };
    out_slice.copy_from_slice(&digest);
    0
}

fn sha1_compute(msg: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let bit_len = (msg.len() as u64).wrapping_mul(8);
    // Pad: append 0x80, then zeros until length % 64 == 56, then 8-byte big-endian length.
    let mut padded = Vec::with_capacity(msg.len() + 72);
    padded.extend_from_slice(msg);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (i, &word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// Fills `buf[0..len]` with cryptographically secure random bytes.
///
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_random_bytes(buf: *mut u8, len: usize) -> i32 {
    if buf.is_null() || len == 0 {
        return -1;
    }
    let slice = unsafe { std::slice::from_raw_parts_mut(buf, len) };
    let rng = SystemRandom::new();
    match rng.fill(slice) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}
