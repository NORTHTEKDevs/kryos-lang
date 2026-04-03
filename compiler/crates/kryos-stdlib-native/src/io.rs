//! File I/O operations for the Kryos native stdlib.
//!
//! Provides `#[no_mangle] pub extern "C"` functions for file open/read/write/close
//! as well as stdout, stderr, and stdin operations. Uses a global file descriptor
//! table (HashMap<i64, File> behind a Mutex) to map Kryos handles to Rust File objects.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::sync::Mutex;

static FD_TABLE: Mutex<Option<FdTable>> = Mutex::new(None);

struct FdTable {
    map: HashMap<i64, File>,
    next_fd: i64,
}

impl FdTable {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            // Start at 3 to avoid collision with stdin/stdout/stderr conventions.
            next_fd: 3,
        }
    }

    fn insert(&mut self, file: File) -> i64 {
        let fd = self.next_fd;
        self.next_fd += 1;
        self.map.insert(fd, file);
        fd
    }

    fn get_mut(&mut self, fd: i64) -> Option<&mut File> {
        self.map.get_mut(&fd)
    }

    fn remove(&mut self, fd: i64) -> Option<File> {
        self.map.remove(&fd)
    }
}

fn with_fd_table<F, R>(f: F) -> R
where
    F: FnOnce(&mut FdTable) -> R,
{
    let mut guard = FD_TABLE.lock().unwrap_or_else(|e| e.into_inner());
    let table = guard.get_or_insert_with(FdTable::new);
    f(table)
}

/// Opens a file at the given path.
///
/// `mode`: 0 = read, 1 = write (create/truncate), 2 = append.
/// Returns a file descriptor (>= 3) on success, or -1 on error.
#[no_mangle]
pub extern "C" fn kryos_file_open(path_ptr: *const u8, path_len: usize, mode: u8) -> i64 {
    if path_ptr.is_null() {
        return -1;
    }
    let path = unsafe { std::slice::from_raw_parts(path_ptr, path_len) };
    let path = match std::str::from_utf8(path) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let file = match mode {
        0 => File::open(path),
        1 => OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path),
        2 => OpenOptions::new()
            .write(true)
            .create(true)
            .append(true)
            .open(path),
        _ => return -1,
    };

    match file {
        Ok(f) => with_fd_table(|table| table.insert(f)),
        Err(_) => -1,
    }
}

/// Reads up to `buf_len` bytes from the file descriptor into `buf`.
///
/// Returns the number of bytes read, 0 on EOF, or -1 on error.
#[no_mangle]
pub extern "C" fn kryos_file_read(fd: i64, buf: *mut u8, buf_len: usize) -> i64 {
    if buf.is_null() || buf_len == 0 {
        return -1;
    }
    let slice = unsafe { std::slice::from_raw_parts_mut(buf, buf_len) };
    with_fd_table(|table| match table.get_mut(fd) {
        Some(file) => match file.read(slice) {
            Ok(n) => n as i64,
            Err(_) => -1,
        },
        None => -1,
    })
}

/// Writes `data_len` bytes from `data` to the file descriptor.
///
/// Returns the number of bytes written, or -1 on error.
#[no_mangle]
pub extern "C" fn kryos_file_write(fd: i64, data: *const u8, data_len: usize) -> i64 {
    if data.is_null() || data_len == 0 {
        return -1;
    }
    let slice = unsafe { std::slice::from_raw_parts(data, data_len) };
    with_fd_table(|table| match table.get_mut(fd) {
        Some(file) => match file.write(slice) {
            Ok(n) => n as i64,
            Err(_) => -1,
        },
        None => -1,
    })
}

/// Closes the file descriptor.
///
/// Returns 0 on success, -1 if the fd was not found.
#[no_mangle]
pub extern "C" fn kryos_file_close(fd: i64) -> i32 {
    with_fd_table(|table| {
        if table.remove(fd).is_some() {
            0
        } else {
            -1
        }
    })
}

/// Writes `len` bytes from `data` to stdout.
///
/// Returns the number of bytes written, or -1 on error.
#[no_mangle]
pub extern "C" fn kryos_stdout_write(data: *const u8, len: usize) -> i64 {
    if data.is_null() || len == 0 {
        return -1;
    }
    let slice = unsafe { std::slice::from_raw_parts(data, len) };
    let mut stdout = std::io::stdout().lock();
    match stdout.write_all(slice) {
        Ok(()) => len as i64,
        Err(_) => -1,
    }
}

/// Writes `len` bytes from `data` to stderr.
///
/// Returns the number of bytes written, or -1 on error.
#[no_mangle]
pub extern "C" fn kryos_stderr_write(data: *const u8, len: usize) -> i64 {
    if data.is_null() || len == 0 {
        return -1;
    }
    let slice = unsafe { std::slice::from_raw_parts(data, len) };
    let mut stderr = std::io::stderr().lock();
    match stderr.write_all(slice) {
        Ok(()) => len as i64,
        Err(_) => -1,
    }
}

/// Reads up to `buf_len` bytes from stdin into `buf`.
///
/// Returns the number of bytes read, or -1 on error.
#[no_mangle]
pub extern "C" fn kryos_stdin_read(buf: *mut u8, buf_len: usize) -> i64 {
    if buf.is_null() || buf_len == 0 {
        return -1;
    }
    let slice = unsafe { std::slice::from_raw_parts_mut(buf, buf_len) };
    let mut stdin = std::io::stdin().lock();
    match stdin.read(slice) {
        Ok(n) => n as i64,
        Err(_) => -1,
    }
}
