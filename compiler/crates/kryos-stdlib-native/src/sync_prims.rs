//! Synchronization primitives for the Kryos native stdlib.
//!
//! Exposes a heap-allocated mutex through opaque pointer handles.
//! The caller is responsible for calling `kryos_mutex_drop` to free the allocation.
//!
//! Implementation uses `Mutex<()>` with the `MutexGuard` stored as a raw pointer
//! so that lock/unlock can be paired across FFI boundaries.

use std::sync::{Mutex, MutexGuard};

/// Internal wrapper holding both the mutex and the currently-held guard (if locked).
struct KryosMutex {
    inner: Mutex<()>,
    /// When locked, this holds a pointer to a heap-allocated MutexGuard.
    /// When unlocked, this is null.
    guard: *mut MutexGuard<'static, ()>,
}

// The guard pointer is only accessed through the kryos_mutex_lock / kryos_mutex_unlock
// functions which take a mutable (exclusive) reference via the opaque pointer contract.
// Therefore cross-thread send/sync is safe as long as callers don't race on the same handle.
unsafe impl Send for KryosMutex {}
unsafe impl Sync for KryosMutex {}

/// Creates a new mutex, returning an opaque pointer.
///
/// Returns null on allocation failure (extremely unlikely).
#[no_mangle]
pub extern "C" fn kryos_mutex_new() -> *mut u8 {
    let km = Box::new(KryosMutex {
        inner: Mutex::new(()),
        guard: std::ptr::null_mut(),
    });
    Box::into_raw(km) as *mut u8
}

/// Locks the mutex.
///
/// Returns 0 on success, -1 on error (null pointer or poisoned mutex).
#[no_mangle]
pub extern "C" fn kryos_mutex_lock(mutex: *mut u8) -> i32 {
    if mutex.is_null() {
        return -1;
    }
    let km = unsafe { &mut *(mutex as *mut KryosMutex) };
    match km.inner.lock() {
        Ok(guard) => {
            // Transmute the guard's lifetime to 'static so we can store it.
            // Safety: the guard will live until kryos_mutex_unlock or kryos_mutex_drop,
            // both of which take the same pointer and therefore the same KryosMutex.
            let guard: MutexGuard<'static, ()> = unsafe { std::mem::transmute(guard) };
            km.guard = Box::into_raw(Box::new(guard));
            0
        }
        Err(_) => -1,
    }
}

/// Unlocks the mutex.
///
/// The caller must ensure the mutex was previously locked by `kryos_mutex_lock`.
///
/// Returns 0 on success, -1 on error (null pointer or not locked).
#[no_mangle]
pub extern "C" fn kryos_mutex_unlock(mutex: *mut u8) -> i32 {
    if mutex.is_null() {
        return -1;
    }
    let km = unsafe { &mut *(mutex as *mut KryosMutex) };
    if km.guard.is_null() {
        return -1; // not locked
    }
    // Drop the guard, which releases the lock.
    unsafe {
        let _ = Box::from_raw(km.guard);
    }
    km.guard = std::ptr::null_mut();
    0
}

/// Drops (frees) the mutex.
///
/// If the mutex is still locked, the guard is dropped first.
/// After this call, the pointer is invalid and must not be used.
#[no_mangle]
pub extern "C" fn kryos_mutex_drop(mutex: *mut u8) {
    if mutex.is_null() {
        return;
    }
    let km = unsafe { *Box::from_raw(mutex as *mut KryosMutex) };
    // Drop the guard if still held.
    if !km.guard.is_null() {
        unsafe {
            let _ = Box::from_raw(km.guard);
        }
    }
    // km (and its inner Mutex) is dropped here.
}
