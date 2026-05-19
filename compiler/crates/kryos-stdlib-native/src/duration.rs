//! Duration arithmetic and human formatting.
//!
//! Durations are i64 nanoseconds — same shape as `time_now_nanos()` in
//! `std::datetime`. All functions are pure (no syscalls).

/// Convert milliseconds to nanoseconds.
#[no_mangle]
pub extern "C" fn kryos_dur_from_millis(ms: i64) -> i64 {
    ms.saturating_mul(1_000_000)
}

/// Convert seconds to nanoseconds.
#[no_mangle]
pub extern "C" fn kryos_dur_from_secs(s: i64) -> i64 {
    s.saturating_mul(1_000_000_000)
}

/// Convert minutes to nanoseconds.
#[no_mangle]
pub extern "C" fn kryos_dur_from_mins(m: i64) -> i64 {
    m.saturating_mul(60_000_000_000)
}

/// Convert hours to nanoseconds (saturates near i64::MAX).
#[no_mangle]
pub extern "C" fn kryos_dur_from_hours(h: i64) -> i64 {
    h.saturating_mul(3_600_000_000_000)
}

/// Format `nanos` as a human-readable string: 3s, 200ms, 45us, 12ns, 2.5min, 1h30min.
/// Writes into `out` (cap recommended >= 32). Returns bytes written or -1.
#[no_mangle]
pub extern "C" fn kryos_dur_format(nanos: i64, out: *mut u8, out_cap: usize) -> i64 {
    if out.is_null() {
        return -1;
    }
    let s = format_dur(nanos);
    let bytes = s.as_bytes();
    if bytes.len() > out_cap {
        return -1;
    }
    let dst = unsafe { std::slice::from_raw_parts_mut(out, bytes.len()) };
    dst.copy_from_slice(bytes);
    bytes.len() as i64
}

fn format_dur(nanos: i64) -> String {
    let abs = nanos.unsigned_abs();
    let sign = if nanos < 0 { "-" } else { "" };
    if abs < 1_000 {
        format!("{sign}{abs}ns")
    } else if abs < 1_000_000 {
        format!("{sign}{:.1}us", abs as f64 / 1_000.0)
    } else if abs < 1_000_000_000 {
        format!("{sign}{:.1}ms", abs as f64 / 1_000_000.0)
    } else if abs < 60_000_000_000 {
        format!("{sign}{:.2}s", abs as f64 / 1_000_000_000.0)
    } else if abs < 3_600_000_000_000 {
        let m = abs / 60_000_000_000;
        let rem_s = (abs % 60_000_000_000) / 1_000_000_000;
        format!("{sign}{m}min{rem_s}s")
    } else {
        let h = abs / 3_600_000_000_000;
        let rem_m = (abs % 3_600_000_000_000) / 60_000_000_000;
        format!("{sign}{h}h{rem_m}min")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(n: i64) -> String {
        format_dur(n)
    }

    #[test]
    fn picks_correct_unit() {
        assert_eq!(fmt(500), "500ns");
        assert_eq!(fmt(5_000), "5.0us");
        assert_eq!(fmt(5_000_000), "5.0ms");
        assert_eq!(fmt(5_000_000_000), "5.00s");
        assert_eq!(fmt(125_000_000_000), "2min5s");
        assert_eq!(fmt(7_320_000_000_000), "2h2min");
    }

    #[test]
    fn negative_durations() {
        assert_eq!(fmt(-200), "-200ns");
        assert_eq!(fmt(-5_000_000), "-5.0ms");
    }

    #[test]
    fn from_helpers() {
        assert_eq!(kryos_dur_from_secs(2), 2_000_000_000);
        assert_eq!(kryos_dur_from_millis(500), 500_000_000);
        assert_eq!(kryos_dur_from_mins(1), 60_000_000_000);
    }
}
