//! Runtime library discovery — locates `libkryos_rt.a` / `kryos_rt.lib` for
//! linking into compiled Kryos programs.
//!
//! Search order:
//! 1. `KRYOS_RT_LIB` environment variable (explicit path override)
//! 2. `<exe_dir>/../lib/` (distribution layout: bin/kryos + lib/libkryos_rt.a)
//! 3. `<exe_dir>/` (flat distribution)
//! 4. Cargo `target/{profile}/` relative to the current directory (development)

use std::path::PathBuf;

/// The static library filename for the current target.
fn rt_lib_filename() -> &'static str {
    if cfg!(all(windows, target_env = "msvc")) {
        "kryos_rt.lib"
    } else {
        "libkryos_rt.a"
    }
}

/// Locate the kryos-rt static library.
///
/// Returns `None` if the runtime cannot be found, which means linking will
/// proceed without it (and fail if the program references any runtime symbols).
pub fn find_runtime_lib() -> Option<PathBuf> {
    let name = rt_lib_filename();

    // 1. Explicit override via environment variable.
    if let Ok(path) = std::env::var("KRYOS_RT_LIB") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }

    // 2. Distribution layout: <exe_dir>/../lib/<name>
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let dist = exe_dir.join("..").join("lib").join(name);
            if dist.is_file() {
                return Some(dist);
            }

            // 3. Flat layout: <exe_dir>/<name>
            let flat = exe_dir.join(name);
            if flat.is_file() {
                return Some(flat);
            }
        }
    }

    // 4. Cargo target directory (development builds).
    //    Check both debug and release profiles.
    for profile in &["debug", "release"] {
        let candidate = PathBuf::from("target").join(profile).join(name);
        if candidate.is_file() {
            return Some(candidate.canonicalize().unwrap_or(candidate));
        }
    }

    None
}

/// Return the system libraries required when linking the Rust-based runtime
/// staticlib on the given target.
///
/// Rust's standard library (embedded in the staticlib) depends on OS-specific
/// system libraries. These must be passed to the linker as `-l` flags.
pub fn system_libs(target: &kryos_linker::Target) -> Vec<String> {
    use kryos_linker::{Os, Env};

    match (target.os, target.env) {
        (Os::Windows, Env::Msvc) => vec![
            "ntdll".into(),
            "userenv".into(),
            "ws2_32".into(),
            "dbghelp".into(),
        ],
        (Os::Windows, _) => vec![
            // MinGW / GNU
            "ntdll".into(),
            "userenv".into(),
            "ws2_32".into(),
            "dbghelp".into(),
            "gcc".into(),
            "pthread".into(),
        ],
        (Os::Linux, _) => vec![
            "pthread".into(),
            "dl".into(),
            "m".into(),
        ],
        (Os::MacOS, _) => vec![
            "System".into(),
            "pthread".into(),
        ],
        _ => vec![],
    }
}
