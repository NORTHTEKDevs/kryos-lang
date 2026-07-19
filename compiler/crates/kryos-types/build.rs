//! Generates a (function name -> stdlib module) index from compiler/stdlib at
//! build time. The checker uses it to turn "undefined variable `sha256`" into
//! an actionable "add `use std::crypto`" note instead of a Levenshtein guess
//! at an unrelated builtin. Scanning the real .kry sources keeps the index
//! from ever drifting out of sync with the shipped stdlib.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let stdlib_dir = manifest.join("../../stdlib");
    println!("cargo:rerun-if-changed={}", stdlib_dir.display());

    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Ok(entries) = fs::read_dir(&stdlib_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("kry") {
                continue;
            }
            let Some(module) = path.file_stem().and_then(|s| s.to_str()).map(String::from)
            else {
                continue;
            };
            let Ok(src) = fs::read_to_string(&path) else {
                continue;
            };
            for line in src.lines() {
                // Top-level functions only: impl methods are indented. Skip
                // `_`-prefixed internals -- suggesting private helpers would
                // send users at names that aren't part of the module's API.
                let Some(rest) = line.strip_prefix("fn ") else {
                    continue;
                };
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if name.is_empty() || name.starts_with('_') {
                    continue;
                }
                pairs.push((name, module.clone()));
            }
        }
    }
    pairs.sort();
    pairs.dedup();

    let mut out = String::from(
        "/// (function name, stdlib module) pairs scanned from compiler/stdlib.\n\
         pub static STDLIB_EXPORTS: &[(&str, &str)] = &[\n",
    );
    for (name, module) in &pairs {
        out.push_str(&format!("    ({name:?}, {module:?}),\n"));
    }
    out.push_str("];\n");

    let dest = PathBuf::from(env::var("OUT_DIR").unwrap()).join("stdlib_index.rs");
    fs::write(dest, out).unwrap();
}
