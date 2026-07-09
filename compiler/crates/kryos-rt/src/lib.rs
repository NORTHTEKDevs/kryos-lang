//! Kryos Runtime Library
//!
//! Core runtime services for compiled Kryos programs. All public functions
//! are `#[no_mangle] extern "C"` for linking from compiled Kryos code.
//!
//! This crate has zero dependencies on other kryos crates.
//!
//! # Safety
//!
//! All public functions in this crate are FFI boundary functions called
//! exclusively from Kryos-compiled machine code via the C ABI. They accept
//! raw pointers and integer handles because they operate below Rust's safety
//! model. Pointer validity is guaranteed by the Kryos compiler's ownership
//! analysis and codegen, not by Rust's borrow checker.
//!
//! # Memory model (as of v4.43.0-rc.4)
//!
//! All three heap container types — `KryosArray`, `KryosString`,
//! `MapHeader` — carry a `ref_count: i64` field and follow a
//! **share-on-clone, leak-on-free** policy:
//!
//! * `kryos_array_clone` / `kryos_string_clone` / `kryos_map_clone`:
//!   increment ref_count and return the same pointer. Trades the original
//!   independent-deep-copy semantics for O(1) sharing — necessary to
//!   unblock stage-1's tokenize/parse hot path which calls these
//!   thousands of times per compilation.
//! * `kryos_array_retain` / `kryos_string_retain` / `kryos_map_retain`:
//!   symmetric explicit retain ABI for codegen.
//! * `kryos_array_free` / `kryos_string_free` / `kryos_map_free`: pure
//!   no-ops. The refcount infrastructure exists (retain still increments)
//!   but no deallocation happens until process exit.
//!
//! Per-invocation memory leak: ~80 MB worst case (full self-host compile).
//! Bounded, well under the leak-guard 2 GB threshold, and harmless for
//! short-lived CLI use. For long-running consumers (LSP server, watch
//! mode), the codegen retain-emission audit must land first so the
//! `*_free` functions can switch to real refcount-decrement-and-dealloc.
//!
//! See `docs/20-self-hosting.md` for the bigger context.
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::missing_safety_doc)]

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

// Allocator note (2026-07-04): mimalloc as #[global_allocator] halves the
// Windows system heap's freed-block retention under string churn (peak
// 2.6GB -> 1.5GB on the mem-probe workload; alloc/free COUNTS balance
// either way -- see memstats). It is NOT enabled because the Cranelift JIT
// host miscompiles/crashes with it (tests/smoke/test_ffi2 segfaults at the
// first allocation after an if-heavy FFI sequence; AOT is unaffected).
// The dependency stays in Cargo.toml for a future AOT-only integration
// (separate allocator staticlib on the AOT link line, never in the JIT
// host). Until then, retention behavior is the platform allocator's.

/// When true, runtime functions (assert) store failure info in a thread-local
/// instead of aborting — used by the `@test` annotation runner.
static TEST_MODE: AtomicBool = AtomicBool::new(false);

thread_local! {
    static TEST_FAILURE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Enable or disable test mode.
pub fn set_test_mode(enabled: bool) {
    TEST_MODE.store(enabled, Ordering::SeqCst);
    if enabled {
        // Clear any stale failure.
        TEST_FAILURE.with(|f| *f.borrow_mut() = None);
    }
}

/// Returns true if the runtime is in test mode.
pub fn is_test_mode() -> bool {
    TEST_MODE.load(Ordering::SeqCst)
}

/// Record a test failure message (called from assert in test mode).
pub fn set_test_failure(msg: String) {
    TEST_FAILURE.with(|f| *f.borrow_mut() = Some(msg));
}

/// Take the stored test failure message (if any), clearing it.
pub fn take_test_failure() -> Option<String> {
    TEST_FAILURE.with(|f| f.borrow_mut().take())
}

/// When KRYOS_LEAK_ON_ZERO=1, the *_free runtime entry points decrement
/// refcount but skip the dealloc path when count hits zero. Restores the
/// H41 bootstrap-reliable model behind an opt-in knob, without requiring
/// the full codegen retain-emission audit. Probed once on first call.
#[inline]
pub fn leak_on_zero() -> bool {
    use std::sync::atomic::AtomicU8;
    static PROBED: AtomicU8 = AtomicU8::new(0);
    static VALUE: AtomicBool = AtomicBool::new(false);
    if PROBED.load(Ordering::Relaxed) == 0 {
        let on = std::env::var("KRYOS_LEAK_ON_ZERO")
            .map(|v| v != "0" && !v.is_empty())
            .unwrap_or(false);
        VALUE.store(on, Ordering::Relaxed);
        PROBED.store(1, Ordering::Relaxed);
    }
    VALUE.load(Ordering::Relaxed)
}

/// When KRYOS_FREE_DIAG=1, the *_free entry points never deallocate;
/// instead they decrement normally and REPORT any over-free (a free call
/// on a header whose refcount already reached zero) to stderr with the
/// container's content. Converts silent heap corruption from a latent
/// double-free into a precise, greppable diagnosis. Debug-only knob.
#[inline]
pub fn free_diag() -> bool {
    use std::sync::atomic::AtomicU8;
    static PROBED: AtomicU8 = AtomicU8::new(0);
    static VALUE: AtomicBool = AtomicBool::new(false);
    if PROBED.load(Ordering::Relaxed) == 0 {
        let on = std::env::var("KRYOS_FREE_DIAG")
            .map(|v| v != "0" && !v.is_empty())
            .unwrap_or(false);
        VALUE.store(on, Ordering::Relaxed);
        PROBED.store(1, Ordering::Relaxed);
    }
    VALUE.load(Ordering::Relaxed)
}

/// Free-site marker (debug tagging builds only): generated code calls this
/// with a site id immediately before each container-free call. The diag
/// reporter prints the last site so a double-free names its emitting
/// function (resolve the id against the KRYOS_EMIT_FREE_TAGS TSV).
#[no_mangle]
pub extern "C" fn kryos_diag_site(site: i64) -> i64 {
    LAST_FREE_SITE.with(|c| c.set(site));
    0
}

thread_local! {
    static LAST_FREE_SITE: std::cell::Cell<i64> = const { std::cell::Cell::new(-1) };
}

/// Bounded reporter for free_diag: prints the first 60 reports, then counts.
pub fn diag_report(msg: &str) {
    use std::sync::atomic::AtomicU32;
    static COUNT: AtomicU32 = AtomicU32::new(0);
    static LIMIT: AtomicU32 = AtomicU32::new(0);
    let mut cap = LIMIT.load(Ordering::Relaxed);
    if cap == 0 {
        cap = std::env::var("KRYOS_FREE_DIAG_MAX")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);
        LIMIT.store(cap, Ordering::Relaxed);
    }
    let n = COUNT.fetch_add(1, Ordering::Relaxed);
    if n < cap {
        let site = LAST_FREE_SITE.with(|c| c.get());
        eprintln!("KRYOS-FREE-DIAG[{n}]: {msg} site={site}");
        diag_stack();
    } else if n == cap {
        eprintln!("KRYOS-FREE-DIAG: (further reports suppressed)");
    }
}

/// When KRYOS_FREE_DIAG_STACK=1, append the caller stack as module-relative
/// RVAs to each diag report. Resolve against the binary's PDB (build with
/// `kryos build -g`) via llvm-symbolizer, or against a /MAP file.
#[cfg(windows)]
fn diag_stack() {
    if std::env::var("KRYOS_FREE_DIAG_STACK").map(|v| v != "0" && !v.is_empty()) != Ok(true) {
        return;
    }
    extern "system" {
        fn RtlCaptureStackBackTrace(
            frames_to_skip: u32,
            frames_to_capture: u32,
            back_trace: *mut *mut core::ffi::c_void,
            hash: *mut u32,
        ) -> u16;
        fn GetModuleHandleW(name: *const u16) -> *mut core::ffi::c_void;
    }
    let mut frames = [std::ptr::null_mut(); 12];
    let n = unsafe { RtlCaptureStackBackTrace(2, 12, frames.as_mut_ptr(), std::ptr::null_mut()) };
    let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
    let rvas: Vec<String> = frames[..n as usize]
        .iter()
        .map(|&f| {
            let a = f as usize;
            if a >= base { format!("+{:#x}", a - base) } else { format!("{a:#x}") }
        })
        .collect();
    eprintln!("  stack: {}", rvas.join(" "));
}

#[cfg(not(windows))]
fn diag_stack() {}

/// Type-stable header freelist. Container headers (KryosString / KryosArray /
/// MapHeader) are NEVER returned to the OS heap on free -- they are wiped to
/// an inert sentinel (len=0, data=null, rc=0) and recycled for the SAME type.
/// Rationale: generated code still contains a tail of historically-forgiven
/// stale releases/reads of dead headers (the pre-header-free runtime leaked
/// headers precisely to absorb them). Freeing headers to the OS turned each
/// of those into ntdll heap corruption (0xC0000374 fastfail) at scale -- the
/// merged self-host bootstrap regression. Type-stable recycling restores that
/// forgiveness with bounded memory: pool size <= peak live count, and reuse
/// keeps RSS flat on churn (the property the header-free change bought).
pub struct HeaderPool {
    list: std::sync::Mutex<Vec<usize>>,
}

impl HeaderPool {
    pub const fn new() -> Self {
        Self { list: std::sync::Mutex::new(Vec::new()) }
    }
    #[inline]
    pub fn put(&self, p: *mut u8) {
        if let Ok(mut l) = self.list.lock() {
            l.push(p as usize);
        }
    }
    #[inline]
    pub fn get(&self) -> Option<*mut u8> {
        self.list.lock().ok().and_then(|mut l| l.pop()).map(|u| u as *mut u8)
    }
}

pub mod actor;
pub mod alloc;
pub mod arc;
pub mod array;
pub mod builtins;
pub mod channel;
pub mod budget;
pub mod debug;
pub mod exception;
pub mod executor;
pub mod floatbits;
pub mod fs;
pub mod future;
pub mod globals;
pub mod memstats;
pub mod fault;
pub mod map;
pub mod panic;
pub mod spawn;
pub mod stack_guard;
pub mod string;
pub mod tensor;
pub mod trace;
