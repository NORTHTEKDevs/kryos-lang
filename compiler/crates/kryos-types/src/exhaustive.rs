//! Pattern exhaustiveness checking for match expressions.
//!
//! After type-checking match arms, this module verifies that the patterns
//! cover all possible values of the subject type.  Non-exhaustive matches
//! on finite types (bool, enums) produce errors; infinite types (int, string)
//! produce warnings suggesting a wildcard catch-all.

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
    match subject_type_name {
        "bool" => {
            // A wildcard or plain identifier binding covers both bool values.
            if has_wildcard_or_ident(patterns) {
                return vec![];
            }
            check_bool(patterns, span)
        }
        // Builtin Option/Result have fixed variant sets but no user EnumDef.
        "Option" => check_variant_exhaustive(patterns, &["None", "Some"], span),
        "Result" => check_variant_exhaustive(patterns, &["Ok", "Err"], span),
        _ if enum_def.is_some() => {
            let variants: Vec<&str> = enum_def
                .unwrap()
                .variants
                .iter()
                .map(|(n, _)| n.as_str())
                .collect();
            check_variant_exhaustive(patterns, &variants, span)
        }
        // Integer, string, and other infinite types require a wildcard/catch-all.
        _ => {
            if has_wildcard_or_ident(patterns) {
                return vec![];
            }
            // An irrefutable struct pattern (`Point { x, y }` -- every field
            // bound by wildcard/ident, no literal or nested-refutable field)
            // covers ALL values of a struct type: a struct has a single shape.
            // Without this a valid exhaustive `match p { Point { x, y } => .. }`
            // drew a spurious non-exhaustive warning.
            if patterns.iter().any(|p| is_irrefutable_struct_pattern(p)) {
                return vec![];
            }
            check_requires_wildcard(subject_type_name, span)
        }
    }
}

/// A struct pattern whose every field sub-pattern is itself irrefutable
/// (wildcard/ident, or a nested all-irrefutable struct/tuple). Such a pattern
/// matches every value of the struct type.
fn is_irrefutable_struct_pattern(pat: &Pattern) -> bool {
    matches!(pat, Pattern::Struct { .. }) && is_irrefutable_subpat(pat)
}

fn is_irrefutable_subpat(pat: &Pattern) -> bool {
    match pat {
        Pattern::Wildcard { .. } | Pattern::Ident { .. } => true,
        Pattern::Struct { fields, .. } => fields.iter().all(|(_, s)| is_irrefutable_subpat(s)),
        Pattern::Tuple { elements, .. } => elements.iter().all(is_irrefutable_subpat),
        // Literal / enum-variant / or-patterns can fail to match.
        _ => false,
    }
}

/// Variant-aware exhaustiveness for enum / Option / Result. Kryos lets a nullary
/// variant be matched by its BARE name (`Zero`, `None`), which the parser emits
/// as a `Pattern::Ident` -- so a plain identifier is a catch-all binding ONLY
/// when its name is not a variant. The old generic `has_wildcard_or_ident`
/// early-out treated every bare-variant-ident as a catch-all and silently
/// disabled exhaustiveness for any match touching a nullary variant (a missing
/// variant then compiled clean and hit no arm at runtime).
fn check_variant_exhaustive(
    patterns: &[&Pattern],
    variants: &[&str],
    span: Span,
) -> Vec<Diagnostic> {
    let mut covered: HashSet<String> = HashSet::new();
    let mut has_catchall = false;
    for pat in patterns {
        collect_variant_coverage(pat, variants, &mut covered, &mut has_catchall);
    }
    if has_catchall {
        return vec![];
    }
    let mut missing: Vec<&str> = variants
        .iter()
        .copied()
        .filter(|v| !covered.contains(*v))
        .collect();
    if missing.is_empty() {
        return vec![];
    }
    missing.sort_unstable();
    let names = missing
        .iter()
        .map(|v| format!("`{v}`"))
        .collect::<Vec<_>>()
        .join(", ");
    vec![
        Diagnostic::error(format!("non-exhaustive match: missing variant(s) {names}")).with_code(kryos_errors::codes::E0112)
            .with_label(span, "add the missing variant(s) or a wildcard `_`"),
    ]
}

/// Collect variant coverage, distinguishing a bare-variant identifier (which
/// covers that variant) from a genuine binding or wildcard (a catch-all).
fn collect_variant_coverage(
    pat: &Pattern,
    variants: &[&str],
    covered: &mut HashSet<String>,
    has_catchall: &mut bool,
) {
    match pat {
        Pattern::Wildcard { .. } => *has_catchall = true,
        Pattern::Ident { name, .. } => {
            if variants.contains(&name.as_str()) {
                covered.insert(name.clone());
            } else {
                *has_catchall = true;
            }
        }
        Pattern::Enum { variant, .. } => {
            covered.insert(variant.clone());
        }
        Pattern::Or { patterns, .. } => {
            for p in patterns {
                collect_variant_coverage(p, variants, covered, has_catchall);
            }
        }
        _ => {}
    }
}

/// Returns true if any pattern is a wildcard `_` or an identifier binding.
fn has_wildcard_or_ident(patterns: &[&Pattern]) -> bool {
    patterns
        .iter()
        .any(|p| matches!(p, Pattern::Wildcard { .. } | Pattern::Ident { .. }))
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
                        } else if let kryos_ast::Expr::BoolLiteral { value: false, .. } =
                            expr.as_ref()
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

    vec![
        Diagnostic::error(format!("non-exhaustive match: missing {missing}")).with_code(kryos_errors::codes::E0112)
            .with_label(span, "add the missing case(s) or a wildcard `_`"),
    ]
}


/// For integer, string, and other infinite types, require a wildcard or
/// identifier catch-all.
fn check_requires_wildcard(type_name: &str, span: Span) -> Vec<Diagnostic> {
    vec![Diagnostic::warning(format!(
        "non-exhaustive match on `{type_name}`: add a wildcard `_` or catch-all pattern"
    ))
    .with_label(span, "this match is not exhaustive")]
}
