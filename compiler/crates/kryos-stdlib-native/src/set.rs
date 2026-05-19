//! Sorted-array set over caller-owned i64 storage.
//!
//! Cheap when N is small (< 1024). For larger sets switch to a real
//! hash set (planned for std::map::HashSet in a future release).
//!
//! All ops keep the buffer sorted ascending; we exploit that for
//! binary-search membership tests.

/// Try to insert `v`. Returns 1 if newly added, 0 if it was already
/// present. Maintains sorted order. Returns -1 if the buffer is full.
#[no_mangle]
pub extern "C" fn kryos_set_insert(
    buf: *mut i64,
    len: *mut i64,
    cap: i64,
    v: i64,
) -> i32 {
    if buf.is_null() || len.is_null() {
        return -1;
    }
    let n = unsafe { *len } as usize;
    let cap = cap as usize;
    if n > cap {
        return -1;
    }
    let slice = unsafe { std::slice::from_raw_parts_mut(buf, cap) };
    // binary search for insertion point
    let (lo, hi) = (0usize, n);
    let pos = lower_bound(&slice[..n], v);
    if pos < n && slice[pos] == v {
        return 0;
    }
    if n >= cap {
        return -1;
    }
    // shift right
    for i in (pos..n).rev() {
        slice[i + 1] = slice[i];
    }
    slice[pos] = v;
    unsafe {
        *len = (n + 1) as i64;
    }
    let _ = (lo, hi);
    1
}

/// Membership check via binary search. Returns 1 if present, 0 if not.
#[no_mangle]
pub extern "C" fn kryos_set_contains(buf: *const i64, len: i64, v: i64) -> i32 {
    if buf.is_null() || len <= 0 {
        return 0;
    }
    let slice = unsafe { std::slice::from_raw_parts(buf, len as usize) };
    match slice.binary_search(&v) {
        Ok(_) => 1,
        Err(_) => 0,
    }
}

/// Remove `v`. Returns 1 if it was present (and removed), 0 if absent.
#[no_mangle]
pub extern "C" fn kryos_set_remove(buf: *mut i64, len: *mut i64, v: i64) -> i32 {
    if buf.is_null() || len.is_null() {
        return 0;
    }
    let n = unsafe { *len } as usize;
    let slice = unsafe { std::slice::from_raw_parts_mut(buf, n) };
    let pos = match slice.binary_search(&v) {
        Ok(p) => p,
        Err(_) => return 0,
    };
    for i in pos..n - 1 {
        slice[i] = slice[i + 1];
    }
    unsafe {
        *len = (n - 1) as i64;
    }
    1
}

fn lower_bound(arr: &[i64], v: i64) -> usize {
    let mut lo = 0;
    let mut hi = arr.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if arr[mid] < v {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_dedup_and_sort() {
        let mut buf = [0i64; 8];
        let mut len = 0i64;
        for v in [5i64, 3, 7, 5, 1] {
            kryos_set_insert(buf.as_mut_ptr(), &mut len, 8, v);
        }
        assert_eq!(len, 4);
        assert_eq!(&buf[..4], &[1, 3, 5, 7]);
    }

    #[test]
    fn contains_via_binary_search() {
        let buf = [1i64, 3, 5, 7, 9];
        assert_eq!(kryos_set_contains(buf.as_ptr(), 5, 5), 1);
        assert_eq!(kryos_set_contains(buf.as_ptr(), 5, 4), 0);
        assert_eq!(kryos_set_contains(buf.as_ptr(), 5, 0), 0);
    }

    #[test]
    fn remove_shrinks() {
        let mut buf = [1i64, 3, 5, 7, 9];
        let mut len = 5i64;
        assert_eq!(kryos_set_remove(buf.as_mut_ptr(), &mut len, 5), 1);
        assert_eq!(len, 4);
        assert_eq!(&buf[..4], &[1, 3, 7, 9]);
        assert_eq!(kryos_set_remove(buf.as_mut_ptr(), &mut len, 5), 0);
    }
}
