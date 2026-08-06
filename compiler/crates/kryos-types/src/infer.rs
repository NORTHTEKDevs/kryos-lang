//! Type inference engine — Hindley-Milner style unification.
//!
//! Manages type variables, substitution maps, and the core unification
//! algorithm. Supports numeric literal defaulting (int → i32, float → f64).

use std::collections::HashMap;

use kryos_errors::{Diagnostic, Span};

use crate::ty::{CapRow, CapVarId, Type};

/// Maximum type-tree node count `InferenceEngine::resolve` will build for a
/// single call before aborting (LEDGER item 19 -- see `resolve`'s doc
/// comment for the full rationale). Set far above any legitimate concrete
/// type's structural size (a handful to a few dozen nodes for realistic
/// generic instantiations, even nested ones like `Option<Result<[Box<T>],
/// str>>`) and far below the point where building/walking the tree itself
/// becomes the bottleneck.
const MAX_RESOLVE_NODES: usize = 4096;

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

    // Unsigned ↔ i64: i64 is the default type produced by int literals, so
    // without this arm `let x: u8 = 200` can never unify (neither rank table
    // spans the signed/unsigned divide). unify() tests both directions, so
    // this also lets unsigned values flow into i64 contexts — mirroring the
    // existing loose signed behavior (i8 ↔ i64 already unifies both ways).
    if matches!(to, Type::I64)
        && matches!(
            from,
            Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 | Type::USize
        )
    {
        return true;
    }
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
    /// Every `impl Trait for Type` seen by the checker, as
    /// (type_name, trait_name). Used to verify concrete → `dyn Trait`
    /// coercion: accepting a struct with NO impl of the trait type-checked
    /// clean and then segfaulted on the first vtable call (backlog #18/#34).
    trait_impls: std::collections::HashSet<(String, String)>,
    /// Trait bounds carried by call-site-instantiated generic vars
    /// (`fn f<T: Trait>(x: T)` → the fresh var for T at each call site).
    /// Enforced when the var binds to a concrete type: an unsatisfied bound
    /// used to type-check clean and die at link time (unresolved trait
    /// method symbol) or at runtime (backlog #89).
    var_bounds: HashMap<u32, Vec<String>>,
    /// Next fresh capability-row variable id. A SEPARATE numbering space
    /// from `next_var` (ordinary type vars) — a `CapVarId` is never an
    /// index into `substitutions`, only into `cap_substitutions`, so the
    /// two spaces are allowed to collide numerically without ambiguity
    /// (each is only ever looked up in its own map). Kept separate (rather
    /// than sharing `next_var`) so capability-row inference can be disabled
    /// without perturbing ordinary type-variable numbering anywhere a test
    /// or diagnostic hardcodes a `?T<n>` id.
    next_cap_var: CapVarId,
    /// Substitution map for capability-row variables: `CapVarId` → resolved
    /// `CapRow` (which may itself still contain other, still-open vars —
    /// chased transitively by `resolve_cap_row`, mirroring `resolve`'s
    /// handling of `substitutions`). See
    /// `docs/capability-effects-spec.md` §2.3.
    cap_substitutions: HashMap<CapVarId, CapRow>,
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
            trait_impls: std::collections::HashSet::new(),
            var_bounds: HashMap::new(),
            next_cap_var: 0,
            cap_substitutions: HashMap::new(),
        }
    }

    /// Generate a fresh, unresolved capability-row variable id.
    pub fn fresh_cap_var(&mut self) -> CapVarId {
        let id = self.next_cap_var;
        self.next_cap_var += 1;
        id
    }

    /// Bind a capability-row variable to a row (used both by ordinary
    /// unification, invariant to it, and by whatever populates a function's
    /// OWN inferred row once its body has been walked).
    pub fn bind_cap_var(&mut self, id: CapVarId, row: CapRow) {
        // A var already bound is UNIONED with the new row rather than
        // overwritten -- mirrors CapRow::union's "more information can only
        // widen, never shrink" direction, and avoids a last-write-wins bug
        // if the same var is bound from two different call sites (e.g. a
        // recursive function whose own row-var is touched more than once
        // while its body is walked).
        let merged = match self.cap_substitutions.get(&id) {
            Some(existing) => existing.union(&row),
            None => row,
        };
        self.cap_substitutions.insert(id, merged);
    }

    /// Resolve a `CapRow`, recursively chasing every var in it through
    /// `cap_substitutions` to a fixed point (bounded by the number of
    /// distinct vars seen, so a genuine cycle -- e.g. an unresolved
    /// self-recursive row-polymorphic HOF, spec §10 -- terminates instead
    /// of looping forever; the unresolved var is left in the result rather
    /// than guessed at, matching this codebase's fail-closed-by-leaving-
    /// unresolved discipline elsewhere).
    pub fn resolve_cap_row(&self, row: &CapRow) -> CapRow {
        let mut seen: std::collections::HashSet<CapVarId> = std::collections::HashSet::new();
        self.resolve_cap_row_inner(row, &mut seen)
    }

    fn resolve_cap_row_inner(
        &self,
        row: &CapRow,
        seen: &mut std::collections::HashSet<CapVarId>,
    ) -> CapRow {
        row.with_vars_replaced(|v| {
            if !seen.insert(v) {
                // Cycle guard: already expanding this var higher up the
                // chase -- stop here rather than recursing forever; leave
                // it open (still a var), never silently drop it.
                return None;
            }
            let next = self.cap_substitutions.get(&v).cloned();
            let resolved = next.map(|r| self.resolve_cap_row_inner(&r, seen));
            seen.remove(&v);
            resolved
        })
    }

    /// Remap `row` through `cap_var_map`, then freshen ONLY whatever is
    /// still an unresolved var after that.
    ///
    /// Critical ordering, worth stating precisely: this RESOLVES `row`
    /// first (chases `cap_substitutions` to current knowledge), and only
    /// then remaps any var STILL open. A capability-row var populated by
    /// `resolve_type_expr` on an unannotated fn-typed position is
    /// ambiguous at creation time between two different roles that only
    /// resolve once the declaration's OWN body has been walked:
    ///
    /// - a position whose value is a FIXED FACT the body determines on its
    ///   own (e.g. a return position built from a closure the body
    ///   constructs internally, independent of any argument) -- this
    ///   var gets bound to a CLOSED row during the declaration's own
    ///   `check_decl` pass, and every reference must see that SAME closed
    ///   value, never a fresh unbound copy (freshening it would silently
    ///   detach the reference from the binding and leave it permanently
    ///   unresolved -- the bug this ordering exists to prevent);
    /// - a position that genuinely VARIES per call site because its value
    ///   is DERIVED FROM one of the declaration's OWN fn-typed parameters
    ///   (a real HOF, spec §4) -- this var is NEVER bound to anything
    ///   closed during the declaration's own body-check (its charge stays
    ///   expressed in terms of the parameter's own row var), so resolving
    ///   it here is a no-op and freshening is correct.
    ///
    /// Resolving first is what tells the two apart: nothing else about a
    /// `TypeExpr::Function` occurrence at creation time distinguishes them.
    /// Known ordering residual (disclosed, not silently assumed away): this
    /// depends on the referenced declaration's OWN body having already been
    /// walked by the time this call site is checked. `check_decl` processes
    /// declarations in file order, so a genuine FORWARD reference (a
    /// function calling another one declared LATER in the same file) can
    /// still observe an unresolved var here and freshen it before the
    /// later declaration's own binding lands -- the same open-item class as
    /// spec §10's self-recursive-HOF residual, not a new soundness claim
    /// this stage makes and fails to keep.
    pub fn instantiate_row(&self, row: &CapRow, cap_var_map: &HashMap<CapVarId, CapVarId>) -> CapRow {
        let resolved = self.resolve_cap_row(row);
        let mut out = CapRow::closed(resolved.concrete_bits());
        for &v in resolved.var_ids() {
            let mapped = cap_var_map.get(&v).copied().unwrap_or(v);
            out = out.union(&CapRow::var(mapped));
        }
        out
    }

    /// Attach trait bounds to a (fresh, call-site) generic type variable.
    pub fn set_var_bounds(&mut self, id: u32, bounds: Vec<String>) {
        if !bounds.is_empty() {
            self.var_bounds.insert(id, bounds);
        }
    }

    /// When a bounded var binds to a concrete type, verify the bounds.
    /// Binding to another var transfers the bounds; `any`/error is exempt.
    fn check_var_bounds(&mut self, id: u32, bound_to: &Type, span: Span) -> Result<(), Diagnostic> {
        let Some(bounds) = self.var_bounds.get(&id).cloned() else {
            return Ok(());
        };
        let type_name = match bound_to {
            Type::Var(other) => {
                let entry = self.var_bounds.entry(*other).or_default();
                for b in bounds {
                    if !entry.contains(&b) {
                        entry.push(b);
                    }
                }
                return Ok(());
            }
            Type::Error => return Ok(()),
            Type::DynTrait { trait_name } => {
                for b in &bounds {
                    if b != trait_name {
                        return Err(Diagnostic::error(format!(
                            "the trait `{b}` is not implemented for `dyn {trait_name}`"
                        ))
                        .with_label(span, format!("`dyn {trait_name}` does not satisfy the `{b}` bound")));
                    }
                }
                return Ok(());
            }
            Type::Struct { name, .. } | Type::Enum { name, .. } => name.clone(),
            other => format!("{other}"),
        };
        for b in &bounds {
            if !self.implements_trait(&type_name, b) {
                return Err(Diagnostic::error(format!(
                    "the trait `{b}` is not implemented for `{type_name}`"
                ))
                .with_label(
                    span,
                    format!("`{type_name}` does not satisfy the `{b}` bound required here"),
                )
                .with_note(format!(
                    "add `impl {b} for {type_name} {{ ... }}` or pass a type that implements the trait"
                )));
            }
        }
        Ok(())
    }

    /// Record that `type_name` implements `trait_name` (called by the
    /// checker when it registers an `impl Trait for Type` block).
    pub fn register_trait_impl(&mut self, type_name: String, trait_name: String) {
        self.trait_impls.insert((type_name, trait_name));
    }

    /// True if an `impl trait_name for type_name` block was registered.
    pub fn implements_trait(&self, type_name: &str, trait_name: &str) -> bool {
        self.trait_impls
            .contains(&(type_name.to_string(), trait_name.to_string()))
    }

    /// Generate a fresh, unresolved type variable.
    pub fn fresh_var(&mut self) -> Type {
        let id = self.next_var;
        self.next_var += 1;
        Type::Var(id)
    }

    /// Apply all known substitutions to a type, recursively resolving
    /// type variables to their concrete types.
    ///
    /// Resource-DoS guard (LEDGER item 19, 2026-08-05): this is the ACTUAL
    /// site of the "generic that pairs its own type parameter" exponential
    /// blowup, not `kryos-mir`'s `mono_mangled_name` as an earlier round of
    /// this investigation assumed from the mangled-name symptom alone --
    /// `kryos check` never reaches MIR lowering at all
    /// (`check_file_with_options_full` stops after type-check/ownership/
    /// capabilities), so the ~65s-at-depth-24 hang measured under `kryos
    /// check` happens entirely here. `fn dup<T>(x: T) -> (T, T)` chained
    /// (`dup(dup(dup(x)))`) binds a type VARIABLE to a `Type::Tuple`
    /// containing the PREVIOUS type TWICE; this function rebuilds a fresh,
    /// fully-expanded (non-shared) tree on every call, so a chain of N
    /// `dup` calls costs O(2^N) to resolve even once -- and `unify` (below)
    /// calls `resolve` on both operands at the START of every single
    /// unification, so the cost is paid repeatedly, not once. Bounded via
    /// `resolve_bounded`, which aborts the walk (not just the allocation)
    /// the instant the node budget is exhausted, so detecting the
    /// violation costs at most `MAX_RESOLVE_NODES` node visits -- never the
    /// exponential true size of the pathological type.
    pub fn resolve(&self, ty: &Type) -> Type {
        let mut budget = MAX_RESOLVE_NODES;
        match self.resolve_bounded(ty, &mut budget) {
            Some(resolved) => resolved,
            None => kryos_errors::ResourceLimitExceeded::abort(format!(
                "type resolution produced a concrete type exceeding \
                 {MAX_RESOLVE_NODES} type-tree nodes while resolving a single type. \
                 This is the signature of a generic that PAIRS or otherwise \
                 duplicates its own type parameter in its return type across a \
                 chain of calls (e.g. `fn f<T>(x: T) -> (T, T) {{ return (x, x) }}` \
                 called as `f(f(f(x)))`), which doubles the concrete type's size at \
                 every level and produces an exponentially large type from a \
                 linear source program. Break the chain, or stop pairing the \
                 generic's own result back into itself."
            )),
        }
    }

    /// Bounded worker for `resolve`: mirrors its exact recursive structure,
    /// decrementing `budget` by one per node visited and returning `None`
    /// the INSTANT `budget` reaches zero -- before descending into any
    /// further children, and short-circuiting the whole walk via `?` the
    /// moment any nested call bails. This keeps the cost of DETECTING a
    /// pathological type bounded by `budget`, never by the type's true
    /// (possibly exponential) size.
    fn resolve_bounded(&self, ty: &Type, budget: &mut usize) -> Option<Type> {
        if *budget == 0 {
            return None;
        }
        *budget -= 1;
        Some(match ty {
            Type::Var(id) => {
                if let Some(resolved) = self.substitutions.get(id) {
                    // Chase the substitution chain (handles transitive vars).
                    self.resolve_bounded(resolved, budget)?
                } else {
                    ty.clone()
                }
            }
            Type::Array { element, size } => Type::Array {
                element: Box::new(self.resolve_bounded(element, budget)?),
                size: *size,
            },
            Type::Tuple { elements } => {
                let mut out = Vec::with_capacity(elements.len());
                for e in elements {
                    out.push(self.resolve_bounded(e, budget)?);
                }
                Type::Tuple { elements: out }
            }
            Type::Map { key, value } => Type::Map {
                key: Box::new(self.resolve_bounded(key, budget)?),
                value: Box::new(self.resolve_bounded(value, budget)?),
            },
            Type::Set { element } => Type::Set {
                element: Box::new(self.resolve_bounded(element, budget)?),
            },
            Type::Option { inner } => Type::Option {
                inner: Box::new(self.resolve_bounded(inner, budget)?),
            },
            Type::Result { ok, err } => Type::Result {
                ok: Box::new(self.resolve_bounded(ok, budget)?),
                err: Box::new(self.resolve_bounded(err, budget)?),
            },
            Type::Struct { name, generics } => {
                let mut out = Vec::with_capacity(generics.len());
                for g in generics {
                    out.push(self.resolve_bounded(g, budget)?);
                }
                Type::Struct {
                    name: name.clone(),
                    generics: out,
                }
            }
            Type::Enum { name, generics } => {
                let mut out = Vec::with_capacity(generics.len());
                for g in generics {
                    out.push(self.resolve_bounded(g, budget)?);
                }
                Type::Enum {
                    name: name.clone(),
                    generics: out,
                }
            }
            Type::Function { params, ret, caps } => {
                let mut out = Vec::with_capacity(params.len());
                for p in params {
                    out.push(self.resolve_bounded(p, budget)?);
                }
                Type::Function {
                    params: out,
                    ret: Box::new(self.resolve_bounded(ret, budget)?),
                    // Capability rows are chased separately (own cycle
                    // guard in `resolve_cap_row`), not charged against the
                    // node budget above -- a row is bounded by its own
                    // (small, finite) var count, never structurally
                    // recursive the way a paired generic type can be.
                    caps: self.resolve_cap_row(caps),
                }
            }
            Type::Reference { inner, mutable } => Type::Reference {
                inner: Box::new(self.resolve_bounded(inner, budget)?),
                mutable: *mutable,
            },
            Type::Shared { inner } => Type::Shared {
                inner: Box::new(self.resolve_bounded(inner, budget)?),
            },
            Type::Weak { inner } => Type::Weak {
                inner: Box::new(self.resolve_bounded(inner, budget)?),
            },
            Type::Pointer { inner, mutable } => Type::Pointer {
                inner: Box::new(self.resolve_bounded(inner, budget)?),
                mutable: *mutable,
            },
            // Primitives and Error pass through unchanged.
            _ => ty.clone(),
        })
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
                    self.check_var_bounds(*id, &b, span)?;
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
                    self.check_var_bounds(*id, &a, span)?;
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

            // Bridge the builtin generic `Result<T, E>` (used by annotations) and
            // the stdlib `enum Result { Ok(any), Err(any) }` (produced by
            // `Result.Ok(..)` construction). Constructors synthesize payload-
            // bearing generics ([ok, err]); check them against the annotation
            // so `-> Result<i64, str> { return Err(42) }` is rejected instead
            // of segfaulting at use (backlog #13/#19). A bare/erased enum (no
            // generics — e.g. a stdlib fn declared `-> Result`) stays
            // interchangeable, matching the documented erasure semantics.
            (Type::Result { ok, err }, Type::Enum { name, generics })
            | (Type::Enum { name, generics }, Type::Result { ok, err })
                if name == "Result" =>
            {
                if generics.len() == 2 {
                    self.unify(ok, &generics[0], span)?;
                    self.unify(err, &generics[1], span)?;
                }
                Ok(())
            }
            // Same bridge for `Option<T>` and `enum Option { Some(any), None }`.
            (Type::Option { inner }, Type::Enum { name, generics })
            | (Type::Enum { name, generics }, Type::Option { inner })
                if name == "Option" =>
            {
                if generics.len() == 1 {
                    self.unify(inner, &generics[0], span)?;
                }
                Ok(())
            }

            // Erased-compat: a bare stdlib `Result`/`Option` enum (no
            // generics) unifies with any payload-bearing instantiation of
            // the same enum — bare annotations erase payloads to i64 slots
            // (CLAUDE.md gotcha #13) and stdlib helpers are typed that way.
            (
                Type::Enum {
                    name: n1,
                    generics: g1,
                },
                Type::Enum {
                    name: n2,
                    generics: g2,
                },
            ) if n1 == n2
                && (n1 == "Result" || n1 == "Option")
                && g1.len() != g2.len()
                && (g1.is_empty() || g2.is_empty()) =>
            {
                Ok(())
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
            //
            // The bare `fn` type (produced by the parser when no signature
            // follows the `fn` keyword) encodes an opaque any-callable as
            // `fn(any) -> any` (after type resolution, that's a function
            // type whose single parameter and return are both Type::Error).
            // Any concrete function type unifies with the opaque callable
            // without arity or parameter-type checking.
            (
                Type::Function {
                    params: p1,
                    ret: r1,
                    caps: c1,
                },
                Type::Function {
                    params: p2,
                    ret: r2,
                    caps: c2,
                },
            ) => {
                let is_opaque = |params: &Vec<Type>, ret: &Box<Type>| -> bool {
                    params.len() == 1
                        && matches!(&params[0], Type::Error)
                        && matches!(ret.as_ref(), Type::Error)
                };
                if is_opaque(p1, r1) || is_opaque(p2, r2) {
                    // Opaque `fn` unifies with anything, capability row
                    // included -- an opaque callable's own row is never
                    // meaningful, so there is nothing to bind here.
                    return Ok(());
                }
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
                self.unify(r1, r2, span)?;
                // Capability-row unification (spec §2.3): never a hard
                // error -- accept/reject for a function TYPE is decided
                // entirely by params/ret above (Stage 1 adds no new
                // rejection). A bare row var on either side binds to the
                // other side's row (mirroring the ordinary `Type::Var`
                // arms above); two closed rows UNION rather than requiring
                // exact equality, which is intentionally MORE permissive
                // than a naive equality check would be (spec §8's
                // documented compatibility note: mixing two closed-row
                // closures, e.g. in an array literal, must still unify).
                match (c1.is_closed(), c2.is_closed()) {
                    (false, _) => {
                        for &v in c1.var_ids() {
                            self.bind_cap_var(v, c2.clone());
                        }
                    }
                    (true, false) => {
                        for &v in c2.var_ids() {
                            self.bind_cap_var(v, c1.clone());
                        }
                    }
                    (true, true) => {
                        // Both closed: nothing to bind, nothing to reject --
                        // the union is implicit at every read site via
                        // `resolve_cap_row`/`CapRow::union` wherever the two
                        // flow together (e.g. an array literal's element
                        // type is unioned explicitly by its own checker
                        // code, not by this arm).
                    }
                }
                Ok(())
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

            // Dynamic trait objects: a concrete struct type can be assigned
            // to `dyn Trait` ONLY when an `impl Trait for Struct` exists —
            // an unimplemented coercion type-checked clean and segfaulted at
            // the first vtable dispatch (backlog #18/#34).
            (Type::Struct { name: sname, .. }, Type::DynTrait { trait_name })
            | (Type::DynTrait { trait_name }, Type::Struct { name: sname, .. }) => {
                if self.implements_trait(sname, trait_name) {
                    Ok(())
                } else {
                    Err(Diagnostic::error(format!(
                        "the trait `{trait_name}` is not implemented for `{sname}`"
                    ))
                    .with_label(
                        span,
                        format!("`{sname}` cannot be coerced to `dyn {trait_name}`"),
                    )
                    .with_note(format!(
                        "add `impl {trait_name} for {sname} {{ ... }}` or pass a type that implements the trait"
                    )))
                }
            }
            // Two dyn Traits unify if they name the same trait.
            (Type::DynTrait { trait_name: t1 }, Type::DynTrait { trait_name: t2 }) if t1 == t2 => {
                Ok(())
            }

            // Never unifies with anything (diverging expressions).
            (Type::Never, _) | (_, Type::Never) => Ok(()),

            // Integer widening: smaller signed → larger signed, smaller unsigned → larger unsigned.
            // This allows integer literals (default i32) to be used where i64 is expected.
            _ if is_int_widening(&a, &b) || is_int_widening(&b, &a) => Ok(()),

            // usize/isize are pointer-sized integers that lower to i64 in MIR
            // (lower_resolved_type maps both to MirType::I64). Treat them as
            // unifiable with any integer type -- the lowering sign/zero-extends.
            // Without this, `let n: usize = 42` failed (42 is i64, n is USize:
            // different signedness -> not int-widening -> mismatch).
            (Type::USize | Type::ISize, other) | (other, Type::USize | Type::ISize)
                if other.is_integer() =>
            {
                Ok(())
            }

            // Everything else is a mismatch.
            _ => {
                let mut diag =
                    Diagnostic::error(format!("type mismatch: expected `{a}`, found `{b}`"))
                        .with_label(span, format!("expected type `{a}`, found `{b}`"))
                        .with_code(kryos_errors::codes::E0100);

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
            Type::Function { params, ret, .. } => {
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
    ///
    /// Capability-row variables nested inside `ty` are left UNCHANGED by
    /// this entry point (empty `cap_var_map`) — every existing caller of
    /// `instantiate` predates capability-row polymorphism and instantiates
    /// ordinary struct/enum generics where no row-variable freshening is
    /// needed. `instantiate_sig` (the one place row-variable freshening
    /// actually matters, spec §2.3) calls `instantiate_with_caps` directly
    /// with a real `cap_var_map` instead.
    pub fn instantiate(&self, ty: &Type, var_map: &HashMap<u32, u32>) -> Type {
        self.instantiate_with_caps(ty, var_map, &HashMap::new())
    }

    /// Same as `instantiate`, but also remaps any `CapVarId` found inside a
    /// nested `Type::Function`'s `caps` row through `cap_var_map` (mirrors
    /// `var_map`'s treatment of ordinary `Type::Var`). See
    /// `docs/capability-effects-spec.md` §2.3.
    pub fn instantiate_with_caps(
        &self,
        ty: &Type,
        var_map: &HashMap<u32, u32>,
        cap_var_map: &HashMap<CapVarId, CapVarId>,
    ) -> Type {
        match ty {
            Type::Var(id) => {
                if let Some(&new_id) = var_map.get(id) {
                    Type::Var(new_id)
                } else {
                    ty.clone()
                }
            }
            Type::Array { element, size } => Type::Array {
                element: Box::new(self.instantiate_with_caps(element, var_map, cap_var_map)),
                size: *size,
            },
            Type::Tuple { elements } => Type::Tuple {
                elements: elements
                    .iter()
                    .map(|e| self.instantiate_with_caps(e, var_map, cap_var_map))
                    .collect(),
            },
            Type::Map { key, value } => Type::Map {
                key: Box::new(self.instantiate_with_caps(key, var_map, cap_var_map)),
                value: Box::new(self.instantiate_with_caps(value, var_map, cap_var_map)),
            },
            Type::Set { element } => Type::Set {
                element: Box::new(self.instantiate_with_caps(element, var_map, cap_var_map)),
            },
            Type::Option { inner } => Type::Option {
                inner: Box::new(self.instantiate_with_caps(inner, var_map, cap_var_map)),
            },
            Type::Result { ok, err } => Type::Result {
                ok: Box::new(self.instantiate_with_caps(ok, var_map, cap_var_map)),
                err: Box::new(self.instantiate_with_caps(err, var_map, cap_var_map)),
            },
            Type::Struct { name, generics } => Type::Struct {
                name: name.clone(),
                generics: generics
                    .iter()
                    .map(|g| self.instantiate_with_caps(g, var_map, cap_var_map))
                    .collect(),
            },
            Type::Enum { name, generics } => Type::Enum {
                name: name.clone(),
                generics: generics
                    .iter()
                    .map(|g| self.instantiate_with_caps(g, var_map, cap_var_map))
                    .collect(),
            },
            Type::Function { params, ret, caps } => Type::Function {
                params: params
                    .iter()
                    .map(|p| self.instantiate_with_caps(p, var_map, cap_var_map))
                    .collect(),
                ret: Box::new(self.instantiate_with_caps(ret, var_map, cap_var_map)),
                caps: self.instantiate_row(caps, cap_var_map),
            },
            Type::Reference { inner, mutable } => Type::Reference {
                inner: Box::new(self.instantiate_with_caps(inner, var_map, cap_var_map)),
                mutable: *mutable,
            },
            Type::Shared { inner } => Type::Shared {
                inner: Box::new(self.instantiate_with_caps(inner, var_map, cap_var_map)),
            },
            Type::Weak { inner } => Type::Weak {
                inner: Box::new(self.instantiate_with_caps(inner, var_map, cap_var_map)),
            },
            Type::Pointer { inner, mutable } => Type::Pointer {
                inner: Box::new(self.instantiate_with_caps(inner, var_map, cap_var_map)),
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
    /// Also freshens every `generic_cap_var_ids` entry the SAME way (spec
    /// §2.3/§4): a row-polymorphic HOF's callback-parameter row gets a
    /// fresh `CapVarId` at each call site, exactly like `T`/`U`, so
    /// unifying THIS call's actual argument only binds THIS call's copy of
    /// the row var, never polluting another call site (mirrors why
    /// ordinary generic vars are freshened at all -- ADR already proven
    /// sound for `T`/`U`, applied one field over).
    /// Returns `(params, ret, var_map, cap_var_map)` -- `cap_var_map` is the
    /// freshening this call site applied to `sig.generic_cap_var_ids`,
    /// returned so the caller can ALSO remap `sig.own_cap_var`'s resolved
    /// row through it (see `check.rs`'s `Expr::Identifier` arm) -- `
    /// own_cap_var` itself is deliberately not a member of
    /// `generic_cap_var_ids`, so it is not freshened directly here.
    pub fn instantiate_sig(
        &mut self,
        sig: &crate::env::FunctionSig,
    ) -> (Vec<Type>, Type, HashMap<u32, u32>, HashMap<CapVarId, CapVarId>) {
        if sig.generic_var_ids.is_empty() && sig.generic_cap_var_ids.is_empty() {
            // Non-generic function — no instantiation needed.
            let params = sig.params.iter().map(|(_, t)| t.clone()).collect();
            return (params, sig.ret.clone(), HashMap::new(), HashMap::new());
        }

        // Build old_id → new_id mapping.
        let mut var_map = HashMap::new();
        for &old_id in &sig.generic_var_ids {
            let new_var = self.fresh_var();
            if let Type::Var(new_id) = new_var {
                var_map.insert(old_id, new_id);
            }
        }

        // Same freshening for the declaration's own capability-row
        // variables (unannotated fn-typed param/return positions).
        let mut cap_var_map: HashMap<CapVarId, CapVarId> = HashMap::new();
        for &old_id in &sig.generic_cap_var_ids {
            cap_var_map.insert(old_id, self.fresh_cap_var());
        }

        let params = sig
            .params
            .iter()
            .map(|(_, t)| self.instantiate_with_caps(t, &var_map, &cap_var_map))
            .collect();
        let ret = self.instantiate_with_caps(&sig.ret, &var_map, &cap_var_map);

        (params, ret, var_map, cap_var_map)
    }

    /// Get the current substitution map (for debugging/testing).
    pub fn substitutions(&self) -> &HashMap<u32, Type> {
        &self.substitutions
    }
}
