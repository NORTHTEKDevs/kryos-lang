//! Token-bucket rate limiter over caller-owned state.
//!
//! State layout `(capacity, tokens_x1000, refill_per_sec_x1000, last_refill_nanos)`.
//! Stored as i64 for ABI cleanliness; the x1000 fields are fixed-point
//! milli-tokens so we don't need floats at the FFI boundary.

/// Initialize a new rate limiter: `capacity` tokens max, refills at
/// `refill_per_sec` tokens/second. `now_nanos` is the current time.
#[no_mangle]
pub extern "C" fn kryos_ratelimit_init(
    state: *mut i64,
    capacity: i64,
    refill_per_sec: i64,
    now_nanos: i64,
) {
    if state.is_null() {
        return;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 4) };
    s[0] = capacity;
    s[1] = capacity.saturating_mul(1000);
    s[2] = refill_per_sec.saturating_mul(1000);
    s[3] = now_nanos;
}

/// Try to consume one token at the given time. Returns 1 if allowed (token
/// consumed), 0 if rate-limited (no token available).
#[no_mangle]
pub extern "C" fn kryos_ratelimit_try_acquire(state: *mut i64, now_nanos: i64) -> i32 {
    if state.is_null() {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 4) };
    refill(s, now_nanos);
    if s[1] >= 1000 {
        s[1] -= 1000;
        1
    } else {
        0
    }
}

/// Return the current token count (rounded down). Doesn't refill.
#[no_mangle]
pub extern "C" fn kryos_ratelimit_tokens(state: *const i64) -> i64 {
    if state.is_null() {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts(state, 4) };
    s[1] / 1000
}

fn refill(s: &mut [i64], now: i64) {
    let elapsed = now.saturating_sub(s[3]);
    if elapsed <= 0 {
        return;
    }
    // tokens added (x1000) = (refill_per_sec_x1000 * elapsed) / 1_000_000_000
    let add = s[2].saturating_mul(elapsed).saturating_div(1_000_000_000);
    let max_x1000 = s[0].saturating_mul(1000);
    s[1] = (s[1].saturating_add(add)).min(max_x1000);
    s[3] = now;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_full() {
        let mut s = [0i64; 4];
        kryos_ratelimit_init(s.as_mut_ptr(), 5, 10, 0);
        assert_eq!(kryos_ratelimit_tokens(s.as_ptr()), 5);
    }

    #[test]
    fn try_acquire_drains_then_blocks() {
        let mut s = [0i64; 4];
        kryos_ratelimit_init(s.as_mut_ptr(), 3, 10, 0);
        assert_eq!(kryos_ratelimit_try_acquire(s.as_mut_ptr(), 0), 1);
        assert_eq!(kryos_ratelimit_try_acquire(s.as_mut_ptr(), 0), 1);
        assert_eq!(kryos_ratelimit_try_acquire(s.as_mut_ptr(), 0), 1);
        // Drained at time 0.
        assert_eq!(kryos_ratelimit_try_acquire(s.as_mut_ptr(), 0), 0);
    }

    #[test]
    fn refills_over_time() {
        let mut s = [0i64; 4];
        kryos_ratelimit_init(s.as_mut_ptr(), 5, 10, 0); // 10 tokens/sec
        // Drain
        for _ in 0..5 {
            kryos_ratelimit_try_acquire(s.as_mut_ptr(), 0);
        }
        // 200ms later, should have ~2 tokens.
        let new_t = 200_000_000i64;
        assert_eq!(kryos_ratelimit_try_acquire(s.as_mut_ptr(), new_t), 1);
        assert_eq!(kryos_ratelimit_try_acquire(s.as_mut_ptr(), new_t), 1);
        // No more at this time.
        assert_eq!(kryos_ratelimit_try_acquire(s.as_mut_ptr(), new_t), 0);
    }

    #[test]
    fn caps_at_capacity() {
        let mut s = [0i64; 4];
        kryos_ratelimit_init(s.as_mut_ptr(), 3, 10, 0);
        // 1 second later, would refill 10 tokens — but cap is 3.
        let later = 1_000_000_000i64;
        kryos_ratelimit_try_acquire(s.as_mut_ptr(), later);
        assert_eq!(kryos_ratelimit_tokens(s.as_ptr()), 2);
    }
}
