//! `kryos diff <a> <b>` — semantic diff between two Kryos source files.
//!
//! Reports which top-level declarations were added / removed / modified
//! (signature change for fns). Less noisy than `diff a.kry b.kry` when
//! whitespace + comments shift around.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct DiffOptions {
    pub a: String,
    pub b: String,
}

#[derive(Debug)]
struct DeclSig {
    name: String,
    kind: &'static str,
    /// For functions, a normalized signature string.
    sig: String,
}

pub fn execute(opts: DiffOptions) -> Result<(), String> {
    let a_decls = collect_decls(&opts.a)?;
    let b_decls = collect_decls(&opts.b)?;

    let a_by_name: BTreeMap<String, &DeclSig> =
        a_decls.iter().map(|d| (d.name.clone(), d)).collect();
    let b_by_name: BTreeMap<String, &DeclSig> =
        b_decls.iter().map(|d| (d.name.clone(), d)).collect();

    let mut added: Vec<&DeclSig> = Vec::new();
    let mut removed: Vec<&DeclSig> = Vec::new();
    let mut modified: Vec<(&DeclSig, &DeclSig)> = Vec::new();
    let mut unchanged: usize = 0;

    for (name, a) in &a_by_name {
        match b_by_name.get(name) {
            None => removed.push(a),
            Some(b) => {
                if a.kind != b.kind || a.sig != b.sig {
                    modified.push((a, b));
                } else {
                    unchanged += 1;
                }
            }
        }
    }
    for (name, b) in &b_by_name {
        if !a_by_name.contains_key(name) {
            added.push(b);
        }
    }

    if !added.is_empty() {
        println!("\x1b[32m+ added\x1b[0m ({})", added.len());
        for d in &added {
            println!("  + {} {}  {}", d.kind, d.name, d.sig);
        }
    }
    if !removed.is_empty() {
        println!("\x1b[31m- removed\x1b[0m ({})", removed.len());
        for d in &removed {
            println!("  - {} {}  {}", d.kind, d.name, d.sig);
        }
    }
    if !modified.is_empty() {
        println!("\x1b[33m~ modified\x1b[0m ({})", modified.len());
        for (a, b) in &modified {
            println!("  ~ {} {}", a.kind, a.name);
            println!("    - {}", a.sig);
            println!("    + {}", b.sig);
        }
    }
    println!();
    println!(
        "summary: +{} -{} ~{} ={}",
        added.len(),
        removed.len(),
        modified.len(),
        unchanged,
    );
    Ok(())
}

fn collect_decls(path: &str) -> Result<Vec<DeclSig>, String> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("kryos diff: read {path}: {e}"))?;
    let tokens = kryos_lexer::Lexer::new(&src, 0).tokenize();
    let module = kryos_parser::parse(tokens)
        .map_err(|e| format!("kryos diff: parse {path}: {e:?}"))?;

    let mut out = Vec::new();
    for decl in &module.declarations {
        match decl {
            kryos_ast::Decl::Function { name, params, ret_ty, is_async, .. } => {
                let p = params
                    .iter()
                    .map(|p| match &p.ty {
                        Some(_) => format!("{}: T", p.name),
                        None => p.name.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let r = match ret_ty {
                    Some(_) => " -> T".to_string(),
                    None => String::new(),
                };
                let prefix = if *is_async { "async fn" } else { "fn" };
                out.push(DeclSig {
                    name: name.clone(),
                    kind: "fn",
                    sig: format!("{prefix} {name}({p}){r}"),
                });
            }
            kryos_ast::Decl::Struct { name, fields, .. } => {
                out.push(DeclSig {
                    name: name.clone(),
                    kind: "struct",
                    sig: format!("struct {name} with {} fields", fields.len()),
                });
            }
            kryos_ast::Decl::Enum { name, variants, .. } => {
                out.push(DeclSig {
                    name: name.clone(),
                    kind: "enum",
                    sig: format!("enum {name} with {} variants", variants.len()),
                });
            }
            kryos_ast::Decl::Trait { name, methods, .. } => {
                out.push(DeclSig {
                    name: name.clone(),
                    kind: "trait",
                    sig: format!("trait {name} with {} methods", methods.len()),
                });
            }
            kryos_ast::Decl::Const { name, .. } => {
                out.push(DeclSig {
                    name: name.clone(),
                    kind: "const",
                    sig: format!("const {name}"),
                });
            }
            _ => {}
        }
    }
    Ok(out)
}
