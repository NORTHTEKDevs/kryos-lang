//! Small dense matrices over i64 (row-major). Caller owns the storage;
//! we just provide the arithmetic.

/// dst[r,c] = a[r,c] + b[r,c]. Returns 1 on success.
#[no_mangle]
pub extern "C" fn kryos_mat_add(
    a: *const i64,
    b: *const i64,
    dst: *mut i64,
    rows: usize,
    cols: usize,
) -> i32 {
    if a.is_null() || b.is_null() || dst.is_null() {
        return 0;
    }
    let n = rows * cols;
    let sa = unsafe { std::slice::from_raw_parts(a, n) };
    let sb = unsafe { std::slice::from_raw_parts(b, n) };
    let sd = unsafe { std::slice::from_raw_parts_mut(dst, n) };
    for i in 0..n {
        sd[i] = sa[i].saturating_add(sb[i]);
    }
    1
}

/// dst = a × b (row-major, sizes (m, k) × (k, n) → (m, n)).
#[no_mangle]
pub extern "C" fn kryos_mat_mul(
    a: *const i64,
    b: *const i64,
    dst: *mut i64,
    m: usize,
    k: usize,
    n: usize,
) -> i32 {
    if a.is_null() || b.is_null() || dst.is_null() {
        return 0;
    }
    let sa = unsafe { std::slice::from_raw_parts(a, m * k) };
    let sb = unsafe { std::slice::from_raw_parts(b, k * n) };
    let sd = unsafe { std::slice::from_raw_parts_mut(dst, m * n) };
    for i in 0..m {
        for j in 0..n {
            let mut acc = 0i64;
            for p in 0..k {
                acc = acc.saturating_add(sa[i * k + p].saturating_mul(sb[p * n + j]));
            }
            sd[i * n + j] = acc;
        }
    }
    1
}

/// dst = transpose of `a` (rows × cols → cols × rows).
#[no_mangle]
pub extern "C" fn kryos_mat_transpose(
    a: *const i64,
    dst: *mut i64,
    rows: usize,
    cols: usize,
) -> i32 {
    if a.is_null() || dst.is_null() {
        return 0;
    }
    let sa = unsafe { std::slice::from_raw_parts(a, rows * cols) };
    let sd = unsafe { std::slice::from_raw_parts_mut(dst, rows * cols) };
    for r in 0..rows {
        for c in 0..cols {
            sd[c * rows + r] = sa[r * cols + c];
        }
    }
    1
}

/// Scalar multiplication: dst[i] = a[i] * scalar.
#[no_mangle]
pub extern "C" fn kryos_mat_scale(
    a: *const i64,
    scalar: i64,
    dst: *mut i64,
    n: usize,
) {
    if a.is_null() || dst.is_null() {
        return;
    }
    let sa = unsafe { std::slice::from_raw_parts(a, n) };
    let sd = unsafe { std::slice::from_raw_parts_mut(dst, n) };
    for i in 0..n {
        sd[i] = sa[i].saturating_mul(scalar);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_2x2() {
        let a = [1i64, 2, 3, 4];
        let b = [10i64, 20, 30, 40];
        let mut dst = [0i64; 4];
        kryos_mat_add(a.as_ptr(), b.as_ptr(), dst.as_mut_ptr(), 2, 2);
        assert_eq!(dst, [11, 22, 33, 44]);
    }

    #[test]
    fn mul_2x3_3x2() {
        // a = [[1,2,3],[4,5,6]]    (2×3)
        // b = [[7,8],[9,10],[11,12]] (3×2)
        // a*b = [[58,64],[139,154]] (2×2)
        let a = [1i64, 2, 3, 4, 5, 6];
        let b = [7i64, 8, 9, 10, 11, 12];
        let mut dst = [0i64; 4];
        kryos_mat_mul(a.as_ptr(), b.as_ptr(), dst.as_mut_ptr(), 2, 3, 2);
        assert_eq!(dst, [58, 64, 139, 154]);
    }

    #[test]
    fn transpose_3x2() {
        // a = [[1,2],[3,4],[5,6]]   (3×2)
        // a^T = [[1,3,5],[2,4,6]]   (2×3)
        let a = [1i64, 2, 3, 4, 5, 6];
        let mut dst = [0i64; 6];
        kryos_mat_transpose(a.as_ptr(), dst.as_mut_ptr(), 3, 2);
        assert_eq!(dst, [1, 3, 5, 2, 4, 6]);
    }

    #[test]
    fn scale_inplace_compatible() {
        let a = [2i64, 4, 6];
        let mut dst = [0i64; 3];
        kryos_mat_scale(a.as_ptr(), 10, dst.as_mut_ptr(), 3);
        assert_eq!(dst, [20, 40, 60]);
    }
}
