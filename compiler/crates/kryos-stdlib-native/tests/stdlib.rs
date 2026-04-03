//! Integration tests for kryos-stdlib-native.

#[cfg(feature = "crypto")]
use kryos_stdlib_native::crypto::{kryos_random_bytes, kryos_sha256};
use kryos_stdlib_native::datetime::{kryos_time_now_millis, kryos_time_now_secs};
use kryos_stdlib_native::fs::{kryos_file_remove, kryos_path_exists};
use kryos_stdlib_native::io::{
    kryos_file_close, kryos_file_open, kryos_file_read, kryos_file_write, kryos_stdout_write,
};
use kryos_stdlib_native::process::kryos_env_get;
use kryos_stdlib_native::re::{kryos_regex_drop, kryos_regex_is_match, kryos_regex_new};
use kryos_stdlib_native::sync_prims::{
    kryos_mutex_drop, kryos_mutex_lock, kryos_mutex_new, kryos_mutex_unlock,
};

#[test]
fn test_stdout_write() {
    let msg = b"kryos stdout test\n";
    let ret = kryos_stdout_write(msg.as_ptr(), msg.len());
    assert_eq!(ret, msg.len() as i64);
}

#[cfg(feature = "crypto")]
#[test]
fn test_sha256_empty_string() {
    // SHA-256 of "" = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
    let data: &[u8] = b"";
    let mut out = [0u8; 32];
    let ret = kryos_sha256(data.as_ptr(), data.len(), out.as_mut_ptr());
    assert_eq!(ret, 0);

    let expected: [u8; 32] = [
        0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
        0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
        0x78, 0x52, 0xb8, 0x55,
    ];
    assert_eq!(out, expected);
}

#[cfg(feature = "crypto")]
#[test]
fn test_random_bytes_non_zero() {
    let mut buf = [0u8; 64];
    let ret = kryos_random_bytes(buf.as_mut_ptr(), buf.len());
    assert_eq!(ret, 0);
    // It's astronomically unlikely that 64 random bytes are all zero.
    assert!(buf.iter().any(|&b| b != 0));
}

#[test]
fn test_time_now_secs_reasonable() {
    let secs = kryos_time_now_secs();
    // Must be after 2023-11-14 (1700000000)
    assert!(secs > 1_700_000_000, "time_now_secs returned {secs}");
}

#[test]
fn test_time_now_millis_greater_than_secs() {
    let secs = kryos_time_now_secs();
    let millis = kryos_time_now_millis();
    assert!(millis > secs * 1000, "millis={millis}, secs*1000={}", secs * 1000);
}

#[test]
fn test_path_exists_current_dir() {
    let path = b".";
    let ret = kryos_path_exists(path.as_ptr(), path.len());
    assert_eq!(ret, 1);
}

#[test]
fn test_path_exists_nonexistent() {
    let path = b"/definitely/does/not/exist/kryos_test_42";
    let ret = kryos_path_exists(path.as_ptr(), path.len());
    assert_eq!(ret, 0);
}

#[test]
fn test_env_get_path() {
    let key = b"PATH";
    let mut buf = [0u8; 4096];
    let ret = kryos_env_get(key.as_ptr(), key.len(), buf.as_mut_ptr(), buf.len());
    assert!(ret > 0, "env_get for PATH returned {ret}");
}

#[test]
fn test_mutex_lifecycle() {
    let m = kryos_mutex_new();
    assert!(!m.is_null());

    let lock_ret = kryos_mutex_lock(m);
    assert_eq!(lock_ret, 0);

    let unlock_ret = kryos_mutex_unlock(m);
    assert_eq!(unlock_ret, 0);

    kryos_mutex_drop(m);
}

#[test]
fn test_regex_lifecycle() {
    let pattern = b"^hello\\s+world$";
    let re = kryos_regex_new(pattern.as_ptr(), pattern.len());
    assert!(!re.is_null());

    let text_match = b"hello   world";
    let ret = kryos_regex_is_match(re, text_match.as_ptr(), text_match.len());
    assert_eq!(ret, 1);

    let text_no_match = b"goodbye world";
    let ret = kryos_regex_is_match(re, text_no_match.as_ptr(), text_no_match.len());
    assert_eq!(ret, 0);

    kryos_regex_drop(re);
}

#[test]
fn test_file_roundtrip() {
    // Create a temp file path using process id to avoid collisions
    let pid = std::process::id();
    let path_str = format!("kryos_test_roundtrip_{pid}.tmp");
    let path = path_str.as_bytes();

    // Open for writing
    let fd = kryos_file_open(path.as_ptr(), path.len(), 1);
    assert!(fd >= 3, "file_open for write returned {fd}");

    // Write data
    let data = b"Hello from Kryos stdlib native!";
    let written = kryos_file_write(fd, data.as_ptr(), data.len());
    assert_eq!(written, data.len() as i64);

    // Close
    let ret = kryos_file_close(fd);
    assert_eq!(ret, 0);

    // Open for reading
    let fd = kryos_file_open(path.as_ptr(), path.len(), 0);
    assert!(fd >= 3, "file_open for read returned {fd}");

    // Read data
    let mut buf = [0u8; 256];
    let read_len = kryos_file_read(fd, buf.as_mut_ptr(), buf.len());
    assert_eq!(read_len, data.len() as i64);
    assert_eq!(&buf[..data.len()], data);

    // Close
    let ret = kryos_file_close(fd);
    assert_eq!(ret, 0);

    // Clean up
    let ret = kryos_file_remove(path.as_ptr(), path.len());
    assert_eq!(ret, 0);
}
