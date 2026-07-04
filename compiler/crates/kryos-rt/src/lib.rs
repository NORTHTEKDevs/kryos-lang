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

/// Global allocator for every Kryos program (the runtime is linked into all
/// AOT binaries and the JIT host). The Windows system heap RETAINS freed
/// blocks under small-allocation churn: a string workload with perfectly
/// balanced alloc/free (live ~1MB by our own memstats) still grew RSS past
/// 2GB through fragmentation. mimalloc returns memory to the OS and handles
/// churn; the same workload plateaus.
#[global_allocator]
static GLOBAL_ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

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
