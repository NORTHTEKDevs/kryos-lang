//! Kryos Runtime Library
//!
//! Core runtime services for compiled Kryos programs. All public functions
//! are `#[no_mangle] extern "C"` for linking from compiled Kryos code.
//!
//! This crate has zero dependencies on other kryos crates.
//!
//! # Safety
//!
//! All public functions in this crate are FFI boundary functions called
//! exclusively from Kryos-compiled machine code via the C ABI. They accept
//! raw pointers and integer handles because they operate below Rust's safety
//! model. Pointer validity is guaranteed by the Kryos compiler's ownership
//! analysis and codegen, not by Rust's borrow checker.
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::missing_safety_doc)]

pub mod alloc;
pub mod arc;
pub mod array;
pub mod actor;
pub mod builtins;
pub mod channel;
pub mod exception;
pub mod map;
pub mod panic;
pub mod spawn;
pub mod string;
pub mod tensor;
pub mod trace;
