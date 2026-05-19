//! Subprocess spawning + output capture for Kryos programs.
//!
//! `kryos_cmd_run` is the workhorse: shell-out a single command, capture
//! stdout/stderr/exit code, return the bundle as a single string formatted
//! `exit_code\nstderr_len\nstderr\nstdout`. Caller parses by indexing.
//!
//! For interactive subprocesses use `std::process` (kept separate so the
//! simple capture case stays simple).

use std::io::Read;
use std::process::{Command, Stdio};

/// Run a command (split on spaces — basic shellword splitting) and write
/// the bundle into `out` (capacity `out_cap`). Format:
///
///   <exit_code>\n<stderr_len>\n<stderr_bytes><stdout_bytes>
///
/// Returns the total number of bytes written, or -1 if `out` was too
/// small (with `*needed` set), -2 on spawn failure.
#[no_mangle]
pub extern "C" fn kryos_cmd_run(
    cmd_ptr: *const u8,
    cmd_len: usize,
    out: *mut u8,
    out_cap: usize,
    needed: *mut i64,
) -> i64 {
    if cmd_ptr.is_null() || out.is_null() {
        return -1;
    }
    let cmd = unsafe { std::slice::from_raw_parts(cmd_ptr, cmd_len) };
    let cmd = match std::str::from_utf8(cmd) {
        Ok(s) => s.to_string(),
        Err(_) => return -1,
    };

    let parts = shellword_split(&cmd);
    if parts.is_empty() {
        return -2;
    }
    let mut c = Command::new(&parts[0]);
    for p in &parts[1..] {
        c.arg(p);
    }
    c.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match c.spawn() {
        Ok(c) => c,
        Err(_) => return -2,
    };

    let mut so = String::new();
    if let Some(mut s) = child.stdout.take() {
        let _ = s.read_to_string(&mut so);
    }
    let mut se = String::new();
    if let Some(mut s) = child.stderr.take() {
        let _ = s.read_to_string(&mut se);
    }
    let status = match child.wait() {
        Ok(s) => s.code().unwrap_or(-1),
        Err(_) => -1,
    };

    let header = format!("{status}\n{}\n", se.len());
    let total_len = header.len() + se.len() + so.len();
    if !needed.is_null() {
        unsafe { *needed = total_len as i64 };
    }
    if total_len > out_cap {
        return -1;
    }
    let dst = unsafe { std::slice::from_raw_parts_mut(out, total_len) };
    let mut pos = 0;
    let hb = header.as_bytes();
    dst[pos..pos + hb.len()].copy_from_slice(hb);
    pos += hb.len();
    let seb = se.as_bytes();
    dst[pos..pos + seb.len()].copy_from_slice(seb);
    pos += seb.len();
    let sob = so.as_bytes();
    dst[pos..pos + sob.len()].copy_from_slice(sob);
    total_len as i64
}

/// Minimal shellword split: respects double quotes and single quotes,
/// no escape sequences. Sufficient for command lines like
/// `cargo build --release -p kryos-cli`.
fn shellword_split(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut in_single = false;
    let mut in_double = false;
    for c in input.chars() {
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ' ' | '\t' if !in_single && !in_double => {
                if !buf.is_empty() {
                    out.push(buf.clone());
                    buf.clear();
                }
            }
            _ => buf.push(c),
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shellword_splits_quoted() {
        assert_eq!(
            shellword_split("echo \"hello world\" foo"),
            vec!["echo", "hello world", "foo"]
        );
        assert_eq!(
            shellword_split("cargo build --release -p kryos-cli"),
            vec!["cargo", "build", "--release", "-p", "kryos-cli"]
        );
    }

    #[test]
    fn shellword_empty() {
        assert!(shellword_split("").is_empty());
        assert!(shellword_split("   ").is_empty());
    }
}
