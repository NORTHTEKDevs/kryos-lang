//! Running statistics (Welford's algorithm) over caller-owned state.
//! Stable mean + variance without storing samples.
//!
//! State layout: `[count, mean_x1000, m2_x1000, min, max]` (5 i64).
//! All arithmetic in milli-units to avoid float roundoff at FFI.

#[no_mangle]
pub extern "C" fn kryos_stat_init(state: *mut i64) {
    if state.is_null() {
        return;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 5) };
    s[0] = 0;
    s[1] = 0;
    s[2] = 0;
    s[3] = i64::MAX;
    s[4] = i64::MIN;
}

/// Add a sample (i64).
#[no_mangle]
pub extern "C" fn kryos_stat_add(state: *mut i64, x: i64) {
    if state.is_null() {
        return;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 5) };
    s[0] += 1;
    let n = s[0];
    let mean_x1000 = s[1];
    let delta = x.saturating_mul(1000).saturating_sub(mean_x1000);
    // mean += delta / n
    let new_mean = mean_x1000.saturating_add(delta / n);
    s[1] = new_mean;
    let delta2 = x.saturating_mul(1000).saturating_sub(new_mean);
    s[2] = s[2].saturating_add((delta / 1000).saturating_mul(delta2 / 1000));
    if x < s[3] {
        s[3] = x;
    }
    if x > s[4] {
        s[4] = x;
    }
}

#[no_mangle]
pub extern "C" fn kryos_stat_count(state: *const i64) -> i64 {
    if state.is_null() {
        return 0;
    }
    unsafe { *state }
}

#[no_mangle]
pub extern "C" fn kryos_stat_mean_x1000(state: *const i64) -> i64 {
    if state.is_null() {
        return 0;
    }
    unsafe { *(state.add(1)) }
}

#[no_mangle]
pub extern "C" fn kryos_stat_min(state: *const i64) -> i64 {
    if state.is_null() {
        return 0;
    }
    let v = unsafe { *(state.add(3)) };
    if v == i64::MAX {
        0
    } else {
        v
    }
}

#[no_mangle]
pub extern "C" fn kryos_stat_max(state: *const i64) -> i64 {
    if state.is_null() {
        return 0;
    }
    let v = unsafe { *(state.add(4)) };
    if v == i64::MIN {
        0
    } else {
        v
    }
}

/// Sample variance in i64 units (M2 / (n - 1)). Returns 0 for n < 2.
#[no_mangle]
pub extern "C" fn kryos_stat_variance(state: *const i64) -> i64 {
    if state.is_null() {
        return 0;
    }
    let n = unsafe { *state };
    if n < 2 {
        return 0;
    }
    let m2 = unsafe { *(state.add(2)) };
    m2 / (n - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_stats() {
        let mut s = [0i64; 5];
        kryos_stat_init(s.as_mut_ptr());
        for v in [10i64, 20, 30, 40, 50] {
            kryos_stat_add(s.as_mut_ptr(), v);
        }
        assert_eq!(kryos_stat_count(s.as_ptr()), 5);
        assert_eq!(kryos_stat_mean_x1000(s.as_ptr()), 30_000); // mean = 30
        assert_eq!(kryos_stat_min(s.as_ptr()), 10);
        assert_eq!(kryos_stat_max(s.as_ptr()), 50);
    }

    #[test]
    fn empty_min_max_is_zero() {
        let mut s = [0i64; 5];
        kryos_stat_init(s.as_mut_ptr());
        assert_eq!(kryos_stat_min(s.as_ptr()), 0);
        assert_eq!(kryos_stat_max(s.as_ptr()), 0);
    }
}
