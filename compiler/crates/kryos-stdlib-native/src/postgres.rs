//! PostgreSQL client driver for Kryos stdlib.
//!
//! Uses the synchronous `postgres` crate with optional `native-tls` for TLS
//! support (required for cloud-hosted Postgres, e.g. Neon).
//!
//! Connections are stored in a global table keyed by i64 handles.
//!
//! API:
//!   kryos_pg_connect(conn_str_ptr, len) -> i64   -- open connection; returns handle or -1
//!   kryos_pg_exec(handle, sql_ptr, len) -> i64   -- run DDL/DML; returns rows affected or -1
//!   kryos_pg_query(handle, sql_ptr, len)         -- run SELECT; returns JSON string handle
//!   kryos_pg_close(handle) -> i64                -- close connection; returns 0 or -1
//!
//! Plus `_ks` wrappers that accept KryosString handles.

use postgres::types::Type;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};

// Counter starts at 7000 to avoid collisions with other handle namespaces.
static PG_COUNTER: AtomicI64 = AtomicI64::new(7000);

fn pg_table() -> &'static Mutex<HashMap<i64, postgres::Client>> {
    static TABLE: OnceLock<Mutex<HashMap<i64, postgres::Client>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

unsafe fn ptr_to_str<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    let bytes = std::slice::from_raw_parts(ptr, len);
    std::str::from_utf8(bytes).ok()
}

/// Detect whether the connection string requests no TLS.
fn wants_no_tls(conn_str: &str) -> bool {
    conn_str.contains("sslmode=disable")
}

/// Build a JSON string from a single postgres::Row.
/// All values are converted to strings; NULLs become JSON null.
fn pg_row_to_json(row: &postgres::Row) -> String {
    let cols = row.columns();
    let mut out = String::from("{");
    for (i, col) in cols.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        // Key
        out.push('"');
        json_escape_into(&mut out, col.name());
        out.push_str("\":");

        // Value — try common types in order. Fall back to null.
        let val_str: Option<String> = try_get_as_string(row, i, col.type_());

        match val_str {
            Some(s) => {
                out.push('"');
                json_escape_into(&mut out, &s);
                out.push('"');
            }
            None => {
                out.push_str("null");
            }
        }
    }
    out.push('}');
    out
}

/// Attempt to read column `i` as a String by trying the most common PG types.
fn try_get_as_string(row: &postgres::Row, i: usize, ty: &Type) -> Option<String> {
    // Try TEXT / VARCHAR / BPCHAR / NAME first (direct string).
    if let Ok(v) = row.try_get::<_, Option<String>>(i) {
        return v;
    }

    // Integers
    match *ty {
        Type::INT2 => {
            if let Ok(Some(v)) = row.try_get::<_, Option<i16>>(i) {
                return Some(v.to_string());
            }
        }
        Type::INT4 => {
            if let Ok(Some(v)) = row.try_get::<_, Option<i32>>(i) {
                return Some(v.to_string());
            }
        }
        Type::INT8 | Type::OID => {
            if let Ok(Some(v)) = row.try_get::<_, Option<i64>>(i) {
                return Some(v.to_string());
            }
        }
        Type::FLOAT4 => {
            if let Ok(Some(v)) = row.try_get::<_, Option<f32>>(i) {
                return Some(v.to_string());
            }
        }
        Type::FLOAT8 => {
            if let Ok(Some(v)) = row.try_get::<_, Option<f64>>(i) {
                return Some(v.to_string());
            }
        }
        Type::BOOL => {
            if let Ok(Some(v)) = row.try_get::<_, Option<bool>>(i) {
                return Some(if v { "true".to_string() } else { "false".to_string() });
            }
        }
        _ => {}
    }

    // For SERIAL (returns as INT4) and similar, the first try_get::<String> may
    // fail, but the type-specific branches above should catch it.
    // If nothing matched, return None (will serialize as null).
    None
}

/// Append `s` to `out` with JSON string escaping.
fn json_escape_into(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                // Control character — encode as \uXXXX
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
}

// ---------------------------------------------------------------------------
// Low-level FFI (ptr/len interface)
// ---------------------------------------------------------------------------

/// Open a PostgreSQL connection.
/// `conn_str` is a standard libpq connection string, e.g.:
///   `postgresql://user:pass@host:port/db?sslmode=require`
///
/// When `sslmode=disable` is present, NoTls is used; otherwise native-tls is
/// used (so Neon and other cloud providers work out of the box).
///
/// Returns a positive handle on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn kryos_pg_connect(conn_str_ptr: *const u8, conn_str_len: usize) -> i64 {
    let conn_str = match ptr_to_str(conn_str_ptr, conn_str_len) {
        Some(s) => s,
        None => return -1,
    };

    let client: Result<postgres::Client, _> = if wants_no_tls(conn_str) {
        postgres::Client::connect(conn_str, postgres::NoTls)
    } else {
        let tls_builder = match native_tls::TlsConnector::builder()
            .danger_accept_invalid_certs(false)
            .build()
        {
            Ok(b) => b,
            Err(_) => return -1,
        };
        let tls = postgres_native_tls::MakeTlsConnector::new(tls_builder);
        postgres::Client::connect(conn_str, tls)
    };

    match client {
        Ok(c) => {
            let id = PG_COUNTER.fetch_add(1, Ordering::Relaxed);
            pg_table().lock().unwrap().insert(id, c);
            id
        }
        Err(_) => -1,
    }
}

/// Execute a statement that returns no rows (DDL, DML).
/// Returns rows-affected count on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn kryos_pg_exec(handle: i64, sql_ptr: *const u8, sql_len: usize) -> i64 {
    let sql = match ptr_to_str(sql_ptr, sql_len) {
        Some(s) => s,
        None => return -1,
    };
    let mut guard = pg_table().lock().unwrap();
    let client = match guard.get_mut(&handle) {
        Some(c) => c,
        None => return -1,
    };
    match client.execute(sql, &[]) {
        Ok(n) => n as i64,
        Err(_) => -1,
    }
}

/// Execute a query and return all rows as a JSON string
/// (pointer to a heap-allocated NUL-terminated C string).
///
/// Format: `[{"col1":"val1","col2":"val2"}, ...]`
/// Returns NULL pointer on error.
#[no_mangle]
pub unsafe extern "C" fn kryos_pg_query(
    handle: i64,
    sql_ptr: *const u8,
    sql_len: usize,
) -> *mut u8 {
    let sql = match ptr_to_str(sql_ptr, sql_len) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };
    let mut guard = pg_table().lock().unwrap();
    let client = match guard.get_mut(&handle) {
        Some(c) => c,
        None => return std::ptr::null_mut(),
    };

    let rows = match client.query(sql, &[]) {
        Ok(r) => r,
        Err(_) => return std::ptr::null_mut(),
    };

    let mut json = String::from("[");
    for (idx, row) in rows.iter().enumerate() {
        if idx > 0 {
            json.push(',');
        }
        json.push_str(&pg_row_to_json(row));
    }
    json.push(']');

    // Leak the string as a C string (caller must free — but in Kryos it's
    // captured by the _ks wrapper which copies it into a KryosString).
    let mut bytes = json.into_bytes();
    bytes.push(0); // NUL terminate
    let ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    ptr
}

/// Close a PostgreSQL connection.
/// Returns 0 on success, -1 if handle is unknown.
#[no_mangle]
pub unsafe extern "C" fn kryos_pg_close(handle: i64) -> i64 {
    if pg_table().lock().unwrap().remove(&handle).is_some() {
        0
    } else {
        -1
    }
}

// ---------------------------------------------------------------------------
// KryosString-handle wrappers (_ks) used by the JIT / AOT compiler
// ---------------------------------------------------------------------------

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

fn str_to_handle(s: &str) -> i64 {
    let bytes = s.as_bytes();
    unsafe {
        let p = kryos_rt::string::kryos_string_new(bytes.as_ptr(), bytes.len() as i64);
        if p.is_null() {
            0
        } else {
            p as i64
        }
    }
}

/// `pg_connect(conn_str: str) -> i64`
#[no_mangle]
pub unsafe extern "C" fn kryos_pg_connect_ks(conn_str_handle: i64) -> i64 {
    let (ptr, len) = handle_to_bytes(conn_str_handle);
    kryos_pg_connect(ptr, len)
}

/// `pg_exec(handle: i64, sql: str) -> i64`
#[no_mangle]
pub unsafe extern "C" fn kryos_pg_exec_ks(handle: i64, sql_handle: i64) -> i64 {
    let (ptr, len) = handle_to_bytes(sql_handle);
    kryos_pg_exec(handle, ptr, len)
}

/// `pg_query(handle: i64, sql: str) -> str`
#[no_mangle]
pub unsafe extern "C" fn kryos_pg_query_ks(handle: i64, sql_handle: i64) -> i64 {
    let (ptr, len) = handle_to_bytes(sql_handle);
    let raw = kryos_pg_query(handle, ptr, len);
    if raw.is_null() {
        return str_to_handle("[]");
    }
    // raw is a NUL-terminated heap string we own
    let c_str = std::ffi::CStr::from_ptr(raw as *const i8);
    let s = c_str.to_str().unwrap_or("[]");
    let handle = str_to_handle(s);
    // Free the raw allocation
    let len = s.len() + 1;
    let _ = Vec::from_raw_parts(raw, len, len);
    handle
}

/// `pg_close(handle: i64) -> i64`
#[no_mangle]
pub unsafe extern "C" fn kryos_pg_close_ks(handle: i64) -> i64 {
    kryos_pg_close(handle)
}
