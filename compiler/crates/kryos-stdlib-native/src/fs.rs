//! Filesystem operations for the Kryos native stdlib.
//!
//! Provides path existence checks, directory creation, and file removal.

/// Checks whether a path exists.
///
/// Returns 1 if the path exists, 0 if it does not, -1 on error (null/invalid UTF-8).
#[no_mangle]
pub extern "C" fn kryos_path_exists(path_ptr: *const u8, path_len: usize) -> i32 {
    if path_ptr.is_null() {
        return -1;
    }
    let path = unsafe { std::slice::from_raw_parts(path_ptr, path_len) };
    let path = match std::str::from_utf8(path) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    if std::path::Path::new(path).exists() {
        1
    } else {
        0
    }
}

/// Creates a directory (and all parent directories) at the given path.
///
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_dir_create(path_ptr: *const u8, path_len: usize) -> i32 {
    if path_ptr.is_null() {
        return -1;
    }
    let path = unsafe { std::slice::from_raw_parts(path_ptr, path_len) };
    let path = match std::str::from_utf8(path) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    match std::fs::create_dir_all(path) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Removes a file at the given path.
///
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_file_remove(path_ptr: *const u8, path_len: usize) -> i32 {
    if path_ptr.is_null() {
        return -1;
    }
    let path = unsafe { std::slice::from_raw_parts(path_ptr, path_len) };
    let path = match std::str::from_utf8(path) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    match std::fs::remove_file(path) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}
