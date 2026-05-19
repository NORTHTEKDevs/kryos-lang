//! Slice operations not already covered: zip, partition, group_by_key.

/// Zip two i64 slices into pairs encoded as `a * 2^32 + b` in `dst`.
/// Returns the number of pairs written (min of input lengths).
#[no_mangle]
pub extern "C" fn kryos_slice_zip_pack(
    a: *const i64,
    a_len: usize,
    b: *const i64,
    b_len: usize,
    dst: *mut i64,
    dst_cap: usize,
) -> i64 {
    if a.is_null() || b.is_null() || dst.is_null() {
        return -1;
    }
    let sa = unsafe { std::slice::from_raw_parts(a, a_len) };
    let sb = unsafe { std::slice::from_raw_parts(b, b_len) };
    let n = sa.len().min(sb.len()).min(dst_cap);
    let d = unsafe { std::slice::from_raw_parts_mut(dst, n) };
    for i in 0..n {
        let lo = sb[i] as u32 as i64;
        d[i] = sa[i].wrapping_shl(32) | lo;
    }
    n as i64
}

/// Partition: copy elements satisfying a predicate to `out_yes`, rest to
/// `out_no`. Predicate codes:
///   0 = positive (>0)
///   1 = negative (<0)
///   2 = >= threshold
///   3 = < threshold
/// Returns count of "yes" entries; "no" count = src_len - yes.
#[no_mangle]
pub extern "C" fn kryos_slice_partition_i64(
    src: *const i64,
    src_len: usize,
    pred_kind: i32,
    threshold: i64,
    out_yes: *mut i64,
    out_yes_cap: usize,
    out_no: *mut i64,
    out_no_cap: usize,
) -> i64 {
    if src.is_null() || out_yes.is_null() || out_no.is_null() {
        return -1;
    }
    let s = unsafe { std::slice::from_raw_parts(src, src_len) };
    let y = unsafe { std::slice::from_raw_parts_mut(out_yes, out_yes_cap) };
    let n = unsafe { std::slice::from_raw_parts_mut(out_no, out_no_cap) };
    let mut yi = 0usize;
    let mut ni = 0usize;
    for &v in s {
        let take_yes = match pred_kind {
            0 => v > 0,
            1 => v < 0,
            2 => v >= threshold,
            3 => v < threshold,
            _ => false,
        };
        if take_yes {
            if yi >= out_yes_cap {
                return -1;
            }
            y[yi] = v;
            yi += 1;
        } else {
            if ni >= out_no_cap {
                return -1;
            }
            n[ni] = v;
            ni += 1;
        }
    }
    yi as i64
}

/// Take the first N elements of `src` into `dst`. Returns count taken
/// (min of N, src_len, dst_cap).
#[no_mangle]
pub extern "C" fn kryos_slice_take(
    src: *const i64,
    src_len: usize,
    n: i64,
    dst: *mut i64,
    dst_cap: usize,
) -> i64 {
    if src.is_null() || dst.is_null() || n <= 0 {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts(src, src_len) };
    let n = (n as usize).min(s.len()).min(dst_cap);
    let d = unsafe { std::slice::from_raw_parts_mut(dst, n) };
    d.copy_from_slice(&s[..n]);
    n as i64
}

/// Drop the first N elements; copy the rest to `dst`. Returns count copied.
#[no_mangle]
pub extern "C" fn kryos_slice_drop(
    src: *const i64,
    src_len: usize,
    n: i64,
    dst: *mut i64,
    dst_cap: usize,
) -> i64 {
    if src.is_null() || dst.is_null() {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts(src, src_len) };
    let n = (n.max(0) as usize).min(s.len());
    let rest = &s[n..];
    let take = rest.len().min(dst_cap);
    let d = unsafe { std::slice::from_raw_parts_mut(dst, take) };
    d.copy_from_slice(&rest[..take]);
    take as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_first_n() {
        let src = [1i64, 2, 3, 4, 5];
        let mut dst = [0i64; 5];
        let n = kryos_slice_take(src.as_ptr(), src.len(), 3, dst.as_mut_ptr(), dst.len());
        assert_eq!(n, 3);
        assert_eq!(&dst[..3], &[1, 2, 3]);
    }

    #[test]
    fn drop_skips_n() {
        let src = [1i64, 2, 3, 4, 5];
        let mut dst = [0i64; 5];
        let n = kryos_slice_drop(src.as_ptr(), src.len(), 2, dst.as_mut_ptr(), dst.len());
        assert_eq!(n, 3);
        assert_eq!(&dst[..3], &[3, 4, 5]);
    }

    #[test]
    fn partition_positives_vs_rest() {
        let src = [1i64, -2, 3, -4, 5];
        let mut y = [0i64; 5];
        let mut n = [0i64; 5];
        let yes = kryos_slice_partition_i64(
            src.as_ptr(),
            src.len(),
            0, // positive
            0,
            y.as_mut_ptr(),
            5,
            n.as_mut_ptr(),
            5,
        );
        assert_eq!(yes, 3);
        assert_eq!(&y[..3], &[1, 3, 5]);
        assert_eq!(&n[..2], &[-2, -4]);
    }
}
