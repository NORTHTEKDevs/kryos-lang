//! Kryos linker — system linker integration for producing final binaries.
//!
//! This crate handles:
//! - Target triple detection and parsing (arch, OS, environment)
//! - Invoking the appropriate system linker (cc, link.exe, wasm-ld)
//! - Constructing correct command lines for Unix, Windows/MSVC, and WASM targets

pub mod target;
pub mod linker;

pub use target::{Target, Arch, Os, Env};
pub use linker::{
    LinkerConfig, LinkType, LinkError,
    link, find_system_linker, build_command, command_to_args,
};
