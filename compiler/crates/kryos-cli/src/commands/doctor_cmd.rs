//! `kryos doctor` — diagnose toolchain installation.
//!
//! Reports on:
//!  1. kryos binary version + platform
//!  2. C/C++ toolchain (cc / clang / link.exe) discovery
//!  3. kryos_rt static lib discovery
//!  4. kryos_stdlib_native static lib discovery
//!  5. Optional: cargo, git (developer convenience)
//!  6. Env vars that affect behavior (KRYOS_RT_LIB, KRYOS_TRACE, etc.)

use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct DoctorOptions {
    /// Verbose output (show full paths and version banners).
    pub verbose: bool,
}

pub fn execute(opts: DoctorOptions) -> Result<(), String> {
    let mut warnings = 0usize;
    let mut errors = 0usize;

    section("kryos toolchain");
    ok("kryos", env!("CARGO_PKG_VERSION"));
    ok(
        "platform",
        &format!(
            "{} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    );
    if let Ok(exe) = std::env::current_exe() {
        info("kryos exe", &exe.display().to_string());
    }

    section("C/C++ toolchain (for `kryos build --release`)");
    let mut have_cc = false;
    for cand in &["clang", "cc", "gcc"] {
        if let Some(v) = probe(cand, &["--version"], opts.verbose) {
            ok(cand, &v.lines().next().unwrap_or(""));
            have_cc = true;
        }
    }
    if cfg!(windows) {
        for cand in &["link"] {
            if probe(cand, &[], opts.verbose).is_some() {
                ok(cand, "available (Visual Studio link.exe)");
                have_cc = true;
            }
        }
    }
    if !have_cc {
        // PATH probing missed everything; ask the actual linker discovery
        // (vswhere/registry on Windows, which is what `kryos build` uses).
        match kryos_linker::find_system_linker(&kryos_linker::Target::host()) {
            Ok(path) => {
                ok("system linker", &path.display().to_string());
                have_cc = true;
            }
            Err(_) => {}
        }
    }
    if !have_cc {
        warn("no C linker found — `kryos build --release` will fail");
        warnings += 1;
    }

    section("Kryos runtime libraries");
    let rt_lib = locate_runtime_lib();
    match &rt_lib {
        Some(p) => ok("kryos_rt", &p.display().to_string()),
        None => {
            err("kryos_rt static lib not found in any known location");
            errors += 1;
        }
    }
    // The stdlib SOURCE directory (std::* modules). This is what a
    // standalone install most commonly gets wrong (see resolve.rs order).
    match kryos_driver::resolve::resolve_stdlib_dir_for_diagnostics() {
        Some(p) => ok("stdlib (std::*)", &p.display().to_string()),
        None => {
            err("stdlib directory not found — `use std::...` will fail; set KRYOS_STDLIB_DIR or keep stdlib/ next to the kryos binary");
            errors += 1;
        }
    }
    let stdlib = locate_stdlib_lib();
    match &stdlib {
        Some(p) => ok("kryos_stdlib_native", &p.display().to_string()),
        None => {
            warn(
                "kryos_stdlib_native not found — programs using std::fs, std::http, std::crypto, etc. may fail to link"
            );
            warnings += 1;
        }
    }

    section("Optional tooling");
    if probe("cargo", &["--version"], opts.verbose).is_some() {
        ok("cargo", "available");
    } else {
        info("cargo", "not found (only needed if rebuilding the compiler from source)");
    }
    if probe("git", &["--version"], opts.verbose).is_some() {
        ok("git", "available");
    } else {
        info("git", "not found (only needed for `kryos pkg add github:...`)");
    }

    section("Environment");
    for key in &[
        "KRYOS_RT_LIB",
        "KRYOS_STDLIB_NATIVE_LIB",
        "KRYOS_TRACE",
        "KRYOS_PROFILE",
        "PATH",
    ] {
        match std::env::var(key) {
            Ok(v) if !v.is_empty() => {
                if opts.verbose || key.starts_with("KRYOS_") {
                    info(key, &v);
                }
            }
            _ => {}
        }
    }

    println!();
    if errors > 0 {
        println!("\x1b[31m{} error(s), {} warning(s)\x1b[0m", errors, warnings);
        Err(format!("kryos doctor: {errors} error(s)"))
    } else if warnings > 0 {
        println!("\x1b[33m0 errors, {} warning(s)\x1b[0m", warnings);
        Ok(())
    } else {
        println!("\x1b[32mall checks passed\x1b[0m");
        Ok(())
    }
}

fn section(title: &str) {
    println!();
    println!("\x1b[1m== {title} ==\x1b[0m");
}

fn ok(name: &str, value: &str) {
    println!("  \x1b[32mOK\x1b[0m   {:<25} {}", name, value);
}

fn info(name: &str, value: &str) {
    println!("  \x1b[36minfo\x1b[0m {:<25} {}", name, value);
}

fn warn(msg: &str) {
    println!("  \x1b[33mWARN\x1b[0m {}", msg);
}

fn err(msg: &str) {
    println!("  \x1b[31mERR\x1b[0m  {}", msg);
}

fn probe(cmd: &str, args: &[&str], verbose: bool) -> Option<String> {
    let output = Command::new(cmd).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if verbose && !stdout.is_empty() {
        let _ = stdout;
    }
    Some(stdout)
}

fn locate_runtime_lib() -> Option<PathBuf> {
    let name = if cfg!(all(windows, target_env = "msvc")) {
        "kryos_rt.lib"
    } else {
        "libkryos_rt.a"
    };

    if let Ok(p) = std::env::var("KRYOS_RT_LIB") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for cand in [dir.join("..").join("lib").join(name), dir.join(name)] {
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }

    for profile in &["debug", "release"] {
        let cand = PathBuf::from("compiler").join("target").join(profile).join(name);
        if cand.is_file() {
            return Some(cand);
        }
        let cand = PathBuf::from("target").join(profile).join(name);
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

fn locate_stdlib_lib() -> Option<PathBuf> {
    let name = if cfg!(all(windows, target_env = "msvc")) {
        "kryos_stdlib_native.lib"
    } else {
        "libkryos_stdlib_native.a"
    };

    if let Ok(p) = std::env::var("KRYOS_STDLIB_NATIVE_LIB") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for cand in [dir.join("..").join("lib").join(name), dir.join(name)] {
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }

    for profile in &["debug", "release"] {
        let cand = PathBuf::from("compiler").join("target").join(profile).join(name);
        if cand.is_file() {
            return Some(cand);
        }
        let cand = PathBuf::from("target").join(profile).join(name);
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}
