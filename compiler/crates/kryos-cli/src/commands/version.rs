//! `kryos version` — print detailed version and build information.

/// Print version information.
pub fn execute() {
    let version = env!("CARGO_PKG_VERSION");
    println!("kryos {version}");
    println!("  edition: 2026");

    // Build metadata.
    #[cfg(debug_assertions)]
    println!("  profile: debug");
    #[cfg(not(debug_assertions))]
    println!("  profile: release");

    // Target triple — set by Cargo at build time.
    if let Some(target) = option_env!("TARGET") {
        println!("  target:  {target}");
    }

    // Host triple.
    println!("  host:    {}", std::env::consts::ARCH);
    println!("  os:      {}", std::env::consts::OS);
}
