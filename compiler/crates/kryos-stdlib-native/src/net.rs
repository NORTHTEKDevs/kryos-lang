//! TCP/UDP networking for the Kryos native stdlib.
//!
//! Uses a global socket descriptor table (similar to io.rs) to map Kryos handles
//! to Rust `TcpStream` and `TcpListener` objects.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;

static SOCKET_TABLE: Mutex<Option<SocketTable>> = Mutex::new(None);

enum SocketEntry {
    Stream(TcpStream),
    Listener(TcpListener),
}

struct SocketTable {
    map: HashMap<i64, SocketEntry>,
    next_fd: i64,
}

impl SocketTable {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            next_fd: 1000, // offset from file fds
        }
    }

    fn insert(&mut self, entry: SocketEntry) -> i64 {
        let fd = self.next_fd;
        self.next_fd += 1;
        self.map.insert(fd, entry);
        fd
    }
}

fn with_socket_table<F, R>(f: F) -> R
where
    F: FnOnce(&mut SocketTable) -> R,
{
    let mut guard = SOCKET_TABLE.lock().unwrap_or_else(|e| e.into_inner());
    let table = guard.get_or_insert_with(SocketTable::new);
    f(table)
}

/// Connects to a TCP server at `host:port`.
///
/// Returns a socket descriptor on success, or -1 on error.
#[no_mangle]
pub extern "C" fn kryos_tcp_connect(host_ptr: *const u8, host_len: usize, port: u16) -> i64 {
    if host_ptr.is_null() {
        return -1;
    }
    let host = unsafe { std::slice::from_raw_parts(host_ptr, host_len) };
    let host = match std::str::from_utf8(host) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let addr = format!("{host}:{port}");
    match TcpStream::connect(&addr) {
        Ok(stream) => with_socket_table(|table| table.insert(SocketEntry::Stream(stream))),
        Err(_) => -1,
    }
}

/// Binds a TCP listener to `host:port`.
///
/// Returns a listener descriptor on success, or -1 on error.
#[no_mangle]
pub extern "C" fn kryos_tcp_bind(host_ptr: *const u8, host_len: usize, port: u16) -> i64 {
    if host_ptr.is_null() {
        return -1;
    }
    let host = unsafe { std::slice::from_raw_parts(host_ptr, host_len) };
    let host = match std::str::from_utf8(host) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let addr = format!("{host}:{port}");
    match TcpListener::bind(&addr) {
        Ok(listener) => with_socket_table(|table| table.insert(SocketEntry::Listener(listener))),
        Err(_) => -1,
    }
}

/// Accepts a connection on a TCP listener.
///
/// Returns a new stream descriptor on success, or -1 on error.
#[no_mangle]
pub extern "C" fn kryos_tcp_accept(listener_fd: i64) -> i64 {
    with_socket_table(|table| {
        let listener = match table.map.get(&listener_fd) {
            Some(SocketEntry::Listener(l)) => l,
            _ => return -1,
        };
        match listener.accept() {
            Ok((stream, _addr)) => {
                let fd = table.next_fd;
                table.next_fd += 1;
                table.map.insert(fd, SocketEntry::Stream(stream));
                fd
            }
            Err(_) => -1,
        }
    })
}

/// Sends data over a TCP stream.
///
/// Returns the number of bytes written, or -1 on error.
#[no_mangle]
pub extern "C" fn kryos_tcp_send(fd: i64, data: *const u8, len: usize) -> i64 {
    if data.is_null() || len == 0 {
        return -1;
    }
    let slice = unsafe { std::slice::from_raw_parts(data, len) };
    with_socket_table(|table| match table.map.get_mut(&fd) {
        Some(SocketEntry::Stream(stream)) => match stream.write(slice) {
            Ok(n) => n as i64,
            Err(_) => -1,
        },
        _ => -1,
    })
}

/// Receives data from a TCP stream.
///
/// Returns the number of bytes read, 0 on EOF, or -1 on error.
#[no_mangle]
pub extern "C" fn kryos_tcp_recv(fd: i64, buf: *mut u8, buf_len: usize) -> i64 {
    if buf.is_null() || buf_len == 0 {
        return -1;
    }
    let slice = unsafe { std::slice::from_raw_parts_mut(buf, buf_len) };
    with_socket_table(|table| match table.map.get_mut(&fd) {
        Some(SocketEntry::Stream(stream)) => match stream.read(slice) {
            Ok(n) => n as i64,
            Err(_) => -1,
        },
        _ => -1,
    })
}

/// Closes a socket (stream or listener).
///
/// Returns 0 on success, -1 if the fd was not found.
#[no_mangle]
pub extern "C" fn kryos_socket_close(fd: i64) -> i32 {
    with_socket_table(|table| {
        if table.map.remove(&fd).is_some() {
            0
        } else {
            -1
        }
    })
}
