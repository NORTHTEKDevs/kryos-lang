//! Source-level debugger runtime support.
//!
//! This module backs the self-contained `kryos dap` debugger. It has two parts:
//!
//! 1. **Debug controller** — a process-global, thread-safe controller that the
//!    DAP server (running on the host thread) and the debuggee (running on a
//!    dedicated thread, JIT-compiled by Cranelift) use to rendezvous. The
//!    instrumented program calls [`kryos_dbg_line`] before each source
//!    statement; the controller decides whether to stop (breakpoint hit or
//!    single-step) and, if so, blocks the debuggee thread until the DAP server
//!    resumes it.
//!
//! 2. **Stdout sink** — because the DAP protocol itself is framed over the
//!    process stdout, the debuggee's own `println` output must not be written
//!    to the real stdout (it would corrupt the protocol stream). When the
//!    debugger installs a sink, the print builtins route their output here
//!    instead; the DAP server forwards it as `output` events. A single relaxed
//!    atomic guards the fast path so normal `kryos run` is unaffected.
//!
//! None of this is active unless the `kryos dap` command installs it, so all
//! other execution paths (normal `kryos run`, `kryos build`, tests) behave
//! exactly as before.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Condvar, Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Stop reasons / events
// ---------------------------------------------------------------------------

/// Why the debuggee stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// Execution reached a line with an active breakpoint.
    Breakpoint,
    /// A single-step (`next`) completed at the following line.
    Step,
}

impl StopReason {
    /// The DAP `reason` string for a `stopped` event.
    pub fn as_dap_str(self) -> &'static str {
        match self {
            StopReason::Breakpoint => "breakpoint",
            StopReason::Step => "step",
        }
    }
}

/// What [`DebugController::wait_for_stop`] observed.
#[derive(Debug, Clone)]
pub enum StopEvent {
    /// The debuggee stopped at `line`. `seq` is a monotonic id used by the
    /// caller to avoid re-reporting the same stop.
    Stopped {
        seq: u64,
        line: i64,
        reason: StopReason,
    },
    /// The debuggee ran to completion.
    Finished,
}

// ---------------------------------------------------------------------------
// Controller state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum StepMode {
    /// Run until a breakpoint is hit.
    Run,
    /// Stop at the very next instrumented line.
    Step,
}

struct DebugState {
    breakpoints: std::collections::HashSet<i64>,
    step_mode: StepMode,
    /// True while the debuggee is parked at a stop, waiting to be resumed.
    stopped: bool,
    stopped_line: i64,
    stop_reason: StopReason,
    /// Monotonic stop counter — incremented on each new stop.
    stop_seq: u64,
    /// Set once the debuggee thread has run to completion.
    finished: bool,
    /// Most recent line the debuggee reported (whether or not it stopped).
    current_line: i64,
    /// Call-stack snapshot captured (on the debuggee thread) at the last stop,
    /// innermost frame first: (function name, line).
    frames: Vec<(String, i64)>,
}

/// Process-global debugger rendezvous point.
pub struct DebugController {
    inner: Mutex<DebugState>,
    cvar: Condvar,
}

static CONTROLLER: OnceLock<DebugController> = OnceLock::new();

/// Initialise (idempotently) and return the global debug controller. Called by
/// the `kryos dap` command before launching the debuggee.
pub fn controller() -> &'static DebugController {
    CONTROLLER.get_or_init(|| DebugController {
        inner: Mutex::new(DebugState {
            breakpoints: std::collections::HashSet::new(),
            step_mode: StepMode::Run,
            stopped: false,
            stopped_line: 0,
            stop_reason: StopReason::Breakpoint,
            stop_seq: 0,
            finished: false,
            current_line: 0,
            frames: Vec::new(),
        }),
        cvar: Condvar::new(),
    })
}

/// True if the debug controller has been initialised (i.e. we are running under
/// `kryos dap`). The instrumented hook short-circuits to a no-op otherwise.
fn controller_active() -> bool {
    CONTROLLER.get().is_some()
}

impl DebugController {
    /// Replace the active breakpoint line set (DAP server thread).
    pub fn set_breakpoints(&self, lines: &[i64]) {
        let mut st = self.lock();
        st.breakpoints = lines.iter().copied().collect();
    }

    /// Resume the debuggee, running until the next breakpoint (DAP `continue`).
    pub fn resume_continue(&self) {
        let mut st = self.lock();
        st.step_mode = StepMode::Run;
        st.stopped = false;
        self.cvar.notify_all();
    }

    /// Resume the debuggee for a single line step (DAP `next`).
    pub fn resume_step(&self) {
        let mut st = self.lock();
        st.step_mode = StepMode::Step;
        st.stopped = false;
        self.cvar.notify_all();
    }

    /// Block (DAP server thread) until the debuggee stops at a new line
    /// (`seq > last_seq`) or finishes.
    pub fn wait_for_stop(&self, last_seq: u64) -> StopEvent {
        let mut st = self.lock();
        loop {
            if st.stopped && st.stop_seq > last_seq {
                return StopEvent::Stopped {
                    seq: st.stop_seq,
                    line: st.stopped_line,
                    reason: st.stop_reason,
                };
            }
            if st.finished {
                return StopEvent::Finished;
            }
            st = self.cvar.wait(st).unwrap();
        }
    }

    /// Mark the debuggee as finished (called from the debuggee thread once
    /// `main` returns) and wake any waiter.
    pub fn mark_finished(&self) {
        let mut st = self.lock();
        st.finished = true;
        self.cvar.notify_all();
    }

    /// The line most recently reported by the debuggee.
    pub fn current_line(&self) -> i64 {
        self.lock().current_line
    }

    /// The call-stack snapshot captured at the most recent stop (innermost
    /// frame first).
    pub fn current_frames(&self) -> Vec<(String, i64)> {
        self.lock().frames.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, DebugState> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Called from the debuggee thread (via [`kryos_dbg_line`]) before each
    /// source statement. Decides whether to stop and, if so, parks the thread.
    fn on_line(&self, line: i64) {
        let mut st = self.lock();
        st.current_line = line;
        let hit_bp = st.breakpoints.contains(&line);
        let should_stop = match st.step_mode {
            StepMode::Step => true,
            StepMode::Run => hit_bp,
        };
        if !should_stop {
            return;
        }

        // Capture the call stack on THIS (debuggee) thread — `snapshot_frames`
        // reads a thread-local maintained by the trace runtime, so it must be
        // read here rather than from the DAP server thread.
        let mut frames: Vec<(String, i64)> = crate::trace::snapshot_frames()
            .into_iter()
            .map(|(name, l)| (name, l as i64))
            .collect();
        // The innermost frame's "line" from the trace stack is the function's
        // entry line; override it with the exact current line.
        if let Some(top) = frames.first_mut() {
            top.1 = line;
        }
        if frames.is_empty() {
            frames.push(("main".to_string(), line));
        }
        st.frames = frames;

        st.stop_reason = if hit_bp {
            StopReason::Breakpoint
        } else {
            StopReason::Step
        };
        st.stopped = true;
        st.stopped_line = line;
        st.stop_seq += 1;
        // Reset to run mode; a subsequent `next` re-arms single-stepping.
        st.step_mode = StepMode::Run;
        self.cvar.notify_all();

        // Park until the DAP server resumes us (or the session is torn down).
        while st.stopped && !st.finished {
            st = self.cvar.wait(st).unwrap();
        }
    }
}

// ---------------------------------------------------------------------------
// Instrumentation hook (called from JIT-compiled code)
// ---------------------------------------------------------------------------

/// Runtime hook emitted by the Cranelift backend before each source statement
/// when running under the debugger. Resolved in-process via the JIT symbol
/// table; never referenced by normal builds.
#[no_mangle]
pub extern "C" fn kryos_dbg_line(line: i64) {
    if !controller_active() {
        return;
    }
    controller().on_line(line);
}

// ---------------------------------------------------------------------------
// Stdout sink (keep debuggee output off the DAP protocol stream)
// ---------------------------------------------------------------------------

static SINK_ACTIVE: AtomicBool = AtomicBool::new(false);
static STDOUT_SINK: Mutex<Option<Sender<String>>> = Mutex::new(None);

/// Install (or clear, with `None`) a stdout sink. While set, the print builtins
/// route their stdout output to `sink` instead of the real stdout so the DAP
/// server can forward it as `output` events without corrupting its own
/// Content-Length framed stream.
pub fn set_stdout_sink(sink: Option<Sender<String>>) {
    let active = sink.is_some();
    *STDOUT_SINK.lock().unwrap() = sink;
    SINK_ACTIVE.store(active, Ordering::SeqCst);
}

/// Try to route `s` to the installed stdout sink. Returns `true` if it was
/// consumed (so the caller must NOT also write to stdout). The fast path is a
/// single relaxed atomic load, so normal runs pay essentially nothing.
#[inline]
pub fn sink_write(s: &str) -> bool {
    if !SINK_ACTIVE.load(Ordering::Relaxed) {
        return false;
    }
    if let Ok(guard) = STDOUT_SINK.lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(s.to_string());
            return true;
        }
    }
    false
}
