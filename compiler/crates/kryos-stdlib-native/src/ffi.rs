//! Foreign Function Interface (FFI) runtime helpers.
//!
//! Provides:
//!   - String <-> raw pointer conversion (Kryos strings are NUL-terminated,
//!     so `cstr()` is a zero-copy reinterpret of `KryosString::data`).
//!   - Dynamic library loading (`dlopen`/`dlsym`/`dlclose`).
//!   - Indirect function-pointer invocation with up to 6 i64 args
//!     (`dlcall0` ... `dlcall6`).
//!
//! Together these let any Kryos program use any shared library on the system
//! at runtime, without rebuilding the compiler or adding link flags.
//!
//! Pointer values are passed across the FFI as `i64`. Null is `0`.

use std::ffi::{c_void, CString};

use kryos_rt::string::KryosString;

/// Return a `*const u8` pointing at the NUL-terminated bytes of a Kryos string.
///
/// Returns 0 if the input handle is null. The caller MUST NOT keep the pointer
/// past the lifetime of the source KryosString.
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_cstr(s: *const KryosString) -> i64 {
    if s.is_null() {
        return 0;
    }
    (*s).data as i64
}

/// Return the length (in bytes, excluding the NUL) of a Kryos string.
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_strlen(s: *const KryosString) -> i64 {
    if s.is_null() {
        return 0;
    }
    (*s).len
}

/// Build a new Kryos string by copying `len` bytes from a raw pointer.
/// If `len < 0`, treats `ptr` as a C string and uses `strlen`.
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_string_from_ptr(ptr: i64, len: i64) -> i64 {
    if ptr == 0 {
        return 0;
    }
    let raw = ptr as *const u8;
    let actual_len: i64 = if len < 0 {
        libc_strlen(raw) as i64
    } else {
        len
    };
    let p = kryos_rt::string::kryos_string_new(raw, actual_len);
    if p.is_null() { 0 } else { p as i64 }
}

/// Local strlen so we don't have to link a separate libc-shim crate.
unsafe fn libc_strlen(mut p: *const u8) -> usize {
    let mut n = 0usize;
    while !p.is_null() && *p != 0 {
        n += 1;
        p = p.add(1);
    }
    n
}

// ===========================================================================
// Dynamic library loading
// ===========================================================================

/// `dlopen(name)` -> handle (i64), or 0 on failure.
///
/// On Linux/macOS this calls `libc::dlopen` with `RTLD_NOW | RTLD_GLOBAL`.
/// On Windows this calls `LoadLibraryA`. The handle should be passed to
/// `kryos_ffi_dlsym` to resolve symbols and to `kryos_ffi_dlclose` to free.
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_dlopen(name: *const KryosString) -> i64 {
    if name.is_null() {
        return 0;
    }
    let bytes = std::slice::from_raw_parts((*name).data, (*name).len as usize);
    let cs = match CString::new(bytes) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    platform::ffi_dlopen(cs.as_ptr()) as i64
}

/// `dlsym(handle, name)` -> function pointer (i64), or 0 on failure.
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_dlsym(handle: i64, name: *const KryosString) -> i64 {
    if handle == 0 || name.is_null() {
        return 0;
    }
    let bytes = std::slice::from_raw_parts((*name).data, (*name).len as usize);
    let cs = match CString::new(bytes) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    platform::ffi_dlsym(handle as *mut c_void, cs.as_ptr()) as i64
}

/// `dlclose(handle)` -> 0 on success, non-zero on error.
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_dlclose(handle: i64) -> i64 {
    if handle == 0 {
        return 0;
    }
    platform::ffi_dlclose(handle as *mut c_void) as i64
}

// ===========================================================================
// Indirect calls
// ===========================================================================
//
// These take a function pointer (i64) plus up to 6 i64 args and a return-type
// hint, then invoke the function via a Rust function-pointer cast. The
// SysV/Win64 ABIs guarantee that the first 6 integer args go in registers, so
// this works for the overwhelming majority of C APIs that take ints or
// pointers.
//
// Float/double args and returns are handled by separate dlcall_f* variants.

type Fn0R = unsafe extern "C" fn() -> i64;
type Fn1R = unsafe extern "C" fn(i64) -> i64;
type Fn2R = unsafe extern "C" fn(i64, i64) -> i64;
type Fn3R = unsafe extern "C" fn(i64, i64, i64) -> i64;
type Fn4R = unsafe extern "C" fn(i64, i64, i64, i64) -> i64;
type Fn5R = unsafe extern "C" fn(i64, i64, i64, i64, i64) -> i64;
type Fn6R = unsafe extern "C" fn(i64, i64, i64, i64, i64, i64) -> i64;
type Fn7R = unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64) -> i64;
type Fn8R = unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64) -> i64;

type Fn0V = unsafe extern "C" fn();
type Fn1V = unsafe extern "C" fn(i64);
type Fn2V = unsafe extern "C" fn(i64, i64);
type Fn3V = unsafe extern "C" fn(i64, i64, i64);
type Fn4V = unsafe extern "C" fn(i64, i64, i64, i64);
type Fn5V = unsafe extern "C" fn(i64, i64, i64, i64, i64);
type Fn6V = unsafe extern "C" fn(i64, i64, i64, i64, i64, i64);
type Fn7V = unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64);
#[allow(dead_code)]
type Fn8V = unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64);

macro_rules! dlcall_arity {
    ($name:ident, $tret:ty, $tvoid:ty, $($arg:ident: $ty:ty),*) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name(fp: i64, $($arg: $ty),*) -> i64 {
            if fp == 0 { return 0 }
            let f: $tret = std::mem::transmute(fp as *const ());
            f($($arg),*)
        }
    };
}

dlcall_arity!(kryos_ffi_dlcall0, Fn0R, Fn0V,);
dlcall_arity!(kryos_ffi_dlcall1, Fn1R, Fn1V, a1: i64);
dlcall_arity!(kryos_ffi_dlcall2, Fn2R, Fn2V, a1: i64, a2: i64);
dlcall_arity!(kryos_ffi_dlcall3, Fn3R, Fn3V, a1: i64, a2: i64, a3: i64);
dlcall_arity!(kryos_ffi_dlcall4, Fn4R, Fn4V, a1: i64, a2: i64, a3: i64, a4: i64);
dlcall_arity!(kryos_ffi_dlcall5, Fn5R, Fn5V, a1: i64, a2: i64, a3: i64, a4: i64, a5: i64);
dlcall_arity!(kryos_ffi_dlcall6, Fn6R, Fn6V, a1: i64, a2: i64, a3: i64, a4: i64, a5: i64, a6: i64);
dlcall_arity!(kryos_ffi_dlcall7, Fn7R, Fn7V, a1: i64, a2: i64, a3: i64, a4: i64, a5: i64, a6: i64, a7: i64);
dlcall_arity!(kryos_ffi_dlcall8, Fn8R, Fn8V, a1: i64, a2: i64, a3: i64, a4: i64, a5: i64, a6: i64, a7: i64, a8: i64);

// Void-return variants — for fns whose return value we don't want to bind.
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_dlcallv0(fp: i64) {
    if fp == 0 { return }
    let f: Fn0V = std::mem::transmute(fp as *const ());
    f();
}
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_dlcallv1(fp: i64, a1: i64) {
    if fp == 0 { return }
    let f: Fn1V = std::mem::transmute(fp as *const ());
    f(a1);
}
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_dlcallv2(fp: i64, a1: i64, a2: i64) {
    if fp == 0 { return }
    let f: Fn2V = std::mem::transmute(fp as *const ());
    f(a1, a2);
}
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_dlcallv3(fp: i64, a1: i64, a2: i64, a3: i64) {
    if fp == 0 { return }
    let f: Fn3V = std::mem::transmute(fp as *const ());
    f(a1, a2, a3);
}
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_dlcallv4(fp: i64, a1: i64, a2: i64, a3: i64, a4: i64) {
    if fp == 0 { return }
    let f: Fn4V = std::mem::transmute(fp as *const ());
    f(a1, a2, a3, a4);
}
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_dlcallv5(fp: i64, a1: i64, a2: i64, a3: i64, a4: i64, a5: i64) {
    if fp == 0 { return }
    let f: Fn5V = std::mem::transmute(fp as *const ());
    f(a1, a2, a3, a4, a5);
}
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_dlcallv6(fp: i64, a1: i64, a2: i64, a3: i64, a4: i64, a5: i64, a6: i64) {
    if fp == 0 { return }
    let f: Fn6V = std::mem::transmute(fp as *const ());
    f(a1, a2, a3, a4, a5, a6);
}
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_dlcallv7(fp: i64, a1: i64, a2: i64, a3: i64, a4: i64, a5: i64, a6: i64, a7: i64) {
    if fp == 0 { return }
    let f: Fn7V = std::mem::transmute(fp as *const ());
    f(a1, a2, a3, a4, a5, a6, a7);
}

// ===========================================================================
// Float-arg/return helpers — for graphics APIs that take coords as f32/f64.
// ===========================================================================

/// Call f(a1: f64, a2: f64, a3: f64, a4: f64) -> void.
/// Useful for raylib's DrawLine, DrawCircle, etc. after f32->f64 conversion.
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_dlcallv_4f(fp: i64, a1: f64, a2: f64, a3: f64, a4: f64) {
    if fp == 0 { return }
    type F = unsafe extern "C" fn(f64, f64, f64, f64);
    let f: F = std::mem::transmute(fp as *const ());
    f(a1, a2, a3, a4);
}

/// Call f(a1: f32, a2: f32, a3: f32, a4: f32) -> void.
/// Each argument is passed as the i64 bit-pattern of the f32 value.
/// This correctly passes float values in xmm registers for functions like glClearColor.
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_dlcallv_4f32(fp: i64, b1: i64, b2: i64, b3: i64, b4: i64) {
    if fp == 0 { return }
    let a1 = f32::from_bits(b1 as u32);
    let a2 = f32::from_bits(b2 as u32);
    let a3 = f32::from_bits(b3 as u32);
    let a4 = f32::from_bits(b4 as u32);
    type F = unsafe extern "C" fn(f32, f32, f32, f32);
    let f: F = std::mem::transmute(fp as *const ());
    f(a1, a2, a3, a4);
}

/// Call f(a1: f32, a2: f32, a3: f32) -> void.
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_dlcallv_3f32(fp: i64, b1: i64, b2: i64, b3: i64) {
    if fp == 0 { return }
    let a1 = f32::from_bits(b1 as u32);
    let a2 = f32::from_bits(b2 as u32);
    let a3 = f32::from_bits(b3 as u32);
    type F = unsafe extern "C" fn(f32, f32, f32);
    let f: F = std::mem::transmute(fp as *const ());
    f(a1, a2, a3);
}

/// Allocate `n` bytes on the C heap (libc malloc). Returns 0 on failure.
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_malloc(n: i64) -> i64 {
    if n <= 0 { return 0 }
    let layout = match std::alloc::Layout::from_size_align(n as usize, 8) {
        Ok(l) => l,
        Err(_) => return 0,
    };
    let p = std::alloc::alloc(layout);
    p as i64
}

/// Free a pointer previously returned by kryos_ffi_malloc.
/// NOTE: caller must pass the same `n` used for allocation.
#[no_mangle]
pub unsafe extern "C" fn kryos_ffi_free(ptr: i64, n: i64) {
    if ptr == 0 || n <= 0 { return }
    let layout = match std::alloc::Layout::from_size_align(n as usize, 8) {
        Ok(l) => l,
        Err(_) => return,
    };
    std::alloc::dealloc(ptr as *mut u8, layout);
}

// Read/write primitive values at a raw pointer.
#[no_mangle] pub unsafe extern "C" fn kryos_ffi_read_i8(p: i64) -> i64  { if p == 0 { 0 } else { *(p as *const i8) as i64 } }
#[no_mangle] pub unsafe extern "C" fn kryos_ffi_read_i16(p: i64) -> i64 { if p == 0 { 0 } else { *(p as *const i16) as i64 } }
#[no_mangle] pub unsafe extern "C" fn kryos_ffi_read_i32(p: i64) -> i64 { if p == 0 { 0 } else { *(p as *const i32) as i64 } }
#[no_mangle] pub unsafe extern "C" fn kryos_ffi_read_i64(p: i64) -> i64 { if p == 0 { 0 } else { *(p as *const i64) } }
#[no_mangle] pub unsafe extern "C" fn kryos_ffi_read_f32(p: i64) -> f64 { if p == 0 { 0.0 } else { *(p as *const f32) as f64 } }
#[no_mangle] pub unsafe extern "C" fn kryos_ffi_read_f64(p: i64) -> f64 { if p == 0 { 0.0 } else { *(p as *const f64) } }

#[no_mangle] pub unsafe extern "C" fn kryos_ffi_write_i8(p: i64, v: i64)  { if p != 0 { *(p as *mut i8)  = v as i8 } }
#[no_mangle] pub unsafe extern "C" fn kryos_ffi_write_i16(p: i64, v: i64) { if p != 0 { *(p as *mut i16) = v as i16 } }
#[no_mangle] pub unsafe extern "C" fn kryos_ffi_write_i32(p: i64, v: i64) { if p != 0 { *(p as *mut i32) = v as i32 } }
#[no_mangle] pub unsafe extern "C" fn kryos_ffi_write_i64(p: i64, v: i64) { if p != 0 { *(p as *mut i64) = v } }
#[no_mangle] pub unsafe extern "C" fn kryos_ffi_write_f32(p: i64, v: f64) { if p != 0 { *(p as *mut f32) = v as f32 } }
#[no_mangle] pub unsafe extern "C" fn kryos_ffi_write_f64(p: i64, v: f64) { if p != 0 { *(p as *mut f64) = v } }

/// Write a 32-bit float at `p` by reinterpreting the low 32 bits of `bits` as an IEEE-754 f32.
/// This allows Kryos source to pass float bit-patterns as i64 literals.
#[no_mangle] pub unsafe extern "C" fn kryos_ffi_write_f32_bits(p: i64, bits: i64) {
    if p != 0 { *(p as *mut u32) = bits as u32 }
}

/// Write a 64-bit float at `p` by reinterpreting `bits` as an IEEE-754 f64.
#[no_mangle] pub unsafe extern "C" fn kryos_ffi_write_f64_bits(p: i64, bits: i64) {
    if p != 0 { *(p as *mut i64) = bits }
}

/// Read a 32-bit float at `p`, returning its bit pattern as an i64.
#[no_mangle] pub unsafe extern "C" fn kryos_ffi_read_f32_bits(p: i64) -> i64 {
    if p == 0 { 0 } else { *(p as *const u32) as i64 }
}

/// Read a 64-bit float at `p`, returning its bit pattern as an i64.
#[no_mangle] pub unsafe extern "C" fn kryos_ffi_read_f64_bits(p: i64) -> i64 {
    if p == 0 { 0 } else { *(p as *const i64) }
}

// ===========================================================================
// Platform shims
// ===========================================================================

#[cfg(unix)]
mod platform {
    use std::os::raw::{c_char, c_int, c_void};

    const RTLD_NOW: c_int = 2;
    const RTLD_GLOBAL: c_int = 0x100;

    extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
    }

    pub unsafe fn ffi_dlopen(name: *const c_char) -> *mut c_void {
        dlopen(name, RTLD_NOW | RTLD_GLOBAL)
    }
    pub unsafe fn ffi_dlsym(handle: *mut c_void, sym: *const c_char) -> *mut c_void {
        dlsym(handle, sym)
    }
    pub unsafe fn ffi_dlclose(handle: *mut c_void) -> i32 {
        dlclose(handle)
    }
}

#[cfg(windows)]
mod platform {
    use std::os::raw::{c_char, c_int, c_void};

    extern "system" {
        fn LoadLibraryA(name: *const c_char) -> *mut c_void;
        fn GetProcAddress(handle: *mut c_void, name: *const c_char) -> *mut c_void;
        fn FreeLibrary(handle: *mut c_void) -> c_int;
    }

    pub unsafe fn ffi_dlopen(name: *const c_char) -> *mut c_void {
        LoadLibraryA(name)
    }
    pub unsafe fn ffi_dlsym(handle: *mut c_void, sym: *const c_char) -> *mut c_void {
        GetProcAddress(handle, sym)
    }
    pub unsafe fn ffi_dlclose(handle: *mut c_void) -> i32 {
        // FreeLibrary returns nonzero on success; flip it so 0 == success.
        if FreeLibrary(handle) != 0 { 0 } else { 1 }
    }
}
