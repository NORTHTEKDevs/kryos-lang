//! SQLite database driver for Kryos stdlib.
//!
//! Connections and query cursors are tracked in global registries keyed by
//! monotonically-increasing i64 handles.  All functions are #[no_mangle]
//! extern "C" so Kryos-compiled programs can link against them directly.
//!
//! API overview:
//!   kryos_db_open(path, len) -> i64    -- open/create DB; returns handle or -1
//!   kryos_db_close(conn) -> i32        -- close connection
//!   kryos_db_exec(conn, sql, len) -> i64 -- execute DDL/DML; returns rows affected or -1
//!   kryos_db_prepare(conn, sql, len) -> i64 -- run SELECT, collect rows; returns cursor or -1
//!   kryos_db_step(cursor) -> i32       -- advance cursor: 1=row available, 0=done, -1=error
//!   kryos_db_col_count(cursor) -> i32  -- number of columns in current result set
//!   kryos_db_col_int(cursor, col) -> i64    -- read integer column
//!   kryos_db_col_text_len(cursor, col) -> i64 -- byte length of text column
//!   kryos_db_col_text_copy(cursor, col, buf, buf_len) -> i64 -- copy text into caller buffer
//!   kryos_db_finalize(cursor) -> i32   -- free cursor

use rusqlite::{types::Value, Connection};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Global registries
// ---------------------------------------------------------------------------

static CONN_COUNTER: AtomicI64 = AtomicI64::new(1);
static CURSOR_COUNTER: AtomicI64 = AtomicI64::new(1);

fn conn_map() -> &'static Mutex<HashMap<i64, Connection>> {
    static MAP: OnceLock<Mutex<HashMap<i64, Connection>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

struct Cursor {
    rows: Vec<Vec<Value>>,
    col_count: usize,
    position: usize,
}

fn cursor_map() -> &'static Mutex<HashMap<i64, Cursor>> {
    static MAP: OnceLock<Mutex<HashMap<i64, Cursor>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
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

// ---------------------------------------------------------------------------
// FFI surface
// ---------------------------------------------------------------------------

/// Open (or create) a SQLite database at `path`.
/// Returns a positive connection handle on success, -1 on failure.
#[no_mangle]
pub unsafe extern "C" fn kryos_db_open(path_ptr: *const u8, path_len: usize) -> i64 {
    let path = match ptr_to_str(path_ptr, path_len) {
        Some(s) => s,
        None => return -1,
    };
    let conn = match Connection::open(path) {
        Ok(c) => c,
        Err(_) => return -1,
    };
    let id = CONN_COUNTER.fetch_add(1, Ordering::Relaxed);
    conn_map().lock().unwrap().insert(id, conn);
    id
}

/// Open an in-memory SQLite database.
/// Returns a positive connection handle on success, -1 on failure.
#[no_mangle]
pub unsafe extern "C" fn kryos_db_open_memory() -> i64 {
    let conn = match Connection::open_in_memory() {
        Ok(c) => c,
        Err(_) => return -1,
    };
    let id = CONN_COUNTER.fetch_add(1, Ordering::Relaxed);
    conn_map().lock().unwrap().insert(id, conn);
    id
}

/// Close a database connection.  Returns 0 on success, -1 if handle unknown.
#[no_mangle]
pub unsafe extern "C" fn kryos_db_close(conn: i64) -> i32 {
    if conn_map().lock().unwrap().remove(&conn).is_some() {
        0
    } else {
        -1
    }
}

/// Execute a DDL or DML statement (CREATE, INSERT, UPDATE, DELETE, …).
/// Returns the number of rows changed on success, or -1 on error.
#[no_mangle]
pub unsafe extern "C" fn kryos_db_exec(conn: i64, sql_ptr: *const u8, sql_len: usize) -> i64 {
    let sql = match ptr_to_str(sql_ptr, sql_len) {
        Some(s) => s,
        None => return -1,
    };
    let mut guard = conn_map().lock().unwrap();
    let conn_obj = match guard.get_mut(&conn) {
        Some(c) => c,
        None => return -1,
    };
    match conn_obj.execute(sql, []) {
        Ok(n) => n as i64,
        Err(_) => -1,
    }
}

/// Execute a SELECT and collect all rows into a cursor.
/// Returns a positive cursor handle on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn kryos_db_prepare(conn: i64, sql_ptr: *const u8, sql_len: usize) -> i64 {
    let sql = match ptr_to_str(sql_ptr, sql_len) {
        Some(s) => s,
        None => return -1,
    };
    let guard = conn_map().lock().unwrap();
    let conn_obj = match guard.get(&conn) {
        Some(c) => c,
        None => return -1,
    };
    let mut stmt = match conn_obj.prepare(sql) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let col_count = stmt.column_count();
    let mut rows: Vec<Vec<Value>> = Vec::new();
    {
        let mut iter = match stmt.query([]) {
            Ok(r) => r,
            Err(_) => return -1,
        };
        loop {
            match iter.next() {
                Ok(Some(row)) => {
                    let mut cols = Vec::with_capacity(col_count);
                    for i in 0..col_count {
                        let val: Value = row.get(i).unwrap_or(Value::Null);
                        cols.push(val);
                    }
                    rows.push(cols);
                }
                Ok(None) => break,
                Err(_) => return -1,
            }
        }
    }
    let cursor = Cursor {
        rows,
        col_count,
        position: 0,
    };
    let id = CURSOR_COUNTER.fetch_add(1, Ordering::Relaxed);
    cursor_map().lock().unwrap().insert(id, cursor);
    id
}

/// Advance the cursor to the next row.
/// Returns 1 if a row is available, 0 if exhausted, -1 if handle unknown.
#[no_mangle]
pub unsafe extern "C" fn kryos_db_step(cursor: i64) -> i32 {
    let mut guard = cursor_map().lock().unwrap();
    match guard.get_mut(&cursor) {
        Some(c) => {
            if c.position < c.rows.len() {
                c.position += 1;
                1
            } else {
                0
            }
        }
        None => -1,
    }
}

/// Return the number of columns in the cursor's result set, or -1 if unknown.
#[no_mangle]
pub unsafe extern "C" fn kryos_db_col_count(cursor: i64) -> i32 {
    cursor_map()
        .lock()
        .unwrap()
        .get(&cursor)
        .map(|c| c.col_count as i32)
        .unwrap_or(-1)
}

/// Read an integer column from the current row (position - 1).
/// Returns 0 for NULL or text columns that aren't integers.
#[no_mangle]
pub unsafe extern "C" fn kryos_db_col_int(cursor: i64, col: i32) -> i64 {
    let guard = cursor_map().lock().unwrap();
    let c = match guard.get(&cursor) {
        Some(c) => c,
        None => return 0,
    };
    let row_idx = c.position.wrapping_sub(1);
    let row = match c.rows.get(row_idx) {
        Some(r) => r,
        None => return 0,
    };
    match row.get(col as usize) {
        Some(Value::Integer(n)) => *n,
        Some(Value::Real(f)) => *f as i64,
        _ => 0,
    }
}

/// Return the byte length of a text column in the current row, or -1 on error.
#[no_mangle]
pub unsafe extern "C" fn kryos_db_col_text_len(cursor: i64, col: i32) -> i64 {
    let guard = cursor_map().lock().unwrap();
    let c = match guard.get(&cursor) {
        Some(c) => c,
        None => return -1,
    };
    let row_idx = c.position.wrapping_sub(1);
    let row = match c.rows.get(row_idx) {
        Some(r) => r,
        None => return -1,
    };
    match row.get(col as usize) {
        Some(Value::Text(s)) => s.len() as i64,
        Some(Value::Integer(n)) => n.to_string().len() as i64,
        Some(Value::Real(f)) => f.to_string().len() as i64,
        Some(Value::Null) | None => 0,
        Some(Value::Blob(b)) => b.len() as i64,
    }
}

/// Copy the text of a column into `buf` (capacity `buf_len` bytes).
/// Returns bytes written, or -1 on error.  Does not NUL-terminate.
#[no_mangle]
pub unsafe extern "C" fn kryos_db_col_text_copy(
    cursor: i64,
    col: i32,
    buf: *mut u8,
    buf_len: usize,
) -> i64 {
    if buf.is_null() {
        return -1;
    }
    let guard = cursor_map().lock().unwrap();
    let c = match guard.get(&cursor) {
        Some(c) => c,
        None => return -1,
    };
    let row_idx = c.position.wrapping_sub(1);
    let row = match c.rows.get(row_idx) {
        Some(r) => r,
        None => return -1,
    };
    let text: String = match row.get(col as usize) {
        Some(Value::Text(s)) => s.clone(),
        Some(Value::Integer(n)) => n.to_string(),
        Some(Value::Real(f)) => f.to_string(),
        Some(Value::Null) | None => return 0,
        Some(Value::Blob(_)) => return -1,
    };
    let bytes = text.as_bytes();
    let n = bytes.len().min(buf_len);
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, n);
    n as i64
}

/// Free a cursor and release its memory.  Returns 0 or -1 if unknown.
#[no_mangle]
pub unsafe extern "C" fn kryos_db_finalize(cursor: i64) -> i32 {
    if cursor_map().lock().unwrap().remove(&cursor).is_some() {
        0
    } else {
        -1
    }
}
