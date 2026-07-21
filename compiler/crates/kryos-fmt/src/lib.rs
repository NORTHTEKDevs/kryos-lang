//! Kryos code formatter — takes an AST and produces canonically formatted source code.
//!
//! The formatter uses 4-space indentation, 80-column line width for signature wrapping,
//! and blank lines between top-level declarations.

#![allow(clippy::too_many_arguments)]

pub mod formatter;

use kryos_ast::Module;
use kryos_errors::Diagnostic;
use kryos_lexer::Lexer;
use kryos_parser::parse;

pub use formatter::Formatter;

/// Convenience function: parse source code, then format the resulting AST.
///
/// Returns the formatted source string on success, or a list of parse diagnostics on failure.
pub fn format_source(source: &str) -> Result<String, Vec<Diagnostic>> {
    let tokens = Lexer::new(source, 0).tokenize();
    let module = parse(tokens)?;
    let fmt = Formatter::new();
    Ok(fmt.format_module(&module))
}

/// Format an already-parsed Module AST into canonical source code.
pub fn format_module(module: &Module) -> String {
    let fmt = Formatter::new();
    fmt.format_module(module)
}

/// Comment-preserving formatting via line-anchored re-insertion.
///
/// The AST does not carry `//` comments, so a plain `format_source` deletes
/// them. This wrapper extracts every non-doc comment from the raw source
/// together with the code line it is anchored to (standalone comment blocks
/// anchor to the NEXT code line; trailing comments anchor to THEIR line),
/// formats the comment-stripped AST, then re-inserts each comment against
/// the formatted counterpart of its anchor line (ordered prefix-token
/// matching, so duplicate-looking lines resolve by position).
///
/// Returns `None` when any comment cannot be re-anchored confidently -- the
/// caller should then leave the file untouched (never destroy comments).
pub fn format_source_preserving_comments(source: &str) -> Result<Option<String>, Vec<Diagnostic>> {
    let formatted = format_source(source)?;

    // Block comments (`/* ... */`) are not carried by the AST and are invisible
    // to the `//`-only extractor below, so reformatting would SILENTLY DELETE
    // them (a critical data-loss bug: `let x = 1 + /* note */ 2` became
    // `let x = 1 + 2`). The re-anchoring machinery here handles only line
    // comments, so refuse to format any file containing a block comment --
    // skip it (leave it untouched) rather than destroy the comment.
    if has_block_comment(source) {
        return Ok(None);
    }

    /// True if `src` contains a `/* ... */` block comment outside a string/char
    /// literal or a `//` line comment.
    fn has_block_comment(src: &str) -> bool {
        let b = src.as_bytes();
        let n = b.len();
        let mut i = 0;
        while i < n {
            match b[i] {
                b'"' => {
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
                    i += 1;
                    while i < n && b[i] != b'\'' {
                        if b[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                    i += 1;
                }
                b'/' if i + 1 < n && b[i + 1] == b'*' => return true,
                b'/' if i + 1 < n && b[i + 1] == b'/' => {
                    while i < n && b[i] != b'\n' {
                        i += 1;
                    }
                }
                _ => i += 1,
            }
        }
        false
    }

    // ---- extract comments with anchors --------------------------------
    #[derive(Debug)]
    struct CommentItem {
        text: String,          // full comment text ("// ...")
        trailing: bool,        // true = same line as code
        anchor: Option<String>, // normalized anchor code line (None = EOF block)
        // for repeated identical anchors: which occurrence (0-based)
        occurrence: usize,
    }

    fn normalize(line: &str) -> String {
        line.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Strip a trailing `//` comment from a line, respecting string literals.
    /// Returns (code_part, Some(comment)) or (line, None).
    fn split_trailing_comment(line: &str) -> (String, Option<String>) {
        let bytes = line.as_bytes();
        let mut in_str = false;
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' if in_str => {
                    i += 2;
                    continue;
                }
                b'"' => in_str = !in_str,
                b'/' if !in_str && i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                    // doc comments (///) are AST-carried; leave them alone
                    if i + 2 < bytes.len() && bytes[i + 2] == b'/' {
                        return (line.to_string(), None);
                    }
                    return (
                        line[..i].trim_end().to_string(),
                        Some(line[i..].trim_end().to_string()),
                    );
                }
                _ => {}
            }
            i += 1;
        }
        (line.to_string(), None)
    }

    let src_lines: Vec<&str> = source.lines().collect();
    let mut items: Vec<CommentItem> = Vec::new();
    let mut anchor_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut pending_block: Vec<String> = Vec::new();

    for (idx, raw) in src_lines.iter().enumerate() {
        let trimmed = raw.trim();
        let is_standalone_comment =
            trimmed.starts_with("//") && !trimmed.starts_with("///");
        if is_standalone_comment {
            pending_block.push(trimmed.to_string());
            continue;
        }
        // A code (or blank/doc) line: flush any pending standalone block,
        // anchored to the next NON-BLANK code line (this one, if code).
        let (code_part, trailing) = split_trailing_comment(raw);
        let norm = normalize(&code_part);
        if !norm.is_empty() {
            let occ = {
                let e = anchor_counts.entry(norm.clone()).or_insert(0);
                let v = *e;
                *e += 1;
                v
            };
            if !pending_block.is_empty() {
                for c in pending_block.drain(..) {
                    items.push(CommentItem {
                        text: c,
                        trailing: false,
                        anchor: Some(norm.clone()),
                        occurrence: occ,
                    });
                }
            }
            if let Some(t) = trailing {
                items.push(CommentItem {
                    text: t,
                    trailing: true,
                    anchor: Some(norm.clone()),
                    occurrence: occ,
                });
            }
        } else if trailing.is_some() {
            // comment after nothing but whitespace was handled as standalone
            pending_block.push(trailing.unwrap());
        }
        let _ = idx;
    }
    // comments at EOF with no following code line
    let eof_block: Vec<String> = pending_block.drain(..).collect();

    if items.is_empty() && eof_block.is_empty() {
        return Ok(Some(formatted));
    }

    // ---- re-anchor against formatted output ---------------------------
    let fmt_lines: Vec<String> = formatted.lines().map(|l| l.to_string()).collect();
    let mut fmt_counts: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
    for (i, l) in fmt_lines.iter().enumerate() {
        let n = normalize(l);
        if !n.is_empty() {
            fmt_counts.entry(n).or_default().push(i);
        }
    }

    // For every comment, find its formatted line index; bail on any miss.
    // insertions: line_idx -> (before-lines, trailing-comments)
    let mut before: std::collections::HashMap<usize, Vec<String>> = std::collections::HashMap::new();
    let mut trail: std::collections::HashMap<usize, Vec<String>> = std::collections::HashMap::new();
    for item in &items {
        let Some(anchor) = &item.anchor else { return Ok(None) };
        let Some(cands) = fmt_counts.get(anchor) else { return Ok(None) };
        let Some(&line_idx) = cands.get(item.occurrence) else { return Ok(None) };
        if item.trailing {
            trail.entry(line_idx).or_default().push(item.text.clone());
        } else {
            before.entry(line_idx).or_default().push(item.text.clone());
        }
    }

    // ---- rebuild -------------------------------------------------------
    let mut out = String::with_capacity(formatted.len() + 256);
    for (i, l) in fmt_lines.iter().enumerate() {
        if let Some(cs) = before.get(&i) {
            let indent: String = l.chars().take_while(|c| *c == ' ').collect();
            for c in cs {
                out.push_str(&indent);
                out.push_str(c);
                out.push('\n');
            }
        }
        out.push_str(l);
        if let Some(ts) = trail.get(&i) {
            for t in ts {
                out.push(' ');
                out.push_str(t);
            }
        }
        out.push('\n');
    }
    for c in &eof_block {
        out.push_str(c);
        out.push('\n');
    }

    // Safety: the result must still parse to the same formatted AST.
    match format_source(&out) {
        Ok(refmt) if refmt == formatted => Ok(Some(out)),
        _ => Ok(None),
    }
}
