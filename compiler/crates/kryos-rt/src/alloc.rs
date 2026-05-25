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
