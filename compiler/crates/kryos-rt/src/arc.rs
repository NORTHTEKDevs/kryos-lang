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

/// Sentinel value used to detect valid ArcHeaders. Helps the runtime avoid
/// dereferencing non-ARC pointers (e.g. static function pointers wrapped
/// as closures) when retain/release is called defensively.
pub const ARC_MAGIC: u64 = 0xA7C0_DEAD_BEEF_CAFEu64;

/// Header prepended to every ARC-managed allocation.
#[repr(C)]
pub struct ArcHeader {
    /// Sentinel — must equal ARC_MAGIC. Checked by retain/release to avoid
    /// touching non-ARC memory (e.g. static fn pointers wrapped as closures).
    pub magic: u64,
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
            magic: ARC_MAGIC,
            ref_count: AtomicUsize::new(1),
            drop_fn: None,
            size,
            align: effective_align,
        });
    }

    // Zero the user-data region. Boxed aggregates (e.g. @copy structs
    // stored into arrays via the LLVM backend's struct->i64 boxing) are
    // written field-by-field; any byte NOT covered by an explicit store
    // (struct padding, or a field the producer left undef) would otherwise
    // hold allocator garbage that reads non-deterministically -> the
    // self-host emits different object bytes each run. Zeroing here matches
    // the Cranelift backend's calloc semantics and makes boxed reads
    // deterministic.
    unsafe {
        std::ptr::write_bytes(base.add(offset), 0, size);
    }

    // Return pointer to user data (base + offset).
    unsafe { base.add(offset) }
}

/// Validate that a pointer looks like a real ARC-allocated object by checking
/// the magic sentinel. Returns true only if the pointer is non-null AND the
/// preceding header carries the expected magic value.
///
/// This protects against retain/release being called on static function
/// pointers or other non-ARC memory that the front-end currently treats as
/// closures (a known limitation of the Function-typed ABI).
///
/// # Safety
/// Reads from `ptr - sizeof(ArcHeader)` if `ptr` is non-null. Caller must
/// ensure the address space is at least readable at that offset; in practice
/// the only failure mode is a SIGSEGV from unmapped memory, which we cannot
/// guard against without a signal handler. The magic check filters out the
/// common case of static-data pointers.
unsafe fn is_arc_ptr(ptr: *mut u8) -> bool {
    if ptr.is_null() {
        return false;
    }
    let min_align = std::mem::align_of::<ArcHeader>();
    let candidate = header_from_user_ptr(ptr, min_align);
    // Read the magic field at a fixed offset. We use std::ptr::read_unaligned
    // since `candidate` may be inside an arbitrary page.
    let magic = std::ptr::read_unaligned(&(*candidate).magic as *const u64);
    magic == ARC_MAGIC
}

/// Atomically increment the reference count of an ARC object.
///
/// Defensive: silently no-ops on non-ARC pointers (detected via magic sentinel),
/// so the front-end can uniformly emit retain/release for Function-typed
/// values even when the underlying value is a static function pointer.
///
/// # Safety
/// `ptr` must either be null, a real `kryos_arc_alloc` return, or a pointer
/// whose preceding bytes are readable (so the magic check can run).
#[no_mangle]
pub extern "C" fn kryos_arc_retain(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        if !is_arc_ptr(ptr) {
            return;
        }
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
        if !is_arc_ptr(ptr) {
            return;
        }
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
        if !is_arc_ptr(ptr) {
            return;
        }
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
        if !is_arc_ptr(ptr) {
            return 0;
        }
        let header = header_from_ptr(ptr);
        (*header).ref_count.load(Ordering::Relaxed)
    }
}
