//! Counting semaphore over caller-owned state via an atomic counter.
//!
//! Layout: a single i64 holding the available permits. Acquire fails
//! immediately if no permits are available (this is a non-blocking
//! semaphore — for blocking waits use `sync_prims::sem_wait` once that
//! lands).
//!
//! Concurrency: each FFI call performs a compare-and-swap loop over the
//! permit count. Safe to call from multiple Kryos threads simultaneously.

use std::sync::atomic::{AtomicI64, Ordering};

fn as_atomic(p: *mut i64) -> &'static AtomicI64 {
    unsafe { &*(p as *const AtomicI64) }
}

/// Initialize the semaphore to `permits` available slots.
#[no_mangle]
pub extern "C" fn kryos_sem_init(state: *mut i64, permits: i64) {
    if state.is_null() {
        return;
    }
    as_atomic(state).store(permits.max(0), Ordering::SeqCst);
}

/// Try to acquire one permit without blocking. Returns 1 on success, 0 if
/// none were available.
#[no_mangle]
pub extern "C" fn kryos_sem_try_acquire(state: *mut i64) -> i32 {
    if state.is_null() {
        return 0;
    }
    let a = as_atomic(state);
    let mut current = a.load(Ordering::SeqCst);
    while current > 0 {
        match a.compare_exchange(
            current,
            current - 1,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => return 1,
            Err(actual) => current = actual,
        }
    }
    0
}

/// Release one permit.
#[no_mangle]
pub extern "C" fn kryos_sem_release(state: *mut i64) {
    if state.is_null() {
        return;
    }
    as_atomic(state).fetch_add(1, Ordering::SeqCst);
}

/// Read the current permit count (non-atomically; for telemetry only).
#[no_mangle]
pub extern "C" fn kryos_sem_permits(state: *const i64) -> i64 {
    if state.is_null() {
        return 0;
    }
    unsafe { (*(state as *const AtomicI64)).load(Ordering::Relaxed) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_and_drain() {
        let mut s = 0i64;
        kryos_sem_init(&mut s, 3);
        assert_eq!(kryos_sem_permits(&s), 3);
        for _ in 0..3 {
            assert_eq!(kryos_sem_try_acquire(&mut s), 1);
        }
        assert_eq!(kryos_sem_try_acquire(&mut s), 0);
        kryos_sem_release(&mut s);
        assert_eq!(kryos_sem_try_acquire(&mut s), 1);
    }

    #[test]
    fn release_above_initial_capacity() {
        // Semaphore allows releasing more than initial permits — typical
        // for counting semaphores. Behavior matches POSIX sem_post.
        let mut s = 0i64;
        kryos_sem_init(&mut s, 1);
        kryos_sem_release(&mut s);
        kryos_sem_release(&mut s);
        assert_eq!(kryos_sem_permits(&s), 3);
    }
}
