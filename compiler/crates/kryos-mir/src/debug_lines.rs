//! Debug-line instrumentation control for the source-level debugger.
//!
//! When the `kryos dap` debugger drives a compilation, it installs a
//! [`DebugLineResolver`] for the duration of MIR lowering. While installed,
//! [`lower::lower_stmt`](crate::lower) emits an [`Instruction::DebugLine`]
//! (carrying the statement's 1-based source line) before each statement it
//! lowers. The Cranelift backend turns those markers into `kryos_dbg_line`
//! runtime-hook calls; every other build path leaves the resolver unset, so no
//! markers are emitted and normal `kryos run` / `kryos build` are unaffected.
//!
//! The resolver is held in a thread-local because lowering runs synchronously
//! on the same thread that the driver invokes it from — the same thread the
//! DAP command uses to compile. This keeps the instrumentation completely out
//! of the shared `BuildConfig` and off every other call site.

use std::cell::{Cell, RefCell};

use kryos_errors::Span;

/// Maps a statement [`Span`] to its 1-based source line, using a snapshot of
/// the per-file line-start tables taken from the driver's `SourceMap`.
pub struct DebugLineResolver {
    /// `line_starts[file_id]` is the byte offset of the start of each line.
    line_starts: Vec<Vec<u32>>,
}

impl DebugLineResolver {
    /// Build a resolver from a `SourceMap::line_starts_snapshot()`.
    pub fn new(line_starts: Vec<Vec<u32>>) -> Self {
        Self { line_starts }
    }

    /// Resolve a span to its 1-based source line, or `None` for a dummy /
    /// out-of-range span (synthetic statements produced by desugaring).
    pub fn line_of(&self, span: Span) -> Option<u32> {
        if span == Span::DUMMY {
            return None;
        }
        let starts = self.line_starts.get(span.file_id as usize)?;
        if starts.is_empty() {
            return None;
        }
        // Mirror `SourceMap::offset_to_line_col`: the line index is the number
        // of line-starts at or before the offset (1-based).
        let line = starts.partition_point(|&s| s <= span.start);
        if line == 0 {
            None
        } else {
            Some(line as u32)
        }
    }
}

thread_local! {
    static DEBUG_LINE_RESOLVER: RefCell<Option<DebugLineResolver>> = const { RefCell::new(None) };
    static DEBUG_INSTRUMENT_REQUESTED: Cell<bool> = const { Cell::new(false) };
}

/// Request (or cancel) debug-line instrumentation for compilations on this
/// thread. The DAP command sets this before invoking the driver; the driver
/// reads it after building the `SourceMap` and installs a resolver for the
/// lowering pass. Kept separate from the resolver itself because only the
/// driver has the `SourceMap` needed to construct one.
pub fn request_debug_instrument(on: bool) {
    DEBUG_INSTRUMENT_REQUESTED.with(|c| c.set(on));
}

/// True if the DAP command has requested debug-line instrumentation.
pub fn debug_instrument_requested() -> bool {
    DEBUG_INSTRUMENT_REQUESTED.with(|c| c.get())
}

/// Install (or clear, with `None`) the resolver for the current thread. The
/// driver sets this around its `lower_module` call when compiling under the
/// debugger and clears it immediately afterwards.
pub fn set_debug_line_resolver(resolver: Option<DebugLineResolver>) {
    DEBUG_LINE_RESOLVER.with(|cell| *cell.borrow_mut() = resolver);
}

/// True if debug-line instrumentation is currently active on this thread.
pub fn debug_lines_active() -> bool {
    DEBUG_LINE_RESOLVER.with(|cell| cell.borrow().is_some())
}

/// Resolve a span to a source line using the installed resolver, if any.
/// Returns `None` when no resolver is installed (the common case).
pub fn resolve_debug_line(span: Span) -> Option<u32> {
    DEBUG_LINE_RESOLVER.with(|cell| cell.borrow().as_ref().and_then(|r| r.line_of(span)))
}
