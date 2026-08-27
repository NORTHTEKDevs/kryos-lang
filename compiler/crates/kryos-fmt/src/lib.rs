//! Kryos code formatter — takes an AST and produces canonically formatted source code.
//!
//! The formatter uses 4-space indentation, 80-column line width for signature wrapping,
//! and blank lines between top-level declarations.

#![allow(clippy::too_many_arguments)]

pub mod formatter;

use kryos_ast::Module;
use kryos_errors::Diagnostic;
use kryos_lexer::Lexer;
use kryos_parser::{parse, parse_with_diagnostics};

pub use formatter::Formatter;

/// True if parsing `source` emits a W0001 warning (an ambiguous newline-led
/// `||`/`-`/`[`/`(` continuation -- CLAUDE.md hard rule 1). The parser
/// silently picks the CONTINUE reading (an already-baked-in behavior, kept
/// for backward compat), so an AST-based re-emit like `format_source` would
/// otherwise print that exact merged reading back out in clean, canonical
/// formatting -- permanently erasing the only trace (the original line
/// break) that the source was ever ambiguous, and doing so with NO warning
/// at all, because `format_source` parses via the diagnostic-discarding
/// `parse()` rather than `parse_with_diagnostics()`. Callers that must not
/// silently launder the trap (the `kryos fmt` CLI) should call this FIRST
/// and refuse to format (skip, leave the file untouched) when it returns
/// true, mirroring the existing "can't re-anchor a comment -> skip, don't
/// destroy" policy `format_source_preserving_comments` already uses.
pub fn source_has_ambiguous_continuation(source: &str) -> bool {
    let tokens = Lexer::new(source, 0).tokenize();
    let (_module, diagnostics) = parse_with_diagnostics(tokens);
    diagnostics
        .iter()
        .any(|d| d.code.as_deref() == Some(kryos_errors::codes::W0001))
}

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
        // Canonicalize beyond whitespace so an anchor still matches its
        // formatted counterpart when the formatter rewrites the line:
        //  - strip a TRAILING comma (fmt drops separators on enum variants,
        //    struct fields, and match arms: `Red,` -> `Red`)
        //  - fold `.` to `::` (fmt rewrites `Color.Red =>` to `Color::Red
        //    =>`); applied to BOTH sides, so ordinary method-call lines stay
        //    self-consistent -- this is only a matching key, never output.
        line.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .trim_end_matches(',')
            .trim_end()
            .replace('.', "::")
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
    // Formatted lines already claimed by a prefix-fallback match, so two
    // comments on identical-looking split arms anchor to distinct lines in
    // source order. Comments sharing the SAME (anchor, occurrence) -- a
    // multi-line comment block above one code line -- must all attach to the
    // same resolved line, so cache the resolution per anchor identity and
    // reuse it instead of re-searching (the claimed-set would otherwise send
    // the block's second comment hunting for a different line and skip the
    // whole file).
    let mut prefix_claimed: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut prefix_cache: std::collections::HashMap<(String, usize), usize> =
        std::collections::HashMap::new();
    for item in &items {
        let Some(anchor) = &item.anchor else { return Ok(None) };
        let line_idx = if let Some(cands) = fmt_counts.get(anchor) {
            match cands.get(item.occurrence) {
                Some(&i) => i,
                None => return Ok(None),
            }
        } else {
            // Structural fallbacks, tried in order. A miss still skips the
            // file; a mismatch is caught by the reparse check below, and a
            // comment can at worst sit one line off -- never be deleted.
            //
            // 1. SPLIT: the formatter split the anchor line (a one-line match
            //    arm `Color.Red => { return "red" }` becomes `Color::Red => {`
            //    + a body line) -- attach to the earliest UNCLAIMED formatted
            //    line whose canonical form is a PREFIX of the anchor (>= 4
            //    chars, so bare braces never match). Claimed-tracking keeps
            //    two identical-looking split arms anchored in source order.
            // 2. JOINED: the formatter merged the anchor's multi-line
            //    construct into one longer line (`let x = if c {` + arms ->
            //    one line) -- attach to the first formatted line the anchor
            //    is a PREFIX of. Sharing is fine here (several comments may
            //    stack before the same joined line).
            // 3. ABSORBED: the anchor line was folded into the MIDDLE of a
            //    joined line -- attach to the first formatted line that
            //    CONTAINS it (>= 8 chars to keep this conservative).
            let cache_key = (anchor.clone(), item.occurrence);
            if let Some(&cached) = prefix_cache.get(&cache_key) {
                cached
            } else {
                let mut found: Option<usize> = None;
                for (i, l) in fmt_lines.iter().enumerate() {
                    if prefix_claimed.contains(&i) {
                        continue;
                    }
                    let k = normalize(l);
                    if k.len() >= 4 && anchor.starts_with(&k) {
                        found = Some(i);
                        prefix_claimed.insert(i);
                        break;
                    }
                }
                if found.is_none() {
                    for (i, l) in fmt_lines.iter().enumerate() {
                        let k = normalize(l);
                        if k.starts_with(anchor.as_str()) {
                            found = Some(i);
                            break;
                        }
                    }
                }
                if found.is_none() && anchor.len() >= 8 {
                    for (i, l) in fmt_lines.iter().enumerate() {
                        if normalize(l).contains(anchor.as_str()) {
                            found = Some(i);
                            break;
                        }
                    }
                }
                match found {
                    Some(i) => {
                        prefix_cache.insert(cache_key, i);
                        i
                    }
                    None => return Ok(None),
                }
            }
        };
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
