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
/// Returns a newly allocated string, or null if unavailable. Caller owns the pointer.
#[no_mangle]
pub extern "C" fn kryos_env_home() -> *mut u8 {
    match std::env::var_os("HOME") {
        Some(home) => {
            let home_str = home.to_string_lossy();
            match CString::new(home_str.into_owned()) {
                Ok(cstr) => cstr.into_raw() as *mut u8,
                Err(_) => std::ptr::null_mut(),
            }
        }
        None => {
            #[cfg(target_os = "windows")]
            {
                match std::env::var_os("USERPROFILE") {
                    Some(home) => {
                        let home_str = home.to_string_lossy();
                        match CString::new(home_str.into_owned()) {
                            Ok(cstr) => cstr.into_raw() as *mut u8,
                            Err(_) => std::ptr::null_mut(),
                        }
                    }
                    None => std::ptr::null_mut(),
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                std::ptr::null_mut()
            }
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
