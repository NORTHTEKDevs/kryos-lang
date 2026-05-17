//! Environment variable and system info access for the Kryos native stdlib.
//!
//! Provides access to environment variables, working directory, and platform info.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// Sets an environment variable.
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_env_set(key: *const u8, val: *const u8) -> i32 {
    if key.is_null() || val.is_null() {
        return -1;
    }
    unsafe {
        let key_str = match CStr::from_ptr(key as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        };
        let val_str = match CStr::from_ptr(val as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        };
        std::env::set_var(key_str, val_str);
        0
    }
}

/// Unsets an environment variable.
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn kryos_env_unset(key: *const u8) -> i32 {
    if key.is_null() {
        return -1;
    }
    unsafe {
        let key_str = match CStr::from_ptr(key as *const c_char).to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        };
        std::env::remove_var(key_str);
        0
    }
}

/// Gets the current working directory.
/// Returns a newly allocated string. Caller owns the pointer.
#[no_mangle]
pub extern "C" fn kryos_env_cwd() -> *mut u8 {
    match std::env::current_dir() {
        Ok(path) => match CString::new(path.to_string_lossy().into_owned()) {
            Ok(cstr) => cstr.into_raw() as *mut u8,
            Err(_) => std::ptr::null_mut(),
        },
        Err(_) => std::ptr::null_mut(),
    }
}

/// Gets the home directory.
///
/// On Unix, returns `$HOME`. On Windows, returns the first of
/// `%USERPROFILE%`, `%HOMEDRIVE%%HOMEPATH%`, or `$HOME` that is set
/// (Windows users rarely set `HOME`, so checking it first would force a
/// fallback for nearly every Windows program). Returns a newly allocated
/// string, or null if no home directory is discoverable. Caller owns the
/// pointer.
#[no_mangle]
pub extern "C" fn kryos_env_home() -> *mut u8 {
    fn return_cstring(s: std::ffi::OsString) -> *mut u8 {
        let owned = s.to_string_lossy().into_owned();
        match CString::new(owned) {
            Ok(cstr) => cstr.into_raw() as *mut u8,
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(home) = std::env::var_os("USERPROFILE") {
            if !home.is_empty() {
                return return_cstring(home);
            }
        }
        // Compose %HOMEDRIVE%%HOMEPATH% if both are set (legacy Windows).
        let drive = std::env::var_os("HOMEDRIVE");
        let path = std::env::var_os("HOMEPATH");
        if let (Some(d), Some(p)) = (drive, path) {
            if !d.is_empty() && !p.is_empty() {
                let mut composed = d;
                composed.push(p);
                return return_cstring(composed);
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            if !home.is_empty() {
                return return_cstring(home);
            }
        }
        std::ptr::null_mut()
    }

    #[cfg(not(target_os = "windows"))]
    {
        match std::env::var_os("HOME") {
            Some(home) if !home.is_empty() => return_cstring(home),
            _ => std::ptr::null_mut(),
        }
    }
}

/// Gets the number of command-line arguments.
/// Returns 0 when called from a library context (no argc/argv).
#[no_mangle]
pub extern "C" fn kryos_env_args_count() -> i64 {
    0
}

/// Gets the platform identifier string.
/// Returns "linux", "macos", or "windows". Caller owns the pointer.
#[no_mangle]
pub extern "C" fn kryos_env_platform() -> *mut u8 {
    let platform = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    };

    match CString::new(platform) {
        Ok(cstr) => cstr.into_raw() as *mut u8,
        Err(_) => std::ptr::null_mut(),
    }
}
