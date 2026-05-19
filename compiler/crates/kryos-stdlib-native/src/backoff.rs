//! Exponential backoff with jitter for retry loops.
//!
//! Pure: returns the next delay (in milliseconds) given the prior delay
//! and a jitter seed. Caller sleeps + retries.

/// Compute the next exponential-backoff delay.
///   prev_ms = the previous delay (0 for the first call)
///   base_ms = base delay (used for the first call when prev_ms = 0)
///   max_ms  = cap on the returned delay
///   jitter_seed = LCG seed for jitter (any value)
///   jitter_frac_x1000 = +/- jitter as fraction of delay, in parts-per-thousand
///                       (e.g. 200 = ±20% jitter).
///
/// Returns: delay in milliseconds for the next attempt.
#[no_mangle]
pub extern "C" fn kryos_backoff_next(
    prev_ms: i64,
    base_ms: i64,
    max_ms: i64,
    jitter_seed: i64,
    jitter_frac_x1000: i64,
) -> i64 {
    let mut delay = if prev_ms <= 0 {
        base_ms
    } else {
        prev_ms.saturating_mul(2)
    };
    if delay > max_ms {
        delay = max_ms;
    }
    // Apply jitter ±(jitter_frac_x1000 / 1000) using a simple LCG step.
    if jitter_frac_x1000 > 0 && delay > 0 {
        let s = (jitter_seed as u64)
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let r = (s >> 33) as i64;
        // r in [-1000, 1000), then scaled to ±jitter_frac_x1000
        let r_sign = (r % 2001) - 1000;
        let scale = (delay * jitter_frac_x1000 * r_sign) / 1_000_000;
        delay = (delay + scale).max(1);
    }
    delay
}

/// Compute the total cumulative delay if you ran exactly `attempts`
/// retries with the given backoff parameters (no jitter applied).
/// Useful for "how long until I should give up" computations.
#[no_mangle]
pub extern "C" fn kryos_backoff_total(base_ms: i64, max_ms: i64, attempts: i64) -> i64 {
    let mut total = 0i64;
    let mut delay = base_ms;
    let mut i = 0i64;
    while i < attempts {
        total = total.saturating_add(delay);
        delay = delay.saturating_mul(2);
        if delay > max_ms {
            delay = max_ms;
        }
        i += 1;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles_until_cap() {
        let mut d = 0i64;
        let base = 100;
        let cap = 1000;
        let mut seq = Vec::new();
        for _ in 0..6 {
            d = kryos_backoff_next(d, base, cap, 0, 0);
            seq.push(d);
        }
        // 100, 200, 400, 800, 1000 (cap), 1000
        assert_eq!(seq, vec![100, 200, 400, 800, 1000, 1000]);
    }

    #[test]
    fn total_grows_correctly() {
        // 5 attempts at base 100, cap 1000:
        // 100 + 200 + 400 + 800 + 1000 = 2500
        assert_eq!(kryos_backoff_total(100, 1000, 5), 2500);
    }

    #[test]
    fn jitter_changes_value() {
        let no_jitter = kryos_backoff_next(0, 1000, 10000, 0, 0);
        let with_jitter = kryos_backoff_next(0, 1000, 10000, 42, 200);
        // 20% jitter on 1000 = ±200ms, so jittered should be in [800, 1200]
        assert!((800..=1200).contains(&with_jitter));
        assert_eq!(no_jitter, 1000);
    }
}
