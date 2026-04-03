//! ARC insertion logic — determines which values need ARC wrapping.
//!
//! After ownership analysis, this module provides utilities to query
//! and filter ARC insertion points by reason, scope, or variable name.

use kryos_errors::Span;

use crate::{ArcInsertion, ArcReason};

/// Filter ARC insertions by reason.
pub fn filter_by_reason<'a>(insertions: &'a [ArcInsertion], reason: &ArcReason) -> Vec<&'a ArcInsertion> {
    insertions.iter().filter(|i| &i.reason == reason).collect()
}

/// Get all explicit `shared` insertions.
pub fn explicit_shared(insertions: &[ArcInsertion]) -> Vec<&ArcInsertion> {
    filter_by_reason(insertions, &ArcReason::ExplicitShared)
}

/// Get all closure-capture-mut insertions.
pub fn closure_captures(insertions: &[ArcInsertion]) -> Vec<&ArcInsertion> {
    filter_by_reason(insertions, &ArcReason::ClosureCaptureMut)
}

/// Get all actor-boundary insertions.
pub fn actor_boundary(insertions: &[ArcInsertion]) -> Vec<&ArcInsertion> {
    filter_by_reason(insertions, &ArcReason::ActorBoundary)
}

/// Get all ARC insertions for a specific variable.
pub fn for_variable<'a>(insertions: &'a [ArcInsertion], name: &str) -> Vec<&'a ArcInsertion> {
    insertions.iter().filter(|i| i.variable == name).collect()
}

/// Get all ARC insertions within a specific span.
pub fn within_span(insertions: &[ArcInsertion], span: Span) -> Vec<&ArcInsertion> {
    insertions
        .iter()
        .filter(|i| {
            i.scope_span.file_id == span.file_id
                && i.scope_span.start >= span.start
                && i.scope_span.end <= span.end
        })
        .collect()
}

/// Summary of ARC insertion statistics.
#[derive(Debug, Clone, Default)]
pub struct ArcSummary {
    pub total: usize,
    pub explicit_shared: usize,
    pub closure_capture_mut: usize,
    pub actor_boundary: usize,
}

/// Compute summary statistics for ARC insertions.
pub fn summarize(insertions: &[ArcInsertion]) -> ArcSummary {
    let mut summary = ArcSummary {
        total: insertions.len(),
        ..Default::default()
    };
    for ins in insertions {
        match ins.reason {
            ArcReason::ExplicitShared => summary.explicit_shared += 1,
            ArcReason::ClosureCaptureMut => summary.closure_capture_mut += 1,
            ArcReason::ActorBoundary => summary.actor_boundary += 1,
        }
    }
    summary
}
