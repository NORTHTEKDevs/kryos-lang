//! Semantic version parsing + comparison (https://semver.org).
//!
//! Supports `MAJOR.MINOR.PATCH` with optional `-prerelease` and
//! `+build` suffixes. The build metadata is ignored for comparison
//! per the spec.

/// Parse a semver string into `(major, minor, patch, has_pre)`.
/// Returns 1 on success, 0 on malformed input.
#[no_mangle]
pub extern "C" fn kryos_semver_parse(
    s: *const u8,
    len: usize,
    out_major: *mut i64,
    out_minor: *mut i64,
    out_patch: *mut i64,
    out_has_pre: *mut i64,
) -> i32 {
    if s.is_null() || out_major.is_null() || out_minor.is_null() || out_patch.is_null() {
        return 0;
    }
    let bytes = unsafe { std::slice::from_raw_parts(s, len) };
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    // Strip the optional `v` prefix (common in git tags).
    let text = text.trim_start_matches('v');
    // Strip the build metadata.
    let text = text.splitn(2, '+').next().unwrap_or(text);
    // Split the prerelease.
    let (core, has_pre) = match text.split_once('-') {
        Some((c, _)) => (c, true),
        None => (text, false),
    };
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return 0;
    }
    let (Ok(maj), Ok(min), Ok(pat)) =
        (parts[0].parse::<u64>(), parts[1].parse::<u64>(), parts[2].parse::<u64>())
    else {
        return 0;
    };
    unsafe {
        *out_major = maj as i64;
        *out_minor = min as i64;
        *out_patch = pat as i64;
        if !out_has_pre.is_null() {
            *out_has_pre = if has_pre { 1 } else { 0 };
        }
    }
    1
}

/// Compare two parsed semvers. Returns -1, 0, or 1. Per spec, a prerelease
/// version sorts *before* the corresponding non-prerelease version.
#[no_mangle]
pub extern "C" fn kryos_semver_compare(
    a_major: i64,
    a_minor: i64,
    a_patch: i64,
    a_has_pre: i64,
    b_major: i64,
    b_minor: i64,
    b_patch: i64,
    b_has_pre: i64,
) -> i32 {
    let ord = (a_major, a_minor, a_patch).cmp(&(b_major, b_minor, b_patch));
    match ord {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Equal => {
            // Equal core triplet — compare prerelease flags.
            // No pre > with pre (the released version is newer).
            match (a_has_pre, b_has_pre) {
                (0, 1) => 1,
                (1, 0) => -1,
                _ => 0,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Option<(i64, i64, i64, i64)> {
        let (mut maj, mut min, mut pat, mut pre) = (0i64, 0i64, 0i64, 0i64);
        let r = kryos_semver_parse(
            s.as_ptr(),
            s.len(),
            &mut maj,
            &mut min,
            &mut pat,
            &mut pre,
        );
        if r == 1 {
            Some((maj, min, pat, pre))
        } else {
            None
        }
    }

    #[test]
    fn parses_canonical() {
        assert_eq!(parse("1.2.3"), Some((1, 2, 3, 0)));
        assert_eq!(parse("0.1.0"), Some((0, 1, 0, 0)));
        assert_eq!(parse("v4.29.0"), Some((4, 29, 0, 0)));
    }

    #[test]
    fn parses_prerelease() {
        assert_eq!(parse("1.0.0-rc.1"), Some((1, 0, 0, 1)));
        assert_eq!(parse("3.0.0-rc.1"), Some((3, 0, 0, 1)));
    }

    #[test]
    fn ignores_build_metadata() {
        assert_eq!(parse("1.0.0+abc.123"), Some((1, 0, 0, 0)));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse("garbage"), None);
        assert_eq!(parse("1.2"), None);
        assert_eq!(parse("1.2.3.4"), None);
    }

    #[test]
    fn compare_ordering() {
        // 1.0.0 < 2.0.0
        assert_eq!(kryos_semver_compare(1, 0, 0, 0, 2, 0, 0, 0), -1);
        // 2.0.0 > 1.0.0
        assert_eq!(kryos_semver_compare(2, 0, 0, 0, 1, 0, 0, 0), 1);
        // Equal
        assert_eq!(kryos_semver_compare(1, 0, 0, 0, 1, 0, 0, 0), 0);
        // 1.0.0-rc.1 < 1.0.0
        assert_eq!(kryos_semver_compare(1, 0, 0, 1, 1, 0, 0, 0), -1);
        // 1.0.0 > 1.0.0-rc.1
        assert_eq!(kryos_semver_compare(1, 0, 0, 0, 1, 0, 0, 1), 1);
    }
}
