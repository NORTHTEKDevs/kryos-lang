//! Process management for the Kryos native stdlib.
//!
//! Provides environment variable access, process exit, subprocess execution,
//! command-line argument access, and directory listing.
//!
//! # Unsafe invariants (file-wide)
//!
//! See `docs/17-unsafe-audit.md` patterns 1 (FFI handle reconstruction) and 7
//! (libc syscall FFI). `KryosString` handles are validated `!= 0` before deref;
//! every CString conversion goes through `CString::new(...).unwrap_or_else`
//! to reject embedded NULs. Subprocess paths use `std::process::Command`
//! which handles fork/exec safety internally.

use kryos_rt::string::{kryos_string_new, KryosString};
use std::process::Command;

/// Reads the environment variable named by `key[0..key_len]` into `val_buf`.
///
/// Returns the length of the value on success (may exceed `val_buf_len` if the
/// buffer is too small — in that case, only `val_buf_len` bytes are written).
/// Returns -1 if the variable is not found or the key is invalid UTF-8.
#[no_mangle]
pub extern "C" fn kryos_env_get(
    key_ptr: *const u8,
    key_len: usize,
    val_buf: *mut u8,
    val_buf_len: usize,
) -> i64 {
    if key_ptr.is_null() {
        return -1;
    }
    let key = unsafe { std::slice::from_raw_parts(key_ptr, key_len) };
    let key = match std::str::from_utf8(key) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    match std::env::var(key) {
        Ok(val) => {
            let val_bytes = val.as_bytes();
            let copy_len = val_bytes.len().min(val_buf_len);
            if !val_buf.is_null() && copy_len > 0 {
                let out = unsafe { std::slice::from_raw_parts_mut(val_buf, copy_len) };
                out.copy_from_slice(&val_bytes[..copy_len]);
            }
            val_bytes.len() as i64
        }
        Err(_) => -1,
    }
}

/// Terminates the process with the given exit code. Never returns.
#[no_mangle]
pub extern "C" fn kryos_process_exit(code: i32) -> ! {
    std::process::exit(code)
}

// ---------------------------------------------------------------------------
// Subprocess execution
// ---------------------------------------------------------------------------

/// Execute a subprocess. Returns a heap-allocated result struct:
///   [exit_code: i64, stdout_handle: i64, stderr_handle: i64]
///
/// `cmd_ptr`/`cmd_len` is the command string (e.g. "ls" or "python3").
/// `args_ptr` is a pointer to an array of KryosString handle i64s.
/// `args_count` is the number of arguments.
///
/// Returns a pointer to [exit_code, stdout_kryos_string, stderr_kryos_string].
/// On failure to launch, exit_code = -1.
#[no_mangle]
pub extern "C" fn kryos_process_exec(
    cmd_ptr: *const u8,
    cmd_len: i64,
    args_ptr: *const i64,
    args_count: i64,
) -> i64 {
    let cmd = if cmd_ptr.is_null() || cmd_len < 0 {
        return -1;
    } else {
        let slice = unsafe { std::slice::from_raw_parts(cmd_ptr, cmd_len as usize) };
        match std::str::from_utf8(slice) {
            Ok(s) => s.to_string(),
            Err(_) => return -1,
        }
    };

    let mut command = Command::new(&cmd);

    // Parse arguments from the args array. Each arg is a KryosString handle.
    if !args_ptr.is_null() && args_count > 0 {
        let handles = unsafe { std::slice::from_raw_parts(args_ptr, args_count as usize) };
        for &handle in handles {
            if handle != 0 {
                let ks = handle as *const KryosString;
                let s = unsafe {
                    let data = (*ks).data;
                    let len = (*ks).len as usize;
                    let slice = std::slice::from_raw_parts(data, len);
                    std::str::from_utf8(slice).unwrap_or("")
                };
                command.arg(s);
            }
        }
    }

    match command.output() {
        Ok(output) => {
            let exit_code = output.status.code().unwrap_or(-1) as i64;

            let stdout_str = String::from_utf8_lossy(&output.stdout);
            let stderr_str = String::from_utf8_lossy(&output.stderr);

            let stdout_handle =
                unsafe { kryos_string_new(stdout_str.as_ptr(), stdout_str.len() as i64) } as i64;
            let stderr_handle =
                unsafe { kryos_string_new(stderr_str.as_ptr(), stderr_str.len() as i64) } as i64;

            // Allocate a 3-slot result: [exit_code, stdout, stderr]
            let result = Box::into_raw(Box::new([exit_code, stdout_handle, stderr_handle]));
            result as i64
        }
        Err(_) => -1,
    }
}

/// Simple exec that runs a command string and returns the exit code.
/// stdout/stderr are inherited (printed to the terminal).
#[no_mangle]
pub extern "C" fn kryos_process_exec_simple(cmd_ptr: *const u8, cmd_len: i64) -> i64 {
    if cmd_ptr.is_null() || cmd_len < 0 {
        return -1;
    }
    let cmd = unsafe { std::slice::from_raw_parts(cmd_ptr, cmd_len as usize) };
    let cmd = match std::str::from_utf8(cmd) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    // Split command on spaces for simple execution.
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() {
        return -1;
    }

    let mut command = Command::new(parts[0]);
    if parts.len() > 1 {
        command.args(&parts[1..]);
    }

    match command.status() {
        Ok(status) => status.code().unwrap_or(-1) as i64,
        Err(_) => -1,
    }
}

// ---------------------------------------------------------------------------
// Command-line arguments
// ---------------------------------------------------------------------------

/// Returns the number of command-line arguments.
#[no_mangle]
pub extern "C" fn kryos_process_argc() -> i64 {
    std::env::args().count() as i64
}

/// Returns the Nth command-line argument as a KryosString handle.
/// Returns 0 (null) if the index is out of bounds.
#[no_mangle]
pub extern "C" fn kryos_process_argv(index: i64) -> i64 {
    if index < 0 {
        return 0;
    }
    let idx = index as usize;
    match std::env::args().nth(idx) {
        Some(arg) => (unsafe { kryos_string_new(arg.as_ptr(), arg.len() as i64) }) as i64,
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// Directory listing
// ---------------------------------------------------------------------------

/// Lists entries in a directory. Returns the count of entries found.
/// Each entry name is pushed as a KryosString handle into `out_array`
/// (a pointer to an array of i64 handles, pre-allocated by the caller).
/// `max_entries` limits how many entries to return.
///
/// Returns the total number of entries (may exceed max_entries), or -1 on error.
#[no_mangle]
pub extern "C" fn kryos_dir_list(
    path_ptr: *const u8,
    path_len: i64,
    out_array: *mut i64,
    max_entries: i64,
) -> i64 {
    if path_ptr.is_null() || path_len < 0 {
        return -1;
    }
    let path = unsafe { std::slice::from_raw_parts(path_ptr, path_len as usize) };
    let path = match std::str::from_utf8(path) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let entries = match std::fs::read_dir(path) {
        Ok(rd) => rd,
        Err(_) => return -1,
    };

    let max = max_entries as usize;
    let mut count: usize = 0;

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if count < max && !out_array.is_null() {
            let handle =
                unsafe { kryos_string_new(name_str.as_ptr(), name_str.len() as i64) } as i64;
            unsafe {
                *out_array.add(count) = handle;
            }
        }
        count += 1;
    }

    count as i64
}

/// Walks a directory tree recursively. Same interface as kryos_dir_list
/// but returns full paths for all files/dirs found recursively.
#[no_mangle]
pub extern "C" fn kryos_dir_walk(
    path_ptr: *const u8,
    path_len: i64,
    out_array: *mut i64,
    max_entries: i64,
) -> i64 {
    if path_ptr.is_null() || path_len < 0 {
        return -1;
    }
    let path = unsafe { std::slice::from_raw_parts(path_ptr, path_len as usize) };
    let path = match std::str::from_utf8(path) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let max = max_entries as usize;
    let mut count: usize = 0;

    fn walk_recursive(dir: &std::path::Path, out_array: *mut i64, max: usize, count: &mut usize) {
        let entries = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let path_str = path.to_string_lossy();
            if *count < max && !out_array.is_null() {
                let handle =
                    unsafe { kryos_string_new(path_str.as_ptr(), path_str.len() as i64) } as i64;
                unsafe {
                    *out_array.add(*count) = handle;
                }
            }
            *count += 1;
            if path.is_dir() {
                walk_recursive(&path, out_array, max, count);
            }
        }
    }

    walk_recursive(std::path::Path::new(path), out_array, max, &mut count);
    count as i64
}
