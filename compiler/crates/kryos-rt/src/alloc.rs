//! Memory allocation helpers for the Kryos runtime.
//!
//! Thin wrappers around Rust's global allocator, exposed as extern "C" functions
//! for use by compiled Kryos code.

use std::alloc::Layout;

/// Allocate `size` bytes with the given alignment.
///
/// Returns a null pointer if `size` is 0 or alignment is invalid.
#[no_mangle]
pub extern "C" fn kryos_alloc(size: usize, align: usize) -> *mut u8 {
    crate::fault::install();
    crate::fault::hang_tick();
    if size == 0 {
        return std::ptr::null_mut();
    }
    let layout = match Layout::from_size_align(size, align) {
        Ok(l) => l,
        Err(_) => return std::ptr::null_mut(),
    };
    // SAFETY: layout has non-zero size (checked above).
    let p = unsafe { std::alloc::alloc(layout) };
    if !p.is_null() {
        crate::memstats::note_struct_new(size as i64);
    }
    p
}

/// Deallocate memory previously allocated by kryos_alloc.
///
/// ptr must have been returned by kryos_alloc with the same size and align.
#[no_mangle]
pub extern "C" fn kryos_dealloc(ptr: *mut u8, size: usize, align: usize) {
    if ptr.is_null() || size == 0 {
        return;
    }
    let layout = match Layout::from_size_align(size, align) {
        Ok(l) => l,
        Err(_) => return,
    };
    crate::memstats::note_struct_free(size as i64);
    // SAFETY: caller guarantees ptr was allocated with this layout.
    unsafe { std::alloc::dealloc(ptr, layout) }
}

/// Reallocate memory, changing its size while preserving contents up to
/// min(old_size, new_size).
///
/// Returns null on failure (original allocation is NOT freed in that case).
#[no_mangle]
pub extern "C" fn kryos_realloc(
    ptr: *mut u8,
    old_size: usize,
    new_size: usize,
    align: usize,
) -> *mut u8 {
    if ptr.is_null() {
        return kryos_alloc(new_size, align);
    }
    if new_size == 0 {
        kryos_dealloc(ptr, old_size, align);
        return std::ptr::null_mut();
    }
    let old_layout = match Layout::from_size_align(old_size, align) {
        Ok(l) => l,
        Err(_) => return std::ptr::null_mut(),
    };
    // SAFETY: caller guarantees ptr was allocated with old_layout.
    unsafe { std::alloc::realloc(ptr, old_layout, new_size) }
}

// ---------------------------------------------------------------------------
// Pooled box allocator for compiled-code heap boxes.
//
// Compiled Kryos code boxes struct/enum values (array elements, drop-helper
// owned boxes) through `kryos_calloc` / `kryos_free`. Those boxes are small
// (8 bytes per field) and churn hard in allocation-heavy programs, where the
// system calloc/free pair dominated profiles (binary_trees: ~2x total
// runtime). This pool serves them from per-thread size-class freelists
// carved out of 64 KB slabs:
//
//   * alloc: pop the class freelist (or carve from the current slab), then
//     zero the block (calloc semantics).
//   * free: push the block onto the freeing thread's class freelist. Blocks
//     may migrate between threads; that is safe - a block is owned by
//     whichever freelist holds it.
//   * Each block carries a 16-byte header recording its size class, because
//     `kryos_free` receives no size. Only this pair touches the header.
//   * Slabs and freelists are never returned to the OS - consistent with the
//     documented leak-on-free policy for CLI-lifetime processes.
//
// Oversized requests (> 1 KB payload) fall through to plain system calloc,
// tagged CLASS_SYSTEM in the header so `kryos_free` routes them back.
//
// `KRYOS_PLAIN_ALLOC=1` (read once per process) bypasses the pool entirely:
// every box goes through ucrt calloc/free with NO header, byte-identical to
// the pre-pool runtime. That keeps the AddressSanitizer debugging method
// intact - ASAN intercepts ucrt, not our slabs. The flag is process-constant
// so alloc/free pairing can never mix.
// ---------------------------------------------------------------------------

use std::cell::RefCell;
use std::sync::atomic::{AtomicU8, Ordering};

// Payload size classes (bytes). Boxes are 8 bytes per field; classes cover
// 1..=128 fields. The header adds 16 bytes on top of these.
const CLASSES: [usize; 8] = [16, 32, 64, 128, 256, 512, 768, 1024];
const HEADER: usize = 16;
const SLAB: usize = 64 * 1024;
const CLASS_SYSTEM: u64 = u64::MAX;

// 0 = undecided, 1 = plain (system calloc/free), 2 = pooled.
static MODE: AtomicU8 = AtomicU8::new(0);

extern "C" {
    fn calloc(count: usize, size: usize) -> *mut u8;
    fn free(ptr: *mut u8);
    fn getenv(name: *const u8) -> *const u8;
}

fn plain_alloc() -> bool {
    match MODE.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            // C getenv: no Rust-side allocation, safe at any init point.
            let v = unsafe { getenv(c"KRYOS_PLAIN_ALLOC".as_ptr() as *const u8) };
            let is_plain = !v.is_null() && unsafe { *v != 0 && *v != b'0' };
            MODE.store(if is_plain { 1 } else { 2 }, Ordering::Relaxed);
            is_plain
        }
    }
}

/// True when `KRYOS_PLAIN_ALLOC=1` routed all box/buffer traffic to the
/// system allocator (the ASAN-visible diagnostic mode).
pub(crate) fn kryos_plain_alloc_mode() -> bool {
    plain_alloc()
}

struct Pool {
    freelists: [Vec<*mut u8>; CLASSES.len()],
    slab: *mut u8,
    slab_used: usize,
}

impl Pool {
    const fn new() -> Self {
        Pool {
            freelists: [
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ],
            slab: std::ptr::null_mut(),
            slab_used: SLAB, // forces a fresh slab on first carve
        }
    }

    fn class_for(size: usize) -> Option<usize> {
        CLASSES.iter().position(|&c| size <= c)
    }

    /// Returns an UNINITIALIZED block of exactly CLASSES[class] bytes,
    /// 16-aligned, popped from the freelist or carved from the slab.
    fn alloc(&mut self, class: usize) -> *mut u8 {
        if let Some(b) = self.freelists[class].pop() {
            return b;
        }
        let block_size = CLASSES[class];
        if self.slab_used + block_size > SLAB {
            let layout = std::alloc::Layout::from_size_align(SLAB, 16).unwrap();
            // SAFETY: SLAB is non-zero; the slab is intentionally never freed.
            self.slab = unsafe { std::alloc::alloc(layout) };
            if self.slab.is_null() {
                return std::ptr::null_mut();
            }
            self.slab_used = 0;
        }
        let b = unsafe { self.slab.add(self.slab_used) };
        self.slab_used += block_size;
        b
    }
}

/// Pool-allocate `size` bytes, UNINITIALIZED, for runtime-internal callers
/// that know the size at free time (array/string buffers). Small sizes come
/// from the per-thread pool; larger ones from the system allocator. Pair
/// with `pool_free(ptr, size)` using the SAME size. In plain mode this is
/// exactly `std::alloc::alloc` with 8-alignment, byte-identical to the
/// pre-pool runtime.
pub(crate) fn pool_alloc(size: usize) -> *mut u8 {
    if size == 0 {
        return std::ptr::null_mut();
    }
    if plain_alloc() {
        let layout = match std::alloc::Layout::from_size_align(size, 8) {
            Ok(l) => l,
            Err(_) => return std::ptr::null_mut(),
        };
        return unsafe { std::alloc::alloc(layout) };
    }
    match Pool::class_for(size) {
        Some(class) => POOL.with(|p| p.borrow_mut().alloc(class)),
        None => {
            let layout = match std::alloc::Layout::from_size_align(size, 8) {
                Ok(l) => l,
                Err(_) => return std::ptr::null_mut(),
            };
            unsafe { std::alloc::alloc(layout) }
        }
    }
}

/// Free a block from `pool_alloc`. `size` must match the alloc call.
pub(crate) fn pool_free(ptr: *mut u8, size: usize) {
    if ptr.is_null() || size == 0 {
        return;
    }
    if plain_alloc() {
        if let Ok(layout) = std::alloc::Layout::from_size_align(size, 8) {
            unsafe { std::alloc::dealloc(ptr, layout) };
        }
        return;
    }
    match Pool::class_for(size) {
        Some(class) => POOL.with(|p| p.borrow_mut().freelists[class].push(ptr)),
        None => {
            if let Ok(layout) = std::alloc::Layout::from_size_align(size, 8) {
                unsafe { std::alloc::dealloc(ptr, layout) };
            }
        }
    }
}

thread_local! {
    static POOL: RefCell<Pool> = const { RefCell::new(Pool::new()) };
}

/// Allocate a zeroed box of `count * size` bytes for compiled Kryos code.
/// Drop-in for libc `calloc` at codegen box sites; pair with `kryos_free`.
#[no_mangle]
pub extern "C" fn kryos_calloc(count: i64, size: i64) -> *mut u8 {
    if count <= 0 || size <= 0 {
        return std::ptr::null_mut();
    }
    let payload = match (count as usize).checked_mul(size as usize) {
        Some(t) => t,
        None => return std::ptr::null_mut(),
    };
    if plain_alloc() {
        return unsafe { calloc(count as usize, size as usize) };
    }
    // The class tag occupies the first HEADER bytes of the block, so the
    // block must cover header + payload.
    match Pool::class_for(HEADER + payload) {
        Some(class) => {
            let block = POOL.with(|p| p.borrow_mut().alloc(class));
            if block.is_null() {
                return std::ptr::null_mut();
            }
            unsafe {
                std::ptr::write_bytes(block, 0, CLASSES[class]);
                *(block as *mut u64) = class as u64;
                block.add(HEADER)
            }
        }
        None => {
            // Oversized: system calloc with a header so free can route it.
            let raw = unsafe { calloc(1, HEADER + payload) };
            if raw.is_null() {
                return std::ptr::null_mut();
            }
            unsafe {
                *(raw as *mut u64) = CLASS_SYSTEM;
                raw.add(HEADER)
            }
        }
    }
}

/// Free a box allocated by `kryos_calloc`. Null is a no-op.
#[no_mangle]
pub extern "C" fn kryos_free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    if plain_alloc() {
        unsafe { free(ptr) };
        return;
    }
    let block = unsafe { ptr.sub(HEADER) };
    let class = unsafe { *(block as *const u64) };
    if class == CLASS_SYSTEM {
        unsafe { free(block) };
        return;
    }
    POOL.with(|p| p.borrow_mut().freelists[class as usize].push(block));
}

#[cfg(test)]
mod pool_tests {
    use super::*;

    #[test]
    fn boxes_are_zeroed_and_recycled() {
        // Force pooled mode regardless of env.
        MODE.store(2, Ordering::Relaxed);
        let a = kryos_calloc(1, 16);
        assert!(!a.is_null());
        unsafe {
            for i in 0..16 {
                assert_eq!(*a.add(i), 0, "fresh box not zeroed at byte {i}");
            }
            std::ptr::write_bytes(a, 0xAB, 16);
        }
        kryos_free(a);
        let b = kryos_calloc(1, 16);
        assert_eq!(a, b, "same-class box should be recycled LIFO");
        unsafe {
            for i in 0..16 {
                assert_eq!(*b.add(i), 0, "recycled box not re-zeroed at byte {i}");
            }
        }
        kryos_free(b);
    }

    #[test]
    fn class_boundaries_and_oversize() {
        MODE.store(2, Ordering::Relaxed);
        for payload in [1i64, 16, 17, 64, 1024, 1025, 100_000] {
            let p = kryos_calloc(1, payload);
            assert!(!p.is_null(), "alloc failed for {payload}");
            unsafe {
                // Touch every byte to catch undersized blocks.
                std::ptr::write_bytes(p, 0xCD, payload as usize);
            }
            kryos_free(p);
        }
    }

    #[test]
    fn many_boxes_distinct() {
        MODE.store(2, Ordering::Relaxed);
        let mut ptrs: Vec<*mut u8> = (0..10_000).map(|_| kryos_calloc(1, 24)).collect();
        ptrs.sort();
        ptrs.dedup();
        assert_eq!(ptrs.len(), 10_000, "pool handed out a duplicate live box");
        for p in ptrs {
            kryos_free(p);
        }
    }

    #[test]
    fn cross_thread_free_is_safe() {
        MODE.store(2, Ordering::Relaxed);
        let ptrs: Vec<usize> = (0..1000).map(|_| kryos_calloc(1, 32) as usize).collect();
        let h = std::thread::spawn(move || {
            for p in ptrs {
                kryos_free(p as *mut u8);
            }
        });
        h.join().unwrap();
        // This thread can still allocate fine afterward.
        let p = kryos_calloc(1, 32);
        assert!(!p.is_null());
        kryos_free(p);
    }

    #[test]
    fn zero_and_negative_sizes() {
        MODE.store(2, Ordering::Relaxed);
        assert!(kryos_calloc(0, 8).is_null());
        assert!(kryos_calloc(-1, 8).is_null());
        assert!(kryos_calloc(1, 0).is_null());
        kryos_free(std::ptr::null_mut()); // must not crash
    }
}
