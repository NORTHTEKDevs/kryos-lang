//! File path operations for the Kryos native stdlib.
//!
//! Provides path manipulation and filesystem checks.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::Path;

/// Joins two path components.
/// Returns a newly allocated string. Caller owns the pointer.
#[no_mangle]
pub extern "C" fn kryos_path_join(a: *const u8, b: *const u8) -> *mut u8 {
    if a.is_null() || b.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        let path_a = match CStr::from_ptr(a as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let path_b = match CStr::from_ptr(b as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let path = Path::new(path_a).join(path_b);
        match CString::new(path.to_string_lossy().into_owned()) {
            Ok(cstr) => cstr.into_raw() as *mut u8,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Returns the parent directory of a path.
/// Returns a newly allocated string. Caller owns the pointer.
#[no_mangle]
pub extern "C" fn kryos_path_dirname(p: *const u8) -> *mut u8 {
    if p.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        let path_str = match CStr::from_ptr(p as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let path = Path::new(path_str);
        let parent = match path.parent() {
            Some(p) => p.to_string_lossy().into_owned(),
            None => return std::ptr::null_mut(),
        };
        match CString::new(parent) {
            Ok(cstr) => cstr.into_raw() as *mut u8,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Returns the filename component of a path.
/// Returns a newly allocated string. Caller owns the pointer.
#[no_mangle]
pub extern "C" fn kryos_path_basename(p: *const u8) -> *mut u8 {
    if p.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        let path_str = match CStr::from_ptr(p as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let path = Path::new(path_str);
        let basename = match path.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => return std::ptr::null_mut(),
        };
        match CString::new(basename) {
            Ok(cstr) => cstr.into_raw() as *mut u8,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Returns the file extension (including the dot), or an empty string if none.
/// Returns a newly allocated string. Caller owns the pointer.
#[no_mangle]
pub extern "C" fn kryos_path_extension(p: *const u8) -> *mut u8 {
    if p.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        let path_str = match CStr::from_ptr(p as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let path = Path::new(path_str);
        let ext = match path.extension() {
            Some(e) => {
                let ext_str = e.to_string_lossy();
                format!(".{}", ext_str)
            }
            None => String::new(),
        };
        match CString::new(ext) {
            Ok(cstr) => cstr.into_raw() as *mut u8,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Checks if path is a file.
/// Returns 1 if file, 0 if not or error.
#[no_mangle]
pub extern "C" fn kryos_path_is_file(p: *const u8) -> i32 {
    if p.is_null() {
        return 0;
    }
    unsafe {
        let path_str = match CStr::from_ptr(p as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return 0,
        };
        match std::fs::metadata(path_str) {
            Ok(meta) => {
                if meta.is_file() {
                    1
                } else {
                    0
                }
            }
            Err(_) => 0,
        }
    }
}

/// Checks if path is a directory.
/// Returns 1 if directory, 0 if not or error.
#[no_mangle]
pub extern "C" fn kryos_path_is_dir(p: *const u8) -> i32 {
    if p.is_null() {
        return 0;
    }
    unsafe {
        let path_str = match CStr::from_ptr(p as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return 0,
        };
        match std::fs::metadata(path_str) {
            Ok(meta) => {
                if meta.is_dir() {
                    1
                } else {
                    0
                }
            }
            Err(_) => 0,
        }
    }
}

/// Canonicalizes a path to an absolute form.
/// Returns a newly allocated string. Caller owns the pointer.
#[no_mangle]
pub extern "C" fn kryos_path_absolute(p: *const u8) -> *mut u8 {
    if p.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        let path_str = match CStr::from_ptr(p as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        match std::fs::canonicalize(path_str) {
            Ok(abs_path) => match CString::new(abs_path.to_string_lossy().into_owned()) {
                Ok(cstr) => cstr.into_raw() as *mut u8,
                Err(_) => std::ptr::null_mut(),
            },
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Frees a path string allocated by path functions.
#[no_mangle]
pub extern "C" fn kryos_path_free(ptr: *mut u8) {
    if !ptr.is_null() {
        unsafe {
            drop(CString::from_raw(ptr as *mut c_char));
        }
    }
}
