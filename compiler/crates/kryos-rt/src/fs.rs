//! File I/O for the Kryos runtime.

use std::fs;

/// Safely convert a raw byte pointer and length to a `&str`.
/// Returns `""` if the pointer is null or the bytes are not valid UTF-8.
unsafe fn bytes_to_str<'a>(ptr: *const u8, len: usize) -> &'a str {
    let slice = std::slice::from_raw_parts(ptr, len);
    std::str::from_utf8(slice).unwrap_or("")
}

/// Read a file's contents as a KryosString. Returns null on error.
#[no_mangle]
pub unsafe extern "C" fn kryos_fs_read(
    path_ptr: *const u8,
    path_len: usize,
) -> *mut crate::string::KryosString {
    if path_ptr.is_null() {
        return std::ptr::null_mut();
    }
    let path = bytes_to_str(path_ptr, path_len);
    match fs::read_to_string(path) {
        Ok(contents) => crate::string::kryos_string_new(contents.as_ptr(), contents.len() as i64),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Write data to a file. Returns 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn kryos_fs_write(
    path_ptr: *const u8,
    path_len: usize,
    data_ptr: *const u8,
    data_len: usize,
) -> i64 {
    if path_ptr.is_null() || data_ptr.is_null() {
        return -1;
    }
    let path = bytes_to_str(path_ptr, path_len);
    let data = bytes_to_str(data_ptr, data_len);
    match fs::write(path, data) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

/// Check if a file exists. Returns 1 if exists, 0 otherwise.
#[no_mangle]
pub extern "C" fn kryos_fs_exists(path_ptr: *const u8, path_len: usize) -> i64 {
    if path_ptr.is_null() {
        return 0;
    }
    let path = unsafe { bytes_to_str(path_ptr, path_len) };
    if std::path::Path::new(path).exists() { 1 } else { 0 }
}

/// Delete a file. Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_fs_delete(path_ptr: *const u8, path_len: usize) -> i64 {
    if path_ptr.is_null() {
        return -1;
    }
    let path = unsafe { bytes_to_str(path_ptr, path_len) };
    match fs::remove_file(path) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}
