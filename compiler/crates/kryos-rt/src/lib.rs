//! Kryos Runtime Library
//!
//! Core runtime services for compiled Kryos programs. All public functions
//! are `#[no_mangle] extern "C"` for linking from compiled Kryos code.
//!
//! This crate has zero dependencies on other kryos crates.

pub mod alloc;
pub mod arc;
pub mod array;
pub mod actor;
pub mod builtins;
pub mod channel;
pub mod map;
pub mod panic;
pub mod spawn;
pub mod string;
pub mod tensor;
