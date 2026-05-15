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
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::missing_safety_doc)]

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

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

pub mod actor;
pub mod alloc;
pub mod arc;
pub mod array;
pub mod builtins;
pub mod channel;
pub mod exception;
pub mod fs;
pub mod future;
pub mod globals;
pub mod map;
pub mod panic;
pub mod spawn;
pub mod stack_guard;
pub mod string;
pub mod tensor;
pub mod trace;
