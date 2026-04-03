//! Process management for the Kryos native stdlib.
//!
//! Provides environment variable access and process exit.

/// Reads the environment variable named by `key[0..key_len]` into `val_buf`.
///
/// Returns the length of the value on success (may exceed `val_buf_len` if the
/// buffer is too small — in that case, only `val_buf_len` bytes are written).
/// Returns -1 if the variable is not found or the key is invalid UTF-8.
#[no_mangle]
pub extern "C" fn kryos_env_get(
    key_ptr: *const u8,
    key_len: usize,
    val_buf: *mut u8,
    val_buf_len: usize,
) -> i64 {
    if key_ptr.is_null() {
        return -1;
    }
    let key = unsafe { std::slice::from_raw_parts(key_ptr, key_len) };
    let key = match std::str::from_utf8(key) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    match std::env::var(key) {
        Ok(val) => {
            let val_bytes = val.as_bytes();
            let copy_len = val_bytes.len().min(val_buf_len);
            if !val_buf.is_null() && copy_len > 0 {
                let out = unsafe { std::slice::from_raw_parts_mut(val_buf, copy_len) };
                out.copy_from_slice(&val_bytes[..copy_len]);
            }
            val_bytes.len() as i64
        }
        Err(_) => -1,
    }
}

/// Terminates the process with the given exit code. Never returns.
#[no_mangle]
pub extern "C" fn kryos_process_exit(code: i32) -> ! {
    std::process::exit(code)
}
