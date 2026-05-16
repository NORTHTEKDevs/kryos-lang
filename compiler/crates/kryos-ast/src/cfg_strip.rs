//! `@cfg(...)` conditional compilation pass.
//!
//! Strips top-level declarations whose `@cfg(...)` predicate does not hold
//! for the current `CfgContext`. Runs after `expand_derives` and before
//! type-checking, so the rest of the pipeline never sees decls that don't
//! apply to the current target.
//!
//! # Supported predicates
//!
//! Each argument inside `@cfg(arg1, arg2, ...)` is an independent predicate
//! and **all** must hold (AND semantics). Recognised predicates:
//!
//! | Predicate | Holds when |
//! |-----------|-----------|
//! | `linux`   | target OS is Linux |
//! | `windows` | target OS is Windows |
//! | `macos`   | target OS is macOS |
//! | `unix`    | target OS is Linux or macOS |
//! | `debug`   | build mode is Debug |
//! | `release` | build mode is Release |
//!
//! Any predicate that is not recognised evaluates to **false**, so the
//! decl is stripped. This is conservative: the compiler never compiles
//! code that asked for a feature gate it doesn't understand.
//!
//! Combinators `not(P)`, `all(P, Q, ...)`, and `any(P, Q, ...)` are also
//! recognised and may be nested arbitrarily. `all()` with no args is
//! vacuously true; `any()` with no args is false. Anything that fails to
//! parse falls through to the unrecognised-predicate rule (false).
//!
//! # Where it runs
//!
//! Driven from `kryos-driver`'s pipeline immediately after `expand_derives`
//! and before `type_check`. A `@cfg(...)`-stripped decl is *completely*
//! removed from the module — its symbols, types, and body never reach
//! type inference, MIR, or codegen.

use crate::decl::{Annotation, Decl, Module};

/// Build-time context the cfg pass evaluates predicates against.
#[derive(Debug, Clone)]
pub struct CfgContext {
    /// Target OS string, normalised lower-case. Examples: `"linux"`,
    /// `"windows"`, `"macos"`.
    pub target_os: String,
    /// True for release builds, false for debug.
    pub release: bool,
}

impl CfgContext {
    /// Build a context that reflects the host the compiler itself is
    /// running on, in debug mode. Mostly used by tests.
    pub fn host_debug() -> Self {
        Self {
            target_os: host_target_os().to_string(),
            release: false,
        }
    }

    /// Build a context for the host OS in the requested release mode.
    pub fn for_host(release: bool) -> Self {
        Self {
            target_os: host_target_os().to_string(),
            release,
        }
    }

    /// Build a context from an explicit target triple. If `triple` is
    /// `None`, falls back to the host OS.
    pub fn from_triple(triple: Option<&str>, release: bool) -> Self {
        let target_os = triple
            .and_then(parse_os_from_triple)
            .unwrap_or_else(|| host_target_os().to_string());
        Self {
            target_os,
            release,
        }
    }
}

/// Strip every `Decl` whose `@cfg(...)` predicate does not hold for `ctx`.
/// Decls without any `@cfg` annotation are always kept.
pub fn strip_cfg(module: &mut Module, ctx: &CfgContext) {
    module
        .declarations
        .retain(|decl| should_keep_decl(decl, ctx));
}

fn should_keep_decl(decl: &Decl, ctx: &CfgContext) -> bool {
    let annotations = decl_annotations(decl);
    match annotations {
        Some(anns) => anns
            .iter()
            .filter(|a| a.name == "cfg")
            .all(|a| evaluate(a, ctx)),
        None => true,
    }
}

/// Return the annotation list for a decl variant that carries one, or
/// `None` for variants that don't (`Trait`, `Impl`, `TypeAlias`, ...).
fn decl_annotations(decl: &Decl) -> Option<&Vec<Annotation>> {
    match decl {
        Decl::Function { annotations, .. }
        | Decl::Struct { annotations, .. }
        | Decl::Enum { annotations, .. }
        | Decl::Actor { annotations, .. } => Some(annotations),
        _ => None,
    }
}

/// Evaluate a single `@cfg(...)` annotation. AND-semantics across args:
/// every arg must match. Empty `@cfg()` is treated as "always true" so it
/// never accidentally removes a decl (useful for placeholders).
fn evaluate(ann: &Annotation, ctx: &CfgContext) -> bool {
    if ann.args.is_empty() {
        return true;
    }
    ann.args.iter().all(|arg| match_predicate(arg, ctx))
}

fn match_predicate(arg: &str, ctx: &CfgContext) -> bool {
    let trimmed = arg.trim();
    // Handle combinators first. `not(X)` / `all(...)` / `any(...)`.
    if let Some(inner) = strip_call(trimmed, "not") {
        // `not(...)` requires exactly one inner predicate.
        let parts = split_top_level(inner);
        if parts.len() != 1 {
            return false;
        }
        return !match_predicate(&parts[0], ctx);
    }
    if let Some(inner) = strip_call(trimmed, "all") {
        let parts = split_top_level(inner);
        return parts.iter().all(|p| match_predicate(p, ctx));
    }
    if let Some(inner) = strip_call(trimmed, "any") {
        let parts = split_top_level(inner);
        return parts.iter().any(|p| match_predicate(p, ctx));
    }
    let key = trimmed.to_ascii_lowercase();
    match key.as_str() {
        "linux" => ctx.target_os == "linux",
        "windows" => ctx.target_os == "windows",
        "macos" => ctx.target_os == "macos",
        "unix" => matches!(ctx.target_os.as_str(), "linux" | "macos"),
        "debug" => !ctx.release,
        "release" => ctx.release,
        _ => false, // Unknown predicates fail closed.
    }
}

/// If `s` looks like `name(...)` with balanced parens, return the inside
/// of the parens. Whitespace between `name` and `(` is tolerated.
fn strip_call<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let s = s.trim();
    let lower = s.to_ascii_lowercase();
    if !lower.starts_with(name) {
        return None;
    }
    let rest = s[name.len()..].trim_start();
    if !rest.starts_with('(') || !rest.ends_with(')') {
        return None;
    }
    // Confirm the closing paren matches the opening paren (i.e. the whole
    // tail of `s` is the parenthesised group, not e.g. `not(x) trailing`).
    let bytes = rest.as_bytes();
    let mut depth: i32 = 0;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    // Must end exactly at the last byte.
                    if i + 1 != bytes.len() {
                        return None;
                    }
                    return Some(&rest[1..i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Split `s` at top-level commas, respecting nested parentheses.
fn split_top_level(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth: i32 = 0;
    let mut cur = String::new();
    for ch in s.chars() {
        match ch {
            '(' => {
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if depth == 0 => {
                let t = cur.trim().to_string();
                if !t.is_empty() {
                    out.push(t);
                }
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    let t = cur.trim().to_string();
    if !t.is_empty() {
        out.push(t);
    }
    out
}

/// Resolve the host operating system, lower-cased. Maps Rust's
/// `cfg!(target_os = "...")` literals onto our normalised set.
fn host_target_os() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        // Be honest about unknown hosts so unit tests on exotic CIs don't
        // silently match a wildcard.
        "unknown"
    }
}

/// Best-effort OS extraction from a target triple like
/// `x86_64-unknown-linux-gnu`. Returns lower-case OS or `None` if the
/// triple is unparseable.
fn parse_os_from_triple(triple: &str) -> Option<String> {
    let t = triple.to_ascii_lowercase();
    if t.contains("linux") {
        Some("linux".into())
    } else if t.contains("windows") || t.contains("msvc") || t.contains("mingw") {
        Some("windows".into())
    } else if t.contains("darwin") || t.contains("apple") || t.contains("macos") {
        Some("macos".into())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decl::Decl;
    use crate::stmt::Block;
    use kryos_errors::Span;

    fn dummy_span() -> Span {
        Span::new(0, 0, 0)
    }

    fn func_with_cfg(name: &str, cfg_args: Vec<&str>) -> Decl {
        let span = dummy_span();
        let annotations = if cfg_args.is_empty() {
            Vec::new()
        } else {
            vec![Annotation {
                name: "cfg".to_string(),
                args: cfg_args.into_iter().map(String::from).collect(),
                span,
            }]
        };
        Decl::Function {
            name: name.to_string(),
            generics: Vec::new(),
            params: Vec::new(),
            ret_ty: None,
            body: Some(Block {
                stmts: Vec::new(),
                span,
            }),
            public: true,
            is_async: false,
            annotations,
            doc_comments: Vec::new(),
            span,
        }
    }

    fn decl_names(m: &Module) -> Vec<&str> {
        m.declarations
            .iter()
            .filter_map(|d| match d {
                Decl::Function { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn keep_when_predicate_matches() {
        let mut m = Module {
            name: "t".into(),
            declarations: vec![
                func_with_cfg("linux_only", vec!["linux"]),
                func_with_cfg("always", vec![]),
            ],
            span: dummy_span(),
        };
        let ctx = CfgContext {
            target_os: "linux".into(),
            release: false,
        };
        strip_cfg(&mut m, &ctx);
        assert_eq!(decl_names(&m), vec!["linux_only", "always"]);
    }

    #[test]
    fn strip_when_predicate_fails() {
        let mut m = Module {
            name: "t".into(),
            declarations: vec![
                func_with_cfg("linux_only", vec!["linux"]),
                func_with_cfg("windows_only", vec!["windows"]),
            ],
            span: dummy_span(),
        };
        let ctx = CfgContext {
            target_os: "linux".into(),
            release: false,
        };
        strip_cfg(&mut m, &ctx);
        assert_eq!(decl_names(&m), vec!["linux_only"]);
    }

    #[test]
    fn multiple_args_are_and() {
        let mut m = Module {
            name: "t".into(),
            declarations: vec![
                func_with_cfg("linux_release", vec!["linux", "release"]),
                func_with_cfg("linux_debug", vec!["linux", "debug"]),
            ],
            span: dummy_span(),
        };
        let ctx = CfgContext {
            target_os: "linux".into(),
            release: true,
        };
        strip_cfg(&mut m, &ctx);
        assert_eq!(decl_names(&m), vec!["linux_release"]);
    }

    #[test]
    fn unix_matches_linux_and_macos() {
        let mut m_linux = Module {
            name: "t".into(),
            declarations: vec![func_with_cfg("u", vec!["unix"])],
            span: dummy_span(),
        };
        let mut m_win = m_linux.clone();
        strip_cfg(
            &mut m_linux,
            &CfgContext {
                target_os: "linux".into(),
                release: false,
            },
        );
        strip_cfg(
            &mut m_win,
            &CfgContext {
                target_os: "windows".into(),
                release: false,
            },
        );
        assert_eq!(decl_names(&m_linux), vec!["u"]);
        assert!(decl_names(&m_win).is_empty());
    }

    #[test]
    fn unknown_predicate_fails_closed() {
        let mut m = Module {
            name: "t".into(),
            declarations: vec![
                func_with_cfg("gated", vec!["sparc_v9"]),
                func_with_cfg("always", vec![]),
            ],
            span: dummy_span(),
        };
        strip_cfg(
            &mut m,
            &CfgContext {
                target_os: "linux".into(),
                release: false,
            },
        );
        assert_eq!(decl_names(&m), vec!["always"]);
    }

    #[test]
    fn empty_args_keeps_decl() {
        let mut m = Module {
            name: "t".into(),
            declarations: vec![func_with_cfg("ungated_but_marked", vec![])],
            span: dummy_span(),
        };
        strip_cfg(&mut m, &CfgContext::host_debug());
        assert_eq!(decl_names(&m).len(), 1);
    }

    #[test]
    fn not_combinator() {
        let ctx = CfgContext {
            target_os: "linux".into(),
            release: false,
        };
        // not(windows) on linux => true (keep)
        assert!(match_predicate("not(windows)", &ctx));
        // not(linux) on linux => false (strip)
        assert!(!match_predicate("not(linux)", &ctx));
        // Nested: not(not(linux)) on linux => true
        assert!(match_predicate("not(not(linux))", &ctx));
        // Malformed: not() with no args => false
        assert!(!match_predicate("not()", &ctx));
        // Malformed: not(a, b) => false (single-arg rule)
        assert!(!match_predicate("not(linux, debug)", &ctx));
    }

    #[test]
    fn all_combinator() {
        let ctx = CfgContext {
            target_os: "linux".into(),
            release: false,
        };
        assert!(match_predicate("all(linux, debug)", &ctx));
        assert!(!match_predicate("all(linux, release)", &ctx));
        assert!(!match_predicate("all(windows, debug)", &ctx));
        // Empty all() is vacuously true.
        assert!(match_predicate("all()", &ctx));
        // Nested.
        assert!(match_predicate("all(linux, not(release))", &ctx));
    }

    #[test]
    fn any_combinator() {
        let ctx = CfgContext {
            target_os: "linux".into(),
            release: false,
        };
        assert!(match_predicate("any(linux, windows)", &ctx));
        assert!(match_predicate("any(windows, linux)", &ctx));
        assert!(!match_predicate("any(windows, macos)", &ctx));
        // Empty any() is false.
        assert!(!match_predicate("any()", &ctx));
        // Nested combinators.
        assert!(match_predicate("any(windows, all(linux, debug))", &ctx));
        assert!(!match_predicate("any(windows, all(linux, release))", &ctx));
    }

    #[test]
    fn deeply_nested_combinators() {
        let ctx = CfgContext {
            target_os: "macos".into(),
            release: true,
        };
        // unix && release && !windows
        assert!(match_predicate(
            "all(unix, release, not(windows))",
            &ctx
        ));
        // (linux || macos) && !debug
        assert!(match_predicate(
            "all(any(linux, macos), not(debug))",
            &ctx
        ));
    }

    #[test]
    fn triple_parser_basics() {
        assert_eq!(
            parse_os_from_triple("x86_64-unknown-linux-gnu").as_deref(),
            Some("linux")
        );
        assert_eq!(
            parse_os_from_triple("x86_64-pc-windows-msvc").as_deref(),
            Some("windows")
        );
        assert_eq!(
            parse_os_from_triple("aarch64-apple-darwin").as_deref(),
            Some("macos")
        );
        assert!(parse_os_from_triple("wasm32-unknown-unknown").is_none());
    }
}
