//! Number formatting helpers — hex/binary/octal, padded decimals,
//! human-readable byte sizes. Output written to caller-provided buffers.

/// Format `value` as lowercase hexadecimal with `0x` prefix.
/// Writes up to 18 bytes (sign + "0x" + 16 hex digits + NUL).
/// Returns bytes written, or -1 on null/short buffer.
#[no_mangle]
pub extern "C" fn kryos_fmt_hex(value: i64, out: *mut u8, out_cap: usize) -> i64 {
    if out.is_null() || out_cap < 4 {
        return -1;
    }
    let s = format!("{value:#x}");
    let bytes = s.as_bytes();
    if bytes.len() > out_cap {
        return -1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len());
    }
    bytes.len() as i64
}

/// Format `value` as binary with `0b` prefix. Up to 66 bytes.
#[no_mangle]
pub extern "C" fn kryos_fmt_bin(value: i64, out: *mut u8, out_cap: usize) -> i64 {
    if out.is_null() {
        return -1;
    }
    let s = format!("{value:#b}");
    let bytes = s.as_bytes();
    if bytes.len() > out_cap {
        return -1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len());
    }
    bytes.len() as i64
}

/// Format `value` left-padded with zeros to `width` digits.
/// e.g. 42, width 5 → "00042".
#[no_mangle]
pub extern "C" fn kryos_fmt_decimal_padded(
    value: i64,
    width: usize,
    out: *mut u8,
    out_cap: usize,
) -> i64 {
    if out.is_null() {
        return -1;
    }
    let s = format!("{value:0width$}");
    let bytes = s.as_bytes();
    if bytes.len() > out_cap {
        return -1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len());
    }
    bytes.len() as i64
}

/// Format a byte count as `<n.n><unit>` where unit ∈ {B, KB, MB, GB, TB}.
/// e.g. 1500 → "1.5 KB", 1572864 → "1.5 MB".
#[no_mangle]
pub extern "C" fn kryos_fmt_bytes(value: i64, out: *mut u8, out_cap: usize) -> i64 {
    if out.is_null() {
        return -1;
    }
    let v = value as f64;
    let s = if v < 1_024.0 {
        format!("{value} B")
    } else if v < 1_048_576.0 {
        format!("{:.1} KB", v / 1_024.0)
    } else if v < 1_073_741_824.0 {
        format!("{:.1} MB", v / 1_048_576.0)
    } else if v < 1_099_511_627_776.0 {
        format!("{:.1} GB", v / 1_073_741_824.0)
    } else {
        format!("{:.1} TB", v / 1_099_511_627_776.0)
    };
    let bytes = s.as_bytes();
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

    fn round(f: extern "C" fn(i64, *mut u8, usize) -> i64, v: i64) -> String {
        let mut buf = [0u8; 64];
        let n = f(v, buf.as_mut_ptr(), buf.len()) as usize;
        String::from_utf8_lossy(&buf[..n]).to_string()
    }

    #[test]
    fn hex_format() {
        assert_eq!(round(kryos_fmt_hex, 255), "0xff");
        assert_eq!(round(kryos_fmt_hex, 0), "0x0");
        assert_eq!(round(kryos_fmt_hex, 4096), "0x1000");
    }

    #[test]
    fn bin_format() {
        assert_eq!(round(kryos_fmt_bin, 5), "0b101");
        assert_eq!(round(kryos_fmt_bin, 0), "0b0");
    }

    #[test]
    fn bytes_format() {
        assert_eq!(round(kryos_fmt_bytes, 100), "100 B");
        assert_eq!(round(kryos_fmt_bytes, 1500), "1.5 KB");
        assert_eq!(round(kryos_fmt_bytes, 1_572_864), "1.5 MB");
        assert_eq!(round(kryos_fmt_bytes, 1_073_741_824), "1.0 GB");
    }

    #[test]
    fn decimal_padded() {
        let mut buf = [0u8; 16];
        let n = kryos_fmt_decimal_padded(42, 5, buf.as_mut_ptr(), buf.len()) as usize;
        assert_eq!(&buf[..n], b"00042");
    }
}
