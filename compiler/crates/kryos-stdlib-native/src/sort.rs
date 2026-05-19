//! In-place sort helpers for arrays of i64.
//!
//! Uses Rust's std slice sort (Timsort). Sorts the i64 array referenced
//! by `ptr` of length `len` in ascending order. For descending, sort then
//! reverse via `kryos_sort_i64_reverse`.
//!
//! Strings + floats get their own variants because the cmp predicates
//! differ.

/// Sort an i64 slice in-place, ascending.
#[no_mangle]
pub extern "C" fn kryos_sort_i64(ptr: *mut i64, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    let slice = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
    slice.sort_unstable();
}

/// Reverse an i64 slice in-place. Combine with `kryos_sort_i64` for
/// descending sort.
#[no_mangle]
pub extern "C" fn kryos_sort_i64_reverse(ptr: *mut i64, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    let slice = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
    slice.reverse();
}

/// Sort an f64 slice in-place, ascending. NaN values sort last.
#[no_mangle]
pub extern "C" fn kryos_sort_f64(ptr: *mut f64, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    let slice = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
    slice.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Greater));
}

/// Binary-search a sorted i64 slice. Returns the index if found, else -1.
#[no_mangle]
pub extern "C" fn kryos_bsearch_i64(ptr: *const i64, len: usize, needle: i64) -> i64 {
    if ptr.is_null() || len == 0 {
        return -1;
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    match slice.binary_search(&needle) {
        Ok(i) => i as i64,
        Err(_) => -1,
    }
}

/// Check if an i64 slice is sorted ascending. Returns 1 if yes, 0 if no.
#[no_mangle]
pub extern "C" fn kryos_is_sorted_i64(ptr: *const i64, len: usize) -> i32 {
    if ptr.is_null() || len < 2 {
        return 1;
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    for window in slice.windows(2) {
        if window[0] > window[1] {
            return 0;
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_i64_ascending() {
        let mut v = [5i64, 3, 8, 1, 4];
        kryos_sort_i64(v.as_mut_ptr(), v.len());
        assert_eq!(v, [1, 3, 4, 5, 8]);
    }

    #[test]
    fn bsearch_finds_present_value() {
        let v = [1i64, 3, 4, 5, 8];
        assert_eq!(kryos_bsearch_i64(v.as_ptr(), v.len(), 4), 2);
        assert_eq!(kryos_bsearch_i64(v.as_ptr(), v.len(), 99), -1);
    }

    #[test]
    fn sort_then_reverse_gives_descending() {
        let mut v = [2i64, 9, 1, 7];
        kryos_sort_i64(v.as_mut_ptr(), v.len());
        kryos_sort_i64_reverse(v.as_mut_ptr(), v.len());
        assert_eq!(v, [9, 7, 2, 1]);
    }

    #[test]
    fn is_sorted_detects_misorder() {
        let ok = [1i64, 2, 3, 4];
        let bad = [1i64, 3, 2, 4];
        assert_eq!(kryos_is_sorted_i64(ok.as_ptr(), ok.len()), 1);
        assert_eq!(kryos_is_sorted_i64(bad.as_ptr(), bad.len()), 0);
    }
}
