//! Kryos diagnostic engine — errors, warnings, source spans.

pub mod codes;
pub mod explain;

/// Panic payload used by a compiler pass to abort on a bounded resource
/// limit (e.g. generic monomorphization depth/count/type-size -- see
/// `kryos-types`'s `InferenceEngine::resolve` and `kryos-mir`'s
/// `monomorphize`/`mono_mangled_name`, LEDGER items 19/23) instead of
/// letting the process exhaust memory or hang unresponsive with no
/// diagnostic. A caller that can safely recover (return an ordinary
/// `Diagnostic` instead of propagating the panic) should use
/// `ResourceLimitExceeded::catch`, which converts exactly this payload type
/// into a normal `Err` and re-raises any OTHER panic unchanged.
#[derive(Debug, Clone)]
pub struct ResourceLimitExceeded {
    pub message: String,
}

thread_local! {
    /// When true, this thread's installed panic hook swallows the default
    /// "thread '...' panicked at ..." trace for a panic in flight. Set only
    /// for the duration of `ResourceLimitExceeded::catch`'s inner call, and
    /// only ever consulted on the panicking thread itself (see `catch`'s
    /// doc comment for why this can't affect any other thread).
    static SUPPRESS_PANIC_HOOK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

static INSTALL_HOOK: std::sync::Once = std::sync::Once::new();

impl ResourceLimitExceeded {
    /// Panic with this payload, aborting the enclosing compiler pass.
    pub fn abort(message: String) -> ! {
        std::panic::panic_any(ResourceLimitExceeded { message })
    }

    /// Run `f`, catching a panic raised via `abort` and returning it as an
    /// `Err` instead of letting it propagate. The default panic hook's raw
    /// "thread '...' panicked at ..." trace is suppressed for exactly this
    /// intentional, bounded abort, so a caller sees only the clean
    /// diagnostic it builds from the returned message -- not a trace that
    /// reads like an internal-compiler-error crash. Any OTHER panic is
    /// re-raised via `resume_unwind` with its normal hook output intact;
    /// only a panic actually raised by `abort` can ever be suppressed.
    ///
    /// Suppression is THREAD-LOCAL, not a global hook swap: the shared hook
    /// installed once via `INSTALL_HOOK` consults a per-thread flag, so a
    /// genuine, unrelated panic firing concurrently on another thread while
    /// this call is in flight still prints normally. Reentrant (nested)
    /// calls on the same thread save/restore the flag correctly.
    pub fn catch<F, T>(f: F) -> Result<T, ResourceLimitExceeded>
    where
        F: FnOnce() -> T + std::panic::UnwindSafe,
    {
        INSTALL_HOOK.call_once(|| {
            let default_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                let suppress = SUPPRESS_PANIC_HOOK.with(|c| c.get());
                if !suppress {
                    default_hook(info);
                }
            }));
        });
        let prev = SUPPRESS_PANIC_HOOK.with(|c| c.replace(true));
        let result = std::panic::catch_unwind(f);
        SUPPRESS_PANIC_HOOK.with(|c| c.set(prev));
        match result {
            Ok(v) => Ok(v),
            Err(payload) => match payload.downcast::<ResourceLimitExceeded>() {
                Ok(limit) => Err(*limit),
                Err(other) => std::panic::resume_unwind(other),
            },
        }
    }
}

/// Source location span: file_id, start byte offset, end byte offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub file_id: u32,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const DUMMY: Span = Span {
        file_id: 0,
        start: 0,
        end: 0,
    };

    pub fn new(file_id: u32, start: u32, end: u32) -> Self {
        Self {
            file_id,
            start,
            end,
        }
    }

    pub fn merge(self, other: Span) -> Span {
        debug_assert_eq!(self.file_id, other.file_id);
        Span {
            file_id: self.file_id,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// Severity level for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
    Info,
    Help,
}

/// A labeled span within a diagnostic.
#[derive(Debug, Clone)]
pub struct Label {
    pub span: Span,
    pub message: String,
    pub is_primary: bool,
}

/// A single diagnostic (error, warning, etc.).
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub level: Level,
    pub message: String,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
    pub code: Option<String>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            level: Level::Error,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
            code: None,
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            level: Level::Warning,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
            code: None,
        }
    }

    pub fn with_label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            message: message.into(),
            is_primary: self.labels.is_empty(),
        });
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn is_error(&self) -> bool {
        self.level == Level::Error
    }
}

/// Source file registry — maps file IDs to names and contents.
#[derive(Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

#[derive(Debug)]
pub struct SourceFile {
    pub name: String,
    pub source: String,
    line_starts: Vec<u32>,
}

impl SourceMap {
    pub fn add_file(&mut self, name: String, source: String) -> u32 {
        let id = self.files.len() as u32;
        let line_starts = std::iter::once(0)
            .chain(source.match_indices('\n').map(|(i, _)| (i + 1) as u32))
            .collect();
        self.files.push(SourceFile {
            name,
            source,
            line_starts,
        });
        id
    }

    pub fn get_file(&self, id: u32) -> Option<&SourceFile> {
        self.files.get(id as usize)
    }

    /// Convert a byte offset to (line, column), both 1-based.
    pub fn offset_to_line_col(&self, file_id: u32, offset: u32) -> (u32, u32) {
        let file = &self.files[file_id as usize];
        let line = file.line_starts.partition_point(|&s| s <= offset);
        let line_start = file.line_starts[line - 1];
        (line as u32, offset - line_start + 1)
    }

    /// Snapshot the per-file line-start offset tables, indexed by `file_id`.
    ///
    /// Used by debug-line instrumentation (the DAP debugger) to map a
    /// statement's byte-offset span to a 1-based source line at MIR lowering
    /// time without holding a borrow on the `SourceMap` itself.
    pub fn line_starts_snapshot(&self) -> Vec<Vec<u32>> {
        self.files.iter().map(|f| f.line_starts.clone()).collect()
    }
}

/// ANSI escape codes for colored terminal output.
mod ansi {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const BOLD_RED: &str = "\x1b[1;31m";
    pub const BOLD_YELLOW: &str = "\x1b[1;33m";
    pub const BOLD_CYAN: &str = "\x1b[1;36m";
    pub const BOLD_BLUE: &str = "\x1b[1;34m";
}

/// Renders a diagnostic to a string with ANSI colored output.
///
/// Colors:
/// - Red for errors
/// - Yellow for warnings
/// - Cyan for notes/suggestions
/// - Blue for line numbers and arrows
/// - Bold for messages
pub fn render_diagnostic(diag: &Diagnostic, source_map: &SourceMap) -> String {
    render_diagnostic_impl(diag, source_map, true)
}

/// Renders a diagnostic to a plain string without ANSI colors.
pub fn render_diagnostic_plain(diag: &Diagnostic, source_map: &SourceMap) -> String {
    render_diagnostic_impl(diag, source_map, false)
}

fn render_diagnostic_impl(diag: &Diagnostic, source_map: &SourceMap, color: bool) -> String {
    let (level_str, level_color) = match diag.level {
        Level::Error => ("error", ansi::BOLD_RED),
        Level::Warning => ("warning", ansi::BOLD_YELLOW),
        Level::Info => ("info", ansi::BOLD_CYAN),
        Level::Help => ("help", ansi::BOLD_CYAN),
    };

    let mut out = String::new();

    // Header line: "error[E001]: message" with colors
    if color {
        if let Some(ref code) = diag.code {
            out.push_str(&format!(
                "{level_color}{level_str}[{code}]{reset}: {bold}{msg}{reset}\n",
                reset = ansi::RESET,
                bold = ansi::BOLD,
                msg = diag.message,
            ));
        } else {
            out.push_str(&format!(
                "{level_color}{level_str}{reset}: {bold}{msg}{reset}\n",
                reset = ansi::RESET,
                bold = ansi::BOLD,
                msg = diag.message,
            ));
        }
    } else if let Some(ref code) = diag.code {
        out.push_str(&format!("{level_str}[{code}]: {}\n", diag.message));
    } else {
        out.push_str(&format!("{level_str}: {}\n", diag.message));
    }

    for label in &diag.labels {
        if let Some(file) = source_map.get_file(label.span.file_id) {
            let (line, col) = source_map.offset_to_line_col(label.span.file_id, label.span.start);
            let arrow = if label.is_primary { "-->" } else { "   " };

            if color {
                out.push_str(&format!(
                    " {blue}{arrow}{reset} {}:{line}:{col}\n",
                    file.name,
                    blue = ansi::BOLD_BLUE,
                    reset = ansi::RESET,
                ));
            } else {
                out.push_str(&format!(" {arrow} {}:{line}:{col}\n", file.name));
            }

            let line_idx = (line - 1) as usize;
            if line_idx < file.line_starts.len() {
                let start = file.line_starts[line_idx] as usize;
                let end = file
                    .line_starts
                    .get(line_idx + 1)
                    .map(|&s| s as usize)
                    .unwrap_or(file.source.len());
                let src_line = &file.source[start..end].trim_end();

                if color {
                    out.push_str(&format!(
                        "  {blue}{line}{reset} | {src_line}\n",
                        blue = ansi::BOLD_BLUE,
                        reset = ansi::RESET,
                    ));
                } else {
                    out.push_str(&format!("  {line} | {src_line}\n"));
                }

                let col_start = (col - 1) as usize;
                let span_len = (label.span.end - label.span.start) as usize;
                let padding = " ".repeat(col_start);
                // Clamp the caret run to the visible portion of THIS line. A
                // MULTI-LINE span (e.g. a whole `match { .. }` expression) has
                // a byte length far larger than the one displayed line, so an
                // unclamped `^`.repeat(span_len) drew carets running off the
                // end of the shown line -- breaking any editor/LSP that maps
                // the caret range back to columns. Single-line spans are
                // unaffected (their length already fits the line).
                let max_underline = src_line.len().saturating_sub(col_start).max(1);
                let underline = "^".repeat(span_len.min(max_underline).max(1));
                let line_num_width = format!("{line}").len();
                let gutter = " ".repeat(line_num_width + 2);

                if color {
                    out.push_str(&format!(
                        "{gutter}| {padding}{color}{underline} {msg}{reset}\n",
                        color = level_color,
                        msg = label.message,
                        reset = ansi::RESET,
                    ));
                } else {
                    out.push_str(&format!(
                        "{gutter}| {padding}{underline} {}\n",
                        label.message
                    ));
                }
            }
        }
    }

    for note in &diag.notes {
        if color {
            out.push_str(&format!(
                "  = {cyan}note{reset}: {note}\n",
                cyan = ansi::BOLD_CYAN,
                reset = ansi::RESET,
            ));
        } else {
            out.push_str(&format!("  = note: {note}\n"));
        }
    }

    out
}

/// Collects diagnostics during compilation.
#[derive(Debug, Default)]
pub struct DiagnosticBag {
    diagnostics: Vec<Diagnostic>,
}

impl DiagnosticBag {
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    pub fn emit(&mut self, diag: Diagnostic) {
        self.diagnostics.push(diag);
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.is_error())
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.is_error()).count()
    }
}
