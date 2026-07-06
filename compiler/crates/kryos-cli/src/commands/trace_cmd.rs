//! `kryos trace` — run a Kryos program with verbose function-entry/exit logging.
//!
//! Wraps `kryos run` (Cranelift JIT path) with `kryos_rt::trace::set_verbose_trace(true)`
//! enabled before main starts. Useful for "what does this program *actually* do" without
//! firing up a real debugger.

use std::path::Path;

pub fn execute(file: &str, args: &[String]) -> Result<(), String> {
    if !Path::new(file).exists() {
        return Err(format!("kryos trace: file '{file}' does not exist"));
    }
    // `kryos run` spawns the compiled binary as a subprocess; setting the
    // in-process atomic flag wouldn't propagate. Set KRYOS_TRACE=1 in the
    // child's environment instead — the runtime probes it on startup.
    std::env::set_var("KRYOS_TRACE", "1");
    kryos_rt::trace::set_verbose_trace(true);
    eprintln!("\x1b[1mkryos trace\x1b[0m enabled for {file}");
    eprintln!();
    let result = super::run::execute(file, args, kryos_driver::CapabilityMode::Inferred);
    kryos_rt::trace::set_verbose_trace(false);
    std::env::remove_var("KRYOS_TRACE");
    result
}
