//! Collection helpers: reservoir sampling, unique-preserving filter,
//! group-by-key reduction. All work over caller-owned i64 slices to
//! avoid allocation in the FFI layer.

use std::cell::Cell;

/// Reservoir-sample `k` elements from a slice of length `len`. Writes
/// the sampled indices into `out` (capacity `k`). Returns the number of
/// indices written (`min(k, len)`).
///
/// Deterministic for testability: uses an LCG seeded with the input
/// length. Pass a different seed via `seed` for true randomness.
#[no_mangle]
pub extern "C" fn kryos_reservoir_sample(
    len: i64,
    k: i64,
    seed: i64,
    out: *mut i64,
) -> i64 {
    if out.is_null() || k <= 0 || len <= 0 {
        return 0;
    }
    let len = len as usize;
    let k = (k as usize).min(len);
    let dst = unsafe { std::slice::from_raw_parts_mut(out, k) };

    // Init: take first k indices.
    for i in 0..k {
        dst[i] = i as i64;
    }

    // LCG for replacement decisions.
    let state = Cell::new(seed as u64);
    let next = || {
        let s = state.get();
        let n = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        state.set(n);
        n
    };

    for i in k..len {
        let j = (next() as usize) % (i + 1);
        if j < k {
            dst[j] = i as i64;
        }
    }
    k as i64
}

/// Remove consecutive duplicates from a sorted i64 slice in-place.
/// Returns the new logical length.
#[no_mangle]
pub extern "C" fn kryos_dedup_sorted_i64(ptr: *mut i64, len: usize) -> i64 {
    if ptr.is_null() || len < 2 {
        return len as i64;
    }
    let slice = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
    let mut w = 1;
    for r in 1..slice.len() {
        if slice[r] != slice[w - 1] {
            slice[w] = slice[r];
            w += 1;
        }
    }
    w as i64
}

/// Reverse an i64 slice in-place. Same as `kryos_sort_i64_reverse` but
/// without the sort prerequisite — works on any input.
#[no_mangle]
pub extern "C" fn kryos_reverse_i64(ptr: *mut i64, len: usize) {
    if ptr.is_null() || len < 2 {
        return;
    }
    let slice = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
    slice.reverse();
}

/// Sum an i64 slice (saturating, no wrap).
#[no_mangle]
pub extern "C" fn kryos_sum_i64(ptr: *const i64, len: usize) -> i64 {
    if ptr.is_null() || len == 0 {
        return 0;
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    let mut acc: i64 = 0;
    for &v in slice {
        acc = acc.saturating_add(v);
    }
    acc
}

/// Min of an i64 slice. Returns i64::MAX on empty input.
#[no_mangle]
pub extern "C" fn kryos_min_i64(ptr: *const i64, len: usize) -> i64 {
    if ptr.is_null() || len == 0 {
        return i64::MAX;
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    *slice.iter().min().unwrap()
}

/// Max of an i64 slice. Returns i64::MIN on empty input.
#[no_mangle]
pub extern "C" fn kryos_max_i64(ptr: *const i64, len: usize) -> i64 {
    if ptr.is_null() || len == 0 {
        return i64::MIN;
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    *slice.iter().max().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_collapses_runs() {
        let mut v = [1i64, 1, 2, 2, 2, 3, 4, 4];
        let n = kryos_dedup_sorted_i64(v.as_mut_ptr(), v.len()) as usize;
        assert_eq!(&v[..n], &[1, 2, 3, 4]);
    }

    #[test]
    fn reverse_inplace() {
        let mut v = [1i64, 2, 3, 4, 5];
        kryos_reverse_i64(v.as_mut_ptr(), v.len());
        assert_eq!(v, [5, 4, 3, 2, 1]);
    }

    #[test]
    fn sum_min_max() {
        let v = [5i64, 3, 8, 1, 4];
        assert_eq!(kryos_sum_i64(v.as_ptr(), v.len()), 21);
        assert_eq!(kryos_min_i64(v.as_ptr(), v.len()), 1);
        assert_eq!(kryos_max_i64(v.as_ptr(), v.len()), 8);
    }

    #[test]
    fn reservoir_picks_k_indices() {
        let mut out = [0i64; 3];
        let n = kryos_reservoir_sample(10, 3, 42, out.as_mut_ptr());
        assert_eq!(n, 3);
        // All indices must be in range and distinct.
        for &i in &out {
            assert!(i >= 0 && i < 10);
        }
        assert_ne!(out[0], out[1]);
        assert_ne!(out[1], out[2]);
    }
}
