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

use rusqlite::{params_from_iter, types::Value, Connection};
//
// # Unsafe invariants (file-wide)
//
// See `docs/17-unsafe-audit.md` pattern 7 (FFI). The SQLite C API is wrapped
// via the `rusqlite` crate (safe Rust); the only unsafe in this module is
// reconstruction of `KryosString` handles from `i64`, which follows pattern 1.
// Connections and prepared cursors live in a Mutex-protected HashMap so
// concurrent Kryos threads can share an opened DB handle.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};

// ---------------------------------------------------------------------------
// Global registries
// ---------------------------------------------------------------------------

static CONN_COUNTER: AtomicI64 = AtomicI64::new(1);
static CURSOR_COUNTER: AtomicI64 = AtomicI64::new(1);
static STMT_COUNTER: AtomicI64 = AtomicI64::new(1);

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

/// A prepared statement awaiting parameter binding and execution.
///
/// We do not hold a live `rusqlite::Statement` (its lifetime borrows the
/// `Connection`, which makes it impossible to store in a `'static` registry).
/// Instead we retain the SQL text plus the bound values keyed by 1-based
/// parameter index, then prepare + bind + run in one shot at execute/query
/// time. The values flow through rusqlite's parameter binding, so a payload
/// like `'); DROP TABLE` is stored as literal data, never executed as SQL.
struct PreparedStmt {
    conn_id: i64,
    sql: String,
    binds: BTreeMap<usize, Value>,
}

fn stmt_map() -> &'static Mutex<HashMap<i64, PreparedStmt>> {
    static MAP: OnceLock<Mutex<HashMap<i64, PreparedStmt>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Build an ordered (1..=max) parameter vector from the sparse bind map,
/// filling any gap with NULL so positional `?`/`?N` placeholders line up.
fn ordered_params(binds: &BTreeMap<usize, Value>) -> Vec<Value> {
    let max = binds.keys().copied().max().unwrap_or(0);
    (1..=max)
        .map(|i| binds.get(&i).cloned().unwrap_or(Value::Null))
        .collect()
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

// ---------------------------------------------------------------------------
// Prepared statements with parameter binding (SQL-injection-safe)
// ---------------------------------------------------------------------------

/// Create a prepared statement over `sql` for connection `conn`.
/// Returns a positive statement handle on success, -1 on error.
/// The SQL is validated lazily at execute/query time, not here.
#[no_mangle]
pub unsafe extern "C" fn kryos_db_stmt_prepare(
    conn: i64,
    sql_ptr: *const u8,
    sql_len: usize,
) -> i64 {
    let sql = match ptr_to_str(sql_ptr, sql_len) {
        Some(s) => s,
        None => return -1,
    };
    // Reject unknown connection handles up front.
    if !conn_map().lock().unwrap().contains_key(&conn) {
        return -1;
    }
    let stmt = PreparedStmt {
        conn_id: conn,
        sql: sql.to_string(),
        binds: BTreeMap::new(),
    };
    let id = STMT_COUNTER.fetch_add(1, Ordering::Relaxed);
    stmt_map().lock().unwrap().insert(id, stmt);
    id
}

/// Bind a text value to 1-based parameter index `idx` of `stmt`.
/// The value is stored as data; it is never interpreted as SQL.
/// Returns 0 on success, -1 if the handle or string is invalid.
#[no_mangle]
pub unsafe extern "C" fn kryos_db_bind_text(
    stmt: i64,
    idx: i64,
    val_ptr: *const u8,
    val_len: usize,
) -> i32 {
    let val = match ptr_to_str(val_ptr, val_len) {
        Some(s) => s.to_string(),
        None => return -1,
    };
    if idx < 1 {
        return -1;
    }
    let mut guard = stmt_map().lock().unwrap();
    match guard.get_mut(&stmt) {
        Some(ps) => {
            ps.binds.insert(idx as usize, Value::Text(val));
            0
        }
        None => -1,
    }
}

/// Bind an integer value to 1-based parameter index `idx` of `stmt`.
/// Returns 0 on success, -1 if the handle is invalid.
#[no_mangle]
pub unsafe extern "C" fn kryos_db_bind_int(stmt: i64, idx: i64, val: i64) -> i32 {
    if idx < 1 {
        return -1;
    }
    let mut guard = stmt_map().lock().unwrap();
    match guard.get_mut(&stmt) {
        Some(ps) => {
            ps.binds.insert(idx as usize, Value::Integer(val));
            0
        }
        None => -1,
    }
}

/// Bind SQL NULL to 1-based parameter index `idx` of `stmt`.
/// Returns 0 on success, -1 if the handle is invalid.
#[no_mangle]
pub unsafe extern "C" fn kryos_db_bind_null(stmt: i64, idx: i64) -> i32 {
    if idx < 1 {
        return -1;
    }
    let mut guard = stmt_map().lock().unwrap();
    match guard.get_mut(&stmt) {
        Some(ps) => {
            ps.binds.insert(idx as usize, Value::Null);
            0
        }
        None => -1,
    }
}

/// Execute a bound DDL/DML statement (INSERT/UPDATE/DELETE/CREATE).
/// Returns the number of rows changed, or -1 on error.
#[no_mangle]
pub unsafe extern "C" fn kryos_db_stmt_execute(stmt: i64) -> i64 {
    // Snapshot the SQL + binds, then release the stmt lock before taking the
    // conn lock (consistent lock ordering avoids deadlock).
    let (conn_id, sql, binds) = {
        let guard = stmt_map().lock().unwrap();
        match guard.get(&stmt) {
            Some(ps) => (ps.conn_id, ps.sql.clone(), ps.binds.clone()),
            None => return -1,
        }
    };
    let mut conn_guard = conn_map().lock().unwrap();
    let conn_obj = match conn_guard.get_mut(&conn_id) {
        Some(c) => c,
        None => return -1,
    };
    let params = ordered_params(&binds);
    match conn_obj.execute(&sql, params_from_iter(params.iter())) {
        Ok(n) => n as i64,
        Err(_) => -1,
    }
}

/// Execute a bound SELECT, collecting all rows into a cursor (same shape as
/// `kryos_db_prepare`). Returns a positive cursor handle, or -1 on error.
/// Read it back with the existing kryos_db_step / col_* / finalize functions.
#[no_mangle]
pub unsafe extern "C" fn kryos_db_stmt_query(stmt: i64) -> i64 {
    let (conn_id, sql, binds) = {
        let guard = stmt_map().lock().unwrap();
        match guard.get(&stmt) {
            Some(ps) => (ps.conn_id, ps.sql.clone(), ps.binds.clone()),
            None => return -1,
        }
    };
    let guard = conn_map().lock().unwrap();
    let conn_obj = match guard.get(&conn_id) {
        Some(c) => c,
        None => return -1,
    };
    let mut prepared = match conn_obj.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let col_count = prepared.column_count();
    let params = ordered_params(&binds);
    let mut rows: Vec<Vec<Value>> = Vec::new();
    {
        let mut iter = match prepared.query(params_from_iter(params.iter())) {
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

/// Free a prepared statement.  Returns 0 on success, -1 if unknown.
#[no_mangle]
pub unsafe extern "C" fn kryos_db_stmt_finalize(stmt: i64) -> i32 {
    if stmt_map().lock().unwrap().remove(&stmt).is_some() {
        0
    } else {
        -1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn exec(conn: i64, sql: &str) -> i64 {
        kryos_db_exec(conn, sql.as_ptr(), sql.len())
    }

    /// The defining test: a classic injection payload bound as a parameter
    /// must be stored as the exact literal string, and the targeted table
    /// must survive (proving the payload was data, not executed SQL).
    #[test]
    fn bound_param_stores_injection_payload_literally() {
        unsafe {
            let conn = kryos_db_open_memory();
            assert!(conn > 0);
            assert!(exec(conn, "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)") >= 0);

            let payload = "Robert'); DROP TABLE t;--";
            let ins = "INSERT INTO t (name) VALUES (?1)";
            let stmt = kryos_db_stmt_prepare(conn, ins.as_ptr(), ins.len());
            assert!(stmt > 0);
            assert_eq!(kryos_db_bind_text(stmt, 1, payload.as_ptr(), payload.len()), 0);
            assert_eq!(kryos_db_stmt_execute(stmt), 1);
            assert_eq!(kryos_db_stmt_finalize(stmt), 0);

            // The table still exists and holds exactly the literal payload.
            let sel = "SELECT name FROM t";
            let qstmt = kryos_db_stmt_prepare(conn, sel.as_ptr(), sel.len());
            assert!(qstmt > 0);
            let cur = kryos_db_stmt_query(qstmt);
            assert!(cur > 0);
            assert_eq!(kryos_db_step(cur), 1);
            let n = kryos_db_col_text_len(cur, 0);
            assert!(n > 0);
            let mut buf = vec![0u8; n as usize];
            let written = kryos_db_col_text_copy(cur, 0, buf.as_mut_ptr(), buf.len());
            let got = std::str::from_utf8(&buf[..written as usize]).unwrap();
            assert_eq!(got, payload);
            assert_eq!(kryos_db_step(cur), 0); // exactly one row
            kryos_db_finalize(cur);
            kryos_db_stmt_finalize(qstmt);
            kryos_db_close(conn);
        }
    }

    #[test]
    fn bind_int_and_null_roundtrip() {
        unsafe {
            let conn = kryos_db_open_memory();
            assert!(exec(conn, "CREATE TABLE n (a INTEGER, b TEXT)") >= 0);
            let ins = "INSERT INTO n (a, b) VALUES (?1, ?2)";
            let stmt = kryos_db_stmt_prepare(conn, ins.as_ptr(), ins.len());
            assert_eq!(kryos_db_bind_int(stmt, 1, 42), 0);
            assert_eq!(kryos_db_bind_null(stmt, 2), 0);
            assert_eq!(kryos_db_stmt_execute(stmt), 1);
            kryos_db_stmt_finalize(stmt);

            let sel = "SELECT a, b FROM n WHERE a = ?1";
            let qstmt = kryos_db_stmt_prepare(conn, sel.as_ptr(), sel.len());
            assert_eq!(kryos_db_bind_int(qstmt, 1, 42), 0);
            let cur = kryos_db_stmt_query(qstmt);
            assert_eq!(kryos_db_step(cur), 1);
            assert_eq!(kryos_db_col_int(cur, 0), 42);
            assert_eq!(kryos_db_col_text_len(cur, 1), 0); // NULL -> zero length
            assert_eq!(kryos_db_step(cur), 0);
            kryos_db_finalize(cur);
            kryos_db_stmt_finalize(qstmt);
            kryos_db_close(conn);
        }
    }

    #[test]
    fn invalid_handles_return_error() {
        unsafe {
            assert_eq!(kryos_db_bind_int(999999, 1, 1), -1);
            assert_eq!(kryos_db_stmt_execute(999999), -1);
            assert_eq!(kryos_db_stmt_query(999999), -1);
            assert_eq!(kryos_db_stmt_finalize(999999), -1);
        }
    }
}
