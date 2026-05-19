//! LRU cache over caller-owned storage.
//!
//! Backed by parallel arrays: `keys[cap]`, `vals[cap]`, and a `recency[cap]`
//! index (lower = more recently used). Cap is fixed at init; on insert
//! into a full cache, the entry with highest recency value is evicted.
//!
//! Lookups are O(N) (linear scan over keys). For larger N use a hash map
//! with intrusive doubly-linked list — coming in a future release.

/// Initialize a new LRU. State layout: `(len, cap, next_recency)`.
#[no_mangle]
pub extern "C" fn kryos_lru_init(state: *mut i64, cap: i64) {
    if state.is_null() {
        return;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 3) };
    s[0] = 0;
    s[1] = cap;
    s[2] = 0;
}

/// Insert or update `(k, v)`. Evicts the least-recently used entry if
/// the cache is full and `k` isn't already present.
#[no_mangle]
pub extern "C" fn kryos_lru_put(
    keys: *mut i64,
    vals: *mut i64,
    recency: *mut i64,
    state: *mut i64,
    k: i64,
    v: i64,
) {
    if keys.is_null() || vals.is_null() || recency.is_null() || state.is_null() {
        return;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 3) };
    let mut len = s[0] as usize;
    let cap = s[1] as usize;
    s[2] += 1;
    let touch = s[2];
    let ks = unsafe { std::slice::from_raw_parts_mut(keys, cap) };
    let vs = unsafe { std::slice::from_raw_parts_mut(vals, cap) };
    let rs = unsafe { std::slice::from_raw_parts_mut(recency, cap) };

    // Update existing
    for i in 0..len {
        if ks[i] == k {
            vs[i] = v;
            rs[i] = touch;
            return;
        }
    }

    // Insert new — evict if full
    let slot = if len < cap {
        len += 1;
        s[0] = len as i64;
        len - 1
    } else {
        // Find the entry with the lowest recency (LRU).
        let mut min_idx = 0;
        for i in 1..len {
            if rs[i] < rs[min_idx] {
                min_idx = i;
            }
        }
        min_idx
    };
    ks[slot] = k;
    vs[slot] = v;
    rs[slot] = touch;
}

/// Get the value for `k`. Returns 1 on hit (writes to `*out`), 0 on miss.
#[no_mangle]
pub extern "C" fn kryos_lru_get(
    keys: *const i64,
    vals: *const i64,
    recency: *mut i64,
    state: *mut i64,
    k: i64,
    out: *mut i64,
) -> i32 {
    if keys.is_null() || vals.is_null() || recency.is_null() || state.is_null() || out.is_null() {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 3) };
    let len = s[0] as usize;
    let cap = s[1] as usize;
    s[2] += 1;
    let touch = s[2];
    let ks = unsafe { std::slice::from_raw_parts(keys, cap) };
    let vs = unsafe { std::slice::from_raw_parts(vals, cap) };
    let rs = unsafe { std::slice::from_raw_parts_mut(recency, cap) };
    for i in 0..len {
        if ks[i] == k {
            rs[i] = touch;
            unsafe {
                *out = vs[i];
            }
            return 1;
        }
    }
    0
}

/// Returns the number of entries currently in the cache.
#[no_mangle]
pub extern "C" fn kryos_lru_len(state: *const i64) -> i64 {
    if state.is_null() {
        return 0;
    }
    unsafe { *state }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_put_get() {
        let mut keys = [0i64; 4];
        let mut vals = [0i64; 4];
        let mut recency = [0i64; 4];
        let mut state = [0i64; 3];
        kryos_lru_init(state.as_mut_ptr(), 4);

        kryos_lru_put(keys.as_mut_ptr(), vals.as_mut_ptr(), recency.as_mut_ptr(), state.as_mut_ptr(), 1, 100);
        kryos_lru_put(keys.as_mut_ptr(), vals.as_mut_ptr(), recency.as_mut_ptr(), state.as_mut_ptr(), 2, 200);

        let mut out = 0i64;
        assert_eq!(
            kryos_lru_get(keys.as_ptr(), vals.as_ptr(), recency.as_mut_ptr(), state.as_mut_ptr(), 1, &mut out),
            1
        );
        assert_eq!(out, 100);
    }

    #[test]
    fn evicts_least_recently_used() {
        let mut keys = [0i64; 2];
        let mut vals = [0i64; 2];
        let mut recency = [0i64; 2];
        let mut state = [0i64; 3];
        kryos_lru_init(state.as_mut_ptr(), 2);

        // Fill
        kryos_lru_put(keys.as_mut_ptr(), vals.as_mut_ptr(), recency.as_mut_ptr(), state.as_mut_ptr(), 1, 100);
        kryos_lru_put(keys.as_mut_ptr(), vals.as_mut_ptr(), recency.as_mut_ptr(), state.as_mut_ptr(), 2, 200);
        // Touch 1 (now 2 is LRU)
        let mut out = 0i64;
        kryos_lru_get(keys.as_ptr(), vals.as_ptr(), recency.as_mut_ptr(), state.as_mut_ptr(), 1, &mut out);
        // Insert 3 — should evict 2
        kryos_lru_put(keys.as_mut_ptr(), vals.as_mut_ptr(), recency.as_mut_ptr(), state.as_mut_ptr(), 3, 300);
        // Verify
        assert_eq!(
            kryos_lru_get(keys.as_ptr(), vals.as_ptr(), recency.as_mut_ptr(), state.as_mut_ptr(), 2, &mut out),
            0,
            "2 should have been evicted"
        );
        assert_eq!(
            kryos_lru_get(keys.as_ptr(), vals.as_ptr(), recency.as_mut_ptr(), state.as_mut_ptr(), 1, &mut out),
            1
        );
        assert_eq!(out, 100);
    }
}
