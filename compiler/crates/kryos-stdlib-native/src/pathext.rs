//! Path operations not in `std::path`: normalize, components, classification.
//!
//! Pure string manipulation — no filesystem syscalls. For existence/type
//! checks see `path::kryos_path_is_file` etc. or `std::fs::*`.

/// Returns 1 if `p` is an absolute path on the current platform (Unix
/// starts with `/`, Windows starts with drive letter + `:`).
#[no_mangle]
pub extern "C" fn kryos_path_is_absolute(p: *const u8, len: usize) -> i32 {
    if p.is_null() || len == 0 {
        return 0;
    }
    let bytes = unsafe { std::slice::from_raw_parts(p, len) };
    if bytes[0] == b'/' {
        return 1;
    }
    if cfg!(windows) && len >= 3 {
        let c0 = bytes[0];
        let c1 = bytes[1];
        let c2 = bytes[2];
        if (c0.is_ascii_alphabetic()) && c1 == b':' && (c2 == b'/' || c2 == b'\\') {
            return 1;
        }
    }
    0
}

/// Normalize `p` in-place per these rules:
///  - Replace `\` with `/` on all platforms.
///  - Collapse runs of `/` to a single `/`.
///  - Resolve `.` segments (removed).
///  - Resolve `..` segments (pop previous unless at the root).
/// Returns the new logical length, or -1 on null input.
#[no_mangle]
pub extern "C" fn kryos_path_normalize(
    src: *const u8,
    src_len: usize,
    out: *mut u8,
    out_cap: usize,
) -> i64 {
    if src.is_null() || out.is_null() {
        return -1;
    }
    let s = unsafe { std::slice::from_raw_parts(src, src_len) };
    let raw = std::str::from_utf8(s).unwrap_or("").replace('\\', "/");
    let is_abs = raw.starts_with('/');

    let mut parts: Vec<&str> = Vec::new();
    for seg in raw.split('/') {
        match seg {
            "" | "." => continue,
            ".." => {
                if parts.last().map(|s| *s != "..").unwrap_or(false) {
                    parts.pop();
                } else if !is_abs {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    let mut result = String::new();
    if is_abs {
        result.push('/');
    }
    result.push_str(&parts.join("/"));
    if result.is_empty() {
        result.push('.');
    }

    let bytes = result.as_bytes();
    if bytes.len() > out_cap {
        return -1;
    }
    let dst = unsafe { std::slice::from_raw_parts_mut(out, bytes.len()) };
    dst.copy_from_slice(bytes);
    bytes.len() as i64
}

/// Count the number of path components in `p` (e.g. "a/b/c" → 3,
/// "/a/b/c" → 3, "" → 0, "." → 1). After normalization.
#[no_mangle]
pub extern "C" fn kryos_path_component_count(p: *const u8, len: usize) -> i64 {
    if p.is_null() || len == 0 {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts(p, len) };
    let raw = std::str::from_utf8(s).unwrap_or("").replace('\\', "/");
    raw.split('/').filter(|seg| !seg.is_empty()).count() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> String {
        let mut buf = [0u8; 256];
        let r = kryos_path_normalize(s.as_ptr(), s.len(), buf.as_mut_ptr(), buf.len());
        String::from_utf8_lossy(&buf[..r as usize]).to_string()
    }

    #[test]
    fn collapses_slashes() {
        assert_eq!(n("a//b"), "a/b");
        assert_eq!(n("a/././b"), "a/b");
    }

    #[test]
    fn resolves_dotdot() {
        assert_eq!(n("a/b/../c"), "a/c");
        assert_eq!(n("a/b/.."), "a");
        assert_eq!(n("a/../../b"), "../b");
    }

    #[test]
    fn handles_absolute() {
        assert_eq!(n("/a/b/../c"), "/a/c");
        assert_eq!(n("/.."), "/");
    }

    #[test]
    fn windows_backslashes() {
        assert_eq!(n("a\\b\\c"), "a/b/c");
    }

    #[test]
    fn component_count_basic() {
        assert_eq!(
            kryos_path_component_count(b"a/b/c".as_ptr(), 5),
            3
        );
        assert_eq!(
            kryos_path_component_count(b"".as_ptr(), 0),
            0
        );
        assert_eq!(
            kryos_path_component_count(b"/a/b".as_ptr(), 4),
            2
        );
    }

    #[test]
    fn absolute_detection() {
        assert_eq!(kryos_path_is_absolute(b"/a".as_ptr(), 2), 1);
        assert_eq!(kryos_path_is_absolute(b"a/b".as_ptr(), 3), 0);
    }
}
