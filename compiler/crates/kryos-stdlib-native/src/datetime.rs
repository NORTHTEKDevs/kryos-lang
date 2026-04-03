//! Date/time operations for the Kryos native stdlib.
//!
//! Uses `std::time::SystemTime` for wall-clock time.

use std::time::SystemTime;

/// Returns the current time as seconds since the Unix epoch.
///
/// Returns -1 if the system clock is before the epoch (should never happen).
#[no_mangle]
pub extern "C" fn kryos_time_now_secs() -> i64 {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(_) => -1,
    }
}

/// Returns the current time as milliseconds since the Unix epoch.
///
/// Returns -1 if the system clock is before the epoch.
#[no_mangle]
pub extern "C" fn kryos_time_now_millis() -> i64 {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_millis() as i64,
        Err(_) => -1,
    }
}
