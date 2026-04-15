//! Kryos linker — system linker integration for producing final binaries.
//!
//! This crate handles:
//! - Target triple detection and parsing (arch, OS, environment)
//! - Invoking the appropriate system linker (cc, link.exe, wasm-ld)
//! - Constructing correct command lines for Unix, Windows/MSVC, and WASM targets

pub mod linker;
pub mod target;

pub use linker::{
    build_command, command_to_args, find_system_linker, link, LinkError, LinkType, LinkerConfig,
};
pub use target::{Arch, Env, Os, Target};
