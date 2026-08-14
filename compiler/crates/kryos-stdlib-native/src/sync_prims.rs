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

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use kryos_rt::panic::kryos_panic;

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

// ---------------------------------------------------------------------------
// Self-reentry detection for `std::sync::Mutex` (LEDGER item 31, PERMANENT
// HANG). `Mutex.lock()`/`.unlock()` (compiler/stdlib/sync.kry) are pure
// value-return methods (`fn lock(self: Mutex) -> Mutex { ... }`) -- the
// documented usage reassigns (`mu = mu.lock()`). Nothing enforces that: a
// bare `mu.lock()` statement compiles clean, silently discards the returned
// `Mutex{locked:true}`, and genuinely locks the REAL native mutex below.
// A second `mu.lock()` on that same never-reassigned binding then issues a
// SECOND real lock against an already-held, plain CAS-spin lock with no
// owner-thread tracking -- spinning forever, 100% of one core, with no
// diagnostic distinguishing it from any other hang. Same root shape as the
// closure-capture lock's self-reentrancy hazard above (LEDGER item 11(a)),
// so the same fix: track which mutex addresses THIS thread currently holds
// and refuse re-entry with a loud `kryos_panic` instead of spinning. A
// DIFFERENT thread genuinely contending for the same address is unaffected
// -- this table is thread-local, so cross-thread mutual exclusion (spin
// until the OTHER thread's unlock) is unchanged; only a same-thread
// double-lock (the only way this hang class occurs) is refused. Deliberately
// a SEPARATE table from `HELD_CLOSURE_LOCKS`: the two locks are logically
// unrelated (one is a user-facing stdlib type, the other a codegen-inserted
// closure-call serializer) even though `kryos_closure_lock_acquire` happens
// to call through this same `kryos_mutex_lock` primitive -- its own
// reentrancy check already runs and panics first in that case, so this
// table is simply populated/cleared alongside it there, a no-op in
// observable behavior for that path.
thread_local! {
    /// Set of mutex addresses this THREAD currently holds via
    /// `kryos_mutex_lock`. Cleared on a matching `kryos_mutex_unlock` (the
    /// normal release path) and unconditionally on `kryos_mutex_drop` (so a
    /// freed-and-reused address, e.g. from a tight `mutex_new()`/`drop()`
    /// loop, never inherits a stale entry from an unrelated prior mutex).
    static HELD_MUTEX_LOCKS: RefCell<HashMap<usize, ()>> = RefCell::new(HashMap::new());
}

/// Locks the mutex, blocking (spin-then-yield) until it is free.
///
/// Returns 0 on success, -1 on a null pointer. PANICS instead of spinning if
/// the CURRENT thread already holds this exact mutex (a same-thread
/// double-lock with no intervening unlock -- LEDGER item 31: this is the
/// only way `std::sync::Mutex` could hang forever with zero diagnostic).
#[no_mangle]
pub extern "C" fn kryos_mutex_lock(mutex: *mut u8) -> i32 {
    if mutex.is_null() {
        return -1;
    }
    let key = mutex as usize;
    let already_held = HELD_MUTEX_LOCKS.with(|m| {
        let mut m = m.borrow_mut();
        if m.contains_key(&key) {
            true
        } else {
            m.insert(key, ());
            false
        }
    });
    if already_held {
        let msg = "deadlock: this thread already holds this std::sync::Mutex -- \
            Mutex.lock() is non-reentrant, so a second lock() on the same handle \
            without an intervening unlock() would spin forever with no diagnostic. \
            A common cause: `mu.lock()` called as a bare statement without \
            reassigning (`mu = mu.lock()`), so `Mutex.unlock()`'s bookkeeping never \
            runs and the real lock is never released.";
        kryos_panic(msg.as_ptr(), msg.len());
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
    let key = mutex as usize;
    HELD_MUTEX_LOCKS.with(|m| {
        m.borrow_mut().remove(&key);
    });
    if was_locked {
        0
    } else {
        -1
    }
}

// ---------------------------------------------------------------------------
// Self-reentry-detecting closure-call lock (LEDGER item 11(a) /
// spawn-throw-and-reentrancy task).
//
// `kryos_mutex_lock`/`unlock` above back TWO independent surfaces: the
// user-facing `std::sync::Mutex` (where non-reentrant, self-deadlock-on-
// double-lock is the normal, expected contract -- same as a Rust/C mutex)
// AND the codegen-inserted serialization lock that wraps every call to a
// MUTATING closure (LEDGER item 7b, closing a spawn-shared data race --
// `MirAttributes::needs_capture_lock` is set for ANY closure with a
// mutated capture, not only spawn-shared ones, so this is reachable with
// zero threads). That second use is compiler-inserted, invisible to the
// user, and keyed on one lock word per closure ENV (offset
// `(num_captures+1)*8`, wired in kryos-codegen-cranelift/codegen.rs and
// the LLVM equivalent). A closure that reaches ITSELF through its own
// stored value (e.g. stashed in a map or struct field it also reads)
// re-enters that SAME lock word on the SAME thread before the outer call
// returns -- with a bare CAS spin lock that has no owner-thread tracking,
// the inner `kryos_mutex_lock` spins forever against a lock the current
// thread already holds: a permanent, unrecoverable hang with no timeout
// (`attack_closure_lock_reentrant_deadlock.kry`, LEDGER item 11(a),
// CONFIRMED).
//
// TWO fix shapes were considered (per this task's own brief: "make the
// lock reentrant, OR detect self-reentry and produce a clean error"):
//
// 1. Silently reentrant (allow the inner call through). REJECTED after
//    measuring it: the closure's mutated capture is a boxed heap cell that
//    is DEREF'd once at each call's ENTRY into a local, mutated locally,
//    and STORED BACK only right before that call's own RETURN (LEDGER
//    item 7's `StoreDeref`-before-`Return` mechanism). A reentrant nested
//    call's entry-deref runs BEFORE the outer call's own store-back (the
//    outer call is still in progress, waiting on the nested call to
//    return) -- so the nested call always reads the STALE pre-outer-call
//    value, not the outer call's in-flight mutation. Verified live:
//    `attack_closure_lock_reentrant_deadlock.kry`'s `f(3)` (three levels
//    of self-reentrancy, each incrementing a shared counter) printed
//    `result: 1` under a working silently-reentrant lock, not the `3` a
//    genuinely-live shared mutable local would produce -- a SILENT WRONG
//    ANSWER, not a crash, and arguably worse than the hang it would
//    replace (a hang is at least loud). Making the write-back eager
//    (store through the pointer at every mutation, not just before
//    return) would fix this properly, but is a materially bigger codegen
//    change with its own correctness surface to re-prove against item 7's
//    existing guarantees -- out of scope for this task; not attempted.
// 2. Detect self-reentry, fail loudly (CHOSEN). A thread that already
//    holds a given closure's lock word is refused re-entry with
//    `kryos_panic` -- the same "kryos panic: <msg>", stack-trace-printing,
//    process-exit(98) path every other unrecoverable Kryos runtime fault
//    (div-by-zero, out-of-bounds, ...) already uses, per CLAUDE.md's
//    documented panic contract. This can never produce the silent wrong
//    answer above (the reentrant call's body never runs), and is
//    consistent with this project's standing preference for a clean,
//    attributable rejection over a silent miscomputation. Not currently
//    catchable via `try`/`catch` (panics aren't, by design, same as every
//    other panic) -- a program that legitimately needs a mutating closure
//    to call itself should restructure to a named recursive function, or
//    keep the recursive state in a separate, non-closure-captured local.
//
// A DIFFERENT thread contending for the same lock address still blocks on
// the genuine atomic exactly as before -- cross-thread mutual exclusion
// (the property item 7b exists for) is unchanged; only same-thread
// self-reentry is refused. This is deliberately a SEPARATE entry point
// from `kryos_mutex_lock`/`unlock` rather than changing those globally:
// the user-facing `Mutex` keeps its normal (non-reentrant, silently-
// deadlocks-on-purpose-double-lock) contract; only the compiler's own
// invisible serialization lock gains this detection.
thread_local! {
    /// Set of lock addresses (the closure env's lock-word pointer, as an
    /// integer key) this THREAD currently holds. A key present means this
    /// thread is mid-call through that closure's thunk; a second acquire
    /// of the same key on the same thread is self-reentry.
    static HELD_CLOSURE_LOCKS: RefCell<HashMap<usize, ()>> = RefCell::new(HashMap::new());
}

/// Acquire the closure-call serialization lock at `mutex`. If the CURRENT
/// thread already holds this exact lock address (a mutating closure
/// reaching itself through its own stored value, directly or indirectly,
/// before its outer call returns), this is a fatal, unrecoverable error --
/// reported and the process exits via `kryos_panic`, matching every other
/// Kryos runtime-fault contract, instead of spinning forever against a
/// lock this thread already holds (see the module doc comment above for
/// why "silently allow it" was tried and rejected).
///
/// Returns 0 on success, -1 on a null pointer. Never returns on detected
/// self-reentry (`kryos_panic` is `-> !`).
#[no_mangle]
pub extern "C" fn kryos_closure_lock_acquire(mutex: *mut u8) -> i32 {
    if mutex.is_null() {
        return -1;
    }
    let key = mutex as usize;
    let already_held = HELD_CLOSURE_LOCKS.with(|m| {
        let mut m = m.borrow_mut();
        if m.contains_key(&key) {
            true
        } else {
            m.insert(key, ());
            false
        }
    });
    if already_held {
        let msg = "reentrant call into a mutating shared closure: this closure \
            mutates its own captured state and cannot call itself, directly or \
            indirectly, while a call on the SAME thread is already in progress \
            (e.g. via a handle to itself stashed in a map or struct field it \
            also reads). Restructure as a named recursive function, or keep \
            the recursive state outside the closure's captures.";
        kryos_panic(msg.as_ptr(), msg.len());
    }
    kryos_mutex_lock(mutex);
    0
}

/// Release the closure-call serialization lock at `mutex` (the matching
/// release for a successful `kryos_closure_lock_acquire`).
///
/// Returns 0 on success, -1 on a null pointer or an unmatched release
/// (release called without a corresponding prior acquire on this thread --
/// a caller bug, harmless: the underlying lock is left untouched).
#[no_mangle]
pub extern "C" fn kryos_closure_lock_release(mutex: *mut u8) -> i32 {
    if mutex.is_null() {
        return -1;
    }
    let key = mutex as usize;
    let held = HELD_CLOSURE_LOCKS.with(|m| m.borrow_mut().remove(&key).is_some());
    if held {
        kryos_mutex_unlock(mutex);
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
    // Clear any stale self-held-lock bookkeeping for this address BEFORE
    // freeing (LEDGER item 31) -- otherwise a freed-and-reused address (a
    // tight `mutex_new()`/`.drop()` loop can and does reuse the allocator's
    // last-freed address) could inherit a stale "already held" entry from
    // an unrelated prior mutex and falsely panic on its very first lock().
    let key = mutex as usize;
    HELD_MUTEX_LOCKS.with(|m| {
        m.borrow_mut().remove(&key);
    });
    // Reclaim and drop the box; the atomic needs no explicit release.
    let _ = unsafe { Box::from_raw(mutex as *mut KryosMutex) };
}
