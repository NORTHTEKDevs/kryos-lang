//! `kryos manifest` — emit a per-function capability manifest for a Kryos source tree.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use kryos_ast::Decl;
use kryos_capabilities::{Capability, CapabilitySet};
use serde::Serialize;

pub struct ManifestOptions {
    pub path: Option<String>,
    pub format: String,
    pub strict: bool,
    pub output: Option<String>,
    pub deny: Vec<String>,
}

#[derive(Serialize)]
struct FnEntry {
    capabilities: Vec<String>,
    annotated: bool,
}

#[derive(Serialize)]
struct SingleManifest {
    schema: &'static str,
    source: String,
    functions: BTreeMap<String, FnEntry>,
    unannotated_count: usize,
}

#[derive(Serialize)]
struct DirFileEntry {
    source: String,
    functions: BTreeMap<String, FnEntry>,
    unannotated_count: usize,
}

#[derive(Serialize)]
struct DirManifest {
    schema: &'static str,
    files: Vec<DirFileEntry>,
}

/// Internal per-function data — keeps the CapabilitySet for deny checks.
struct FnData {
    entry: FnEntry,
    cap_set: Option<CapabilitySet>,
}

/// Internal per-file result.
struct FileResult {
    source: String,
    functions: BTreeMap<String, FnData>,
    unannotated_count: usize,
}

fn collect_kry(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_file() && p.extension().is_some_and(|x| x == "kry") {
            out.push(p);
        } else if p.is_dir() {
            if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                if name.starts_with('.') || name == "target" || name == "node_modules" {
                    continue;
                }
            }
            collect_kry(&p, out);
        }
    }
}

fn caps_from_annotations(annotations: &[kryos_ast::Annotation]) -> (Vec<String>, CapabilitySet) {
    let set = CapabilitySet::from_annotations(annotations);
    let caps = if set.has(Capability::All) {
        vec!["all".to_string()]
    } else {
        let mut v: Vec<String> = set.iter().map(|c| c.to_string()).collect();
        v.sort();
        v
    };
    (caps, set)
}

fn scan_file(path: &Path, strict: bool) -> FileResult {
    let source = path.display().to_string().replace('\\', "/");

    let src_text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("kryos manifest: warning: skipped {source} (read error: {e})");
            return FileResult { source, functions: BTreeMap::new(), unannotated_count: 0 };
        }
    };
    let tokens = kryos_lexer::Lexer::new(&src_text, 0).tokenize();
    let module = match kryos_parser::parse(tokens) {
        Ok(m) => m,
        Err(_) => {
            eprintln!("kryos manifest: warning: skipped {source} (parse error)");
            return FileResult { source, functions: BTreeMap::new(), unannotated_count: 0 };
        }
    };

    let mut functions: BTreeMap<String, FnData> = BTreeMap::new();
    let mut unannotated_count = 0usize;

    for decl in &module.declarations {
        match decl {
            Decl::Function { name, annotations, .. } => {
                let annotated = annotations.iter().any(|a| a.name == "capabilities");
                if annotated {
                    let (caps, set) = caps_from_annotations(annotations);
                    functions.insert(name.clone(), FnData {
                        entry: FnEntry { capabilities: caps, annotated: true },
                        cap_set: Some(set),
                    });
                } else {
                    unannotated_count += 1;
                    if strict {
                        functions.insert(name.clone(), FnData {
                            entry: FnEntry { capabilities: vec![], annotated: false },
                            cap_set: None,
                        });
                    }
                }
            }
            Decl::Impl { target, methods, .. } => {
                for method in methods {
                    if let Decl::Function { name: method_name, annotations, .. } = method {
                        let key = format!("{target}::{method_name}");
                        let annotated = annotations.iter().any(|a| a.name == "capabilities");
                        if annotated {
                            let (caps, set) = caps_from_annotations(annotations);
                            functions.insert(key, FnData {
                                entry: FnEntry { capabilities: caps, annotated: true },
                                cap_set: Some(set),
                            });
                        } else {
                            unannotated_count += 1;
                            if strict {
                                functions.insert(key, FnData {
                                    entry: FnEntry { capabilities: vec![], annotated: false },
                                    cap_set: None,
                                });
                            }
                        }
                    }
                }
            }
            Decl::Trait { name: trait_name, methods, .. } => {
                for method in methods {
                    if let Decl::Function { name: method_name, annotations, .. } = method {
                        let key = format!("{trait_name}::{method_name}");
                        let annotated = annotations.iter().any(|a| a.name == "capabilities");
                        if annotated {
                            let (caps, set) = caps_from_annotations(annotations);
                            functions.insert(key, FnData {
                                entry: FnEntry { capabilities: caps, annotated: true },
                                cap_set: Some(set),
                            });
                        } else {
                            unannotated_count += 1;
                            if strict {
                                functions.insert(key, FnData {
                                    entry: FnEntry { capabilities: vec![], annotated: false },
                                    cap_set: None,
                                });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    FileResult { source, functions, unannotated_count }
}

pub fn execute(opts: ManifestOptions) -> Result<(), String> {
    let root = match opts.path.as_deref() {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from("."),
    };
    if !root.exists() {
        return Err(format!(
            "kryos manifest: path '{}' does not exist",
            root.display()
        ));
    }

    let mut files: Vec<PathBuf> = Vec::new();
    let is_single = root.is_file();
    if is_single {
        files.push(root.clone());
    } else {
        collect_kry(&root, &mut files);
        files.sort();
    }

    // Parse deny caps (warn on unknown, skip them).
    let deny_caps: Vec<Capability> = opts
        .deny
        .iter()
        .filter_map(|s| {
            match Capability::from_str(s) {
                Some(c) => Some(c),
                None => {
                    eprintln!("kryos manifest: warning: unknown capability '{s}' in --deny, skipping");
                    None
                }
            }
        })
        .collect();

    // Scan all files.
    let file_results: Vec<FileResult> = files
        .iter()
        .map(|p| scan_file(p, opts.strict))
        .collect();

    // Validate format early.
    match opts.format.as_str() {
        "json" | "pretty" => {}
        other => return Err(format!(
            "kryos manifest: unknown --format '{}': expected 'json' or 'pretty'",
            other
        )),
    }

    // Build per-file function maps for serialization / pretty output.
    let make_fn_map = |fr: &FileResult| -> BTreeMap<String, FnEntry> {
        fr.functions.iter().map(|(k, v)| {
            (k.clone(), FnEntry {
                capabilities: v.entry.capabilities.clone(),
                annotated: v.entry.annotated,
            })
        }).collect()
    };

    // Build output string based on format.
    let output_str: String = if opts.format == "pretty" {
        // Human-readable: one line per annotated function, then unannotated count.
        let mut lines: Vec<String> = Vec::new();
        for fr in &file_results {
            if file_results.len() > 1 {
                lines.push(format!("{}:", fr.source));
            }
            for (name, fn_data) in &fr.functions {
                if fn_data.entry.annotated {
                    lines.push(format!("fn {}: [{}]", name, fn_data.entry.capabilities.join(", ")));
                }
            }
            lines.push(format!("unannotated: {}", fr.unannotated_count));
        }
        lines.join("\n")
    } else {
        // JSON (default).
        if is_single {
            let fr = file_results.iter().next().map(|fr| SingleManifest {
                schema: "kryos-manifest-v1",
                source: fr.source.clone(),
                functions: make_fn_map(fr),
                unannotated_count: fr.unannotated_count,
            }).unwrap_or_else(|| SingleManifest {
                schema: "kryos-manifest-v1",
                source: root.display().to_string().replace('\\', "/"),
                functions: BTreeMap::new(),
                unannotated_count: 0,
            });
            serde_json::to_string_pretty(&fr).map_err(|e| e.to_string())?
        } else {
            let mut dir_files: Vec<DirFileEntry> = file_results
                .iter()
                .map(|fr| DirFileEntry {
                    source: fr.source.clone(),
                    functions: make_fn_map(fr),
                    unannotated_count: fr.unannotated_count,
                })
                .collect();
            dir_files.sort_by(|a, b| a.source.cmp(&b.source));
            let manifest = DirManifest { schema: "kryos-manifest-v1", files: dir_files };
            serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?
        }
    };

    // Alias for emit + deny steps below.
    let json = output_str;

    // Emit output (before deny check, per design).
    if let Some(ref out_path) = opts.output {
        fs::write(out_path, &json).map_err(|e| format!("kryos manifest: write error: {e}"))?;
    } else {
        println!("{json}");
    }

    // Deny gate.
    if !deny_caps.is_empty() {
        let mut violations: Vec<(String, String)> = Vec::new();
        for fr in &file_results {
            for (fn_key, fn_data) in &fr.functions {
                if let Some(ref cap_set) = fn_data.cap_set {
                    for &denied in &deny_caps {
                        if cap_set.has(denied) {
                            violations.push((fn_key.clone(), denied.to_string()));
                        }
                    }
                }
            }
        }
        if !violations.is_empty() {
            for (fn_key, cap) in &violations {
                eprintln!("kryos manifest: denied capability '{cap}' found in: {fn_key}");
            }
            std::process::exit(1);
        }
    }

    Ok(())
}
