//! Exit-time allocation balance report (gated by env KRYOS_ALLOC_STATS).
//!
//! Counts the two heap-owner kinds whose balance the ownership fixes are
//! proven with, and prints one line per kind to stderr when the process
//! exits:
//!
//!   [alloc-stats] arc allocs=N frees=M live=L
//!   [alloc-stats] box allocs=N frees=M live=L
//!
//! * arc: `kryos_arc_alloc` vs. an arc release that reaches zero. `shared T`
//!   values and closure environments are arc blocks.
//! * box: `kryos_calloc` (struct/enum boxes) vs. a `kryos_free` that really
//!   releases the block. A `kryos_free` that only consumes an extra owner,
//!   or that is ignored as a double free, is not a free.
//!
//! `live` is allocs - frees; a program whose arc and box blocks are all
//! released ends at 0 for both kinds ("arc/box balanced"). That is NOT "no
//! leak": strings, arrays, maps, raw `alloc()` and the libc-malloc'd Range
//! are not counted here (see the older KRYOS_MEMSTATS diagnostic, and the
//! RSS-based tests/mem_*_gate.sh gates, for those). A negative `live` means
//! more frees than allocs, i.e. an over-release / double free.
//!
//! Enabling: only explicit truthy values turn the report on -- `1`, `true`,
//! `yes` or `on`, case-insensitive. Unset, empty, `0`, `false`, `no`, `off`
//! and anything else mean off (no output, no registered exit hook).
//!
//! Limits:
//! * KRYOS_FREE_DIAG=1 is NOT supported. Its quarantine path skips `drop_fn`,
//!   so every arc owned by a quarantined block's drop cascade is never
//!   released and stays "live"; the counts are not meaningful there. The
//!   report prints a `free_diag=on` marker line when that mode is active, and
//!   tests/mem_balance_gate.sh refuses to run with it set.
//! * A hard fault exit (stack overflow, access violation, abort) prints no
//!   report. The gate requires rc=0 for that reason. The runtime's own
//!   `process::exit` sites (exit builtin, panic, uncaught exception) do report.
//! * Under KRYOS_PLAIN_ALLOC=1 `kryos_free` has no double-free detection, so a
//!   double free counts twice and `live` goes negative (the gate FAILs on any
//!   negative live, which is the intended signal).
//! * The disabled-path cost (one relaxed atomic load and a branch per counted
//!   call) is reasoned, not benchmarked.
//!
//! The env var is read with C `getenv`, like KRYOS_PLAIN_ALLOC in alloc.rs,
//! so it is safe at any init point and never allocates. Enabling registers a
//! C `atexit` callback once; `report()` is also called before the runtime's
//! own `process::exit` sites, because on Windows `std::process::exit` can
//! bypass `atexit`. `report()` prints at most once.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

static ARC_ALLOCS: AtomicU64 = AtomicU64::new(0);
static ARC_FREES: AtomicU64 = AtomicU64::new(0);
static BOX_ALLOCS: AtomicU64 = AtomicU64::new(0);
static BOX_FREES: AtomicU64 = AtomicU64::new(0);

// 0 = undecided, 1 = off, 2 = on.
static MODE: AtomicU8 = AtomicU8::new(0);
static REPORTED: AtomicBool = AtomicBool::new(false);

extern "C" {
    fn getenv(name: *const u8) -> *const u8;
    fn atexit(cb: extern "C" fn()) -> i32;
}

extern "C" fn atexit_report() {
    report();
}

/// Explicit truthy values only: 1, true, yes, on (ASCII case-insensitive).
pub fn is_truthy(v: &[u8]) -> bool {
    [&b"1"[..], b"true", b"yes", b"on"]
        .iter()
        .any(|t| v.eq_ignore_ascii_case(t))
}

#[cold]
fn decide() -> bool {
    let v = unsafe { getenv(c"KRYOS_ALLOC_STATS".as_ptr() as *const u8) };
    let on = !v.is_null()
        && is_truthy(unsafe { std::ffi::CStr::from_ptr(v as *const std::ffi::c_char) }.to_bytes());
    // Racing first calls compute the same value; only the one that moves
    // MODE off 0 registers the exit hook.
    let prev = MODE
        .compare_exchange(0, if on { 2 } else { 1 }, Ordering::AcqRel, Ordering::Acquire)
        .unwrap_or_else(|seen| seen);
    if prev == 0 && on {
        unsafe {
            atexit(atexit_report);
        }
    }
    MODE.load(Ordering::Acquire) == 2
}

#[inline]
fn on() -> bool {
    match MODE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => decide(),
    }
}

/// Decide the mode (and register the exit hook when enabled) at process
/// start, so a program that never allocates a counted block still reports.
/// Called from `kryos_rt_init`, which every compiled `main` runs first.
pub fn init() {
    let _ = on();
}

#[inline]
pub fn note_arc_alloc() {
    if on() {
        ARC_ALLOCS.fetch_add(1, Ordering::Relaxed);
    }
}

#[inline]
pub fn note_arc_free() {
    if on() {
        ARC_FREES.fetch_add(1, Ordering::Relaxed);
    }
}

#[inline]
pub fn note_box_alloc() {
    if on() {
        BOX_ALLOCS.fetch_add(1, Ordering::Relaxed);
    }
}

#[inline]
pub fn note_box_free() {
    if on() {
        BOX_FREES.fetch_add(1, Ordering::Relaxed);
    }
}

/// Force the counters on without touching the process environment or
/// registering the exit hook. For the isolated integration test binary
/// (tests/allocstats.rs) only; not part of the runtime ABI.
#[doc(hidden)]
pub fn force_on_for_tests() {
    MODE.store(2, Ordering::Release);
    REPORTED.store(true, Ordering::Release);
}

/// Counter snapshot: (arc allocs, arc frees, box allocs, box frees).
pub fn snapshot() -> (u64, u64, u64, u64) {
    (
        ARC_ALLOCS.load(Ordering::Relaxed),
        ARC_FREES.load(Ordering::Relaxed),
        BOX_ALLOCS.load(Ordering::Relaxed),
        BOX_FREES.load(Ordering::Relaxed),
    )
}

/// Render the two report lines for a snapshot.
pub fn format_report(s: (u64, u64, u64, u64)) -> String {
    let live = |a: u64, f: u64| a as i128 - f as i128;
    format!(
        "[alloc-stats] arc allocs={} frees={} live={}\n[alloc-stats] box allocs={} frees={} live={}\n",
        s.0,
        s.1,
        live(s.0, s.1),
        s.2,
        s.3,
        live(s.2, s.3)
    )
}

/// Print the report to stderr once. No-op unless KRYOS_ALLOC_STATS is truthy.
pub fn report() {
    if !on() {
        return;
    }
    if REPORTED.swap(true, Ordering::AcqRel) {
        return;
    }
    use std::io::Write;
    let mut text = format_report(snapshot());
    if crate::free_diag() {
        text.push_str(
            "[alloc-stats] free_diag=on: arc/box counts are NOT meaningful (quarantine skips drop_fn)\n",
        );
    }
    let _ = std::io::stderr().write_all(text.as_bytes());
}

// Counting through the real alloc/arc entry points is tested in
// tests/allocstats.rs: it needs a process where nothing else touches the
// counters, which a unit test inside the shared lib test binary cannot give.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_is_one_line_per_kind() {
        assert_eq!(
            format_report((5, 3, 10, 10)),
            "[alloc-stats] arc allocs=5 frees=3 live=2\n[alloc-stats] box allocs=10 frees=10 live=0\n"
        );
    }

    #[test]
    fn negative_live_is_printed_as_negative() {
        assert!(format_report((1, 3, 0, 0)).contains("arc allocs=1 frees=3 live=-2"));
    }

    #[test]
    fn only_explicit_truthy_values_enable() {
        for v in ["1", "true", "TRUE", "True", "yes", "YES", "on", "On"] {
            assert!(is_truthy(v.as_bytes()), "{v} must enable");
        }
        for v in ["", "0", "false", "FALSE", "off", "no", "2", "enabled", "1 ", " 1", "truee"] {
            assert!(!is_truthy(v.as_bytes()), "{v:?} must not enable");
        }
    }
}
