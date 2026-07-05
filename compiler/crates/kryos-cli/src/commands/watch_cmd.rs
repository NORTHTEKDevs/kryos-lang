//! `kryos watch <file>` — auto-rebuild + re-run on source change.
//!
//! Polls the file's mtime every 250ms. On change: type-check (`kryos check`)
//! then run via the Cranelift JIT. Designed to avoid external `notify`
//! crate dependency, so the dev-loop works on every supported platform
//! without extra deps.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone)]
pub struct WatchOptions {
    pub file: String,
    /// Poll interval in milliseconds. Default 250.
    pub interval_ms: Option<u64>,
    /// Run command: "run" (default) or "check" (skip execution).
    pub run: String,
}

pub fn execute(opts: WatchOptions) -> Result<(), String> {
    let path = PathBuf::from(&opts.file);
    if !path.exists() {
        return Err(format!("kryos watch: '{}' does not exist", path.display()));
    }
    let interval = Duration::from_millis(opts.interval_ms.unwrap_or(250));

    eprintln!(
        "\x1b[1mkryos watch\x1b[0m {} (interval={}ms, action={})",
        path.display(),
        interval.as_millis(),
        opts.run
    );
    eprintln!("press Ctrl-C to exit");
    eprintln!();

    let mut last_mtime = read_mtime(&path);
    // Run once at startup so the user sees the current state.
    invoke(&path, &opts.run);

    loop {
        std::thread::sleep(interval);
        let now = read_mtime(&path);
        if now != last_mtime {
            last_mtime = now;
            eprintln!();
            eprintln!("\x1b[1m\x1b[33mwatch:\x1b[0m change detected at {}", format_clock());
            invoke(&path, &opts.run);
        }
    }
}

fn read_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn invoke(path: &Path, action: &str) {
    let result = match action {
        "check" => super::check::execute(
            &path.to_string_lossy(),
            false,
            kryos_driver::CapabilityMode::Permissive,
        ),
        _ => super::run::execute(&path.to_string_lossy(), &[]),
    };
    match result {
        Ok(()) => eprintln!("\x1b[32mwatch:\x1b[0m ok"),
        Err(e) => eprintln!("\x1b[31mwatch:\x1b[0m {e}"),
    }
}

fn format_clock() -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let secs_of_day = (now % 86_400) as i64;
    let h = secs_of_day / 3600;
    let mi = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;
    format!("{h:02}:{mi:02}:{s:02} UTC")
}
