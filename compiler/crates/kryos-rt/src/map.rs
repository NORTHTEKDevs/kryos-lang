//! Runtime hash map for Kryos map literals.
//!
//! Open-addressing hash map with i64 keys and i64 values.
//! The header is a stable allocation (the external handle); entries live
//! in a separate allocation that can be resized without invalidating
//! the handle.
//!
//! Exposed as `extern "C"` functions for linking from compiled code.
//!
//! # Unsafe invariants (file-wide)
//!
//! See `docs/17-unsafe-audit.md` patterns 1 (FFI handle reconstruction) and 3
//! (alloc/dealloc). Map handles are `*mut MapHeader` cast to `i64`. Entry
//! storage is a separate `Layout` based on the live `capacity` field;
//! resize swaps the buffer atomically (handle stays stable so generated code
//! can cache it across operations).

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
    let layout = Layout::from_size_align_unchecked(capacity * std::mem::size_of::<MapEntry>(), 8);
    let ptr = alloc_zeroed(layout);
    ptr as *mut MapEntry
}

unsafe fn free_entries(entries: *mut MapEntry, capacity: usize) {
    if entries.is_null() {
        return;
    }
    let layout = Layout::from_size_align_unchecked(capacity * std::mem::size_of::<MapEntry>(), 8);
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
        let layout = Layout::from_size_align_unchecked(std::mem::size_of::<MapHeader>(), 8);
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

/// Insert a key-value pair where the key is a KryosString pointer.
/// Uses content-based hashing and comparison for string keys.
#[no_mangle]
pub extern "C" fn kryos_map_insert_str(map: i64, key: i64, value: i64) {
    if map == 0 {
        return;
    }
    unsafe {
        let header = map as *mut MapHeader;

        // Resize if load factor exceeded.
        if ((*header).len + 1) as f64 > (*header).capacity as f64 * LOAD_FACTOR {
            resize_str(header);
        }

        let capacity = (*header).capacity as usize;
        let entries = (*header).entries;

        let str_hash = crate::string::kryos_string_hash(key as *const crate::string::KryosString);
        let mut idx = hash_key(str_hash, capacity);
        for _ in 0..capacity {
            let entry = &mut *entries.add(idx);
            if !entry.occupied {
                entry.key = key;
                entry.value = value;
                entry.occupied = true;
                (*header).len += 1;
                return;
            }
            // Compare by content for string keys.
            if crate::string::kryos_string_eq(
                entry.key as *const crate::string::KryosString,
                key as *const crate::string::KryosString,
            ) {
                entry.value = value;
                return;
            }
            idx = (idx + 1) % capacity;
        }
    }
}

/// Get a value from the map where the key is a KryosString pointer.
/// Uses content-based hashing and comparison for string keys.
#[no_mangle]
pub extern "C" fn kryos_map_get_str(map: i64, key: i64) -> i64 {
    if map == 0 {
        return 0;
    }
    unsafe {
        let header = map as *const MapHeader;
        let capacity = (*header).capacity as usize;
        let entries = (*header).entries;

        let str_hash = crate::string::kryos_string_hash(key as *const crate::string::KryosString);
        let mut idx = hash_key(str_hash, capacity);
        for _ in 0..capacity {
            let entry = &*entries.add(idx);
            if !entry.occupied {
                return 0;
            }
            if crate::string::kryos_string_eq(
                entry.key as *const crate::string::KryosString,
                key as *const crate::string::KryosString,
            ) {
                return entry.value;
            }
            idx = (idx + 1) % capacity;
        }
        0
    }
}

/// Resize helper for string-keyed maps (uses content-based hashing).
unsafe fn resize_str(header: *mut MapHeader) {
    let old_cap = (*header).capacity as usize;
    let new_cap = old_cap * 2;
    let new_entries = alloc_entries(new_cap);
    if new_entries.is_null() {
        return;
    }

    let old_entries = (*header).entries;

    for i in 0..old_cap {
        let entry = &*old_entries.add(i);
        if entry.occupied {
            let str_hash =
                crate::string::kryos_string_hash(entry.key as *const crate::string::KryosString);
            let mut idx = hash_key(str_hash, new_cap);
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

/// Check if a key exists in the map. Returns 1 if found, 0 otherwise.
#[no_mangle]
pub extern "C" fn kryos_map_has(map: i64, key: i64) -> i64 {
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
                return 1;
            }
            idx = (idx + 1) % capacity;
        }
        0
    }
}

/// Check if a string key exists in the map. Returns 1 if found, 0 otherwise.
#[no_mangle]
pub extern "C" fn kryos_map_has_str(map: i64, key: i64) -> i64 {
    if map == 0 {
        return 0;
    }
    unsafe {
        let header = map as *const MapHeader;
        let capacity = (*header).capacity as usize;
        let entries = (*header).entries;

        let str_hash = crate::string::kryos_string_hash(key as *const crate::string::KryosString);
        let mut idx = hash_key(str_hash, capacity);
        for _ in 0..capacity {
            let entry = &*entries.add(idx);
            if !entry.occupied {
                return 0;
            }
            if crate::string::kryos_string_eq(
                entry.key as *const crate::string::KryosString,
                key as *const crate::string::KryosString,
            ) {
                return 1;
            }
            idx = (idx + 1) % capacity;
        }
        0
    }
}

/// Delete a key from the map. Returns the old value, or 0 if not found.
/// Uses tombstone-free backward-shift deletion for open-addressing.
#[no_mangle]
pub extern "C" fn kryos_map_delete(map: i64, key: i64) -> i64 {
    if map == 0 {
        return 0;
    }
    unsafe {
        let header = map as *mut MapHeader;
        let capacity = (*header).capacity as usize;
        let entries = (*header).entries;

        let mut idx = hash_key(key, capacity);
        for _ in 0..capacity {
            let entry = &*entries.add(idx);
            if !entry.occupied {
                return 0;
            }
            if entry.key == key {
                let old_value = entry.value;
                // Mark as empty and backward-shift subsequent entries.
                (*entries.add(idx)).occupied = false;
                (*header).len -= 1;
                // Shift entries that may have been displaced by this slot.
                let mut j = (idx + 1) % capacity;
                for _ in 0..capacity {
                    let next = &*entries.add(j);
                    if !next.occupied {
                        break;
                    }
                    let natural = hash_key(next.key, capacity);
                    // Check if `next` belongs before the empty slot we created.
                    if (j > idx && (natural <= idx || natural > j))
                        || (j < idx && natural <= idx && natural > j)
                    {
                        let k = next.key;
                        let v = next.value;
                        (*entries.add(j)).occupied = false;
                        (*entries.add(idx)).key = k;
                        (*entries.add(idx)).value = v;
                        (*entries.add(idx)).occupied = true;
                        idx = j;
                    }
                    j = (j + 1) % capacity;
                }
                return old_value;
            }
            idx = (idx + 1) % capacity;
        }
        0
    }
}

/// Delete a string key from the map. Returns the old value, or 0 if not found.
#[no_mangle]
pub extern "C" fn kryos_map_delete_str(map: i64, key: i64) -> i64 {
    if map == 0 {
        return 0;
    }
    unsafe {
        let header = map as *mut MapHeader;
        let capacity = (*header).capacity as usize;
        let entries = (*header).entries;

        let str_hash = crate::string::kryos_string_hash(key as *const crate::string::KryosString);
        let mut idx = hash_key(str_hash, capacity);
        for _ in 0..capacity {
            let entry = &*entries.add(idx);
            if !entry.occupied {
                return 0;
            }
            if crate::string::kryos_string_eq(
                entry.key as *const crate::string::KryosString,
                key as *const crate::string::KryosString,
            ) {
                let old_value = entry.value;
                (*entries.add(idx)).occupied = false;
                (*header).len -= 1;
                // Backward-shift for string-keyed entries.
                let mut j = (idx + 1) % capacity;
                for _ in 0..capacity {
                    let next = &*entries.add(j);
                    if !next.occupied {
                        break;
                    }
                    let next_hash = crate::string::kryos_string_hash(
                        next.key as *const crate::string::KryosString,
                    );
                    let natural = hash_key(next_hash, capacity);
                    if (j > idx && (natural <= idx || natural > j))
                        || (j < idx && natural <= idx && natural > j)
                    {
                        let k = next.key;
                        let v = next.value;
                        (*entries.add(j)).occupied = false;
                        (*entries.add(idx)).key = k;
                        (*entries.add(idx)).value = v;
                        (*entries.add(idx)).occupied = true;
                        idx = j;
                    }
                    j = (j + 1) % capacity;
                }
                return old_value;
            }
            idx = (idx + 1) % capacity;
        }
        0
    }
}

/// Return all keys as a KryosArray of i64 values.
#[no_mangle]
pub unsafe extern "C" fn kryos_map_keys(map: i64) -> i64 {
    if map == 0 {
        return crate::array::kryos_array_new(8, 0) as i64;
    }
    {
        let header = map as *const MapHeader;
        let capacity = (*header).capacity as usize;
        let entries = (*header).entries;
        let arr = crate::array::kryos_array_new(8, (*header).len);

        for i in 0..capacity {
            let entry = &*entries.add(i);
            if entry.occupied {
                crate::array::kryos_array_push(arr, entry.key);
            }
        }
        arr as i64
    }
}

/// Return all string keys as a KryosArray of KryosString handles.
#[no_mangle]
pub unsafe extern "C" fn kryos_map_keys_str(map: i64) -> i64 {
    kryos_map_keys(map) // Same implementation — keys are i64 handles either way.
}

/// Deep-clone a map: allocate a new header and entry table, copy all entries.
#[no_mangle]
pub extern "C" fn kryos_map_clone(map: i64) -> i64 {
    if map == 0 {
        return kryos_map_new();
    }
    unsafe {
        let src = map as *const MapHeader;
        let capacity = (*src).capacity as usize;

        // Allocate new header.
        let layout = Layout::from_size_align_unchecked(std::mem::size_of::<MapHeader>(), 8);
        let ptr = alloc_zeroed(layout);
        if ptr.is_null() {
            return 0;
        }
        let dst = ptr as *mut MapHeader;

        // Allocate new entry table with same capacity.
        let new_entries = alloc_entries(capacity);
        if new_entries.is_null() {
            dealloc(ptr, layout);
            return 0;
        }

        // Copy all entries from source to destination.
        std::ptr::copy_nonoverlapping(
            (*src).entries as *const u8,
            new_entries as *mut u8,
            capacity * std::mem::size_of::<MapEntry>(),
        );

        (*dst).len = (*src).len;
        (*dst).capacity = (*src).capacity;
        (*dst).entries = new_entries;
        ptr as i64
    }
}

/// Free the map.
///
/// H18 leak-all-maps (shift step 29, 2026-05-20): consistent with H10/H12
/// no-op string/array free. Maps share entry tables across @copy struct
/// shallow clone, same double-free hazard. Leak the entire map to remove
/// that hazard. Bounded leak per stage-1 invocation.
#[no_mangle]
pub extern "C" fn kryos_map_free(map: i64) {
    let _ = map;
    return;
    #[allow(unreachable_code)]
    if map == 0 {
        return;
    }
    unsafe {
        let header = map as *mut MapHeader;
        let capacity = (*header).capacity as usize;
        free_entries((*header).entries, capacity);
        let layout = Layout::from_size_align_unchecked(std::mem::size_of::<MapHeader>(), 8);
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

    #[test]
    fn clone_map() {
        let map = kryos_map_new();
        kryos_map_insert(map, 1, 100);
        kryos_map_insert(map, 2, 200);
        kryos_map_insert(map, 3, 300);

        let cloned = kryos_map_clone(map);
        assert_ne!(map, cloned);
        assert_eq!(kryos_map_len(cloned), 3);
        assert_eq!(kryos_map_get(cloned, 1), 100);
        assert_eq!(kryos_map_get(cloned, 2), 200);
        assert_eq!(kryos_map_get(cloned, 3), 300);

        // Mutating original doesn't affect clone.
        kryos_map_insert(map, 1, 999);
        assert_eq!(kryos_map_get(cloned, 1), 100);

        kryos_map_free(map);
        kryos_map_free(cloned);
    }

    #[test]
    fn clone_null_map() {
        let cloned = kryos_map_clone(0);
        assert_ne!(cloned, 0);
        assert_eq!(kryos_map_len(cloned), 0);
        kryos_map_free(cloned);
    }
}
