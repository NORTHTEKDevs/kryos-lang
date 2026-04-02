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
