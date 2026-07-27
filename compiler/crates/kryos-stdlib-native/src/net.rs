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

/// Returns a clone of the TCP stream for the given fd, if it exists.
///
/// Used by adjacent modules (e.g. `websocket`) that need to do I/O on a fd
/// the user has already established via `tcp_accept`.
pub(crate) fn with_tcp_stream(fd: i64) -> Option<TcpStream> {
    with_socket_table(|t| match t.map.get(&fd) {
        Some(SocketEntry::Stream(s)) => s.try_clone().ok(),
        _ => None,
    })
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
    // On a coop task, connect off the baton so sibling async tasks run while
    // the (slow) handshake blocks.
    match kryos_rt::executor::io_offload(|| TcpStream::connect(&addr)) {
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

    // Phase 2: block on accept() WITHOUT holding the mutex (off the coop baton
    // so other async tasks run while we wait for a connection).
    match kryos_rt::executor::io_offload(|| listener_arc.accept()) {
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
///
/// The global socket mutex is released before calling the blocking `write()`
/// so that other socket operations (accept/recv/close on other fds) can
/// proceed concurrently.
#[no_mangle]
pub extern "C" fn kryos_tcp_send(fd: i64, data: *const u8, len: usize) -> i64 {
    if data.is_null() || len == 0 {
        return -1;
    }
    let slice = unsafe { std::slice::from_raw_parts(data, len) };

    // Phase 1: hold the lock only long enough to clone the stream handle.
    let mut stream = match with_socket_table(|table| match table.map.get(&fd) {
        Some(SocketEntry::Stream(s)) => s.try_clone().ok(),
        _ => None,
    }) {
        Some(s) => s,
        None => return -1,
    };

    // Phase 2: blocking I/O WITHOUT holding the mutex (off the coop baton).
    match kryos_rt::executor::io_offload(|| stream.write(slice)) {
        Ok(n) => n as i64,
        Err(_) => -1,
    }
}

/// Receives data from a TCP stream.
///
/// Returns the number of bytes read, 0 on EOF, or -1 on error.
///
/// The global socket mutex is released before calling the blocking `read()`
/// so that other socket operations (accept/send/close on other fds) can
/// proceed concurrently. This is critical for servers that handle multiple
/// connections from spawned threads.
#[no_mangle]
pub extern "C" fn kryos_tcp_recv(fd: i64, buf: *mut u8, buf_len: usize) -> i64 {
    if buf.is_null() || buf_len == 0 {
        return -1;
    }
    let slice = unsafe { std::slice::from_raw_parts_mut(buf, buf_len) };

    // Phase 1: hold the lock only long enough to clone the stream handle.
    let mut stream = match with_socket_table(|table| match table.map.get(&fd) {
        Some(SocketEntry::Stream(s)) => s.try_clone().ok(),
        _ => None,
    }) {
        Some(s) => s,
        None => return -1,
    };

    // Phase 2: blocking I/O WITHOUT holding the mutex (off the coop baton).
    match kryos_rt::executor::io_offload(|| stream.read(slice)) {
        Ok(n) => n as i64,
        Err(_) => -1,
    }
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

// Layout MUST match kryos_rt::string::KryosString (32 bytes; ref_count
// added step 37). Local duplicate; consider unifying via shared crate.
#[repr(C)]
struct KryosString {
    len: i64,
    cap: i64,
    data: *mut u8,
    ref_count: i64,
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
    // Build the string with the RUNTIME's constructor, not a hand-rolled
    // Box. kryos_string_free deallocates the data buffer with
    // KryosString::layout(cap) -- size cap+1, for the null terminator -- but
    // a `Vec::into_boxed_slice` allocation is exactly `len` bytes, so every
    // received buffer was freed under a layout it was never allocated with.
    // Rust's allocator contract makes that undefined, and in practice the
    // block was not returned: a TCP server leaked one receive buffer per
    // request (isolated to ~250 bytes/request over 60k requests; the same
    // server with the recv removed was perfectly flat).
    unsafe { kryos_rt::string::kryos_string_new(buf.as_ptr(), n as i64) as i64 }
}

/// `tcp_close(fd: i64) -> void`
#[no_mangle]
pub extern "C" fn kryos_socket_close_ks(fd: i64) -> i64 {
    kryos_socket_close(fd) as i64
}

// =============================================================================
// Async / non-blocking primitives (Gap 3 minimum viable async)
// =============================================================================
//
// Kryos does NOT yet have async/await syntax. What we expose instead is a set
// of non-blocking I/O primitives + a sleep helper that, combined with a Kryos
// while-loop, lets you write a single-threaded event loop:
//
//   let listener = tcp_listen("0.0.0.0", 8080)
//   tcp_set_nonblocking(listener, true)
//   while running {
//       let fd = tcp_try_accept(listener)
//       if fd > 0 {
//           handle_new_connection(fd)
//       }
//       sleep_ms(1)
//   }
//
// On top of this, `std.poll` provides a slightly higher-level interface.

/// Remove and return the underlying TcpStream for the given fd.
/// Used by the TLS module to promote a raw TCP fd to a TLS server stream.
/// Returns None if the fd is not found or is not a stream.
pub(crate) fn take_tcp_stream(fd: i64) -> Option<TcpStream> {
    with_socket_table(|table| match table.map.remove(&fd) {
        Some(SocketEntry::Stream(s)) => Some(s),
        Some(other) => {
            // Not a stream — put it back to avoid breaking the caller
            table.map.insert(fd, other);
            None
        }
        None => None,
    })
}

/// Set the non-blocking flag on a Kryos socket fd.
/// Returns 0 on success, -1 on failure.
#[no_mangle]
pub extern "C" fn kryos_tcp_set_nonblocking(fd: i64, nonblocking: i64) -> i64 {
    let nb = nonblocking != 0;
    with_socket_table(|table| {
        match table.map.get(&fd) {
            Some(SocketEntry::Stream(s)) => s.set_nonblocking(nb).map(|_| 0).unwrap_or(-1),
            Some(SocketEntry::Listener(l)) => l.set_nonblocking(nb).map(|_| 0).unwrap_or(-1),
            None => -1,
        }
    })
}

/// Non-blocking accept. Returns the new stream fd, 0 if no pending connection
/// (would-block), or -1 on error.
#[no_mangle]
pub extern "C" fn kryos_tcp_try_accept(listener_fd: i64) -> i64 {
    let listener_arc = with_socket_table(|table| match table.map.get(&listener_fd) {
        Some(SocketEntry::Listener(l)) => Some(Arc::clone(l)),
        _ => None,
    });
    let listener_arc = match listener_arc {
        Some(a) => a,
        None => return -1,
    };
    match listener_arc.accept() {
        Ok((stream, _addr)) => {
            // Inherit non-blocking from listener for convenience.
            let _ = stream.set_nonblocking(true);
            with_socket_table(|table| {
                let fd = table.next_fd;
                table.next_fd += 1;
                table.map.insert(fd, SocketEntry::Stream(stream));
                fd
            })
        }
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => 0,
        Err(_) => -1,
    }
}

/// Non-blocking recv. Returns:
///   > 0  : bytes actually read (data is in the returned string)
///     0  : EOF (peer closed) OR no data ready (would-block)
///   -1   : error
/// Use tcp_recv_status() to disambiguate 0.
///
/// For now we encode would-block as an empty string + a thread-local flag;
/// callers can simply retry on empty until they want to time out.
#[no_mangle]
pub extern "C" fn kryos_tcp_try_recv_ks(fd: i64, max_bytes: i64) -> i64 {
    let buf_len = if max_bytes <= 0 { 4096 } else { max_bytes as usize };
    let mut buf = vec![0u8; buf_len];
    let stream_opt = with_socket_table(|table| match table.map.get(&fd) {
        Some(SocketEntry::Stream(s)) => s.try_clone().ok(),
        _ => None,
    });
    let mut stream = match stream_opt {
        Some(s) => s,
        None => return empty_string_handle(),
    };
    // Force non-blocking just in case the caller didn't set it.
    let _ = stream.set_nonblocking(true);
    let n = match stream.read(&mut buf) {
        Ok(n) => n,
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => 0,
        Err(_) => 0,
    };
    // Build the string with the RUNTIME's constructor, not a hand-rolled
    // Box. kryos_string_free deallocates the data buffer with
    // KryosString::layout(cap) -- size cap+1, for the null terminator -- but
    // a `Vec::into_boxed_slice` allocation is exactly `len` bytes, so every
    // received buffer was freed under a layout it was never allocated with.
    // Rust's allocator contract makes that undefined, and in practice the
    // block was not returned: a TCP server leaked one receive buffer per
    // request (isolated to ~250 bytes/request over 60k requests; the same
    // server with the recv removed was perfectly flat).
    unsafe { kryos_rt::string::kryos_string_new(buf.as_ptr(), n as i64) as i64 }
}

fn empty_string_handle() -> i64 {
    // Same allocator-contract reason as the recv paths above: let the runtime
    // build the header and its buffer.
    unsafe { kryos_rt::string::kryos_string_new(std::ptr::null(), 0) as i64 }
}

// kryos_sleep_ms lives in kryos-rt::spawn; do not duplicate here.

/// Poll an array of fds for readability. Returns a bitmask (i64) where bit i
/// is set if fds[i] became readable within `timeout_ms`. Up to 63 fds.
///
/// This is a simple polling helper: it loops over the fds doing peek() on
/// each one and returns either as soon as any becomes readable, or after the
/// timeout elapses. Not as efficient as epoll/kqueue, but portable and good
/// enough for handfuls of fds.
#[no_mangle]
pub extern "C" fn kryos_poll_readable(fds_arr: *const i64, n_fds: i64, timeout_ms: i64) -> i64 {
    if fds_arr.is_null() || n_fds <= 0 || n_fds > 63 {
        return 0;
    }
    let n = n_fds as usize;
    let fds = unsafe { std::slice::from_raw_parts(fds_arr, n) };
    let start = std::time::Instant::now();
    let deadline = start + std::time::Duration::from_millis(timeout_ms.max(0) as u64);
    loop {
        let mut mask: i64 = 0;
        for (i, &fd) in fds.iter().enumerate() {
            let ready = with_socket_table(|table| match table.map.get(&fd) {
                Some(SocketEntry::Stream(s)) => {
                    if let Ok(clone) = s.try_clone() {
                        let _ = clone.set_nonblocking(true);
                        let mut peek = [0u8; 1];
                        // peek without consuming
                        match clone.peek(&mut peek) {
                            Ok(n) if n > 0 => true,
                            _ => false,
                        }
                    } else { false }
                }
                Some(SocketEntry::Listener(_)) => true, // listeners reported as ready; caller uses try_accept
                None => false,
            });
            if ready { mask |= 1i64 << i; }
        }
        if mask != 0 || std::time::Instant::now() >= deadline {
            return mask;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
