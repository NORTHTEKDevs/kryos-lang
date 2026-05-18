//! Software call stack tracing for Kryos programs.
//!
//! The compiled code emits `kryos_trace_enter` at function entry and
//! `kryos_trace_exit` at function return. The runtime maintains a
//! thread-local call stack that is printed when a panic occurs.

use std::cell::RefCell;
use std::fmt::Write;
use std::sync::atomic::{AtomicBool, Ordering};

/// A single frame on the software call stack.
struct TraceFrame {
    func_name: String,
    file: String,
    line: u32,
}

thread_local! {
    static CALL_STACK: RefCell<Vec<TraceFrame>> = RefCell::new(Vec::with_capacity(64));
}

/// When set to `true`, the trace runtime prints a line on every function
/// entry and exit instead of (or in addition to) building the silent call
/// stack. Enable from the `kryos trace` subcommand.
static VERBOSE_TRACE: AtomicBool = AtomicBool::new(false);

/// Toggle verbose tracing globally. Safe to call from outside Kryos code
/// (e.g. from a host CLI command before invoking JIT-compiled main).
pub fn set_verbose_trace(on: bool) {
    VERBOSE_TRACE.store(on, Ordering::Relaxed);
}

fn verbose_trace_enabled() -> bool {
    // Lazy env probe: read once at runtime startup. The atomic flag is
    // mutable in-process; the env variable lets child processes spawned by
    // `kryos trace` inherit the setting without explicit plumbing.
    use std::sync::atomic::{AtomicU8, Ordering as O};
    static ENV_PROBED: AtomicU8 = AtomicU8::new(0); // 0=unprobed, 1=off, 2=on
    let probed = ENV_PROBED.load(O::Relaxed);
    if probed == 0 {
        let on = std::env::var("KRYOS_TRACE").map(|v| v != "0" && !v.is_empty()).unwrap_or(false);
        ENV_PROBED.store(if on { 2 } else { 1 }, O::Relaxed);
        if on {
            VERBOSE_TRACE.store(true, Ordering::Relaxed);
        }
    }
    VERBOSE_TRACE.load(Ordering::Relaxed)
}

/// Push a frame onto the call stack. Called at function entry.
///
/// # Safety
///
/// `name_ptr` must point to `name_len` valid UTF-8 bytes (or be null).
/// `file_ptr` must point to `file_len` valid UTF-8 bytes (or be null).
#[no_mangle]
pub extern "C" fn kryos_trace_enter(
    name_ptr: *const u8,
    name_len: usize,
    file_ptr: *const u8,
    file_len: usize,
    line: u32,
) {
    let name = if name_ptr.is_null() || name_len == 0 {
        "<unknown>".to_string()
    } else {
        unsafe {
            String::from_utf8_lossy(std::slice::from_raw_parts(name_ptr, name_len)).into_owned()
        }
    };
    let file = if file_ptr.is_null() || file_len == 0 {
        "<unknown>".to_string()
    } else {
        unsafe {
            String::from_utf8_lossy(std::slice::from_raw_parts(file_ptr, file_len)).into_owned()
        }
    };
    if verbose_trace_enabled() {
        let depth = CALL_STACK.with(|s| s.borrow().len());
        let indent = "  ".repeat(depth);
        eprintln!("\x1b[36mtrace\x1b[0m {indent}→ {name}() at {file}:{line}");
    }
    CALL_STACK.with(|stack| {
        stack.borrow_mut().push(TraceFrame {
            func_name: name,
            file,
            line,
        });
    });
}

/// Pop a frame from the call stack. Called at function return.
#[no_mangle]
pub extern "C" fn kryos_trace_exit() {
    let popped = CALL_STACK.with(|stack| stack.borrow_mut().pop());
    if verbose_trace_enabled() {
        if let Some(frame) = popped {
            let depth = CALL_STACK.with(|s| s.borrow().len());
            let indent = "  ".repeat(depth);
            eprintln!("\x1b[36mtrace\x1b[0m {indent}← {}()", frame.func_name);
        }
    }
}

/// Format the current call stack as a human-readable string.
///
/// Returns an empty string if the stack is empty.
pub fn format_stack_trace() -> String {
    CALL_STACK.with(|stack| {
        let stack = stack.borrow();
        if stack.is_empty() {
            return String::new();
        }
        let mut out = String::from("\nstack trace (most recent call last):\n");
        for (i, frame) in stack.iter().rev().enumerate() {
            let _ = writeln!(
                out,
                "  {}: {}() at {}:{}",
                i, frame.func_name, frame.file, frame.line
            );
        }
        out
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_stack_captures_frames() {
        // Push two frames.
        kryos_trace_enter("main".as_ptr(), 4, "test.kry".as_ptr(), 8, 1);
        kryos_trace_enter("helper".as_ptr(), 6, "test.kry".as_ptr(), 8, 5);

        let trace = format_stack_trace();
        assert!(
            trace.contains("helper()"),
            "trace should contain helper(): {trace}"
        );
        assert!(
            trace.contains("main()"),
            "trace should contain main(): {trace}"
        );
        assert!(
            trace.contains("test.kry"),
            "trace should contain file name: {trace}"
        );
        assert!(
            trace.contains("most recent call last"),
            "trace should have header: {trace}",
        );

        // Pop both frames.
        kryos_trace_exit();
        kryos_trace_exit();

        // After popping, trace should be empty.
        let trace_after = format_stack_trace();
        assert!(
            trace_after.is_empty(),
            "trace should be empty after exit: {trace_after}"
        );
    }

    #[test]
    fn trace_empty_stack() {
        // Ensure no leftover frames from other tests (thread-local is per-thread).
        // Clear any residual frames.
        CALL_STACK.with(|stack| stack.borrow_mut().clear());

        let trace = format_stack_trace();
        assert!(trace.is_empty(), "empty stack should produce empty string");
    }

    #[test]
    fn trace_null_pointers() {
        kryos_trace_enter(std::ptr::null(), 0, std::ptr::null(), 0, 0);
        let trace = format_stack_trace();
        assert!(
            trace.contains("<unknown>()"),
            "null name should show <unknown>: {trace}"
        );
        kryos_trace_exit();
    }
}
