//! Pseudorandom number generation for Kryos.
//!
//! Wraps a thread-local splitmix64 + xorshift64 chain. Not cryptographic
//! — for that enable the `crypto` feature (`std::crypto::rand_bytes`).
//! Deterministic via `kryos_random_seed`.

use std::cell::Cell;

thread_local! {
    /// Per-thread state. Seeded lazily from time/pid on first use.
    static STATE: Cell<u64> = const { Cell::new(0) };
}

fn ensure_seeded() {
    STATE.with(|s| {
        if s.get() == 0 {
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0xdeadbeef);
            s.set(t ^ (std::process::id() as u64));
        }
    });
}

fn next_u64() -> u64 {
    ensure_seeded();
    STATE.with(|s| {
        // splitmix64
        let mut z = s.get().wrapping_add(0x9E3779B97F4A7C15);
        let prev_z = z;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^= z >> 31;
        s.set(prev_z);
        z
    })
}

/// Seed the per-thread generator with `seed`. Subsequent calls are
/// deterministic until reseeded. Passing 0 reverts to time-based seeding.
#[no_mangle]
pub extern "C" fn kryos_random_seed(seed: i64) {
    STATE.with(|s| s.set(seed as u64));
}

/// Random i64 (full range).
#[no_mangle]
pub extern "C" fn kryos_random_i64() -> i64 {
    next_u64() as i64
}

/// Random i64 uniformly in [min, max). Returns min on empty range.
#[no_mangle]
pub extern "C" fn kryos_random_range(min: i64, max: i64) -> i64 {
    if max <= min {
        return min;
    }
    let range = (max - min) as u64;
    min + (next_u64() % range) as i64
}

/// Random f64 uniformly in [0.0, 1.0).
#[no_mangle]
pub extern "C" fn kryos_random_f64() -> f64 {
    // 53 bits of randomness in the mantissa.
    let v = next_u64() >> 11;
    (v as f64) / ((1u64 << 53) as f64)
}

/// Fill `buf` with random bytes (non-cryptographic). Use std::crypto::
/// rand_bytes for cryptographic randomness.
#[no_mangle]
pub extern "C" fn kryos_random_fill(buf: *mut u8, len: usize) {
    if buf.is_null() || len == 0 {
        return;
    }
    let slice = unsafe { std::slice::from_raw_parts_mut(buf, len) };
    let mut i = 0;
    while i < len {
        let r = next_u64().to_le_bytes();
        let take = (len - i).min(8);
        slice[i..i + take].copy_from_slice(&r[..take]);
        i += take;
    }
}

/// Shuffle an i64 slice in-place (Fisher-Yates).
#[no_mangle]
pub extern "C" fn kryos_random_shuffle_i64(ptr: *mut i64, len: usize) {
    if ptr.is_null() || len < 2 {
        return;
    }
    let slice = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
    for i in (1..slice.len()).rev() {
        let j = (next_u64() % (i as u64 + 1)) as usize;
        slice.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_is_deterministic() {
        kryos_random_seed(42);
        let a = kryos_random_i64();
        kryos_random_seed(42);
        let b = kryos_random_i64();
        assert_eq!(a, b);
    }

    #[test]
    fn range_respects_bounds() {
        kryos_random_seed(1);
        for _ in 0..100 {
            let v = kryos_random_range(10, 20);
            assert!(v >= 10 && v < 20);
        }
    }

    #[test]
    fn f64_in_unit_range() {
        kryos_random_seed(1);
        for _ in 0..100 {
            let v = kryos_random_f64();
            assert!((0.0..1.0).contains(&v));
        }
    }

    #[test]
    fn shuffle_changes_order() {
        kryos_random_seed(123);
        let mut v = (0..100i64).collect::<Vec<_>>();
        let orig = v.clone();
        kryos_random_shuffle_i64(v.as_mut_ptr(), v.len());
        assert_ne!(v, orig);
        let mut sorted = v.clone();
        sorted.sort();
        assert_eq!(sorted, (0..100i64).collect::<Vec<_>>());
    }
}
