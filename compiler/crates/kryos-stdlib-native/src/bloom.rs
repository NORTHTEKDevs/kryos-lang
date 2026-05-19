//! Bloom filter — space-efficient probabilistic set membership test.
//!
//! False positives possible; false negatives never. Sized by the caller
//! (bits = ~10x expected element count for ~1% false-positive rate).
//!
//! Uses FNV-1a + double hashing (two independent hash positions) to
//! avoid coordinating multiple hash functions.

/// Add a byte slice to the filter. The filter's backing buffer is
/// `bits_buf` of `bits_cap` bits (caller allocates as `(bits_cap + 7) / 8`
/// bytes).
#[no_mangle]
pub extern "C" fn kryos_bloom_add(
    bits_buf: *mut u8,
    bits_cap: usize,
    data: *const u8,
    data_len: usize,
) {
    if bits_buf.is_null() || data.is_null() || bits_cap == 0 {
        return;
    }
    let byte_len = (bits_cap + 7) / 8;
    let slice = unsafe { std::slice::from_raw_parts_mut(bits_buf, byte_len) };
    let d = unsafe { std::slice::from_raw_parts(data, data_len) };

    let h1 = fnv1a(d) as usize;
    let h2 = fnv1a_seed(d, 0xDEADBEEF) as usize;
    // 7 probes — typical for ~1% FPR at ~10 bits/element.
    for i in 0usize..7 {
        let idx = h1.wrapping_add(i.wrapping_mul(h2)) % bits_cap;
        slice[idx / 8] |= 1u8 << (idx % 8);
    }
}

/// Test whether `data` is possibly in the filter. Returns 1 = possibly
/// present (false positives possible), 0 = definitely absent.
#[no_mangle]
pub extern "C" fn kryos_bloom_contains(
    bits_buf: *const u8,
    bits_cap: usize,
    data: *const u8,
    data_len: usize,
) -> i32 {
    if bits_buf.is_null() || data.is_null() || bits_cap == 0 {
        return 0;
    }
    let byte_len = (bits_cap + 7) / 8;
    let slice = unsafe { std::slice::from_raw_parts(bits_buf, byte_len) };
    let d = unsafe { std::slice::from_raw_parts(data, data_len) };
    let h1 = fnv1a(d) as usize;
    let h2 = fnv1a_seed(d, 0xDEADBEEF) as usize;
    for i in 0usize..7 {
        let idx = h1.wrapping_add(i.wrapping_mul(h2)) % bits_cap;
        if (slice[idx / 8] >> (idx % 8)) & 1 == 0 {
            return 0;
        }
    }
    1
}

/// Approximate fraction of bits set (load factor). Returns 0..=1000
/// (so callers don't need float math). 600 means 60% loaded.
#[no_mangle]
pub extern "C" fn kryos_bloom_load_ppm(bits_buf: *const u8, bits_cap: usize) -> i64 {
    if bits_buf.is_null() || bits_cap == 0 {
        return 0;
    }
    let byte_len = (bits_cap + 7) / 8;
    let slice = unsafe { std::slice::from_raw_parts(bits_buf, byte_len) };
    let mut set_bits = 0u64;
    for b in slice {
        set_bits += b.count_ones() as u64;
    }
    (set_bits * 1000 / bits_cap as u64) as i64
}

fn fnv1a(bytes: &[u8]) -> u64 {
    fnv1a_seed(bytes, 0xcbf29ce484222325)
}

fn fnv1a_seed(bytes: &[u8], seed: u64) -> u64 {
    let mut h = seed;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_then_contains() {
        let mut bits = vec![0u8; 1024];
        let bits_cap = 1024 * 8;
        for s in &["alice", "bob", "carol"] {
            kryos_bloom_add(bits.as_mut_ptr(), bits_cap, s.as_ptr(), s.len());
        }
        for s in &["alice", "bob", "carol"] {
            assert_eq!(
                kryos_bloom_contains(bits.as_ptr(), bits_cap, s.as_ptr(), s.len()),
                1,
                "expected `{s}` to be present"
            );
        }
    }

    #[test]
    fn missing_likely_absent() {
        let mut bits = vec![0u8; 1024];
        let bits_cap = 1024 * 8;
        for s in &["alice", "bob", "carol"] {
            kryos_bloom_add(bits.as_mut_ptr(), bits_cap, s.as_ptr(), s.len());
        }
        // We can't guarantee a specific missing string returns 0 without
        // larger filter sizes — but most short strings should miss.
        let mut hits = 0;
        let tries = ["x", "y", "z", "qqq", "ttt", "uuu", "vvv", "www"];
        for s in &tries {
            if kryos_bloom_contains(bits.as_ptr(), bits_cap, s.as_ptr(), s.len()) == 1 {
                hits += 1;
            }
        }
        // Most should miss; allow some FPR.
        assert!(hits < tries.len(), "too many false positives");
    }

    #[test]
    fn load_factor_is_reasonable() {
        let mut bits = vec![0u8; 1024];
        let bits_cap = 1024 * 8;
        for i in 0..100 {
            let s = format!("item-{i}");
            kryos_bloom_add(bits.as_mut_ptr(), bits_cap, s.as_ptr(), s.len());
        }
        let load = kryos_bloom_load_ppm(bits.as_ptr(), bits_cap);
        // 100 items × 7 probes / 8192 bits ≈ 85.4‰ before collisions —
        // realistic load after collisions is ~70-85‰.
        assert!(
            (50..=200).contains(&load),
            "unexpected load: {load}"
        );
    }
}
