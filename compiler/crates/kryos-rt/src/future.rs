//! Minimal async runtime — single-threaded executor with a ready queue.

use std::collections::VecDeque;
use std::sync::Mutex;

static READY_QUEUE: Mutex<VecDeque<Box<dyn FnOnce() + Send>>> = Mutex::new(VecDeque::new());

/// Spawn a function onto the async executor's ready queue.
#[no_mangle]
pub extern "C" fn kryos_async_spawn(fn_ptr: extern "C" fn()) {
    READY_QUEUE
        .lock()
        .unwrap()
        .push_back(Box::new(move || fn_ptr()));
}

/// Drain and run all tasks in the ready queue.
#[no_mangle]
pub extern "C" fn kryos_async_run() {
    loop {
        let task = READY_QUEUE.lock().unwrap().pop_front();
        match task {
            Some(f) => f(),
            None => break,
        }
    }
}
