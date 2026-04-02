//! Integration tests for the Kryos runtime library.

use std::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------------------
// ARC tests
// ---------------------------------------------------------------------------

#[test]
fn arc_alloc_returns_non_null() {
    let ptr = kryos_rt::arc::kryos_arc_alloc(64, 8);
    assert!(!ptr.is_null());
    assert_eq!(kryos_rt::arc::kryos_arc_ref_count(ptr), 1);
    kryos_rt::arc::kryos_arc_release(ptr);
}

#[test]
fn arc_retain_release_cycle() {
    let ptr = kryos_rt::arc::kryos_arc_alloc(32, 8);
    assert_eq!(kryos_rt::arc::kryos_arc_ref_count(ptr), 1);

    kryos_rt::arc::kryos_arc_retain(ptr);
    assert_eq!(kryos_rt::arc::kryos_arc_ref_count(ptr), 2);

    kryos_rt::arc::kryos_arc_release(ptr);
    assert_eq!(kryos_rt::arc::kryos_arc_ref_count(ptr), 1);

    // Final release deallocates -- we can't read ref_count after this,
    // but we verify no crash occurs.
    kryos_rt::arc::kryos_arc_release(ptr);
}

#[test]
fn arc_multiple_retain_release() {
    let ptr = kryos_rt::arc::kryos_arc_alloc(16, 8);
    assert_eq!(kryos_rt::arc::kryos_arc_ref_count(ptr), 1);

    for _ in 0..5 {
        kryos_rt::arc::kryos_arc_retain(ptr);
    }
    assert_eq!(kryos_rt::arc::kryos_arc_ref_count(ptr), 6);

    for _ in 0..5 {
        kryos_rt::arc::kryos_arc_release(ptr);
    }
    assert_eq!(kryos_rt::arc::kryos_arc_ref_count(ptr), 1);

    kryos_rt::arc::kryos_arc_release(ptr);
}

/// Global flag set by the drop function to verify it was called.
static DROP_CALLED: AtomicBool = AtomicBool::new(false);

extern "C" fn test_drop_fn(_ptr: *mut u8) {
    DROP_CALLED.store(true, Ordering::SeqCst);
}

#[test]
fn arc_drop_fn_called_on_final_release() {
    DROP_CALLED.store(false, Ordering::SeqCst);

    let ptr = kryos_rt::arc::kryos_arc_alloc(24, 8);
    kryos_rt::arc::kryos_arc_set_drop(ptr, test_drop_fn);

    assert!(!DROP_CALLED.load(Ordering::SeqCst));

    // Retain then release -- should NOT trigger drop yet.
    kryos_rt::arc::kryos_arc_retain(ptr);
    kryos_rt::arc::kryos_arc_release(ptr);
    assert!(!DROP_CALLED.load(Ordering::SeqCst));

    // Final release -- should trigger drop.
    kryos_rt::arc::kryos_arc_release(ptr);
    assert!(DROP_CALLED.load(Ordering::SeqCst));
}

#[test]
fn arc_zero_size_returns_null() {
    let ptr = kryos_rt::arc::kryos_arc_alloc(0, 8);
    assert!(ptr.is_null());
}

#[test]
fn arc_null_ptr_operations_are_safe() {
    // These should not panic or crash.
    kryos_rt::arc::kryos_arc_retain(std::ptr::null_mut());
    kryos_rt::arc::kryos_arc_release(std::ptr::null_mut());
    assert_eq!(kryos_rt::arc::kryos_arc_ref_count(std::ptr::null_mut()), 0);
}

#[test]
fn arc_user_data_is_writable() {
    let ptr = kryos_rt::arc::kryos_arc_alloc(128, 8);
    assert!(!ptr.is_null());

    // Write a pattern into the user data area.
    unsafe {
        for i in 0..128u8 {
            *ptr.add(i as usize) = i;
        }
        // Read it back.
        for i in 0..128u8 {
            assert_eq!(*ptr.add(i as usize), i);
        }
    }

    kryos_rt::arc::kryos_arc_release(ptr);
}

// ---------------------------------------------------------------------------
// Channel tests
// ---------------------------------------------------------------------------

#[test]
fn channel_send_recv_roundtrip() {
    let chan = kryos_rt::channel::kryos_chan_new(4);
    assert!(!chan.is_null());

    let value: u32 = 0xDEADBEEF;
    let send_result = kryos_rt::channel::kryos_chan_send(
        chan,
        &value as *const u32 as *const u8,
        std::mem::size_of::<u32>(),
    );
    assert_eq!(send_result, 0);

    let mut recv_buf: u32 = 0;
    let recv_result = kryos_rt::channel::kryos_chan_recv(
        chan,
        &mut recv_buf as *mut u32 as *mut u8,
        std::mem::size_of::<u32>(),
    );
    assert_eq!(recv_result, 4);
    assert_eq!(recv_buf, 0xDEADBEEF);

    kryos_rt::channel::kryos_chan_drop(chan);
}

#[test]
fn channel_multiple_messages() {
    let chan = kryos_rt::channel::kryos_chan_new(8);

    for i in 0u64..10 {
        let result = kryos_rt::channel::kryos_chan_send(
            chan,
            &i as *const u64 as *const u8,
            8,
        );
        assert_eq!(result, 0);
    }

    for expected in 0u64..10 {
        let mut buf: u64 = 0;
        let result = kryos_rt::channel::kryos_chan_recv(
            chan,
            &mut buf as *mut u64 as *mut u8,
            8,
        );
        assert_eq!(result, 8);
        assert_eq!(buf, expected);
    }

    kryos_rt::channel::kryos_chan_drop(chan);
}

#[test]
fn channel_close_returns_error_on_send() {
    let chan = kryos_rt::channel::kryos_chan_new(4);

    kryos_rt::channel::kryos_chan_close(chan);

    let value: u32 = 42;
    let result = kryos_rt::channel::kryos_chan_send(
        chan,
        &value as *const u32 as *const u8,
        4,
    );
    assert_eq!(result, -1);

    kryos_rt::channel::kryos_chan_drop(chan);
}

#[test]
fn channel_close_recv_returns_zero() {
    let chan = kryos_rt::channel::kryos_chan_new(4);

    kryos_rt::channel::kryos_chan_close(chan);

    let mut buf: u32 = 0;
    let result = kryos_rt::channel::kryos_chan_recv(
        chan,
        &mut buf as *mut u32 as *mut u8,
        4,
    );
    assert_eq!(result, 0);

    kryos_rt::channel::kryos_chan_drop(chan);
}

#[test]
fn channel_multiple_send_recv() {
    let chan = kryos_rt::channel::kryos_chan_new(8);

    // Send multiple values sequentially
    for i in 0u64..10 {
        let result = kryos_rt::channel::kryos_chan_send(
            chan,
            &i as *const u64 as *const u8,
            8,
        );
        assert_eq!(result, 0);
    }

    // Receive them all
    let mut received = Vec::new();
    for _ in 0..10 {
        let mut buf: u64 = 0;
        let result = kryos_rt::channel::kryos_chan_recv(
            chan,
            &mut buf as *mut u64 as *mut u8,
            8,
        );
        assert_eq!(result, 8);
        received.push(buf);
    }

    for (i, &v) in received.iter().enumerate() {
        assert_eq!(v, i as u64);
    }

    kryos_rt::channel::kryos_chan_drop(chan);
}

#[test]
fn channel_wrong_size_returns_error() {
    let chan = kryos_rt::channel::kryos_chan_new(4);

    let value: u64 = 42;
    let result = kryos_rt::channel::kryos_chan_send(
        chan,
        &value as *const u64 as *const u8,
        8, // elem_size is 4, sending 8
    );
    assert_eq!(result, -1);

    kryos_rt::channel::kryos_chan_drop(chan);
}

// ---------------------------------------------------------------------------
// Panic format tests (we test the formatters, not the abort)
// ---------------------------------------------------------------------------

#[test]
fn panic_format_message() {
    let msg = kryos_rt::panic::format_panic_message("something went wrong");
    assert_eq!(msg, "kryos panic: something went wrong");
}

#[test]
fn panic_format_with_location() {
    let msg = kryos_rt::panic::format_panic_with_location(
        "index out of bounds",
        "main.kry",
        42,
        10,
    );
    assert_eq!(msg, "kryos panic at main.kry:42:10: index out of bounds");
}

#[test]
fn panic_format_empty_message() {
    let msg = kryos_rt::panic::format_panic_message("");
    assert_eq!(msg, "kryos panic: ");
}

// ---------------------------------------------------------------------------
// Allocator tests
// ---------------------------------------------------------------------------

#[test]
fn alloc_dealloc_roundtrip() {
    let ptr = kryos_rt::alloc::kryos_alloc(256, 8);
    assert!(!ptr.is_null());

    // Write a pattern.
    unsafe {
        for i in 0..=255u8 {
            *ptr.add(i as usize) = i;
        }
        for i in 0..=255u8 {
            assert_eq!(*ptr.add(i as usize), i);
        }
    }

    kryos_rt::alloc::kryos_dealloc(ptr, 256, 8);
}

#[test]
fn alloc_zero_size_returns_null() {
    let ptr = kryos_rt::alloc::kryos_alloc(0, 8);
    assert!(ptr.is_null());
}

#[test]
fn realloc_grows_memory() {
    let ptr = kryos_rt::alloc::kryos_alloc(64, 8);
    assert!(!ptr.is_null());

    // Write data.
    unsafe {
        for i in 0..64u8 {
            *ptr.add(i as usize) = i;
        }
    }

    let new_ptr = kryos_rt::alloc::kryos_realloc(ptr, 64, 256, 8);
    assert!(!new_ptr.is_null());

    // Verify original data preserved.
    unsafe {
        for i in 0..64u8 {
            assert_eq!(*new_ptr.add(i as usize), i);
        }
    }

    kryos_rt::alloc::kryos_dealloc(new_ptr, 256, 8);
}

#[test]
fn realloc_null_acts_as_alloc() {
    let ptr = kryos_rt::alloc::kryos_realloc(std::ptr::null_mut(), 0, 128, 8);
    assert!(!ptr.is_null());
    kryos_rt::alloc::kryos_dealloc(ptr, 128, 8);
}

#[test]
fn realloc_to_zero_frees() {
    let ptr = kryos_rt::alloc::kryos_alloc(64, 8);
    assert!(!ptr.is_null());

    let result = kryos_rt::alloc::kryos_realloc(ptr, 64, 0, 8);
    assert!(result.is_null());
}

#[test]
fn dealloc_null_is_safe() {
    // Should not panic or crash.
    kryos_rt::alloc::kryos_dealloc(std::ptr::null_mut(), 64, 8);
}

// ---------------------------------------------------------------------------
// Actor tests
// ---------------------------------------------------------------------------

extern "C" fn echo_actor(state_ptr: *mut u8) {
    // State pointer is unused for this test actor, but we use the mailbox.
    let _ = state_ptr;
    let mut buf = [0u8; 256];
    loop {
        let n = kryos_rt::actor::kryos_actor_recv(
            buf.as_mut_ptr(),
            buf.len(),
        );
        if n <= 0 {
            break;
        }
        // Just consume messages until mailbox is closed.
    }
}

#[test]
fn actor_spawn_returns_nonzero_id() {
    let id = kryos_rt::actor::kryos_actor_spawn(echo_actor, std::ptr::null_mut());
    assert!(id > 0);

    // Give the actor a moment then send a message.
    let msg = b"hello";
    let result = kryos_rt::actor::kryos_actor_send(id, msg.as_ptr(), msg.len());
    // The actor is alive, so send should succeed.
    assert_eq!(result, 0);
}

#[test]
fn actor_send_to_invalid_id_fails() {
    let msg = b"test";
    let result = kryos_rt::actor::kryos_actor_send(999_999, msg.as_ptr(), msg.len());
    assert_eq!(result, -1);
}

#[test]
fn actor_recv_from_non_actor_thread_fails() {
    let mut buf = [0u8; 64];
    let result = kryos_rt::actor::kryos_actor_recv(buf.as_mut_ptr(), buf.len());
    assert_eq!(result, -1);
}
