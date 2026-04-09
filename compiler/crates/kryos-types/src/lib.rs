//! Kryos type system — Hindley-Milner inference, type checking, and generics.
//!
//! This crate provides:
//! - `ty::Type` — internal type representation (distinct from AST `TypeExpr`)
//! - `env::TypeEnv` — scope-based symbol table for type lookups
//! - `infer::InferenceEngine` — unification-based type inference
//! - `check::TypeChecker` — AST type-checking pass

pub mod ty;
pub mod env;
pub mod infer;
pub mod check;
pub mod exhaustive;

pub use ty::Type;
pub use env::TypeEnv;
pub use infer::InferenceEngine;
pub use check::{type_check, TypeChecker};
