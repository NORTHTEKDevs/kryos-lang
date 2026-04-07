//! Panic handling for the Kryos runtime.
//!
//! Provides functions for compiled Kryos code to report fatal errors.
//! These functions print a diagnostic to stderr and abort the process.

use std::io::Write;

/// Format a panic message into a String (for testing without aborting).
pub fn format_panic_message(msg: &str) -> String {
    format!("kryos panic: {}", msg)
}

/// Format a panic message with source location (for testing without aborting).
pub fn format_panic_with_location(msg: &str, file: &str, line: u32, col: u32) -> String {
    format!("kryos panic at {}:{}:{}: {}", file, line, col, msg)
}

/// Print an error message to stderr and abort the process.
///
/// Called by compiled Kryos code when an unrecoverable error occurs.
#[no_mangle]
pub extern "C" fn kryos_panic(msg_ptr: *const u8, msg_len: usize) -> ! {
    let msg = if msg_ptr.is_null() || msg_len == 0 {
        "<no message>"
    } else {
        unsafe {
            let slice = std::slice::from_raw_parts(msg_ptr, msg_len);
            std::str::from_utf8(slice).unwrap_or("<invalid utf-8>")
        }
    };

    let formatted = format_panic_message(msg);
    let stack = crate::trace::format_stack_trace();
    let _ = writeln!(std::io::stderr(), "{}{}", formatted, stack);
    std::process::abort();
}

/// Print an error message with source location to stderr and abort.
///
/// Provides file, line, and column information for diagnostics.
#[no_mangle]
pub extern "C" fn kryos_panic_with_location(
    msg_ptr: *const u8,
    msg_len: usize,
    file_ptr: *const u8,
    file_len: usize,
    line: u32,
    col: u32,
) -> ! {
    let msg = if msg_ptr.is_null() || msg_len == 0 {
        "<no message>"
    } else {
        unsafe {
            let slice = std::slice::from_raw_parts(msg_ptr, msg_len);
            std::str::from_utf8(slice).unwrap_or("<invalid utf-8>")
        }
    };

    let file = if file_ptr.is_null() || file_len == 0 {
        "<unknown>"
    } else {
        unsafe {
            let slice = std::slice::from_raw_parts(file_ptr, file_len);
            std::str::from_utf8(slice).unwrap_or("<invalid utf-8>")
        }
    };

    let formatted = format_panic_with_location(msg, file, line, col);
    let stack = crate::trace::format_stack_trace();
    let _ = writeln!(std::io::stderr(), "{}{}", formatted, stack);
    std::process::abort();
}
