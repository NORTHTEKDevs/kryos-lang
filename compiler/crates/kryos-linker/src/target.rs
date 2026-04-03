//! Target triple handling — arch, OS, environment detection and parsing.

use std::fmt;

/// A compilation target described by architecture, operating system, and environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub arch: Arch,
    pub os: Os,
    pub env: Env,
}

/// CPU architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
    Wasm32,
}

/// Operating system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Windows,
    MacOS,
    Wasi,
    Unknown,
}

/// ABI environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Env {
    Gnu,
    Msvc,
    Musl,
    None,
}

impl Target {
    /// Detect the host target from compile-time cfg attributes.
    pub fn host() -> Self {
        let arch = if cfg!(target_arch = "x86_64") {
            Arch::X86_64
        } else if cfg!(target_arch = "aarch64") {
            Arch::Aarch64
        } else if cfg!(target_arch = "wasm32") {
            Arch::Wasm32
        } else {
            // Fallback — best guess for unknown host
            Arch::X86_64
        };

        let os = if cfg!(target_os = "linux") {
            Os::Linux
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOS
        } else if cfg!(target_os = "wasi") {
            Os::Wasi
        } else {
            Os::Unknown
        };

        let env = if cfg!(target_env = "gnu") {
            Env::Gnu
        } else if cfg!(target_env = "msvc") {
            Env::Msvc
        } else if cfg!(target_env = "musl") {
            Env::Musl
        } else {
            Env::None
        };

        Self { arch, os, env }
    }

    /// Parse a target triple string like `"x86_64-unknown-linux-gnu"`.
    ///
    /// Accepted forms:
    /// - `arch-vendor-os-env`  (e.g. `x86_64-unknown-linux-gnu`)
    /// - `arch-vendor-os`      (e.g. `aarch64-apple-darwin`)
    /// - `arch-os-env`         (e.g. `wasm32-wasi`)
    pub fn from_triple(triple: &str) -> Result<Self, String> {
        let parts: Vec<&str> = triple.split('-').collect();
        if parts.len() < 2 {
            return Err(format!("invalid target triple: '{triple}' (too few components)"));
        }

        let arch = parse_arch(parts[0])?;

        // Determine OS and env from remaining parts.
        // Standard triples: arch-vendor-os[-env]
        // Wasm triples:     wasm32-unknown-unknown, wasm32-wasi
        let (os, env) = match parts.len() {
            2 => {
                // arch-os (e.g. wasm32-wasi)
                let os = parse_os(parts[1])?;
                (os, Env::None)
            }
            3 => {
                // arch-vendor-os (e.g. aarch64-apple-darwin)
                // or arch-os-env in some rare cases
                let os = parse_os(parts[2]).or_else(|_| parse_os(parts[1]))?;
                (os, Env::None)
            }
            4 => {
                // arch-vendor-os-env (e.g. x86_64-unknown-linux-gnu)
                let os = parse_os(parts[2])?;
                let env = parse_env(parts[3]);
                (os, env)
            }
            _ => {
                return Err(format!("invalid target triple: '{triple}' (too many components)"));
            }
        };

        Ok(Self { arch, os, env })
    }

    /// Reconstruct the canonical triple string.
    pub fn triple_string(&self) -> String {
        let arch = match self.arch {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
            Arch::Wasm32 => "wasm32",
        };

        let (vendor, os) = match self.os {
            Os::Linux => ("unknown", "linux"),
            Os::Windows => ("pc", "windows"),
            Os::MacOS => ("apple", "darwin"),
            Os::Wasi => ("unknown", "wasi"),
            Os::Unknown => ("unknown", "unknown"),
        };

        let env = match self.env {
            Env::Gnu => "gnu",
            Env::Msvc => "msvc",
            Env::Musl => "musl",
            Env::None => "",
        };

        if env.is_empty() {
            format!("{arch}-{vendor}-{os}")
        } else {
            format!("{arch}-{vendor}-{os}-{env}")
        }
    }

    /// Returns `true` if this target is a WebAssembly target.
    pub fn is_wasm(&self) -> bool {
        self.arch == Arch::Wasm32
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.triple_string())
    }
}

fn parse_arch(s: &str) -> Result<Arch, String> {
    match s {
        "x86_64" => Ok(Arch::X86_64),
        "aarch64" | "arm64" => Ok(Arch::Aarch64),
        "wasm32" => Ok(Arch::Wasm32),
        _ => Err(format!("unsupported architecture: '{s}'")),
    }
}

fn parse_os(s: &str) -> Result<Os, String> {
    match s {
        "linux" => Ok(Os::Linux),
        "windows" => Ok(Os::Windows),
        "darwin" | "macos" => Ok(Os::MacOS),
        "wasi" => Ok(Os::Wasi),
        "unknown" | "none" => Ok(Os::Unknown),
        _ => Err(format!("unsupported OS: '{s}'")),
    }
}

fn parse_env(s: &str) -> Env {
    match s {
        "gnu" => Env::Gnu,
        "msvc" => Env::Msvc,
        "musl" => Env::Musl,
        _ => Env::None,
    }
}
