//! Synchronization primitives for the Kryos native stdlib.
//!
//! Exposes a heap-allocated mutex through opaque pointer handles.
//! The caller is responsible for calling `kryos_mutex_drop` to free the allocation.
//!
//! Implementation is a self-contained atomic spin-then-yield lock. The previous
//! `Mutex<()>` + stored-`MutexGuard`-pointer design was RACY across threads: the
//! guard pointer was a single shared mutable field, and `kryos_mutex_unlock`
//! dropped the guard (releasing the real lock) BEFORE nulling `km.guard`. A
//! second thread blocked in `lock()` could wake in that window, acquire the
//! lock, and store its own guard pointer -- which the first thread's trailing
//! `km.guard = null` then clobbered. The second thread's later unlock saw a
//! null guard, returned -1 ("mutex unlock failed"), and never released the
//! inner lock: a program-wide deadlock under real contention (AtomicInt /
//! WaitGroup / any std::sync primitive on a shared handle from `spawn`ed
//! threads). The atomic lock has no shared mutable pointer -- each lock/unlock
//! is a single atomic read-modify-write, so it is correct for exactly the
//! concurrent same-handle use these primitives exist for.

use std::sync::atomic::{AtomicBool, Ordering};

/// Internal wrapper holding the lock state. `true` = held, `false` = free.
struct KryosMutex {
    locked: AtomicBool,
}

// The atomic is safe to share across threads (that is the whole point).
unsafe impl Send for KryosMutex {}
unsafe impl Sync for KryosMutex {}

/// Creates a new mutex, returning an opaque pointer.
#[no_mangle]
pub extern "C" fn kryos_mutex_new() -> *mut u8 {
    let km = Box::new(KryosMutex {
        locked: AtomicBool::new(false),
    });
    Box::into_raw(km) as *mut u8
}

/// Locks the mutex, blocking (spin-then-yield) until it is free.
///
/// Returns 0 on success, -1 on a null pointer.
#[no_mangle]
pub extern "C" fn kryos_mutex_lock(mutex: *mut u8) -> i32 {
    if mutex.is_null() {
        return -1;
    }
    let km = unsafe { &*(mutex as *mut KryosMutex) };
    // CAS false -> true. Spin briefly, then yield to the OS scheduler so a
    // longer critical section doesn't burn a core (avoids the pure-spin
    // pathology under heavy contention).
    let mut spins: u32 = 0;
    while km
        .locked
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        spins = spins.wrapping_add(1);
        if spins < 64 {
            std::hint::spin_loop();
        } else {
            std::thread::yield_now();
        }
    }
    0
}

/// Unlocks the mutex.
///
/// Returns 0 on success, -1 on a null pointer or if the mutex was not locked
/// (a best-effort double-unlock signal; the release is idempotent).
#[no_mangle]
pub extern "C" fn kryos_mutex_unlock(mutex: *mut u8) -> i32 {
    if mutex.is_null() {
        return -1;
    }
    let km = unsafe { &*(mutex as *mut KryosMutex) };
    // swap returns the previous state; releasing an already-free lock is a
    // caller error but harmless (stays free).
    let was_locked = km.locked.swap(false, Ordering::Release);
    if was_locked {
        0
    } else {
        -1
    }
}

/// Drops (frees) the mutex. The pointer is invalid after this call.
#[no_mangle]
pub extern "C" fn kryos_mutex_drop(mutex: *mut u8) {
    if mutex.is_null() {
        return;
    }
    // Reclaim and drop the box; the atomic needs no explicit release.
    let _ = unsafe { Box::from_raw(mutex as *mut KryosMutex) };
}
