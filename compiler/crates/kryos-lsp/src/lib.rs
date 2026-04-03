//! Kryos Language Server Protocol implementation.
//!
//! Provides IDE features: diagnostics, completion, hover, go-to-definition.
//! Communicates via JSON-RPC over stdin/stdout.

pub mod protocol;
pub mod server;
pub mod diagnostics;
pub mod completion;
pub mod hover;
pub mod goto_def;

pub use server::LspServer;
