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
