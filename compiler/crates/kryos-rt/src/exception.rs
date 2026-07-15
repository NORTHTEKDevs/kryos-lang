//! Thread-local exception propagation for cross-function `throw`.
//!
//! When a `throw` executes outside a `try` block (e.g., inside a called
//! function), it stores the exception value in a thread-local and returns
//! from the function.  After every function call, compiled code checks
//! `kryos_exception_check()`.  If an exception is pending:
//!
//! - Inside a `try` block: the exception is taken via `kryos_exception_take()`
//!   and routed to the `catch` handler.
//! - Outside a `try` block: the function returns immediately, propagating
//!   the exception up the call stack until a `try/catch` is reached.

use std::cell::Cell;

thread_local! {
    /// Whether an exception is currently pending.
    static HAS_EXCEPTION: Cell<bool> = const { Cell::new(false) };
    /// The pending exception value (only valid when HAS_EXCEPTION is true).
    static EXCEPTION_VALUE: Cell<i64> = const { Cell::new(0) };
}

/// Store an exception value and mark it as pending.
///
/// Called by compiled code when `throw` executes outside a `try` block.
/// The calling function will return immediately after this call.
///
/// The value is always a KryosString pointer (the MIR lowering stringifies
/// thrown values at the throw site). The runtime stores its OWN COPY: the
/// throwing function's return path drops its locals — including the temp
/// that held the thrown string — so keeping the raw pointer here was a
/// use-after-free once the unwind crossed a frame whose cleanup reused the
/// allocation (caught message printed as garbage / `<invalid utf-8>`).
/// `kryos_exception_take` transfers ownership of the copy to the catcher.
#[no_mangle]
pub extern "C" fn kryos_exception_throw(value: i64) {
    let owned = if value != 0 {
        let s = value as *const crate::string::KryosString;
        unsafe {
            let len = (*s).len;
            let data = (*s).data;
            if !data.is_null() {
                crate::string::kryos_string_new(data, len) as i64
            } else {
                value
            }
        }
    } else {
        value
    };
    HAS_EXCEPTION.with(|h| h.set(true));
    EXCEPTION_VALUE.with(|v| v.set(owned));
}

/// Check whether an exception is pending.
///
/// Returns 1 if an exception is pending, 0 otherwise.
/// Called by compiled code after every function call to implement unwinding.
#[no_mangle]
pub extern "C" fn kryos_exception_check() -> i64 {
    HAS_EXCEPTION.with(|h| if h.get() { 1 } else { 0 })
}

/// Take the pending exception value and clear the pending flag.
///
/// Returns the exception value.  Must only be called when an exception
/// is pending (i.e., `kryos_exception_check()` returned 1).
/// Called by compiled code in `catch` blocks to retrieve the thrown value.
#[no_mangle]
pub extern "C" fn kryos_exception_take() -> i64 {
    HAS_EXCEPTION.with(|h| h.set(false));
    EXCEPTION_VALUE.with(|v| v.get())
}

/// Exit code used when a thrown exception reaches the end of `main`
/// without being caught.
pub const UNCAUGHT_EXCEPTION_EXIT_CODE: i32 = 101;

/// If an exception is pending in THIS thread, print it to stderr and clear
/// it — used at spawned-thread exit, where the thread dies but the process
/// continues (Rust thread-panic semantics). Returns 1 if one was reported.
#[no_mangle]
pub extern "C" fn kryos_exception_report_thread_if_pending() -> i64 {
    if !HAS_EXCEPTION.with(|h| h.get()) {
        return 0;
    }
    let value = kryos_exception_take();
    let mut printed = false;
    if value != 0 {
        let s = value as *const crate::string::KryosString;
        unsafe {
            let len = (*s).len as usize;
            let data = (*s).data;
            if !data.is_null() {
                let slice = std::slice::from_raw_parts(data, len);
                if let Ok(text) = std::str::from_utf8(slice) {
                    eprintln!("kryos: uncaught exception in spawned thread: {text}");
                    printed = true;
                }
            }
        }
    }
    if !printed {
        eprintln!("kryos: uncaught exception in spawned thread (value: {value})");
    }
    1
}

/// Report an exception caught by an actor's mailbox-dispatch loop after a
/// handler call (see `generate_actor_dispatch` in kryos-mir, which places
/// an explicit `kryos_exception_check`/`kryos_exception_take` pair right
/// after every handler call so it can recover instead of dying silently).
///
/// `label` is a compile-time constant KryosString like "Worker.process"
/// (`<actor>.<handler>`), not user data. `exc_val` is the exception value
/// already taken via `kryos_exception_take` (ownership transferred to this
/// call). Mirrors `kryos_exception_report_thread_if_pending`'s wording for
/// the `spawn` path, so both surfaces read the same way in logs. Unlike the
/// `spawn`/`main` report paths, this does NOT kill the caller — the actor's
/// dispatch loop continues to the next message after this returns.
#[no_mangle]
pub extern "C" fn kryos_actor_report_exception(label: i64, exc_val: i64) -> i64 {
    let label_text = if label != 0 {
        let s = label as *const crate::string::KryosString;
        unsafe {
            let len = (*s).len as usize;
            let data = (*s).data;
            if !data.is_null() {
                let slice = std::slice::from_raw_parts(data, len);
                std::str::from_utf8(slice)
                    .unwrap_or("<invalid utf-8>")
                    .to_string()
            } else {
                "<actor>".to_string()
            }
        }
    } else {
        "<actor>".to_string()
    };

    let mut printed = false;
    if exc_val != 0 {
        let s = exc_val as *const crate::string::KryosString;
        unsafe {
            let len = (*s).len as usize;
            let data = (*s).data;
            if !data.is_null() {
                let slice = std::slice::from_raw_parts(data, len);
                if let Ok(text) = std::str::from_utf8(slice) {
                    eprintln!("[actor error] {label_text}: uncaught exception: {text}");
                    printed = true;
                }
            }
        }
    }
    if !printed {
        eprintln!("[actor error] {label_text}: uncaught exception (value: {exc_val})");
    }
    1
}

/// If an exception is pending, print it to stderr and exit nonzero.
///
/// Compiled code inserts a call to this before every `return` in `main`,
/// so a `throw` that unwinds all the way out of the program reports
/// itself instead of silently exiting 0. The exception value is always a
/// KryosString pointer (the MIR lowering stringifies thrown values at the
/// throw site), but a null/garbage-tolerant read keeps this safe against
/// hand-rolled callers.
#[no_mangle]
pub extern "C" fn kryos_exception_report_uncaught_if_pending() {
    if !HAS_EXCEPTION.with(|h| h.get()) {
        return;
    }
    let value = kryos_exception_take();
    let mut printed = false;
    if value != 0 {
        let s = value as *const crate::string::KryosString;
        unsafe {
            let len = (*s).len as usize;
            let data = (*s).data;
            if !data.is_null() {
                let slice = std::slice::from_raw_parts(data, len);
                if let Ok(text) = std::str::from_utf8(slice) {
                    eprintln!("kryos: uncaught exception: {text}");
                    printed = true;
                }
            }
        }
    }
    if !printed {
        eprintln!("kryos: uncaught exception (value: {value})");
    }
    std::process::exit(UNCAUGHT_EXCEPTION_EXIT_CODE);
}

/// If an exception is pending, take it and return its message (clearing the
/// pending flag). Returns None when no exception is pending. Used by the
/// test harness: a `throw` that unwinds out of a `@test` fn must FAIL the
/// test, not leak into the next one.
pub fn take_pending_exception_message() -> Option<String> {
    if !HAS_EXCEPTION.with(|h| h.get()) {
        return None;
    }
    let value = kryos_exception_take();
    if value != 0 {
        let s = value as *const crate::string::KryosString;
        unsafe {
            let len = (*s).len as usize;
            let data = (*s).data;
            if !data.is_null() {
                let slice = std::slice::from_raw_parts(data, len);
                if let Ok(text) = std::str::from_utf8(slice) {
                    return Some(format!("uncaught exception: {text}"));
                }
            }
        }
    }
    Some(format!("uncaught exception (value: {value})"))
}
