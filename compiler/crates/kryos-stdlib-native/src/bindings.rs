//! Kryos-string-handle (`_ks`) wrappers for stdlib-native functions.
//!
//! These wrap the low-level FFI functions (which take `*const u8, usize` byte
//! pointers, or write into out-buffers) and expose them as functions whose
//! arguments and return values are i64 handles to `KryosString` and other
//! Kryos types. This is the calling convention used by Kryos builtins.
//!
//! All public functions in this module are `#[no_mangle] pub extern "C"`.

use kryos_rt::string::KryosString;
use std::net::ToSocketAddrs;

// Re-export FFI helpers from other modules.
use crate::re;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Decode a KryosString handle into (ptr, len). Returns (null, 0) for handle 0.
unsafe fn handle_to_bytes(handle: i64) -> (*const u8, usize) {
    if handle == 0 {
        return (std::ptr::null(), 0);
    }
    let s = handle as *const KryosString;
    ((*s).data as *const u8, (*s).len as usize)
}

/// Decode a KryosString handle into a `&str`. Returns empty string on null/invalid UTF-8.
unsafe fn handle_to_str<'a>(handle: i64) -> &'a str {
    let (ptr, len) = handle_to_bytes(handle);
    if ptr.is_null() || len == 0 {
        return "";
    }
    let slice = std::slice::from_raw_parts(ptr, len);
    std::str::from_utf8(slice).unwrap_or("")
}

/// Build a new KryosString handle from a byte slice. Returns 0 on null/alloc failure.
fn bytes_to_handle(bytes: &[u8]) -> i64 {
    unsafe {
        let p = kryos_rt::string::kryos_string_new(bytes.as_ptr(), bytes.len() as i64);
        if p.is_null() {
            0
        } else {
            p as i64
        }
    }
}

fn str_to_handle(s: &str) -> i64 {
    bytes_to_handle(s.as_bytes())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

// ---------------------------------------------------------------------------
// Crypto: SHA-256 / SHA-512 / random_bytes — handle-based wrappers
// ---------------------------------------------------------------------------

/// `sha256(s: str) -> str` — hex-encoded SHA-256 digest of the input string.
#[cfg(feature = "crypto")]
#[no_mangle]
pub extern "C" fn kryos_sha256_ks(input_handle: i64) -> i64 {
    let s = unsafe { handle_to_str(input_handle) };
    let mut out = [0u8; 32];
    let rc = crate::crypto::kryos_sha256(s.as_ptr(), s.len(), out.as_mut_ptr());
    if rc != 0 {
        return str_to_handle("");
    }
    str_to_handle(&hex_encode(&out))
}

/// `sha512(s: str) -> str` — hex-encoded SHA-512 digest.
#[cfg(feature = "crypto")]
#[no_mangle]
pub extern "C" fn kryos_sha512_ks(input_handle: i64) -> i64 {
    let s = unsafe { handle_to_str(input_handle) };
    let mut out = [0u8; 64];
    let rc = crate::crypto::kryos_sha512(s.as_ptr(), s.len(), out.as_mut_ptr());
    if rc != 0 {
        return str_to_handle("");
    }
    str_to_handle(&hex_encode(&out))
}

/// `random_bytes(n: i64) -> str` — n cryptographically-secure random bytes, hex-encoded.
#[cfg(feature = "crypto")]
#[no_mangle]
pub extern "C" fn kryos_random_bytes_ks(n: i64) -> i64 {
    if n <= 0 {
        return str_to_handle("");
    }
    let n = n.min(4096) as usize; // bound for safety
    let mut buf = vec![0u8; n];
    let rc = crate::crypto::kryos_random_bytes(buf.as_mut_ptr(), n);
    if rc != 0 {
        return str_to_handle("");
    }
    str_to_handle(&hex_encode(&buf))
}

// ---------------------------------------------------------------------------
// Regex: handle-based wrappers + helpers
// ---------------------------------------------------------------------------

/// `regex_new(pattern: str) -> i64` — opaque handle (regex pointer cast to i64).
/// Returns 0 if the pattern is invalid.
#[no_mangle]
pub extern "C" fn kryos_regex_new_ks(pattern_handle: i64) -> i64 {
    let s = unsafe { handle_to_str(pattern_handle) };
    let ptr = re::kryos_regex_new(s.as_ptr(), s.len()) as i64;
    ptr
}

/// `regex_match(re: i64, text: str) -> bool` (returns 0/1).
#[no_mangle]
pub extern "C" fn kryos_regex_is_match_ks(re_handle: i64, text_handle: i64) -> i64 {
    if re_handle == 0 {
        return 0;
    }
    let s = unsafe { handle_to_str(text_handle) };
    let rc = re::kryos_regex_is_match(re_handle as *mut u8, s.as_ptr(), s.len());
    if rc == 1 {
        1
    } else {
        0
    }
}

/// `regex_find(re: i64, text: str) -> str` — first match, empty string if none.
#[no_mangle]
pub extern "C" fn kryos_regex_find_ks(re_handle: i64, text_handle: i64) -> i64 {
    if re_handle == 0 {
        return str_to_handle("");
    }
    let s = unsafe { handle_to_str(text_handle) };
    let re = unsafe { &*(re_handle as *const regex::Regex) };
    match re.find(s) {
        Some(m) => str_to_handle(m.as_str()),
        None => str_to_handle(""),
    }
}

/// `regex_replace_all(re: i64, text: str, replacement: str) -> str`.
#[no_mangle]
pub extern "C" fn kryos_regex_replace_all_ks(
    re_handle: i64,
    text_handle: i64,
    repl_handle: i64,
) -> i64 {
    if re_handle == 0 {
        let s = unsafe { handle_to_str(text_handle) };
        return str_to_handle(s);
    }
    let text = unsafe { handle_to_str(text_handle) };
    let repl = unsafe { handle_to_str(repl_handle) };
    let re = unsafe { &*(re_handle as *const regex::Regex) };
    let result = re.replace_all(text, repl);
    str_to_handle(&result)
}

/// `regex_drop(re: i64)` — frees the compiled regex.
#[no_mangle]
pub extern "C" fn kryos_regex_drop_ks(re_handle: i64) {
    if re_handle == 0 {
        return;
    }
    re::kryos_regex_drop(re_handle as *mut u8);
}

// ---------------------------------------------------------------------------
// HTTP / HTTPS request — universal client built on stdlib + (optionally) rustls
// ---------------------------------------------------------------------------

/// Parse a URL into (scheme, host, port, path). Defaults to http://.
fn parse_url(url: &str) -> (String, String, u16, String) {
    let (scheme, rest) = if let Some(rest) = url.strip_prefix("https://") {
        ("https", rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        ("http", rest)
    } else {
        ("http", url)
    };
    let (host_port, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = if let Some(i) = host_port.rfind(':') {
        let p: u16 = host_port[i + 1..].parse().unwrap_or(0);
        (&host_port[..i], p)
    } else {
        (host_port, 0u16)
    };
    let port = if port == 0 {
        if scheme == "https" {
            443
        } else {
            80
        }
    } else {
        port
    };
    (
        scheme.to_string(),
        host.to_string(),
        port,
        path.to_string(),
    )
}

fn split_status_headers_body(buf: &[u8]) -> (i64, String, Vec<u8>) {
    // Find end-of-headers.
    let hdr_end = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .unwrap_or(buf.len());
    let head = std::str::from_utf8(&buf[..hdr_end.saturating_sub(4).min(hdr_end)]).unwrap_or("");
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    // "HTTP/1.x NNN msg" -> parse NNN.
    let status: i64 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut headers = String::new();
    for line in lines {
        if !line.is_empty() {
            headers.push_str(line);
            headers.push('\n');
        }
    }
    let body = if hdr_end <= buf.len() {
        buf[hdr_end..].to_vec()
    } else {
        Vec::new()
    };
    (status, headers, body)
}

fn build_request(method: &str, host: &str, path: &str, headers: &str, body: &str) -> Vec<u8> {
    let mut req = String::new();
    req.push_str(method);
    req.push(' ');
    req.push_str(path);
    req.push_str(" HTTP/1.0\r\n");
    req.push_str("Host: ");
    req.push_str(host);
    req.push_str("\r\n");
    req.push_str("Connection: close\r\n");
    if !body.is_empty() {
        req.push_str("Content-Length: ");
        req.push_str(&body.len().to_string());
        req.push_str("\r\n");
    }
    // headers is a newline-delimited list of "Key: Value"
    for line in headers.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        req.push_str(line);
        req.push_str("\r\n");
    }
    req.push_str("\r\n");
    let mut out = req.into_bytes();
    out.extend_from_slice(body.as_bytes());
    out
}

/// Perform an HTTP/HTTPS request. Returns a JSON node handle for an object:
///   { "status": int, "headers": str, "body": str }
/// Returns a JSON null node on transport failure.
///
/// Signature in Kryos:
///   `http_request(method: str, url: str, headers: str, body: str, timeout_ms: i64) -> i64`
#[no_mangle]
pub extern "C" fn kryos_http_request_ks(
    method_handle: i64,
    url_handle: i64,
    headers_handle: i64,
    body_handle: i64,
    timeout_ms: i64,
) -> i64 {
    let method = unsafe { handle_to_str(method_handle) };
    let url = unsafe { handle_to_str(url_handle) };
    let headers = unsafe { handle_to_str(headers_handle) };
    let body = unsafe { handle_to_str(body_handle) };
    let timeout = if timeout_ms <= 0 {
        std::time::Duration::from_secs(15)
    } else {
        std::time::Duration::from_millis(timeout_ms as u64)
    };

    let (scheme, host, port, path) = parse_url(url);

    let request = build_request(
        if method.is_empty() { "GET" } else { method },
        &host,
        &path,
        headers,
        body,
    );

    let result_buf: Option<Vec<u8>> = if scheme == "https" {
        #[cfg(feature = "tls")]
        {
            do_https_request(&host, port, &request, timeout)
        }
        #[cfg(not(feature = "tls"))]
        {
            None
        }
    } else {
        do_http_request(&host, port, &request, timeout)
    };

    let buf = match result_buf {
        Some(b) => b,
        None => return crate::json::kryos_json_null(),
    };

    let (status, hdrs, body_bytes) = split_status_headers_body(&buf);
    let body_str = String::from_utf8_lossy(&body_bytes).into_owned();

    // Build JSON: { status: int, headers: str, body: str }
    let status_node = crate::json::kryos_json_number(status as f64);
    let headers_node = crate::json::kryos_json_string(str_to_handle(&hdrs));
    let body_node = crate::json::kryos_json_string(str_to_handle(&body_str));

    let keys_handle = build_str_array(&["status", "headers", "body"]);
    let vals_handle = build_i64_array(&[status_node, headers_node, body_node]);
    crate::json::kryos_json_object(keys_handle, vals_handle)
}

/// `https_get(url: str) -> str` — body only, empty on failure. Convenience.
#[no_mangle]
pub extern "C" fn kryos_https_get_ks(url_handle: i64) -> i64 {
    let url = unsafe { handle_to_str(url_handle) };
    let m = str_to_handle("GET");
    let u = str_to_handle(url);
    let h = str_to_handle("");
    let b = str_to_handle("");
    let resp = kryos_http_request_ks(m, u, h, b, 15_000);
    if resp <= 0 {
        return str_to_handle("");
    }
    let body_key = str_to_handle("body");
    let body_node = crate::json::kryos_json_get(resp, body_key);
    if body_node <= 0 {
        return str_to_handle("");
    }
    crate::json::kryos_json_to_str(body_node)
}

fn do_http_request(
    host: &str,
    port: u16,
    request: &[u8],
    timeout: std::time::Duration,
) -> Option<Vec<u8>> {
    use std::io::{Read, Write};
    let addr = format!("{host}:{port}");
    let sock_addr = addr.to_socket_addrs().ok()?.next()?;
    let mut stream = std::net::TcpStream::connect_timeout(&sock_addr, timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok();
    stream.set_write_timeout(Some(timeout)).ok();
    stream.write_all(request).ok()?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).ok()?;
    Some(buf)
}

#[cfg(feature = "tls")]
fn ensure_crypto_provider() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // Install ring as the default provider; ignore the error if a provider
        // is already installed (e.g. by another component in the same process).
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

#[cfg(feature = "tls")]
fn do_https_request(
    host: &str,
    port: u16,
    request: &[u8],
    timeout: std::time::Duration,
) -> Option<Vec<u8>> {
    use rustls::pki_types::ServerName;
    use rustls::{ClientConfig, ClientConnection, StreamOwned};
    use std::io::{Read, Write};
    use std::sync::Arc;

    ensure_crypto_provider();

    let root_store = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.into(),
    };
    let cfg = Arc::new(
        ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth(),
    );
    let server_name: ServerName<'_> = host.to_string().try_into().ok()?;
    let addr = format!("{host}:{port}");
    let sock_addr = addr.to_socket_addrs().ok()?.next()?;
    let tcp = std::net::TcpStream::connect_timeout(&sock_addr, timeout).ok()?;
    tcp.set_read_timeout(Some(timeout)).ok();
    tcp.set_write_timeout(Some(timeout)).ok();
    let conn = ClientConnection::new(cfg, server_name).ok()?;
    let mut tls = StreamOwned::new(conn, tcp);
    tls.write_all(request).ok()?;
    let mut buf = Vec::new();
    tls.read_to_end(&mut buf).ok()?;
    Some(buf)
}

// ---------------------------------------------------------------------------
// Helpers: build KryosArray<str> / KryosArray<i64> from Rust slices.
//
// These wrap kryos_rt::array primitives. We deliberately don't depend on the
// exact layout — instead we reuse json.rs's array builders by going through
// json_array (which takes a KryosArray<i64> handle). For string keys we need a
// KryosArray<str> built via array push.
// ---------------------------------------------------------------------------

fn build_str_array(strs: &[&str]) -> i64 {
    unsafe {
        let arr = kryos_rt::array::kryos_array_new(8, strs.len().max(4) as i64);
        for &s in strs {
            let h = str_to_handle(s);
            kryos_rt::array::kryos_array_push(arr, h);
        }
        arr as i64
    }
}

fn build_i64_array(items: &[i64]) -> i64 {
    unsafe {
        let arr = kryos_rt::array::kryos_array_new(8, items.len().max(4) as i64);
        for &v in items {
            kryos_rt::array::kryos_array_push(arr, v);
        }
        arr as i64
    }
}
