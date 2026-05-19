//! `kryos profile <file>` — run a Kryos program with call-count profiling.
//!
//! Sets `KRYOS_PROFILE=1` in the subprocess environment. The runtime detects
//! the env var, enables a per-name call counter, and installs an atexit hook
//! that prints the hot-list when the process exits.

use std::path::Path;

pub fn execute(file: &str, args: &[String]) -> Result<(), String> {
    if !Path::new(file).exists() {
        return Err(format!("kryos profile: file '{file}' does not exist"));
    }
    std::env::set_var("KRYOS_PROFILE", "1");
    kryos_rt::trace::set_profile_mode(true);
    eprintln!("\x1b[1mkryos profile\x1b[0m enabled for {file}");
    eprintln!();
    let result = super::run::execute(file, args);
    kryos_rt::trace::set_profile_mode(false);
    std::env::remove_var("KRYOS_PROFILE");
    result
}
