//! Kryos Language Server Protocol implementation.
//!
//! Provides IDE features: diagnostics, completion, hover, go-to-definition.
//! Communicates via JSON-RPC over stdin/stdout.

pub mod completion;
pub mod diagnostics;
pub mod goto_def;
pub mod hover;
pub mod protocol;
pub mod server;

pub use server::LspServer;
