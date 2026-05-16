//! `kryos doc` — generate documentation from Kryos source files.

use std::path::Path;

use kryos_doc::{extract_docs, render_html, render_html_index, render_markdown};

/// Execute the doc command.
///
/// Parses the given source file(s), extracts doc comments and declarations,
/// and renders them as markdown (default) or HTML to stdout / a directory.
pub fn execute(files: &[String], output_dir: Option<&str>, html: bool) -> Result<(), String> {
    let targets = if files.is_empty() {
        discover_kry_files(Path::new("."))?
    } else {
        files.to_vec()
    };

    if targets.is_empty() {
        eprintln!("kryos doc: no .kry files found");
        return Ok(());
    }

    let output_path = output_dir.map(Path::new);

    // If an output directory is specified, create it.
    if let Some(dir) = output_path {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create output directory `{}`: {e}", dir.display()))?;
    }

    let mut all_modules: Vec<(String, Vec<kryos_doc::DocItem>)> = Vec::new();

    for path in &targets {
        let source =
            std::fs::read_to_string(path).map_err(|e| format!("cannot read `{path}`: {e}"))?;

        let items = extract_docs(&source);

        // Derive module name from file path.
        let module_name = Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let rendered = if html {
            render_html(&items, &module_name)
        } else {
            render_markdown(&items, &module_name)
        };
        let ext = if html { "html" } else { "md" };

        if let Some(dir) = output_path {
            let out_file = dir.join(format!("{module_name}.{ext}"));
            std::fs::write(&out_file, &rendered)
                .map_err(|e| format!("cannot write `{}`: {e}", out_file.display()))?;
            eprintln!("  wrote {}", out_file.display());
        } else {
            // Print to stdout
            print!("{rendered}");
        }

        all_modules.push((module_name, items));
    }

    // If writing to a directory, also generate an index.
    if let Some(dir) = output_path {
        if all_modules.len() > 1 {
            let (index, ext) = if html {
                (render_html_index(&all_modules), "html")
            } else {
                (kryos_doc::render_module_index(&all_modules), "md")
            };
            let index_path = dir.join(format!("index.{ext}"));
            std::fs::write(&index_path, &index)
                .map_err(|e| format!("cannot write `{}`: {e}", index_path.display()))?;
            eprintln!("  wrote {}", index_path.display());
        }
        let total_items: usize = all_modules.iter().map(|(_, items)| items.len()).sum();
        eprintln!(
            "generated documentation for {} module{} ({} items)",
            all_modules.len(),
            if all_modules.len() == 1 { "" } else { "s" },
            total_items,
        );
    }

    Ok(())
}

/// Recursively find all `.kry` files under a directory.
fn discover_kry_files(dir: &Path) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    walk_dir(dir, &mut result)?;
    result.sort();
    Ok(result)
}

fn walk_dir(dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read `{}`: {e}", dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
        }

        if path.is_dir() {
            walk_dir(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("kry") {
            out.push(path.display().to_string());
        }
    }

    Ok(())
}
