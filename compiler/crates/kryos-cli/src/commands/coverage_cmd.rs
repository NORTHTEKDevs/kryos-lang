//! `kryos coverage [path]` — function-level coverage report.
//!
//! Runs the test suite (or any program) with `KRYOS_PROFILE=1` set in the
//! subprocess environment, then crosses the resulting call-count hot-list
//! against the static set of function names parsed from each `.kry` file
//! in the project. Functions with zero calls are flagged uncovered.
//!
//! The reuse of the profile-counter infrastructure means no extra codegen
//! work is needed — every function entry already increments a counter.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct CoverageOptions {
    pub path: Option<String>,
    /// Output format: pretty (default) or json.
    pub format: String,
}

pub fn execute(opts: CoverageOptions) -> Result<(), String> {
    let root = match opts.path.as_deref() {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir().map_err(|e| format!("kryos coverage: {e}"))?,
    };
    if !root.exists() {
        return Err(format!("kryos coverage: '{}' does not exist", root.display()));
    }

    // 1. Collect every function name declared in the project.
    let mut all_fns: HashSet<String> = HashSet::new();
    let mut files: Vec<PathBuf> = Vec::new();
    if root.is_file() {
        files.push(root.clone());
    } else {
        collect_kry(&root, &mut files);
    }
    for f in &files {
        let Ok(src) = std::fs::read_to_string(f) else { continue };
        let tokens = kryos_lexer::Lexer::new(&src, 0).tokenize();
        let Ok(module) = kryos_parser::parse(tokens) else {
            continue;
        };
        for decl in &module.declarations {
            if let kryos_ast::Decl::Function { name, .. } = decl {
                all_fns.insert(name.clone());
            }
        }
    }

    if all_fns.is_empty() {
        return Err("kryos coverage: no functions found in project".into());
    }

    // 2. Run `kryos test` with profile mode on. The test runner JIT-compiles
    // and executes @test functions in-process, so a programmatic flag is
    // enough — no env var needed (avoids the libc atexit dump firing twice).
    kryos_rt::trace::set_profile_mode(true);
    let test_result = super::test_cmd::execute(super::test_cmd::TestOptions::default());
    let counts = kryos_rt::trace::take_profile_counts();
    kryos_rt::trace::set_profile_mode(false);

    // 3. Cross-reference: which declared functions were called?
    let called: HashSet<String> = counts.iter().map(|(n, _)| n.clone()).collect();

    let mut hit: Vec<(&String, u64)> = Vec::new();
    let mut missed: Vec<&String> = Vec::new();
    for fn_name in &all_fns {
        match counts.iter().find(|(n, _)| n == fn_name) {
            Some((_, c)) => hit.push((fn_name, *c)),
            None => missed.push(fn_name),
        }
    }
    hit.sort_by(|a, b| b.1.cmp(&a.1));
    missed.sort();

    let total = all_fns.len();
    let covered = hit.len();
    let pct = if total > 0 {
        (covered as f64) * 100.0 / (total as f64)
    } else {
        0.0
    };

    match opts.format.as_str() {
        "json" => {
            print!("{{");
            print!("\"total\":{total},\"covered\":{covered},\"percent\":{pct:.1},");
            print!("\"hit\":[");
            for (i, (n, c)) in hit.iter().enumerate() {
                if i > 0 { print!(","); }
                print!("{{\"name\":\"{n}\",\"calls\":{c}}}");
            }
            print!("],\"missed\":[");
            for (i, n) in missed.iter().enumerate() {
                if i > 0 { print!(","); }
                print!("\"{n}\"");
            }
            println!("]}}");
        }
        _ => {
            println!();
            println!("\x1b[1mkryos coverage\x1b[0m");
            println!(
                "  {} of {} functions exercised ({:.1}%)",
                covered, total, pct
            );
            println!();
            if !missed.is_empty() {
                println!("\x1b[33mUncovered:\x1b[0m");
                for n in &missed {
                    println!("  - {n}");
                }
                println!();
            }
            if !hit.is_empty() {
                println!("\x1b[32mTop 10 hot functions:\x1b[0m");
                for (n, c) in hit.iter().take(10) {
                    println!("  {:>10}  {n}", c);
                }
            }
        }
    }

    let _ = called;
    test_result
}

fn collect_kry(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_file() && p.extension().is_some_and(|x| x == "kry") {
            out.push(p);
        } else if p.is_dir() {
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') || name == "target" || name == "node_modules" {
                    continue;
                }
            }
            collect_kry(&p, out);
        }
    }
}
