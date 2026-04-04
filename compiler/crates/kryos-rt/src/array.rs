//! KryosArray — heap-allocated, bounds-checked dynamic array.
//!
//! Layout: `{ len: i64, cap: i64, elem_size: i64, data: *mut u8 }`.
//! Elements are stored as i64-sized values (8 bytes each) for uniform
//! representation. All functions are `#[no_mangle] extern "C"`.

use std::alloc::{alloc, dealloc, realloc, Layout};
use std::ptr;

/// Heap-allocated dynamic array with explicit length and capacity.
#[repr(C)]
pub struct KryosArray {
    pub len: i64,
    pub cap: i64,
    pub elem_size: i64,
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
    (*arr).data = data;
    arr
}

/// Push an i64-sized value onto the end of the array, growing if necessary.
#[no_mangle]
pub unsafe extern "C" fn kryos_array_push(arr: *mut KryosArray, val: i64) {
    if arr.is_null() {
        return;
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
            // Allocation failed — silently drop the push.
            return;
        }
        // Zero new region.
        ptr::write_bytes(new_data.add(cap * ELEM_SIZE), 0, (new_cap - cap) * ELEM_SIZE);
        (*arr).data = new_data;
        (*arr).cap = new_cap as i64;
    }

    let slot = (*arr).data.add(len * ELEM_SIZE) as *mut i64;
    *slot = val;
    (*arr).len = (len + 1) as i64;
}

/// Get the element at `idx`. Bounds-checked: returns 0 and prints an error
/// message on out-of-bounds access (trapping would require platform-specific
/// code; for now we return a safe default).
#[no_mangle]
pub unsafe extern "C" fn kryos_array_get(arr: *const KryosArray, idx: i64) -> i64 {
    if arr.is_null() {
        return 0;
    }
    let len = (*arr).len;
    if idx < 0 || idx >= len {
        eprintln!(
            "kryos: array index out of bounds: index {} but length is {}",
            idx, len
        );
        return 0;
    }
    let slot = (*arr).data.add(idx as usize * ELEM_SIZE) as *const i64;
    *slot
}

/// Set the element at `idx`. Bounds-checked.
#[no_mangle]
pub unsafe extern "C" fn kryos_array_set(arr: *mut KryosArray, idx: i64, val: i64) {
    if arr.is_null() {
        return;
    }
    let len = (*arr).len;
    if idx < 0 || idx >= len {
        eprintln!(
            "kryos: array index out of bounds: index {} but length is {}",
            idx, len
        );
        return;
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

/// Free a KryosArray and its data buffer.
#[no_mangle]
pub unsafe extern "C" fn kryos_array_free(arr: *mut KryosArray) {
    if arr.is_null() {
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

    #[test]
    fn out_of_bounds_returns_zero() {
        unsafe {
            let arr = kryos_array_new(8, 4);
            kryos_array_push(arr, 42);
            // Index 5 is out of bounds.
            assert_eq!(kryos_array_get(arr, 5), 0);
            // Negative index.
            assert_eq!(kryos_array_get(arr, -1), 0);
            kryos_array_free(arr);
        }
    }

    #[test]
    fn null_safety() {
        unsafe {
            assert_eq!(kryos_array_len(std::ptr::null()), 0);
            assert_eq!(kryos_array_get(std::ptr::null(), 0), 0);
            kryos_array_push(std::ptr::null_mut(), 1); // should not crash
            kryos_array_free(std::ptr::null_mut()); // should not crash
        }
    }
}
