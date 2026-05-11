//! MPMC channel runtime for Kryos.
//!
//! Provides a simple multi-producer, multi-consumer channel built on a
//! Mutex-protected VecDeque. Each channel handle is reference-counted so
//! it can be shared across threads.
//!
//! # Unsafe invariants (file-wide)
//!
//! See `docs/17-unsafe-audit.md` patterns 1, 4 (atomic refcounting), and 6
//! (threading). The channel header carries its own atomic refcount; clone
//! uses Relaxed, drop uses Release + Acquire fence. The Mutex / Condvar pair
//! is held only for short critical sections (queue push/pop, closed flag).
//! No `unsafe` is held across blocking waits.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

/// Internal channel state.
struct ChannelInner {
    /// Element size in bytes.
    elem_size: usize,
    /// The queue of raw byte messages, each `elem_size` bytes long.
    queue: Mutex<VecDeque<Vec<u8>>>,
    /// Condition variable for receivers waiting on data.
    not_empty: Condvar,
    /// Whether the channel has been closed.
    closed: AtomicBool,
    /// Reference count for the handle.
    ref_count: AtomicUsize,
}

/// Create a new MPMC channel for elements of `elem_size` bytes.
///
/// Returns an opaque handle (pointer to ChannelInner).
#[no_mangle]
pub extern "C" fn kryos_chan_new(elem_size: usize) -> *mut u8 {
    let inner = Box::new(ChannelInner {
        elem_size,
        queue: Mutex::new(VecDeque::new()),
        not_empty: Condvar::new(),
        closed: AtomicBool::new(false),
        ref_count: AtomicUsize::new(1),
    });
    Box::into_raw(inner) as *mut u8
}

/// Send data through a channel.
///
/// Copies `data_len` bytes from `data_ptr` into the channel queue.
/// Returns 0 on success, -1 if the channel is closed or data_len != elem_size.
#[no_mangle]
pub extern "C" fn kryos_chan_send(handle: *mut u8, data_ptr: *const u8, data_len: usize) -> i32 {
    if handle.is_null() || data_ptr.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *const ChannelInner) };

    if inner.closed.load(Ordering::Acquire) {
        return -1;
    }

    if data_len != inner.elem_size {
        return -1;
    }

    let data = unsafe { std::slice::from_raw_parts(data_ptr, data_len) }.to_vec();

    {
        let mut queue = match inner.queue.lock() {
            Ok(q) => q,
            Err(p) => p.into_inner(),
        };
        queue.push_back(data);
    }
    inner.not_empty.notify_one();

    0
}

/// Receive data from a channel.
///
/// Blocks until data is available or the channel is closed.
/// Copies up to `buf_len` bytes into `buf_ptr`.
/// Returns the number of bytes written on success, 0 if the channel is closed
/// and empty, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_chan_recv(handle: *mut u8, buf_ptr: *mut u8, buf_len: usize) -> i32 {
    if handle.is_null() || buf_ptr.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *const ChannelInner) };

    let mut queue = match inner.queue.lock() {
        Ok(q) => q,
        Err(p) => p.into_inner(),
    };

    // Wait until there is data or the channel is closed.
    while queue.is_empty() {
        if inner.closed.load(Ordering::Acquire) {
            return 0;
        }
        queue = match inner.not_empty.wait(queue) {
            Ok(q) => q,
            Err(p) => p.into_inner(),
        };
    }

    let data = match queue.pop_front() {
        Some(d) => d,
        None => return 0,
    };
    drop(queue);

    let copy_len = data.len().min(buf_len);
    unsafe {
        std::ptr::copy_nonoverlapping(data.as_ptr(), buf_ptr, copy_len);
    }

    copy_len as i32
}

/// Try to receive data from a channel without blocking.
///
/// Returns bytes written on success, 0 if no data available, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_chan_try_recv(handle: *mut u8, buf_ptr: *mut u8, buf_len: usize) -> i32 {
    if handle.is_null() || buf_ptr.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *const ChannelInner) };

    let mut queue = match inner.queue.lock() {
        Ok(q) => q,
        Err(p) => p.into_inner(),
    };
    let data = match queue.pop_front() {
        Some(d) => d,
        None => {
            if inner.closed.load(Ordering::Acquire) {
                return -1;
            }
            return 0;
        }
    };
    drop(queue);

    let copy_len = data.len().min(buf_len);
    unsafe {
        std::ptr::copy_nonoverlapping(data.as_ptr(), buf_ptr, copy_len);
    }

    copy_len as i32
}

/// Close a channel. Senders will get -1, and receivers will drain remaining
/// messages then get 0.
#[no_mangle]
pub extern "C" fn kryos_chan_close(handle: *mut u8) {
    if handle.is_null() {
        return;
    }
    let inner = unsafe { &*(handle as *const ChannelInner) };
    inner.closed.store(true, Ordering::Release);
    inner.not_empty.notify_all();
}

/// Drop a channel handle. Decrements the reference count and frees when it
/// reaches zero.
#[no_mangle]
pub extern "C" fn kryos_chan_drop(handle: *mut u8) {
    if handle.is_null() {
        return;
    }
    let inner = unsafe { &*(handle as *const ChannelInner) };
    let prev = inner.ref_count.fetch_sub(1, Ordering::Release);
    if prev == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        unsafe {
            drop(Box::from_raw(handle as *mut ChannelInner));
        }
    }
}

/// Check if a channel is closed.
///
/// Returns 1 if the channel is closed, 0 if open, -1 on error (null handle).
#[no_mangle]
pub extern "C" fn kryos_chan_is_closed(handle: *mut u8) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *const ChannelInner) };
    if inner.closed.load(Ordering::Acquire) {
        1
    } else {
        0
    }
}

/// Clone a channel handle (increment reference count).
#[no_mangle]
pub extern "C" fn kryos_chan_clone(handle: *mut u8) -> *mut u8 {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    let inner = unsafe { &*(handle as *const ChannelInner) };
    inner.ref_count.fetch_add(1, Ordering::Relaxed);
    handle
}

/// Receive an i64 from a channel with timeout (in milliseconds).
/// Returns the value on success, -1 on timeout or error.
#[no_mangle]
pub extern "C" fn kryos_chan_recv_timeout_i64(fd: i64, timeout_ms: i64) -> i64 {
    if fd <= 0 || timeout_ms < 0 {
        return -1;
    }
    let handle = fd as *mut u8;
    let timeout = std::time::Duration::from_millis(timeout_ms as u64);
    let start = std::time::Instant::now();

    loop {
        let mut buf = [0u8; 8];
        let ret = kryos_chan_try_recv(handle, buf.as_mut_ptr(), 8);
        if ret > 0 {
            return i64::from_le_bytes(buf);
        }
        if ret < 0 {
            return -1;
        }

        // Check if timeout elapsed
        if start.elapsed() >= timeout {
            return -1;
        }

        // Small sleep to avoid busy-waiting
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
