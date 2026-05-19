//! `kryos-stdlib-native` — Rust FFI layer providing syscall-backed stdlib functions
//! for compiled Kryos programs.
//!
//! Every public function in the submodules is `#[no_mangle] pub extern "C"` so
//! compiled Kryos object code can link directly against this crate's static library.
//!
//! # Cargo features
//!
//! - `full` (default) — enables `crypto`, `sqlite`, `tls`, `postgres`, `http2`.
//!   This is the recommended variant for desktop / server targets.
//! - `minimal` — explicit no-extras alias; equivalent to building with
//!   `--no-default-features`. Excludes every heavy native dep (`ring`,
//!   `rusqlite`, `rustls`, `postgres`, `reqwest`). The core POSIX-backed
//!   stdlib (`fs`, `net`, `env`, `datetime`, `json`, `regex`, `math`,
//!   `term`, `process`, `websocket` framing, etc.) is still present.
//!   Functions that require crypto (e.g. `kryos_ws_accept_key_ks`) become
//!   stubs that signal failure to the caller.
//! - Individual feature flags (`crypto`, `sqlite`, `tls`, `postgres`,
//!   `http2`) can be enabled à la carte on top of `minimal` to build a
//!   target-tailored stdlib.
//!
//! Examples:
//!
//! ```text
//! # Full build (default):
//! cargo build -p kryos-stdlib-native
//!
//! # Minimal build, no external native deps:
//! cargo build -p kryos-stdlib-native --no-default-features
//!
//! # Minimal + only SQLite:
//! cargo build -p kryos-stdlib-native \
//!   --no-default-features --features sqlite
//! ```
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
pub mod backoff;
pub mod base64;
pub mod bindings;
pub mod bloom;
pub mod bytes;
pub mod circuit;
pub mod cmd;
pub mod collections;
pub mod datetime;
pub mod deque;
pub mod duration;
pub mod env;
pub mod ffi;
pub mod fs;
pub mod fuzzy;
pub mod hash;
pub mod heap;
pub mod histogram;
pub mod io;
pub mod iter;
pub mod json;
pub mod log;
pub mod lru;
pub mod math;
pub mod mathx;
pub mod net;
pub mod numfmt;
pub mod path;
pub mod pathext;
pub mod process;
pub mod queue;
pub mod rand;
pub mod random;
pub mod ratelimit;
pub mod re;
pub mod semaphore;
pub mod semver;
pub mod set;
pub mod slice_ops;
pub mod sort;
#[cfg(feature = "sqlite")]
pub mod sqlite;
pub mod stack;
pub mod stat;
pub mod strext;
pub mod string;
pub mod sync_prims;
pub mod term;
pub mod trie;
pub mod utf8;
pub mod unix_socket;
pub mod uuid;
pub mod websocket;
#[cfg(feature = "tls")]
pub mod tls;
#[cfg(feature = "postgres")]
pub mod postgres;
pub mod http2;
