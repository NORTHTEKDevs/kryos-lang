//! Fixed-bucket histogram over caller-owned counters. Buckets are
//! left-edge inclusive, right-edge exclusive.
//!
//! Layout: caller provides `edges[N+1]` (sorted ascending) and
//! `counts[N+1]` (with one underflow + N regular + one overflow slot).

/// Record `value`. Increments the bucket whose edge range contains it.
/// Counts vector layout:
///   counts[0]      — underflow (values < edges[0])
///   counts[1..=N]  — bucket i (values in [edges[i-1], edges[i]))
///   counts[N+1]    — overflow (values >= edges[N])
#[no_mangle]
pub extern "C" fn kryos_hist_record(
    edges: *const i64,
    n_edges: usize,
    counts: *mut i64,
    value: i64,
) {
    if edges.is_null() || counts.is_null() || n_edges == 0 {
        return;
    }
    let e = unsafe { std::slice::from_raw_parts(edges, n_edges) };
    let c = unsafe { std::slice::from_raw_parts_mut(counts, n_edges + 1) };
    if value < e[0] {
        c[0] += 1;
        return;
    }
    // Find the rightmost edge ≤ value via binary search.
    let bucket = match e.binary_search(&value) {
        Ok(i) => i + 1, // exact match falls into bucket starting at edge[i]
        Err(i) => i,    // i is the first index where e[i] > value
    };
    if bucket > n_edges {
        c[n_edges] += 1;
    } else {
        c[bucket] += 1;
    }
}

/// Sum of all counts in a histogram (including under/overflow).
#[no_mangle]
pub extern "C" fn kryos_hist_total(counts: *const i64, n_edges: usize) -> i64 {
    if counts.is_null() || n_edges == 0 {
        return 0;
    }
    let c = unsafe { std::slice::from_raw_parts(counts, n_edges + 1) };
    c.iter().sum()
}

/// Approximate percentile (0..=100) — returns the smallest edge whose
/// cumulative count fraction first exceeds the target. Returns the last
/// edge if the histogram is empty.
#[no_mangle]
pub extern "C" fn kryos_hist_percentile(
    edges: *const i64,
    n_edges: usize,
    counts: *const i64,
    percentile: i64,
) -> i64 {
    if edges.is_null() || counts.is_null() || n_edges == 0 {
        return 0;
    }
    let e = unsafe { std::slice::from_raw_parts(edges, n_edges) };
    let c = unsafe { std::slice::from_raw_parts(counts, n_edges + 1) };
    let total: i64 = c.iter().sum();
    if total == 0 {
        return *e.last().unwrap_or(&0);
    }
    let target = total * percentile / 100;
    let mut cum: i64 = 0;
    // Skip underflow bucket [0]; iterate the N regular buckets [1..=n_edges].
    for i in 1..=n_edges {
        cum += c[i];
        if cum >= target {
            // Bucket i ends at edges[i-1] (since bucket i contains values
            // in [edges[i-1], edges[i])); but for percentile estimation,
            // return the right edge of the bucket.
            return e[(i - 1).min(n_edges - 1)];
        }
    }
    *e.last().unwrap_or(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_record_and_total() {
        let edges = [10i64, 20, 30, 40];
        let mut counts = [0i64; 5]; // n_edges + 1
        for v in [5i64, 15, 25, 35, 99, 25, 25] {
            kryos_hist_record(edges.as_ptr(), 4, counts.as_mut_ptr(), v);
        }
        assert_eq!(kryos_hist_total(counts.as_ptr(), 4), 7);
        // 5 → underflow
        assert_eq!(counts[0], 1);
        // 15 → bucket 1 (edges[0]=10..edges[1]=20)
        assert_eq!(counts[1], 1);
        // 25 (x3) → bucket 2 (edges[1]=20..edges[2]=30)
        assert_eq!(counts[2], 3);
        // 35 → bucket 3 (edges[2]=30..edges[3]=40)
        assert_eq!(counts[3], 1);
        // 99 → overflow (>= edges[3]=40)
        assert_eq!(counts[4], 1);
    }

    #[test]
    fn percentile_p50_p99() {
        let edges = [10i64, 20, 30, 40, 50];
        let mut counts = [0i64; 6];
        // 100 values: 20 in each bucket 1..=5
        for i in 0..100 {
            let v = 10 + (i % 50);
            kryos_hist_record(edges.as_ptr(), 5, counts.as_mut_ptr(), v);
        }
        let p50 = kryos_hist_percentile(edges.as_ptr(), 5, counts.as_ptr(), 50);
        // p50 should be around 30 (middle of the distribution).
        assert!(
            (20..=40).contains(&p50),
            "p50 = {p50}, expected ~30"
        );
    }
}
