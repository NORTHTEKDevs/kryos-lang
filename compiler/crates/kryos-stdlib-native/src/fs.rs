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

/// Renames (moves) a file or directory from one path to another.
///
/// This is an atomic rename on the same filesystem (unlike a copy+remove,
/// which can lose data if the process dies between the two steps). Returns
/// 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_fs_rename(
    from_ptr: *const u8,
    from_len: usize,
    to_ptr: *const u8,
    to_len: usize,
) -> i32 {
    if from_ptr.is_null() || to_ptr.is_null() {
        return -1;
    }
    let from = unsafe { std::slice::from_raw_parts(from_ptr, from_len) };
    let to = unsafe { std::slice::from_raw_parts(to_ptr, to_len) };
    let (from, to) = match (std::str::from_utf8(from), std::str::from_utf8(to)) {
        (Ok(f), Ok(t)) => (f, t),
        _ => return -1,
    };
    match std::fs::rename(from, to) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

#[cfg(test)]
mod rename_tests {
    use super::*;

    #[test]
    fn rename_moves_file_contents() {
        let dir = std::env::temp_dir();
        let src = dir.join("kryos_rename_src.txt");
        let dst = dir.join("kryos_rename_dst.txt");
        let _ = std::fs::remove_file(&dst);
        std::fs::write(&src, b"payload").unwrap();
        let s = src.to_str().unwrap();
        let d = dst.to_str().unwrap();
        let rc = kryos_fs_rename(s.as_ptr(), s.len(), d.as_ptr(), d.len());
        assert_eq!(rc, 0);
        assert!(!src.exists(), "source should be gone after rename");
        assert_eq!(std::fs::read(&dst).unwrap(), b"payload");
        let _ = std::fs::remove_file(&dst);
    }

    #[test]
    fn rename_missing_source_errors() {
        let dir = std::env::temp_dir();
        let src = dir.join("kryos_rename_nope_src.txt");
        let _ = std::fs::remove_file(&src);
        let dst = dir.join("kryos_rename_nope_dst.txt");
        let s = src.to_str().unwrap();
        let d = dst.to_str().unwrap();
        assert_eq!(kryos_fs_rename(s.as_ptr(), s.len(), d.as_ptr(), d.len()), -1);
    }

    #[test]
    fn rename_null_pointers_error() {
        assert_eq!(kryos_fs_rename(std::ptr::null(), 0, std::ptr::null(), 0), -1);
    }
}
