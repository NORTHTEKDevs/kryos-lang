//! TCP/UDP networking for the Kryos native stdlib.
//!
//! Uses a global socket descriptor table (similar to io.rs) to map Kryos handles
//! to Rust `TcpStream` and `TcpListener` objects.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

static SOCKET_TABLE: Mutex<Option<SocketTable>> = Mutex::new(None);

enum SocketEntry {
    Stream(TcpStream),
    // Wrapped in Arc so we can clone a handle, release the mutex, then call
    // the blocking accept() without holding the global lock the whole time.
    Listener(Arc<TcpListener>),
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
        Ok(listener) => {
            with_socket_table(|table| table.insert(SocketEntry::Listener(Arc::new(listener))))
        }
        Err(_) => -1,
    }
}

/// Accepts a connection on a TCP listener.
///
/// Returns a new stream descriptor on success, or -1 on error.
///
/// The global socket mutex is released before calling the blocking `accept()`
/// so that other socket operations (recv/send/close) on spawned threads can
/// proceed while the main thread waits for a new connection.
#[no_mangle]
pub extern "C" fn kryos_tcp_accept(listener_fd: i64) -> i64 {
    // Phase 1: hold the lock only long enough to clone the Arc.
    let listener_arc = with_socket_table(|table| match table.map.get(&listener_fd) {
        Some(SocketEntry::Listener(l)) => Some(Arc::clone(l)),
        _ => None,
    });

    let listener_arc = match listener_arc {
        Some(a) => a,
        None => return -1,
    };

    // Phase 2: block on accept() WITHOUT holding the mutex.
    match listener_arc.accept() {
        Ok((stream, _addr)) => {
            // Phase 3: re-acquire briefly to insert the new stream.
            with_socket_table(|table| {
                let fd = table.next_fd;
                table.next_fd += 1;
                table.map.insert(fd, SocketEntry::Stream(stream));
                fd
            })
        }
        Err(_) => -1,
    }
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

// ── Kryos-string-handle wrappers ─────────────────────────────────────────────
// Kryos represents strings as i64 handles pointing to KryosString structs.
// These wrappers accept i64 handles and delegate to the raw-pointer functions above.

#[repr(C)]
struct KryosString {
    len: i64,
    cap: i64,
    data: *mut u8,
}

unsafe fn handle_to_bytes(handle: i64) -> (*const u8, usize) {
    if handle == 0 {
        return (std::ptr::null(), 0);
    }
    let s = handle as *const KryosString;
    ((*s).data as *const u8, (*s).len as usize)
}

/// `tcp_connect(host: str, port: i64) -> i64`
#[no_mangle]
pub extern "C" fn kryos_tcp_connect_ks(host_handle: i64, port: i64) -> i64 {
    let (ptr, len) = unsafe { handle_to_bytes(host_handle) };
    if ptr.is_null() {
        return -1;
    }
    kryos_tcp_connect(ptr, len, port as u16)
}

/// `tcp_listen(host: str, port: i64) -> i64`
#[no_mangle]
pub extern "C" fn kryos_tcp_bind_ks(host_handle: i64, port: i64) -> i64 {
    let (ptr, len) = unsafe { handle_to_bytes(host_handle) };
    if ptr.is_null() {
        return -1;
    }
    kryos_tcp_bind(ptr, len, port as u16)
}

/// `tcp_send(fd: i64, data: str) -> i64`
#[no_mangle]
pub extern "C" fn kryos_tcp_send_ks(fd: i64, data_handle: i64) -> i64 {
    let (ptr, len) = unsafe { handle_to_bytes(data_handle) };
    if ptr.is_null() {
        return 0;
    }
    kryos_tcp_send(fd, ptr, len)
}

/// `tcp_recv(fd: i64, max_bytes: i64) -> str`
/// Returns a new KryosString handle containing the received bytes.
#[no_mangle]
pub extern "C" fn kryos_tcp_recv_ks(fd: i64, max_bytes: i64) -> i64 {
    let buf_len = if max_bytes <= 0 {
        4096
    } else {
        max_bytes as usize
    };
    let mut buf = vec![0u8; buf_len];
    let n = kryos_tcp_recv(fd, buf.as_mut_ptr(), buf_len);
    let n = n.max(0) as usize;
    // Build a heap KryosString from the received bytes.
    let data = buf[..n].to_vec();
    let boxed = Box::new(KryosString {
        len: n as i64,
        cap: n as i64,
        data: Box::into_raw(data.into_boxed_slice()) as *mut u8,
    });
    Box::into_raw(boxed) as i64
}

/// `tcp_close(fd: i64) -> void`
#[no_mangle]
pub extern "C" fn kryos_socket_close_ks(fd: i64) -> i64 {
    kryos_socket_close(fd) as i64
}
