//! Kryos ownership analysis and ARC insertion pass.
//!
//! Implements the Kryos memory model: ownership + ARC (like Swift/Mojo).
//! - Every value has one owner. Assignment/passing moves ownership.
//! - `shared` keyword wraps a value in ARC (atomic reference counting).
//! - Closures capturing mutable variables auto-ARC wrap them.
//! - Values sent across channels are moved.
//! - Copy types (primitives, @copy structs) bypass ownership.
//! - `weak` creates a non-owning reference.

pub mod analysis;
pub mod arc;

use kryos_errors::{Diagnostic, Span};

/// The ownership state of a variable at a given point in the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnershipState {
    /// Normal ownership — the variable holds the value.
    Owned,
    /// Value has been moved away (assignment, function call, channel send).
    Moved,
    /// ARC-wrapped (explicit `shared` keyword or auto-detected).
    Shared,
    /// Some fields have been moved out of a struct.
    PartiallyMoved,
    /// Declared but not yet assigned a value.
    Uninitialized,
}

/// Reason an ARC insertion is needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArcReason {
    /// Explicit `shared` keyword in source.
    ExplicitShared,
    /// Closure captures a mutable outer variable.
    ClosureCaptureMut,
    /// Value shared across actor boundaries.
    ActorBoundary,
}

/// A point where ARC wrapping must be inserted.
#[derive(Debug, Clone)]
pub struct ArcInsertion {
    pub variable: String,
    pub scope_span: Span,
    pub reason: ArcReason,
}

/// Result of the ownership analysis pass.
#[derive(Debug)]
pub struct OwnershipResult {
    pub errors: Vec<Diagnostic>,
    pub arc_insertions: Vec<ArcInsertion>,
}

/// Run ownership analysis on a module.
///
/// Walks the entire AST, tracks ownership state for every variable in every
/// scope, detects use-after-move errors, and determines ARC insertion points.
pub fn analyze_ownership(module: &kryos_ast::Module) -> OwnershipResult {
    let mut analyzer = analysis::OwnershipAnalyzer::new();
    analyzer.analyze_module(module);
    OwnershipResult {
        errors: analyzer.diagnostics,
        arc_insertions: analyzer.arc_insertions,
    }
}
