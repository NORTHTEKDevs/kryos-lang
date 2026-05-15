//! TLS (HTTPS) support for the Kryos native stdlib.
//!
//! Uses rustls with webpki-roots CA certificates.  TLS streams are stored in a
//! global table keyed by i64 handles, identical to the pattern in net.rs.
//!
//! Supports both client-side (TLS connect) and server-side (TLS accept) streams.

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, ServerConfig, ServerConnection, StreamOwned};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

static TLS_COUNTER: AtomicI64 = AtomicI64::new(5000);
static TLS_SERVER_CFG_COUNTER: AtomicI64 = AtomicI64::new(7000);

// ---------------------------------------------------------------------------
// TLS stream enum — holds either a client or a server stream
// ---------------------------------------------------------------------------

enum TlsStream {
    Client(StreamOwned<ClientConnection, TcpStream>),
    Server(StreamOwned<ServerConnection, TcpStream>),
}

impl TlsStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            TlsStream::Client(s) => s.write(buf),
            TlsStream::Server(s) => s.write(buf),
        }
    }

    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            TlsStream::Client(s) => s.read(buf),
            TlsStream::Server(s) => s.read(buf),
        }
    }
}

fn tls_table() -> &'static Mutex<HashMap<i64, TlsStream>> {
    static TABLE: OnceLock<Mutex<HashMap<i64, TlsStream>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

// ---------------------------------------------------------------------------
// Server config table — stores Arc<ServerConfig> keyed by i64 handles
// ---------------------------------------------------------------------------

fn tls_server_config_table() -> &'static Mutex<HashMap<i64, Arc<ServerConfig>>> {
    static TABLE: OnceLock<Mutex<HashMap<i64, Arc<ServerConfig>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

// ---------------------------------------------------------------------------
// Crypto provider initialization (rustls 0.23 requires explicit provider)
// ---------------------------------------------------------------------------

fn ensure_crypto_provider() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

// ---------------------------------------------------------------------------
// Client TLS helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Client-side: tls_connect
// ---------------------------------------------------------------------------

/// Establish a TLS connection to `host:port`.
/// Returns a positive handle on success, -1 on failure.
#[no_mangle]
pub unsafe extern "C" fn kryos_tls_connect(host_ptr: *const u8, host_len: usize, port: u16) -> i64 {
    ensure_crypto_provider();

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
    tls_table().lock().unwrap().insert(id, TlsStream::Client(stream));
    id
}

// ---------------------------------------------------------------------------
// Shared send / recv / close (work for both client and server streams)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Server-side: tls_server_config
// ---------------------------------------------------------------------------

/// Load a TLS server configuration from PEM cert chain + private key files.
/// Returns a positive config handle on success, -1 on failure.
///
/// Raw pointer variant (used by the _ks wrapper below).
#[no_mangle]
pub unsafe extern "C" fn kryos_tls_server_config(
    cert_path_ptr: *const u8,
    cert_path_len: usize,
    key_path_ptr: *const u8,
    key_path_len: usize,
) -> i64 {
    ensure_crypto_provider();

    let cert_path = match ptr_to_str(cert_path_ptr, cert_path_len) {
        Some(s) => s,
        None => return -1,
    };
    let key_path = match ptr_to_str(key_path_ptr, key_path_len) {
        Some(s) => s,
        None => return -1,
    };

    // Read cert file
    let cert_bytes = match std::fs::read(cert_path) {
        Ok(b) => b,
        Err(_) => return -1,
    };
    // Read key file
    let key_bytes = match std::fs::read(key_path) {
        Ok(b) => b,
        Err(_) => return -1,
    };

    // Parse certificates
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> = {
        let mut cursor = std::io::Cursor::new(&cert_bytes);
        match rustls_pemfile::certs(&mut cursor).collect::<Result<Vec<_>, _>>() {
            Ok(c) => c,
            Err(_) => return -1,
        }
    };

    if certs.is_empty() {
        return -1;
    }

    // Parse private key
    let key: rustls::pki_types::PrivateKeyDer<'static> = {
        let mut cursor = std::io::Cursor::new(&key_bytes);
        match rustls_pemfile::private_key(&mut cursor) {
            Ok(Some(k)) => k,
            _ => return -1,
        }
    };

    // Build ServerConfig
    let config = match ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
    {
        Ok(c) => Arc::new(c),
        Err(_) => return -1,
    };

    let id = TLS_SERVER_CFG_COUNTER.fetch_add(1, Ordering::Relaxed);
    tls_server_config_table().lock().unwrap().insert(id, config);
    id
}

// ---------------------------------------------------------------------------
// Kryos-string-handle wrappers for tls_send / tls_recv (used by JIT builtins)
// ---------------------------------------------------------------------------

/// Kryos-string-handle wrapper for `tls_server_config`.
/// Takes string handles for cert_path and key_path.
#[repr(C)]
struct KryosString {
    len: i64,
    cap: i64,
    data: *mut u8,
}

unsafe fn handle_to_bytes_tls(handle: i64) -> (*const u8, usize) {
    if handle == 0 {
        return (std::ptr::null(), 0);
    }
    let s = handle as *const KryosString;
    ((*s).data as *const u8, (*s).len as usize)
}

#[no_mangle]
pub unsafe extern "C" fn kryos_tls_server_config_ks(cert_handle: i64, key_handle: i64) -> i64 {
    let (cert_ptr, cert_len) = handle_to_bytes_tls(cert_handle);
    let (key_ptr, key_len) = handle_to_bytes_tls(key_handle);
    kryos_tls_server_config(cert_ptr, cert_len, key_ptr, key_len)
}

// ---------------------------------------------------------------------------
// Server-side: tls_accept
// ---------------------------------------------------------------------------

/// Wrap an accepted TCP stream in a TLS ServerConnection using the given config.
/// `client_fd` is a Kryos TCP fd (from tcp_accept / tcp_try_accept) that gets
/// *removed* from the SOCKET_TABLE and promoted into a server-side TLS stream.
///
/// Returns a TLS handle (compatible with tls_send / tls_recv / tls_close), or -1.
#[no_mangle]
pub unsafe extern "C" fn kryos_tls_accept(client_fd: i64, config_handle: i64) -> i64 {
    // Look up the ServerConfig
    let config = match tls_server_config_table().lock().unwrap().get(&config_handle).cloned() {
        Some(c) => c,
        None => return -1,
    };

    // Take the TcpStream out of the net module's SOCKET_TABLE
    let tcp_stream = match crate::net::take_tcp_stream(client_fd) {
        Some(s) => s,
        None => return -1,
    };

    // Set to blocking mode for the TLS handshake
    let _ = tcp_stream.set_nonblocking(false);

    // Create a ServerConnection
    let conn = match ServerConnection::new(config) {
        Ok(c) => c,
        Err(_) => return -1,
    };

    // Wrap in StreamOwned and complete the handshake via flush()
    let mut stream = StreamOwned::new(conn, tcp_stream);
    // Drive the handshake by doing a flush (which processes pending TLS records)
    let _ = stream.flush();

    let id = TLS_COUNTER.fetch_add(1, Ordering::Relaxed);
    tls_table().lock().unwrap().insert(id, TlsStream::Server(stream));
    id
}

// ---------------------------------------------------------------------------
// Kryos-string-handle wrappers for tls_send / tls_recv
// These allow tls_send and tls_recv to be used as compiler builtins.
// ---------------------------------------------------------------------------

/// `tls_send(fd: i64, data: str) -> i64` — KS wrapper.
#[no_mangle]
pub unsafe extern "C" fn kryos_tls_send_ks(fd: i64, data_handle: i64) -> i64 {
    let (ptr, len) = handle_to_bytes_tls(data_handle);
    if ptr.is_null() {
        return 0;
    }
    kryos_tls_send(fd, ptr, len)
}

/// `tls_recv(fd: i64, max_bytes: i64) -> str` — KS wrapper. Returns a KryosString handle.
#[no_mangle]
pub unsafe extern "C" fn kryos_tls_recv_ks(fd: i64, max_bytes: i64) -> i64 {
    let buf_len = if max_bytes <= 0 { 4096 } else { max_bytes as usize };
    let mut buf = vec![0u8; buf_len];
    let n = kryos_tls_recv(fd, buf.as_mut_ptr(), buf_len);
    let n = n.max(0) as usize;
    let data = buf[..n].to_vec();
    let boxed = Box::new(KryosString {
        len: n as i64,
        cap: n as i64,
        data: Box::into_raw(data.into_boxed_slice()) as *mut u8,
    });
    Box::into_raw(boxed) as i64
}

/// `tls_close(fd: i64) -> i64` — KS wrapper.
#[no_mangle]
pub unsafe extern "C" fn kryos_tls_close_ks(fd: i64) -> i64 {
    kryos_tls_close(fd) as i64
}
