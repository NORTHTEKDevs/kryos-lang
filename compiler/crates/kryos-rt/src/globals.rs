//! Mutable module-level globals.
//!
//! Each top-level `let mut NAME: TY = expr` becomes a slot in this runtime
//! registry. The MIR lowering emits a synthesized initializer that runs at
//! the start of `main` and calls [`kryos_global_set`] with the result of the
//! initializer expression. Every subsequent read of `NAME` from any function
//! lowers to [`kryos_global_get`], and every assignment `NAME = expr` lowers
//! to [`kryos_global_set`].
//!
//! Values are stored as raw `i64` slots. Kryos already uses i64 as the
//! uniform ABI for all reference-typed values (string handles, array handles,
//! struct heap pointers cast to i64, JSON node ids, ...) and for the
//! primitive numeric types (i64, bool). For f64 globals the bit pattern is
//! transmuted on entry/exit.
//!
//! This file is part of the Kryos language runtime and the slot table is
//! process-wide. Concurrent access is guarded by a `Mutex`; the registry is
//! lock-free on the fast path because reads/writes do not block on each
//! other for different slots (they just contend for the lock briefly).
//!
//! Keys are Kryos string handles (i64). The runtime decodes the bytes once
//! and uses the resulting `String` as the HashMap key. The handle itself is
//! never stored (its lifetime is independent).

use crate::string::KryosString;
use std::collections::HashMap;
use std::sync::Mutex;

static GLOBALS: Mutex<Option<HashMap<String, i64>>> = Mutex::new(None);

fn with_globals<R>(f: impl FnOnce(&mut HashMap<String, i64>) -> R) -> R {
    let mut guard = GLOBALS.lock().expect("kryos globals mutex poisoned");
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    f(guard.as_mut().unwrap())
}

/// Decode a Kryos string handle to a Rust `String`.
unsafe fn handle_to_string(handle: i64) -> String {
    if handle == 0 {
        return String::new();
    }
    let s = handle as *const KryosString;
    if s.is_null() {
        return String::new();
    }
    let ptr = (*s).data as *const u8;
    let len = (*s).len as usize;
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    let bytes = std::slice::from_raw_parts(ptr, len);
    std::str::from_utf8(bytes).unwrap_or("").to_string()
}

/// Store `value` for global named by the Kryos string handle `name_handle`.
///
/// Returns the stored value so the JIT can keep a uniform i64 return
/// signature for all extern calls. Callers discard the result.
#[no_mangle]
pub extern "C" fn kryos_global_set(name_handle: i64, value: i64) -> i64 {
    let name = unsafe { handle_to_string(name_handle) };
    if name.is_empty() {
        return value;
    }
    with_globals(|m| {
        m.insert(name, value);
    });
    value
}

/// Fetch the i64 slot for the global named by `name_handle`. Returns 0 if
/// the global has not been set yet (callers should ensure the initializer
/// has run before any reads).
#[no_mangle]
pub extern "C" fn kryos_global_get(name_handle: i64) -> i64 {
    let name = unsafe { handle_to_string(name_handle) };
    if name.is_empty() {
        return 0;
    }
    with_globals(|m| m.get(&name).copied().unwrap_or(0))
}

/// True (1) if the global has been set, else 0.
#[no_mangle]
pub extern "C" fn kryos_global_has(name_handle: i64) -> i64 {
    let name = unsafe { handle_to_string(name_handle) };
    if name.is_empty() {
        return 0;
    }
    with_globals(|m| if m.contains_key(&name) { 1 } else { 0 })
}

/// Test-only: clear every global. Useful for unit tests that share a
/// process and need a clean slate between runs.
#[doc(hidden)]
pub fn _kryos_globals_clear_for_tests() {
    with_globals(|m| m.clear());
}
