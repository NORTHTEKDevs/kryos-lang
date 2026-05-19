//! Iterator-style helpers for i64 slices that aren't reductions
//! (those live in `collections`). Map / filter / scan / range over
//! caller-owned buffers.

/// Fill `out` (capacity `len`) with `start, start+step, start+2*step, ...`.
/// Returns the number of items written.
#[no_mangle]
pub extern "C" fn kryos_iter_range(
    start: i64,
    step: i64,
    len: usize,
    out: *mut i64,
) -> i64 {
    if out.is_null() || len == 0 {
        return 0;
    }
    let dst = unsafe { std::slice::from_raw_parts_mut(out, len) };
    let mut v = start;
    for i in 0..len {
        dst[i] = v;
        v = v.wrapping_add(step);
    }
    len as i64
}

/// Filter `src[..src_len]` keeping only values matching `predicate_kind`:
///   0 = positive   (> 0)
///   1 = negative   (< 0)
///   2 = even       (v % 2 == 0)
///   3 = odd        (v % 2 != 0)
///   4 = >= threshold
///   5 = <= threshold
///
/// Writes survivors into `out` (capacity `out_cap`). Returns the number of
/// items written, or -1 on overflow.
#[no_mangle]
pub extern "C" fn kryos_iter_filter_i64(
    src: *const i64,
    src_len: usize,
    predicate_kind: i32,
    threshold: i64,
    out: *mut i64,
    out_cap: usize,
) -> i64 {
    if src.is_null() || out.is_null() {
        return -1;
    }
    let s = unsafe { std::slice::from_raw_parts(src, src_len) };
    let d = unsafe { std::slice::from_raw_parts_mut(out, out_cap) };
    let mut w = 0usize;
    for &v in s {
        let keep = match predicate_kind {
            0 => v > 0,
            1 => v < 0,
            2 => v.rem_euclid(2) == 0,
            3 => v.rem_euclid(2) != 0,
            4 => v >= threshold,
            5 => v <= threshold,
            _ => false,
        };
        if !keep {
            continue;
        }
        if w >= out_cap {
            return -1;
        }
        d[w] = v;
        w += 1;
    }
    w as i64
}

/// Apply a closed-form transform to each element of `src` into `dst`:
///   0 = identity
///   1 = abs
///   2 = negate
///   3 = square
///   4 = add `c`
///   5 = mul `c`
///
/// Returns the number of items written (same as `len`), or -1 on null/short
/// output.
#[no_mangle]
pub extern "C" fn kryos_iter_map_i64(
    src: *const i64,
    len: usize,
    kind: i32,
    c: i64,
    dst: *mut i64,
) -> i64 {
    if src.is_null() || dst.is_null() {
        return -1;
    }
    let s = unsafe { std::slice::from_raw_parts(src, len) };
    let d = unsafe { std::slice::from_raw_parts_mut(dst, len) };
    for (i, &v) in s.iter().enumerate() {
        d[i] = match kind {
            0 => v,
            1 => v.wrapping_abs(),
            2 => v.wrapping_neg(),
            3 => v.wrapping_mul(v),
            4 => v.wrapping_add(c),
            5 => v.wrapping_mul(c),
            _ => 0,
        };
    }
    len as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_writes_ascending() {
        let mut out = [0i64; 5];
        let n = kryos_iter_range(10, 2, out.len(), out.as_mut_ptr());
        assert_eq!(n, 5);
        assert_eq!(out, [10, 12, 14, 16, 18]);
    }

    #[test]
    fn filter_keeps_evens() {
        let src = [1i64, 2, 3, 4, 5, 6];
        let mut dst = [0i64; 6];
        let n = kryos_iter_filter_i64(src.as_ptr(), src.len(), 2, 0, dst.as_mut_ptr(), dst.len());
        assert_eq!(n, 3);
        assert_eq!(&dst[..n as usize], &[2, 4, 6]);
    }

    #[test]
    fn map_squares() {
        let src = [1i64, 2, 3, -4];
        let mut dst = [0i64; 4];
        kryos_iter_map_i64(src.as_ptr(), src.len(), 3, 0, dst.as_mut_ptr());
        assert_eq!(dst, [1, 4, 9, 16]);
    }

    #[test]
    fn map_add_constant() {
        let src = [1i64, 2, 3];
        let mut dst = [0i64; 3];
        kryos_iter_map_i64(src.as_ptr(), src.len(), 4, 10, dst.as_mut_ptr());
        assert_eq!(dst, [11, 12, 13]);
    }
}
