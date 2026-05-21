//! Unix domain socket support for the Kryos native stdlib.
//!
//! These functions are only available on Unix-like platforms. On Windows they
//! are exposed as stubs that return -1.
//!
//! Handles use a dedicated descriptor table (uds_*).

#![allow(dead_code)]

#[cfg(unix)]
mod imp {
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::{Arc, Mutex};

    static UDS_TABLE: Mutex<Option<UdsTable>> = Mutex::new(None);

    enum UdsEntry {
        Stream(UnixStream),
        Listener(Arc<UnixListener>),
    }

    struct UdsTable {
        map: HashMap<i64, UdsEntry>,
        next_fd: i64,
    }

    impl UdsTable {
        fn new() -> Self {
            Self { map: HashMap::new(), next_fd: 5000 }
        }
        fn insert(&mut self, e: UdsEntry) -> i64 {
            let fd = self.next_fd;
            self.next_fd += 1;
            self.map.insert(fd, e);
            fd
        }
    }

    fn with_table<F, R>(f: F) -> R
    where
        F: FnOnce(&mut UdsTable) -> R,
    {
        let mut guard = UDS_TABLE.lock().unwrap_or_else(|e| e.into_inner());
        let table = guard.get_or_insert_with(UdsTable::new);
        f(table)
    }

    fn path_from_raw(ptr: *const u8, len: usize) -> Option<String> {
        if ptr.is_null() {
            return None;
        }
        let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
        std::str::from_utf8(slice).ok().map(str::to_owned)
    }

    #[no_mangle]
    pub extern "C" fn kryos_uds_connect(path_ptr: *const u8, path_len: usize) -> i64 {
        let Some(path) = path_from_raw(path_ptr, path_len) else { return -1 };
        match UnixStream::connect(&path) {
            Ok(s) => with_table(|t| t.insert(UdsEntry::Stream(s))),
            Err(_) => -1,
        }
    }

    #[no_mangle]
    pub extern "C" fn kryos_uds_bind(path_ptr: *const u8, path_len: usize) -> i64 {
        let Some(path) = path_from_raw(path_ptr, path_len) else { return -1 };
        // Best-effort: remove a stale socket file at this path.
        let _ = std::fs::remove_file(&path);
        match UnixListener::bind(&path) {
            Ok(l) => with_table(|t| t.insert(UdsEntry::Listener(Arc::new(l)))),
            Err(_) => -1,
        }
    }

    #[no_mangle]
    pub extern "C" fn kryos_uds_accept(listener_fd: i64) -> i64 {
        let listener = with_table(|t| match t.map.get(&listener_fd) {
            Some(UdsEntry::Listener(l)) => Some(l.clone()),
            _ => None,
        });
        let Some(listener) = listener else { return -1 };
        match listener.accept() {
            Ok((s, _addr)) => with_table(|t| t.insert(UdsEntry::Stream(s))),
            Err(_) => -1,
        }
    }

    #[no_mangle]
    pub extern "C" fn kryos_uds_send(fd: i64, data_ptr: *const u8, data_len: usize) -> i64 {
        if data_ptr.is_null() {
            return -1;
        }
        let buf = unsafe { std::slice::from_raw_parts(data_ptr, data_len) };
        let mut stream = with_table(|t| match t.map.get(&fd) {
            Some(UdsEntry::Stream(s)) => s.try_clone().ok(),
            _ => None,
        });
        match stream.as_mut() {
            Some(s) => match s.write_all(buf) {
                Ok(_) => buf.len() as i64,
                Err(_) => -1,
            },
            None => -1,
        }
    }

    /// Reads up to `cap` bytes into `out_ptr`. Returns bytes read, 0 on EOF, -1 on error.
    #[no_mangle]
    pub extern "C" fn kryos_uds_recv(fd: i64, out_ptr: *mut u8, cap: usize) -> i64 {
        if out_ptr.is_null() || cap == 0 {
            return -1;
        }
        let mut stream = with_table(|t| match t.map.get(&fd) {
            Some(UdsEntry::Stream(s)) => s.try_clone().ok(),
            _ => None,
        });
        let buf = unsafe { std::slice::from_raw_parts_mut(out_ptr, cap) };
        match stream.as_mut() {
            Some(s) => match s.read(buf) {
                Ok(n) => n as i64,
                Err(_) => -1,
            },
            None => -1,
        }
    }

    #[no_mangle]
    pub extern "C" fn kryos_uds_close(fd: i64) -> i64 {
        with_table(|t| {
            t.map.remove(&fd);
        });
        0
    }
}

#[cfg(unix)]
pub use imp::*;

// ---- KryosString-handle wrappers (the `_ks` ABI used by codegen) -------------

// Layout MUST match kryos_rt::string::KryosString (32 bytes; ref_count
// added step 37). Local duplicate; consider unifying via shared crate.
#[repr(C)]
struct KryosString { len: i64, cap: i64, data: *mut u8, ref_count: i64 }

unsafe fn handle_to_bytes(handle: i64) -> (*const u8, usize) {
    if handle == 0 {
        return (std::ptr::null(), 0);
    }
    let s = &*(handle as *const KryosString);
    (s.data as *const u8, s.len.max(0) as usize)
}

fn vec_to_handle(v: Vec<u8>) -> i64 {
    let len = v.len() as i64;
    let cap = v.capacity() as i64;
    let boxed = Box::new(KryosString {
        len,
        cap,
        data: Box::into_raw(v.into_boxed_slice()) as *mut u8,
        ref_count: 1,
    });
    Box::into_raw(boxed) as i64
}

#[no_mangle]
pub extern "C" fn kryos_uds_connect_ks(path_handle: i64) -> i64 {
    let (ptr, len) = unsafe { handle_to_bytes(path_handle) };
    kryos_uds_connect(ptr, len)
}

#[no_mangle]
pub extern "C" fn kryos_uds_bind_ks(path_handle: i64) -> i64 {
    let (ptr, len) = unsafe { handle_to_bytes(path_handle) };
    kryos_uds_bind(ptr, len)
}

#[no_mangle]
pub extern "C" fn kryos_uds_send_ks(fd: i64, data_handle: i64) -> i64 {
    let (ptr, len) = unsafe { handle_to_bytes(data_handle) };
    kryos_uds_send(fd, ptr, len)
}

#[no_mangle]
pub extern "C" fn kryos_uds_recv_ks(fd: i64, max_bytes: i64) -> i64 {
    let cap = if max_bytes <= 0 { 4096 } else { max_bytes as usize };
    let mut buf = vec![0u8; cap];
    let n = kryos_uds_recv(fd, buf.as_mut_ptr(), cap);
    let n = n.max(0) as usize;
    buf.truncate(n);
    vec_to_handle(buf)
}

#[cfg(not(unix))]
mod stub {
    #[no_mangle]
    pub extern "C" fn kryos_uds_connect(_p: *const u8, _l: usize) -> i64 { -1 }
    #[no_mangle]
    pub extern "C" fn kryos_uds_bind(_p: *const u8, _l: usize) -> i64 { -1 }
    #[no_mangle]
    pub extern "C" fn kryos_uds_accept(_fd: i64) -> i64 { -1 }
    #[no_mangle]
    pub extern "C" fn kryos_uds_send(_fd: i64, _p: *const u8, _l: usize) -> i64 { -1 }
    #[no_mangle]
    pub extern "C" fn kryos_uds_recv(_fd: i64, _p: *mut u8, _c: usize) -> i64 { -1 }
    #[no_mangle]
    pub extern "C" fn kryos_uds_close(_fd: i64) -> i64 { 0 }
}

#[cfg(not(unix))]
pub use stub::*;
