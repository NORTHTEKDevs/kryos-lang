//! Kryos Language Server Protocol implementation.
//!
//! Provides IDE features: diagnostics, completion, hover, go-to-definition,
//! document symbols, references, rename, signature help, inlay hints,
//! document highlight, folding ranges, and formatting.
//! Communicates via JSON-RPC over stdin/stdout.

pub mod cap_surface;
pub mod code_actions;
pub mod completion;
pub mod diagnostics;
pub mod document_symbols;
pub mod folding;
pub mod formatting;
pub mod goto_def;
pub mod highlight;
pub mod hover;
pub mod inlay_hints;
pub mod protocol;
pub mod references;
pub mod semantic_tokens;
pub mod server;
pub mod signature_help;
pub mod workspace_symbols;

pub use server::LspServer;
