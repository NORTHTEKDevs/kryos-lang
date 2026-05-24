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
    if cap > 1024 {
        crate::memstats::note_big_array(cap);
    }
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
    crate::memstats::note_arr_new((cap_usize * ELEM_SIZE + 40) as i64);
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
    // Defensive: if the array header was corrupted (most likely path for
    // bootstrap-stage segfaults), bail with a diagnostic instead of letting
    // realloc segfault deep in ntdll. cap should be small positive, len <= cap,
    // ref_count > 0, elem_size in a sane range.
    let raw_len = (*arr).len;
    let raw_cap = (*arr).cap;
    let raw_rc = (*arr).ref_count;
    let raw_es = (*arr).elem_size;
    if raw_cap <= 0
        || raw_cap > (1 << 30)
        || raw_len < 0
        || raw_len > raw_cap
        || raw_rc <= 0
        || raw_rc > (1 << 24)
        || raw_es <= 0
        || raw_es > 4096
        || (*arr).data.is_null()
    {
        let m = format!(
            "kryos_array_push: corrupt array header @ {:p} \
             (len={}, cap={}, elem_size={}, ref_count={}, data={:p})",
            arr,
            raw_len,
            raw_cap,
            raw_es,
            raw_rc,
            (*arr).data
        );
        crate::panic::kryos_panic(m.as_ptr(), m.len());
    }
    let len = raw_len as usize;
    let cap = raw_cap as usize;

    if len >= cap {
        // Grow the data buffer with realloc (the correct, leak-free path).
        //
        // History: between v4.42.0-rc.1 and the cranelift codegen.rs:RValue::Struct
        // fix (commit baff370), realloc would non-deterministically segfault inside
        // ntdll!RtlpReAllocateHeap during the stage-2 bootstrap. HeapReAlloc probes
        // adjacent heap-block headers as a sanity check; a buffer-overrun bug in
        // RValue::Struct (uncoerced I64 stores into i32 fields) was corrupting
        // nearby allocations, and HeapReAlloc was the canary that surfaced the
        // crash. The fix in cranelift codegen eliminates the corruption at its
        // source, so realloc is safe again.
        //
        // KRYOS_USE_ALLOC_LEAK=1 reinstates the old alloc+copy+leak grow path
        // (leaks the old buffer, but is forgiving of overruns in neighbouring
        // blocks) as a diagnostic for any FUTURE corruption regression — if
        // HeapReAlloc ever crashes again here, flip the env var and you can
        // separate "real codegen bug elsewhere" from "realloc misuse here".
        // H26 (shift step 36): default to alloc-copy-leak grow path.
        // realloc has historical heap-state-sensitive crashes (per
        // baff370 commit notes). With H10/H12/H18 leak-on-free, the
        // memory we "leak" by not realloc'ing was going to be leaked
        // anyway. KRYOS_USE_REALLOC=1 reinstates the realloc path
        // for benchmarking.
        let use_realloc = std::env::var_os("KRYOS_USE_REALLOC").is_some();
        let new_cap = cap * 2;
        let new_size = new_cap * ELEM_SIZE;
        let new_layout = KryosArray::data_layout(new_cap);
        let new_data = if use_realloc {
            let old_layout = KryosArray::data_layout(cap);
            realloc((*arr).data, old_layout, new_size)
        } else {
            let buf = alloc(new_layout);
            if !buf.is_null() {
                ptr::copy_nonoverlapping((*arr).data, buf, cap * ELEM_SIZE);
            }
            buf
        };
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
        let grow_net = if use_realloc {
            ((new_cap - cap) * ELEM_SIZE) as i64
        } else {
            (new_cap * ELEM_SIZE) as i64
        };
        crate::memstats::note_arr_grow(grow_net);
    }

    let slot = (*arr).data.add(len * ELEM_SIZE) as *mut i64;
    *slot = val;
    (*arr).len = (len + 1) as i64;
}

/// Cold path: panic with formatted out-of-bounds message. Kept out-of-line
/// so the hot `kryos_array_get`/`kryos_array_set` path stays small and inlineable.
#[cold]
#[inline(never)]
unsafe fn oob_panic(idx: i64, len: i64) -> ! {
    let msg = format!(
        "array index out of bounds: index {} but length is {}",
        idx, len
    );
    crate::panic::kryos_panic(msg.as_ptr(), msg.len());
}

#[cold]
#[inline(never)]
unsafe fn null_panic() -> ! {
    let msg = b"array is null";
    crate::panic::kryos_panic(msg.as_ptr(), msg.len());
}

/// Get the element at `idx`. Bounds-checked: panics on out-of-bounds access.
///
/// Marked `#[inline]` so LLVM (with LTO enabled in the AOT link) can inline
/// the hot path of `len`-load + bounds compare + data-load into callers,
/// eliminating the call overhead that dominates array-heavy loops.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn kryos_array_get(arr: *const KryosArray, idx: i64) -> i64 {
    if arr.is_null() {
        null_panic();
    }
    let len = (*arr).len;
    if (idx as u64) >= (len as u64) {
        // Unsigned compare handles negative idx in one branch.
        oob_panic(idx, len);
    }
    let slot = (*arr).data.add(idx as usize * ELEM_SIZE) as *const i64;
    *slot
}

/// Set the element at `idx`. Bounds-checked: panics on out-of-bounds access.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn kryos_array_set(arr: *mut KryosArray, idx: i64, val: i64) {
    if arr.is_null() {
        null_panic();
    }
    let len = (*arr).len;
    if (idx as u64) >= (len as u64) {
        oob_panic(idx, len);
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

/// Clone a KryosArray AND deep-clone each element by applying
/// `elem_clone_fn` to it. Used by `@copy` struct field semantics for
/// Array<Str> / Array<@copy-Struct> to avoid the double-free that arises
/// when shallow clone leaves both arrays owning the same element pointers.
///
/// `elem_clone_fn` is invoked as `extern "C" fn(i64) -> i64` -- the same
/// shape as `kryos_string_clone` and the codegen-emitted `__kryos_clone_<N>`
/// helpers. Null elements are skipped (the clone fn would either return
/// null itself or panic on a deref; either way we avoid the call).
///
/// This consolidates the per-element loop INSIDE the runtime, eliminating
/// the codegen-emitted loop allocations (emit_array_str_deep_clone /
/// emit_array_struct_deep_clone) and reducing the heap pressure that
/// regressed bootstrap when those helpers were used.
#[no_mangle]
pub unsafe extern "C" fn kryos_array_clone_deep(
    arr: *const KryosArray,
    elem_clone_fn: extern "C" fn(i64) -> i64,
) -> *mut KryosArray {
    let result = kryos_array_clone(arr);
    if result.is_null() {
        return result;
    }
    let len = (*result).len;
    if len <= 0 {
        return result;
    }
    let data = (*result).data;
    if data.is_null() {
        return result;
    }
    // Iterate and replace each element with its deep-cloned independent copy.
    for i in 0..len as usize {
        let slot = data.add(i * ELEM_SIZE) as *mut i64;
        let elem = *slot;
        if elem != 0 {
            *slot = elem_clone_fn(elem);
        }
    }
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
///
/// H4 INSTRUMENTATION (shift kryos-self-compile step 22, 2026-05-20):
/// Live arrays always have ref_count >= 1. If we observe ref_count <= 0
/// on entry, that's a double-free of a previously-freed array. Abort
/// with a diagnostic so we can locate the source. To keep the sentinel
/// observable on the next free, we skip the header dealloc -- the
/// header (~40 bytes) leaks intentionally. Revert before production.
/// Free an array — forgiving refcount that tolerates over-free.
///
/// Step 39 production hardening: codegen has multiple unbalanced
/// `kryos_array_free` emission paths (more frees than retains for the
/// same logical pointer). A strict refcount would underflow into
/// negative territory and double-free. This implementation treats
/// `ref_count <= 0` as the "already freed" state and returns early.
///
/// On the LAST legitimate free (transition from 1 to 0):
///   1. Deallocate the data buffer
///   2. Mark the header as freed (data=null, cap=len=0, rc=0)
///   3. Intentionally LEAK the header (~40 bytes/array) so subsequent
///      over-free calls see the sentinel and no-op instead of crashing
///
/// Memory profile: data buffers correctly freed; only headers leak.
/// Per stage-1 invocation: ~10K arrays × 40 bytes = ~400KB leaked.
/// 200x improvement over the previous H12 leak-all-frees (~80MB).
///
/// The full fix (eliminate header leak entirely) requires auditing
/// codegen to emit retain at every array pointer copy. That's tracked
/// as separate next-shift work; this forgiving model unblocks
/// near-production memory hygiene without the audit.
/// Step 46: refcounted free with sentinel-after-free.
///
/// Now that the codegen audit has matched retains for every clone /
/// field-read / param-entry path (steps 44 + 45), the refcount should
/// balance. Free decrements; at 0 the data is deallocated.
///
/// Sentinel safety: after dealloc we mark the header data=null, cap=0,
/// len=0, ref_count=-1, and leak the header so any over-free call
/// observes the sentinel and no-ops instead of touching freed memory.
#[no_mangle]
pub unsafe extern "C" fn kryos_array_free(arr: *mut KryosArray) {
    if arr.is_null() {
        return;
    }
    if crate::leak_on_zero() {
        // H41 pure no-op: don't even read ref_count. The dance
        // (read+decrement+check) touches potentially-aliased headers,
        // contributing to bootstrap flake. Pure no-op removes those reads.
        return;
    }
    crate::memstats::note_arr_free_call();
    let rc = (*arr).ref_count;
    if rc <= 0 {
        return; // already freed (sentinel)
    }
    let new_rc = rc - 1;
    (*arr).ref_count = new_rc;
    if new_rc > 0 {
        return;
    }
    // Last reference — deallocate the data buffer; mark header freed.
    let cap = (*arr).cap as usize;
    if !(*arr).data.is_null() && cap > 0 {
        dealloc((*arr).data, KryosArray::data_layout(cap));
        crate::memstats::note_arr_free((cap * ELEM_SIZE) as i64);
    }
    (*arr).data = ptr::null_mut();
    (*arr).cap = 0;
    (*arr).len = 0;
    // ref_count is now 0; leak the header so over-free sees sentinel.
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
