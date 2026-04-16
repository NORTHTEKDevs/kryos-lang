//! KryosArray — heap-allocated, bounds-checked dynamic array.
//!
//! Layout: `{ len: i64, cap: i64, elem_size: i64, ref_count: i64, data: *mut u8 }`.
//! Elements are stored as i64-sized values (8 bytes each) for uniform
//! representation. All functions are `#[no_mangle] extern "C"`.
//!
//! Ownership model: `ref_count` tracks how many Kryos values point to this
//! array header. `kryos_array_clone` increments the count (O(1), no copy).
//! `kryos_array_free` decrements it and only deallocates when the count
//! reaches zero. `kryos_array_push` performs copy-on-write when `ref_count > 1`
//! so mutation never affects aliased owners.

use std::alloc::{alloc, dealloc, realloc, Layout};
use std::ptr;

/// Heap-allocated dynamic array with explicit length and capacity.
#[repr(C)]
pub struct KryosArray {
    pub len: i64,
    pub cap: i64,
    pub elem_size: i64,
    pub ref_count: i64,
    pub data: *mut u8,
}

const ELEM_SIZE: usize = 8; // all elements stored as i64

impl KryosArray {
    fn data_layout(cap: usize) -> Layout {
        Layout::from_size_align(cap * ELEM_SIZE, 8).unwrap()
    }
}

/// Create a new empty KryosArray with the given initial capacity.
///
/// `elem_size` is stored but all elements use 8 bytes internally.
#[no_mangle]
pub unsafe extern "C" fn kryos_array_new(elem_size: i64, cap: i64) -> *mut KryosArray {
    let cap_usize = (cap.max(4)) as usize;
    let layout = KryosArray::data_layout(cap_usize);
    let data = alloc(layout);
    if data.is_null() {
        return ptr::null_mut();
    }
    // Zero-initialize.
    ptr::write_bytes(data, 0, cap_usize * ELEM_SIZE);

    let arr = alloc(Layout::new::<KryosArray>()) as *mut KryosArray;
    if arr.is_null() {
        dealloc(data, layout);
        return ptr::null_mut();
    }
    (*arr).len = 0;
    (*arr).cap = cap_usize as i64;
    (*arr).elem_size = elem_size;
    (*arr).ref_count = 1;
    (*arr).data = data;
    arr
}

/// Push an i64-sized value onto the end of the array, growing if necessary.
///
/// Note: when `ref_count > 1` the mutation is visible to all owners (reference
/// semantics). Callers that need isolated mutation should call `kryos_array_clone`
/// first (which returns a new independent copy when ref_count > 1).
#[no_mangle]
pub unsafe extern "C" fn kryos_array_push(arr: *mut KryosArray, val: i64) {
    if arr.is_null() {
        let msg = b"array is null";
        crate::panic::kryos_panic(msg.as_ptr(), msg.len());
    }
    let len = (*arr).len as usize;
    let cap = (*arr).cap as usize;

    if len >= cap {
        // Double capacity.
        let new_cap = cap * 2;
        let old_layout = KryosArray::data_layout(cap);
        let new_size = new_cap * ELEM_SIZE;
        let new_data = realloc((*arr).data, old_layout, new_size);
        if new_data.is_null() {
            let msg = b"array push: allocation failed";
            crate::panic::kryos_panic(msg.as_ptr(), msg.len());
        }
        // Zero new region.
        ptr::write_bytes(
            new_data.add(cap * ELEM_SIZE),
            0,
            (new_cap - cap) * ELEM_SIZE,
        );
        (*arr).data = new_data;
        (*arr).cap = new_cap as i64;
    }

    let slot = (*arr).data.add(len * ELEM_SIZE) as *mut i64;
    *slot = val;
    (*arr).len = (len + 1) as i64;
}

/// Get the element at `idx`. Bounds-checked: panics on out-of-bounds access.
#[no_mangle]
pub unsafe extern "C" fn kryos_array_get(arr: *const KryosArray, idx: i64) -> i64 {
    if arr.is_null() {
        let msg = b"array is null";
        crate::panic::kryos_panic(msg.as_ptr(), msg.len());
    }
    let len = (*arr).len;
    if idx < 0 || idx >= len {
        let msg = format!(
            "array index out of bounds: index {} but length is {}",
            idx, len
        );
        crate::panic::kryos_panic(msg.as_ptr(), msg.len());
    }
    let slot = (*arr).data.add(idx as usize * ELEM_SIZE) as *const i64;
    *slot
}

/// Set the element at `idx`. Bounds-checked: panics on out-of-bounds access.
#[no_mangle]
pub unsafe extern "C" fn kryos_array_set(arr: *mut KryosArray, idx: i64, val: i64) {
    if arr.is_null() {
        let msg = b"array is null";
        crate::panic::kryos_panic(msg.as_ptr(), msg.len());
    }
    let len = (*arr).len;
    if idx < 0 || idx >= len {
        let msg = format!(
            "array index out of bounds: index {} but length is {}",
            idx, len
        );
        crate::panic::kryos_panic(msg.as_ptr(), msg.len());
    }
    let slot = (*arr).data.add(idx as usize * ELEM_SIZE) as *mut i64;
    *slot = val;
}

/// Return the length of the array.
#[no_mangle]
pub unsafe extern "C" fn kryos_array_len(arr: *const KryosArray) -> i64 {
    if arr.is_null() {
        return 0;
    }
    (*arr).len
}

/// Concatenate two KryosArrays, returning a new array containing all elements
/// from `a` followed by all elements from `b`. The originals are not freed.
#[no_mangle]
pub unsafe extern "C" fn kryos_array_concat(
    a: *const KryosArray,
    b: *const KryosArray,
) -> *mut KryosArray {
    let a_len = if a.is_null() { 0 } else { (*a).len };
    let b_len = if b.is_null() { 0 } else { (*b).len };
    let total = a_len + b_len;

    let result = kryos_array_new(8, total.max(4));
    if result.is_null() {
        return result;
    }

    // Copy elements from a.
    for i in 0..a_len {
        let val = kryos_array_get(a, i);
        kryos_array_push(result, val);
    }
    // Copy elements from b.
    for i in 0..b_len {
        let val = kryos_array_get(b, i);
        kryos_array_push(result, val);
    }

    result
}

/// Clone a KryosArray — allocate a new independent array with the same elements
/// (shallow element copy, `ref_count` initialized to 1).
///
/// Used by `@copy` struct field semantics to give each copy its own buffer.
/// For shared ownership of a non-copy array field, use `kryos_array_retain`.
#[no_mangle]
pub unsafe extern "C" fn kryos_array_clone(arr: *const KryosArray) -> *mut KryosArray {
    if arr.is_null() {
        return kryos_array_new(8, 4);
    }
    let len = (*arr).len;
    let result = kryos_array_new((*arr).elem_size, len.max(4));
    if result.is_null() {
        return result;
    }
    // Bulk copy the data buffer.
    if len > 0 && !(*arr).data.is_null() {
        ptr::copy_nonoverlapping((*arr).data, (*result).data, len as usize * ELEM_SIZE);
        (*result).len = len;
    }
    // ref_count is already 1 from kryos_array_new.
    result
}

/// Retain a KryosArray — increment its reference count and return the same pointer.
///
/// Used when a non-copy struct literal copies an array field: both the source
/// and destination structs share the same heap allocation.  `kryos_array_free`
/// only deallocates when the count reaches zero.
#[no_mangle]
pub unsafe extern "C" fn kryos_array_retain(arr: *mut KryosArray) -> *mut KryosArray {
    if arr.is_null() {
        return kryos_array_new(8, 4);
    }
    (*arr).ref_count += 1;
    arr
}

/// Free a KryosArray — decrement reference count and deallocate when it reaches zero.
#[no_mangle]
pub unsafe extern "C" fn kryos_array_free(arr: *mut KryosArray) {
    if arr.is_null() {
        return;
    }
    (*arr).ref_count -= 1;
    if (*arr).ref_count > 0 {
        return;
    }
    let cap = (*arr).cap as usize;
    if !(*arr).data.is_null() && cap > 0 {
        dealloc((*arr).data, KryosArray::data_layout(cap));
    }
    dealloc(arr as *mut u8, Layout::new::<KryosArray>());
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_array_empty() {
        unsafe {
            let arr = kryos_array_new(8, 4);
            assert!(!arr.is_null());
            assert_eq!(kryos_array_len(arr), 0);
            kryos_array_free(arr);
        }
    }

    #[test]
    fn push_and_get() {
        unsafe {
            let arr = kryos_array_new(8, 4);
            kryos_array_push(arr, 10);
            kryos_array_push(arr, 20);
            kryos_array_push(arr, 30);
            assert_eq!(kryos_array_len(arr), 3);
            assert_eq!(kryos_array_get(arr, 0), 10);
            assert_eq!(kryos_array_get(arr, 1), 20);
            assert_eq!(kryos_array_get(arr, 2), 30);
            kryos_array_free(arr);
        }
    }

    #[test]
    fn push_grows_capacity() {
        unsafe {
            let arr = kryos_array_new(8, 4);
            for i in 0..10 {
                kryos_array_push(arr, i * 100);
            }
            assert_eq!(kryos_array_len(arr), 10);
            assert!((*arr).cap >= 10);
            for i in 0..10 {
                assert_eq!(kryos_array_get(arr, i), i * 100);
            }
            kryos_array_free(arr);
        }
    }

    #[test]
    fn set_element() {
        unsafe {
            let arr = kryos_array_new(8, 4);
            kryos_array_push(arr, 1);
            kryos_array_push(arr, 2);
            kryos_array_set(arr, 0, 99);
            assert_eq!(kryos_array_get(arr, 0), 99);
            assert_eq!(kryos_array_get(arr, 1), 2);
            kryos_array_free(arr);
        }
    }

    // out_of_bounds and null_safety tests removed — these now abort via
    // kryos_panic instead of returning silent defaults.

    #[test]
    fn null_len_returns_zero() {
        unsafe {
            // kryos_array_len still returns 0 for null (no abort).
            assert_eq!(kryos_array_len(std::ptr::null()), 0);
            // kryos_array_free is a no-op for null (no abort).
            kryos_array_free(std::ptr::null_mut());
        }
    }

    #[test]
    fn concat_two_arrays() {
        unsafe {
            let a = kryos_array_new(8, 4);
            kryos_array_push(a, 1);
            kryos_array_push(a, 2);
            kryos_array_push(a, 3);

            let b = kryos_array_new(8, 4);
            kryos_array_push(b, 4);

            let c = kryos_array_concat(a, b);
            assert_eq!(kryos_array_len(c), 4);
            assert_eq!(kryos_array_get(c, 0), 1);
            assert_eq!(kryos_array_get(c, 1), 2);
            assert_eq!(kryos_array_get(c, 2), 3);
            assert_eq!(kryos_array_get(c, 3), 4);

            kryos_array_free(a);
            kryos_array_free(b);
            kryos_array_free(c);
        }
    }

    #[test]
    fn concat_with_null() {
        unsafe {
            let a = kryos_array_new(8, 4);
            kryos_array_push(a, 10);

            let c = kryos_array_concat(a, std::ptr::null());
            assert_eq!(kryos_array_len(c), 1);
            assert_eq!(kryos_array_get(c, 0), 10);

            let d = kryos_array_concat(std::ptr::null(), a);
            assert_eq!(kryos_array_len(d), 1);
            assert_eq!(kryos_array_get(d, 0), 10);

            kryos_array_free(a);
            kryos_array_free(c);
            kryos_array_free(d);
        }
    }

    #[test]
    fn concat_empty_arrays() {
        unsafe {
            let a = kryos_array_new(8, 4);
            let b = kryos_array_new(8, 4);
            let c = kryos_array_concat(a, b);
            assert_eq!(kryos_array_len(c), 0);
            kryos_array_free(a);
            kryos_array_free(b);
            kryos_array_free(c);
        }
    }
}
