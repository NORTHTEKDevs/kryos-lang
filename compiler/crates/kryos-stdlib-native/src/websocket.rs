//! WebSocket (RFC 6455) server-side helpers for the Kryos native stdlib.
//!
//! These helpers are intentionally low-level: a Kryos program accepts a TCP
//! connection (via `tcp_accept`), reads the HTTP Upgrade request itself, then
//! uses these functions to:
//!   - compute the `Sec-WebSocket-Accept` value (`ws_accept_key`)
//!   - encode an outbound text/binary/control frame (`ws_encode_*`)
//!   - decode an inbound frame from a byte buffer (`ws_decode_frame`)
//!   - unmask a masked client frame in place (`ws_unmask`)
//!
//! All `str`-typed returns are boxed `KryosString` handles, matching the
//! convention in `net.rs`, `tls.rs`, and `postgres.rs`.

#![allow(clippy::missing_safety_doc)]

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

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
    });
    Box::into_raw(boxed) as i64
}

/// `ws_accept_key(key: str) -> str` — computes the Sec-WebSocket-Accept header.
#[no_mangle]
pub extern "C" fn kryos_ws_accept_key_ks(key_handle: i64) -> i64 {
    let (ptr, len) = unsafe { handle_to_bytes(key_handle) };
    if ptr.is_null() {
        return 0;
    }
    let key_bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let key_str = match std::str::from_utf8(key_bytes) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let mut concat = String::new();
    concat.push_str(key_str.trim());
    concat.push_str(GUID);
    let mut digest = [0u8; 20];
    let cb = concat.as_bytes();
    let rc = crate::crypto::kryos_sha1(cb.as_ptr(), cb.len(), digest.as_mut_ptr() as *mut u8);
    if rc != 0 {
        return 0;
    }
    let encoded = B64.encode(digest);
    vec_to_handle(encoded.into_bytes())
}

fn encode_frame(opcode: u8, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 10);
    out.push(0x80 | (opcode & 0x0F)); // FIN=1
    let len = data.len();
    if len < 126 {
        out.push(len as u8);
    } else if len <= u16::MAX as usize {
        out.push(126);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(127);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }
    out.extend_from_slice(data);
    out
}

/// `ws_encode_text(s: str) -> str` — wraps a text payload in an unmasked frame.
#[no_mangle]
pub extern "C" fn kryos_ws_encode_text_ks(payload_handle: i64) -> i64 {
    let (ptr, len) = unsafe { handle_to_bytes(payload_handle) };
    let payload = if ptr.is_null() { &[][..] } else { unsafe { std::slice::from_raw_parts(ptr, len) } };
    vec_to_handle(encode_frame(0x1, payload))
}

/// `ws_encode_binary(buf: str) -> str` — wraps binary payload in an unmasked frame.
#[no_mangle]
pub extern "C" fn kryos_ws_encode_binary_ks(payload_handle: i64) -> i64 {
    let (ptr, len) = unsafe { handle_to_bytes(payload_handle) };
    let payload = if ptr.is_null() { &[][..] } else { unsafe { std::slice::from_raw_parts(ptr, len) } };
    vec_to_handle(encode_frame(0x2, payload))
}

/// `ws_encode_close(code: i64) -> str`
#[no_mangle]
pub extern "C" fn kryos_ws_encode_close(code: i64) -> i64 {
    let payload = (code as u16).to_be_bytes();
    vec_to_handle(encode_frame(0x8, &payload))
}

/// `ws_encode_ping(buf: str) -> str`
#[no_mangle]
pub extern "C" fn kryos_ws_encode_ping_ks(payload_handle: i64) -> i64 {
    let (ptr, len) = unsafe { handle_to_bytes(payload_handle) };
    let payload = if ptr.is_null() { &[][..] } else { unsafe { std::slice::from_raw_parts(ptr, len) } };
    vec_to_handle(encode_frame(0x9, payload))
}

/// `ws_encode_pong(buf: str) -> str`
#[no_mangle]
pub extern "C" fn kryos_ws_encode_pong_ks(payload_handle: i64) -> i64 {
    let (ptr, len) = unsafe { handle_to_bytes(payload_handle) };
    let payload = if ptr.is_null() { &[][..] } else { unsafe { std::slice::from_raw_parts(ptr, len) } };
    vec_to_handle(encode_frame(0xA, payload))
}

/// `ws_decode_frame(buf: str, out_opcode: &i64, out_fin: &i64,
///                  out_payload_off: &i64, out_payload_len: &i64,
///                  out_total_len: &i64, out_mask_off: &i64) -> i64`
///
/// Returns 0 on success, -1 if `buf` has insufficient bytes, -2 on protocol error.
#[no_mangle]
pub extern "C" fn kryos_ws_decode_frame_ks(
    buf_handle: i64,
    out_opcode: *mut i64,
    out_fin: *mut i64,
    out_payload_off: *mut i64,
    out_payload_len: *mut i64,
    out_total_len: *mut i64,
    out_mask_off: *mut i64,
) -> i64 {
    let (buf_ptr, buf_len) = unsafe { handle_to_bytes(buf_handle) };
    if buf_ptr.is_null() || buf_len < 2 {
        return -1;
    }
    let buf = unsafe { std::slice::from_raw_parts(buf_ptr, buf_len) };
    let b0 = buf[0];
    let b1 = buf[1];
    let fin = (b0 & 0x80) != 0;
    let opcode = (b0 & 0x0F) as i64;
    let masked = (b1 & 0x80) != 0;
    let len7 = (b1 & 0x7F) as usize;
    let mut idx = 2usize;
    let payload_len: usize = if len7 < 126 {
        len7
    } else if len7 == 126 {
        if buf_len < idx + 2 { return -1; }
        let l = u16::from_be_bytes([buf[idx], buf[idx+1]]) as usize;
        idx += 2;
        l
    } else {
        if buf_len < idx + 8 { return -1; }
        let mut arr = [0u8; 8];
        arr.copy_from_slice(&buf[idx..idx+8]);
        let l = u64::from_be_bytes(arr) as usize;
        idx += 8;
        l
    };
    let mask_off = if masked {
        if buf_len < idx + 4 { return -1; }
        let off = idx as i64;
        idx += 4;
        off
    } else {
        -1
    };
    let payload_off = idx;
    let total_len = idx + payload_len;
    if buf_len < total_len {
        return -1;
    }
    unsafe {
        if !out_opcode.is_null() { *out_opcode = opcode; }
        if !out_fin.is_null() { *out_fin = if fin { 1 } else { 0 }; }
        if !out_payload_off.is_null() { *out_payload_off = payload_off as i64; }
        if !out_payload_len.is_null() { *out_payload_len = payload_len as i64; }
        if !out_total_len.is_null() { *out_total_len = total_len as i64; }
        if !out_mask_off.is_null() { *out_mask_off = mask_off; }
    }
    0
}

/// `ws_read_frame(fd: i64) -> str` — reads exactly one WebSocket frame from a
/// connected TCP fd (must be in blocking mode). Returns the unmasked payload
/// as a string. Returns an empty string on close/EOF or any error.
///
/// Control frames (close, ping, pong) are returned with a 1-byte prefix:
///   - first byte = opcode (0x8 close, 0x9 ping, 0xA pong)
///   - rest      = payload
/// Text/binary frames are returned with the raw payload only (opcode is implied).
///
/// To distinguish: use a sentinel byte by checking length+content, or call the
/// lower-level decode functions instead. This convenience function is for the
/// common echo/chat case.
#[no_mangle]
pub extern "C" fn kryos_ws_read_frame_ks(fd: i64) -> i64 {
    use std::io::Read;
    // Acquire a clone of the stream from the global TCP socket table.
    let mut stream = match crate::net::with_tcp_stream(fd) {
        Some(s) => s,
        None => return 0,
    };
    let mut header = [0u8; 2];
    if stream.read_exact(&mut header).is_err() {
        return 0;
    }
    let b0 = header[0];
    let b1 = header[1];
    let opcode = b0 & 0x0F;
    let masked = (b1 & 0x80) != 0;
    let len7 = (b1 & 0x7F) as usize;
    let payload_len: usize = if len7 < 126 {
        len7
    } else if len7 == 126 {
        let mut b = [0u8; 2];
        if stream.read_exact(&mut b).is_err() { return 0; }
        u16::from_be_bytes(b) as usize
    } else {
        let mut b = [0u8; 8];
        if stream.read_exact(&mut b).is_err() { return 0; }
        u64::from_be_bytes(b) as usize
    };
    let mut mask = [0u8; 4];
    if masked && stream.read_exact(&mut mask).is_err() {
        return 0;
    }
    let mut payload = vec![0u8; payload_len];
    if payload_len > 0 && stream.read_exact(&mut payload).is_err() {
        return 0;
    }
    if masked {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= mask[i & 3];
        }
    }
    // For text/binary just return payload; for control frames prefix with opcode.
    let result = if opcode == 0x1 || opcode == 0x2 {
        payload
    } else {
        let mut v = Vec::with_capacity(payload.len() + 1);
        v.push(opcode);
        v.extend_from_slice(&payload);
        v
    };
    vec_to_handle(result)
}

/// `ws_unmask(buf: str, payload_off: i64, payload_len: i64, mask_off: i64) -> str`
/// Returns a new KryosString handle containing the unmasked payload only.
#[no_mangle]
pub extern "C" fn kryos_ws_unmask_ks(
    buf_handle: i64,
    payload_off: i64,
    payload_len: i64,
    mask_off: i64,
) -> i64 {
    let (buf_ptr, buf_len) = unsafe { handle_to_bytes(buf_handle) };
    if buf_ptr.is_null() {
        return 0;
    }
    let buf = unsafe { std::slice::from_raw_parts(buf_ptr, buf_len) };
    let off = payload_off.max(0) as usize;
    let len = payload_len.max(0) as usize;
    if off + len > buf_len {
        return 0;
    }
    let mut payload = buf[off..off + len].to_vec();
    if mask_off >= 0 {
        let m = mask_off as usize;
        if m + 4 > buf_len {
            return 0;
        }
        let mask = [buf[m], buf[m+1], buf[m+2], buf[m+3]];
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= mask[i & 3];
        }
    }
    vec_to_handle(payload)
}
