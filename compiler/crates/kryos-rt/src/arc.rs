//! Atomic Reference Counting runtime for Kryos.
//!
//! Every ARC-managed object has an ArcHeader prepended to the user data.
//! The pointer returned to Kryos user code points past the header to the
//! payload. All bookkeeping (retain, release, drop) operates via the header.
//!
//! Memory layout:
//!   [ArcHeader | user data ... ]
//!   ^                ^
//!   |                |-- ptr returned to user
//!   |-- actual allocation start

use std::alloc::Layout;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Header prepended to every ARC-managed allocation.
#[repr(C)]
pub struct ArcHeader {
    /// Current reference count (starts at 1).
    pub ref_count: AtomicUsize,
    /// Optional destructor called when ref_count reaches 0.
    /// Receives the user-data pointer (NOT the header pointer).
    pub drop_fn: Option<extern "C" fn(*mut u8)>,
    /// Size of the user data region (excluding the header).
    pub size: usize,
    /// Alignment of the user data region.
    pub align: usize,
}

/// Compute the combined Layout for ArcHeader + payload and the offset to the
/// payload start.
fn arc_layout(size: usize, align: usize) -> Option<(Layout, usize)> {
    let header_layout = Layout::new::<ArcHeader>();
    let payload_layout = Layout::from_size_align(size, align).ok()?;
    let (combined, offset) = header_layout.extend(payload_layout).ok()?;
    Some((combined.pad_to_align(), offset))
}

/// Recover the ArcHeader pointer from a user-data pointer and the alignment
/// that was used when allocating.
///
/// # Safety
/// `ptr` must have been returned by `kryos_arc_alloc`.
unsafe fn header_from_user_ptr(ptr: *mut u8, align: usize) -> *mut ArcHeader {
    let header_layout = Layout::new::<ArcHeader>();
    // We only need the offset, so we use size=1 -- the offset depends only
    // on the alignment, not the payload size.
    let payload_layout = Layout::from_size_align(1, align).unwrap();
    let (_, offset) = header_layout.extend(payload_layout).unwrap();
    ptr.sub(offset) as *mut ArcHeader
}

/// Recover the ArcHeader from a user pointer.
///
/// Since we enforce align >= align_of::<ArcHeader>() in kryos_arc_alloc,
/// and for all such alignments Layout::extend produces a deterministic offset,
/// we first probe at the minimum alignment, read the stored align, and
/// recompute if necessary.
///
/// # Safety
/// `ptr` must have been returned by `kryos_arc_alloc`.
unsafe fn header_from_ptr(ptr: *mut u8) -> *mut ArcHeader {
    let min_align = std::mem::align_of::<ArcHeader>();
    let candidate = header_from_user_ptr(ptr, min_align);
    let stored_align = (*candidate).align;
    if stored_align == min_align {
        candidate
    } else {
        header_from_user_ptr(ptr, stored_align)
    }
}

/// Allocate an ARC-managed object with the given payload size and alignment.
///
/// The returned pointer points to the user data area. The reference count
/// starts at 1. The drop function is initially None; set it with
/// `kryos_arc_set_drop`.
///
/// Returns null on allocation failure or zero size.
#[no_mangle]
pub extern "C" fn kryos_arc_alloc(size: usize, align: usize) -> *mut u8 {
    if size == 0 {
        return std::ptr::null_mut();
    }
    // Ensure alignment is at least that of ArcHeader so header recovery works.
    let effective_align = align.max(std::mem::align_of::<ArcHeader>());

    let (combined, offset) = match arc_layout(size, effective_align) {
        Some(v) => v,
        None => return std::ptr::null_mut(),
    };

    // SAFETY: combined layout has non-zero size.
    let base = unsafe { std::alloc::alloc(combined) };
    if base.is_null() {
        return std::ptr::null_mut();
    }

    // Initialize the header.
    let header = base as *mut ArcHeader;
    unsafe {
        header.write(ArcHeader {
            ref_count: AtomicUsize::new(1),
            drop_fn: None,
            size,
            align: effective_align,
        });
    }

    // Return pointer to user data (base + offset).
    unsafe { base.add(offset) }
}

/// Atomically increment the reference count of an ARC object.
///
/// # Safety
/// `ptr` must have been returned by `kryos_arc_alloc` and must still be live.
#[no_mangle]
pub extern "C" fn kryos_arc_retain(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let header = header_from_ptr(ptr);
        (*header).ref_count.fetch_add(1, Ordering::Relaxed);
    }
}

/// Atomically decrement the reference count. When it reaches zero, calls the
/// drop function (if set) and deallocates the memory.
///
/// # Safety
/// `ptr` must have been returned by `kryos_arc_alloc` and the caller must
/// own a reference.
#[no_mangle]
pub extern "C" fn kryos_arc_release(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let header = header_from_ptr(ptr);

        // Use Release ordering on decrement so all prior writes are visible
        // before we potentially drop.
        let prev = (*header).ref_count.fetch_sub(1, Ordering::Release);
        if prev == 1 {
            // Synchronize with all prior Release decrements.
            std::sync::atomic::fence(Ordering::Acquire);

            // Read fields before deallocation.
            let drop_fn = (*header).drop_fn;
            let size = (*header).size;
            let align = (*header).align;

            // Call drop function if set.
            if let Some(f) = drop_fn {
                f(ptr);
            }

            // Deallocate the entire block.
            let (combined, _) = arc_layout(size, align).unwrap();
            std::alloc::dealloc(header as *mut u8, combined);
        }
    }
}

/// Set the drop function for an ARC-managed object.
///
/// The drop function receives the user-data pointer when the reference count
/// reaches zero, before the memory is freed.
#[no_mangle]
pub extern "C" fn kryos_arc_set_drop(ptr: *mut u8, drop_fn: extern "C" fn(*mut u8)) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let header = header_from_ptr(ptr);
        (*header).drop_fn = Some(drop_fn);
    }
}

/// Get the current reference count of an ARC-managed object.
///
/// Primarily useful for debugging and testing.
#[no_mangle]
pub extern "C" fn kryos_arc_ref_count(ptr: *mut u8) -> usize {
    if ptr.is_null() {
        return 0;
    }
    unsafe {
        let header = header_from_ptr(ptr);
        (*header).ref_count.load(Ordering::Relaxed)
    }
}
