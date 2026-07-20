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

/// `regex_find_pos(re: i64, text: str, from: i64) -> i64` — start byte offset of the
/// next match in `text` at or after `from`, or -1 if no match. Out-of-range `from`
/// returns -1.
#[no_mangle]
pub extern "C" fn kryos_regex_find_pos_ks(re_handle: i64, text_handle: i64, from: i64) -> i64 {
    if re_handle == 0 {
        return -1;
    }
    let s = unsafe { handle_to_str(text_handle) };
    let start = if from < 0 { 0 } else { from as usize };
    if start > s.len() {
        return -1;
    }
    let re = unsafe { &*(re_handle as *const regex::Regex) };
    match re.find_at(s, start) {
        Some(m) => m.start() as i64,
        None => -1,
    }
}

/// `regex_find_end(re: i64, text: str, from: i64) -> i64` — end byte offset of the
/// next match in `text` at or after `from`, or -1 if no match.
#[no_mangle]
pub extern "C" fn kryos_regex_find_end_ks(re_handle: i64, text_handle: i64, from: i64) -> i64 {
    if re_handle == 0 {
        return -1;
    }
    let s = unsafe { handle_to_str(text_handle) };
    let start = if from < 0 { 0 } else { from as usize };
    if start > s.len() {
        return -1;
    }
    let re = unsafe { &*(re_handle as *const regex::Regex) };
    match re.find_at(s, start) {
        Some(m) => m.end() as i64,
        None => -1,
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
        #[cfg(feature = "http2")]
        {
            // Prefer reqwest's blocking client for HTTPS: it handles HTTP/1.1,
            // h2, chunked transfer, and (critically) servers that close without
            // a TLS close_notify on POST. The hand-rolled rustls client below
            // returns a fast `None` on those POSTs (read_to_end -> UnexpectedEof),
            // which surfaced as bogus "transport failure" on every LLM chat call.
            let eff_method = if method.is_empty() { "GET" } else { method };
            crate::http2::https_request_wire(eff_method, url, headers, body, timeout)
        }
        #[cfg(all(feature = "tls", not(feature = "http2")))]
        {
            do_https_request(&host, port, &request, timeout)
        }
        #[cfg(all(not(feature = "tls"), not(feature = "http2")))]
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

#[cfg(all(feature = "tls", not(feature = "http2")))]
fn ensure_crypto_provider() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // Install ring as the default provider; ignore the error if a provider
        // is already installed (e.g. by another component in the same process).
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

#[cfg(all(feature = "tls", not(feature = "http2")))]
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

// ---------------------------------------------------------------------------
// WASM v0.4 web builtins — native shims
//
// Under the WASM backend these are host imports satisfied by the JS runner.
// Under cranelift/LLVM (native), they're best-effort fallbacks: dom/canvas
// ops print a diagnostic to stderr, fetch_text shells out to https_get,
// alert() prints to stderr.
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn kryos_dom_set_text_ks(id: i64, text: i64) {
    let id_s = unsafe { handle_to_str(id) };
    let txt_s = unsafe { handle_to_str(text) };
    eprintln!("[dom_set_text] #{id_s} <- {txt_s}");
}

#[no_mangle]
pub extern "C" fn kryos_dom_get_value_ks(id: i64) -> i64 {
    let id_s = unsafe { handle_to_str(id) };
    eprintln!("[dom_get_value] #{id_s} -> (native stub: empty)");
    str_to_handle("")
}

#[no_mangle]
pub extern "C" fn kryos_alert_ks(msg: i64) {
    let s = unsafe { handle_to_str(msg) };
    eprintln!("[alert] {s}");
}

#[no_mangle]
pub extern "C" fn kryos_canvas_fill_rect_ks(
    id: i64, x: i64, y: i64, w: i64, h: i64, color: i64,
) {
    let id_s = unsafe { handle_to_str(id) };
    let c_s = unsafe { handle_to_str(color) };
    eprintln!("[canvas_fill_rect] #{id_s} ({x},{y}) {w}x{h} {c_s}");
}

#[no_mangle]
pub extern "C" fn kryos_canvas_clear_ks(id: i64) {
    let id_s = unsafe { handle_to_str(id) };
    eprintln!("[canvas_clear] #{id_s}");
}

#[no_mangle]
pub extern "C" fn kryos_fetch_text_ks(url: i64) -> i64 {
    // On native, fetch_text is just https_get.
    kryos_https_get_ks(url)
}

// ---------------------------------------------------------------------------
// SHA-1 (legacy) + Base64 — needed for WebSocket handshakes and binary I/O.
// ---------------------------------------------------------------------------

/// `sha1_hex(s: str) -> str` — hex-encoded SHA-1 digest of the input string.
#[cfg(feature = "crypto")]
#[no_mangle]
pub extern "C" fn kryos_sha1_hex_ks(input_handle: i64) -> i64 {
    let s = unsafe { handle_to_str(input_handle) };
    let mut out = [0u8; 20];
    let rc = crate::crypto::kryos_sha1(s.as_ptr(), s.len(), out.as_mut_ptr());
    if rc != 0 {
        return str_to_handle("");
    }
    str_to_handle(&hex_encode(&out))
}

/// `sha1_base64(s: str) -> str` — base64-encoded SHA-1 digest (WebSocket handshake).
#[cfg(feature = "crypto")]
#[no_mangle]
pub extern "C" fn kryos_sha1_base64_ks(input_handle: i64) -> i64 {
    let s = unsafe { handle_to_str(input_handle) };
    let mut out = [0u8; 20];
    let rc = crate::crypto::kryos_sha1(s.as_ptr(), s.len(), out.as_mut_ptr());
    if rc != 0 {
        return str_to_handle("");
    }
    str_to_handle(&base64_encode(&out))
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    let chunks = input.chunks(3);
    for chunk in chunks {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };
        let triple = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(BASE64_ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        out.push(BASE64_ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(BASE64_ALPHABET[((triple >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(BASE64_ALPHABET[(triple & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn base64_decode(input: &str) -> Vec<u8> {
    let mut lut = [255u8; 256];
    for (i, &b) in BASE64_ALPHABET.iter().enumerate() {
        lut[b as usize] = i as u8;
    }
    let bytes: Vec<u8> = input.bytes().filter(|&b| b != b'\n' && b != b'\r').collect();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let mut i = 0;
    while i + 4 <= bytes.len() {
        let q = [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]];
        let vals = [lut[q[0] as usize], lut[q[1] as usize], lut[q[2] as usize], lut[q[3] as usize]];
        if vals[0] == 255 || vals[1] == 255 {
            break;
        }
        let triple = ((vals[0] as u32) << 18)
            | ((vals[1] as u32) << 12)
            | (if vals[2] == 255 { 0 } else { (vals[2] as u32) << 6 })
            | (if vals[3] == 255 { 0 } else { vals[3] as u32 });
        out.push(((triple >> 16) & 0xff) as u8);
        if q[2] != b'=' {
            out.push(((triple >> 8) & 0xff) as u8);
        }
        if q[3] != b'=' {
            out.push((triple & 0xff) as u8);
        }
        i += 4;
    }
    out
}

/// `base64_encode(s: str) -> str` — encode the raw bytes of `s` as base64.
#[no_mangle]
pub extern "C" fn kryos_base64_encode_ks(input_handle: i64) -> i64 {
    let s = unsafe { handle_to_str(input_handle) };
    // Read each CODEPOINT as one byte (latin-1), matching base64_decode_ks's
    // 1-codepoint-per-byte output (and the chr/byte_at binary model). The old
    // `s.as_bytes()` used the string's UTF-8 encoding, so a byte >= 0x80 that
    // decode produced as U+0080..U+00FF (2 UTF-8 bytes) re-encoded as 2 bytes ->
    // `base64_encode(base64_decode(x)) != x` for any binary payload. For pure
    // ASCII this is identical to as_bytes().
    let bytes: Vec<u8> = s.chars().map(|c| c as u8).collect();
    str_to_handle(&base64_encode(&bytes))
}

/// `base64_decode(s: str) -> str` — decode base64 back to a (possibly binary) string.
/// Note: non-UTF8 byte sequences are passed through via UTF-8 lossy conversion.
#[no_mangle]
pub extern "C" fn kryos_base64_decode_ks(input_handle: i64) -> i64 {
    let s = unsafe { handle_to_str(input_handle) };
    let bytes = base64_decode(s);
    // Use latin-1 (1-byte-per-codepoint) so byte values survive round-trip
    // when fed back through byte_at(); this matches how WebSocket payloads
    // are typically threaded through Kryos strings.
    let out: String = bytes.iter().map(|&b| b as char).collect();
    str_to_handle(&out)
}

// ---------------------------------------------------------------------------
// Byte primitives: `chr` and `byte_at` (for binary protocols implemented in Kryos)
// ---------------------------------------------------------------------------

/// `chr(n: i64) -> str` — returns a 1-codepoint string containing the byte
/// value `n & 0xff`. We use U+0000..U+00FF (latin-1) so a round trip through
/// `byte_at` recovers the original byte.
#[no_mangle]
pub extern "C" fn kryos_chr_ks(n: i64) -> i64 {
    let b = (n & 0xff) as u8;
    let c = b as char;
    let s: String = c.to_string();
    str_to_handle(&s)
}

/// `byte_at(s: str, idx: i64) -> i64` — returns the byte at the i-th position
/// (latin-1 codepoint scalar value). For ASCII strings this is the same as
/// the standard byte; for strings produced by `chr(n)` this round-trips. Out
/// of range returns -1.
#[no_mangle]
pub extern "C" fn kryos_byte_at_ks(input_handle: i64, idx: i64) -> i64 {
    let s = unsafe { handle_to_str(input_handle) };
    if idx < 0 {
        return -1;
    }
    let i = idx as usize;
    // Walk codepoints, returning the scalar value of the i-th one -- the FULL
    // Unicode codepoint, matching char_code and the documented contract
    // ("CODEPOINT of the i-th CHARACTER"). The previous `& 0xff` truncated
    // any codepoint >= 256 (CJK, emoji, most non-Latin scripts) to its low
    // byte -- byte_at("日", 0) returned 229 (26085 mod 256) instead of 26085.
    // For a latin-1 buffer (codepoints 0-255, the base64/chr byte model) the
    // mask was a no-op, which hid the bug.
    for (k, ch) in s.chars().enumerate() {
        if k == i {
            return (ch as u32) as i64;
        }
    }
    -1
}

// ---------------------------------------------------------------------------
// HTTP/2 client — reqwest-backed, ALPN h2 with HTTP/1.1 fallback (Gap C)
// ---------------------------------------------------------------------------

/// `http2_get(url: str) -> str` — GET request, returns body. Empty string on error.
#[no_mangle]
pub extern "C" fn kryos_http2_get_ks(url_handle: i64) -> i64 {
    let url = unsafe { handle_to_str(url_handle) };
    let result = crate::http2::http2_get(url);
    str_to_handle(&result)
}

/// `http2_post(url: str, body: str) -> str` — POST with body, returns response body.
#[no_mangle]
pub extern "C" fn kryos_http2_post_ks(url_handle: i64, body_handle: i64) -> i64 {
    let url = unsafe { handle_to_str(url_handle) };
    let body = unsafe { handle_to_str(body_handle) };
    let result = crate::http2::http2_post(url, body);
    str_to_handle(&result)
}

/// `http2_request(method: str, url: str, headers: str, body: str) -> str`
/// Full request. headers is `"Name1: val1\nName2: val2"` newline-separated.
/// Returns `"<status>\n<headers>\n\n<body>"` for full access.
#[no_mangle]
pub extern "C" fn kryos_http2_request_ks(
    method_handle: i64,
    url_handle: i64,
    headers_handle: i64,
    body_handle: i64,
) -> i64 {
    let method = unsafe { handle_to_str(method_handle) };
    let url = unsafe { handle_to_str(url_handle) };
    let headers = unsafe { handle_to_str(headers_handle) };
    let body = unsafe { handle_to_str(body_handle) };
    let result = crate::http2::http2_request(method, url, headers, body);
    str_to_handle(&result)
}

// ---------------------------------------------------------------------------
// Crypto: HMAC / Ed25519 / PBKDF2 — handle-based wrappers (ring-backed)
// ---------------------------------------------------------------------------

/// Decode a hex string into bytes. Returns None on odd length / bad digit.
#[cfg(feature = "crypto")]
fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    let nib = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    for pair in bytes.chunks(2) {
        out.push((nib(pair[0])? << 4) | nib(pair[1])?);
    }
    Some(out)
}

/// `hmac_sha256(key: str, data: str) -> str` — hex-encoded HMAC-SHA256 tag.
#[cfg(feature = "crypto")]
#[no_mangle]
pub extern "C" fn kryos_hmac_sha256_ks(key_handle: i64, data_handle: i64) -> i64 {
    let key = unsafe { handle_to_str(key_handle) };
    let data = unsafe { handle_to_str(data_handle) };
    let k = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key.as_bytes());
    let tag = ring::hmac::sign(&k, data.as_bytes());
    str_to_handle(&hex_encode(tag.as_ref()))
}

/// `ed25519_generate() -> str` — hex-encoded PKCS#8 v2 keypair document.
/// Empty string on RNG failure.
#[cfg(feature = "crypto")]
#[no_mangle]
pub extern "C" fn kryos_ed25519_generate_ks() -> i64 {
    let rng = ring::rand::SystemRandom::new();
    match ring::signature::Ed25519KeyPair::generate_pkcs8(&rng) {
        Ok(doc) => str_to_handle(&hex_encode(doc.as_ref())),
        Err(_) => str_to_handle(""),
    }
}

/// `ed25519_public(pkcs8_hex: str) -> str` — hex of the 32-byte public key.
#[cfg(feature = "crypto")]
#[no_mangle]
pub extern "C" fn kryos_ed25519_public_ks(pkcs8_handle: i64) -> i64 {
    use ring::signature::KeyPair;
    let pkcs8_hex = unsafe { handle_to_str(pkcs8_handle) };
    let Some(pkcs8) = hex_decode(pkcs8_hex) else {
        return str_to_handle("");
    };
    match ring::signature::Ed25519KeyPair::from_pkcs8(&pkcs8) {
        Ok(kp) => str_to_handle(&hex_encode(kp.public_key().as_ref())),
        Err(_) => str_to_handle(""),
    }
}

/// `ed25519_sign(pkcs8_hex: str, msg: str) -> str` — hex of the 64-byte signature.
#[cfg(feature = "crypto")]
#[no_mangle]
pub extern "C" fn kryos_ed25519_sign_ks(pkcs8_handle: i64, msg_handle: i64) -> i64 {
    let pkcs8_hex = unsafe { handle_to_str(pkcs8_handle) };
    let msg = unsafe { handle_to_str(msg_handle) };
    let Some(pkcs8) = hex_decode(pkcs8_hex) else {
        return str_to_handle("");
    };
    match ring::signature::Ed25519KeyPair::from_pkcs8(&pkcs8) {
        Ok(kp) => str_to_handle(&hex_encode(kp.sign(msg.as_bytes()).as_ref())),
        Err(_) => str_to_handle(""),
    }
}

/// `ed25519_verify(pub_hex: str, msg: str, sig_hex: str) -> i64` — 1 valid, 0 not.
#[cfg(feature = "crypto")]
#[no_mangle]
pub extern "C" fn kryos_ed25519_verify_ks(pub_handle: i64, msg_handle: i64, sig_handle: i64) -> i64 {
    let pub_hex = unsafe { handle_to_str(pub_handle) };
    let msg = unsafe { handle_to_str(msg_handle) };
    let sig_hex = unsafe { handle_to_str(sig_handle) };
    let (Some(pubkey), Some(sig)) = (hex_decode(pub_hex), hex_decode(sig_hex)) else {
        return 0;
    };
    let key = ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, pubkey);
    match key.verify(msg.as_bytes(), &sig) {
        Ok(()) => 1,
        Err(_) => 0,
    }
}

/// `pbkdf2_sha256(password: str, salt_hex: str, iters: i64) -> str` — hex of
/// the 32-byte derived key. Iterations clamped to [1_000, 10_000_000].
#[cfg(feature = "crypto")]
#[no_mangle]
pub extern "C" fn kryos_pbkdf2_sha256_ks(pw_handle: i64, salt_handle: i64, iters: i64) -> i64 {
    let pw = unsafe { handle_to_str(pw_handle) };
    let salt_hex = unsafe { handle_to_str(salt_handle) };
    let Some(salt) = hex_decode(salt_hex) else {
        return str_to_handle("");
    };
    let iters = iters.clamp(1_000, 10_000_000) as u32;
    let Some(n) = std::num::NonZeroU32::new(iters) else {
        return str_to_handle("");
    };
    let mut out = [0u8; 32];
    ring::pbkdf2::derive(
        ring::pbkdf2::PBKDF2_HMAC_SHA256,
        n,
        &salt,
        pw.as_bytes(),
        &mut out,
    );
    str_to_handle(&hex_encode(&out))
}

/// `hex_to_base64url(hex: str) -> str` — re-encode hex-encoded bytes as
/// unpadded base64url (RFC 4648 §5). Empty string on malformed hex.
/// Binary-safe bridge for JWT/DKIM signatures (Kryos strings are UTF-8, so
/// raw signature bytes travel as hex and re-encode here).
#[cfg(feature = "crypto")]
#[no_mangle]
pub extern "C" fn kryos_hex_to_b64url_ks(hex_handle: i64) -> i64 {
    const ALPHA: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let hex = unsafe { handle_to_str(hex_handle) };
    let Some(bytes) = hex_decode(hex) else {
        return str_to_handle("");
    };
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        out.push(ALPHA[(b[0] >> 2) as usize] as char);
        out.push(ALPHA[(((b[0] & 0x03) << 4) | (b[1] >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHA[(((b[1] & 0x0f) << 2) | (b[2] >> 6)) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHA[(b[2] & 0x3f) as usize] as char);
        }
    }
    str_to_handle(&out)
}

/// `base64url_to_hex(b64url: str) -> str` — decode unpadded base64url to
/// hex-encoded bytes. Empty string on malformed input.
#[cfg(feature = "crypto")]
#[no_mangle]
pub extern "C" fn kryos_b64url_to_hex_ks(b64_handle: i64) -> i64 {
    let s = unsafe { handle_to_str(b64_handle) };
    let val = |c: u8| -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    };
    let raw: &[u8] = s.as_bytes();
    let mut bytes = Vec::with_capacity(raw.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits = 0u32;
    for &c in raw {
        let Some(v) = val(c) else {
            return str_to_handle("");
        };
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buf >> bits) as u8);
        }
    }
    str_to_handle(&hex_encode(&bytes))
}
