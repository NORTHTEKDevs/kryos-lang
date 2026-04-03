//! `kryos-stdlib-native` — Rust FFI layer providing syscall-backed stdlib functions
//! for compiled Kryos programs.
//!
//! Every public function in the submodules is `#[no_mangle] pub extern "C"` so
//! compiled Kryos object code can link directly against this crate's static library.

pub mod io;
pub mod net;
#[cfg(feature = "crypto")]
pub mod crypto;
pub mod process;
pub mod term;
pub mod datetime;
pub mod fs;
pub mod sync_prims;
pub mod re;
