//! Kryos compile-time capability checking.
//!
//! This crate enforces the Kryos capability system — a static analysis pass
//! that restricts what functions, actors, and scopes are permitted to do.
//!
//! # Key rules
//!
//! 1. **Explicit capabilities**: Functions using stdlib modules must declare
//!    matching capabilities via `@capabilities(net, io, ...)`.
//!
//! 2. **Attenuation**: Child scopes cannot exceed the capabilities of their
//!    parent scope. A function called from a `@capabilities(net)` scope cannot
//!    use `io` unless the caller also has `io`.
//!
//! 3. **Immutability**: Capabilities are fixed at compile time. No runtime
//!    escalation is permitted — self-heal actions like `add_capability`,
//!    `widen_sandbox`, etc. are prohibited.
//!
//! 4. **Budget and sandbox**: `@budget(N)` and `@sandbox` annotations provide
//!    additional resource and execution restrictions.
//!
//! # Usage
//!
//! ```ignore
//! use kryos_capabilities::check_capabilities;
//!
//! let diagnostics = check_capabilities(&module);
//! for diag in &diagnostics {
//!     if diag.is_error() {
//!         // Capability violation found
//!     }
//! }
//! ```

pub mod model;
pub mod checker;

pub use model::{Capability, CapabilitySet, Budget, Sandbox};
pub use checker::check_capabilities;
