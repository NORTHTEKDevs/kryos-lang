//! Kryos type system — Hindley-Milner inference, type checking, and generics.
//!
//! This crate provides:
//! - `ty::Type` — internal type representation (distinct from AST `TypeExpr`)
//! - `env::TypeEnv` — scope-based symbol table for type lookups
//! - `infer::InferenceEngine` — unification-based type inference
//! - `check::TypeChecker` — AST type-checking pass

pub mod check;
pub mod env;
pub mod exhaustive;
pub mod infer;
pub mod suggest;
pub mod ty;

pub use check::{type_check, type_check_with_lambda_params, TypeChecker};
pub use env::TypeEnv;
pub use infer::InferenceEngine;
pub use ty::Type;
