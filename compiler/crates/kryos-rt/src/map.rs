//! Runtime hash map for Kryos map literals.
//!
//! Simple open-addressing hash map with i64 keys and i64 values.
//! Exposed as `extern "C"` functions for linking from compiled code.

use std::alloc::{alloc_zeroed, dealloc, Layout};

const INITIAL_CAPACITY: usize = 16;
const LOAD_FACTOR: f64 = 0.75;

/// Entry in the hash map: key, value, and occupied flag.
#[repr(C)]
struct MapEntry {
    key: i64,
    value: i64,
    occupied: bool,
}

/// Hash map header stored at the start of the allocation.
#[repr(C)]
struct MapHeader {
    len: i64,
    capacity: i64,
    // Followed by `capacity` MapEntry values.
}

fn hash_key(key: i64, capacity: usize) -> usize {
    // Simple multiplicative hash.
    let h = (key as u64).wrapping_mul(0x9E3779B97F4A7C15);
    (h as usize) % capacity
}

unsafe fn get_entries(ptr: *mut u8) -> *mut MapEntry {
    ptr.add(std::mem::size_of::<MapHeader>()) as *mut MapEntry
}

unsafe fn map_layout(capacity: usize) -> Layout {
    let size = std::mem::size_of::<MapHeader>()
        + capacity * std::mem::size_of::<MapEntry>();
    Layout::from_size_align_unchecked(size, 8)
}

/// Create a new empty map. Returns an opaque pointer as i64.
#[no_mangle]
pub extern "C" fn kryos_map_new() -> i64 {
    unsafe {
        let layout = map_layout(INITIAL_CAPACITY);
        let ptr = alloc_zeroed(layout);
        if ptr.is_null() {
            return 0;
        }
        let header = ptr as *mut MapHeader;
        (*header).len = 0;
        (*header).capacity = INITIAL_CAPACITY as i64;
        ptr as i64
    }
}

/// Insert a key-value pair into the map.
#[no_mangle]
pub extern "C" fn kryos_map_insert(map: i64, key: i64, value: i64) {
    if map == 0 {
        return;
    }
    unsafe {
        let ptr = map as *mut u8;
        let header = ptr as *mut MapHeader;
        let capacity = (*header).capacity as usize;
        let entries = get_entries(ptr);

        // Check load factor — resize if needed.
        if ((*header).len + 1) as f64 > capacity as f64 * LOAD_FACTOR {
            // TODO: resize. For now, just proceed.
        }

        let mut idx = hash_key(key, capacity);
        for _ in 0..capacity {
            let entry = &mut *entries.add(idx);
            if !entry.occupied || entry.key == key {
                entry.key = key;
                entry.value = value;
                if !entry.occupied {
                    entry.occupied = true;
                    (*header).len += 1;
                }
                return;
            }
            idx = (idx + 1) % capacity;
        }
    }
}

/// Get a value from the map. Returns the value, or 0 if not found.
#[no_mangle]
pub extern "C" fn kryos_map_get(map: i64, key: i64) -> i64 {
    if map == 0 {
        return 0;
    }
    unsafe {
        let ptr = map as *mut u8;
        let header = ptr as *const MapHeader;
        let capacity = (*header).capacity as usize;
        let entries = get_entries(ptr);

        let mut idx = hash_key(key, capacity);
        for _ in 0..capacity {
            let entry = &*entries.add(idx);
            if !entry.occupied {
                return 0;
            }
            if entry.key == key {
                return entry.value;
            }
            idx = (idx + 1) % capacity;
        }
        0
    }
}

/// Get the number of entries in the map.
#[no_mangle]
pub extern "C" fn kryos_map_len(map: i64) -> i64 {
    if map == 0 {
        return 0;
    }
    unsafe {
        let header = map as *const MapHeader;
        (*header).len
    }
}

/// Free the map.
#[no_mangle]
pub extern "C" fn kryos_map_free(map: i64) {
    if map == 0 {
        return;
    }
    unsafe {
        let ptr = map as *mut u8;
        let header = ptr as *const MapHeader;
        let capacity = (*header).capacity as usize;
        let layout = map_layout(capacity);
        dealloc(ptr, layout);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_insert() {
        let map = kryos_map_new();
        assert_ne!(map, 0);
        kryos_map_insert(map, 1, 100);
        kryos_map_insert(map, 2, 200);
        assert_eq!(kryos_map_len(map), 2);
        assert_eq!(kryos_map_get(map, 1), 100);
        assert_eq!(kryos_map_get(map, 2), 200);
        assert_eq!(kryos_map_get(map, 3), 0); // not found
        kryos_map_free(map);
    }

    #[test]
    fn overwrite_key() {
        let map = kryos_map_new();
        kryos_map_insert(map, 1, 100);
        kryos_map_insert(map, 1, 999);
        assert_eq!(kryos_map_len(map), 1);
        assert_eq!(kryos_map_get(map, 1), 999);
        kryos_map_free(map);
    }

    #[test]
    fn empty_map() {
        let map = kryos_map_new();
        assert_eq!(kryos_map_len(map), 0);
        assert_eq!(kryos_map_get(map, 42), 0);
        kryos_map_free(map);
    }

    #[test]
    fn null_safety() {
        kryos_map_insert(0, 1, 1);
        assert_eq!(kryos_map_get(0, 1), 0);
        assert_eq!(kryos_map_len(0), 0);
        kryos_map_free(0);
    }
}
