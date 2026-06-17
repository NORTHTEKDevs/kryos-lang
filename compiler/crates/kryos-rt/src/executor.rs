//! Cooperative single-active-task executor for Kryos async/await.
//!
//! This is a **real** cooperative scheduler: multiple tasks make progress
//! INTERLEAVED, and `await` / an explicit yield hands control to the
//! scheduler instead of running straight through.
//!
//! # Design (Option A: runtime green tasks backed by OS threads)
//!
//! Each spawned task runs its body straight-line on its own OS thread, but a
//! global **baton** guarantees that exactly one task is running at any instant
//! (the runtime is *cooperative*, not parallel). The protocol is a strict
//! ping-pong between the scheduler thread and whichever task currently holds
//! the baton:
//!
//! - The scheduler (`kryos_coop_run`, runs on the main thread) pops the next
//!   ready task from a round-robin queue and hands it the baton.
//! - The task runs until it calls `kryos_coop_yield` (the suspension point) or
//!   returns. `yield` re-queues the task and hands the baton back to the
//!   scheduler; return marks the task done and hands the baton back.
//! - The scheduler regains the baton, picks the next ready task, repeats —
//!   until no tasks remain.
//!
//! Because a yielding task re-queues itself at the *back* of the ready queue,
//! two tasks that each yield N times interleave their effects A,B,A,B,...,
//! which is the defining proof of real cooperative scheduling (as opposed to
//! the previous behavior where `await` lowered to a direct synchronous call
//! and one task ran fully before the next started).
//!
//! No CPS / state-machine transform is required: the task body is ordinary
//! straight-line code, and the suspension happens by parking the task's OS
//! thread on a condvar while the baton is elsewhere.
//!
//! All public functions are `#[no_mangle] extern "C"` so compiled Kryos code
//! links them via the C ABI.

#![allow(clippy::missing_safety_doc)]

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, OnceLock};

/// Whose turn it is to run. The baton alternates strictly between the
/// scheduler and exactly one task.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Turn {
    /// The scheduler holds the baton and may grant it to a ready task.
    Scheduler,
    /// Task `id` holds the baton and is the single running task.
    Task(u64),
}

/// Mutable scheduler state, guarded by `Sched::state`.
struct Shared {
    /// Whose turn it is right now.
    turn: Turn,
    /// Tasks waiting to be granted the baton, in round-robin order. A task
    /// that yields re-queues itself at the back, producing interleaving.
    ready: VecDeque<u64>,
    /// Number of tasks that have been spawned but not yet completed.
    live: u64,
    /// Monotonic task id source.
    next_id: u64,
    /// Order log: tags recorded by tasks via `kryos_coop_record`, in the
    /// exact order tasks ran. This is the observable interleaving proof.
    log: Vec<String>,
}

struct Sched {
    state: Mutex<Shared>,
    cv: Condvar,
}

fn sched() -> &'static Sched {
    static S: OnceLock<Sched> = OnceLock::new();
    S.get_or_init(|| Sched {
        state: Mutex::new(Shared {
            turn: Turn::Scheduler,
            ready: VecDeque::new(),
            live: 0,
            next_id: 0,
            log: Vec::new(),
        }),
        cv: Condvar::new(),
    })
}

/// Lock the scheduler state, recovering from a poisoned mutex (a task that
/// panicked while holding the lock should not wedge the whole runtime).
fn lock(s: &Sched) -> std::sync::MutexGuard<'_, Shared> {
    s.state.lock().unwrap_or_else(|p| p.into_inner())
}

thread_local! {
    /// Id of the coop task running on this thread, or 0 if this thread is not
    /// a coop task (e.g. the main/scheduler thread). `kryos_coop_yield` uses
    /// this to know who to park; on a non-task thread it is a safe no-op.
    static CURRENT_TASK: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

// ---------------------------------------------------------------------------
// Public C-ABI surface
// ---------------------------------------------------------------------------

/// Spawn a cooperative task. `fn_ptr` is the task body's address (cast to
/// i64); `arg` is a single i64 passed to it. The task is registered on the
/// ready queue and its OS thread immediately parks until the scheduler grants
/// it the baton. Returns the task id (>0), or 0 if `fn_ptr` is null.
///
/// The body is invoked as `extern "C" fn(i64) -> i64`; a 0-argument Kryos
/// task simply ignores the extra register, and the return value is discarded.
#[no_mangle]
pub extern "C" fn kryos_coop_spawn(fn_ptr: i64, arg: i64) -> u64 {
    if fn_ptr == 0 {
        return 0;
    }
    let s = sched();
    let id = {
        let mut g = lock(s);
        g.next_id += 1;
        let id = g.next_id;
        g.ready.push_back(id);
        g.live += 1;
        id
    };

    let addr = fn_ptr as usize;
    std::thread::spawn(move || {
        CURRENT_TASK.with(|c| c.set(id));
        // Park until the scheduler first grants us the baton.
        wait_for_turn(s, id);

        // Run the task body straight-through. Any `kryos_coop_yield` inside
        // hands the baton back to the scheduler and parks here until re-granted.
        // SAFETY: `addr` is a valid Kryos function address per spawn contract.
        unsafe {
            let f: extern "C" fn(i64) -> i64 = std::mem::transmute(addr);
            f(arg);
        }
        // A `throw` that unwound out of the task would otherwise vanish with
        // the thread; report it like the spawn runtime does.
        crate::exception::kryos_exception_report_thread_if_pending();

        // Completion: mark done and hand the baton back to the scheduler.
        {
            let mut g = lock(s);
            g.live -= 1;
            g.turn = Turn::Scheduler;
        }
        s.cv.notify_all();
    });

    id
}

/// Block until the calling thread's task is granted the baton.
fn wait_for_turn(s: &Sched, id: u64) {
    let mut g = lock(s);
    while g.turn != Turn::Task(id) {
        g = s.cv.wait(g).unwrap_or_else(|p| p.into_inner());
    }
}

/// Yield the baton from the current task back to the scheduler, re-queueing
/// the current task as ready. This is the cooperative suspension point that
/// produces interleaving. No-op when not called from a coop task thread
/// (so direct, non-spawned async calls degrade to ordinary synchronous calls).
#[no_mangle]
pub extern "C" fn kryos_coop_yield() {
    let id = CURRENT_TASK.with(|c| c.get());
    if id == 0 {
        return;
    }
    let s = sched();
    let mut g = lock(s);
    // Re-queue ourselves at the back (round-robin) and hand the baton back to
    // the scheduler. We then park on the condvar until the scheduler grants us
    // the baton again. Holding the lock across set-ready + set-turn + wait
    // closes any lost-wakeup window.
    g.ready.push_back(id);
    g.turn = Turn::Scheduler;
    s.cv.notify_all();
    while g.turn != Turn::Task(id) {
        g = s.cv.wait(g).unwrap_or_else(|p| p.into_inner());
    }
}

/// Drive all spawned tasks to completion, round-robin. Runs on the calling
/// (scheduler) thread; must not be called from within a task.
#[no_mangle]
pub extern "C" fn kryos_coop_run() {
    let s = sched();
    loop {
        let mut g = lock(s);
        let next = g.ready.pop_front();
        let id = match next {
            Some(id) => id,
            None => {
                // No ready tasks. If none are live, we're done; otherwise a
                // task is blocked with no way to be woken — bail rather than
                // hang (cannot happen in the cooperative model).
                return;
            }
        };
        g.turn = Turn::Task(id);
        s.cv.notify_all();
        // Wait until the task hands the baton back (yielded or completed).
        while g.turn != Turn::Scheduler {
            g = s.cv.wait(g).unwrap_or_else(|p| p.into_inner());
        }
        // The task either re-queued itself (yield) or finished; loop on.
        drop(g);
    }
}

/// Append `tag` (read from a Kryos `str`) to the order log. Called by tasks to
/// record the exact interleaved order in which they ran.
#[no_mangle]
pub extern "C" fn kryos_coop_record(s_ptr: *const crate::string::KryosString) {
    if s_ptr.is_null() {
        return;
    }
    // SAFETY: s_ptr is a valid *const KryosString per the str ABI.
    let ks = unsafe { &*s_ptr };
    let len = if ks.len < 0 { 0 } else { ks.len as usize };
    let tag = if len == 0 || ks.data.is_null() {
        String::new()
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(ks.data, len) };
        String::from_utf8_lossy(bytes).into_owned()
    };
    record_raw(&tag);
}

/// Return the order log as a space-joined Kryos `str` handle.
#[no_mangle]
pub extern "C" fn kryos_coop_order() -> *mut crate::string::KryosString {
    let joined = order_raw();
    let bytes = joined.as_bytes();
    // SAFETY: kryos_string_new copies the bytes; ptr/len are valid.
    unsafe { crate::string::kryos_string_new(bytes.as_ptr(), bytes.len() as i64) }
}

/// Reset the executor: clear the ready queue, the order log, and the baton.
/// Intended for tests and for a fresh `coop_run` cycle. Does not reset the
/// monotonic task-id source.
#[no_mangle]
pub extern "C" fn kryos_coop_reset() {
    let s = sched();
    let mut g = lock(s);
    g.ready.clear();
    g.log.clear();
    g.live = 0;
    g.turn = Turn::Scheduler;
}

/// Return the current coop task id, or 0 if not on a coop task thread.
#[no_mangle]
pub extern "C" fn kryos_coop_current() -> u64 {
    CURRENT_TASK.with(|c| c.get())
}

// ---------------------------------------------------------------------------
// Internal helpers (also used by the Rust unit tests).
// ---------------------------------------------------------------------------

/// Append a tag to the order log directly (no Kryos string marshalling).
pub(crate) fn record_raw(tag: &str) {
    let s = sched();
    let mut g = lock(s);
    g.log.push(tag.to_string());
}

/// Read the order log as a space-joined string.
pub(crate) fn order_raw() -> String {
    let s = sched();
    let g = lock(s);
    g.log.join(" ")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// Serialize tests that share the global executor so they don't interleave
    /// each other's tasks/logs.
    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    extern "C" fn task_a(_arg: i64) -> i64 {
        for i in 0..3 {
            record_raw(&format!("A{i}"));
            kryos_coop_yield();
        }
        0
    }

    extern "C" fn task_b(_arg: i64) -> i64 {
        for i in 0..3 {
            record_raw(&format!("B{i}"));
            kryos_coop_yield();
        }
        0
    }

    fn fn_addr(f: extern "C" fn(i64) -> i64) -> i64 {
        f as usize as i64
    }

    /// THE DEFINING PROOF: two cooperative tasks that each yield three times
    /// must interleave their effects A,B,A,B,A,B — not run one fully then the
    /// other (which would be "A0 A1 A2 B0 B1 B2").
    #[test]
    fn two_tasks_interleave() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        kryos_coop_reset();
        kryos_coop_spawn(fn_addr(task_a), 0);
        kryos_coop_spawn(fn_addr(task_b), 0);
        kryos_coop_run();
        assert_eq!(order_raw(), "A0 B0 A1 B1 A2 B2");
    }

    /// Negative control: a single task records all of its tags consecutively
    /// (there is nothing to interleave with).
    #[test]
    fn single_task_is_not_interleaved() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        kryos_coop_reset();
        kryos_coop_spawn(fn_addr(task_a), 0);
        kryos_coop_run();
        assert_eq!(order_raw(), "A0 A1 A2");
    }

    /// Three tasks interleave round-robin: A,B,C,A,B,C,...
    extern "C" fn task_c(_arg: i64) -> i64 {
        for i in 0..2 {
            record_raw(&format!("C{i}"));
            kryos_coop_yield();
        }
        0
    }

    #[test]
    fn three_tasks_round_robin() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        kryos_coop_reset();
        kryos_coop_spawn(fn_addr(task_a), 0);
        kryos_coop_spawn(fn_addr(task_b), 0);
        kryos_coop_spawn(fn_addr(task_c), 0);
        kryos_coop_run();
        // A,B,C each fire round 0 and 1; then A,B finish their 3rd round
        // (C only had 2 rounds) — C drops out after round 1.
        assert_eq!(order_raw(), "A0 B0 C0 A1 B1 C1 A2 B2");
    }

    /// Yield from a non-task thread (CURRENT_TASK == 0) is a safe no-op.
    #[test]
    fn yield_off_task_is_noop() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        kryos_coop_yield(); // must not hang or panic
    }
}
