//! `kryos eval <expr>` — one-liner evaluation.
//!
//! Wraps the expression in a generated `main` and writes it to a temp file,
//! then invokes the existing `kryos run` path. Useful for quick checks:
//!
//!     kryos eval 'println(to_string(42 + 1))'
//!     # → 43
//!
//! For multi-statement input separated by `;` the helper rewrites them as
//! newline-terminated statements first (Kryos uses newlines, not semicolons,
//! to terminate statements at parse time).

#[derive(Debug, Clone, Default)]
pub struct EvalOptions {
    /// The expression / statements to evaluate.
    pub source: String,
    /// If true, emit the generated source to stderr before running.
    pub show_source: bool,
}

pub fn execute(opts: EvalOptions) -> Result<(), String> {
    let body = rewrite_semicolons(&opts.source);
    let wrapped = format!("fn main() {{\n    {body}\n}}\n");

    if opts.show_source {
        eprintln!("\x1b[36mkryos eval\x1b[0m wrapping:");
        for (i, line) in wrapped.lines().enumerate() {
            eprintln!("  {:>3} | {}", i + 1, line);
        }
        eprintln!();
    }

    let tmp = std::env::temp_dir().join(format!("kryos_eval_{}.kry", std::process::id()));
    std::fs::write(&tmp, wrapped.as_bytes())
        .map_err(|e| format!("kryos eval: write tmp: {e}"))?;

    let result = super::run::execute(&tmp.to_string_lossy(), &[], kryos_driver::CapabilityMode::Inferred);
    let _ = std::fs::remove_file(&tmp);
    result
}

/// Resolve `kryos eval [-v|--show-source] <expr...>` argv into options.
/// The CLI binds `expr` as `Vec<String>` (trailing var args). We join on
/// spaces so multi-token strings round-trip.
pub fn from_argv(expr: Vec<String>, show_source: bool) -> EvalOptions {
    EvalOptions {
        source: expr.join(" "),
        show_source,
    }
}

/// Replace statement-separating `;` with newlines, but ONLY outside string
/// and char literals — a naive `replace(';', "\n")` split `println("a;b")`
/// into three lines and corrupted the string (backlog #99).
fn rewrite_semicolons(src: &str) -> String {
    let b = src.as_bytes();
    let n = b.len();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < n {
        match b[i] {
            b'"' | b'\'' => {
                let quote = b[i];
                out.push(quote);
                i += 1;
                while i < n && b[i] != quote {
                    if b[i] == b'\\' && i + 1 < n {
                        out.push(b[i]);
                        out.push(b[i + 1]);
                        i += 2;
                        continue;
                    }
                    out.push(b[i]);
                    i += 1;
                }
                if i < n {
                    out.push(quote);
                    i += 1;
                }
            }
            b';' => {
                out.push(b'\n');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    // Input was valid UTF-8 and we only reordered/duplicated whole bytes on
    // literal boundaries, so this is always valid UTF-8.
    String::from_utf8(out).unwrap_or_else(|_| src.to_string())
}

/// For testability we expose the wrap-and-return-source path separately.
#[allow(dead_code)] // used by #[cfg(test)] mod tests below
pub fn wrap_body(body: &str) -> String {
    let cleaned = rewrite_semicolons(body);
    format!("fn main() {{\n    {cleaned}\n}}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_inserts_main_function() {
        let out = wrap_body("println(\"hi\")");
        assert!(out.contains("fn main()"));
        assert!(out.contains("println(\"hi\")"));
    }

    #[test]
    fn semicolons_become_newlines() {
        let out = wrap_body("let x = 1; let y = 2");
        // Semicolons must be replaced; both let bindings present in output.
        assert!(!out.contains(';'), "semicolons should be rewritten");
        assert!(out.contains("let x = 1"));
        assert!(out.contains("let y = 2"));
    }
}
