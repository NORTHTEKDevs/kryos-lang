//! Pattern exhaustiveness checking for match expressions.
//!
//! After type-checking match arms, this module verifies that the patterns
//! cover all possible values of the subject type.  Non-exhaustive matches
//! produce warnings (not errors) — this is deliberately lenient since the
//! language is still young and hard errors would break existing code.

use std::collections::HashSet;

use kryos_ast::Pattern;
use kryos_errors::{Diagnostic, Span};

use crate::env::EnumDef;

/// Check that `patterns` exhaustively cover the subject type.
///
/// Returns a list of warnings for any missing cases.
///
/// * `subject_type_name` — `"bool"`, `"i64"`, `"str"`, or enum name.
/// * `patterns` — the patterns from the match arms.
/// * `enum_def` — if the subject is an enum, its definition (variant list).
/// * `span` — the span of the overall match expression, used for diagnostics.
pub fn check_exhaustive(
    subject_type_name: &str,
    patterns: &[&Pattern],
    enum_def: Option<&EnumDef>,
    span: Span,
) -> Vec<Diagnostic> {
    // A wildcard or identifier binding covers everything.
    if has_wildcard_or_ident(patterns) {
        return vec![];
    }

    match subject_type_name {
        "bool" => check_bool(patterns, span),
        _ if enum_def.is_some() => check_enum(patterns, enum_def.unwrap(), span),
        // Integer, string, and other infinite types require a wildcard/catch-all.
        _ => check_requires_wildcard(subject_type_name, span),
    }
}

/// Returns true if any pattern is a wildcard `_` or an identifier binding.
fn has_wildcard_or_ident(patterns: &[&Pattern]) -> bool {
    patterns.iter().any(|p| matches!(p, Pattern::Wildcard { .. } | Pattern::Ident { .. }))
}

/// Bool: must cover both `true` and `false`.
fn check_bool(patterns: &[&Pattern], span: Span) -> Vec<Diagnostic> {
    let mut seen_true = false;
    let mut seen_false = false;

    for pat in patterns {
        match pat {
            Pattern::Literal { expr, .. } => {
                if let kryos_ast::Expr::BoolLiteral { value: true, .. } = expr.as_ref() {
                    seen_true = true;
                } else if let kryos_ast::Expr::BoolLiteral { value: false, .. } = expr.as_ref() {
                    seen_false = true;
                }
            }
            // Or-patterns: recurse into each alternative.
            Pattern::Or { patterns: alts, .. } => {
                let alt_refs: Vec<&Pattern> = alts.iter().collect();
                if has_wildcard_or_ident(&alt_refs) {
                    return vec![];
                }
                for alt in alts {
                    if let Pattern::Literal { expr, .. } = alt {
                        if let kryos_ast::Expr::BoolLiteral { value: true, .. } = expr.as_ref() {
                            seen_true = true;
                        } else if let kryos_ast::Expr::BoolLiteral { value: false, .. } = expr.as_ref()
                        {
                            seen_false = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if seen_true && seen_false {
        return vec![];
    }

    let missing = match (seen_true, seen_false) {
        (true, false) => "`false`",
        (false, true) => "`true`",
        _ => "`true` and `false`",
    };

    vec![Diagnostic::warning(
        format!("non-exhaustive match: missing {missing}"),
    ).with_label(span, "this match is not exhaustive")]
}

/// Enum: must cover all variants.
fn check_enum(patterns: &[&Pattern], enum_def: &EnumDef, span: Span) -> Vec<Diagnostic> {
    let all_variants: HashSet<&str> = enum_def
        .variants
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();

    let mut covered: HashSet<&str> = HashSet::new();

    for pat in patterns {
        collect_enum_variants(pat, &mut covered);
    }

    let missing: Vec<&str> = all_variants
        .difference(&covered)
        .copied()
        .collect();

    if missing.is_empty() {
        return vec![];
    }

    let mut missing_sorted: Vec<&str> = missing;
    missing_sorted.sort_unstable();
    let names = missing_sorted
        .iter()
        .map(|v| format!("`{v}`"))
        .collect::<Vec<_>>()
        .join(", ");

    vec![Diagnostic::warning(
        format!("non-exhaustive match: missing variant(s) {names}"),
    ).with_label(span, "this match is not exhaustive")]
}

/// Collect covered enum variant names from a pattern (handles Or-patterns).
fn collect_enum_variants<'a>(pattern: &'a Pattern, covered: &mut HashSet<&'a str>) {
    match pattern {
        Pattern::Enum { variant, .. } => {
            covered.insert(variant.as_str());
        }
        Pattern::Or { patterns, .. } => {
            for p in patterns {
                collect_enum_variants(p, covered);
            }
        }
        _ => {}
    }
}

/// For integer, string, and other infinite types, require a wildcard or
/// identifier catch-all.
fn check_requires_wildcard(type_name: &str, span: Span) -> Vec<Diagnostic> {
    vec![Diagnostic::warning(
        format!(
            "non-exhaustive match on `{type_name}`: add a wildcard `_` or catch-all pattern"
        ),
    ).with_label(span, "this match is not exhaustive")]
}
