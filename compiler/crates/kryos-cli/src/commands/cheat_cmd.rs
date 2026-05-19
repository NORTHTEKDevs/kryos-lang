//! `kryos cheat` — print the syntax cheatsheet to stdout.
//!
//! Mirrors `docs/learn/cheatsheet.md`. Embedded at compile time so it's
//! always available even when docs are unpacked separately.

const CHEATSHEET: &str = include_str!("../../../../../docs/learn/cheatsheet.md");

pub fn execute() -> Result<(), String> {
    print!("{CHEATSHEET}");
    Ok(())
}
