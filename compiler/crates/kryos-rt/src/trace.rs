//! Software call stack tracing for Kryos programs.
//!
//! The compiled code emits `kryos_trace_enter` at function entry and
//! `kryos_trace_exit` at function return. The runtime maintains a
//! thread-local call stack that is printed when a panic occurs.

use std::cell::RefCell;
use std::fmt::Write;
use std::sync::atomic::{AtomicBool, Ordering};

/// A single frame on the software call stack.
///
/// v3.8: stores raw pointers into the compiled program's .data section
/// instead of allocating `String`s. The pointers come from data segments
/// declared by the codegen at function-definition time and have `'static`
/// lifetime in the loaded image, so they're safe to store. This eliminates
/// the per-call String allocation that previously dominated trace overhead.
struct TraceFrame {
    name_ptr: *const u8,
    name_len: usize,
    file_ptr: *const u8,
    file_len: usize,
    line: u32,
}

// SAFETY: Raw pointers into .data segments are 'static and not mutated.
unsafe impl Send for TraceFrame {}

thread_local! {
    static CALL_STACK: RefCell<Vec<TraceFrame>> = RefCell::new(Vec::with_capacity(64));
}

/// When set to `true`, the trace runtime prints a line on every function
/// entry and exit instead of (or in addition to) building the silent call
/// stack. Enable from the `kryos trace` subcommand.
static VERBOSE_TRACE: AtomicBool = AtomicBool::new(false);

/// When set to `true`, every `kryos_trace_enter` call increments a per-name
/// counter. Read by `kryos profile` at exit to produce a hot-list.
static PROFILE_MODE: AtomicBool = AtomicBool::new(false);

/// Global counter table — Mutex (not thread-local) so the atexit hook can
/// safely read it during shutdown without hitting TLS destruction-order
/// issues.
static PROFILE_COUNTS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, u64>>,
> = std::sync::OnceLock::new();

fn profile_counts(
) -> &'static std::sync::Mutex<std::collections::HashMap<String, u64>> {
    PROFILE_COUNTS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Toggle verbose tracing globally. Safe to call from outside Kryos code
/// (e.g. from a host CLI command before invoking JIT-compiled main).
pub fn set_verbose_trace(on: bool) {
    VERBOSE_TRACE.store(on, Ordering::Relaxed);
}

/// Toggle profile (call-count) mode. When on, each trace_enter increments
/// a counter keyed by function name. Read via `take_profile_counts`.
pub fn set_profile_mode(on: bool) {
    PROFILE_MODE.store(on, Ordering::Relaxed);
}

/// Atomically remove and return all accumulated profile counts.
/// Returns a Vec of (name, count) sorted descending by count.
pub fn take_profile_counts() -> Vec<(String, u64)> {
    let map = profile_counts();
    let guard = match map.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let mut v: Vec<(String, u64)> = guard.iter().map(|(k, v)| (k.clone(), *v)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

fn profile_mode_enabled() -> bool {
    PROFILE_MODE.load(Ordering::Relaxed)
}

fn verbose_trace_enabled() -> bool {
    // Lazy env probe: read once at runtime startup. The atomic flag is
    // mutable in-process; the env variables let child processes spawned by
    // `kryos trace` or `kryos profile` inherit the setting without explicit
    // plumbing.
    use std::sync::atomic::{AtomicU8, Ordering as O};
    static ENV_PROBED: AtomicU8 = AtomicU8::new(0); // 0=unprobed, 1=done
    let probed = ENV_PROBED.load(O::Relaxed);
    if probed == 0 {
        let trace_on =
            std::env::var("KRYOS_TRACE").map(|v| v != "0" && !v.is_empty()).unwrap_or(false);
        let profile_on =
            std::env::var("KRYOS_PROFILE").map(|v| v != "0" && !v.is_empty()).unwrap_or(false);
        if trace_on {
            VERBOSE_TRACE.store(true, Ordering::Relaxed);
        }
        if profile_on {
            PROFILE_MODE.store(true, Ordering::Relaxed);
            // Register an atexit-style hook via a Drop guard on a static.
            install_profile_dump_hook();
        }
        ENV_PROBED.store(1, O::Relaxed);
    }
    VERBOSE_TRACE.load(Ordering::Relaxed)
}

/// Install a hook that dumps the profile counts on process exit.
///
/// Uses libc `atexit` so the dump runs at controlled-shutdown time, *not*
/// during thread-local destruction (which would deny access to TLS values
/// and crash with "cannot access a Thread Local Storage value during or
/// after destruction").
fn install_profile_dump_hook() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // SAFETY: libc::atexit takes a `extern "C" fn()` — our hook is
        // exactly that. The hook only locks a global Mutex and writes to
        // stderr, both safe at process-exit time.
        unsafe {
            extern "C" {
                fn atexit(cb: extern "C" fn()) -> i32;
            }
            atexit(profile_dump_hook);
        }
    });
}

extern "C" fn profile_dump_hook() {
    let counts = take_profile_counts();
    if counts.is_empty() {
        return;
    }
    eprintln!();
    eprintln!(
        "\x1b[1mkryos profile\x1b[0m — call counts (top {})",
        counts.len().min(20)
    );
    let max_name_w = counts.iter().take(20).map(|(n, _)| n.len()).max().unwrap_or(20);
    for (name, n) in counts.iter().take(20) {
        eprintln!("  {:<width$}  {:>10}", name, n, width = max_name_w);
    }
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
    // Probe env vars on first call. Re-checks profile/verbose state after.
    let verbose = verbose_trace_enabled();

    // Cheap path: tag a counter if profile mode is on.
    if profile_mode_enabled() && !name_ptr.is_null() && name_len > 0 {
        let name_str = unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(name_ptr, name_len))
        };
        if let Ok(mut counts) = profile_counts().lock() {
            *counts.entry(name_str.to_string()).or_insert(0) += 1;
        }
    }

    if verbose {
        // Lazy borrow of the name/file slices for the rare verbose path.
        let name = if name_ptr.is_null() || name_len == 0 {
            "<unknown>"
        } else {
            unsafe {
                std::str::from_utf8_unchecked(std::slice::from_raw_parts(name_ptr, name_len))
            }
        };
        let file = if file_ptr.is_null() || file_len == 0 {
            "<unknown>"
        } else {
            unsafe {
                std::str::from_utf8_unchecked(std::slice::from_raw_parts(file_ptr, file_len))
            }
        };
        let depth = CALL_STACK.with(|s| s.borrow().len());
        let indent = "  ".repeat(depth);
        eprintln!("\x1b[36mtrace\x1b[0m {indent}→ {name}() at {file}:{line}");
    }
    CALL_STACK.with(|stack| {
        stack.borrow_mut().push(TraceFrame {
            name_ptr,
            name_len,
            file_ptr,
            file_len,
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
            let name = if frame.name_ptr.is_null() || frame.name_len == 0 {
                "<unknown>"
            } else {
                unsafe {
                    std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                        frame.name_ptr,
                        frame.name_len,
                    ))
                }
            };
            eprintln!("\x1b[36mtrace\x1b[0m {indent}← {name}()");
        }
    }
}

/// Snapshot the current call stack as `(function_name, line)` pairs, innermost
/// frame first. Used by the source-level debugger to build DAP `stackTrace`
/// responses. Must be called on the debuggee thread (the call stack is a
/// thread-local).
pub fn snapshot_frames() -> Vec<(String, u32)> {
    CALL_STACK.with(|stack| {
        let stack = stack.borrow();
        stack
            .iter()
            .rev()
            .map(|frame| {
                let name = if frame.name_ptr.is_null() || frame.name_len == 0 {
                    "<unknown>".to_string()
                } else {
                    unsafe {
                        std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                            frame.name_ptr,
                            frame.name_len,
                        ))
                        .to_string()
                    }
                };
                (name, frame.line)
            })
            .collect()
    })
}

/// Format the current call stack as a human-readable string.
///
/// Returns an empty string if the stack is empty.
///
/// KNOWN LIMITATION (line precision): each frame's `line` is set ONCE at
/// `kryos_trace_enter` to the function's *declaration* line (`MirFunction::
/// source_line`) and never advances, so every frame reports where the function
/// was defined, not the call/panic site inside it. A leaf frame that panics at
/// line 9 of a function declared at line 1 still prints `:1`.
///
/// Fixing this requires per-frame "current line" updates: emit a
/// `kryos_trace_line(line)` hook (setting the TOP frame's line -- always the
/// currently-executing frame) at each call site and each panic-capable
/// operation. The source lines are already available as MIR `DebugLine`
/// markers, but those are gated to the `kryos dap` debug path
/// (`debug_lines::debug_lines_active()`) because always-on per-statement
/// instrumentation adds a runtime hook to every statement and regresses hot
/// loops. Enabling it for normal builds (globally, or scoped to call/panic
/// sites) is a performance-vs-diagnostics tradeoff left as an owner decision;
/// it also requires the corresponding emission in BOTH the Cranelift and LLVM
/// backends.
pub fn format_stack_trace() -> String {
    CALL_STACK.with(|stack| {
        let stack = stack.borrow();
        if stack.is_empty() {
            return String::new();
        }
        let mut out = String::from("\nstack trace (most recent call last):\n");
        for (i, frame) in stack.iter().rev().enumerate() {
            let name = if frame.name_ptr.is_null() || frame.name_len == 0 {
                "<unknown>"
            } else {
                unsafe {
                    std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                        frame.name_ptr,
                        frame.name_len,
                    ))
                }
            };
            let file = if frame.file_ptr.is_null() || frame.file_len == 0 {
                "<unknown>"
            } else {
                unsafe {
                    std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                        frame.file_ptr,
                        frame.file_len,
                    ))
                }
            };
            let _ = writeln!(out, "  {i}: {name}() at {file}:{line}", line = frame.line);
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
