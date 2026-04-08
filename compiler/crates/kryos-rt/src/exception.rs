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
#[no_mangle]
pub extern "C" fn kryos_exception_throw(value: i64) {
    HAS_EXCEPTION.with(|h| h.set(true));
    EXCEPTION_VALUE.with(|v| v.set(value));
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
