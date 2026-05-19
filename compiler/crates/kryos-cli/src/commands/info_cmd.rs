//! `kryos info [path]` — project summary.
//!
//! Walks the project (defaults to cwd), counts .kry files / total LoC /
//! functions / structs / enums / traits. Reports package metadata if
//! a kryos.toml is present.

use std::path::{Path, PathBuf};

use kryos_package::Manifest;

#[derive(Debug, Clone, Default)]
pub struct InfoOptions {
    pub path: Option<String>,
}

pub fn execute(opts: InfoOptions) -> Result<(), String> {
    let root = match opts.path.as_deref() {
        Some(p) => PathBuf::from(p),
        None => std::env::current_dir().map_err(|e| format!("kryos info: {e}"))?,
    };
    if !root.exists() {
        return Err(format!("kryos info: '{}' does not exist", root.display()));
    }

    // Package metadata if available.
    let manifest_path = root.join("kryos.toml");
    if manifest_path.is_file() {
        match Manifest::from_file(&manifest_path) {
            Ok(m) => {
                println!(
                    "\x1b[1m{}\x1b[0m v{}",
                    m.package.name, m.package.version
                );
                if let Some(desc) = &m.package.description {
                    println!("  {desc}");
                }
                let dep_count = m.dependencies.len();
                if dep_count > 0 {
                    println!("  {} direct dependencies", dep_count);
                }
            }
            Err(e) => {
                println!("\x1b[33mkryos.toml present but unreadable:\x1b[0m {e}");
            }
        }
    } else {
        println!("\x1b[1m{}\x1b[0m (no kryos.toml)", root.display());
    }

    // Source stats.
    let mut files: Vec<PathBuf> = Vec::new();
    collect_kry(&root, &mut files);

    let mut total_lines = 0usize;
    let mut total_funcs = 0usize;
    let mut total_structs = 0usize;
    let mut total_enums = 0usize;
    let mut total_traits = 0usize;
    let mut total_tests = 0usize;
    let mut total_benches = 0usize;

    for f in &files {
        let Ok(src) = std::fs::read_to_string(f) else {
            continue;
        };
        total_lines += src.lines().count();
        let tokens = kryos_lexer::Lexer::new(&src, 0).tokenize();
        if let Ok(module) = kryos_parser::parse(tokens) {
            for decl in &module.declarations {
                match decl {
                    kryos_ast::Decl::Function { annotations, .. } => {
                        total_funcs += 1;
                        for a in annotations {
                            if a.name == "test" {
                                total_tests += 1;
                            }
                            if a.name == "bench" {
                                total_benches += 1;
                            }
                        }
                    }
                    kryos_ast::Decl::Struct { .. } => total_structs += 1,
                    kryos_ast::Decl::Enum { .. } => total_enums += 1,
                    kryos_ast::Decl::Trait { .. } => total_traits += 1,
                    _ => {}
                }
            }
        }
    }

    println!();
    println!("\x1b[1mSource stats\x1b[0m");
    println!("  files:     {}", files.len());
    println!("  lines:     {}", total_lines);
    println!("  fns:       {}", total_funcs);
    println!("  @tests:    {}", total_tests);
    println!("  @benches:  {}", total_benches);
    println!("  structs:   {}", total_structs);
    println!("  enums:     {}", total_enums);
    println!("  traits:    {}", total_traits);
    Ok(())
}

fn collect_kry(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
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
