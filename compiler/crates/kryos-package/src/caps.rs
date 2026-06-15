//! Capability badge for registry entries (project 05 — registry capability badging).
//!
//! A `CapsBadge` is a machine-readable summary of the capabilities a package's
//! source declares via `@capabilities(...)` annotations. It is computed from the
//! source at publish time (see `kryos manifest`/`extract_package_caps` in the CLI),
//! embedded in the registry index entry, and surfaced by `kryos pkg show` /
//! `kryos pkg audit` so a user can see what a package can do *before* installing it.
//!
//! Honesty: the badge reflects ANNOTATED functions only. Kryos capabilities are
//! opt-in per function today (no deny-by-default), so `annotation_coverage_pct`
//! states how much of the package is statically proven. A package with zero
//! annotations emits an empty `capabilities` list and `annotation_coverage_pct: 0`
//! — that is "unknown", not "no capabilities". Display code must say so.

use serde::{Deserialize, Serialize};

/// Machine-readable capability summary for one package version.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapsBadge {
    /// Badge format version, e.g. "kryos-caps/1".
    pub schema: String,
    /// Sorted, de-duplicated union of all annotated-function capability sets.
    pub capabilities: Vec<String>,
    /// Subset of `capabilities` considered high-risk (ffi, process).
    pub dangerous: Vec<String>,
    /// Percentage of functions carrying a `@capabilities` annotation (0..=100).
    pub annotation_coverage_pct: u8,
    /// Capabilities inferred from builtins in UNannotated functions (best effort).
    #[serde(default)]
    pub inferred_uncovered: Vec<String>,
}

impl CapsBadge {
    /// Current badge schema identifier.
    pub const SCHEMA: &'static str = "kryos-caps/1";

    /// Capabilities flagged as high-risk in the badge display.
    pub fn dangerous_caps() -> &'static [&'static str] {
        &["ffi", "process"]
    }

    /// Build a badge from already-extracted capability strings.
    ///
    /// `capabilities` is the union of annotated-function capability sets (any
    /// order; it is sorted + de-duplicated here). `annotated_fns`/`total_fns`
    /// drive the coverage percentage. `inferred_uncovered` is the best-effort
    /// set of capabilities seen in unannotated functions.
    pub fn from_caps(
        capabilities: Vec<String>,
        annotated_fns: usize,
        total_fns: usize,
        mut inferred_uncovered: Vec<String>,
    ) -> Self {
        let mut capabilities = capabilities;
        capabilities.sort();
        capabilities.dedup();

        let dangerous: Vec<String> = capabilities
            .iter()
            .filter(|c| Self::dangerous_caps().contains(&c.as_str()))
            .cloned()
            .collect();

        // Integer percentage, clamped to 100. total_fns == 0 -> 0 (nothing proven).
        let pct: u8 = if total_fns == 0 {
            0
        } else {
            let raw = (annotated_fns.saturating_mul(100)) / total_fns;
            raw.min(100) as u8
        };

        inferred_uncovered.sort();
        inferred_uncovered.dedup();

        CapsBadge {
            schema: Self::SCHEMA.to_string(),
            capabilities,
            dangerous,
            annotation_coverage_pct: pct,
            inferred_uncovered,
        }
    }

    /// Serialize to a compact JSON object string.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Parse a badge from a JSON object string.
    pub fn from_json(s: &str) -> Option<Self> {
        serde_json::from_str(s).ok()
    }

    /// True if this badge declares any high-risk capability.
    pub fn is_dangerous(&self) -> bool {
        !self.dangerous.is_empty()
    }

    /// Dangerous capabilities present here but NOT in `prev` (audit escalation).
    /// A non-empty result means `kryos pkg audit` should fail.
    pub fn dangerous_escalations(&self, prev: &CapsBadge) -> Vec<String> {
        self.dangerous
            .iter()
            .filter(|c| !prev.dangerous.contains(c))
            .cloned()
            .collect()
    }

    /// Any capabilities (not just dangerous) present here but NOT in `prev`.
    /// Used by `kryos pkg audit --strict`.
    pub fn new_capabilities(&self, prev: &CapsBadge) -> Vec<String> {
        self.capabilities
            .iter()
            .filter(|c| !prev.capabilities.contains(c))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_caps_sorts_dedups_and_flags_dangerous() {
        let b = CapsBadge::from_caps(
            vec!["net".into(), "ffi".into(), "net".into()],
            3,
            4,
            vec![],
        );
        assert_eq!(b.capabilities, vec!["ffi", "net"]); // sorted + deduped
        assert_eq!(b.dangerous, vec!["ffi"]);
        assert_eq!(b.annotation_coverage_pct, 75);
        assert!(b.is_dangerous());
        assert_eq!(b.schema, "kryos-caps/1");
    }

    #[test]
    fn coverage_zero_when_no_functions() {
        let b = CapsBadge::from_caps(vec![], 0, 0, vec![]);
        assert_eq!(b.annotation_coverage_pct, 0);
        assert!(b.capabilities.is_empty());
        assert!(!b.is_dangerous());
    }

    #[test]
    fn coverage_caps_at_100() {
        let b = CapsBadge::from_caps(vec!["io".into()], 10, 5, vec![]);
        assert_eq!(b.annotation_coverage_pct, 100);
    }

    #[test]
    fn json_round_trips() {
        let b = CapsBadge::from_caps(vec!["net".into(), "crypto".into()], 4, 5, vec!["io".into()]);
        let json = b.to_json();
        let back = CapsBadge::from_json(&json).expect("must parse");
        assert_eq!(b, back);
    }

    #[test]
    fn audit_detects_dangerous_escalation() {
        let prev = CapsBadge::from_caps(vec!["net".into()], 5, 5, vec![]);
        let next = CapsBadge::from_caps(vec!["net".into(), "ffi".into()], 5, 5, vec![]);
        let esc = next.dangerous_escalations(&prev);
        assert_eq!(esc, vec!["ffi"]);
        // Reverse direction: dropping ffi is not an escalation.
        assert!(prev.dangerous_escalations(&next).is_empty());
    }

    #[test]
    fn audit_strict_detects_any_new_capability() {
        let prev = CapsBadge::from_caps(vec!["net".into()], 5, 5, vec![]);
        let next = CapsBadge::from_caps(vec!["net".into(), "crypto".into()], 5, 5, vec![]);
        assert_eq!(next.new_capabilities(&prev), vec!["crypto"]);
        assert!(next.dangerous_escalations(&prev).is_empty()); // crypto is not dangerous
    }
}
