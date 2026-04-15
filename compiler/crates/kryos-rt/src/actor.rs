//! Actor spawn and mailbox runtime for Kryos.
//!
//! Each actor runs on its own OS thread with a dedicated mailbox (an MPMC
//! channel). Actors are identified by a monotonically increasing u64 ID.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};

/// Global actor ID counter.
static NEXT_ACTOR_ID: AtomicU64 = AtomicU64::new(1);

/// Per-actor mailbox.
struct Mailbox {
    queue: VecDeque<Vec<u8>>,
    closed: bool,
}

/// Global state: per-actor mailbox, condvar, and send lock.
struct ActorEntry {
    mailbox: Mutex<Mailbox>,
    condvar: Condvar,
    /// Spinlock held by senders during multi-word message sequences
    /// (tag + args) to prevent interleaving from concurrent senders.
    send_locked: std::sync::atomic::AtomicBool,
}

/// The global actor registry maps actor IDs to their mailbox entries.
/// The outer Mutex is only held briefly to insert/lookup entries.
static REGISTRY: OnceLock<Mutex<HashMap<u64, std::sync::Arc<ActorEntry>>>> = OnceLock::new();

fn get_registry() -> &'static Mutex<HashMap<u64, std::sync::Arc<ActorEntry>>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

// Thread-local storage for the current actor's ID.
// 0 means "not an actor thread."
thread_local! {
    static CURRENT_ACTOR_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Spawn an actor on a new OS thread.
///
/// `entry_fn` is the actor's main function, receiving `state_ptr` as argument.
/// The actor can call `kryos_actor_recv` to read from its mailbox.
///
/// Returns the actor's unique ID (always > 0).
#[no_mangle]
pub extern "C" fn kryos_actor_spawn(entry_fn: extern "C" fn(*mut u8), state_ptr: *mut u8) -> u64 {
    let actor_id = NEXT_ACTOR_ID.fetch_add(1, Ordering::Relaxed);

    let entry = std::sync::Arc::new(ActorEntry {
        mailbox: Mutex::new(Mailbox {
            queue: VecDeque::new(),
            closed: false,
        }),
        condvar: Condvar::new(),
        send_locked: std::sync::atomic::AtomicBool::new(false),
    });

    // Register before spawning so early sends don't race.
    {
        let mut reg = match get_registry().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        reg.insert(actor_id, entry.clone());
    }

    // Convert the raw pointer to usize so the closure is Send.
    // extern "C" fn pointers are Copy + Send, but *mut u8 is not Send.
    let state_addr = state_ptr as usize;

    let entry_for_thread = entry;
    std::thread::spawn(move || {
        CURRENT_ACTOR_ID.set(actor_id);

        let ptr = state_addr as *mut u8;
        entry_fn(ptr);

        // Mark mailbox closed after actor function returns.
        {
            let mut mb = match entry_for_thread.mailbox.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            mb.closed = true;
        }
        entry_for_thread.condvar.notify_all();
    });

    actor_id
}

/// Send a message to an actor's mailbox.
///
/// Copies `msg_len` bytes from `msg_ptr` into the actor's queue.
/// Returns 0 on success, -1 if the actor doesn't exist or its mailbox is closed.
#[no_mangle]
pub extern "C" fn kryos_actor_send(actor_id: u64, msg_ptr: *const u8, msg_len: usize) -> i32 {
    if msg_ptr.is_null() || msg_len == 0 {
        return -1;
    }

    // Look up the actor entry (brief registry lock).
    let entry = {
        let reg = match get_registry().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        match reg.get(&actor_id) {
            Some(e) => e.clone(),
            None => return -1,
        }
    };

    let mut mb = match entry.mailbox.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if mb.closed {
        return -1;
    }

    let data = unsafe { std::slice::from_raw_parts(msg_ptr, msg_len) }.to_vec();
    mb.queue.push_back(data);
    drop(mb);
    entry.condvar.notify_one();

    0
}

/// Receive a message from the calling actor's own mailbox.
///
/// Must be called from within an actor thread (spawned by `kryos_actor_spawn`).
/// Blocks until a message is available.
///
/// Returns bytes written into `buf_ptr`, 0 if the mailbox is closed and empty,
/// -1 on error (e.g., called from a non-actor thread).
#[no_mangle]
pub extern "C" fn kryos_actor_recv(buf_ptr: *mut u8, buf_len: usize) -> i32 {
    if buf_ptr.is_null() {
        return -1;
    }

    let actor_id = CURRENT_ACTOR_ID.get();
    if actor_id == 0 {
        return -1;
    }

    // Look up own entry (brief registry lock).
    let entry = {
        let reg = match get_registry().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        match reg.get(&actor_id) {
            Some(e) => e.clone(),
            None => return -1,
        }
    };

    let mut mb = match entry.mailbox.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };

    loop {
        if let Some(data) = mb.queue.pop_front() {
            let copy_len = data.len().min(buf_len);
            unsafe {
                std::ptr::copy_nonoverlapping(data.as_ptr(), buf_ptr, copy_len);
            }
            return copy_len as i32;
        }
        if mb.closed {
            return 0;
        }
        mb = match entry.condvar.wait(mb) {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
    }
}

/// Acquire the send lock for an actor. This must be held while sending
/// a multi-word message (tag + arguments) to prevent interleaving from
/// concurrent senders. Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_actor_lock(actor_id: u64) -> i32 {
    let entry = {
        let reg = match get_registry().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        match reg.get(&actor_id) {
            Some(e) => e.clone(),
            None => return -1,
        }
    };
    // Spin until we acquire the lock.
    while entry
        .send_locked
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        std::hint::spin_loop();
    }
    0
}

/// Release the send lock for an actor. Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_actor_unlock(actor_id: u64) -> i32 {
    let entry = {
        let reg = match get_registry().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        match reg.get(&actor_id) {
            Some(e) => e.clone(),
            None => return -1,
        }
    };
    entry.send_locked.store(false, Ordering::Release);
    0
}
