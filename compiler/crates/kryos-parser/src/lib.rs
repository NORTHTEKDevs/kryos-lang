//! Kryos parser — recursive descent + Pratt expression parsing.
//!
//! Transforms a token stream from `kryos-lexer` into an AST defined in `kryos-ast`.

pub mod parser;

use kryos_ast::Module;
use kryos_errors::Diagnostic;
use kryos_lexer::Token;

/// Parse a token stream into a `Module` AST.
///
/// Returns `Ok(Module)` on success. On failure, returns `Err(diagnostics)`.
/// The parser performs error recovery so partial results may still be available
/// in the `Ok` path even when diagnostics were emitted (soft errors).
pub fn parse(tokens: Vec<Token>) -> Result<Module, Vec<Diagnostic>> {
    let mut p = parser::Parser::new(tokens);
    let module = p.parse_module();
    let diagnostics = p.into_diagnostics();
    if diagnostics.iter().any(|d| d.is_error()) {
        Err(diagnostics)
    } else {
        Ok(module)
    }
}

/// Like `parse`, but always returns every diagnostic the parser collected --
/// errors AND warnings -- instead of silently discarding non-error
/// diagnostics on success. `parse`'s `Ok` branch drops them, which is the
/// right default for a caller that only cares about success/failure, but
/// wrong for any path that should surface a parser-level WARNING (e.g. the
/// ambiguous newline-led `||`/`|` continuation, W0001) to the user. Prefer
/// this in `kryos run`/`kryos build`/`kryos check`'s pipeline.
pub fn parse_with_diagnostics(tokens: Vec<Token>) -> (Option<Module>, Vec<Diagnostic>) {
    let mut p = parser::Parser::new(tokens);
    let module = p.parse_module();
    let diagnostics = p.into_diagnostics();
    let has_error = diagnostics.iter().any(|d| d.is_error());
    (if has_error { None } else { Some(module) }, diagnostics)
}
