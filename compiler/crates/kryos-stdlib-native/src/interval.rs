//! Interval-set operations: merge overlapping ranges, test containment.
//!
//! Intervals are stored as [start, end) pairs in a flat i64 array of
//! length `2 * count`. The caller maintains sortedness.

/// Merge overlapping/adjacent intervals in `intervals[0..2*count]`
/// (caller already sorted by start). Returns the new count.
#[no_mangle]
pub extern "C" fn kryos_interval_merge(intervals: *mut i64, count: usize) -> i64 {
    if intervals.is_null() || count == 0 {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(intervals, count * 2) };
    let mut w = 1; // write index for next merged interval
    for r in 1..count {
        let rs = s[r * 2];
        let re = s[r * 2 + 1];
        let prev_end = s[(w - 1) * 2 + 1];
        if rs <= prev_end {
            // Overlap → extend prev
            if re > prev_end {
                s[(w - 1) * 2 + 1] = re;
            }
        } else {
            // No overlap → keep
            s[w * 2] = rs;
            s[w * 2 + 1] = re;
            w += 1;
        }
    }
    w as i64
}

/// Does `point` lie within any interval in the sorted set?
/// Binary search; returns 1 if found, 0 otherwise.
#[no_mangle]
pub extern "C" fn kryos_interval_contains(
    intervals: *const i64,
    count: usize,
    point: i64,
) -> i32 {
    if intervals.is_null() || count == 0 {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts(intervals, count * 2) };
    let mut lo = 0usize;
    let mut hi = count;
    while lo < hi {
        let mid = (lo + hi) / 2;
        let start = s[mid * 2];
        let end = s[mid * 2 + 1];
        if point < start {
            hi = mid;
        } else if point >= end {
            lo = mid + 1;
        } else {
            return 1;
        }
    }
    0
}

/// Total length of all intervals (assumed non-overlapping after merge).
#[no_mangle]
pub extern "C" fn kryos_interval_total_length(
    intervals: *const i64,
    count: usize,
) -> i64 {
    if intervals.is_null() || count == 0 {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts(intervals, count * 2) };
    let mut total = 0i64;
    for i in 0..count {
        let span = s[i * 2 + 1] - s[i * 2];
        total = total.saturating_add(span.max(0));
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_overlapping() {
        // [(1,5), (3,7), (10,15)] → [(1,7), (10,15)]
        let mut data = [1i64, 5, 3, 7, 10, 15];
        let n = kryos_interval_merge(data.as_mut_ptr(), 3);
        assert_eq!(n, 2);
        assert_eq!(&data[..4], &[1, 7, 10, 15]);
    }

    #[test]
    fn merge_adjacent() {
        // [(1,5), (5,10)] → [(1,10)]
        let mut data = [1i64, 5, 5, 10];
        let n = kryos_interval_merge(data.as_mut_ptr(), 2);
        assert_eq!(n, 1);
        assert_eq!(&data[..2], &[1, 10]);
    }

    #[test]
    fn contains_point() {
        let data = [1i64, 5, 10, 15, 20, 25];
        assert_eq!(kryos_interval_contains(data.as_ptr(), 3, 3), 1);
        assert_eq!(kryos_interval_contains(data.as_ptr(), 3, 7), 0);
        assert_eq!(kryos_interval_contains(data.as_ptr(), 3, 22), 1);
        assert_eq!(kryos_interval_contains(data.as_ptr(), 3, 100), 0);
    }

    #[test]
    fn total_length() {
        let data = [0i64, 5, 10, 12];
        assert_eq!(kryos_interval_total_length(data.as_ptr(), 2), 7);
    }
}
