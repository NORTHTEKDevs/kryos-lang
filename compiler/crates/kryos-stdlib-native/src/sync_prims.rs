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
use std::sync::{Mutex, OnceLock};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use kryos_rt::executor::{kryos_coop_current, kryos_coop_yield};
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
    // pathology under heavy contention). `spin_acquire` is shared with the
    // closure-call lock below, which passes a real deadlock-checking hook;
    // here it is a no-op, so `std::sync::Mutex`'s spin behavior is
    // unchanged byte-for-byte.
    spin_acquire(km, || {});
    0
}

/// Shared spin-then-yield CAS acquire loop. `on_stall` is invoked once per
/// yield iteration (i.e. only after the initial short spin-loop phase, so
/// it costs nothing on the common uncontended-or-briefly-contended path);
/// `kryos_mutex_lock` passes a no-op, `kryos_closure_lock_acquire` passes a
/// hook that periodically walks the cross-thread wait-for graph for a
/// deadlock (LEDGER item 46b).
fn spin_acquire(km: &KryosMutex, mut on_stall: impl FnMut()) {
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
            on_stall();
        }
    }
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

// ---------------------------------------------------------------------------
// Cross-closure AB-BA deadlock detection (LEDGER item 46b).
//
// The self-reentry table above only catches a thread re-entering the SAME
// lock address it already holds. It says nothing about the textbook
// lock-ordering deadlock between TWO DIFFERENT closures' locks: thread 1
// holds closure A's lock and blocks wanting closure B's; thread 2 holds
// closure B's lock and blocks wanting closure A's. Each lock lives at a
// distinct address (a fixed offset inside its OWN closure env block), so
// neither thread's self-reentry check ever sees its own address again --
// they hang forever with zero diagnostic (`attack_cross_closure_lock_
// deadlock.kry`, CONFIRMED).
//
// Two fix shapes were on the table (per this task's brief): a global
// lock-ORDERING (acquire addresses low->high), or a DETECTOR that fails
// loudly. Ordering was rejected: the codegen-inserted acquire happens the
// instant a mutating closure's thunk is entered, one call at a time -- by
// the time thread 1 is inside A's lock, it does not yet know (this is a
// dynamic call graph, not a batch "acquire everything up front" protocol)
// that the call it is about to make will want B's lock too. There is
// nothing to sort; the acquisitions are inherently sequential/nested, not
// a fixed set known before the first one is taken. So: a DETECTOR, same
// choice item 11(a) made for self-reentry and for the same reason (a loud,
// attributable failure beats a silent hang, and beats a silent wrong
// answer even more).
//
// Implementation: a small global wait-for graph, guarded by one `Mutex` (a
// real blocking mutex is fine here -- the graph is only touched for a
// handful of instructions per acquire/release/stall-check, never held
// across the actual closure-lock spin). `owners[lock_addr] = thread` while
// a thread holds a closure lock; `waiting[thread] = lock_addr` while a
// thread is blocked wanting one. An entry only ever exists in `waiting`
// while a thread is genuinely inside the CAS spin loop below (inserted
// just before it starts, removed the instant it acquires) -- so walking
// `waiting_for_key -> owner -> owner's waiting_for_key -> ...` and finding
// it loop back to the STARTING thread within a bounded number of hops is
// not a heuristic guess: every edge in that chain is a thread that is
// ACTUALLY blocked wanting a lock ACTUALLY held by the next thread in the
// chain, which is Coffman's circular-wait condition directly, not a proxy
// for it. A thread that is merely about to release soon (not itself
// blocked on something) has no `waiting` entry, so no edge extends through
// it -- it cannot cause a false-positive cycle.
struct ClosureLockWaitGraph {
    /// lock address -> thread that currently holds it.
    owners: HashMap<usize, ThreadId>,
    /// thread -> lock address it is currently blocked trying to acquire.
    waiting: HashMap<ThreadId, usize>,
}

fn closure_lock_graph() -> &'static Mutex<ClosureLockWaitGraph> {
    static GRAPH: OnceLock<Mutex<ClosureLockWaitGraph>> = OnceLock::new();
    GRAPH.get_or_init(|| {
        Mutex::new(ClosureLockWaitGraph {
            owners: HashMap::new(),
            waiting: HashMap::new(),
        })
    })
}

/// Bound on how many hops the wait-for chain is followed before giving up
/// (not-yet-provable-deadlock, keep spinning). Generous relative to any
/// realistic thread count so a genuine cycle of any length this runtime
/// could actually create -- 2 (AB-BA), 3 (A->B->C->A), or more -- is always
/// found well within the bound.
const CLOSURE_LOCK_CYCLE_HOP_LIMIT: usize = 4096;

/// Called periodically (from `spin_acquire`'s stall hook, throttled to
/// roughly every 25ms of real stall time -- see `kryos_closure_lock_
/// acquire`) while `tid` is blocked wanting the closure lock at
/// `waiting_for_key`. Walks the wait-for graph; if the chain of "who holds
/// the lock I want, and what do THEY want" leads back to `tid`, every
/// thread on that chain is permanently stuck and none can ever proceed --
/// panics with the full chain (thread ids + lock addresses) instead of
/// spinning forever. A no-op (returns normally) if no cycle is found yet.
fn check_closure_deadlock(tid: ThreadId, waiting_for_key: usize) {
    let g = closure_lock_graph().lock().unwrap();
    // Each step is (blocked_thread, wants_key, held_by_thread) -- recorded
    // as a real triple (not two separately-indexed fields) specifically so
    // the panic message below can't accidentally pair a "wants" address
    // with the wrong step, the way an earlier version of this function did.
    let mut steps: Vec<(ThreadId, usize, ThreadId)> = Vec::new();
    let mut cur_thread = tid;
    let mut cur_key = waiting_for_key;
    for _ in 0..CLOSURE_LOCK_CYCLE_HOP_LIMIT {
        let holder = match g.owners.get(&cur_key) {
            Some(o) => *o,
            // Lock is currently unowned in our bookkeeping (e.g. released
            // between the CAS failure that triggered this check and now) --
            // no cycle through this edge.
            None => return,
        };
        steps.push((cur_thread, cur_key, holder));
        if holder == tid {
            let mut msg = format!(
                "deadlock: {} mutating closures are waiting on each other's locks \
                 in a cycle and NONE of them can ever proceed (this is the classic \
                 lock-ordering AB-BA deadlock, generalized to a cycle of any \
                 length). Chain: ",
                steps.len(),
            );
            for (i, (blocked, key, holder)) in steps.iter().enumerate() {
                if i > 0 {
                    msg.push_str(" -> ");
                }
                msg.push_str(&format!(
                    "thread {:?} wants closure-lock @0x{:x} (held by thread {:?})",
                    blocked, key, holder
                ));
            }
            msg.push_str(
                ". Restructure so these mutating closures never call into each \
                 other while shared across threads -- e.g. route the interaction \
                 through a channel instead of a direct call, or keep the \
                 cross-closure call OUTSIDE the locked section.",
            );
            drop(g);
            kryos_panic(msg.as_ptr(), msg.len());
        }
        cur_key = match g.waiting.get(&holder) {
            Some(k) => *k,
            // `holder` is not itself blocked on anything -- it will finish
            // and release normally. No cycle through this edge.
            None => return,
        };
        cur_thread = holder;
    }
    // Hop limit exhausted without the chain closing back on `tid` -- not
    // (yet) a provable cycle involving this thread; keep spinning.
}

/// Acquire the closure-call serialization lock at `mutex`. If the CURRENT
/// thread already holds this exact lock address (a mutating closure
/// reaching itself through its own stored value, directly or indirectly,
/// before its outer call returns), this is a fatal, unrecoverable error --
/// reported and the process exits via `kryos_panic`, matching every other
/// Kryos runtime-fault contract, instead of spinning forever against a
/// lock this thread already holds (see the module doc comment above for
/// why "silently allow it" was tried and rejected). If a DIFFERENT thread
/// holds a DIFFERENT closure's lock in a way that forms a wait-for cycle
/// back to this thread (an AB-BA deadlock or longer cycle across two or
/// more distinct closures), that is also fatal and also reported via
/// `kryos_panic` naming the full chain -- see `check_closure_deadlock`
/// (LEDGER item 46b).
///
/// Returns 0 on success, -1 on a null pointer. Never returns on detected
/// self-reentry or cross-closure deadlock (`kryos_panic` is `-> !`).
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

    let tid = thread::current().id();
    {
        let mut g = closure_lock_graph().lock().unwrap();
        g.waiting.insert(tid, key);
    }

    let km = unsafe { &*(mutex as *mut KryosMutex) };
    let mut last_check = Instant::now();
    spin_acquire(km, || {
        // COOP-AWARE SPIN (LEDGER item 46c). This bare CAS loop has no idea
        // a cooperative executor (kryos-rt/src/executor.rs) exists -- it
        // only yields the OS SCHEDULER (`thread::yield_now`, in
        // `spin_acquire`). If the CURRENT thread is a coop task (not a
        // plain OS `spawn` thread), that is not enough: the coop scheduler
        // runs exactly ONE task at a time, gated by a baton, and a task
        // that never calls `kryos_coop_yield` never hands that baton back
        // no matter how much OS-level yielding it does. Concretely: task A
        // acquires this lock, then hits a real yield point inside the
        // locked body (`sleep_ms`/`await`/`http_get`/...) and hands the
        // COOP BATON to the scheduler while its OS thread is genuinely
        // away; the scheduler grants the baton to task B; B calls into the
        // SAME shared closure, finds the lock held, and spins -- and
        // before this fix, that spin never released the baton, so the
        // scheduler could never grant it back to task A even after A's own
        // blocking op finished and it re-queued itself wanting its turn.
        // Permanent hang: the real atomic lock is waiting on task A, task
        // A is waiting on the coop baton, the coop baton is stuck with
        // task B, and task B is waiting on the real atomic lock -- every
        // party blocked on the next, forever
        // (`attack_closure_lock_coop_yield_deadlock.kry`, CONFIRMED both
        // backends).
        //
        // Fix: yield the COOP BATON, not just the OS thread, on every
        // failed CAS attempt while on a coop task. `kryos_coop_yield` is
        // a no-op on a non-task thread (`kryos_coop_current() == 0`), so a
        // real OS `spawn` thread spinning here behaves EXACTLY as before
        // (item 7b's plain-thread serialization guarantee is untouched).
        // On a coop task, each failed attempt now hands the baton back to
        // the scheduler and parks until this task's next turn -- letting
        // the scheduler resume whichever OTHER task can make progress,
        // including, eventually, the lock holder once its own blocking op
        // completes and it re-queues itself. This alone resolves a simple
        // two-task lock-then-yield-then-contend hang (the repro's shape).
        // It does NOT by itself resolve a coop-level AB-BA (two coop tasks
        // each holding one lock and spinning for the other's, which would
        // otherwise ping-pong the baton forever with neither progressing)
        // -- that residual case is caught by the SAME cross-thread
        // deadlock detector item 46b added below, which sees exactly the
        // same wait-for graph regardless of whether the blocked threads
        // happen to be coop tasks or plain OS threads.
        if kryos_coop_current() != 0 {
            kryos_coop_yield();
        }
        // Throttled to a real-time interval (not a spin-count) so detection
        // latency is predictable regardless of scheduler/CPU speed: a
        // genuine cycle is found within ~25ms of the stall starting,
        // re-checked every ~25ms thereafter until it either resolves or is
        // proven a deadlock.
        if last_check.elapsed() >= Duration::from_millis(25) {
            last_check = Instant::now();
            check_closure_deadlock(tid, key);
        }
    });

    {
        let mut g = closure_lock_graph().lock().unwrap();
        g.waiting.remove(&tid);
        g.owners.insert(key, tid);
    }
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
        {
            let mut g = closure_lock_graph().lock().unwrap();
            g.owners.remove(&key);
        }
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
