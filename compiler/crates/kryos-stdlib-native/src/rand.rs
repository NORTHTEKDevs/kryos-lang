//! Pseudo-random number generation for the Kryos native stdlib.
//!
//! Provides a simple thread-local LCG (Linear Congruential Generator) for random number generation.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

// Global RNG state: LCG with parameters from MINSTD
static RNG_STATE: AtomicU64 = AtomicU64::new(0);
static RNG_INIT: Mutex<bool> = Mutex::new(false);

fn init_rng() {
    let mut initialized = RNG_INIT.lock().unwrap();
    if !*initialized {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(12345);
        RNG_STATE.store(seed.max(1), Ordering::SeqCst);
        *initialized = true;
    }
}

fn next_u64() -> u64 {
    init_rng();
    loop {
        let old = RNG_STATE.load(Ordering::SeqCst);
        // LCG: x' = (a*x + c) mod 2^64
        // Using parameters from glibc/POSIX
        let new = old.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        match RNG_STATE.compare_exchange(old, new, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return new,
            Err(_) => continue,
        }
    }
}

/// Returns a random f64 in the range [0.0, 1.0).
#[no_mangle]
pub extern "C" fn kryos_rand_f64() -> f64 {
    let u = next_u64();
    // Convert upper bits to [0.0, 1.0)
    ((u >> 11) as f64) * (1.0 / 9007199254740992.0)
}

/// Returns a random i64 in the range [lo, hi).
#[no_mangle]
pub extern "C" fn kryos_rand_i64(lo: i64, hi: i64) -> i64 {
    if lo >= hi {
        return lo;
    }
    let range = (hi - lo) as u64;
    let u = next_u64() % range;
    lo + (u as i64)
}

/// Returns a random boolean (0 or 1).
#[no_mangle]
pub extern "C" fn kryos_rand_bool() -> i32 {
    if (next_u64() & 1) == 0 { 0 } else { 1 }
}

/// Seeds the global RNG.
#[no_mangle]
pub extern "C" fn kryos_rand_seed(seed: u64) {
    RNG_STATE.store(seed.max(1), Ordering::SeqCst);
    let mut initialized = RNG_INIT.lock().unwrap();
    *initialized = true;
}

/// Fills a buffer with random bytes.
#[no_mangle]
pub extern "C" fn kryos_rand_bytes(buf: *mut u8, len: usize) {
    if buf.is_null() || len == 0 {
        return;
    }
    unsafe {
        let slice = std::slice::from_raw_parts_mut(buf, len);
        let mut i = 0;
        while i < len {
            let u = next_u64();
            let bytes = u.to_le_bytes();
            let remaining = len - i;
            let to_copy = remaining.min(8);
            slice[i..i + to_copy].copy_from_slice(&bytes[..to_copy]);
            i += to_copy;
        }
    }
}
