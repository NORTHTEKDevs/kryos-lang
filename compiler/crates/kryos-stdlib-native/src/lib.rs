//! `kryos-stdlib-native` — Rust FFI layer providing syscall-backed stdlib functions
//! for compiled Kryos programs.
//!
//! Every public function in the submodules is `#[no_mangle] pub extern "C"` so
//! compiled Kryos object code can link directly against this crate's static library.
//!
//! # Safety
//!
//! All public functions are FFI boundary functions called exclusively from
//! Kryos-compiled machine code via the C ABI. Pointer validity is guaranteed
//! by the Kryos compiler's ownership analysis and codegen.
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::missing_safety_doc)]

#[cfg(feature = "crypto")]
pub mod crypto;
pub mod bindings;
pub mod datetime;
pub mod env;
pub mod ffi;
pub mod fs;
pub mod io;
pub mod json;
pub mod math;
pub mod net;
pub mod path;
pub mod process;
pub mod rand;
pub mod re;
#[cfg(feature = "sqlite")]
pub mod sqlite;
pub mod string;
pub mod sync_prims;
pub mod term;
#[cfg(feature = "tls")]
pub mod tls;
#[cfg(feature = "postgres")]
pub mod postgres;
pub mod http2;
