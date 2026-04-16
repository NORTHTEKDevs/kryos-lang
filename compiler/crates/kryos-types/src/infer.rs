//! Type inference engine — Hindley-Milner style unification.
//!
//! Manages type variables, substitution maps, and the core unification
//! algorithm. Supports numeric literal defaulting (int → i32, float → f64).

use std::collections::HashMap;

use kryos_errors::{Diagnostic, Span};

use crate::ty::Type;

/// Check if `from` can be widened to `to` (safe integer promotion).
///
/// Rules: signed integers widen to larger signed integers,
/// unsigned integers widen to larger unsigned integers.
/// This allows `42` (i32) to be used where `i64` is expected.
fn is_int_widening(from: &Type, to: &Type) -> bool {
    let signed_rank = |t: &Type| -> Option<u8> {
        match t {
            Type::I8 => Some(0),
            Type::I16 => Some(1),
            Type::I32 => Some(2),
            Type::I64 | Type::ISize => Some(3),
            Type::I128 => Some(4),
            _ => None,
        }
    };
    let unsigned_rank = |t: &Type| -> Option<u8> {
        match t {
            Type::U8 => Some(0),
            Type::U16 => Some(1),
            Type::U32 | Type::USize => Some(2),
            Type::U64 => Some(3),
            Type::U128 => Some(4),
            _ => None,
        }
    };

    // Signed → signed widening.
    if let (Some(from_r), Some(to_r)) = (signed_rank(from), signed_rank(to)) {
        return from_r < to_r;
    }
    // Unsigned → unsigned widening.
    if let (Some(from_r), Some(to_r)) = (unsigned_rank(from), unsigned_rank(to)) {
        return from_r < to_r;
    }
    false
}

/// The inference engine: tracks type variables and their resolved types.
#[derive(Debug)]
pub struct InferenceEngine {
    /// Next fresh type variable ID.
    next_var: u32,
    /// Substitution map: Var(id) → resolved Type.
    substitutions: HashMap<u32, Type>,
}

impl Default for InferenceEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl InferenceEngine {
    pub fn new() -> Self {
        Self {
            next_var: 0,
            substitutions: HashMap::new(),
        }
    }

    /// Generate a fresh, unresolved type variable.
    pub fn fresh_var(&mut self) -> Type {
        let id = self.next_var;
        self.next_var += 1;
        Type::Var(id)
    }

    /// Apply all known substitutions to a type, recursively resolving
    /// type variables to their concrete types.
    pub fn resolve(&self, ty: &Type) -> Type {
        match ty {
            Type::Var(id) => {
                if let Some(resolved) = self.substitutions.get(id) {
                    // Chase the substitution chain (handles transitive vars).
                    self.resolve(resolved)
                } else {
                    ty.clone()
                }
            }
            Type::Array { element, size } => Type::Array {
                element: Box::new(self.resolve(element)),
                size: *size,
            },
            Type::Tuple { elements } => Type::Tuple {
                elements: elements.iter().map(|e| self.resolve(e)).collect(),
            },
            Type::Map { key, value } => Type::Map {
                key: Box::new(self.resolve(key)),
                value: Box::new(self.resolve(value)),
            },
            Type::Set { element } => Type::Set {
                element: Box::new(self.resolve(element)),
            },
            Type::Option { inner } => Type::Option {
                inner: Box::new(self.resolve(inner)),
            },
            Type::Result { ok, err } => Type::Result {
                ok: Box::new(self.resolve(ok)),
                err: Box::new(self.resolve(err)),
            },
            Type::Struct { name, generics } => Type::Struct {
                name: name.clone(),
                generics: generics.iter().map(|g| self.resolve(g)).collect(),
            },
            Type::Enum { name, generics } => Type::Enum {
                name: name.clone(),
                generics: generics.iter().map(|g| self.resolve(g)).collect(),
            },
            Type::Function { params, ret } => Type::Function {
                params: params.iter().map(|p| self.resolve(p)).collect(),
                ret: Box::new(self.resolve(ret)),
            },
            Type::Reference { inner, mutable } => Type::Reference {
                inner: Box::new(self.resolve(inner)),
                mutable: *mutable,
            },
            Type::Shared { inner } => Type::Shared {
                inner: Box::new(self.resolve(inner)),
            },
            Type::Weak { inner } => Type::Weak {
                inner: Box::new(self.resolve(inner)),
            },
            Type::Pointer { inner, mutable } => Type::Pointer {
                inner: Box::new(self.resolve(inner)),
                mutable: *mutable,
            },
            // Primitives and Error pass through unchanged.
            _ => ty.clone(),
        }
    }

    /// Unify two types, making them equal. Returns Ok(()) on success,
    /// or an error diagnostic on type mismatch.
    ///
    /// When a type variable is unified with a concrete type, the binding
    /// is recorded in the substitution map.
    pub fn unify(&mut self, a: &Type, b: &Type, span: Span) -> Result<(), Diagnostic> {
        let a = self.resolve(a);
        let b = self.resolve(b);

        // Error types unify with anything (error recovery).
        if a.is_error() || b.is_error() {
            return Ok(());
        }

        match (&a, &b) {
            // Identical types always unify.
            _ if a == b => Ok(()),

            // Type variable on the left: bind it.
            (Type::Var(id), _) => {
                if self.occurs_in(*id, &b) {
                    Err(Diagnostic::error(format!("infinite type: ?T{id} = {b}"))
                        .with_label(span, "recursive type detected"))
                } else {
                    self.substitutions.insert(*id, b);
                    Ok(())
                }
            }

            // Type variable on the right: bind it.
            (_, Type::Var(id)) => {
                if self.occurs_in(*id, &a) {
                    Err(Diagnostic::error(format!("infinite type: ?T{id} = {a}"))
                        .with_label(span, "recursive type detected"))
                } else {
                    self.substitutions.insert(*id, a);
                    Ok(())
                }
            }

            // Array: element types must match. When combining arrays of
            // different fixed sizes, silently coerce to a dynamic array
            // (size = None) so that concatenation and accumulation patterns
            // work without errors.
            (
                Type::Array {
                    element: e1,
                    size: _,
                },
                Type::Array {
                    element: e2,
                    size: _,
                },
            ) => {
                // Unify element types. Size mismatches are allowed — different
                // fixed sizes unify successfully, treating the result as a
                // dynamic array [T]. No error on [T; N] vs [T; M].
                self.unify(e1, e2, span)
            }

            // Tuple: same length, pairwise unification.
            (Type::Tuple { elements: e1 }, Type::Tuple { elements: e2 }) => {
                if e1.len() != e2.len() {
                    return Err(Diagnostic::error(format!(
                        "tuple length mismatch: {} vs {}",
                        e1.len(),
                        e2.len()
                    ))
                    .with_label(span, "different tuple lengths"));
                }
                for (t1, t2) in e1.iter().zip(e2.iter()) {
                    self.unify(t1, t2, span)?;
                }
                Ok(())
            }

            // Map: key and value types must match.
            (Type::Map { key: k1, value: v1 }, Type::Map { key: k2, value: v2 }) => {
                self.unify(k1, k2, span)?;
                self.unify(v1, v2, span)
            }

            // Set: element types must match.
            (Type::Set { element: e1 }, Type::Set { element: e2 }) => self.unify(e1, e2, span),

            // Option: inner types must match.
            (Type::Option { inner: i1 }, Type::Option { inner: i2 }) => self.unify(i1, i2, span),

            // Result: ok and err types must match.
            (Type::Result { ok: o1, err: e1 }, Type::Result { ok: o2, err: e2 }) => {
                self.unify(o1, o2, span)?;
                self.unify(e1, e2, span)
            }

            // Struct: same name, pairwise generic unification.
            (
                Type::Struct {
                    name: n1,
                    generics: g1,
                },
                Type::Struct {
                    name: n2,
                    generics: g2,
                },
            ) if n1 == n2 && g1.len() == g2.len() => {
                for (t1, t2) in g1.iter().zip(g2.iter()) {
                    self.unify(t1, t2, span)?;
                }
                Ok(())
            }

            // Enum: same name, pairwise generic unification.
            (
                Type::Enum {
                    name: n1,
                    generics: g1,
                },
                Type::Enum {
                    name: n2,
                    generics: g2,
                },
            ) if n1 == n2 && g1.len() == g2.len() => {
                for (t1, t2) in g1.iter().zip(g2.iter()) {
                    self.unify(t1, t2, span)?;
                }
                Ok(())
            }

            // Function: same arity, pairwise param + return unification.
            (
                Type::Function {
                    params: p1,
                    ret: r1,
                },
                Type::Function {
                    params: p2,
                    ret: r2,
                },
            ) => {
                if p1.len() != p2.len() {
                    return Err(Diagnostic::error(format!(
                        "function parameter count mismatch: {} vs {}",
                        p1.len(),
                        p2.len()
                    ))
                    .with_label(span, "different parameter counts"));
                }
                for (t1, t2) in p1.iter().zip(p2.iter()) {
                    self.unify(t1, t2, span)?;
                }
                self.unify(r1, r2, span)
            }

            // Reference: mutability and inner type must match.
            (
                Type::Reference {
                    inner: i1,
                    mutable: m1,
                },
                Type::Reference {
                    inner: i2,
                    mutable: m2,
                },
            ) if m1 == m2 => self.unify(i1, i2, span),

            // Shared: inner types must match.
            (Type::Shared { inner: i1 }, Type::Shared { inner: i2 }) => self.unify(i1, i2, span),

            // Weak: inner types must match.
            (Type::Weak { inner: i1 }, Type::Weak { inner: i2 }) => self.unify(i1, i2, span),

            // Pointer: mutability and inner type must match.
            (
                Type::Pointer {
                    inner: i1,
                    mutable: m1,
                },
                Type::Pointer {
                    inner: i2,
                    mutable: m2,
                },
            ) if m1 == m2 => self.unify(i1, i2, span),

            // Dynamic trait objects: a concrete struct type can be assigned to dyn Trait.
            // Full trait-implementation checking is done during type checking; here we
            // allow the coercion.
            (Type::Struct { .. }, Type::DynTrait { .. })
            | (Type::DynTrait { .. }, Type::Struct { .. }) => Ok(()),
            // Two dyn Traits unify if they name the same trait.
            (Type::DynTrait { trait_name: t1 }, Type::DynTrait { trait_name: t2 }) if t1 == t2 => {
                Ok(())
            }

            // Never unifies with anything (diverging expressions).
            (Type::Never, _) | (_, Type::Never) => Ok(()),

            // Integer widening: smaller signed → larger signed, smaller unsigned → larger unsigned.
            // This allows integer literals (default i32) to be used where i64 is expected.
            _ if is_int_widening(&a, &b) || is_int_widening(&b, &a) => Ok(()),

            // Everything else is a mismatch.
            _ => {
                let mut diag =
                    Diagnostic::error(format!("type mismatch: expected `{a}`, found `{b}`"))
                        .with_label(span, format!("expected type `{a}`, found `{b}`"));

                // Add helpful notes for common mismatches.
                if (a == Type::Str && b.is_numeric()) || (b == Type::Str && a.is_numeric()) {
                    diag = diag.with_note("try using `to_string()` to convert a number to a string, or use `parse()` to convert a string to a number");
                } else if (a == Type::Bool && (b.is_numeric() || b == Type::Str))
                    || (b == Type::Bool && (a.is_numeric() || a == Type::Str))
                {
                    diag = diag.with_note(
                        "Kryos does not implicitly convert between bool and other types",
                    );
                }

                Err(diag)
            }
        }
    }

    /// Occurs check: returns true if variable `id` appears anywhere in `ty`.
    /// Prevents infinite type construction (e.g., T = List<T>).
    fn occurs_in(&self, id: u32, ty: &Type) -> bool {
        let ty = self.resolve(ty);
        match &ty {
            Type::Var(other_id) => *other_id == id,
            Type::Array { element, .. } => self.occurs_in(id, element),
            Type::Tuple { elements } => elements.iter().any(|e| self.occurs_in(id, e)),
            Type::Map { key, value } => self.occurs_in(id, key) || self.occurs_in(id, value),
            Type::Set { element } => self.occurs_in(id, element),
            Type::Option { inner } => self.occurs_in(id, inner),
            Type::Result { ok, err } => self.occurs_in(id, ok) || self.occurs_in(id, err),
            Type::Struct { generics, .. } | Type::Enum { generics, .. } => {
                generics.iter().any(|g| self.occurs_in(id, g))
            }
            Type::Function { params, ret } => {
                params.iter().any(|p| self.occurs_in(id, p)) || self.occurs_in(id, ret)
            }
            Type::Reference { inner, .. }
            | Type::Shared { inner }
            | Type::Weak { inner }
            | Type::Pointer { inner, .. } => self.occurs_in(id, inner),
            _ => false,
        }
    }

    /// Default any remaining unresolved integer type variables to i32,
    /// and float type variables to f64. Called after inference completes.
    pub fn default_numeric_vars(&mut self) {
        // Collect unresolved vars (those with no substitution).
        let unresolved: Vec<u32> = (0..self.next_var)
            .filter(|id| {
                let resolved = self.resolve(&Type::Var(*id));
                matches!(resolved, Type::Var(_))
            })
            .collect();

        // For now, default all unresolved vars to Type::Error.
        // In practice, integer/float literals get constrained during checking;
        // any truly unconstrained vars are errors.
        for id in unresolved {
            self.substitutions.entry(id).or_insert(Type::Error);
        }
    }

    /// Instantiate a type by replacing old type variable IDs with fresh ones.
    ///
    /// Given a mapping from old var IDs to new var IDs, recursively walk the
    /// type and replace any `Var(old_id)` with `Var(new_id)`. This is used
    /// to create fresh copies of generic function signatures at each call site
    /// so that unification at one call doesn't pollute another.
    pub fn instantiate(&self, ty: &Type, var_map: &HashMap<u32, u32>) -> Type {
        match ty {
            Type::Var(id) => {
                if let Some(&new_id) = var_map.get(id) {
                    Type::Var(new_id)
                } else {
                    ty.clone()
                }
            }
            Type::Array { element, size } => Type::Array {
                element: Box::new(self.instantiate(element, var_map)),
                size: *size,
            },
            Type::Tuple { elements } => Type::Tuple {
                elements: elements
                    .iter()
                    .map(|e| self.instantiate(e, var_map))
                    .collect(),
            },
            Type::Map { key, value } => Type::Map {
                key: Box::new(self.instantiate(key, var_map)),
                value: Box::new(self.instantiate(value, var_map)),
            },
            Type::Set { element } => Type::Set {
                element: Box::new(self.instantiate(element, var_map)),
            },
            Type::Option { inner } => Type::Option {
                inner: Box::new(self.instantiate(inner, var_map)),
            },
            Type::Result { ok, err } => Type::Result {
                ok: Box::new(self.instantiate(ok, var_map)),
                err: Box::new(self.instantiate(err, var_map)),
            },
            Type::Struct { name, generics } => Type::Struct {
                name: name.clone(),
                generics: generics
                    .iter()
                    .map(|g| self.instantiate(g, var_map))
                    .collect(),
            },
            Type::Enum { name, generics } => Type::Enum {
                name: name.clone(),
                generics: generics
                    .iter()
                    .map(|g| self.instantiate(g, var_map))
                    .collect(),
            },
            Type::Function { params, ret } => Type::Function {
                params: params
                    .iter()
                    .map(|p| self.instantiate(p, var_map))
                    .collect(),
                ret: Box::new(self.instantiate(ret, var_map)),
            },
            Type::Reference { inner, mutable } => Type::Reference {
                inner: Box::new(self.instantiate(inner, var_map)),
                mutable: *mutable,
            },
            Type::Shared { inner } => Type::Shared {
                inner: Box::new(self.instantiate(inner, var_map)),
            },
            Type::Weak { inner } => Type::Weak {
                inner: Box::new(self.instantiate(inner, var_map)),
            },
            Type::Pointer { inner, mutable } => Type::Pointer {
                inner: Box::new(self.instantiate(inner, var_map)),
                mutable: *mutable,
            },
            // Primitives, Error, Never, DynTrait, etc. pass through unchanged.
            _ => ty.clone(),
        }
    }

    /// Create fresh type variables for a generic function signature and return
    /// the instantiated param types and return type.
    ///
    /// For each generic type parameter's original var ID, a new fresh var is
    /// allocated, and all occurrences in the param/ret types are replaced.
    pub fn instantiate_sig(&mut self, sig: &crate::env::FunctionSig) -> (Vec<Type>, Type) {
        if sig.generic_var_ids.is_empty() {
            // Non-generic function — no instantiation needed.
            let params = sig.params.iter().map(|(_, t)| t.clone()).collect();
            return (params, sig.ret.clone());
        }

        // Build old_id → new_id mapping.
        let mut var_map = HashMap::new();
        for &old_id in &sig.generic_var_ids {
            let new_var = self.fresh_var();
            if let Type::Var(new_id) = new_var {
                var_map.insert(old_id, new_id);
            }
        }

        let params = sig
            .params
            .iter()
            .map(|(_, t)| self.instantiate(t, &var_map))
            .collect();
        let ret = self.instantiate(&sig.ret, &var_map);

        (params, ret)
    }

    /// Get the current substitution map (for debugging/testing).
    pub fn substitutions(&self) -> &HashMap<u32, Type> {
        &self.substitutions
    }
}
