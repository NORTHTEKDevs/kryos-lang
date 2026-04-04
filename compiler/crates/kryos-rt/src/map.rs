//! Runtime hash map for Kryos map literals.
//!
//! Open-addressing hash map with i64 keys and i64 values.
//! The header is a stable allocation (the external handle); entries live
//! in a separate allocation that can be resized without invalidating
//! the handle.
//!
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

/// Hash map header — stable allocation that serves as the external handle.
/// Entries are stored in a separate allocation pointed to by `entries`.
#[repr(C)]
struct MapHeader {
    len: i64,
    capacity: i64,
    entries: *mut MapEntry,
}

fn hash_key(key: i64, capacity: usize) -> usize {
    let h = (key as u64).wrapping_mul(0x9E3779B97F4A7C15);
    (h as usize) % capacity
}

unsafe fn alloc_entries(capacity: usize) -> *mut MapEntry {
    let layout = Layout::from_size_align_unchecked(
        capacity * std::mem::size_of::<MapEntry>(),
        8,
    );
    let ptr = alloc_zeroed(layout);
    ptr as *mut MapEntry
}

unsafe fn free_entries(entries: *mut MapEntry, capacity: usize) {
    if entries.is_null() {
        return;
    }
    let layout = Layout::from_size_align_unchecked(
        capacity * std::mem::size_of::<MapEntry>(),
        8,
    );
    dealloc(entries as *mut u8, layout);
}

/// Resize the map to double its current capacity.
unsafe fn resize(header: *mut MapHeader) {
    let old_cap = (*header).capacity as usize;
    let new_cap = old_cap * 2;
    let new_entries = alloc_entries(new_cap);
    if new_entries.is_null() {
        return; // OOM — leave existing map intact
    }

    let old_entries = (*header).entries;

    // Rehash all occupied entries into the new table.
    for i in 0..old_cap {
        let entry = &*old_entries.add(i);
        if entry.occupied {
            let mut idx = hash_key(entry.key, new_cap);
            loop {
                let slot = &mut *new_entries.add(idx);
                if !slot.occupied {
                    slot.key = entry.key;
                    slot.value = entry.value;
                    slot.occupied = true;
                    break;
                }
                idx = (idx + 1) % new_cap;
            }
        }
    }

    free_entries(old_entries, old_cap);
    (*header).entries = new_entries;
    (*header).capacity = new_cap as i64;
}

/// Create a new empty map. Returns an opaque pointer as i64.
#[no_mangle]
pub extern "C" fn kryos_map_new() -> i64 {
    unsafe {
        let layout = Layout::from_size_align_unchecked(
            std::mem::size_of::<MapHeader>(),
            8,
        );
        let ptr = alloc_zeroed(layout);
        if ptr.is_null() {
            return 0;
        }
        let header = ptr as *mut MapHeader;
        let entries = alloc_entries(INITIAL_CAPACITY);
        if entries.is_null() {
            dealloc(ptr, layout);
            return 0;
        }
        (*header).len = 0;
        (*header).capacity = INITIAL_CAPACITY as i64;
        (*header).entries = entries;
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
        let header = map as *mut MapHeader;

        // Resize if load factor exceeded.
        if ((*header).len + 1) as f64 > (*header).capacity as f64 * LOAD_FACTOR {
            resize(header);
        }

        let capacity = (*header).capacity as usize;
        let entries = (*header).entries;

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
        let header = map as *const MapHeader;
        let capacity = (*header).capacity as usize;
        let entries = (*header).entries;

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
        let header = map as *mut MapHeader;
        let capacity = (*header).capacity as usize;
        free_entries((*header).entries, capacity);
        let layout = Layout::from_size_align_unchecked(
            std::mem::size_of::<MapHeader>(),
            8,
        );
        dealloc(map as *mut u8, layout);
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

    #[test]
    fn resize_under_load() {
        let map = kryos_map_new();
        // Insert enough entries to trigger multiple resizes.
        // Initial capacity is 16, load factor 0.75 → resize at 13.
        for i in 0..100 {
            kryos_map_insert(map, i, i * 10);
        }
        assert_eq!(kryos_map_len(map), 100);
        // Verify all entries survived the resizes.
        for i in 0..100 {
            assert_eq!(kryos_map_get(map, i), i * 10);
        }
        kryos_map_free(map);
    }

    #[test]
    fn resize_preserves_overwrites() {
        let map = kryos_map_new();
        for i in 0..50 {
            kryos_map_insert(map, i, i);
        }
        // Overwrite all values.
        for i in 0..50 {
            kryos_map_insert(map, i, i + 1000);
        }
        assert_eq!(kryos_map_len(map), 50);
        for i in 0..50 {
            assert_eq!(kryos_map_get(map, i), i + 1000);
        }
        kryos_map_free(map);
    }
}
