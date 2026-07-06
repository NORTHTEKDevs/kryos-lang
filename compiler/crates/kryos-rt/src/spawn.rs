//! Spawn runtime for Kryos — fire-and-forget thread creation.
//!
//! `kryos_spawn` takes a function pointer and an array of i64 arguments,
//! spawns an OS thread to run the function, and returns immediately.
//! All spawned threads are tracked so `kryos_spawn_wait_all` can join
//! them before the process exits.

use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;

/// Global registry of spawned thread handles.
static HANDLES: OnceLock<Mutex<Vec<JoinHandle<()>>>> = OnceLock::new();

fn get_handles() -> &'static Mutex<Vec<JoinHandle<()>>> {
    HANDLES.get_or_init(|| Mutex::new(Vec::new()))
}

/// Spawn a function on a new OS thread.
///
/// `fn_ptr` is the function address (cast to i64).
/// `args_ptr` points to a contiguous array of i64 arguments.
/// `arg_count` is the number of arguments (0..=8 supported).
///
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_spawn(fn_ptr: i64, args_ptr: *const i64, arg_count: i64) -> i64 {
    if fn_ptr == 0 {
        return -1;
    }

    // Copy arguments so the spawned thread owns them.
    let args: Vec<i64> = if arg_count > 0 && !args_ptr.is_null() {
        unsafe { std::slice::from_raw_parts(args_ptr, arg_count as usize).to_vec() }
    } else {
        Vec::new()
    };

    let handle = std::thread::spawn(move || unsafe {
        match args.len() {
            0 => {
                let f: extern "C" fn() -> i64 = std::mem::transmute(fn_ptr as usize);
                f();
            }
            1 => {
                let f: extern "C" fn(i64) -> i64 = std::mem::transmute(fn_ptr as usize);
                f(args[0]);
            }
            2 => {
                let f: extern "C" fn(i64, i64) -> i64 = std::mem::transmute(fn_ptr as usize);
                f(args[0], args[1]);
            }
            3 => {
                let f: extern "C" fn(i64, i64, i64) -> i64 = std::mem::transmute(fn_ptr as usize);
                f(args[0], args[1], args[2]);
            }
            4 => {
                let f: extern "C" fn(i64, i64, i64, i64) -> i64 =
                    std::mem::transmute(fn_ptr as usize);
                f(args[0], args[1], args[2], args[3]);
            }
            5 => {
                let f: extern "C" fn(i64, i64, i64, i64, i64) -> i64 =
                    std::mem::transmute(fn_ptr as usize);
                f(args[0], args[1], args[2], args[3], args[4]);
            }
            6 => {
                let f: extern "C" fn(i64, i64, i64, i64, i64, i64) -> i64 =
                    std::mem::transmute(fn_ptr as usize);
                f(args[0], args[1], args[2], args[3], args[4], args[5]);
            }
            7 => {
                let f: extern "C" fn(i64, i64, i64, i64, i64, i64, i64) -> i64 =
                    std::mem::transmute(fn_ptr as usize);
                f(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6],
                );
            }
            8 => {
                let f: extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64) -> i64 =
                    std::mem::transmute(fn_ptr as usize);
                f(
                    args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7],
                );
            }
            _ => {
                eprintln!(
                    "[spawn warning] function has {} arguments; \
                               spawned functions support at most 8 arguments directly",
                    args.len()
                );
            }
        }
        // A `throw` that unwound out of the thread's entry function would
        // otherwise vanish with the thread-local state. Report it like a
        // Rust thread panic: message to stderr, thread dies, process lives.
        crate::exception::kryos_exception_report_thread_if_pending();
    });

    let mut handles = match get_handles().lock() {
        Ok(h) => h,
        Err(p) => p.into_inner(),
    };
    handles.push(handle);
    0
}

/// Wait for all spawned threads to complete.
///
/// Called at the end of `main` to ensure fire-and-forget threads finish
/// before the process exits.
#[no_mangle]
pub extern "C" fn kryos_spawn_wait_all() {
    let mut handles = match get_handles().lock() {
        Ok(h) => h,
        Err(p) => p.into_inner(),
    };
    for h in handles.drain(..) {
        let _ = h.join();
    }
}

/// Sleep the current thread for `seconds` (f64, reinterpreted from i64 bits).
#[no_mangle]
pub extern "C" fn kryos_sleep(seconds_bits: i64) {
    let seconds = f64::from_bits(seconds_bits as u64);
    if seconds > 0.0 {
        let dur = std::time::Duration::from_secs_f64(seconds);
        std::thread::sleep(dur);
    }
}

/// Sleep the current thread for `millis` milliseconds.
#[no_mangle]
pub extern "C" fn kryos_sleep_ms(millis: i64) {
    if millis > 0 {
        let dur = std::time::Duration::from_millis(millis as u64);
        // On a coop task, yield the baton while sleeping so sibling async tasks
        // run concurrently (`await sleep(..)` becomes a non-blocking suspension).
        crate::executor::io_offload(|| std::thread::sleep(dur));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};

    // Use test-specific counters to avoid parallel test contamination.
    static COUNTER_A: AtomicI64 = AtomicI64::new(0);
    static COUNTER_B: AtomicI64 = AtomicI64::new(0);

    extern "C" fn increment_a() -> i64 {
        COUNTER_A.fetch_add(1, Ordering::SeqCst);
        0
    }

    extern "C" fn add_value_b(v: i64) -> i64 {
        COUNTER_B.fetch_add(v, Ordering::SeqCst);
        0
    }

    #[test]
    fn spawn_no_args() {
        COUNTER_A.store(0, Ordering::SeqCst);
        let fn_ptr = increment_a as *const () as usize as i64;
        assert_eq!(kryos_spawn(fn_ptr, std::ptr::null(), 0), 0);
        // Give the thread time to run and join.
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(COUNTER_A.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn spawn_one_arg() {
        COUNTER_B.store(0, Ordering::SeqCst);
        let fn_ptr = add_value_b as *const () as usize as i64;
        let args: [i64; 1] = [42];
        assert_eq!(kryos_spawn(fn_ptr, args.as_ptr(), 1), 0);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(COUNTER_B.load(Ordering::SeqCst), 42);
    }

    #[test]
    fn spawn_null_fn_returns_error() {
        assert_eq!(kryos_spawn(0, std::ptr::null(), 0), -1);
    }

    #[test]
    fn sleep_works() {
        let start = std::time::Instant::now();
        let bits = 0.05_f64.to_bits() as i64;
        kryos_sleep(bits);
        assert!(start.elapsed().as_millis() >= 40);
    }
}
