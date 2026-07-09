//! `kryos fmt` — format Kryos source files.

use std::path::Path;

/// Execute the fmt command.
pub fn execute(files: &[String], check: bool) -> Result<(), String> {
    let targets = if files.is_empty() {
        discover_kry_files(Path::new("."))?
    } else {
        files.to_vec()
    };

    if targets.is_empty() {
        eprintln!("kryos fmt: no .kry files found");
        return Ok(());
    }

    let mut unformatted = 0usize;
    let mut skipped = 0usize;

    for path in &targets {
        let source =
            std::fs::read_to_string(path).map_err(|e| format!("cannot read `{path}`: {e}"))?;

        // The AST does not carry `//` line/inline comments (only `///` doc
        // comments survive), so a naive re-emit would DELETE them. For files
        // with such comments, use the comment-preserving path: it re-anchors
        // every comment against the formatted output and refuses (skip, not
        // destroy) when any comment cannot be placed confidently.
        let formatted = if has_non_doc_comment(&source) {
            match kryos_fmt::format_source_preserving_comments(&source).map_err(|diags| {
                let msgs: Vec<String> = diags.iter().map(|d| d.message.clone()).collect();
                format!("parse error in `{path}`: {}", msgs.join("; "))
            })? {
                Some(f) => f,
                None => {
                    eprintln!(
                        "  skipped {path} (a comment could not be re-anchored; file left untouched)"
                    );
                    skipped += 1;
                    continue;
                }
            }
        } else {
            kryos_fmt::format_source(&source).map_err(|diags| {
                let msgs: Vec<String> = diags.iter().map(|d| d.message.clone()).collect();
                format!("parse error in `{path}`: {}", msgs.join("; "))
            })?
        };

        if source == formatted {
            continue;
        }

        if check {
            eprintln!("  would reformat {path}");
            unformatted += 1;
        } else {
            std::fs::write(path, &formatted).map_err(|e| format!("cannot write `{path}`: {e}"))?;
            eprintln!("  formatted {path}");
        }
    }

    if skipped > 0 {
        eprintln!(
            "kryos fmt: skipped {skipped} file{} to avoid deleting comments (see above)",
            if skipped == 1 { "" } else { "s" }
        );
    }

    if check && unformatted > 0 {
        return Err(format!(
            "{unformatted} file{} would be reformatted",
            if unformatted == 1 { "" } else { "s" }
        ));
    }

    Ok(())
}

/// True if the source contains a regular `//` line or inline comment (NOT a
/// `///` doc comment, which the formatter preserves). Scans with minimal
/// awareness of string/char literals and block comments so a `//` inside a
/// string (e.g. a URL) does not trigger a false positive.
fn has_non_doc_comment(src: &str) -> bool {
    let b = src.as_bytes();
    let n = b.len();
    let mut i = 0;
    while i < n {
        match b[i] {
            b'"' => {
                // Skip a string literal (respecting backslash escapes).
                i += 1;
                while i < n && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'\'' => {
                // Skip a char literal (respecting backslash escapes).
                i += 1;
                while i < n && b[i] != b'\'' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'/' if i + 1 < n && b[i + 1] == b'*' => {
                // Skip a (possibly nested) block comment.
                let mut depth = 1;
                i += 2;
                while i < n && depth > 0 {
                    if i + 1 < n && b[i] == b'/' && b[i + 1] == b'*' {
                        depth += 1;
                        i += 2;
                    } else if i + 1 < n && b[i] == b'*' && b[i + 1] == b'/' {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            b'/' if i + 1 < n && b[i + 1] == b'/' => {
                // A doc comment `///` is preserved by the formatter; anything
                // else (`//` or `//!`) is a regular comment that would be lost.
                let is_doc = i + 2 < n && b[i + 2] == b'/';
                if !is_doc {
                    return true;
                }
                // Skip the rest of the doc-comment line.
                while i < n && b[i] != b'\n' {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    false
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
