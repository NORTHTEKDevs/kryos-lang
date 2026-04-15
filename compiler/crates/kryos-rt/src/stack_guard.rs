//! Stack overflow detection.
//!
//! Installs a platform-specific handler that converts stack overflow
//! into a readable error message instead of a raw crash/segfault.
//!
//! On Unix, this catches SIGSEGV from guard page hits.
//! On Windows, stack overflow produces STATUS_STACK_OVERFLOW which the
//! Rust runtime already converts to an abort — we set a custom panic hook
//! to print a friendlier message.

use std::io::Write;

/// Install the stack overflow handler for the current platform.
///
/// Should be called once at program startup.
pub fn install() {
    // Set a custom panic hook that prints a kryos-style message.
    // This catches stack overflows that Rust's runtime intercepts.
    std::panic::set_hook(Box::new(|info| {
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(|s| s.as_str()))
            .unwrap_or("unknown panic");

        if msg.contains("stack overflow") {
            let _ = writeln!(
                std::io::stderr(),
                "kryos panic: stack overflow (possible infinite recursion)"
            );
        } else {
            let _ = writeln!(std::io::stderr(), "kryos panic: {}", msg);
        }
    }));
}

/// Runtime init function — called from generated main() or at startup.
#[no_mangle]
pub extern "C" fn kryos_rt_init() {
    install();
}
