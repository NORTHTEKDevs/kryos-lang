//! `kryos audit` -- capability + extern + secret usage report.
//!
//! Walks every `.kry` file under the project and produces:
//!
//! 1. **Parse failures** -- any file `audit` could not read, lex, or parse
//!    at all. THE MOST IMPORTANT SECTION: a file audit cannot see into is
//!    reported LOUDLY here and forces a nonzero exit, rather than the file
//!    silently vanishing from every other section as if it were clean.
//! 2. **Capability violations** -- the SAME inferred-mode capability
//!    inference/enforcement pass `kryos check`/`run`/`build` use, run
//!    per-file and filtered to capability/extern-gate diagnostics
//!    (E0500-E0508). LEDGER item 13: the report below (annotation text
//!    only) used to come back clean on code `kryos check` rejects outright.
//! 3. **Capability inventory** -- every function with an `@capabilities(...)`
//!    annotation, grouped by capability. A TEXTUAL inventory of what is
//!    declared, not what is required.
//! 4. **Extern surface** -- every `extern "C" { ... }` block and the items
//!    it declares.
//! 5. **Secret patterns** -- string literals matching common credential
//!    shapes.
//!
//! `audit` is a report, not a gate: it exits non-zero when it finds a real
//! capability violation OR a file it could not parse, so CI can treat it as
//! a genuine check, but it is not a substitute for `kryos check`/`build`.
//!
//! Output is human-readable by default; `--format=json` emits a single JSON
//! document on stdout.

use std::fs;
use std::path::{Path, PathBuf};

use kryos_ast::Decl;
use kryos_driver::CapabilityMode;
use kryos_errors::SourceMap;

#[derive(Debug, Clone)]
pub struct AuditOptions {
    pub path: Option<String>,
    pub format: String,
}

impl Default for AuditOptions {
    fn default() -> Self {
        Self { path: None, format: "pretty".into() }
    }
}

#[derive(Debug, Default)]
struct AuditReport {
    parse_failures: Vec<ParseFailure>,
    cap_violations: Vec<CapViolation>,
    capabilities: std::collections::BTreeMap<String, Vec<String>>,
    extern_blocks: Vec<ExternEntry>,
    secrets: Vec<SecretEntry>,
}

/// A file `audit` could NOT analyze -- failed to be read as UTF-8, or
/// failed lexing, or failed parsing. Before this fix such a file vanished
/// from the report completely: `scan_file` did
/// `let Ok(module) = kryos_parser::parse(tokens) else { return }`,
/// discarding the diagnostics and returning before the extern/capability
/// AST walk AND before `check_cap_violations` ran. A file containing an
/// extern block declaring a dangerous native symbol plus
/// `@capabilities(all)`, wrapped in a `main` missing a close-paren, audited
/// as clean, rc=0 -- indistinguishable from a genuinely safe file.
#[derive(Debug)]
struct ParseFailure {
    file: PathBuf,
    stage: &'static str,
    message: String,
    line: u32,
    col: u32,
}

/// A capability/extern-gate diagnostic (E0500-E0508) that `kryos
/// check`/`run`/`build` would reject. See LEDGER item 13.
#[derive(Debug)]
struct CapViolation {
    file: PathBuf,
    code: String,
    message: String,
    line: u32,
    col: u32,
}

const CAP_VIOLATION_CODES: &[&str] = &[
    "E0500", "E0501", "E0502", "E0503", "E0504", "E0505", "E0506", "E0507", "E0508",
];

#[derive(Debug)]
struct ExternEntry {
    file: PathBuf,
    abi: String,
    item_count: usize,
}

#[derive(Debug)]
struct SecretEntry {
    file: PathBuf,
    line: u32,
    pattern: &'static str,
    excerpt: String,
}

pub fn execute(opts: AuditOptions) -> Result<(), String> {
    let root = match opts.path.as_deref() {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from("."),
    };
    if !root.exists() {
        return Err(format!("kryos audit: path '{}' does not exist", root.display()));
    }

    let mut files: Vec<PathBuf> = Vec::new();
    if root.is_file() {
        files.push(root.clone());
    } else {
        collect_kry(&root, &mut files);
    }

    let mut report = AuditReport::default();
    for f in &files {
        scan_file(f, &mut report);
    }

    match opts.format.as_str() {
        "json" => emit_json(&report),
        _ => emit_pretty(&report, files.len()),
    }

    let mut problems: Vec<String> = Vec::new();
    if !report.parse_failures.is_empty() {
        problems.push(format!(
            "{} file{} could not be parsed -- capability/extern/secret analysis is INCOMPLETE for {} (see \"Parse failures\" above); a file audit cannot read is NEVER reported clean",
            report.parse_failures.len(),
            if report.parse_failures.len() == 1 { "" } else { "s" },
            if report.parse_failures.len() == 1 { "it" } else { "them" }
        ));
    }
    if !report.cap_violations.is_empty() {
        problems.push(format!(
            "{} capability violation{} found -- this code would be REJECTED by `kryos check`/`kryos build` (see \"Capability violations\" above)",
            report.cap_violations.len(),
            if report.cap_violations.len() == 1 { "" } else { "s" }
        ));
    }
    if !problems.is_empty() {
        return Err(format!(
            "kryos audit: {}. `kryos audit` is a report, not a substitute for `kryos check`.",
            problems.join("; ")
        ));
    }

    Ok(())
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

fn scan_file(path: &Path, report: &mut AuditReport) {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            report.parse_failures.push(ParseFailure {
                file: path.to_path_buf(),
                stage: "read",
                message: format!("could not read file: {e}"),
                line: 0,
                col: 0,
            });
            return;
        }
    };

    for (idx, line) in source.lines().enumerate() {
        for (pat, name) in SECRET_PATTERNS {
            if line.contains(pat) {
                report.secrets.push(SecretEntry {
                    file: path.to_path_buf(),
                    line: idx as u32 + 1,
                    pattern: name,
                    excerpt: line.trim().chars().take(120).collect(),
                });
            }
        }
    }

    // Lexer AND parser diagnostics are both captured now (previously
    // `.tokenize()` silently dropped lexer-level errors like an
    // unterminated string, and a parser error discarded its diagnostics
    // entirely via `let Ok(module) = ... else { return }` -- both are now
    // recorded as a loud ParseFailure instead of vanishing).
    let (tokens, lex_diags) = kryos_lexer::Lexer::new(&source, 0).tokenize_with_diagnostics();
    let lex_error = lex_diags.iter().find(|d| d.is_error());
    let parse_result = kryos_parser::parse(tokens);

    let first_error = lex_error.or_else(|| match &parse_result {
        Err(diags) => diags.iter().find(|d| d.is_error()),
        Ok(_) => None,
    });

    if let Some(diag) = first_error {
        let stage = if lex_error.is_some() { "lex" } else { "parse" };
        let mut sm = SourceMap::default();
        let fid = sm.add_file(path.display().to_string(), source.clone());
        let (line, col) = diag
            .labels
            .iter()
            .find(|l| l.is_primary)
            .or_else(|| diag.labels.first())
            .map(|l| sm.offset_to_line_col(fid, l.span.start))
            .unwrap_or((0, 0));
        report.parse_failures.push(ParseFailure {
            file: path.to_path_buf(),
            stage,
            message: diag.message.clone(),
            line,
            col,
        });
        // Cannot safely walk an AST that does not exist, and re-running the
        // real capability checker on a file that does not parse would only
        // rediscover the same failure under a different code path -- the
        // ParseFailure above already makes this loud and fails the run.
        return;
    }

    let module = parse_result.expect("checked for parser errors above");

    for decl in &module.declarations {
        match decl {
            Decl::Function { name, annotations, .. } => {
                for ann in annotations {
                    if ann.name == "capabilities" {
                        for cap in &ann.args {
                            report
                                .capabilities
                                .entry(cap.clone())
                                .or_default()
                                .push(name.clone());
                        }
                    }
                }
            }
            Decl::Extern { abi, items, .. } => {
                report.extern_blocks.push(ExternEntry {
                    file: path.to_path_buf(),
                    abi: abi.clone(),
                    item_count: items.len(),
                });
            }
            _ => {}
        }
    }

    check_cap_violations(path, report);
}

fn check_cap_violations(path: &Path, report: &mut AuditReport) {
    let (diagnostics, source_map) =
        kryos_driver::check_file_with_options_full(path, true, CapabilityMode::Inferred);
    for diag in &diagnostics {
        if !diag.is_error() {
            continue;
        }
        let Some(code) = diag.code.as_deref() else { continue };
        if !CAP_VIOLATION_CODES.contains(&code) {
            continue;
        }
        let (line, col) = diag
            .labels
            .iter()
            .find(|l| l.is_primary)
            .or_else(|| diag.labels.first())
            .map(|l| source_map.offset_to_line_col(l.span.file_id, l.span.start))
            .unwrap_or((0, 0));
        report.cap_violations.push(CapViolation {
            file: path.to_path_buf(),
            code: code.to_string(),
            message: diag.message.clone(),
            line,
            col,
        });
    }
}

const SECRET_PATTERNS: &[(&str, &str)] = &[
    ("AKIA", "AWS-access-key-id"),
    ("ghp_", "GitHub-personal-token"),
    ("github_pat_", "GitHub-fine-grained-PAT"),
    ("xoxb-", "Slack-bot-token"),
    ("xoxp-", "Slack-user-token"),
    ("sk-", "OpenAI-API-key-prefix"),
    ("Bearer ", "Bearer-auth-header"),
    ("BEGIN PRIVATE KEY", "PEM-private-key"),
    ("BEGIN RSA PRIVATE KEY", "PEM-RSA-private-key"),
    ("BEGIN OPENSSH PRIVATE KEY", "OpenSSH-private-key"),
    ("password=", "url-password-param"),
    ("API_KEY=", "API-key-env-assignment"),
    ("api_key=", "API-key-env-assignment"),
];

fn emit_pretty(report: &AuditReport, file_count: usize) {
    println!("\x1b[1mkryos audit\x1b[0m");
    println!("scanned {} file{}", file_count, if file_count == 1 { "" } else { "s" });
    println!("note: audit is a report, not a substitute for `kryos check`/`kryos build`.");
    println!();

    println!("\x1b[1m== Parse failures (audit could NOT analyze these files) ==\x1b[0m");
    if report.parse_failures.is_empty() {
        println!("  \x1b[32m(none -- every scanned file lexed and parsed cleanly)\x1b[0m");
    } else {
        for p in &report.parse_failures {
            println!(
                "  \x1b[31mCRITICAL\x1b[0m {}:{}:{} [{}] {}",
                p.file.display(),
                p.line,
                p.col,
                p.stage,
                p.message
            );
        }
        println!();
        println!(
            "\x1b[31m{} file(s) failed to parse -- every OTHER section below is INCOMPLETE for these files, not a clean bill for them.\x1b[0m",
            report.parse_failures.len()
        );
    }

    println!();
    println!("\x1b[1m== Capability violations (kryos check would reject) ==\x1b[0m");
    if report.cap_violations.is_empty() {
        println!("  \x1b[32m(none -- every file passes the same inferred-mode capability check `kryos check` runs)\x1b[0m");
    } else {
        for v in &report.cap_violations {
            println!(
                "  \x1b[31mCRITICAL\x1b[0m {}:{}:{} [{}] {}",
                v.file.display(),
                v.line,
                v.col,
                v.code,
                v.message
            );
        }
        println!();
        println!(
            "\x1b[31m{} capability violation(s) -- this code will NOT compile with `kryos check`/`kryos build`.\x1b[0m",
            report.cap_violations.len()
        );
    }

    println!();
    println!("\x1b[1m== Capability inventory (declared annotations only) ==\x1b[0m");
    if report.capabilities.is_empty() {
        println!("  (no @capabilities annotations found)");
    } else {
        for (cap, fns) in &report.capabilities {
            println!("  {cap}: {} function{}", fns.len(), if fns.len() == 1 { "" } else { "s" });
            for f in fns {
                println!("    - {f}");
            }
        }
    }
    if !report.parse_failures.is_empty() {
        println!(
            "  \x1b[33m({} unparseable file(s) excluded from this section -- see Parse failures above)\x1b[0m",
            report.parse_failures.len()
        );
    }

    println!();
    println!("\x1b[1m== Extern blocks ==\x1b[0m");
    if report.extern_blocks.is_empty() {
        println!("  (no extern blocks)");
    } else {
        for ex in &report.extern_blocks {
            println!(
                "  {}: extern \"{}\" -- {} item{}",
                ex.file.display(),
                ex.abi,
                ex.item_count,
                if ex.item_count == 1 { "" } else { "s" }
            );
        }
    }
    if !report.parse_failures.is_empty() {
        println!(
            "  \x1b[33m({} unparseable file(s) excluded from this section -- see Parse failures above)\x1b[0m",
            report.parse_failures.len()
        );
    }

    println!();
    println!("\x1b[1m== Secret patterns ==\x1b[0m");
    if report.secrets.is_empty() {
        println!("  \x1b[32m(none detected)\x1b[0m");
    } else {
        for s in &report.secrets {
            println!(
                "  \x1b[31mCRITICAL\x1b[0m {}:{} [{}] {}",
                s.file.display(),
                s.line,
                s.pattern,
                s.excerpt
            );
        }
        println!();
        println!("\x1b[31m{} potential secret(s) -- review and revoke if real.\x1b[0m", report.secrets.len());
    }
}

fn emit_json(report: &AuditReport) {
    let mut out = String::from("{");

    out.push_str("\"parse_failures\":[");
    for (i, p) in report.parse_failures.iter().enumerate() {
        if i > 0 { out.push(','); }
        out.push_str(&format!(
            r#"{{"file":"{}","stage":"{}","line":{},"col":{},"message":"{}"}}"#,
            p.file.display().to_string().replace('\\', "/"),
            p.stage,
            p.line,
            p.col,
            json_escape(&p.message)
        ));
    }
    out.push_str("],");

    out.push_str("\"capability_violations\":[");
    for (i, v) in report.cap_violations.iter().enumerate() {
        if i > 0 { out.push(','); }
        out.push_str(&format!(
            r#"{{"file":"{}","line":{},"col":{},"code":"{}","message":"{}"}}"#,
            v.file.display().to_string().replace('\\', "/"),
            v.line,
            v.col,
            v.code,
            json_escape(&v.message)
        ));
    }
    out.push_str("],");

    out.push_str("\"capabilities\":{");
    let mut first = true;
    for (cap, fns) in &report.capabilities {
        if !first { out.push(','); }
        first = false;
        out.push('"');
        out.push_str(cap);
        out.push_str("\":[");
        let mut f_first = true;
        for f in fns {
            if !f_first { out.push(','); }
            f_first = false;
            out.push('"');
            out.push_str(&json_escape(f));
            out.push('"');
        }
        out.push(']');
    }
    out.push_str("},");

    out.push_str("\"extern_blocks\":[");
    for (i, ex) in report.extern_blocks.iter().enumerate() {
        if i > 0 { out.push(','); }
        out.push_str(&format!(
            r#"{{"file":"{}","abi":"{}","item_count":{}}}"#,
            ex.file.display().to_string().replace('\\', "/"),
            ex.abi,
            ex.item_count
        ));
    }
    out.push_str("],");

    out.push_str("\"secrets\":[");
    for (i, s) in report.secrets.iter().enumerate() {
        if i > 0 { out.push(','); }
        out.push_str(&format!(
            r#"{{"file":"{}","line":{},"pattern":"{}","excerpt":"{}"}}"#,
            s.file.display().to_string().replace('\\', "/"),
            s.line,
            json_escape(&s.pattern),
            json_escape(&s.excerpt)
        ));
    }
    out.push_str("]}");
    println!("{}", out);
}

/// Escape a string for embedding in a JSON string literal. The old emitter
/// escaped only `"`, so a source excerpt containing a backslash (e.g. a
/// Windows path or a regex) produced invalid JSON like `"\Users"` that no
/// parser could read (backlog #124).
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
