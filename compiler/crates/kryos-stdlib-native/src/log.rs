//! Structured logging for Kryos programs.
//!
//! Outputs single-line key-value records to stderr by default. Format
//! is `LEVEL ts=<epoch_secs> msg="..." key1=value1 key2=value2`. Cheap
//! to parse with `awk` / `grep` / `jq`-style tools.
//!
//! Levels: 0=trace, 1=debug, 2=info, 3=warn, 4=error, 5=fatal.

use std::io::Write;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::SystemTime;

/// Minimum level that will be printed. Default: 2 (info).
static MIN_LEVEL: AtomicI32 = AtomicI32::new(2);

/// Set the minimum level. Anything below is silently dropped.
#[no_mangle]
pub extern "C" fn kryos_log_set_level(level: i32) {
    MIN_LEVEL.store(level, Ordering::Relaxed);
}

/// Log one line at the given level. `msg_ptr`/`msg_len` is the human
/// message; `kv_ptr`/`kv_len` is the already-formatted key-value
/// suffix (caller controls escaping). Both may be null/0.
#[no_mangle]
pub extern "C" fn kryos_log_emit(
    level: i32,
    msg_ptr: *const u8,
    msg_len: usize,
    kv_ptr: *const u8,
    kv_len: usize,
) {
    if level < MIN_LEVEL.load(Ordering::Relaxed) {
        return;
    }
    let level_name = match level {
        0 => "TRACE",
        1 => "DEBUG",
        2 => "INFO",
        3 => "WARN",
        4 => "ERROR",
        _ => "FATAL",
    };
    let msg = if msg_ptr.is_null() || msg_len == 0 {
        ""
    } else {
        unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(msg_ptr, msg_len))
        }
    };
    let kv = if kv_ptr.is_null() || kv_len == 0 {
        ""
    } else {
        unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(kv_ptr, kv_len)) }
    };

    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut stderr = std::io::stderr().lock();
    if kv.is_empty() {
        let _ = writeln!(stderr, "{level_name} ts={ts} msg=\"{msg}\"");
    } else {
        let _ = writeln!(stderr, "{level_name} ts={ts} msg=\"{msg}\" {kv}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_level_filters() {
        kryos_log_set_level(3); // warn or higher
        // Below threshold — should not print, but also not crash.
        kryos_log_emit(2, b"info".as_ptr(), 4, std::ptr::null(), 0);
        kryos_log_emit(3, b"warn".as_ptr(), 4, std::ptr::null(), 0);
        kryos_log_set_level(2);
    }
}
