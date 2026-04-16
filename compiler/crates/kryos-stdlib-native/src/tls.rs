//! TLS (HTTPS) support for the Kryos native stdlib.
//!
//! Uses rustls with webpki-roots CA certificates.  TLS streams are stored in a
//! global table keyed by i64 handles, identical to the pattern in net.rs.

#![cfg(feature = "tls")]

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex, OnceLock};
use std::sync::atomic::{AtomicI64, Ordering};

static TLS_COUNTER: AtomicI64 = AtomicI64::new(5000);

type TlsStream = StreamOwned<ClientConnection, TcpStream>;

fn tls_table() -> &'static Mutex<HashMap<i64, TlsStream>> {
    static TABLE: OnceLock<Mutex<HashMap<i64, TlsStream>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn make_tls_config() -> Arc<ClientConfig> {
    let root_store = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.into(),
    };
    Arc::new(
        ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth(),
    )
}

unsafe fn ptr_to_str<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).ok()
}

/// Establish a TLS connection to `host:port`.
/// Returns a positive handle on success, -1 on failure.
#[no_mangle]
pub unsafe extern "C" fn kryos_tls_connect(
    host_ptr: *const u8,
    host_len: usize,
    port: u16,
) -> i64 {
    let host = match ptr_to_str(host_ptr, host_len) {
        Some(s) => s,
        None => return -1,
    };

    let server_name = match ServerName::try_from(host.to_string()) {
        Ok(n) => n,
        Err(_) => return -1,
    };

    let config = make_tls_config();
    let conn = match ClientConnection::new(config, server_name) {
        Ok(c) => c,
        Err(_) => return -1,
    };

    let tcp = match TcpStream::connect(format!("{host}:{port}")) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let stream = StreamOwned::new(conn, tcp);
    let id = TLS_COUNTER.fetch_add(1, Ordering::Relaxed);
    tls_table().lock().unwrap().insert(id, stream);
    id
}

/// Send data over a TLS stream.
/// Returns bytes written, or -1 on error.
#[no_mangle]
pub unsafe extern "C" fn kryos_tls_send(fd: i64, data: *const u8, len: usize) -> i64 {
    if data.is_null() || len == 0 {
        return -1;
    }
    let slice = std::slice::from_raw_parts(data, len);
    let mut guard = tls_table().lock().unwrap();
    match guard.get_mut(&fd) {
        Some(stream) => match stream.write(slice) {
            Ok(n) => n as i64,
            Err(_) => -1,
        },
        None => -1,
    }
}

/// Receive data from a TLS stream.
/// Returns bytes read, 0 on EOF, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn kryos_tls_recv(fd: i64, buf: *mut u8, buf_len: usize) -> i64 {
    if buf.is_null() || buf_len == 0 {
        return -1;
    }
    let slice = std::slice::from_raw_parts_mut(buf, buf_len);
    let mut guard = tls_table().lock().unwrap();
    match guard.get_mut(&fd) {
        Some(stream) => match stream.read(slice) {
            Ok(n) => n as i64,
            Err(_) => -1,
        },
        None => -1,
    }
}

/// Flush and close a TLS stream.
/// Returns 0 on success, -1 if handle unknown.
#[no_mangle]
pub unsafe extern "C" fn kryos_tls_close(fd: i64) -> i32 {
    if tls_table().lock().unwrap().remove(&fd).is_some() {
        0
    } else {
        -1
    }
}
