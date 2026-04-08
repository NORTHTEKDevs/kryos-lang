//! Kryos code formatter — takes an AST and produces canonically formatted source code.
//!
//! The formatter uses 4-space indentation, 80-column line width for signature wrapping,
//! and blank lines between top-level declarations.

#![allow(clippy::too_many_arguments)]

pub mod formatter;

use kryos_ast::Module;
use kryos_errors::Diagnostic;
use kryos_lexer::Lexer;
use kryos_parser::parse;

pub use formatter::Formatter;

/// Convenience function: parse source code, then format the resulting AST.
///
/// Returns the formatted source string on success, or a list of parse diagnostics on failure.
pub fn format_source(source: &str) -> Result<String, Vec<Diagnostic>> {
    let tokens = Lexer::new(source, 0).tokenize();
    let module = parse(tokens)?;
    let fmt = Formatter::new();
    Ok(fmt.format_module(&module))
}

/// Format an already-parsed Module AST into canonical source code.
pub fn format_module(module: &Module) -> String {
    let fmt = Formatter::new();
    fmt.format_module(module)
}
