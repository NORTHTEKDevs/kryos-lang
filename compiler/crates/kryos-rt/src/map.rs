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
///
/// Step 37 production hardening: `ref_count` placed AFTER `entries` so the
/// header stays at the same offset for any consumer that reads `len` /
/// `capacity` / `entries` at fixed positions. `kryos_map_new` initializes
/// `ref_count = 1`; `kryos_map_clone` increments + returns same pointer;
/// `kryos_map_free` decrements and deallocates at 0.
#[repr(C)]
struct MapHeader {
    len: i64,
    capacity: i64,
    entries: *mut MapEntry,
    ref_count: i64,
}

// `capacity` is ALWAYS a power of two (INITIAL_CAPACITY = 16, resize doubles),
// so `x % capacity` is equivalent to `x & (capacity - 1)` and compiles to a
// single AND instead of going through the division unit. Applied to hash_key
// and every linear-probe step below.
fn hash_key(key: i64, capacity: usize) -> usize {
    let h = (key as u64).wrapping_mul(0x9E3779B97F4A7C15);
    (h as usize) & (capacity - 1)
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
                idx = (idx + 1) & (new_cap - 1);
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
        (*header).ref_count = 1;
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
            idx = (idx + 1) & (capacity - 1);
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
            idx = (idx + 1) & (capacity - 1);
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
            idx = (idx + 1) & (capacity - 1);
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
            idx = (idx + 1) & (capacity - 1);
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
                idx = (idx + 1) & (new_cap - 1);
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
            idx = (idx + 1) & (capacity - 1);
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
            idx = (idx + 1) & (capacity - 1);
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
                let mut j = (idx + 1) & (capacity - 1);
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
                    j = (j + 1) & (capacity - 1);
                }
                return old_value;
            }
            idx = (idx + 1) & (capacity - 1);
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
                let mut j = (idx + 1) & (capacity - 1);
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
                    j = (j + 1) & (capacity - 1);
                }
                return old_value;
            }
            idx = (idx + 1) & (capacity - 1);
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

/// Clone a map — H20 (shift step 30) shares the underlying pointer.
///
/// Clone a map — refcount-based share (step 37 production hardening).
///
/// Maps share entry tables across @copy struct clones via ref_count.
/// Trade-off: mutation post-clone visible across all aliases. Stage-1
/// coding style replaces locals via function returns rather than mutating
/// shared state, so this is safe in practice. With refcounted free,
/// memory is bounded (no leak).
#[no_mangle]
pub extern "C" fn kryos_map_clone(map: i64) -> i64 {
    if map == 0 {
        return kryos_map_new();
    }
    unsafe {
        let header = map as *mut MapHeader;
        (*header).ref_count += 1;
    }
    map
}

/// Retain a map — increment ref_count, return same pointer.
#[no_mangle]
pub extern "C" fn kryos_map_retain(map: i64) -> i64 {
    if map == 0 {
        return kryos_map_new();
    }
    unsafe {
        let header = map as *mut MapHeader;
        (*header).ref_count += 1;
    }
    map
}

/// Step 46c: refcounted free for maps. Retest after string_concat fix
/// to confirm whether map flakes were similar uninitialized-ref_count
/// bugs.
#[no_mangle]
pub extern "C" fn kryos_map_free(map: i64) {
    if map == 0 {
        return;
    }
    if crate::leak_on_zero() {
        // H41 pure no-op for bootstrap reliability.
        return;
    }
    unsafe {
        let header = map as *mut MapHeader;
        let rc = (*header).ref_count;
        if rc <= 0 {
            return;
        }
        let new_rc = rc - 1;
        (*header).ref_count = new_rc;
        if new_rc > 0 {
            return;
        }
        let capacity = (*header).capacity as usize;
        free_entries((*header).entries, capacity);
        // Free the header itself (symmetric with kryos_map_new's
        // alloc_zeroed). The leak-the-header-as-sentinel pattern was an
        // unbounded per-map leak; double-frees are bugs to catch, not
        // absorb.
        let layout =
            Layout::from_size_align_unchecked(std::mem::size_of::<MapHeader>(), 8);
        std::alloc::dealloc(header as *mut u8, layout);
    }
}

/// Reassignment release: free `old` unless it is the SAME handle as `new`.
/// Map handles are i64; 0 = uninitialized. Emitted by the compiler before
/// storing a new value into a mutable map local.
#[no_mangle]
pub extern "C" fn kryos_map_release_if_ne(old: i64, new: i64) -> i64 {
    if old != 0 && old != new {
        kryos_map_free(old);
    }
    0
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
        // Step 37+ semantics: kryos_map_clone is a refcount retain, not a
        // deep copy. The returned handle is the same pointer as the source,
        // sharing the underlying entry table. Reflects the share-everywhere
        // model that unblocked stage-1 self-compile.
        let map = kryos_map_new();
        kryos_map_insert(map, 1, 100);
        kryos_map_insert(map, 2, 200);
        kryos_map_insert(map, 3, 300);

        let cloned = kryos_map_clone(map);
        assert_eq!(map, cloned, "clone returns same pointer (share semantics)");
        assert_eq!(kryos_map_len(cloned), 3);
        assert_eq!(kryos_map_get(cloned, 1), 100);
        assert_eq!(kryos_map_get(cloned, 2), 200);
        assert_eq!(kryos_map_get(cloned, 3), 300);

        // Mutating "original" also mutates "clone" -- they alias the same
        // entry table. This is the trade-off accepted for stage-1's coding
        // style (functions return new structs rather than mutating shared
        // state).
        kryos_map_insert(map, 1, 999);
        assert_eq!(kryos_map_get(cloned, 1), 999, "mutation visible across aliases");

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
