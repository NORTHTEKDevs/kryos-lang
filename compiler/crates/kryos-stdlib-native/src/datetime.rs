//! Date/time operations for the Kryos native stdlib.
//!
//! All time values are seconds (or milliseconds / nanoseconds) since the Unix
//! epoch, UTC. Date breakdowns use the proleptic Gregorian calendar.

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
#[no_mangle]
pub extern "C" fn kryos_time_now_millis() -> i64 {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_millis() as i64,
        Err(_) => -1,
    }
}

/// Returns the current time as nanoseconds since the Unix epoch.
#[no_mangle]
pub extern "C" fn kryos_time_now_nanos() -> i64 {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i64,
        Err(_) => -1,
    }
}

/// Sleep the current thread for `millis` milliseconds.
#[no_mangle]
pub extern "C" fn kryos_time_sleep_millis(millis: i64) {
    if millis <= 0 {
        return;
    }
    std::thread::sleep(std::time::Duration::from_millis(millis as u64));
}

// ─── Date breakdown ────────────────────────────────────────────────────────
//
// Civil-from-days algorithm (Howard Hinnant). Converts a count of days since
// the Unix epoch (1970-01-01) into (year, month, day) and back. Calendar is
// the proleptic Gregorian; year/month/day are 1-based.

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy =
        (153 * (if m > 2 { m - 3 } else { m + 9 }) as i64 + 2) / 5 + (d - 1) as i64;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Get the UTC year component for the given epoch-seconds value.
#[no_mangle]
pub extern "C" fn kryos_time_year_utc(epoch_secs: i64) -> i64 {
    let days = epoch_secs.div_euclid(86_400);
    civil_from_days(days).0
}

/// Get the UTC month (1-12) component for the given epoch-seconds value.
#[no_mangle]
pub extern "C" fn kryos_time_month_utc(epoch_secs: i64) -> i64 {
    let days = epoch_secs.div_euclid(86_400);
    civil_from_days(days).1 as i64
}

/// Get the UTC day-of-month (1-31) component for the given epoch-seconds value.
#[no_mangle]
pub extern "C" fn kryos_time_day_utc(epoch_secs: i64) -> i64 {
    let days = epoch_secs.div_euclid(86_400);
    civil_from_days(days).2 as i64
}

/// Get the UTC hour (0-23) component for the given epoch-seconds value.
#[no_mangle]
pub extern "C" fn kryos_time_hour_utc(epoch_secs: i64) -> i64 {
    epoch_secs.rem_euclid(86_400) / 3600
}

/// Get the UTC minute (0-59) component for the given epoch-seconds value.
#[no_mangle]
pub extern "C" fn kryos_time_minute_utc(epoch_secs: i64) -> i64 {
    (epoch_secs.rem_euclid(86_400) % 3600) / 60
}

/// Get the UTC second (0-59) component for the given epoch-seconds value.
#[no_mangle]
pub extern "C" fn kryos_time_second_utc(epoch_secs: i64) -> i64 {
    epoch_secs.rem_euclid(60)
}

/// Get the day-of-week for the given epoch-seconds (0=Sunday, 6=Saturday).
#[no_mangle]
pub extern "C" fn kryos_time_weekday_utc(epoch_secs: i64) -> i64 {
    let days = epoch_secs.div_euclid(86_400);
    (days + 4).rem_euclid(7)
}

/// Build an epoch-seconds value from UTC (year, month, day, hour, minute, second).
#[no_mangle]
pub extern "C" fn kryos_time_from_ymdhms_utc(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
) -> i64 {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return -1;
    }
    let days = days_from_civil(year, month as u32, day as u32);
    days * 86_400 + hour * 3600 + minute * 60 + second
}

/// Format epoch-seconds as a UTC RFC 3339 string. Writes the bytes into
/// `out` (caller-provided buffer of size ≥ 20) and returns the number of
/// bytes written. On overflow returns -1. Output: `YYYY-MM-DDTHH:MM:SSZ`.
#[no_mangle]
pub extern "C" fn kryos_time_format_rfc3339_utc(
    epoch_secs: i64,
    out: *mut u8,
    out_cap: usize,
) -> i64 {
    if out.is_null() || out_cap < 20 {
        return -1;
    }
    let days = epoch_secs.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let secs_of_day = epoch_secs.rem_euclid(86_400);
    let h = secs_of_day / 3600;
    let mi = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;
    let formatted = format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z");
    let bytes = formatted.as_bytes();
    if bytes.len() > out_cap {
        return -1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len());
    }
    bytes.len() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_civil_days() {
        // 1970-01-01 → day 0
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2000-01-01 → day 10957
        assert_eq!(days_from_civil(2000, 1, 1), 10957);
        assert_eq!(civil_from_days(10957), (2000, 1, 1));
    }

    #[test]
    fn ymdhms_to_epoch_and_back() {
        // 2026-05-18T12:34:56Z
        let secs = kryos_time_from_ymdhms_utc(2026, 5, 18, 12, 34, 56);
        assert!(secs > 0);
        assert_eq!(kryos_time_year_utc(secs), 2026);
        assert_eq!(kryos_time_month_utc(secs), 5);
        assert_eq!(kryos_time_day_utc(secs), 18);
        assert_eq!(kryos_time_hour_utc(secs), 12);
        assert_eq!(kryos_time_minute_utc(secs), 34);
        assert_eq!(kryos_time_second_utc(secs), 56);
    }

    #[test]
    fn weekday_known() {
        // 2026-05-18 is a Monday → weekday 1 (Sunday=0)
        let secs = kryos_time_from_ymdhms_utc(2026, 5, 18, 0, 0, 0);
        assert_eq!(kryos_time_weekday_utc(secs), 1);
    }

    #[test]
    fn rfc3339_format() {
        let secs = kryos_time_from_ymdhms_utc(2026, 5, 18, 12, 34, 56);
        let mut buf = [0u8; 32];
        let n = kryos_time_format_rfc3339_utc(secs, buf.as_mut_ptr(), buf.len()) as usize;
        let s = std::str::from_utf8(&buf[..n]).unwrap();
        assert_eq!(s, "2026-05-18T12:34:56Z");
    }
}
