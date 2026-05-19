//! Circuit breaker — wraps a flaky downstream call with three states:
//! CLOSED (passing), OPEN (fast-fail), HALF_OPEN (probing).
//!
//! State layout (i64 array, length 6):
//!   [0] state:        0=closed, 1=open, 2=half_open
//!   [1] failures:     consecutive failures since last success
//!   [2] threshold:    failures before transitioning closed → open
//!   [3] reset_nanos:  duration before open → half_open trial
//!   [4] opened_at:    nanos timestamp when we opened (0 if not open)
//!   [5] last_now:     last passed-in `now_nanos` (for time accounting)

pub const STATE_CLOSED: i64 = 0;
pub const STATE_OPEN: i64 = 1;
pub const STATE_HALF_OPEN: i64 = 2;

/// Initialize. `threshold` failures will open the breaker; after
/// `reset_nanos` of being open, it goes to half-open for one trial call.
#[no_mangle]
pub extern "C" fn kryos_cb_init(state: *mut i64, threshold: i64, reset_nanos: i64) {
    if state.is_null() {
        return;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 6) };
    s[0] = STATE_CLOSED;
    s[1] = 0;
    s[2] = threshold;
    s[3] = reset_nanos;
    s[4] = 0;
    s[5] = 0;
}

/// Check whether the next call should proceed.
/// Returns: 1 = allow, 0 = fail-fast.
#[no_mangle]
pub extern "C" fn kryos_cb_allow(state: *mut i64, now_nanos: i64) -> i32 {
    if state.is_null() {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 6) };
    s[5] = now_nanos;
    match s[0] {
        STATE_CLOSED => 1,
        STATE_OPEN => {
            // Check if cooldown elapsed → half_open.
            if now_nanos.saturating_sub(s[4]) >= s[3] {
                s[0] = STATE_HALF_OPEN;
                1
            } else {
                0
            }
        }
        STATE_HALF_OPEN => 1,
        _ => 0,
    }
}

/// Record a successful call. Resets the failure counter and closes the
/// breaker.
#[no_mangle]
pub extern "C" fn kryos_cb_record_success(state: *mut i64) {
    if state.is_null() {
        return;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 6) };
    s[0] = STATE_CLOSED;
    s[1] = 0;
}

/// Record a failed call. After `threshold` consecutive failures, opens
/// the breaker. In half-open state, a single failure also opens it.
#[no_mangle]
pub extern "C" fn kryos_cb_record_failure(state: *mut i64) {
    if state.is_null() {
        return;
    }
    let s = unsafe { std::slice::from_raw_parts_mut(state, 6) };
    if s[0] == STATE_HALF_OPEN {
        s[0] = STATE_OPEN;
        s[4] = s[5];
        return;
    }
    s[1] += 1;
    if s[1] >= s[2] {
        s[0] = STATE_OPEN;
        s[4] = s[5];
    }
}

/// Read the current state (0/1/2).
#[no_mangle]
pub extern "C" fn kryos_cb_state(state: *const i64) -> i64 {
    if state.is_null() {
        return STATE_OPEN;
    }
    unsafe { *state }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_after_threshold() {
        let mut s = [0i64; 6];
        kryos_cb_init(s.as_mut_ptr(), 3, 1_000_000_000);
        for _ in 0..3 {
            assert_eq!(kryos_cb_allow(s.as_mut_ptr(), 0), 1);
            kryos_cb_record_failure(s.as_mut_ptr());
        }
        assert_eq!(kryos_cb_state(s.as_ptr()), STATE_OPEN);
        assert_eq!(kryos_cb_allow(s.as_mut_ptr(), 100), 0);
    }

    #[test]
    fn closes_on_success() {
        let mut s = [0i64; 6];
        kryos_cb_init(s.as_mut_ptr(), 3, 1_000_000_000);
        kryos_cb_record_failure(s.as_mut_ptr());
        kryos_cb_record_failure(s.as_mut_ptr());
        kryos_cb_record_success(s.as_mut_ptr());
        assert_eq!(kryos_cb_state(s.as_ptr()), STATE_CLOSED);
    }

    #[test]
    fn half_open_on_cooldown_expiry() {
        let mut s = [0i64; 6];
        kryos_cb_init(s.as_mut_ptr(), 1, 1_000_000_000);
        kryos_cb_record_failure(s.as_mut_ptr()); // opens
        // Now state is OPEN; before cooldown should fail-fast.
        assert_eq!(kryos_cb_allow(s.as_mut_ptr(), 0), 0);
        // After cooldown, allow once → half_open.
        assert_eq!(kryos_cb_allow(s.as_mut_ptr(), 2_000_000_000), 1);
        assert_eq!(kryos_cb_state(s.as_ptr()), STATE_HALF_OPEN);
        // A success closes it again.
        kryos_cb_record_success(s.as_mut_ptr());
        assert_eq!(kryos_cb_state(s.as_ptr()), STATE_CLOSED);
    }

    #[test]
    fn half_open_failure_reopens() {
        let mut s = [0i64; 6];
        kryos_cb_init(s.as_mut_ptr(), 1, 100);
        kryos_cb_record_failure(s.as_mut_ptr());
        kryos_cb_allow(s.as_mut_ptr(), 1000); // half_open
        kryos_cb_record_failure(s.as_mut_ptr());
        assert_eq!(kryos_cb_state(s.as_ptr()), STATE_OPEN);
    }
}
