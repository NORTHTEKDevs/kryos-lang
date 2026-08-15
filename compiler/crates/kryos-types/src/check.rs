//! Type checking pass - walks the AST and validates types.
//!
//! Resolves `TypeExpr` → `Type`, infers types for unannotated let bindings,
//! checks function bodies, binary ops, calls, field access, etc.
//! Reports mismatches as `Diagnostic` errors.

use kryos_ast::{BinOp, Block, Decl, Expr, Module, Pattern, Stmt, TypeExpr, UnOp};
use kryos_errors::{Diagnostic, Span};

use crate::env::{EnumDef, FunctionSig, StructDef, TypeEnv};
use crate::infer::InferenceEngine;
use crate::ty::Type;

/// The main type checker.
pub struct TypeChecker {
    pub env: TypeEnv,
    pub engine: InferenceEngine,
    pub diagnostics: Vec<Diagnostic>,
    /// The expected return type of the function currently being checked.
    current_return_type: Option<Type>,
    /// The name of the function currently being checked (for better error messages).
    current_function_name: Option<String>,
    /// Functions marked with @deprecated - emit warnings on call.
    deprecated_functions: std::collections::HashSet<String>,
    /// Top-level function names already registered this module, for duplicate
    /// detection (local-vs-local and local-vs-imported -- imports are merged
    /// as ordinary decls by the resolver). Builtin shadowing stays allowed
    /// (builtins are not decls).
    seen_fn_names: std::collections::HashSet<String>,
    /// Functions marked with @pure - cannot call non-pure or do I/O.
    pure_functions: std::collections::HashSet<String>,
    /// Names declared as ACTORS. Actors register as struct-like types (so
    /// `self.field` type-checks in handlers), which wrongly let the
    /// struct-literal form `Counter { count: 0 }` type-check -- the LLVM
    /// backend cannot compile it, and actor state is private by design.
    actor_names: std::collections::HashSet<String>,
    /// Whether we are currently inside a @pure function body.
    in_pure_function: bool,
    /// The current `Self` type - set when checking trait/impl blocks.
    current_self_type: Option<Type>,
    /// Duplicate-binding detection for the pattern currently being bound.
    /// Set to an empty map at each top-level `bind_pattern` entry; every
    /// GENUINE binding (not a bare-variant tag test) records its name here,
    /// and a repeat (`let (a, a) = ..`, `P(x, x) => ..`) is rejected --
    /// previously the last duplicate silently won.
    pattern_dup_seen: Option<std::collections::HashMap<String, Span>>,
    /// Maps generic type variable id → list of trait bound names. Populated
    /// when entering a generic function with bounds (e.g. `<T: Showable>`).
    /// Used in MethodCall resolution: when `obj_ty` resolves to a Type::Var
    /// that has registered bounds, the method is looked up on the bound
    /// traits' method signatures so `x.show()` typechecks inside
    /// `fn announce<T: Showable>(x: T)`.
    generic_var_bounds: std::collections::HashMap<u32, Vec<String>>,
    /// Bidirectional inference: pushed expected types for lambda args,
    /// keyed by the lambda's span. Set by `Expr::FnCall` before inferring
    /// a Lambda arg whose corresponding param is a Function type; consumed
    /// by `Expr::Lambda` to pre-unify un-annotated params and return.
    lambda_expected_types: std::collections::HashMap<Span, (Vec<Type>, Type)>,
    /// Resolved concrete types for each lambda's UN-annotated parameters,
    /// keyed by the lambda's span. Recorded after a lambda is checked so the
    /// MIR lowering can type the closure's params (otherwise it defaults them
    /// to i64, miscompiling e.g. a `str` closure passed to a higher-order fn).
    /// Per param: Some(TypeExpr) for an inferred un-annotated param, None when
    /// the param was annotated or its type stayed unresolved.
    resolved_lambda_params: std::collections::HashMap<Span, Vec<Option<TypeExpr>>>,
    /// Resolved type of an unannotated `let X = []` (empty-array) binding, keyed
    /// by the Let span. The element type is only known after later `push(X, v)`
    /// unification, so this is resolved at end-of-check. The MIR consumes it to
    /// type the local (its own inference defaults an empty array's element to
    /// i64, which mis-types `X[i].field` / aggregate elements on AOT).
    resolved_let_types: std::collections::HashMap<Span, Type>,
    /// Nesting depth of enclosing `unsafe { }` blocks. Raw-pointer dereference
    /// is only permitted when this is > 0 (else E0500).
    unsafe_depth: u32,
    /// Actor handlers declared with a non-void return type, keyed by
    /// (actor name, handler name) -> declared return type. Actor dispatch is
    /// genuinely asynchronous (each actor runs on its own OS thread; sends
    /// enqueue into a mailbox with no reply channel -- see
    /// kryos-rt/src/actor.rs), so a handler's return value can never reach
    /// the call site. The handler BODY still type-checks against its
    /// declared return (so `fn add(x: i64) -> i64 { return memory }` is
    /// valid Kryos, matching docs/09-concurrency.md), but calling it is
    /// rejected here rather than silently threading back 0 (the previous
    /// bug: the call-site FunctionSig.ret was wrongly set to the declared
    /// type even though codegen discards the actual return).
    actor_nonvoid_handlers: std::collections::HashMap<(String, String), Type>,
    /// Set by `Stmt::Let` just before checking a Lambda-valued initializer
    /// (`let name = |...| { ... }`, which is also what a nested/local named
    /// `fn name(...) { ... }` desugars to in the parser). Consumed once by
    /// the very next `Expr::Lambda` check, which pre-binds `name` to its own
    /// (still-inferring) Function type in the ENCLOSING scope before pushing
    /// the lambda's body scope -- so a self-recursive call inside the body
    /// resolves instead of raising E0102 "undefined variable". Mirrors the
    /// top-level two-pass model (signatures collected before any body is
    /// checked) for this one nested/let-bound case.
    pending_self_recursive_name: Option<String>,
    /// Spans of array-literal expressions whose declared/parameter type is
    /// SPECIFICALLY `[dyn Trait]` (an array of a `dyn` element), which
    /// `reject_dyn_in_container` already rejects with E0110 before the
    /// literal is ever inferred. Consulted by `Expr::ArrayLiteral` to skip
    /// its normal pairwise element-unification: forcing `[A{}, B{}]`'s
    /// elements to unify with each other produced a second, confusing
    /// `E0100 type mismatch: expected A, found B` on top of the real E0110
    /// -- noise, since the annotation was already rejected as unsupported
    /// and the elements were never supposed to be the SAME concrete type.
    /// Populated by `Stmt::Let` from the raw (pre-resolution) `TypeExpr`, so
    /// this is narrowly scoped to the dyn-in-array shape specifically --
    /// NOT to "any annotation that happened to resolve to Type::Error"
    /// (e.g. a plain unknown-type-name annotation over a genuinely
    /// mismatched array literal must keep BOTH diagnostics).
    suppress_array_elem_unify: std::collections::HashSet<Span>,
    /// `(function_name, param_index)` pairs whose DECLARED param type was
    /// rejected by `reject_dyn_in_container` (a `[dyn Trait]`/`(dyn Trait,
    /// ..)` container annotation, already reported as E0110 at the
    /// function's own declaration) and therefore resolved to `Type::Error`
    /// in `FunctionSig.params`. `FunctionSig` only stores the final
    /// resolved `Type`, with no way to tell "this Error came from a
    /// rejected dyn container" apart from "this Error came from an
    /// unrelated unknown-type-name annotation" at a later call site -- so
    /// this side table is populated once, at the point the raw `TypeExpr`
    /// is still available (signature registration), and consulted by the
    /// call-argument checker to decide whether an array-literal ARGUMENT
    /// passed for that exact parameter should skip its own pairwise
    /// element-unify (mirrors `suppress_array_elem_unify`'s already-fixed
    /// `let x: [dyn Trait] = [A{}, B{}]` case, extended to a call site:
    /// `use_handlers([A{}, B{}])`). Narrowly scoped to this one param
    /// identity, not to "any Type::Error param", so an unrelated genuinely
    /// mismatched array literal passed to a DIFFERENT opaque-typed param
    /// keeps both diagnostics.
    dyn_container_reject_params: std::collections::HashSet<(String, usize)>,

    // ── Capability-typed fn values (Stage 1: representation + inference,
    // no enforcement -- docs/capability-effects-spec.md) ──────────────
    /// Fresh capability-row variable ids collected while resolving a
    /// SINGLE declaration's own signature (`resolve_type_expr` pushes one
    /// entry per `TypeExpr::Function` it resolves, top-level or nested
    /// inside a container/generic arg). Drained by the caller right after
    /// signature resolution into that declaration's `FunctionSig::
    /// generic_cap_var_ids`. Left un-drained (and simply ignored) for
    /// non-declaration resolutions (e.g. a struct field's own type, a
    /// `let` annotation) -- harmless, since nothing else reads this field.
    pending_cap_var_ids: Vec<crate::ty::CapVarId>,
    /// One entry per function/lambda body CURRENTLY being checked (a
    /// stack because bodies nest: a lambda literal inside a function
    /// body inside another lambda...). Every gated-builtin call and every
    /// call through a resolved `Type::Function` value unions its
    /// `caps`/required bits into `.last_mut()`. Popped and used to
    /// finalize that body's own inferred row when the body finishes.
    cap_accum_stack: Vec<crate::ty::CapRow>,
    /// State-field names of the actor whose handler body is currently being
    /// checked, restricted to fields whose type CONTAINS a function anywhere.
    /// Empty outside an actor handler.
    ///
    /// Reading one of these fields yields `CapRow::Unknown`, never the
    /// declaration's row var. An actor's state is mutable storage that ANY
    /// handler may write at ANY prior dispatch, so the closure sitting in a
    /// fn-bearing state field at a given call site is genuinely not knowable
    /// statically -- exactly the condition `CapRow::Unknown` exists for. The
    /// capability checker already takes this stance for `self.<field>()` (see
    /// `resolve_actor_self_field_invoke_caps`); this is the same rule applied
    /// to the row.
    current_actor_fn_state_fields: std::collections::HashSet<String>,
    /// `KRYOS_DUMP_FN_EFFECTS=1` debug dump, read once at construction.
    /// When set, every declared function and lambda literal's FINAL
    /// (fully `resolve_cap_row`-resolved) capability row is recorded here
    /// as it finishes checking, and `kryos-driver` prints it after the
    /// whole module is checked -- ground truth for verifying the
    /// inference against, per this stage's acceptance criteria.
    dump_fn_effects: bool,
    /// `(label, span, row)` entries recorded when `dump_fn_effects` is on.
    /// `row` is recorded RAW (not yet `resolve_cap_row`-resolved) since
    /// more bindings can still land after this entry is pushed (e.g. an
    /// earlier function referencing a later one); the actual dump output
    /// resolves each entry's row once, at the very end of the whole
    /// module (see `TypeChecker::dump_fn_effects_report`).
    pub cap_effects_log: Vec<(String, Span, crate::ty::CapRow)>,
    /// One shared, permanently-empty-bound capability-row var used as
    /// EVERY compiler-builtin `FunctionSig`'s `own_cap_var`. A builtin
    /// referenced as a first-class value therefore always displays `{}`
    /// via this field -- the REAL gated-capability charge for a builtin
    /// CALL is applied separately, by name, at the call site
    /// (`accumulate_builtin_call`, reusing `kryos_capabilities::model::
    /// required_capability_for_builtin` directly) rather than by encoding
    /// per-builtin capability info into all ~200 registrations below,
    /// which would duplicate that table's maintenance burden a second
    /// time in this crate.
    builtin_cap_var: crate::ty::CapVarId,
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeChecker {
    pub fn new() -> Self {
        let mut engine = InferenceEngine::new();
        let builtin_cap_var = engine.fresh_cap_var();
        engine.bind_cap_var(builtin_cap_var, crate::ty::CapRow::empty());
        Self {
            env: TypeEnv::new(),
            engine,
            diagnostics: Vec::new(),
            current_return_type: None,
            current_function_name: None,
            deprecated_functions: std::collections::HashSet::new(),
            seen_fn_names: std::collections::HashSet::new(),
            pure_functions: std::collections::HashSet::new(),
            actor_names: std::collections::HashSet::new(),
            in_pure_function: false,
            current_self_type: None,
            generic_var_bounds: std::collections::HashMap::new(),
            lambda_expected_types: std::collections::HashMap::new(),
            resolved_lambda_params: std::collections::HashMap::new(),
            resolved_let_types: std::collections::HashMap::new(),
            unsafe_depth: 0,
            actor_nonvoid_handlers: std::collections::HashMap::new(),
            pending_self_recursive_name: None,
            pattern_dup_seen: None,
            suppress_array_elem_unify: std::collections::HashSet::new(),
            dyn_container_reject_params: std::collections::HashSet::new(),
            pending_cap_var_ids: Vec::new(),
            cap_accum_stack: Vec::new(),
            current_actor_fn_state_fields: std::collections::HashSet::new(),
            dump_fn_effects: std::env::var("KRYOS_DUMP_FN_EFFECTS")
                .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true")),
            cap_effects_log: Vec::new(),
            builtin_cap_var,
        }
    }

    /// Union `row` into the capability accumulator for whichever
    /// function/lambda body is currently being checked (no-op if nothing
    /// is on the stack -- e.g. a call made while resolving a top-level
    /// `let mut` initializer outside any function body). See the "single
    /// enforcement rule" write-up in `docs/capability-effects-spec.md` §6:
    /// this is the ONE place authority a value carries gets folded into
    /// its caller's own requirement, driven entirely by the callee's
    /// TYPE -- never by re-deriving provenance from the call's syntax.
    /// True if `ty` contains a function type anywhere reachable inside it
    /// (directly, or nested in a tuple / option / array / map / reference).
    /// Used to decide whether an actor state field is fn-BEARING; a tuple
    /// element counts, which is exactly the shape that leaked (LEDGER item 32:
    /// `pair: (i64, fn() -> str)`).
    fn type_contains_function(ty: &Type) -> bool {
        match ty {
            Type::Function { .. } => true,
            Type::Tuple { elements } => elements.iter().any(Self::type_contains_function),
            Type::Array { element, .. } => Self::type_contains_function(element),
            Type::Option { inner } | Type::Reference { inner, .. } => {
                Self::type_contains_function(inner)
            }
            Type::Map { key, value } => {
                Self::type_contains_function(key) || Self::type_contains_function(value)
            }
            _ => false,
        }
    }

    fn accumulate_caps(&mut self, row: &crate::ty::CapRow) {
        if let Some(top) = self.cap_accum_stack.last_mut() {
            *top = top.union(row);
        }
    }

    /// Union a single gated-builtin/extern capability bit, by NAME, into
    /// the current accumulator. Reuses `kryos_capabilities::model`'s
    /// builtin-name table directly (this crate depends on
    /// `kryos-capabilities` for exactly this bridge -- see `ty.rs`'s
    /// `CapBits::from_capability` doc comment) rather than duplicating an
    /// ~80-line, actively-maintained table a second time.
    fn accumulate_builtin_call(&mut self, name: &str) {
        if let Some(cap) = kryos_capabilities::model::required_capability_for_builtin(name) {
            let bits = crate::ty::CapBits::from_capability(cap);
            if let Some(top) = self.cap_accum_stack.last_mut() {
                *top = top.union_bits(bits);
            }
        }
    }

    /// Record a fn-typed binding's row for the `KRYOS_DUMP_FN_EFFECTS`
    /// debug dump. `row` is stored as given (may still contain open vars
    /// resolved only once the WHOLE module finishes checking); resolution
    /// happens in `dump_fn_effects_report`, called once at the very end.
    fn log_fn_effect(&mut self, label: impl Into<String>, span: Span, row: crate::ty::CapRow) {
        if self.dump_fn_effects {
            self.cap_effects_log.push((label.into(), span, row));
        }
    }

    /// Render the final (fully resolved) capability row for every logged
    /// fn-typed binding, one line per entry, in recording order. Called
    /// once by `kryos-driver` after the whole module (register_decl +
    /// check_decl for every declaration) has finished checking, so every
    /// forward reference has had a chance to resolve.
    pub fn dump_fn_effects_report(&self) -> String {
        let mut out = String::new();
        for (label, span, row) in &self.cap_effects_log {
            let resolved = self.engine.resolve_cap_row(row);
            out.push_str(&format!(
                "{label} @ {}:{} => {}\n",
                span.start, span.end, resolved.display()
            ));
        }
        out
    }

    /// Reject duplicate names in a parameter list. Params are stored
    /// positionally with no uniqueness check, so `fn f(a: i64, a: i64)`
    /// silently resolved `a` to the LAST duplicate.
    fn check_duplicate_params(&mut self, params: &[kryos_ast::Param], what: &str) {
        let mut seen = std::collections::HashSet::new();
        for p in params {
            // `_` is the discard placeholder -- `fn f(_: i64, _: i64)` is
            // deliberate "ignore both", not a duplicate.
            if p.name == "_" {
                continue;
            }
            if !seen.insert(p.name.as_str()) {
                self.error(
                    format!(
                        "duplicate parameter `{}` in {what} -- each parameter name may appear only once",
                        p.name
                    ),
                    p.span,
                );
            }
        }
    }

    /// Reject duplicate generic parameter names (`fn f<T, T>(..)`).
    /// Reject an enum-variant construction whose argument count differs from
    /// the variant's declared field count. Silently zipping min(args, fields)
    /// dropped extra args and left too-few constructions with garbage payload.
    fn check_variant_construct_arity(
        &mut self,
        variant: &str,
        declared: usize,
        provided: usize,
        span: Span,
    ) {
        if declared != provided {
            self.error_with_code(
                format!(
                    "enum variant `{variant}` takes {declared} argument{} but {provided} {} provided",
                    if declared == 1 { "" } else { "s" },
                    if provided == 1 { "was" } else { "were" }
                ),
                span,
                kryos_errors::codes::E0110,
            );
        }
    }

    /// Bidirectional inference for a TAIL-position closure literal (implicit
    /// return): `fn mk() -> fn(f64) -> f64 { |x| x * x }`. If the body's last
    /// statement is a bare un-annotated lambda and the enclosing function's
    /// declared return type is `fn(A) -> B`, seed `lambda_expected_types` so
    /// the lambda's params/return infer from the declaration -- the same
    /// seeding the FnCall-arg and `return <lambda>` paths do. Without this the
    /// tail lambda type-checked with FRESH vars that never unified against the
    /// declared return (check_block validates statements only), so its params
    /// silently defaulted to i64 -- an f64 body computed on garbage bits.
    /// Call after `current_return_type` is set, before `check_block(body)`.
    fn seed_tail_lambda_expected(&mut self, body: &Block) {
        if let Some(Stmt::Expr {
            expr:
                Expr::Lambda {
                    params: lparams,
                    span: lspan,
                    ..
                },
            ..
        }) = body.stmts.last()
        {
            if let Some(ref expected) = self.current_return_type {
                let resolved = self.engine.resolve(expected);
                if let Type::Function { params: eps, ret: er, .. } = &resolved {
                    if eps.len() == lparams.len() {
                        self.lambda_expected_types
                            .insert(*lspan, (eps.clone(), (**er).clone()));
                    }
                }
            }
        }
    }

    fn check_duplicate_generics(
        &mut self,
        generics: &[kryos_ast::GenericParam],
        what: &str,
        span: Span,
    ) {
        let mut seen = std::collections::HashSet::new();
        for gp in generics {
            if !seen.insert(gp.name.as_str()) {
                self.error(
                    format!(
                        "duplicate generic parameter `{}` on {what} -- each generic parameter may appear only once",
                        gp.name
                    ),
                    span,
                );
            }
        }
    }

    /// Report an error diagnostic.
    fn error(&mut self, msg: impl Into<String>, span: Span) {
        // Uncategorized type errors still get a code so every diagnostic is
        // explainable via `kryos explain`. Specific cases use error_with_code.
        self.diagnostics.push(
            Diagnostic::error(msg)
                .with_label(span, "here")
                .with_code(kryos_errors::codes::E0110),
        );
    }

    /// Report an error diagnostic with an error code.
    fn error_with_code(&mut self, msg: impl Into<String>, span: Span, code: &str) {
        self.diagnostics.push(
            Diagnostic::error(msg)
                .with_label(span, "here")
                .with_code(code),
        );
    }

    /// Report a warning diagnostic.
    fn warning(&mut self, msg: impl Into<String>, span: Span) {
        self.diagnostics
            .push(Diagnostic::warning(msg).with_label(span, "here"));
    }

    /// LEDGER item 40, the `[any]`-container sibling of item 24.
    ///
    /// `any` resolves to `Type::Error` -- the erasure sentinel (item 6, an
    /// ABI-blocked design note: `any` is a bare i64 with NO runtime type tag).
    /// `bool` (i1) and `f64`/`f32` are not i64-shaped, so a value of those
    /// types reaching an `any` slot is reinterpreted, not converted.
    ///
    /// Item 24 closed the DIRECT shape (`let x: any = <bool>`) at `Stmt::Let`.
    /// It cannot see this one: by the time `let b: any = args[0]` runs, the
    /// element type of an `[any]` PARAMETER was already erased at the
    /// parameter's own declaration, so no concrete `Bool` survives to inspect.
    /// The last point a concrete type still exists is the CALL SITE, where a
    /// `[bool]` argument is unified against an `[any]` parameter -- so the
    /// check belongs here.
    ///
    /// Measured before the fix: `log_event([true])` where `fn log_event(args:
    /// [any])` builds CLEAN on both backends and prints THREE different values
    /// for one program -- correct `true`, JIT `1`, AOT `-1`. AOT reached a
    /// clean build, a clean run, and a wrong number, which is the worst case in
    /// this repo's ranking doctrine (a silent wrong answer outranks a crash).
    ///
    /// Only fires when the ARGUMENT side carries a CONCRETE `Bool`/`F64`/`F32`
    /// at a position where the PARAMETER side is the `any` sentinel. That
    /// pairing is what codegen mishandles. A genuine upstream type error
    /// produces `Type::Error` on the ARGUMENT side, which this never reports
    /// on, so a broken program does not get extra noise from this check.
    fn reject_untagged_scalar_into_any(&mut self, param_ty: &Type, arg_ty: &Type, span: Span) {
        let p = self.engine.resolve(param_ty);
        let a = self.engine.resolve(arg_ty);
        // DELIBERATELY NOT CHECKED AT THE TOP LEVEL. A bare `Type::Error`
        // parameter is ALSO how the polymorphic builtins (`to_string`, `abs`,
        // `len`) are typed, so checking `(Error, Bool)` directly would reject
        // `to_string(true)` -- a cascade into ordinary correct code, caught by
        // `tests/type_soundness.sh`'s `polymorphic_builtins_still_work` probe
        // before this shipped. Item 24 already covers the direct storage shape
        // (`let x: any = <bool>`) at `Stmt::Let`. Only CONTAINER positions are
        // unambiguous: nothing polymorphic is typed `[any]`/`map<_, any>`, so
        // a concrete `Bool`/`F64` landing inside one is always the item-40 bug.
        let nested = match (&p, &a) {
            (Type::Array { element: pe, .. }, Type::Array { element: ae, .. }) => {
                Self::untagged_scalar_into_any(pe, ae)
            }
            (Type::Set { element: pe }, Type::Set { element: ae }) => {
                Self::untagged_scalar_into_any(pe, ae)
            }
            (Type::Option { inner: pi }, Type::Option { inner: ai }) => {
                Self::untagged_scalar_into_any(pi, ai)
            }
            (Type::Tuple { elements: pv }, Type::Tuple { elements: av }) => pv
                .iter()
                .zip(av.iter())
                .find_map(|(x, y)| Self::untagged_scalar_into_any(x, y)),
            (Type::Map { key: pk, value: pv }, Type::Map { key: ak, value: av }) => {
                Self::untagged_scalar_into_any(pk, ak)
                    .or_else(|| Self::untagged_scalar_into_any(pv, av))
            }
            (Type::Result { ok: po, err: pe }, Type::Result { ok: ao, err: ae }) => {
                Self::untagged_scalar_into_any(po, ao)
                    .or_else(|| Self::untagged_scalar_into_any(pe, ae))
            }
            _ => None,
        };
        if let Some(bad) = nested {
            self.error_with_code(
                format!(
                    "cannot pass a `{bad}` value through an `any` slot -- `any` is erased to a bare i64 at runtime with no type tag, and `{bad}`'s native representation is not i64-compatible, so the value is reinterpreted rather than converted (measured: the same program prints a different wrong number on each backend, with no diagnostic). Use a concrete element type (e.g. `[{bad}]`) instead of `[any]`, or convert to a final form (e.g. `to_string(..)`) before passing it"
                ),
                span,
                kryos_errors::codes::E0110,
            );
        }
    }

    /// Walk a parameter type and an argument type in parallel, looking for a
    /// non-i64-shaped scalar on the ARGUMENT side sitting where the PARAMETER
    /// side is the `any` sentinel. Returns the offending scalar type.
    fn untagged_scalar_into_any(p: &Type, a: &Type) -> Option<Type> {
        match (p, a) {
            (Type::Error, Type::Bool) => Some(Type::Bool),
            (Type::Error, Type::F64) => Some(Type::F64),
            (Type::Error, Type::F32) => Some(Type::F32),
            (Type::Array { element: pe, .. }, Type::Array { element: ae, .. }) => {
                Self::untagged_scalar_into_any(pe, ae)
            }
            (Type::Set { element: pe }, Type::Set { element: ae }) => {
                Self::untagged_scalar_into_any(pe, ae)
            }
            (Type::Option { inner: pi }, Type::Option { inner: ai }) => {
                Self::untagged_scalar_into_any(pi, ai)
            }
            (Type::Tuple { elements: pv }, Type::Tuple { elements: av }) => pv
                .iter()
                .zip(av.iter())
                .find_map(|(x, y)| Self::untagged_scalar_into_any(x, y)),
            (
                Type::Map {
                    key: pk,
                    value: pv,
                },
                Type::Map {
                    key: ak,
                    value: av,
                },
            ) => Self::untagged_scalar_into_any(pk, ak)
                .or_else(|| Self::untagged_scalar_into_any(pv, av)),
            (
                Type::Result {
                    ok: po,
                    err: pe,
                },
                Type::Result {
                    ok: ao,
                    err: ae,
                },
            ) => Self::untagged_scalar_into_any(po, ao)
                .or_else(|| Self::untagged_scalar_into_any(pe, ae)),
            _ => None,
        }
    }

    /// `@pure` enforcement for a free-function / builtin callee identified by
    /// name (direct `f(..)` or module-qualified `m::f(..)`). Rejects I/O
    /// builtins and non-`@pure` user functions; a curated set of pure builtins
    /// is allowed. Method and static-method dispatch are handled separately at
    /// their call sites -- the concrete callee's purity cannot be verified
    /// (impl-method `@pure` is not tracked, and a name-based method check would
    /// be unsound across same-named methods on different types), so those are
    /// rejected outright to keep the `@pure` CSE/dead-call optimization sound.
    fn check_pure_free_call(&mut self, name: &str, span: Span) {
        if !self.in_pure_function {
            return;
        }
        let io_builtins = ["println", "print", "eprintln", "exit"];
        if io_builtins.contains(&name) {
            self.error(
                format!("`@pure` function cannot call I/O builtin `{name}`"),
                span,
            );
            return;
        }
        // An INDIRECT call through a fn-typed VALUE (`f()` where `f` is a
        // function parameter or a local holding a closure, not a named
        // function) has an unverifiable callee -- it could be impure. A @pure
        // function calling one silently ran the side effect (check passed, run
        // did the I/O, and the enclosing fn was still marked pure -> a CSE/dead-
        // call miscompile). Reject it, like method/static dispatch, to keep
        // @pure sound. (Kryos has no "pure fn" parameter type to verify against,
        // so the conservative rejection is the only sound choice.)
        if let Some(vty) = self.env.lookup_var(name).cloned() {
            if matches!(self.engine.resolve(&vty), Type::Function { .. }) {
                self.error(
                    format!(
                        "`@pure` function cannot call through the function value `{name}` -- the callee's purity cannot be verified"
                    ),
                    span,
                );
                return;
            }
        }
        if self.pure_functions.contains(name) {
            return;
        }
        // Builtins that are side-effect free may be called from a @pure fn.
        let side_effect_free_builtins = [
            "len", "range", "to_string", "typeof", "sizeof", "min", "max", "min_f", "max_f",
            "abs", "abs_f", "sqrt", "pow", "floor", "ceil", "round", "log", "log2", "log10", "sin",
            "cos", "tan", "push", "pop", "contains", "keys", "values", "split", "trim",
            "starts_with", "ends_with", "to_upper", "to_lower", "char_at", "substring", "parse_int",
            "parse_float",
        ];
        if self.env.lookup_function(name).is_some()
            && !side_effect_free_builtins.contains(&name)
        {
            self.error(
                format!("`@pure` function cannot call non-pure function `{name}`"),
                span,
            );
        }
    }

    // ── TypeExpr → Type resolution ───────────────────────────────────

    /// Resolve an AST TypeExpr to a concrete Type.
    /// Reject `dyn Trait` in a CONTAINER position (array/tuple element,
    /// Option/Result/map/user-generic argument). Trait objects in containers
    /// are unimplemented at the codegen level and used to SEGFAULT or hang at
    /// runtime on both backends -- surface a clean compile error until
    /// fat-pointer container storage lands. Single `dyn Trait` positions
    /// (params, returns, lets, struct fields) remain fully supported.
    fn reject_dyn_in_container<'a>(
        &mut self,
        container: &str,
        args: impl Iterator<Item = &'a TypeExpr>,
    ) -> bool {
        for a in args {
            if let TypeExpr::DynTrait { trait_name, span } = a {
                self.error_with_code(
                    format!(
                        "`dyn {trait_name}` cannot be stored in {container} yet -- trait objects in containers are unimplemented; use an enum with one variant per concrete type and `match`"
                    ),
                    *span,
                    kryos_errors::codes::E0110,
                );
                return true;
            }
            // An ACTOR value stored in an array/tuple/map loses its logical
            // actor type through the handle erasure (actors are opaque i64
            // handles), so a later `container[i].handler(...)` is lowered as an
            // ordinary unmangled call and fails to LINK (unresolved external
            // symbol = bare handler name) even though `check` passed. The
            // compiler only recovers an actor's type through a struct FIELD or a
            // function RETURN, not an array element / map value. Reject up front
            // with a clear diagnostic instead of the cryptic linker error.
            if let TypeExpr::Simple { name, span } = a {
                if self.actor_names.contains(name) {
                    self.error_with_code(
                        format!(
                            "actor `{name}` cannot be stored in {container} yet -- an actor in an array/map/tuple loses its type through the handle erasure, so a method call on it fails to link; hold it in a struct field or a plain local instead"
                        ),
                        *span,
                        kryos_errors::codes::E0110,
                    );
                    return true;
                }
            }
        }
        false
    }

    pub fn resolve_type_expr(&mut self, te: &TypeExpr) -> Type {
        match te {
            TypeExpr::Simple { name, span } => {
                // Resolve `Self` to the current impl/trait target type.
                if name == "Self" {
                    return if let Some(ref self_ty) = self.current_self_type {
                        self_ty.clone()
                    } else {
                        self.error_with_code(
                            "`Self` used outside of impl or trait block",
                            *span,
                            kryos_errors::codes::E0109,
                        );
                        Type::Error
                    };
                }
                if name == "Map" || name == "map" {
                    // Bare `map`/`Map`: create a map with fresh type variables
                    Type::Map {
                        key: Box::new(self.engine.fresh_var()),
                        value: Box::new(self.engine.fresh_var()),
                    }
                } else if name == "f32" {
                    // f32 works as a SCALAR: `let a: f32 = 3.14` (literal
                    // inference), arithmetic, and `as f32` casts -- those two
                    // positions resolve f32 directly WITHOUT reaching here
                    // (see Stmt::Let and Expr::Cast). Every OTHER annotation
                    // position (struct fields, fn params/returns, array/tuple
                    // elements, generic args) is NOT wired through either
                    // code generator: both backends ICE on an f32 struct
                    // field store or an f32 call argument (invalid cast
                    // float->i64 on AOT, verifier error on JIT). Reject with
                    // a clear diagnostic instead of crashing; mirrors the
                    // i128/u128 arm below. Full f32 layout support is
                    // backlogged.
                    self.error_with_code(
                        format!(
                            "`f32` is currently supported only for scalar locals, arithmetic, and `as f32` casts -- use `f64` in this position (f32 struct fields, params, returns, and collection elements are not yet wired through the code generators)"
                        ),
                        *span,
                        kryos_errors::codes::E0110,
                    );
                    Type::Error
                } else if name == "i128" || name == "u128" {
                    // 128-bit integers are declared in the type system but the
                    // code generators do not implement them: the Cranelift JIT
                    // hits a verifier ICE (`entered unreachable code`) and the
                    // LLVM AOT backend fails (`i128 but expected ptr`) on even a
                    // trivial `let a: i128 = 100`. Reject at type-check with a
                    // clear diagnostic instead of crashing the compiler; nothing
                    // in the stdlib/tests uses them (full 128-bit codegen is
                    // backlogged).
                    self.error_with_code(
                        format!(
                            "`{name}` is not yet supported by the code generator -- use `i64`/`u64`"
                        ),
                        *span,
                        kryos_errors::codes::E0110,
                    );
                    Type::Error
                } else if let Some(ty) = Type::from_name(name) {
                    ty
                } else {
                    // Check if it's a known struct or enum name.
                    if self.env.lookup_struct(name).is_some() {
                        Type::Struct {
                            name: name.clone(),
                            generics: vec![],
                        }
                    } else if self.env.lookup_enum(name).is_some() {
                        Type::Enum {
                            name: name.clone(),
                            generics: vec![],
                        }
                    } else if let Some(ty) = self.env.lookup_var(name) {
                        // Type alias or generic parameter registered as a variable.
                        ty.clone()
                    } else {
                        self.error_with_code(
                            format!("unknown type `{name}`"),
                            *span,
                            kryos_errors::codes::E0101,
                        );
                        let known = self.env.all_type_names();
                        if let Some(suggestion) =
                            crate::suggest::closest_match(name, known.iter().map(|s| s.as_str()))
                        {
                            if let Some(diag) = self.diagnostics.last_mut() {
                                diag.notes.push(format!("did you mean `{suggestion}`?"));
                            }
                        }
                        Type::Error
                    }
                }
            }
            TypeExpr::Generic { name, args, span } => {
                if self.reject_dyn_in_container(&format!("`{name}<..>`"), args.iter()) {
                    return Type::Error;
                }
                // A rejected GENERIC type alias is registered as `Type::Error`
                // at its definition (which already reported the clear
                // not-yet-supported diagnostic). Resolve its uses to `Error` too
                // so they do not cascade into phantom-struct / enum-vs-struct
                // errors.
                if matches!(self.env.lookup_var(name), Some(Type::Error)) {
                    return Type::Error;
                }
                let resolved_args: Vec<Type> =
                    args.iter().map(|a| self.resolve_type_expr(a)).collect();

                // Handle well-known generic types.
                match name.as_str() {
                    "Option" => {
                        if resolved_args.len() == 1 {
                            Type::Option {
                                inner: Box::new(resolved_args[0].clone()),
                            }
                        } else {
                            self.error("Option expects exactly 1 type argument", *span);
                            Type::Error
                        }
                    }
                    "Result" => {
                        if resolved_args.len() == 2 {
                            Type::Result {
                                ok: Box::new(resolved_args[0].clone()),
                                err: Box::new(resolved_args[1].clone()),
                            }
                        } else {
                            self.error("Result expects exactly 2 type arguments", *span);
                            Type::Error
                        }
                    }
                    // The builtin `Map`/`Set` type sugar only applies when no
                    // user-defined struct of that name is in scope. This lets
                    // std::collections ship real generic `Set<T>` / `Map<..>`
                    // structs that shadow the builtin sugar (the `_` arm below
                    // resolves them as ordinary user structs). Bare `map<K,V>`
                    // literals keep working because nobody defines a struct
                    // named `map`.
                    "Map" | "map" if self.env.lookup_struct(name).is_none() => {
                        if resolved_args.len() == 2 {
                            Type::Map {
                                key: Box::new(resolved_args[0].clone()),
                                value: Box::new(resolved_args[1].clone()),
                            }
                        } else {
                            self.error("map expects exactly 2 type arguments", *span);
                            Type::Error
                        }
                    }
                    // chan<T> - channels are opaque i64 handles at runtime.
                    "chan" => Type::I64,
                    "Set" | "set" if self.env.lookup_struct(name).is_none() => {
                        if resolved_args.len() == 1 {
                            Type::Set {
                                element: Box::new(resolved_args[0].clone()),
                            }
                        } else {
                            self.error("set expects exactly 1 type argument", *span);
                            Type::Error
                        }
                    }
                    _ => {
                        // User-defined generic struct or enum.
                        if self.env.lookup_struct(name).is_some() {
                            Type::Struct {
                                name: name.clone(),
                                generics: resolved_args,
                            }
                        } else if self.env.lookup_enum(name).is_some() {
                            Type::Enum {
                                name: name.clone(),
                                generics: resolved_args,
                            }
                        } else {
                            // Treat as struct by default (forward reference).
                            Type::Struct {
                                name: name.clone(),
                                generics: resolved_args,
                            }
                        }
                    }
                }
            }
            TypeExpr::Array {
                element,
                size,
                span: _,
            } => {
                if self.reject_dyn_in_container("an array", std::iter::once(element.as_ref())) {
                    return Type::Error;
                }
                Type::Array {
                    element: Box::new(self.resolve_type_expr(element)),
                    size: *size,
                }
            }
            TypeExpr::Tuple { elements, span: _ } => {
                if self.reject_dyn_in_container("a tuple", elements.iter()) {
                    return Type::Error;
                }
                Type::Tuple {
                    elements: elements.iter().map(|e| self.resolve_type_expr(e)).collect(),
                }
            }
            TypeExpr::Function {
                params,
                ret,
                span: _,
            } => {
                // No `@{...}` surface syntax yet (Stage 1: representation +
                // inference only) -- every fn-typed position is therefore
                // "unannotated" by construction, and gets a fresh
                // capability-row variable per spec §2.3/§2.4. Collected
                // into `pending_cap_var_ids` so the caller resolving a
                // DECLARATION's own signature (register_decl) can drain it
                // into that declaration's `generic_cap_var_ids`; harmless
                // (simply unused) for any other resolution context (a
                // struct field, a `let` annotation, ...).
                let cap_var = self.engine.fresh_cap_var();
                self.pending_cap_var_ids.push(cap_var);
                Type::Function {
                    params: params.iter().map(|p| self.resolve_type_expr(p)).collect(),
                    ret: Box::new(self.resolve_type_expr(ret)),
                    caps: crate::ty::CapRow::var(cap_var),
                }
            }
            TypeExpr::Optional { inner, span: _ } => Type::Option {
                inner: Box::new(self.resolve_type_expr(inner)),
            },
            TypeExpr::Reference {
                inner,
                mutable,
                span: _,
            } => Type::Reference {
                inner: Box::new(self.resolve_type_expr(inner)),
                mutable: *mutable,
            },
            TypeExpr::Shared { inner, span: _ } => Type::Shared {
                inner: Box::new(self.resolve_type_expr(inner)),
            },
            TypeExpr::Weak { inner, span: _ } => Type::Weak {
                inner: Box::new(self.resolve_type_expr(inner)),
            },
            TypeExpr::Pointer {
                inner,
                mutable,
                span: _,
            } => Type::Pointer {
                inner: Box::new(self.resolve_type_expr(inner)),
                mutable: *mutable,
            },
            TypeExpr::DynTrait { trait_name, span } => {
                // Verify the trait exists.
                if self.env.lookup_trait(trait_name).is_none() {
                    self.error_with_code(
                        format!("unknown trait `{trait_name}`"),
                        *span,
                        kryos_errors::codes::E0105,
                    );
                    return Type::Error;
                }
                Type::DynTrait {
                    trait_name: trait_name.clone(),
                }
            }
            TypeExpr::Inferred { span: _ } => {
                // Create a fresh type variable to be inferred.
                self.engine.fresh_var()
            }
        }
    }

    // ── Declaration checking ─────────────────────────────────────────

    /// Type-check an entire module.
    pub fn check_module(&mut self, module: &Module) {
        // An ACTOR name colliding with a struct/enum of the same name is a
        // hard error: an actor registers itself in the struct table (so
        // `self.field` resolves in handlers), so a same-named struct/enum
        // silently OVERWRITES the actor's field table, producing incoherent
        // "no field X" errors pointing INTO the actor's own valid body. (A
        // struct+enum same-name pair is fine -- disambiguated by `Name{}` vs
        // `Name.Variant` syntax -- so only the actor collision is rejected.)
        {
            let mut actor_names = std::collections::HashSet::new();
            let mut structy_names = std::collections::HashSet::new();
            for decl in &module.declarations {
                match decl {
                    Decl::Actor { name, .. } => {
                        actor_names.insert(name.clone());
                    }
                    Decl::Struct { name, .. } | Decl::Enum { name, .. } => {
                        structy_names.insert(name.clone());
                    }
                    _ => {}
                }
            }
            // A struct stores its fields INLINE, so a cycle of plain
            // struct-typed fields describes a value of infinite size. Nothing
            // rejected it: `struct A { b: B }  struct B { a: A }` type-checked
            // clean and then made the COMPILER recurse forever computing the
            // layout, dying with "stack overflow (unbounded recursion?)" --
            // a crash on a plausible modelling mistake, with no diagnostic
            // pointing at the cause. Report the cycle instead.
            //
            // Only DIRECT embedding counts. `[S]`, `map<K, S>` and `Option<S>`
            // are handles, so a cycle through one of them is finite and stays
            // legal (self-referential trees and linked lists keep working).
            {
                use std::collections::HashMap;
                fn direct_fields(t: &kryos_ast::TypeExpr, out: &mut Vec<String>) {
                    match t {
                        kryos_ast::TypeExpr::Simple { name, .. } => out.push(name.clone()),
                        // A tuple is stored inline too, so it propagates the cycle.
                        kryos_ast::TypeExpr::Tuple { elements, .. } => {
                            for e in elements {
                                direct_fields(e, out);
                            }
                        }
                        _ => {}
                    }
                }
                let mut edges: HashMap<String, Vec<String>> = HashMap::new();
                let mut spans: HashMap<String, kryos_errors::Span> = HashMap::new();
                for decl in &module.declarations {
                    if let Decl::Struct { name, fields, span, .. } = decl {
                        let mut outs = Vec::new();
                        for f in fields {
                            direct_fields(&f.ty, &mut outs);
                        }
                        edges.insert(name.clone(), outs);
                        spans.insert(name.clone(), *span);
                    }
                }
                // Report each cycle once, at the declaration that closes it.
                let mut reported: std::collections::HashSet<String> = Default::default();
                for start in edges.keys() {
                    if reported.contains(start) {
                        continue;
                    }
                    // Depth-first walk back to `start`; `path` records the route.
                    let mut stack = vec![(start.clone(), vec![start.clone()])];
                    let mut seen: std::collections::HashSet<String> = Default::default();
                    while let Some((node, path)) = stack.pop() {
                        for next in edges.get(&node).into_iter().flatten() {
                            if next == start {
                                let mut route = path.clone();
                                route.push(next.clone());
                                for n in &route {
                                    reported.insert(n.clone());
                                }
                                let Some(span) = spans.get(start).copied() else {
                                    continue;
                                };
                                self.error(
                                    format!(
                                        "recursive struct `{start}` has infinite size: {} -- a struct stores its fields inline, so this cycle can never be laid out; put one field behind an indirection (`[{next}]`, `map<str, {next}>`, or `Option<{next}>`)",
                                        route.join(" -> ")
                                    ),
                                    span,
                                );
                                stack.clear();
                                break;
                            }
                            if edges.contains_key(next) && seen.insert(next.clone()) {
                                let mut route = path.clone();
                                route.push(next.clone());
                                stack.push((next.clone(), route));
                            }
                        }
                    }
                }
            }
            for decl in &module.declarations {
                if let Decl::Actor { name, span, .. } = decl {
                    if structy_names.contains(name) {
                        self.error(
                            format!(
                                "actor `{name}` collides with a struct or enum of the same name -- rename one; an actor and a struct/enum cannot share a name"
                            ),
                            *span,
                        );
                    }
                }
                let _ = &actor_names;
            }
        }
        // Top-level const dependency CYCLES (`A = B + 1; B = A + 1`, or routed
        // THROUGH a function `A = f(); fn f() { return A + 1 }`). The const
        // evaluator inlines each immutable const's initializer at its use
        // sites, so a cycle recursed to a runtime STACK OVERFLOW (exit 253).
        // (A direct self-reference `X = X + 1` is caught per-const below with a
        // targeted message; this pass covers mutual, longer, and
        // function-indirected cycles.) The graph is INTERPROCEDURAL: a const's
        // dependencies include every const reachable through the functions its
        // initializer calls (transitively), so indirection through a helper is
        // visible. Reported at check time.
        {
            let consts: Vec<(&str, &Expr, Span)> = module
                .declarations
                .iter()
                .filter_map(|d| match d {
                    Decl::Const {
                        name, value, span, ..
                    } => Some((name.as_str(), value.as_ref(), *span)),
                    _ => None,
                })
                .collect();
            let const_set: std::collections::HashSet<&str> =
                consts.iter().map(|(n, _, _)| *n).collect();
            // Every function's body-referenced names.
            let fns: std::collections::HashMap<&str, std::collections::HashSet<String>> = module
                .declarations
                .iter()
                .filter_map(|d| match d {
                    Decl::Function {
                        name,
                        body: Some(b),
                        ..
                    } => {
                        let mut names = std::collections::HashSet::new();
                        collect_names_block(b, &mut names);
                        Some((name.as_str(), names))
                    }
                    _ => None,
                })
                .collect();
            // Fixpoint: fn_consts[f] = consts f references directly + consts
            // reachable through the functions f calls (transitively).
            let mut fn_consts: std::collections::HashMap<&str, std::collections::HashSet<String>> =
                fns.iter()
                    .map(|(f, names)| {
                        (
                            *f,
                            names
                                .iter()
                                .filter(|n| const_set.contains(n.as_str()))
                                .cloned()
                                .collect(),
                        )
                    })
                    .collect();
            loop {
                let mut changed = false;
                for (f, names) in &fns {
                    let mut add: Vec<String> = Vec::new();
                    for called in names {
                        if let Some(cc) = fn_consts.get(called.as_str()) {
                            for c in cc {
                                if !fn_consts[f].contains(c) {
                                    add.push(c.clone());
                                }
                            }
                        }
                    }
                    if !add.is_empty() {
                        changed = true;
                        let e = fn_consts.get_mut(f).unwrap();
                        for c in add {
                            e.insert(c);
                        }
                    }
                }
                if !changed {
                    break;
                }
            }
            // Per const: names its initializer references, expanded through any
            // functions it calls.
            let idx_of: std::collections::HashMap<&str, usize> =
                consts.iter().enumerate().map(|(i, (n, _, _))| (*n, i)).collect();
            let deps: Vec<Vec<usize>> = consts
                .iter()
                .enumerate()
                .map(|(i, (_, val, _))| {
                    let mut refs = std::collections::HashSet::new();
                    collect_names_expr(val, &mut refs);
                    let mut dep_consts: std::collections::HashSet<&str> = std::collections::HashSet::new();
                    for r in &refs {
                        if const_set.contains(r.as_str()) {
                            dep_consts.insert(r.as_str());
                        }
                        if let Some(cc) = fn_consts.get(r.as_str()) {
                            for c in cc {
                                dep_consts.insert(c.as_str());
                            }
                        }
                    }
                    // Self-edges are KEPT: a const that depends on itself
                    // through a function (`let A = helper()` where helper reads
                    // A) is a real cycle with no other node, and the direct
                    // `A = A + 1` self-edge is a cycle too. Both are reported
                    // here (the per-const direct-self check was removed as
                    // redundant with this).
                    let _ = i;
                    dep_consts
                        .iter()
                        .filter_map(|c| idx_of.get(c).copied())
                        .collect()
                })
                .collect();
            // DFS cycle detection (0=unvisited, 1=in-stack, 2=done). A
            // self-edge (v == u) is detected because state[u] is already 1.
            let n = consts.len();
            let mut state = vec![0u8; n];
            fn dfs(u: usize, deps: &[Vec<usize>], state: &mut [u8]) -> bool {
                state[u] = 1;
                for &v in &deps[u] {
                    if state[v] == 1 {
                        return true;
                    }
                    if state[v] == 0 && dfs(v, deps, state) {
                        return true;
                    }
                }
                state[u] = 2;
                false
            }
            let mut reported = false;
            for i in 0..n {
                if state[i] == 0 && dfs(i, &deps, &mut state) && !reported {
                    reported = true;
                    self.error_with_code(
                        format!(
                            "circular dependency among top-level consts (including `{}`) -- their values depend on each other (directly or through a called function) and cannot be resolved",
                            consts[i].0
                        ),
                        consts[i].2,
                        kryos_errors::codes::E0102,
                    );
                }
            }
        }
        // Pass 0: pre-register all struct/enum NAMES so self-referential
        // types (e.g. `struct Expr { left: [Expr] }`) can resolve.
        for decl in &module.declarations {
            self.pre_register_type_name(decl);
        }
        // First pass: register all type and function declarations.
        for decl in &module.declarations {
            self.register_decl(decl);
            // Track annotated functions.
            if let Decl::Function {
                name, annotations, ..
            } = decl
            {
                for ann in annotations {
                    match ann.name.as_str() {
                        "deprecated" => {
                            self.deprecated_functions.insert(name.clone());
                        }
                        "pure" => {
                            self.pure_functions.insert(name.clone());
                        }
                        _ => {}
                    }
                }
            }
        }
        // Pre-pass: bind every actor handler's OWN capability row before any
        // body that may call it is checked.
        //
        // A handler's row is only known once its body has been walked, so a
        // caller declared EARLIER in the module (`actor Sender` before `actor
        // Receiver`) walks its own body while the callee's `own_cap_var` is
        // still unbound. The dispatch site then snapshots that bare var and
        // applies the callee's instantiation map to it -- but the map is keyed
        // on the callee's `generic_cap_var_ids`, and a bare unbound var is not
        // one of them, so the remap is a no-op and the row never gets expressed
        // in terms of anything the CALLER can bind. The chain terminates on a
        // var no call site will ever touch, and the call costs zero authority.
        // This is LEDGER item 33, and it is SILENT: enforcement runs, resolves
        // an open row, and passes.
        //
        // Measured: the identical program with `Receiver` declared FIRST is
        // correctly rejected -- only declaration order differed.
        //
        // Safe to run twice because `bind_cap_var` UNIONS rather than
        // overwrites (rows only widen), and diagnostics from this pass are
        // discarded so the real pass below is the only one that reports.
        let diag_mark = self.diagnostics.len();
        for decl in &module.declarations {
            if matches!(decl, Decl::Actor { .. }) {
                self.check_decl(decl);
            }
        }
        self.diagnostics.truncate(diag_mark);

        // Second pass: check function bodies and expressions.
        for decl in &module.declarations {
            self.check_decl(decl);
        }
    }

    /// Pre-register struct/enum names with empty fields so self-referential
    /// types resolve during the full registration pass.
    fn pre_register_type_name(&mut self, decl: &Decl) {
        match decl {
            Decl::Struct { name, generics, .. } => {
                self.env.define_struct(StructDef {
                    name: name.clone(),
                    generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                    generic_var_ids: vec![],
                    fields: vec![],
                });
            }
            Decl::Enum { name, generics, .. } => {
                self.env.define_enum(EnumDef {
                    name: name.clone(),
                    generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                    generic_var_ids: vec![],
                    variants: vec![],
                });
            }
            // An actor is, to the type system, a struct-like handle: its name is
            // a type and a zero-arg constructor (`Counter()` spawns it). Register
            // the name here so it resolves during the full pass.
            Decl::Actor { name, .. } => {
                self.actor_names.insert(name.clone());
                self.env.define_struct(StructDef {
                    name: name.clone(),
                    generic_params: vec![],
                    generic_var_ids: vec![],
                    fields: vec![],
                });
            }
            _ => {}
        }
    }

    /// Register a declaration in the environment (forward declaration pass).
    fn register_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Function {
                name,
                generics,
                params,
                ret_ty,
                body,
                span,
                ..
            } => {
                // Duplicate top-level function: two DEFINED user fns with the
                // same name, or a defined local fn colliding with an IMPORTED
                // one -- previously passed `kryos check` silently and died in
                // codegen with a raw internal "Duplicate definition of
                // identifier" dump. Only a BODIED definition reserves the
                // name: a bodyless forward declaration (`fn f(..) -> T` with no
                // block, used for mutual recursion / prototypes) followed by
                // its definition is legal and must NOT be flagged. (Import-vs-
                // import collisions are already caught by the resolver.)
                if body.is_some() && !self.seen_fn_names.insert(name.clone()) {
                    self.error(
                        format!(
                            "duplicate definition of function `{name}` -- a function with this name is already defined (or imported) in this program"
                        ),
                        *span,
                    );
                }
                self.check_duplicate_params(params, &format!("function `{name}`"));
                self.check_duplicate_generics(generics, &format!("function `{name}`"), *span);
                // Temporarily bind generic params so they resolve in param/return types.
                // Capture the type variable IDs so we can instantiate fresh copies
                // at each call site (prevents generic pinning bug).
                // Also register trait bounds keyed by the *sig* var IDs - these
                // are the IDs that show up in parameter types when the function
                // body is checked, so MethodCall bound-resolution must see them.
                let mut generic_var_ids = Vec::new();
                if !generics.is_empty() {
                    self.env.push_scope();
                    for gp in generics {
                        let tv = self.engine.fresh_var();
                        if let Type::Var(id) = &tv {
                            generic_var_ids.push(*id);
                            if !gp.bounds.is_empty() {
                                self.generic_var_bounds
                                    .insert(*id, gp.bounds.clone());
                            }
                        }
                        self.env.define_var(gp.name.clone(), tv);
                    }
                }

                // Capability-row inference (Stage 1): every unannotated
                // fn-typed param/return position resolved below gets a
                // fresh row var (`resolve_type_expr`'s `TypeExpr::Function`
                // arm), collected into `pending_cap_var_ids`. Clear first
                // so only THIS declaration's own positions are drained.
                self.pending_cap_var_ids.clear();
                let param_types: Vec<(String, Type)> = params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let ty =
                            p.ty.as_ref()
                                .map(|t| self.resolve_type_expr(t))
                                .unwrap_or_else(|| self.engine.fresh_var());
                        // Record which params were rejected specifically as a
                        // dyn-in-container annotation (see
                        // `dyn_container_reject_params`) so a call-site array
                        // literal passed for this exact param can skip its
                        // own pairwise element-unify without touching the
                        // general Type::Error unify-anything path.
                        if ty == Type::Error
                            && matches!(
                                p.ty.as_ref(),
                                Some(TypeExpr::Array { element, .. })
                                    if matches!(element.as_ref(), TypeExpr::DynTrait { .. })
                            )
                        {
                            self.dyn_container_reject_params
                                .insert((name.clone(), i));
                        }
                        (p.name.clone(), ty)
                    })
                    .collect();

                let ret = ret_ty
                    .as_ref()
                    .map(|t| self.resolve_type_expr(t))
                    .unwrap_or(Type::Void);

                if !generics.is_empty() {
                    self.env.pop_scope();
                }

                let generic_cap_var_ids = std::mem::take(&mut self.pending_cap_var_ids);
                // This declaration's OWN total capability requirement --
                // bound once the body is actually walked (`check_decl`);
                // see `FunctionSig::own_cap_var`'s doc comment.
                let own_cap_var = self.engine.fresh_cap_var();
                let sig = FunctionSig {
                    name: name.clone(),
                    generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                    generic_var_ids,
                    generic_cap_var_ids,
                    own_cap_var,
                    params: param_types,
                    ret,
                };
                self.env.define_function(sig);
            }
            Decl::Struct {
                name,
                generics,
                fields,
                span,
                ..
            } => {
                // Duplicate field names in the DECLARATION. Fields are stored
                // positionally in a Vec with no uniqueness check, so which of
                // two same-named fields a literal/access resolved to was
                // implementation-defined -- silent wrong data. Mirrors the
                // duplicate-function check above.
                {
                    let mut seen_fields = std::collections::HashSet::new();
                    for f in fields {
                        if !seen_fields.insert(f.name.as_str()) {
                            self.error(
                                format!(
                                    "duplicate field `{}` in struct `{name}` -- each field name may appear only once",
                                    f.name
                                ),
                                *span,
                            );
                        }
                    }
                }
                self.check_duplicate_generics(generics, &format!("struct `{name}`"), *span);
                // Bind generic params so they resolve in field types.
                let mut generic_var_ids = Vec::new();
                if !generics.is_empty() {
                    self.env.push_scope();
                    for gp in generics {
                        let tv = self.engine.fresh_var();
                        if let Type::Var(id) = &tv {
                            generic_var_ids.push(*id);
                        }
                        self.env.define_var(gp.name.clone(), tv);
                    }
                }
                let field_types: Vec<(String, Type)> = fields
                    .iter()
                    .map(|f| (f.name.clone(), self.resolve_type_expr(&f.ty)))
                    .collect();
                if !generics.is_empty() {
                    self.env.pop_scope();
                }
                self.env.define_struct(StructDef {
                    name: name.clone(),
                    generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                    generic_var_ids,
                    fields: field_types,
                });
            }
            Decl::Enum {
                name,
                generics,
                variants,
                span,
                ..
            } => {
                // Duplicate variant names. Variants are tag-indexed by
                // POSITION, so two same-named variants gave the constructor
                // and every match arm an ambiguous tag -- which one won was
                // implementation-defined (silent wrong dispatch).
                {
                    let mut seen_variants = std::collections::HashSet::new();
                    for v in variants {
                        if !seen_variants.insert(v.name.as_str()) {
                            self.error(
                                format!(
                                    "duplicate variant `{}` in enum `{name}` -- each variant name may appear only once",
                                    v.name
                                ),
                                *span,
                            );
                        }
                    }
                }
                self.check_duplicate_generics(generics, &format!("enum `{name}`"), *span);
                // Bind generic params so they resolve in variant field types.
                let mut generic_var_ids = Vec::new();
                if !generics.is_empty() {
                    self.env.push_scope();
                    for gp in generics {
                        let tv = self.engine.fresh_var();
                        if let Type::Var(id) = &tv {
                            generic_var_ids.push(*id);
                        }
                        self.env.define_var(gp.name.clone(), tv);
                    }
                }
                let variant_types: Vec<(String, Vec<Type>)> = variants
                    .iter()
                    .map(|v| {
                        let tys = v.fields.iter().map(|t| self.resolve_type_expr(t)).collect();
                        (v.name.clone(), tys)
                    })
                    .collect();
                if !generics.is_empty() {
                    self.env.pop_scope();
                }
                self.env.define_enum(EnumDef {
                    name: name.clone(),
                    generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                    generic_var_ids,
                    variants: variant_types,
                });
            }
            Decl::Impl {
                target,
                trait_name,
                methods,
                generics: impl_generics,
                span: impl_span,
                ..
            } => {
                // Set Self type for the duration of this impl block registration.
                let prev_self = self.current_self_type.take();

                self.check_duplicate_generics(
                    impl_generics,
                    &format!("impl `{target}`"),
                    *impl_span,
                );

                // Detect a method-name collision with a method already registered
                // on this type from ANOTHER impl block (two same-named methods --
                // from two traits, or a trait impl + an inherent impl -- mangle to
                // the same symbol `Type__method` and fail at codegen with an
                // internal DuplicateDefinition). Report it cleanly at check time.
                // (No prior impl's methods for THIS block are registered yet, so
                // lookup_method only sees earlier impls -- the same-BLOCK set
                // below covers sibling duplicates, which previously escaped to
                // the same internal codegen dump.)
                let mut block_methods = std::collections::HashSet::new();
                for m in methods {
                    if let Decl::Function {
                        name: mname,
                        params: mparams,
                        generics: mgenerics,
                        span: mspan,
                        ..
                    } = m
                    {
                        if self.env.lookup_method(target, mname).is_some() {
                            self.error(
                                format!(
                                    "type `{target}` already has a method named `{mname}` from another impl block; a type cannot define the same method name twice (both mangle to the same symbol). Rename one, or merge the impls"
                                ),
                                *mspan,
                            );
                        }
                        if !block_methods.insert(mname.as_str()) {
                            self.error(
                                format!(
                                    "type `{target}` defines method `{mname}` twice in the same impl block -- both mangle to the same symbol; rename one"
                                ),
                                *mspan,
                            );
                        }
                        self.check_duplicate_params(mparams, &format!("method `{mname}`"));
                        self.check_duplicate_generics(
                            mgenerics,
                            &format!("method `{mname}`"),
                            *mspan,
                        );
                    }
                }

                // Bring the impl's own type parameters (`impl<T> Box<T>`) into
                // scope as fresh type vars so method signatures that mention
                // them (`self: Box<T>`, `-> T`) resolve instead of raising
                // E0101. Concrete impls (`impl Box<i64>`) have no generics, so
                // this is a no-op for them.
                let scoped_impl_generics = !impl_generics.is_empty();
                // Capture the impl's generic var ids so each method signature
                // can record them. Without this the sigs carry
                // `generic_var_ids: vec![]` and are treated as non-generic:
                // `instantiate_sig` skips freshening, so the impl's return-type
                // var is SHARED across every call site. Calling `.get()` on
                // Box<str> then `let x: f64 = box_f64.get()` then failed with
                // "expected f64, found str" -- cross-instantiation
                // contamination through the shared var.
                let mut impl_generic_var_ids: Vec<u32> = Vec::new();
                if scoped_impl_generics {
                    self.env.push_scope();
                    for gp in impl_generics {
                        let tv = self.engine.fresh_var();
                        if let Type::Var(id) = &tv {
                            impl_generic_var_ids.push(*id);
                            if !gp.bounds.is_empty() {
                                self.generic_var_bounds.insert(*id, gp.bounds.clone());
                            }
                        }
                        self.env.define_var(gp.name.clone(), tv);
                    }
                }

                // Resolve the impl target type once for binding `self` params.
                let impl_target_ty = if self.env.lookup_struct(target).is_some() {
                    Some(Type::Struct {
                        name: target.clone(),
                        generics: vec![],
                    })
                } else if self.env.lookup_enum(target).is_some() {
                    Some(Type::Enum {
                        name: target.clone(),
                        generics: vec![],
                    })
                } else {
                    None
                };

                self.current_self_type = impl_target_ty.clone();

                let method_sigs: Vec<FunctionSig> = methods
                    .iter()
                    .filter_map(|m| {
                        if let Decl::Function {
                            name,
                            generics,
                            params,
                            ret_ty,
                            ..
                        } = m
                        {
                            self.pending_cap_var_ids.clear();
                            let param_types: Vec<(String, Type)> = params
                                .iter()
                                .map(|p| {
                                    let ty = if p.name == "self" && p.ty.is_none() {
                                        // `self` with no annotation gets the impl target type.
                                        impl_target_ty
                                            .clone()
                                            .unwrap_or_else(|| self.engine.fresh_var())
                                    } else {
                                        p.ty.as_ref()
                                            .map(|t| self.resolve_type_expr(t))
                                            .unwrap_or_else(|| self.engine.fresh_var())
                                    };
                                    (p.name.clone(), ty)
                                })
                                .collect();
                            let ret = ret_ty
                                .as_ref()
                                .map(|t| self.resolve_type_expr(t))
                                .unwrap_or(Type::Void);
                            // NOTE (Stage 1 scope): impl-method own_cap_var is
                            // allocated fresh but left UNBOUND here -- a
                            // method's body-accumulated row is not wired
                            // back into it in this stage (only top-level
                            // functions, lambdas, and actor handlers are).
                            // An unbound var resolves to itself (stays
                            // open) in the debug dump, never to a silently
                            // wrong empty set -- a disclosed limitation,
                            // not a soundness gap, since nothing enforces
                            // on this yet.
                            let method_generic_cap_var_ids =
                                std::mem::take(&mut self.pending_cap_var_ids);
                            let method_own_cap_var = self.engine.fresh_cap_var();
                            Some(FunctionSig {
                                name: name.clone(),
                                generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                                // Record the impl's generic vars so per-call
                                // instantiation freshens them (fixes the shared-
                                // var contamination). Vars the method does not
                                // mention are a no-op during instantiation.
                                generic_var_ids: impl_generic_var_ids.clone(),
                                generic_cap_var_ids: method_generic_cap_var_ids,
                                own_cap_var: method_own_cap_var,
                                params: param_types,
                                ret,
                            })
                        } else {
                            None
                        }
                    })
                    .collect();
                // Impl methods are NOT registered as global functions.
                // They live only in the `impls` map and are looked up via
                // `lookup_method`. The per-impl-method check site below
                // pushes a scope and defines the method signature there,
                // so `check_decl(Function)` can find the right param types
                // (including `self`) while checking the method body.
                //
                // This avoids polluting the global namespace: a stdlib impl
                // method named `push` no longer shadows the `push` builtin
                // for unrelated call sites - including call sites inside
                // sibling impl method bodies.
                let _ = &method_sigs;

                // Pop the impl-generic scope NOW: the method signatures are
                // fully resolved (their Var ids persist in the type engine),
                // and `define_impl` below registers into the *current* scope.
                // If we popped after define_impl, the registration would land
                // in this temporary scope and be discarded -- the method would
                // then be "not found" at every call site.
                if scoped_impl_generics {
                    self.env.pop_scope();
                }

                // If this is a trait impl, inherit default methods from the
                // trait that are not explicitly overridden in this impl block.
                let mut all_method_sigs = method_sigs.clone();
                if let Some(ref tname) = trait_name {
                    if let Some(trait_def) = self.env.lookup_trait(tname).cloned() {
                        let explicit_names: std::collections::HashSet<&str> =
                            method_sigs.iter().map(|s| s.name.as_str()).collect();
                        for trait_method in &trait_def.methods {
                            if !explicit_names.contains(trait_method.name.as_str()) {
                                // Rewrite the `self` parameter type to the concrete
                                // impl target type.
                                let rewritten_params: Vec<(String, Type)> = trait_method
                                    .params
                                    .iter()
                                    .map(|(pname, pty)| {
                                        if pname == "self" {
                                            (
                                                pname.clone(),
                                                impl_target_ty
                                                    .clone()
                                                    .unwrap_or_else(|| pty.clone()),
                                            )
                                        } else {
                                            (pname.clone(), pty.clone())
                                        }
                                    })
                                    .collect();
                                let default_sig = FunctionSig {
                                    name: trait_method.name.clone(),
                                    generic_params: trait_method.generic_params.clone(),
                                    generic_var_ids: trait_method.generic_var_ids.clone(),
                                    // Inherits the trait's own (unbound --
                                    // see the impl-method site above)
                                    // capability-row vars verbatim; a
                                    // default method's body is the TRAIT's
                                    // body, not re-walked per impl.
                                    generic_cap_var_ids: trait_method.generic_cap_var_ids.clone(),
                                    own_cap_var: trait_method.own_cap_var,
                                    params: rewritten_params,
                                    ret: trait_method.ret.clone(),
                                };
                                // Inherited trait defaults: also kept out of
                                // the global function table; available via
                                // method/static-method lookup only.
                                all_method_sigs.push(default_sig);
                            }
                        }
                    }
                }

                self.env
                    .define_impl(target.clone(), trait_name.clone(), all_method_sigs);
                // Record the (type, trait) pair for `dyn Trait` coercion
                // verification in unify (backlog #18/#34).
                if let Some(ref tname) = trait_name {
                    self.engine
                        .register_trait_impl(target.clone(), tname.clone());
                }

                // (impl-generic scope already popped above, before define_impl)
                self.current_self_type = prev_self;
            }
            Decl::Trait {
                name,
                generics,
                methods,
                span: trait_span,
                ..
            } => {
                // Duplicate method declarations in one trait -- the second
                // silently shadowed the first in the method table.
                {
                    let mut trait_methods = std::collections::HashSet::new();
                    for m in methods {
                        if let Decl::Function {
                            name: mname,
                            params: mparams,
                            generics: mgenerics,
                            span: mspan,
                            ..
                        } = m
                        {
                            if !trait_methods.insert(mname.as_str()) {
                                self.error(
                                    format!(
                                        "trait `{name}` declares method `{mname}` twice -- each method name may appear only once"
                                    ),
                                    *mspan,
                                );
                            }
                            self.check_duplicate_params(mparams, &format!("method `{mname}`"));
                            self.check_duplicate_generics(
                                mgenerics,
                                &format!("method `{mname}`"),
                                *mspan,
                            );
                        }
                    }
                }
                self.check_duplicate_generics(generics, &format!("trait `{name}`"), *trait_span);
                // Set Self to DynTrait so `self: Self` in trait method
                // signatures resolves correctly.
                let prev_self = self.current_self_type.take();
                self.current_self_type = Some(Type::DynTrait {
                    trait_name: name.clone(),
                });

                // Pre-register the trait NAME (empty methods) before resolving
                // its method signatures, so a method mentioning `dyn <this
                // trait>` -- e.g. a default method comparing two values of the
                // trait, `fn bigger(self, o: dyn Areo) -> bool` -- resolves
                // instead of a false E0105 unknown-trait. The complete def
                // (with methods) overwrites this stub below.
                self.env.define_trait(crate::env::TraitDef {
                    name: name.clone(),
                    generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                    methods: vec![],
                });

                // Bring the trait's own declared type parameters
                // (`trait Foo<T>`) into scope as fresh type vars so method
                // signatures that mention them (`fn convert(self: Self) -> T`)
                // resolve instead of raising E0101 -- mirrors the same
                // push_scope/define_var/pop_scope pattern already used for
                // Decl::Struct, Decl::Enum, Decl::Function, and the impl's own
                // generics in Decl::Impl above. Previously this scope was
                // never pushed, so a generic trait's `T` resolved as an
                // unknown concrete type name.
                let scoped_trait_generics = !generics.is_empty();
                if scoped_trait_generics {
                    self.env.push_scope();
                    for gp in generics {
                        let tv = self.engine.fresh_var();
                        self.env.define_var(gp.name.clone(), tv);
                    }
                }

                let method_sigs: Vec<FunctionSig> = methods
                    .iter()
                    .filter_map(|m| {
                        if let Decl::Function {
                            name,
                            generics,
                            params,
                            ret_ty,
                            ..
                        } = m
                        {
                            self.pending_cap_var_ids.clear();
                            let param_types: Vec<(String, Type)> = params
                                .iter()
                                .map(|p| {
                                    let ty =
                                        p.ty.as_ref()
                                            .map(|t| self.resolve_type_expr(t))
                                            .unwrap_or_else(|| self.engine.fresh_var());
                                    (p.name.clone(), ty)
                                })
                                .collect();
                            let ret = ret_ty
                                .as_ref()
                                .map(|t| self.resolve_type_expr(t))
                                .unwrap_or(Type::Void);
                            let trait_method_generic_cap_var_ids =
                                std::mem::take(&mut self.pending_cap_var_ids);
                            Some(FunctionSig {
                                name: name.clone(),
                                generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                                generic_var_ids: vec![],
                                generic_cap_var_ids: trait_method_generic_cap_var_ids,
                                // A trait method DECLARATION (no body here --
                                // bodies live in each `impl`) has no
                                // meaningful own-row to bind; fresh + unbound.
                                own_cap_var: self.engine.fresh_cap_var(),
                                params: param_types,
                                ret,
                            })
                        } else {
                            None
                        }
                    })
                    .collect();

                if scoped_trait_generics {
                    self.env.pop_scope();
                }

                self.env.define_trait(crate::env::TraitDef {
                    name: name.clone(),
                    generic_params: generics.iter().map(|g| g.name.clone()).collect(),
                    methods: method_sigs,
                });

                self.current_self_type = prev_self;
            }
            Decl::TypeAlias {
                name,
                generics,
                ty,
                span,
                ..
            } => {
                if !generics.is_empty() {
                    // A GENERIC type alias (`type Pair<T> = Result<T, str>`)
                    // parses, but is not yet fully lowered: expanding it per use
                    // works for a concrete instantiation but leaks an unsized
                    // `%T` into the LLVM IR when the alias is used inside a
                    // generic function (`fn f<T>() -> Pair<T>`), an AOT build
                    // failure. Reject it up front with a clear, actionable
                    // message instead of the old cascade (the alias's own `T`
                    // was reported as an unknown type at the definition, then
                    // every use of the alias cascaded). Register the name as an
                    // error type so uses do not additionally report "unknown
                    // type". (Non-generic aliases are fully supported.)
                    self.error_with_code(
                        format!(
                            "generic type alias `{name}` is not yet supported -- use the \
                             underlying generic type directly (e.g. `Result<T, str>` in \
                             place of `{name}<T>`)"
                        ),
                        *span,
                        kryos_errors::codes::E0110,
                    );
                    self.env.define_var(name.clone(), Type::Error);
                } else {
                    let resolved = self.resolve_type_expr(ty);
                    // Register as a variable in the type namespace for lookup.
                    self.env.define_var(name.clone(), resolved);
                }
            }
            Decl::Const {
                name,
                ty,
                value,
                mutable,
                span,
                ..
            } => {
                // (Self-referential and mutual/function-indirected const
                // cycles are detected up front by the interprocedural cycle
                // pass in check_module; no per-const check needed here.)
                let resolved_ty = if let Some(t) = ty {
                    let decl_ty = self.resolve_type_expr(t);
                    // Check the value against the annotation (mirrors the
                    // local Stmt::Let path). Previously skipped entirely, so
                    // `let X: str = 42` passed the checker and produced a
                    // garbage value at runtime. Forward-ref guard: a value
                    // calling a function declared LATER in the file infers
                    // Error (with a spurious E0102) because registration is
                    // sequential - suppress those diagnostics and skip the
                    // unify, preserving the previously-accepted pattern (the
                    // unannotated branch has the same limitation).
                    let diags_before = self.diagnostics.len();
                    let inferred_ty = self.infer_expr(value);
                    if matches!(self.engine.resolve(&inferred_ty), Type::Error) {
                        self.diagnostics.truncate(diags_before);
                    } else if let Err(diag) = self.engine.unify(&decl_ty, &inferred_ty, *span) {
                        self.diagnostics.push(diag);
                    }
                    self.check_int_literal_range(&decl_ty, value, *span);
                    decl_ty
                } else {
                    self.infer_expr(value)
                };
                if *mutable {
                    self.env.define_var_mut(name.clone(), resolved_ty);
                } else {
                    self.env.define_var(name.clone(), resolved_ty);
                }
            }
            Decl::Actor {
                name,
                state_fields,
                handlers,
                span,
                ..
            } => {
                self.actor_names.insert(name.clone());
                // Duplicate state field names -- an actor's state uses the same
                // positional field storage as a struct, so a repeated name made
                // `self.v` resolve to an implementation-defined occurrence
                // (mirror of the Pass-45 struct-field fix).
                {
                    let mut seen = std::collections::HashSet::new();
                    for f in state_fields {
                        if !seen.insert(f.name.as_str()) {
                            self.error(
                                format!(
                                    "duplicate state field `{}` in actor `{name}` -- each field name may appear only once",
                                    f.name
                                ),
                                *span,
                            );
                        }
                    }
                }
                // Duplicate handler names -- two same-named handlers mangle to
                // the same symbol `Actor__handler` and previously died at
                // codegen with a raw internal Cranelift ABI dump instead of a
                // clean diagnostic (mirror of dup_method_same_impl).
                {
                    let mut seen = std::collections::HashSet::new();
                    for h in handlers {
                        if !seen.insert(h.name.as_str()) {
                            self.error(
                                format!(
                                    "duplicate handler `{}` in actor `{name}` -- an actor cannot define the same handler name twice",
                                    h.name
                                ),
                                *span,
                            );
                        }
                    }
                }
                // Register the actor as a struct-like type with its state fields
                // (so `self.field` in handlers type-checks).
                let field_types: Vec<(String, Type)> = state_fields
                    .iter()
                    .map(|f| (f.name.clone(), self.resolve_type_expr(&f.ty)))
                    .collect();
                self.env.define_struct(StructDef {
                    name: name.clone(),
                    generic_params: vec![],
                    generic_var_ids: vec![],
                    fields: field_types,
                });
                let actor_ty = Type::Struct {
                    name: name.clone(),
                    generics: vec![],
                };
                // `Counter()` -- zero-arg constructor spawning the actor.
                // Constructing (not yet invoking any handler) requires no
                // authority of its own -- bind its own_cap_var to empty
                // immediately rather than leaving it open.
                let ctor_cap_var = self.engine.fresh_cap_var();
                self.engine.bind_cap_var(ctor_cap_var, crate::ty::CapRow::empty());
                self.env.define_function(FunctionSig {
                    name: name.clone(),
                    generic_params: vec![],
                    generic_var_ids: vec![],
                    generic_cap_var_ids: vec![],
                    own_cap_var: ctor_cap_var,
                    params: vec![],
                    ret: actor_ty.clone(),
                });
                // Handlers become methods `c.handler(msg_args)`; sends are
                // fire-and-forget so the observable return is unit.
                let method_sigs: Vec<FunctionSig> = handlers
                    .iter()
                    .map(|h| {
                        // Resolving a fn-typed param mints a fresh capability-row
                        // var into `pending_cap_var_ids`. Clear first so this
                        // handler's signature drains ONLY its own.
                        self.pending_cap_var_ids.clear();
                        let params: Vec<(String, Type)> = h
                            .params
                            .iter()
                            .map(|p| {
                                if p.name == "self" {
                                    (p.name.clone(), actor_ty.clone())
                                } else {
                                    let ty = p
                                        .ty
                                        .as_ref()
                                        .map(|t| self.resolve_type_expr(t))
                                        .unwrap_or_else(|| self.engine.fresh_var());
                                    (p.name.clone(), ty)
                                }
                            })
                            .collect();
                        // Call-site return type is ALWAYS Void: actor dispatch is
                        // genuinely asynchronous (each actor runs on its own OS
                        // thread; `kryos_actor_send` just enqueues a message into
                        // a mailbox -- there is no reply channel in the runtime),
                        // so a handler's return value can never reach the call
                        // site. A declared non-void return (`-> i64`, `-> f64`,
                        // ...) is recorded in `actor_nonvoid_handlers` so a call
                        // to it is rejected with a loud, precise error instead of
                        // silently threading back 0 (or crashing for f64).
                        if let Some(t) = h.ret_ty.as_ref() {
                            let declared = self.resolve_type_expr(t);
                            if declared != Type::Void {
                                self.actor_nonvoid_handlers
                                    .insert((name.clone(), h.name.clone()), declared);
                            }
                        }
                        FunctionSig {
                            name: h.name.clone(),
                            generic_params: vec![],
                            generic_var_ids: vec![],
                            // A handler's fn-typed PARAMETER row vars belong to
                            // this signature and must be freshened per call site,
                            // exactly as `register_decl` does for a plain function
                            // (line ~1243). Hardcoding this to `vec![]` meant a
                            // handler that is row-polymorphic in a closure param
                            // never had that var freshened, so the concrete
                            // argument bound a different var and the row charged
                            // at the call site stayed permanently open -- a
                            // closure passed actor-to-actor cost zero
                            // (LEDGER item 33).
                            generic_cap_var_ids: std::mem::take(
                                &mut self.pending_cap_var_ids,
                            ),
                            // Bound for real once the handler BODY is walked
                            // -- see the `Decl::Actor` arm of `check_decl`,
                            // which looks this same var back up via
                            // `lookup_method` and binds it there.
                            own_cap_var: self.engine.fresh_cap_var(),
                            params,
                            ret: Type::Void,
                        }
                    })
                    .collect();
                self.env.define_impl(name.clone(), None, method_sigs);
            }
            Decl::Import { .. } => {}
            Decl::Extern { items, .. } => {
                // Register extern function declarations so they're callable.
                // Validate the shape FIRST (E0508) -- declaring an unsupported
                // extern is rejected at check time rather than left to fail
                // later at link time or crash at runtime; see
                // `check_extern_item_shape`.
                for item in items {
                    self.check_extern_item_shape(item);
                    self.register_decl(item);
                }
            }
        }
    }

    /// `kryos_*` runtime symbols this compiler is verified to marshal
    /// correctly when hand-declared with a `str`-typed parameter or return.
    /// Every OTHER `kryos_*` name must stick to raw i64/i32/f64/ptr types --
    /// the real native ABI for a symbol lives in kryos-rt/kryos-stdlib-native
    /// and is not otherwise visible from this crate, so this is kept as an
    /// explicit, small, reviewable allowlist rather than inferred. Sourced
    /// from a repo-wide grep of every `kryos_*` extern signature that
    /// actually uses `str` (`compiler/stdlib/{ffi,strext,string}.kry` +
    /// the `examples/*.kry` files that redeclare the same `kryos_ffi_*`
    /// helpers locally) -- see LEDGER.md's FFI wave for how this was built.
    const EXTERN_STR_SAFE_KRYOS_NAMES: &[&str] = &[
        "kryos_builtin_to_upper",
        "kryos_builtin_to_lower",
        "kryos_ffi_dlopen",
        "kryos_ffi_dlsym",
        "kryos_ffi_cstr",
        "kryos_ffi_strlen",
        "kryos_ffi_string_from_ptr",
    ];

    /// A type an `extern` signature may use to describe a raw C-ABI-
    /// compatible value: plain scalars, the opaque `ptr` type, and `*T` raw
    /// pointers. Every other Kryos type (`str`, array, map, struct/enum,
    /// tuple, `fn`, `dyn Trait`, `Optional`/`Shared`/`Weak`) is heap/handle-
    /// managed and is NOT the bit pattern a real C ABI (or this compiler's
    /// own raw-pointer runtime symbols) expects.
    fn is_ffi_scalar_type(ty: &TypeExpr) -> bool {
        match ty {
            TypeExpr::Simple { name, .. } => matches!(
                name.as_str(),
                "i8" | "i16"
                    | "i32"
                    | "i64"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "f32"
                    | "f64"
                    | "bool"
                    | "ptr"
                    | "never"
                    | "void"
            ),
            TypeExpr::Pointer { .. } => true,
            _ => false,
        }
    }

    /// Reject an `extern` function declaration whose signature this compiler
    /// cannot safely emit a call for (E0508 -- `kryos explain E0508` has the
    /// full rationale). Two shapes are rejected:
    ///
    /// 1. A non-`kryos_*` name: arbitrary C-library FFI is not implemented
    ///    (the extern's param/symbol info is not threaded to codegen), so
    ///    such a declaration either fails to link, fails with a confusing
    ///    type mismatch, or "succeeds" only via an unrelated builtin-name
    ///    collision -- verified live: `extern "C" { fn getpid() -> i32 }`
    ///    fails AOT codegen with "use of undefined value '@getpid'";
    ///    `extern "C" { fn abs(x: i32) -> i32 }` fails AOT with a type
    ///    mismatch and only "works" on `kryos run` because `abs` collides
    ///    with the ambient builtin; `extern "C" { fn puts(s: str) -> i32 }`
    ///    builds and runs but silently prints nothing at all.
    /// 2. A `kryos_*`-prefixed name with a str/array/map/struct/tuple/enum/fn
    ///    -typed parameter or return, outside `EXTERN_STR_SAFE_KRYOS_NAMES`:
    ///    the real native symbols behind these names expect raw pointer/
    ///    length pairs (e.g. `kryos_env_get(key_ptr: i64, key_len: i64,
    ///    val_buf: i64, val_buf_len: i64) -> i64`, per `std::os`), not a
    ///    Kryos `str` handle -- hand-declaring `kryos_env_get(key: str) ->
    ///    str` compiles clean and SEGFAULTS both backends at runtime
    ///    (verified live).
    ///
    /// This fires on the DECLARATION itself, not the call site: the defect
    /// is in the signature's shape, independent of whether or how it's ever
    /// called (mirrors the capability model's "declaring is free, but here
    /// the shape itself -- not authority -- is the problem").
    fn check_extern_item_shape(&mut self, item: &Decl) {
        let Decl::Function {
            name,
            params,
            ret_ty,
            span,
            ..
        } = item
        else {
            return;
        };

        if !name.starts_with("kryos_") {
            self.error_with_code(
                format!(
                    "extern function `{name}` is not a `kryos_*` runtime symbol -- arbitrary C-library FFI is not implemented by this compiler (declaring it is accepted, but calling it will not reliably link, marshal, or execute correctly; see docs/13-ffi.md, `kryos explain E0508`)"
                ),
                *span,
                kryos_errors::codes::E0508,
            );
            return;
        }

        if Self::EXTERN_STR_SAFE_KRYOS_NAMES.contains(&name.as_str()) {
            return;
        }

        let has_unsafe_param = params
            .iter()
            .any(|p| p.ty.as_ref().is_some_and(|t| !Self::is_ffi_scalar_type(t)));
        let has_unsafe_ret = ret_ty
            .as_ref()
            .is_some_and(|t| !Self::is_ffi_scalar_type(t));

        if has_unsafe_param || has_unsafe_ret {
            self.error_with_code(
                format!(
                    "extern function `{name}` hand-declares a runtime symbol with a str/array/map/struct-typed parameter or return -- this bypasses the runtime's internal marshalling and segfaults at runtime (call the safe builtin/stdlib wrapper instead; see `kryos explain E0508`)"
                ),
                *span,
                kryos_errors::codes::E0508,
            );
        }
    }

    /// Check a declaration's body (second pass).
    fn check_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Function {
                name,
                generics,
                params,
                ret_ty,
                body: Some(body),
                span,
                ..
            } => {
                self.env.push_scope();

                // Set @pure flag for the duration of this function body.
                let was_pure = self.in_pure_function;
                if self.pure_functions.contains(name) {
                    self.in_pure_function = true;
                }

                // Bind generic type parameters as type variables so they resolve
                // in parameter types, return types, and the function body.
                // Record trait bounds so method calls on this type var can be
                // resolved through the bound trait's method signatures.
                let mut bound_var_ids: Vec<u32> = Vec::new();
                for gp in generics {
                    let tv = self.engine.fresh_var();
                    if let Type::Var(id) = &tv {
                        if !gp.bounds.is_empty() {
                            self.generic_var_bounds.insert(*id, gp.bounds.clone());
                            bound_var_ids.push(*id);
                        }
                    }
                    self.env.define_var(gp.name.clone(), tv);
                }

                // Bind parameters in function scope.
                let sig = self.env.lookup_function(name).cloned();
                if let Some(ref sig) = sig {
                    for (pname, pty) in &sig.params {
                        self.env.define_var(pname.clone(), pty.clone());
                    }
                    self.current_return_type = Some(sig.ret.clone());
                } else {
                    // Fallback: resolve params directly.
                    for param in params {
                        let ty = param
                            .ty
                            .as_ref()
                            .map(|t| self.resolve_type_expr(t))
                            .unwrap_or_else(|| self.engine.fresh_var());
                        self.env.define_var(param.name.clone(), ty);
                    }
                    self.current_return_type = ret_ty.as_ref().map(|t| self.resolve_type_expr(t));
                }

                self.current_function_name = Some(name.clone());
                self.seed_tail_lambda_expected(body);
                self.cap_accum_stack.push(crate::ty::CapRow::empty());
                self.check_block(body);
                let fn_caps = self.cap_accum_stack.pop().unwrap_or_else(crate::ty::CapRow::empty);
                if std::env::var("KRYOS_ROW_TRACE").is_ok() {
                    eprintln!("[row] fn {} = {}", name, self.engine.resolve_cap_row(&fn_caps).display());
                }
                if let Some(own_var) = sig.as_ref().map(|s| s.own_cap_var) {
                    self.engine.bind_cap_var(own_var, fn_caps.clone());
                    self.log_fn_effect(name.clone(), *span, crate::ty::CapRow::var(own_var));
                }
                self.current_function_name = None;

                // Check for missing return in non-void functions.
                if let Some(ref ret) = self.current_return_type {
                    if *ret != Type::Void && !block_returns(body) {
                        self.error(
                            format!(
                                "function `{name}` has return type but not all paths return a value"
                            ),
                            *span,
                        );
                    }
                }

                self.current_return_type = None;
                self.in_pure_function = was_pure;
                // Clear trait bounds for this function's generic vars so they
                // don't leak into the next function's checking.
                for id in &bound_var_ids {
                    self.generic_var_bounds.remove(id);
                }
                self.env.pop_scope();

                let _ = span; // suppress unused warning
            }
            Decl::Actor {
                name,
                state_fields,
                handlers,
                ..
            } => {
                // register_decl builds the actor's state struct + handler
                // signatures for CALL-SITE typing, but the handler BODIES were
                // never type-checked -- any type error, undefined variable/
                // function, or bad field access inside a handler silently passed
                // `kryos check`. Check each body here, exactly like an impl
                // method, with `self` (the actor's state struct) and the message
                // params in scope.
                let actor_ty = Type::Struct {
                    name: name.clone(),
                    generics: vec![],
                };
                let prev_self = self.current_self_type.take();
                self.current_self_type = Some(actor_ty.clone());
                let prev_fn_state = std::mem::take(&mut self.current_actor_fn_state_fields);
                for f in state_fields.iter() {
                    let fty = self.resolve_type_expr(&f.ty);
                    if Self::type_contains_function(&fty) {
                        self.current_actor_fn_state_fields.insert(f.name.clone());
                    }
                }
                for h in handlers {
                    self.env.push_scope();
                    // Actor state fields are accessible (and mutable) by BARE
                    // name inside a handler (implicit self), e.g. `count = count + 1`.
                    for sf in state_fields {
                        let ft = self.resolve_type_expr(&sf.ty);
                        self.env.define_var_mut(sf.name.clone(), ft);
                    }
                    // Bind params from the REGISTERED signature, not by
                    // re-resolving `p.ty` here. `register_decl` already resolved
                    // the same `TypeExpr` to build the handler's `FunctionSig`,
                    // minting the capability-row var that lands in
                    // `generic_cap_var_ids` -- the only var a CALL SITE can remap
                    // through `instantiate_row`. Re-resolving mints a SECOND,
                    // unrelated row var: the body then charges that one while
                    // callers remap the registered one, so the handler's own row
                    // resolves to a var nothing will ever bind and the call costs
                    // zero (LEDGER item 33). The impl-method path below already
                    // binds from `sig.params` for this reason; actor handlers were
                    // the odd one out. The failure is SILENT -- enforcement runs,
                    // finds an open row, and passes.
                    let h_sig = self.env.lookup_method(name, &h.name).cloned();
                    for p in &h.params {
                        let pty = if p.name == "self" {
                            actor_ty.clone()
                        } else {
                            let from_sig = h_sig.as_ref().and_then(|sig| {
                                sig.params
                                    .iter()
                                    .find(|(pn, _)| pn == &p.name)
                                    .map(|(_, t)| t.clone())
                            });
                            match from_sig {
                                Some(t) => t,
                                None => match p.ty.as_ref() {
                                    Some(t) => self.resolve_type_expr(t),
                                    None => self.engine.fresh_var(),
                                },
                            }
                        };
                        self.env.define_var(p.name.clone(), pty);
                    }
                    // `self` is available even in the bare-name handler style
                    // (no explicit `self` param).
                    if !h.params.iter().any(|p| p.name == "self") {
                        self.env.define_var("self".to_string(), actor_ty.clone());
                    }
                    let prev_ret = self.current_return_type.take();
                    // Check the handler body against its DECLARED return type, not
                    // a hardcoded Void -- a request-response handler
                    // (`fn add(x: f64) -> f64 { .. return memory }`, the documented
                    // primary actor pattern) otherwise failed to type-check
                    // ("declared to return void, but body evaluates to f64").
                    self.current_return_type = Some(
                        h.ret_ty
                            .as_ref()
                            .map(|t| self.resolve_type_expr(t))
                            .unwrap_or(Type::Void),
                    );
                    let prev_fn = self.current_function_name.take();
                    self.current_function_name = Some(h.name.clone());
                    self.cap_accum_stack.push(crate::ty::CapRow::empty());
                    self.check_block(&h.body);
                    let handler_caps = self.cap_accum_stack.pop().unwrap_or_else(crate::ty::CapRow::empty);
                    if std::env::var("KRYOS_ROW_TRACE").is_ok() {
                        eprintln!("[row] handler {}::{} = {}", name, h.name,
                            self.engine.resolve_cap_row(&handler_caps).display());
                    }
                    if let Some(own_var) = self
                        .env
                        .lookup_method(name, &h.name)
                        .map(|sig| sig.own_cap_var)
                    {
                        self.engine.bind_cap_var(own_var, handler_caps.clone());
                        self.log_fn_effect(
                            format!("{name}::{}", h.name),
                            h.span,
                            crate::ty::CapRow::var(own_var),
                        );
                    }
                    self.current_function_name = prev_fn;
                    self.current_return_type = prev_ret;
                    self.env.pop_scope();
                }
                self.current_self_type = prev_self;
                self.current_actor_fn_state_fields = prev_fn_state;
            }
            Decl::Impl {
                target,
                trait_name,
                methods,
                generics: impl_generics,
                span: impl_span,
                ..
            } => {
                // Set Self type for the duration of this impl block.
                let prev_self = self.current_self_type.take();

                // Scope the impl's type parameters so method bodies that
                // mention them (`let x: T = ...`, `-> T` in the fallback
                // path) resolve. No-op for concrete impls.
                let scoped_impl_generics = !impl_generics.is_empty();
                if scoped_impl_generics {
                    self.env.push_scope();
                    for gp in impl_generics {
                        let tv = self.engine.fresh_var();
                        self.env.define_var(gp.name.clone(), tv);
                    }
                }

                // Resolve the target type so we can bind `self` in methods.
                let target_ty = if self.env.lookup_struct(target).is_some() {
                    Some(Type::Struct {
                        name: target.clone(),
                        generics: vec![],
                    })
                } else if self.env.lookup_enum(target).is_some() {
                    Some(Type::Enum {
                        name: target.clone(),
                        generics: vec![],
                    })
                } else {
                    None
                };

                self.current_self_type = target_ty.clone();

                // Trait/impl conformance. A `impl Trait for Type` whose method
                // signature DISAGREES with the trait's declaration (param or
                // return type) was accepted, then dispatched through a generic
                // bound or `dyn` vtable at the trait's declared ABI while the
                // body used the impl's ABI -- a memory-safety SEGFAULT. Also,
                // an `impl UnknownTrait for Type` (typo'd trait name) silently
                // degraded to an inherent impl with no signal. Both checked
                // here (all decls are registered by now, so lookup is
                // order-independent). The comparison is CONSERVATIVE: it flags
                // only a conflict between two fully-GROUND types (primitives /
                // named / ground containers), never one involving a type var,
                // `Self`, a generic param, or a function type -- so a legit
                // generic trait method is never falsely rejected.
                if let Some(tname) = &trait_name {
                    match self.env.lookup_trait(tname).cloned() {
                        None => {
                            self.error_with_code(
                                format!(
                                    "unknown trait `{tname}` in `impl {tname} for {target}` -- the trait is not declared; a misspelled trait name silently becomes an unchecked inherent impl"
                                ),
                                *impl_span,
                                kryos_errors::codes::E0105,
                            );
                        }
                        Some(tdef) => {
                            fn ground_name(t: &Type) -> Option<String> {
                                match t {
                                    Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128
                                    | Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128
                                    | Type::F32 | Type::F64 | Type::Bool | Type::Char
                                    | Type::Str | Type::USize | Type::ISize | Type::Void => {
                                        Some(format!("{t:?}"))
                                    }
                                    Type::Struct { name, .. } => Some(format!("struct {name}")),
                                    Type::Enum { name, .. } => Some(format!("enum {name}")),
                                    Type::Array { element, .. } => {
                                        ground_name(element).map(|e| format!("[{e}]"))
                                    }
                                    Type::Option { inner } => {
                                        ground_name(inner).map(|i| format!("Option<{i}>"))
                                    }
                                    Type::Result { ok, err } => match (
                                        ground_name(ok),
                                        ground_name(err),
                                    ) {
                                        (Some(o), Some(e)) => Some(format!("Result<{o},{e}>")),
                                        _ => None,
                                    },
                                    _ => None,
                                }
                            }
                            for method in methods {
                                if let Decl::Function {
                                    name: mname,
                                    params: m_params,
                                    ret_ty: m_ret,
                                    span: m_span,
                                    ..
                                } = method
                                {
                                    let Some(tsig) =
                                        tdef.methods.iter().find(|s| &s.name == mname)
                                    else {
                                        continue;
                                    };
                                    // Compare non-self params positionally.
                                    let impl_np: Vec<&kryos_ast::Param> = m_params
                                        .iter()
                                        .filter(|p| p.name != "self")
                                        .collect();
                                    let trait_np: Vec<&Type> = tsig
                                        .params
                                        .iter()
                                        .filter(|(n, _)| n != "self")
                                        .map(|(_, t)| t)
                                        .collect();
                                    // Arity mismatch: an impl method with more or
                                    // fewer params than the trait declares was
                                    // dispatched through a generic bound at the
                                    // trait's arg count and died with a raw
                                    // Cranelift "mismatched argument count" ICE.
                                    if impl_np.len() != trait_np.len() {
                                        self.error_with_code(
                                            format!("method `{mname}` in `impl {tname} for {target}` takes {} parameter{} but the trait declares {}", impl_np.len(), if impl_np.len() == 1 { "" } else { "s" }, trait_np.len()),
                                            *m_span,
                                            kryos_errors::codes::E0110,
                                        );
                                    }
                                    if impl_np.len() == trait_np.len() {
                                        for (ip, tp) in impl_np.iter().zip(trait_np.iter()) {
                                            if let Some(ty) = &ip.ty {
                                                let ity = self.resolve_type_expr(ty);
                                                let it = self.engine.resolve(&ity);
                                                let tt = self.engine.resolve(tp);
                                                if let (Some(a), Some(b)) =
                                                    (ground_name(&it), ground_name(&tt))
                                                {
                                                    if a != b {
                                                        self.error_with_code(
                                                            format!("method `{mname}` in `impl {tname} for {target}` has parameter type `{it}` but the trait declares `{tt}`"),
                                                            *m_span,
                                                            kryos_errors::codes::E0100,
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    // Compare return types.
                                    let impl_ret = match m_ret.as_ref() {
                                        Some(t) => {
                                            let rt = self.resolve_type_expr(t);
                                            self.engine.resolve(&rt)
                                        }
                                        None => Type::Void,
                                    };
                                    let trait_ret = self.engine.resolve(&tsig.ret);
                                    if let (Some(a), Some(b)) =
                                        (ground_name(&impl_ret), ground_name(&trait_ret))
                                    {
                                        if a != b {
                                            self.error_with_code(
                                                format!("method `{mname}` in `impl {tname} for {target}` returns `{impl_ret}` but the trait declares `{trait_ret}`"),
                                                *m_span,
                                                kryos_errors::codes::E0100,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Self-type conformance (all impls, inherent + trait). A method
                // whose explicit `self: T` annotation names a DIFFERENT concrete
                // struct/enum than the impl target reinterprets the receiver's
                // memory through the wrong layout -> SEGFAULT / garbage read
                // (`impl Speaker for Dog { fn speak(self: Cat) ... }`). `self:
                // Self` and an unannotated `self` resolve to the target and are
                // fine; only a concrete NON-target aggregate is rejected.
                if let Some(tt) = &target_ty {
                    let target_ground = match tt {
                        Type::Struct { name, .. } => Some(name.clone()),
                        Type::Enum { name, .. } => Some(name.clone()),
                        _ => None,
                    };
                    for method in methods {
                        if let Decl::Function {
                            name: mname,
                            params: m_params,
                            span: m_span,
                            ..
                        } = method
                        {
                            if let Some(sp) = m_params.iter().find(|p| p.name == "self") {
                                if let Some(sty) = &sp.ty {
                                    let rs = self.resolve_type_expr(sty);
                                    let self_ground = match self.engine.resolve(&rs) {
                                        Type::Struct { name, .. } => Some(name),
                                        Type::Enum { name, .. } => Some(name),
                                        _ => None, // Self/DynTrait/Var -> fine
                                    };
                                    if let (Some(sg), Some(tg)) = (&self_ground, &target_ground) {
                                        if sg != tg {
                                            self.error_with_code(
                                                format!("method `{mname}` declares `self: {sg}` but is implemented on `{tg}` -- the receiver type must be `{tg}` (or `Self`)"),
                                                *m_span,
                                                kryos_errors::codes::E0100,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                for method in methods {
                    if let (
                        Some(ref ty),
                        Decl::Function {
                            name: mname,
                            body: Some(body),
                            params: m_params,
                            ret_ty: m_ret,
                            generics: m_generics,
                            ..
                        },
                    ) = (&target_ty, method)
                    {
                        // Check the method body inline, binding `self` and
                        // the rest of the params directly from the impl's
                        // signature - without registering the method as a
                        // global function (which would shadow same-named
                        // builtins inside the body).
                        self.env.push_scope();
                        // Fallback binding: the bare (non-generic) impl target
                        // type. For a generic impl this is a placeholder --
                        // it's overwritten below from the method's own
                        // signature, which carries the correct generic
                        // parameterization (e.g. `Box<T>`, not bare `Box`).
                        self.env.define_var("self".to_string(), ty.clone());

                        // Bind generics so param/return types resolve.
                        for gp in m_generics {
                            let tv = self.engine.fresh_var();
                            self.env.define_var(gp.name.clone(), tv);
                        }

                        // Bind params (including `self`) using the impl
                        // method's recorded signature when available
                        // (preserves the exact types used elsewhere,
                        // including `self`'s generic args), otherwise resolve
                        // them directly from the AST.
                        //
                        // `self` MUST be rebound here too, not skipped: the
                        // signature's `self` entry was resolved in
                        // register_decl while the impl's `<T>` scope was
                        // active (see the FunctionSig build above), so it
                        // carries `Box<T>` with the SAME generic var id as
                        // `sig.ret`. The fallback binding above has no
                        // generics at all (`Box`), so a bare `return self`
                        // failed to unify against a declared `-> Box<T>`
                        // return -- self's generic parameterization was
                        // silently dropped.
                        if let Some(sig) = self.env.lookup_method(target, mname).cloned() {
                            for (pname, pty) in &sig.params {
                                self.env.define_var(pname.clone(), pty.clone());
                            }
                            let prev_ret = self.current_return_type.take();
                            self.current_return_type = Some(sig.ret.clone());
                            let prev_fn = self.current_function_name.take();
                            self.current_function_name = Some(mname.clone());
                            self.seed_tail_lambda_expected(body);
                            // An impl method's OWN row was never computed: this
                            // site had no accumulator frame at all, unlike plain
                            // functions and actor handlers. So `own_cap_var` stayed
                            // permanently unbound, every call site resolved it to
                            // nothing, and a privileged closure handed to an impl
                            // method (`Invoker::run(reader)`) cost zero -- LEDGER
                            // item 35. Mirrors the Decl::Function pattern exactly.
                            self.cap_accum_stack.push(crate::ty::CapRow::empty());
                            self.check_block(body);
                            let method_caps = self
                                .cap_accum_stack
                                .pop()
                                .unwrap_or_else(crate::ty::CapRow::empty);
                            self.engine.bind_cap_var(sig.own_cap_var, method_caps);
                            self.current_function_name = prev_fn;
                            self.current_return_type = prev_ret;
                        } else {
                            for param in m_params {
                                if param.name == "self" {
                                    continue;
                                }
                                let ty = param
                                    .ty
                                    .as_ref()
                                    .map(|t| self.resolve_type_expr(t))
                                    .unwrap_or_else(|| self.engine.fresh_var());
                                self.env.define_var(param.name.clone(), ty);
                            }
                            let prev_ret = self.current_return_type.take();
                            self.current_return_type =
                                m_ret.as_ref().map(|t| self.resolve_type_expr(t));
                            let prev_fn = self.current_function_name.take();
                            self.current_function_name = Some(mname.clone());
                            self.seed_tail_lambda_expected(body);
                            // No signature to bind, but still give the body its own
                            // frame so its authority cannot leak into an unrelated
                            // enclosing accumulator.
                            self.cap_accum_stack.push(crate::ty::CapRow::empty());
                            self.check_block(body);
                            let _ = self.cap_accum_stack.pop();
                            self.current_function_name = prev_fn;
                            self.current_return_type = prev_ret;
                        }

                        self.env.pop_scope();
                    } else {
                        self.check_decl(method);
                    }
                }

                if scoped_impl_generics {
                    self.env.pop_scope();
                }
                self.current_self_type = prev_self;
            }
            _ => {}
        }
    }

    // ── Block / statement checking ───────────────────────────────────

    pub fn check_block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            self.check_stmt(stmt);
        }
    }

    /// The value type of a trailing `try { .. } catch e { .. }` used as a
    /// block's tail VALUE (mirrors the `Stmt::If`/`Expr::IfExpr` branch-
    /// unification logic in `infer_block_as_expr`/`Expr::IfExpr`): unify the
    /// try block's tail type with the catch block's tail type, excluding
    /// either side from unification if it always diverges (throw/return) --
    /// same divergence rule as an if/else branch pair, via `block_diverges`.
    fn infer_try_catch_value_type(
        &mut self,
        try_block: &Block,
        catch_name: &str,
        catch_block: &Block,
        span: Span,
    ) -> Type {
        self.env.push_scope();
        let try_ty = self.infer_block_as_expr(try_block);
        self.env.pop_scope();
        let try_diverges = block_diverges(try_block);

        self.env.push_scope();
        self.env.define_var(catch_name.to_string(), Type::Str);
        let catch_ty = self.infer_block_as_expr(catch_block);
        self.env.pop_scope();
        let catch_diverges = block_diverges(catch_block);

        match (try_diverges, catch_diverges) {
            (true, true) => Type::Void,
            (true, false) => catch_ty,
            (false, true) => try_ty,
            (false, false) => {
                if let Err(diag) = self.engine.unify(&try_ty, &catch_ty, span) {
                    self.diagnostics.push(diag);
                }
                try_ty
            }
        }
    }

    /// Like `check_block`, but returns the type of the tail expression if the
    /// last statement is an `Expr` statement (i.e. the block is used as a value).
    /// Returns `Type::Void` if the block is empty or ends with a non-expr statement.
    fn infer_block_as_expr(&mut self, block: &Block) -> Type {
        let n = block.stmts.len();
        for (i, stmt) in block.stmts.iter().enumerate() {
            if i == n - 1 {
                if let Stmt::Expr { expr, .. } = stmt {
                    return self.infer_expr(expr);
                }
                // An if-else as the last statement can produce a value just like
                // an if expression.  We infer the branch types without emitting
                // diagnostics for the stmt itself here (check_stmt does that).
                if let Stmt::If {
                    condition,
                    then_block,
                    elif_clauses,
                    else_block: Some(else_blk),
                    span,
                } = stmt
                {
                    let cond_ty = self.infer_expr(condition);
                    if let Err(diag) = self.engine.unify(&Type::Bool, &cond_ty, *span) {
                        self.diagnostics.push(diag);
                    }
                    self.env.push_scope();
                    let mut branch_ty = self.infer_block_as_expr(then_block);
                    self.env.pop_scope();
                    // A branch whose control flow always diverges
                    // (throw/return) never yields a value -- it must not be
                    // unified against, or override, a real branch type (see
                    // the matching fix in `Expr::IfExpr`).
                    let mut branch_diverges = block_diverges(then_block);
                    // Walk elif clauses; their branch type must match unless
                    // one side is divergent.
                    for (elif_cond, elif_block) in elif_clauses {
                        let ec_ty = self.infer_expr(elif_cond);
                        if let Err(diag) = self.engine.unify(&Type::Bool, &ec_ty, *span) {
                            self.diagnostics.push(diag);
                        }
                        self.env.push_scope();
                        let elif_ty = self.infer_block_as_expr(elif_block);
                        self.env.pop_scope();
                        let elif_diverges = block_diverges(elif_block);
                        if elif_diverges {
                            // Diverging elif contributes nothing; keep the
                            // accumulated branch type/divergence as-is.
                        } else if branch_diverges {
                            branch_ty = elif_ty;
                            branch_diverges = false;
                        } else if let Err(diag) = self.engine.unify(&branch_ty, &elif_ty, *span) {
                            self.diagnostics.push(diag);
                        }
                    }
                    self.env.push_scope();
                    let else_ty = self.infer_block_as_expr(else_blk);
                    self.env.pop_scope();
                    let else_diverges = block_diverges(else_blk);
                    if else_diverges {
                        // Diverging else contributes nothing; branch_ty (or
                        // Void if every branch diverged) stands as-is.
                    } else if branch_diverges {
                        branch_ty = else_ty;
                    } else if let Err(diag) = self.engine.unify(&branch_ty, &else_ty, *span) {
                        self.diagnostics.push(diag);
                        // If unification fails return the else type so the outer
                        // expression still gets a concrete (non-void) type.
                        branch_ty = else_ty;
                    }
                    return branch_ty;
                }
                // A trailing `try { .. } catch e { .. }` is the block's tail
                // VALUE too, same as the if-else case just above. Without
                // this a value-position block ending in try/catch (or an
                // if/match branch ending in one) typed as `void` -- e.g.
                // `let r = if n > 0 { try { n * 2 } catch e { -1 } } else {
                // 0 }` failed with "type mismatch: expected void, found
                // i64" on the else branch.
                if let Stmt::TryCatch {
                    try_block,
                    catch_name,
                    catch_block,
                    span,
                } = stmt
                {
                    return self.infer_try_catch_value_type(
                        try_block, catch_name, catch_block, *span,
                    );
                }
            }
            self.check_stmt(stmt);
        }
        Type::Void
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                name,
                mutable,
                ty,
                value,
                span,
                pattern,
            } => {
                // A BARE `f32` let-annotation is one of the two legal scalar
                // f32 positions (with `as f32` casts) -- resolve it directly,
                // bypassing resolve_type_expr's composite-position rejection.
                let declared_ty = ty.as_ref().map(|t| {
                    if matches!(t, TypeExpr::Simple { name, .. } if name == "f32") {
                        Type::F32
                    } else {
                        self.resolve_type_expr(t)
                    }
                });
                // A plain `let name = |...| { ... }` binding (no destructuring
                // pattern) may recurse through its own name -- this is exactly
                // what a nested/local `fn name(...) { ... }` desugars to in the
                // parser. Flag it so the upcoming `Expr::Lambda` check can
                // pre-bind `name` before checking its own body.
                if pattern.is_none() && matches!(value, Some(Expr::Lambda { .. })) {
                    self.pending_self_recursive_name = Some(name.clone());
                }
                // See `suppress_array_elem_unify`: a `[dyn Trait]` annotation
                // over an array literal is already rejected (E0110) by
                // `resolve_type_expr` above -- don't let the array literal's
                // own pairwise unify pile on a second, confusing diagnostic.
                // Checked against the RAW annotation, not the resolved
                // Type::Error it produced: Type::Error alone is ambiguous
                // (an unrelated unknown-type-name annotation also resolves
                // to it, and there a genuinely mismatched array literal
                // must keep its own element diagnostic).
                if let (Some(TypeExpr::Array { element, .. }), Some(Expr::ArrayLiteral { span: arr_span, .. })) =
                    (ty.as_ref(), value.as_ref())
                {
                    if matches!(element.as_ref(), TypeExpr::DynTrait { .. }) {
                        self.suppress_array_elem_unify.insert(*arr_span);
                    }
                }
                let inferred_ty = value.as_ref().map(|v| self.infer_expr(v));

                // LEDGER item 24: `any` erases to a bare i64 at codegen with
                // NO runtime type tag (item 6, ABI-blocked design note --
                // fixing this generally needs a tagged-value ABI change).
                // `bool` (i1) and `f64`/`f32` (double/float) have a NATIVE
                // LLVM representation that differs from i64, so storing one
                // directly into an explicit `any` slot either fails the LLVM
                // AOT build ("defined with type 'i1'/'double' but expected
                // 'i64'") or silently misrenders on the Cranelift JIT (prints
                // the raw bit pattern instead of the value) -- reproduced
                // live both ways. `i64`/`str`/array/map/struct values are
                // already i64-shaped (value or pointer) and are unaffected.
                // Reject at check time with a clear diagnostic instead of
                // letting either backend discover it downstream as a build
                // failure or a silent wrong answer.
                if let Some(TypeExpr::Simple { name: tn, .. }) = ty.as_ref() {
                    if tn == "any" || tn == "Any" {
                        if let Some(ref inferred) = inferred_ty {
                            let resolved = self.engine.resolve(inferred);
                            if matches!(resolved, Type::Bool | Type::F64 | Type::F32) {
                                self.error_with_code(
                                    format!(
                                        "cannot store a `{resolved}` value in an `any`-typed binding -- `any` is erased to a bare i64 at runtime with no type tag, and `{resolved}`'s native representation is not i64-compatible, so this either fails to build on the AOT backend or silently prints the wrong value on the JIT backend. Keep the value in its concrete type (`let x: {resolved} = ..`) instead of erasing it to `any`, or convert it to its final form (e.g. `to_string(..)`) before storing it"
                                    ),
                                    *span,
                                    kryos_errors::codes::E0110,
                                );
                            }
                        }
                    }
                }

                let final_ty = match (declared_ty, inferred_ty) {
                    (Some(decl), Some(mut inferred)) => {
                        // A bare FLOAT LITERAL adapts to a narrower declared
                        // float type: `let a: f32 = 3.14` types the literal
                        // f32, mirroring narrow-int literal inference
                        // (`let b: u8 = 200`). Literal-only by design --
                        // a COMPUTED f64 still needs an explicit `as f32`
                        // (lossy narrowing must be visible), and negated
                        // literals (`-1.5`, a unary op on a literal) count.
                        let is_float_literal = match value.as_ref() {
                            Some(Expr::FloatLiteral { .. }) => true,
                            Some(Expr::UnaryOp { operand, .. }) => {
                                matches!(operand.as_ref(), Expr::FloatLiteral { .. })
                            }
                            _ => false,
                        };
                        if is_float_literal
                            && inferred == Type::F64
                            && self.engine.resolve(&decl) == Type::F32
                        {
                            inferred = Type::F32;
                        }
                        // Both declared and inferred: unify them.
                        if let Err(diag) = self.engine.unify(&decl, &inferred, *span) {
                            self.diagnostics.push(diag);
                        }
                        if let Some(v) = value.as_ref() {
                            self.check_int_literal_range(&decl, v, *span);
                        }
                        decl
                    }
                    (Some(decl), None) => decl,
                    (None, Some(inferred)) => inferred,
                    (None, None) => {
                        // No type info at all - create a fresh variable.
                        self.engine.fresh_var()
                    }
                };

                // Record the checker's authoritative type for unannotated
                // bindings the MIR's static inference gets wrong:
                //   - empty-array `let a = []` (element type comes from later
                //     `push` calls);
                //   - block-valued `let z = { ...; tail }` whose tail
                //     references a block-local -- MIR's infer_expr_type can't
                //     resolve a local declared earlier in the same block, so
                //     it defaulted to i64 and mis-coerced a str/ptr value into
                //     an `inttoptr` on AOT (clang rejected the module).
                // `concrete_type_to_type_expr` (applied when the map is
                // consumed) keeps only fully-resolved concrete types.
                if ty.is_none() {
                    let record = match value.as_ref() {
                        Some(Expr::ArrayLiteral { elements, .. }) => elements.is_empty(),
                        // Empty map `let m = {}` -- like the empty-array case, the
                        // key/value types come from later `m[k] = v` assignments,
                        // which unify the map's fresh type vars. Without recording
                        // the authoritative type, MIR defaulted the value slot to
                        // i64 and a str/aggregate value read back as raw pointer
                        // bits (silent miscompile).
                        Some(Expr::MapLiteral { entries, .. }) => entries.is_empty(),
                        Some(Expr::Block { .. }) => true,
                        _ => false,
                    };
                    if record {
                        self.resolved_let_types.insert(*span, final_ty.clone());
                    }
                }

                // Binding the result of the IN-PLACE builtins `sort`/`reverse`
                // (typed Void deliberately -- see the FnCall arm) previously
                // surfaced only as downstream "type `void` is not indexable" /
                // "cannot iterate over `void`" errors that never mentioned the
                // actual mistake. Steer directly at the binding site.
                if self.engine.resolve(&final_ty) == Type::Void {
                    if let Some(Expr::FnCall { callee, .. }) = value.as_ref() {
                        if let Expr::Identifier { name: cn, .. } = callee.as_ref() {
                            if cn == "sort" || cn == "reverse" {
                                self.error(
                                    format!(
                                        "the builtin `{cn}` sorts in place and returns nothing -- call `{cn}(arr)` as a statement, or `use std::iter::{{{cn}}}` for the copy-returning version"
                                    ),
                                    *span,
                                );
                            }
                        }
                    }
                }

                if let Some(pat) = pattern {
                    // Tuple / struct destructuring: bind each variable in the pattern.
                    // Pass through the outer `let mut` so `let mut (a, b) = ...`
                    // makes both bindings mutable, and `let (mut a, b) = ...`
                    // makes only `a` mutable via the per-element `mut` flag
                    // inside bind_pattern_with_mut.
                    self.pattern_dup_seen = Some(std::collections::HashMap::new());
                    self.bind_pattern_with_mut(pat, &final_ty, *mutable);
                    self.pattern_dup_seen = None;
                } else if *mutable {
                    self.env.define_var_mut(name.clone(), final_ty);
                } else {
                    self.env.define_var(name.clone(), final_ty);
                }
            }
            Stmt::Assign {
                target,
                value,
                span,
                ..
            } => {
                let target_ty = self.infer_expr(target);
                let mut value_ty = self.infer_expr(value);
                self.check_int_literal_range(&target_ty, value, *span);
                // Float-literal adaptation to an f32 target, mirroring the
                // `let` path: `m = 2.5` on an f32 var types the literal f32.
                // Literal-only; computed f64 still needs an explicit cast.
                let assign_is_float_literal = match value {
                    Expr::FloatLiteral { .. } => true,
                    Expr::UnaryOp { operand, .. } => {
                        matches!(operand.as_ref(), Expr::FloatLiteral { .. })
                    }
                    _ => false,
                };
                if assign_is_float_literal
                    && value_ty == Type::F64
                    && self.engine.resolve(&target_ty) == Type::F32
                {
                    value_ty = Type::F32;
                }
                if let Err(diag) = self.engine.unify(&target_ty, &value_ty, *span) {
                    self.diagnostics.push(diag);
                }
                // Enforce immutability: only `let mut` variables can be
                // reassigned. This is an ERROR (E0302), matching CLAUDE.md and
                // `kryos explain E0302` - it was previously only a warning, so
                // immutability was silently unenforced (backlog #113).
                //
                // NOTE: this checks only a BARE-identifier reassignment
                // (`x = v`). Field/index mutation through an immutable binding
                // (`let p = Point{..}; p.x = 9`, or a struct-typed PARAMETER's
                // `self.field = v`) is DELIBERATELY allowed -- it is the
                // established, conformance-tested Kryos contract (gotcha #23,
                // conf_ownership `mutate_pair`), and every impl method mutating
                // `self.field` relies on it. (docs/19 §7.4 claims the opposite;
                // that line is aspirational and contradicted by the language's
                // own tests -- do NOT extend E0302 to compound targets.)
                if let Expr::Identifier { name, .. } = target {
                    if !self.env.is_mutable(name) {
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "assignment to immutable variable `{name}`"
                            ))
                            .with_label(*span, "help: consider declaring with `let mut`")
                            .with_code(kryos_errors::codes::E0302),
                        );
                    }
                }
            }
            Stmt::Return { value, span } => {
                // Bidirectional inference for `return <lambda>`: a closure
                // literal returned from a function whose declared return type
                // is `fn(A) -> B` infers its un-annotated params/return from
                // that declaration, exactly like a lambda passed to a typed
                // call param (the FnCall arg path). Without this, `fn mk() ->
                // fn(f64) -> f64 { return |x| x * x }` failed E0100 (`x` only
                // inferred from typed expressions inside the body). Seeding
                // lambda_expected_types also records the resolved params in
                // resolved_lambda_params, so MIR types the closure params too
                // (no silent i64 erasure).
                if let Some(Expr::Lambda {
                    params: lparams,
                    span: lspan,
                    ..
                }) = value.as_ref()
                {
                    if let Some(ref expected) = self.current_return_type {
                        let resolved_ret = self.engine.resolve(expected);
                        if let Type::Function { params: eps, ret: er, .. } = &resolved_ret {
                            if eps.len() == lparams.len() {
                                self.lambda_expected_types
                                    .insert(*lspan, (eps.clone(), (**er).clone()));
                            }
                        }
                    }
                }
                let ret_ty = value
                    .as_ref()
                    .map(|v| self.infer_expr(v))
                    .unwrap_or(Type::Void);

                if let Some(ref expected) = self.current_return_type {
                    if let Err(mut diag) = self.engine.unify(expected, &ret_ty, *span) {
                        // Enhance error message with function context.
                        if let Some(ref fn_name) = self.current_function_name {
                            let msg = format!(
                                "function `{fn_name}` declared to return `{expected}`, but body evaluates to `{ret_ty}`"
                            );
                            diag = Diagnostic::error(msg)
                                .with_code(kryos_errors::codes::E0100)
                                .with_label(
                                    *span,
                                    format!("expected `{expected}`, found `{ret_ty}`"),
                                );
                        }
                        self.diagnostics.push(diag);
                    }
                }
            }
            Stmt::If {
                condition,
                then_block,
                elif_clauses,
                else_block,
                span,
            } => {
                let cond_ty = self.infer_expr(condition);
                if let Err(diag) = self.engine.unify(&Type::Bool, &cond_ty, *span) {
                    self.diagnostics.push(diag);
                }
                self.env.push_scope();
                self.check_block(then_block);
                self.env.pop_scope();
                for (elif_cond, elif_block) in elif_clauses {
                    let elif_ty = self.infer_expr(elif_cond);
                    if let Err(diag) = self.engine.unify(&Type::Bool, &elif_ty, *span) {
                        self.diagnostics.push(diag);
                    }
                    self.env.push_scope();
                    self.check_block(elif_block);
                    self.env.pop_scope();
                }
                if let Some(else_blk) = else_block {
                    self.env.push_scope();
                    self.check_block(else_blk);
                    self.env.pop_scope();
                }
            }
            Stmt::While {
                condition,
                body,
                span,
            } => {
                let cond_ty = self.infer_expr(condition);
                if let Err(diag) = self.engine.unify(&Type::Bool, &cond_ty, *span) {
                    self.diagnostics.push(diag);
                }
                self.env.push_scope();
                self.check_block(body);
                self.env.pop_scope();
            }
            Stmt::For {
                pattern,
                iterable,
                body,
                ..
            } => {
                // Infer the iterable type and determine element type.
                let raw_iter_ty = self.infer_expr(iterable);
                let iter_ty = self.engine.resolve(&raw_iter_ty);
                let elem_ty = match &iter_ty {
                    Type::Array { element, .. } => *element.clone(),
                    // `range(a,b)` returns `[i64]`; a bare `a..b` infers to the
                    // `Range` struct. Both are special-cased in MIR lowering to a
                    // counter loop with an i64 element.
                    Type::Struct { name, .. } if name == "Range" => Type::I64,
                    // Unresolved (still-generic) iterables are left permissive so
                    // we never reject legitimately-inferred code -- default the
                    // element to i64 so ordinary uses in the body still check.
                    Type::Var(_) => Type::I64,
                    // An already-errored iterable (e.g. `[dyn Trait]`, rejected by
                    // `reject_dyn_in_container` with E0110) must propagate the
                    // poison as the element type too, NOT default to i64: defaulting
                    // to i64 here doesn't avoid a second diagnostic, it just delays
                    // it to the first method/field use inside the loop body, which
                    // then reports a confusing, unrelated "no method `foo` found for
                    // type `i64`" alongside the real E0110 -- the original error
                    // already told the user everything they need to know.
                    Type::Error => Type::Error,
                    // Every other concrete type (str, map, set, scalars, tuples,
                    // non-Range structs/enums, ...) is NOT iterable by the array
                    // desugar: it read the value's bytes as an array header and
                    // SEGFAULTED at runtime (e.g. `for c in some_string`). Reject
                    // at type-check with an actionable message instead of crashing.
                    other => {
                        self.error(
                            format!(
                                "cannot iterate over a value of type `{other}`; \
                                 `for x in ...` requires an array `[T]` or a range. \
                                 To iterate a string, split it into an array first \
                                 (e.g. `split(s, \" \")`, `split_lines(s)`) or index \
                                 characters with `substr(s, i, i + 1)`. \
                                 To iterate a map, loop over `keys(m)`."
                            ),
                            iterable.span(),
                        );
                        Type::I64
                    }
                };

                self.env.push_scope();

                // Bind the loop pattern into scope. Was Ident-only, so
                // `for (a, b) in pairs` left a/b undefined (E0102); route
                // through bind_pattern to destructure tuples/structs/enums.
                self.bind_pattern(pattern, &elem_ty);

                self.check_block(body);
                self.env.pop_scope();
            }
            Stmt::Expr { expr, span } => {
                // LEDGER item 42, second half. A `comptime` block in STATEMENT
                // position is meaningless and silently so: `comptime` is not a
                // compile-time evaluator, MIR lowering keeps only the block's
                // VALUE, and in statement position that value is discarded --
                // so the whole block evaporates. Measured 2026-08-14:
                // `comptime { println("INSIDE") }` emits NO println into MIR at
                // all; only the surrounding code survives. The user sees nothing
                // and reasonably concludes the block did not run. It did not,
                // but nothing said so.
                //
                // The value form (`let x = comptime { 6 * 7 }`) is the supported
                // shape and is unaffected; it is what all 9 real uses in this
                // repo do.
                if matches!(expr, Expr::ComptimeBlock { .. }) {
                    self.error_with_code(
                        "a `comptime` block in statement position does nothing -- `comptime` is not a compile-time evaluator yet, so only its VALUE is kept and here that value is discarded, silently dropping the whole block (measured: a `println` inside one never reaches codegen). Bind it (`let x = comptime { .. }`) or remove the `comptime` keyword to run the body normally",
                        *span,
                        kryos_errors::codes::E0110,
                    );
                }
                let ty = self.infer_expr(expr);
                // must-use lint (W0400): discarding a `Tracked<T>` value as a
                // statement silently drops its provenance/lineage chain. The
                // tracked stdlib fns are functional (each returns a NEW Tracked),
                // so ignoring the result loses the audit trail. A non-void fn must
                // `return`, so a bare `Stmt::Expr` of Tracked type is always a real
                // discard, never an implicit return. `tracked_discard(t, reason)`
                // is the explicit opt-out (it returns the inner, non-Tracked value).
                //
                // Known limitation: keyed on the struct NAME only (Type::Struct has no
                // module origin), so a user struct literally named `Tracked` would also
                // trip this. Acceptable for v1 -- the name is distinctive and this is a
                // warning, not an error. A module-origin guard is the proper fix later.
                if matches!(&ty, Type::Struct { name, .. } if name == "Tracked") {
                    self.diagnostics.push(
                        Diagnostic::warning(
                            "tracked value is discarded; its provenance/lineage is lost. \
                             Use the value, or call `tracked_discard(t, reason)` to drop it explicitly.",
                        )
                        .with_label(*span, "tracked value dropped here")
                        .with_code(kryos_errors::codes::W0400),
                    );
                }
            }
            Stmt::Throw { expr, .. } => {
                let ty = self.infer_expr(expr);
                let resolved = self.engine.resolve(&ty);
                // The thrown value is stringified at the throw site and `catch`
                // always binds a `str`. Codegen only stringifies str/bool/char/
                // int/float; an aggregate (struct/enum/array/map/set/tuple/option/
                // result/fn) was coerced to a raw pointer and concatenated as
                // GARBAGE. Reject those so a rich error type fails at compile time
                // rather than producing a corrupt catch value at runtime.
                if matches!(
                    resolved,
                    Type::Struct { .. }
                        | Type::Enum { .. }
                        | Type::Array { .. }
                        | Type::Tuple { .. }
                        | Type::Map { .. }
                        | Type::Set { .. }
                        | Type::Option { .. }
                        | Type::Result { .. }
                        | Type::Function { .. }
                ) {
                    self.error(
                        format!(
                            "cannot throw a value of type `{resolved}`: the thrown value is stringified and `catch` binds a `str`, but this type has no string form -- build an error string first (e.g. `throw \"...\" + to_string(x)`)"
                        ),
                        expr.span(),
                    );
                }
            }
            Stmt::TryCatch {
                try_block,
                catch_name,
                catch_block,
                ..
            } => {
                self.env.push_scope();
                self.check_block(try_block);
                self.env.pop_scope();
                self.env.push_scope();
                self.env.define_var(catch_name.clone(), Type::Str);
                self.check_block(catch_block);
                self.env.pop_scope();
            }
            Stmt::Spawn { expr, .. } => {
                self.infer_expr(expr);
            }
            Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::Select { .. } => {}
            // deny!(...) { body } is a compile-time capability-narrowing wrapper;
            // for type-checking it is just a scoped block of statements.
            Stmt::DenyBlock { denied, body, span } => {
                // STAGE 2 ENFORCEMENT. `deny!(fs:read) { .. }` narrows authority
                // for its body, and until now the type checker treated it as an
                // ordinary scoped block -- rows were INFERRED for the body and
                // then thrown away, because stage 1 computed and reported but
                // never emitted a diagnostic (`dump_fn_effects_report` has no
                // caller anywhere in the tree).
                //
                // Checking it HERE, on the accumulated row, is what the
                // shape-directed checker in `kryos-capabilities` could never do:
                // the row is charged from the CALLEE'S OWN TYPE at every call
                // site, so a callee reached through a tuple element, an
                // accessor-call receiver, an `if`/`match` receiver, a `&`/`*`
                // indirection or a struct field is charged identically to a
                // direct call. There is no expression shape to enumerate and
                // therefore no shape to miss.
                self.env.push_scope();
                self.cap_accum_stack.push(crate::ty::CapRow::empty());
                self.check_block(body);
                let body_caps = self
                    .cap_accum_stack
                    .pop()
                    .unwrap_or_else(crate::ty::CapRow::empty);
                let resolved = self.engine.resolve_cap_row(&body_caps);

                // Lattice-aware, per `CapBits::contains_bits`' own warning: a
                // raw bit test would let a declared coarse `io` slip past a
                // denied `fs:read`. Route both sides through `Capability`.
                let mut used = kryos_capabilities::model::CapabilitySet::empty();
                for name in resolved.concrete_bits().names() {
                    if let Some(c) = kryos_capabilities::model::Capability::from_str(name) {
                        used.insert(c);
                    }
                }
                for d in denied {
                    let Some(dcap) = kryos_capabilities::model::Capability::from_str(d) else {
                        continue;
                    };
                    if used.satisfies_required(&dcap) {
                        self.error(
                            format!(
                                "capability `{d}` is denied in this block, but the block uses it"
                            ),
                            *span,
                        );
                    }
                }

                // The body's authority still HAPPENED -- propagate it outward so
                // an enclosing function's own row stays honest.
                self.accumulate_caps(&body_caps);
                self.env.pop_scope();
            }
        }
    }

    // ── Pattern binding ──────────────────────────────────────────────

    /// Bind variables introduced by a pattern into the current scope.
    ///
    /// For example, `Option::Some(val)` binds `val` to a fresh type variable.
    /// `Ident` patterns bind the name to the subject type.
    /// Wildcards and literals bind nothing.
    ///
    /// `outer_mut` is true when the binding site (`let mut (...)` /
    /// match arm under a `mut` binding) declared the whole pattern
    /// mutable.  Per-element `mut` inside the pattern
    /// (`let (mut a, b) = ...`) is also honored - the binding is
    /// mutable if either source says so.
    /// In a MATCH ARM, a bare CAPITALIZED identifier is a variant TAG test
    /// (`Red`, `Some`), not a binding -- Kryos bindings are lowercase by
    /// convention (as in Rust). If the subject is a known enum/Option/Result
    /// and the capitalized name is NOT one of its variants, it is a typo
    /// (`Bluee`) that `bind_pattern` would otherwise silently accept as a
    /// catch-all binding -- masking the intended arm and making the match
    /// spuriously exhaustive. Report it with a did-you-mean, matching the
    /// qualified-variant and struct-field-typo diagnostics. Recurses into
    /// or-pattern alternatives (`Red | Bluee`) and tuple elements so a typo in
    /// any position is caught; lowercase idents (real bindings) are untouched.
    fn check_arm_variant_typo(&mut self, pattern: &Pattern, subject_ty: &Type) {
        match pattern {
            Pattern::Ident { name, span, .. } => {
                if name == "_" || !name.chars().next().is_some_and(|c| c.is_uppercase()) {
                    return;
                }
                let resolved = self.engine.resolve(subject_ty);
                let (variants, ty_label) = match &resolved {
                    Type::Enum { name: en, .. } => match self.env.lookup_enum(en) {
                        Some(d) => (
                            d.variants.iter().map(|(v, _)| v.clone()).collect::<Vec<_>>(),
                            format!(" on type `{en}`"),
                        ),
                        None => return,
                    },
                    Type::Option { .. } => {
                        (vec!["Some".to_string(), "None".to_string()], " on `Option`".to_string())
                    }
                    Type::Result { .. } => {
                        (vec!["Ok".to_string(), "Err".to_string()], " on `Result`".to_string())
                    }
                    _ => return,
                };
                if !variants.iter().any(|v| v == name) {
                    // Distinguish a TYPO from an intentional capitalized catch-
                    // all binding. A typo is CLOSE (small edit distance) to a
                    // real variant; report only then. A capitalized ident far
                    // from every variant -- or one that names a real
                    // struct/enum (a binding named after a type, as some
                    // examples do) -- is an intentional binding, so leave it to
                    // `bind_pattern`. Without this guard a legitimate catch-all
                    // like `Rect => ..` on a `Shape` value (variants Circle /
                    // Rectangle) was wrongly rejected.
                    let suggestion =
                        crate::suggest::closest_match(name, variants.iter().map(|s| s.as_str()));
                    let is_known_type =
                        self.env.lookup_struct(name).is_some() || self.env.lookup_enum(name).is_some();
                    if let Some(s) = suggestion {
                        if !is_known_type {
                            self.error_with_code(
                                format!("unknown variant `{name}`{ty_label}"),
                                *span,
                                kryos_errors::codes::E0102,
                            );
                            if let Some(diag) = self.diagnostics.last_mut() {
                                diag.notes.push(format!("did you mean `{s}`?"));
                            }
                        }
                    }
                }
            }
            Pattern::Or { patterns, .. } => {
                for alt in patterns {
                    self.check_arm_variant_typo(alt, subject_ty);
                }
            }
            Pattern::Tuple { elements, .. } => {
                let resolved = self.engine.resolve(subject_ty);
                if let Type::Tuple { elements: ts } = &resolved {
                    for (i, elem) in elements.iter().enumerate() {
                        if let Some(et) = ts.get(i) {
                            self.check_arm_variant_typo(elem, et);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn bind_pattern(&mut self, pattern: &Pattern, subject_ty: &Type) {
        let fresh_dup_scope = self.pattern_dup_seen.is_none();
        if fresh_dup_scope {
            self.pattern_dup_seen = Some(std::collections::HashMap::new());
        }
        self.bind_pattern_with_mut(pattern, subject_ty, false);
        if fresh_dup_scope {
            self.pattern_dup_seen = None;
        }
    }

    fn bind_pattern_with_mut(
        &mut self,
        pattern: &Pattern,
        subject_ty: &Type,
        outer_mut: bool,
    ) {
        match pattern {
            Pattern::Wildcard { .. } => {}
            Pattern::Literal { expr, span } => {
                // A match-arm integer literal is compared against the
                // scrutinee at the scrutinee's WIDTH, so an out-of-range
                // literal was silently truncated: `match x { 999 => .. }` on a
                // `u8` scrutinee truncated 999 to 231 and could fire the wrong
                // arm (231 == 231) while the source said 999. Range-check the
                // literal against the scrutinee type, exactly like a narrow
                // let/assign (E0111).
                self.check_int_literal_range(subject_ty, expr, *span);
            }
            Pattern::Ident {
                name,
                mutable,
                span,
            } => {
                // Duplicate-binding detection: record every GENUINE binding
                // in the top-level pattern's map and reject a repeat
                // (`let (a, a) = ..`, `P(x, x) => ..`) -- the last duplicate
                // silently won. A bare ident that names a VARIANT of the
                // subject's enum type is a tag TEST, not a binding
                // (`(Red, Red)` is legal), so it is excluded; an UNRESOLVED
                // subject (struct-pattern fields bind through fresh vars)
                // can't be ruled out as a variant test, so it is skipped
                // rather than risk rejecting valid code.
                if self.pattern_dup_seen.is_some() {
                    let resolved = self.engine.resolve(subject_ty);
                    let skip = match &resolved {
                        Type::Enum { name: en, .. } => self
                            .env
                            .lookup_enum(en)
                            .map(|d| d.variants.iter().any(|(v, _)| v == name))
                            .unwrap_or(false),
                        Type::Option { .. } => name == "Some" || name == "None",
                        Type::Result { .. } => name == "Ok" || name == "Err",
                        Type::Var(_) => true,
                        _ => false,
                    };
                    let dup = if skip {
                        false
                    } else if let Some(seen) = self.pattern_dup_seen.as_mut() {
                        seen.insert(name.clone(), *span).is_some()
                    } else {
                        false
                    };
                    if dup {
                        self.error(
                            format!(
                                "identifier `{name}` is bound more than once in the same pattern"
                            ),
                            *span,
                        );
                    }
                }
                if outer_mut || *mutable {
                    self.env.define_var_mut(name.clone(), subject_ty.clone());
                } else {
                    self.env.define_var(name.clone(), subject_ty.clone());
                }
            }
            Pattern::Tuple { elements, span } => {
                // Resolve the subject type so we can extract element types.
                let resolved = self.engine.resolve(subject_ty);
                let elem_tys: Vec<Type> = if let Type::Tuple { elements: ts } = &resolved {
                    ts.clone()
                } else {
                    vec![]
                };
                // Arity check: a tuple pattern must bind EXACTLY as many
                // elements as the (concrete) tuple type has. Without this,
                // over-binding (`let (a,b,c) = (1,"x")`) passed the checker and
                // panicked at runtime with the internal array-OOB message, and
                // under-binding silently dropped the trailing elements. Only
                // fires when the subject is a concrete tuple (a fresh/unknown
                // type var is left to later unification).
                if let Type::Tuple { elements: ts } = &resolved {
                    if ts.len() != elements.len() {
                        self.error_with_code(
                            format!(
                                "tuple pattern binds {} element{} but the value is a {}-tuple",
                                elements.len(),
                                if elements.len() == 1 { "" } else { "s" },
                                ts.len()
                            ),
                            *span,
                            kryos_errors::codes::E0100,
                        );
                    }
                }
                for (i, elem) in elements.iter().enumerate() {
                    let elem_ty = elem_tys
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| self.engine.fresh_var());
                    self.bind_pattern_with_mut(elem, &elem_ty, outer_mut);
                }
            }
            Pattern::Struct { fields, .. } => {
                for (_field_name, pat) in fields {
                    let tv = self.engine.fresh_var();
                    self.bind_pattern_with_mut(pat, &tv, outer_mut);
                }
            }
            Pattern::Enum {
                name,
                variant,
                fields,
                span: enum_pat_span,
            } => {
                // Resolve the subject first so type variables don't slip through.
                let resolved_subject = self.engine.resolve(subject_ty);
                // Option<T> / Result<T,E> are DISTINCT Type variants from
                // Type::Enum - derive their payload types directly from the
                // generic arguments. Without this they fall to the enum-name
                // lookup below with name "", binding the payload to a fresh
                // ?Tn (so `Some(u) => u.field` failed to type-check).
                let field_types: Vec<Type> = match (&resolved_subject, variant.as_str()) {
                    (Type::Option { inner }, "Some") => vec![(**inner).clone()],
                    (Type::Option { .. }, "None") => vec![],
                    (Type::Result { ok, .. }, "Ok") => vec![(**ok).clone()],
                    (Type::Result { err, .. }, "Err") => vec![(**err).clone()],
                    // Cross-type mismatches. These previously fell to the enum
                    // fallback, found no user enum named Result/Option, and bound
                    // the payload to a fresh var with NO diagnostic. The `?`
                    // operator desugars to a `Result.Ok/Err` match, so applying
                    // `?` to an Option value hit exactly this silent hole (an
                    // Option matched against Result patterns -> type confusion).
                    (Type::Option { .. }, "Ok") | (Type::Option { .. }, "Err") => {
                        self.error(
                            format!(
                                "pattern `{variant}` matches a `Result`, but this value is an `Option` -- use `Some`/`None` (note: `?` desugars to a `Result` match, so it cannot be applied to an `Option`)"
                            ),
                            *enum_pat_span,
                        );
                        vec![]
                    }
                    (Type::Result { .. }, "Some") | (Type::Result { .. }, "None") => {
                        self.error(
                            format!(
                                "pattern `{variant}` matches an `Option`, but this value is a `Result` -- use `Ok`/`Err`"
                            ),
                            *enum_pat_span,
                        );
                        vec![]
                    }
                    _ => {
                        // An enum pattern against a CONCRETE non-enum subject
                        // is a type error, full stop. This previously fell to
                        // the lookup below, found nothing, and silently bound
                        // the payload to a fresh var -- so `parse_int(s)?`
                        // (`?` desugars to a Result match; parse_int returns
                        // a plain i64) type-checked with only a stray
                        // non-exhaustive-match WARNING, ran under the JIT, and
                        // crashed the AOT build with invalid LLVM IR
                        // (`extractvalue` on a scalar). Var/Never/Error stay
                        // exempt (inference in progress / diverging /
                        // already-reported), as do reference-like wrappers.
                        let subject_is_concrete_non_enum = matches!(
                            &resolved_subject,
                            Type::I8
                                | Type::I16
                                | Type::I32
                                | Type::I64
                                | Type::I128
                                | Type::U8
                                | Type::U16
                                | Type::U32
                                | Type::U64
                                | Type::U128
                                | Type::F32
                                | Type::F64
                                | Type::Bool
                                | Type::Char
                                | Type::Str
                                | Type::USize
                                | Type::ISize
                                | Type::Void
                                | Type::Array { .. }
                                | Type::Tuple { .. }
                                | Type::Map { .. }
                                | Type::Set { .. }
                                | Type::Struct { .. }
                                | Type::Function { .. }
                        );
                        if subject_is_concrete_non_enum {
                            let is_try_desugar = name == "Result"
                                && fields.iter().any(|f| {
                                    matches!(f, Pattern::Ident { name, .. }
                                        if name.starts_with("__kry_try_"))
                                });
                            if is_try_desugar {
                                self.error_with_code(
                                    format!(
                                        "`?` requires a `Result` value, but this expression has type `{resolved_subject}` -- only apply `?` to `Result`-returning calls (wrap fallible plain calls in try/catch instead)"
                                    ),
                                    *enum_pat_span,
                                    kryos_errors::codes::E0100,
                                );
                            } else {
                                let shown = if name.is_empty() {
                                    variant.clone()
                                } else {
                                    format!("{name}.{variant}")
                                };
                                self.error_with_code(
                                    format!(
                                        "pattern `{shown}` matches an enum variant, but this value has type `{resolved_subject}`"
                                    ),
                                    *enum_pat_span,
                                    kryos_errors::codes::E0100,
                                );
                            }
                            for pat in fields.iter() {
                                let fresh = self.engine.fresh_var();
                                self.bind_pattern_with_mut(pat, &fresh, outer_mut);
                            }
                            return;
                        }
                        // Bare (unqualified) variant patterns carry an empty
                        // enum name; resolve it from the matched subject type
                        // when it is an enum.
                        let resolved_name = if name.is_empty() {
                            match &resolved_subject {
                                Type::Enum { name: n, .. } => n.clone(),
                                _ => name.clone(),
                            }
                        } else {
                            name.clone()
                        };
                        // Look up the enum variant's field types by variant name.
                        if let Some(edef) = self.env.lookup_enum(&resolved_name).cloned() {
                            let raw_field_types: Vec<Type> = edef
                                .variants
                                .iter()
                                .find(|(v, _)| v == variant)
                                .map(|(_, tys)| tys.clone())
                                .unwrap_or_default();
                            // The declared field types reference the enum's
                            // SHARED generic_var_ids, allocated ONCE at
                            // `enum Foo<T> { .. }` declaration time. Binding
                            // the pattern payload straight from
                            // `raw_field_types` (as before) reused those
                            // shared var ids across EVERY match in the
                            // program: matching a `Maybe<i64>` local bound
                            // the shared var to i64 for good, so a later
                            // match on a `Maybe<str>` local unified its `str`
                            // payload against the same already-i64-bound var
                            // and was wrongly rejected (E0100). Mirror the
                            // fresh-per-use instantiation already used for
                            // enum-variant CONSTRUCTION (the MethodCall /
                            // StaticMethodCall variant-constructor arms
                            // above) and for generic methods/static fns
                            // (`instantiate_sig`): allocate fresh vars for
                            // THIS match, substitute them into the field
                            // types, then bind them to the scrutinee's
                            // already-known concrete type arguments (e.g.
                            // `mi: Maybe<i64>` resolves to
                            // `Type::Enum{generics: [i64]}`) via unify --
                            // never touching the shared declaration var ids.
                            if edef.generic_var_ids.is_empty() {
                                raw_field_types
                            } else {
                                let mut var_map = std::collections::HashMap::new();
                                let mut fresh_generics =
                                    Vec::with_capacity(edef.generic_var_ids.len());
                                for &old_id in &edef.generic_var_ids {
                                    let fresh = self.engine.fresh_var();
                                    if let Type::Var(new_id) = &fresh {
                                        var_map.insert(old_id, *new_id);
                                    }
                                    fresh_generics.push(fresh);
                                }
                                let instantiated: Vec<Type> = raw_field_types
                                    .iter()
                                    .map(|t| self.engine.instantiate(t, &var_map))
                                    .collect();
                                if let Type::Enum {
                                    generics: concrete, ..
                                } = &resolved_subject
                                {
                                    if concrete.len() == fresh_generics.len() {
                                        for (fresh, conc) in
                                            fresh_generics.iter().zip(concrete.iter())
                                        {
                                            let _ =
                                                self.engine.unify(fresh, conc, *enum_pat_span);
                                        }
                                    }
                                }
                                instantiated
                            }
                        } else {
                            vec![]
                        }
                    }
                };

                // Arity: an enum-variant pattern must bind exactly as many
                // sub-patterns as the variant has payload fields. Over-binding
                // (`Circle(x, y)` on `Circle(i64)`) previously read
                // uninitialized memory for the phantom fields (nondeterministic
                // garbage), and under-binding silently dropped fields. Only
                // enforced when the variant's arity is authoritatively known
                // (concrete Option/Result/Enum subject + resolved variant); a
                // still-unresolved subject is left to later unification.
                let expected_arity: Option<usize> = match (&resolved_subject, variant.as_str()) {
                    (Type::Option { .. }, "Some") => Some(1),
                    (Type::Option { .. }, "None") => Some(0),
                    (Type::Result { .. }, "Ok") | (Type::Result { .. }, "Err") => Some(1),
                    _ => {
                        let rname = if name.is_empty() {
                            match &resolved_subject {
                                Type::Enum { name: n, .. } => n.clone(),
                                _ => String::new(),
                            }
                        } else {
                            name.clone()
                        };
                        self.env.lookup_enum(&rname).and_then(|d| {
                            d.variants
                                .iter()
                                .find(|(v, _)| v == variant)
                                .map(|(_, tys)| tys.len())
                        })
                    }
                };
                if let Some(n) = expected_arity {
                    if n != fields.len() {
                        self.error_with_code(
                            format!(
                                "enum variant `{variant}` binds {} field{} but the variant has {}",
                                fields.len(),
                                if fields.len() == 1 { "" } else { "s" },
                                n
                            ),
                            *enum_pat_span,
                            kryos_errors::codes::E0100,
                        );
                    }
                }
                for (i, pat) in fields.iter().enumerate() {
                    let field_ty = field_types
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| self.engine.fresh_var());
                    self.bind_pattern_with_mut(pat, &field_ty, outer_mut);
                }
            }
            Pattern::Or { patterns, .. } => {
                // Or-pattern alternatives must be NON-BINDING (literals or bare
                // enum variants) -- CLAUDE.md gotcha #14. A binding alternative
                // was silently accepted and each alt overwrote the binding with
                // its OWN field type: `Num(x) | Label(x)` (i64 vs str) produced
                // type confusion, and `Pos(x) | Neg(y)` bound both names so the
                // non-matching path read an uninitialized value. Reject them.
                // A bare IDENT alternative parses as Pattern::Ident whether it
                // is a genuine binding (`x | y` -- illegal) or a nullary
                // variant tag test (`Red | Green` -- the documented legal
                // form). pattern_is_binding can't tell them apart without type
                // context, and classifying every Ident as binding rejected the
                // exact syntax this error's own message recommends. Resolve
                // the ident against the SUBJECT's type: a variant of its enum
                // (or Some/None/Ok/Err for Option/Result) is a tag test, not
                // a binding.
                let resolved_subject = self.engine.resolve(subject_ty);
                for alt in patterns.iter() {
                    let is_variant_test = if let Pattern::Ident { name, .. } = alt {
                        match &resolved_subject {
                            Type::Enum { name: en, .. } => self
                                .env
                                .lookup_enum(en)
                                .map(|d| d.variants.iter().any(|(v, _)| v == name))
                                .unwrap_or(false),
                            Type::Option { .. } => name == "Some" || name == "None",
                            Type::Result { .. } => name == "Ok" || name == "Err",
                            _ => false,
                        }
                    } else {
                        false
                    };
                    if !is_variant_test && pattern_is_binding(alt) {
                        self.error(
                            "or-pattern alternatives must be non-binding: use literals (`1 | 2`) or bare enum variants (`Red | Green`); a pattern that binds a variable is not allowed here because alternatives may bind different names or types"
                                .to_string(),
                            alt.span(),
                        );
                    }
                }
                for pat in patterns {
                    self.bind_pattern_with_mut(pat, subject_ty, outer_mut);
                }
            }
        }
    }

    // (free-fn `pattern_is_binding` is defined at module scope below)

    // ── Expression type inference ────────────────────────────────────

    /// Infer the type of an expression.
    /// If `expr` is an integer literal (or negated literal) and `declared`
    /// resolves to a narrow integer type, reject values outside the type's
    /// range. Without this, `let x: u8 = 999` silently truncated to 231 - 
    /// silent data corruption at the source level. Explicit casts
    /// (`999 as u8`) keep their documented truncation semantics.
    fn check_int_literal_range(&mut self, declared: &Type, expr: &Expr, span: Span) {
        let (min, max): (i128, i128) = match self.engine.resolve(declared) {
            Type::I8 => (i8::MIN as i128, i8::MAX as i128),
            Type::I16 => (i16::MIN as i128, i16::MAX as i128),
            Type::I32 => (i32::MIN as i128, i32::MAX as i128),
            Type::U8 => (0, u8::MAX as i128),
            Type::U16 => (0, u16::MAX as i128),
            Type::U32 => (0, u32::MAX as i128),
            Type::U64 => (0, u64::MAX as i128),
            _ => return,
        };
        // Unsigned targets have min == 0; only they may hold values above
        // i64::MAX. A bare integer literal is stored as an i64 bit-pattern
        // (the parser bit-casts u64 values above i64::MAX to a negative i64),
        // so for an unsigned target reinterpret those bits as unsigned - 
        // otherwise u64::MAX reads back as -1 and is wrongly rejected. A
        // NEGATED literal (`-5`) stays negative and is still rejected for
        // unsigned types, as intended.
        let unsigned_target = min == 0;
        let value: Option<i128> = match expr {
            Expr::IntLiteral { value, .. } => {
                if unsigned_target && *value < 0 {
                    Some(*value as u64 as i128)
                } else {
                    Some(*value as i128)
                }
            }
            Expr::UnaryOp {
                op: UnOp::Neg,
                operand,
                ..
            } => match operand.as_ref() {
                Expr::IntLiteral { value, .. } => Some(-(*value as i128)),
                _ => None,
            },
            _ => None,
        };
        if let Some(v) = value {
            if v < min || v > max {
                let ty_name = format!("{:?}", self.engine.resolve(declared)).to_lowercase();
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "integer literal `{v}` is out of range for `{ty_name}` (valid range: {min}..={max})"
                    ))
                    .with_label(span, "out-of-range literal")
                    .with_note(format!(
                        "use a wider type, or `as {ty_name}` if truncation is intended"
                    ))
                    .with_code(kryos_errors::codes::E0111),
                );
            }
        }
    }

    /// Substitute generic-param type variables in a struct field's type with
    /// the instance's concrete generics (StructDef.generic_var_ids are 1:1
    /// with the decl's generic params; instance generics are positional).
    /// `Boxed<str>.value` resolves to `str`, not the registration-time var.
    fn substitute_struct_generics(&mut self, name: &str, instance_generics: &[Type], fty: &Type) -> Type {
        if instance_generics.is_empty() {
            return fty.clone();
        }
        let Some(def) = self.env.lookup_struct(name) else {
            return fty.clone();
        };
        if def.generic_var_ids.is_empty() {
            return fty.clone();
        }
        let map: std::collections::HashMap<u32, Type> = def
            .generic_var_ids
            .iter()
            .copied()
            .zip(instance_generics.iter().cloned())
            .collect();
        fn walk(t: &Type, map: &std::collections::HashMap<u32, Type>) -> Type {
            match t {
                Type::Var(id) => map.get(id).cloned().unwrap_or_else(|| t.clone()),
                Type::Array { element, size } => Type::Array {
                    element: Box::new(walk(element, map)),
                    size: *size,
                },
                Type::Tuple { elements } => Type::Tuple {
                    elements: elements.iter().map(|e| walk(e, map)).collect(),
                },
                Type::Map { key, value } => Type::Map {
                    key: Box::new(walk(key, map)),
                    value: Box::new(walk(value, map)),
                },
                Type::Option { inner } => Type::Option {
                    inner: Box::new(walk(inner, map)),
                },
                Type::Result { ok, err } => Type::Result {
                    ok: Box::new(walk(ok, map)),
                    err: Box::new(walk(err, map)),
                },
                Type::Struct { name, generics } => Type::Struct {
                    name: name.clone(),
                    generics: generics.iter().map(|g| walk(g, map)).collect(),
                },
                Type::Enum { name, generics } => Type::Enum {
                    name: name.clone(),
                    generics: generics.iter().map(|g| walk(g, map)).collect(),
                },
                Type::Function { params, ret, caps } => Type::Function {
                    params: params.iter().map(|p| walk(p, map)).collect(),
                    ret: Box::new(walk(ret, map)),
                    // Capability row is untouched by ORDINARY generic
                    // substitution (it has nothing to do with `T`/`U`) --
                    // carried through unchanged, same as e.g. a field's
                    // `mutable`/`size` flag on the arms around this one.
                    caps: caps.clone(),
                },
                Type::Reference { inner, mutable } => Type::Reference {
                    inner: Box::new(walk(inner, map)),
                    mutable: *mutable,
                },
                Type::Shared { inner } => Type::Shared {
                    inner: Box::new(walk(inner, map)),
                },
                other => other.clone(),
            }
        }
        walk(fty, &map)
    }

    /// Enum sibling of `substitute_struct_generics`: given an enum's declared
    /// variant-field type `fty` (which may reference the enum's own generic
    /// type-variable ids) and the concrete `instance_generics` a particular
    /// value was constructed/annotated with, substitute the type variables
    /// with their concrete bindings.
    fn substitute_enum_generics(&mut self, name: &str, instance_generics: &[Type], fty: &Type) -> Type {
        if instance_generics.is_empty() {
            return fty.clone();
        }
        let Some(def) = self.env.lookup_enum(name) else {
            return fty.clone();
        };
        if def.generic_var_ids.is_empty() {
            return fty.clone();
        }
        let map: std::collections::HashMap<u32, Type> = def
            .generic_var_ids
            .iter()
            .copied()
            .zip(instance_generics.iter().cloned())
            .collect();
        fn walk(t: &Type, map: &std::collections::HashMap<u32, Type>) -> Type {
            match t {
                Type::Var(id) => map.get(id).cloned().unwrap_or_else(|| t.clone()),
                Type::Array { element, size } => Type::Array {
                    element: Box::new(walk(element, map)),
                    size: *size,
                },
                Type::Tuple { elements } => Type::Tuple {
                    elements: elements.iter().map(|e| walk(e, map)).collect(),
                },
                Type::Map { key, value } => Type::Map {
                    key: Box::new(walk(key, map)),
                    value: Box::new(walk(value, map)),
                },
                Type::Option { inner } => Type::Option {
                    inner: Box::new(walk(inner, map)),
                },
                Type::Result { ok, err } => Type::Result {
                    ok: Box::new(walk(ok, map)),
                    err: Box::new(walk(err, map)),
                },
                Type::Struct { name, generics } => Type::Struct {
                    name: name.clone(),
                    generics: generics.iter().map(|g| walk(g, map)).collect(),
                },
                Type::Enum { name, generics } => Type::Enum {
                    name: name.clone(),
                    generics: generics.iter().map(|g| walk(g, map)).collect(),
                },
                Type::Function { params, ret, caps } => Type::Function {
                    params: params.iter().map(|p| walk(p, map)).collect(),
                    ret: Box::new(walk(ret, map)),
                    // Capability row is untouched by ORDINARY generic
                    // substitution (it has nothing to do with `T`/`U`) --
                    // carried through unchanged, same as e.g. a field's
                    // `mutable`/`size` flag on the arms around this one.
                    caps: caps.clone(),
                },
                Type::Reference { inner, mutable } => Type::Reference {
                    inner: Box::new(walk(inner, map)),
                    mutable: *mutable,
                },
                Type::Shared { inner } => Type::Shared {
                    inner: Box::new(walk(inner, map)),
                },
                other => other.clone(),
            }
        }
        walk(fty, &map)
    }

    /// True if `ty` is, or (recursively, through struct/enum fields/variant
    /// payloads and `Option`/`Result`/tuple wrappers) contains, an array or
    /// map type. Used to reject `==`/`!=` on a struct/enum whose structural
    /// comparison is not yet implemented for array/map-shaped fields (see
    /// the `BinOp::Eq | BinOp::Neq` arm in `check_binary_op`), rather than
    /// let it through to codegen where it previously either silently
    /// compared handles (Cranelift JIT) or failed the AOT LLVM build.
    /// `visited` guards against infinite recursion on a self-referential
    /// struct/enum definition.
    fn contains_array_or_map(
        &mut self,
        ty: &Type,
        visited: &mut std::collections::HashSet<String>,
    ) -> bool {
        let ty = self.engine.resolve(ty);
        match &ty {
            Type::Array { .. } | Type::Map { .. } => true,
            Type::Option { inner } => self.contains_array_or_map(inner, visited),
            Type::Result { ok, err } => {
                self.contains_array_or_map(ok, visited) || self.contains_array_or_map(err, visited)
            }
            Type::Tuple { elements } => elements
                .iter()
                .any(|e| self.contains_array_or_map(e, visited)),
            Type::Struct { name, generics } => {
                if !visited.insert(format!("struct:{name}")) {
                    return false;
                }
                let Some(def) = self.env.lookup_struct(name).cloned() else {
                    return false;
                };
                let generics = generics.clone();
                for (_, fty) in def.fields {
                    let substituted = self.substitute_struct_generics(name, &generics, &fty);
                    if self.contains_array_or_map(&substituted, visited) {
                        return true;
                    }
                }
                false
            }
            Type::Enum { name, generics } => {
                if !visited.insert(format!("enum:{name}")) {
                    return false;
                }
                let Some(def) = self.env.lookup_enum(name).cloned() else {
                    return false;
                };
                let generics = generics.clone();
                for (_, fields) in def.variants {
                    for fty in fields {
                        let substituted = self.substitute_enum_generics(name, &generics, &fty);
                        if self.contains_array_or_map(&substituted, visited) {
                            return true;
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// The stdlib bridge enums `Result`/`Option` are declared non-generic
    /// with `any` payloads; their constructors used to produce a bare
    /// `Enum{Result, []}` that unified with ANY `Result<T, E>` annotation,
    /// so `fn f() -> Result<i64, str> { return Err(42) }` type-checked
    /// clean and then segfaulted when the i64 payload was read through the
    /// str-typed match binding (backlog #13/#19). Synthesize payload-bearing
    /// generics from the constructed variant so the annotation bridge in
    /// `unify` can check them. The un-constrained slot gets a fresh var.
    fn stdlib_bridge_generics(
        &mut self,
        enum_name: &str,
        variant: &str,
        arg_tys: &[Type],
    ) -> Vec<Type> {
        let payload = arg_tys.first().cloned().unwrap_or(Type::Error);
        match (enum_name, variant) {
            ("Result", "Ok") => vec![payload, self.engine.fresh_var()],
            ("Result", "Err") => vec![self.engine.fresh_var(), payload],
            ("Option", "Some") => vec![payload],
            ("Option", "None") => vec![self.engine.fresh_var()],
            _ => Vec::new(),
        }
    }

    pub fn infer_expr(&mut self, expr: &Expr) -> Type {
        match expr {
            // Literals.
            Expr::IntLiteral { .. } => Type::I64,
            Expr::FloatLiteral { .. } => Type::F64,
            Expr::StringLiteral { .. } => Type::Str,
            Expr::InterpolatedString { parts, .. } => {
                // Type-check every interpolated expression. Previously the
                // parts were never visited, so an undefined identifier in
                // `"{...}"` passed `kryos check` and ran as 0 (backlog #23).
                for part in parts {
                    if let kryos_ast::StringPart::Expr(e) = part {
                        let ty = self.infer_expr(e);
                        // Only scalars and `str` have a built-in string form.
                        // Interpolating an AGGREGATE (struct/enum/array/tuple/
                        // map/set/Option/Result/fn) fell through to the i64/
                        // pointer path -> the raw pointer printed on JIT, and
                        // blank output / an AOT SEGFAULT on LLVM (e.g. through
                        // std::tracked's explain()/to_json()). Reject it with a
                        // clear error. (A generic `T` is an unresolved Var here
                        // and is allowed; only concrete aggregates are caught.)
                        let resolved = self.engine.resolve(&ty);
                        let interpolatable = matches!(
                            resolved,
                            Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128
                                | Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128
                                | Type::F32 | Type::F64 | Type::Bool | Type::Char | Type::Str
                                | Type::USize | Type::ISize | Type::Var(_) | Type::Error
                        );
                        if !interpolatable {
                            let desc = match &resolved {
                                Type::Struct { name, .. } => name.clone(),
                                Type::Enum { name, .. } => name.clone(),
                                Type::Array { .. } => "array".to_string(),
                                Type::Tuple { .. } => "tuple".to_string(),
                                Type::Map { .. } => "map".to_string(),
                                Type::Set { .. } => "set".to_string(),
                                Type::Option { .. } => "Option".to_string(),
                                Type::Result { .. } => "Result".to_string(),
                                Type::Function { .. } => "function".to_string(),
                                _ => "this type".to_string(),
                            };
                            self.error(
                                format!(
                                    "cannot interpolate a value of type `{desc}` into a string: it has no built-in string representation. Interpolate a field/element (e.g. `{{v.field}}` or `{{v[0]}}`) or convert explicitly with `to_string(v)` / `.to_string()`."
                                ),
                                e.span(),
                            );
                        }
                    }
                }
                Type::Str
            }
            Expr::CharLiteral { .. } => Type::Char,
            Expr::BoolLiteral { .. } => Type::Bool,
            Expr::NoneLiteral { .. } => Type::Option {
                inner: Box::new(self.engine.fresh_var()),
            },

            // Identifier lookup.
            Expr::Identifier { name, span } => {
                if let Some(ty) = self.env.lookup_var(name) {
                    ty.clone()
                } else if let Some(sig) = self.env.lookup_function(name) {
                    // Function used as a value - return its function type.
                    // For generic functions, instantiate fresh type variables so
                    // each call site gets independent type inference (prevents
                    // generic type pinning across call sites).
                    let sig = sig.clone();
                    let (params, ret, var_map, cap_var_map) = self.engine.instantiate_sig(&sig);
                    // Carry the sig's trait bounds onto the fresh call-site
                    // vars so unify can enforce them (backlog #89).
                    for (old_id, new_id) in &var_map {
                        if let Some(bounds) = self.generic_var_bounds.get(old_id).cloned() {
                            self.engine.set_var_bounds(*new_id, bounds);
                        }
                    }
                    // This reference's own row: resolve the declaration's
                    // OWN capability var (its body's inferred total, which
                    // may itself still mention one of `sig`'s
                    // `generic_cap_var_ids` -- e.g. a HOF whose total
                    // charge IS exactly its callback parameter's row) as
                    // far as currently known, THEN remap whatever's left
                    // through THIS call site's fresh `cap_var_map` -- so a
                    // row-polymorphic function's OWN charge tracks the
                    // SAME freshened var its param type uses, not the
                    // shared template. See `FunctionSig::own_cap_var`'s
                    // doc comment and `docs/capability-effects-spec.md` §4.
                    let own_row = self
                        .engine
                        .resolve_cap_row(&crate::ty::CapRow::var(sig.own_cap_var));
                    let ref_caps = self.engine.instantiate_row(&own_row, &cap_var_map);
                    Type::Function {
                        params,
                        ret: Box::new(ret),
                        caps: ref_caps,
                    }
                } else if let Some(edef) = self.env.lookup_enum(name).cloned() {
                    // Enum name used as a namespace (e.g., `Color` in `Color.Red`).
                    // Instantiate FRESH generics matching the enum's declared
                    // arity here too -- this type flows straight into the dot
                    // FieldAccess arm below for a NULLARY variant (`Box.Empty`),
                    // which used to inherit a hardcoded EMPTY generics vec no
                    // matter the enum's arity. That made `Box.Empty` type as
                    // bare `Box` (0 generics) instead of `Box<?T>`, so it could
                    // never unify with an expected concrete instantiation like
                    // `Box<i64>` (the unify arm below requires equal generics
                    // length) -- a nullary variant of a user generic enum was
                    // wrongly rejected (E0100) in a `let`/return/match context
                    // that the same-shaped `Box::Empty` (qualified path just
                    // below) and bare `Empty` (further down) already handled
                    // correctly via this same fresh-var-per-generic-param
                    // instantiation.
                    let fresh_generics: Vec<Type> = edef
                        .generic_var_ids
                        .iter()
                        .map(|_| self.engine.fresh_var())
                        .collect();
                    Type::Enum {
                        name: name.clone(),
                        generics: fresh_generics,
                    }
                } else if let Some((ename, edef)) = name.split_once("::").and_then(|(e, v)| {
                    // Qualified NULLARY enum variant, e.g. `Opt::None`. The parser
                    // emits this as an `Identifier` named "Opt::None" (the payload
                    // form `Opt::Some(7)` is a StaticMethodCall handled elsewhere).
                    self.env
                        .lookup_enum(e)
                        .filter(|d| d.variants.iter().any(|(vn, _)| vn == v))
                        .map(|d| (e.to_string(), d.clone()))
                }) {
                    let fresh_generics: Vec<Type> = edef
                        .generic_var_ids
                        .iter()
                        .map(|_| self.engine.fresh_var())
                        .collect();
                    Type::Enum {
                        name: ename,
                        generics: fresh_generics,
                    }
                } else if let Some(edef) = self.env.find_enum_by_variant(name).cloned().filter(
                    |edef| {
                        edef.variants
                            .iter()
                            .any(|(v, ftys)| v == name && ftys.is_empty())
                    },
                ) {
                    // Bare (unqualified) NULLARY enum variant used as a value, e.g.
                    // `Nil` / `Done` / `Red`. This mirrors the bare with-args
                    // constructor path in the FnCall arm (`Cons(1, t)` already
                    // resolves by unambiguous variant name) and the MIR lowering,
                    // which already resolves bare nullary variants via
                    // `find_enum_variant`. Only NULLARY variants resolve here - a
                    // with-args variant used bare without a call is not a value.
                    // Closes the inconsistency where `Cons(1, Nil)` rejected only
                    // the `Nil`, forcing recursive/tree enums to qualify every leaf.
                    let fresh_generics: Vec<Type> = edef
                        .generic_var_ids
                        .iter()
                        .map(|_| self.engine.fresh_var())
                        .collect();
                    Type::Enum {
                        name: edef.name.clone(),
                        generics: fresh_generics,
                    }
                } else {
                    self.error_with_code(
                        format!("undefined variable `{name}`"),
                        *span,
                        kryos_errors::codes::E0102,
                    );
                    // Newcomer hints for names carried over from other
                    // languages beat a Levenshtein suggestion here.
                    let newcomer_note: Option<&str> = match name.as_str() {
                        "null" | "nil" | "undefined" | "NULL" | "nullptr" => Some(
                            "Kryos has no null -- use `Option<T>` from std::option (`Some(x)` / `None()`)",
                        ),
                        "console" => Some("there is no `console` -- print with `println(...)`"),
                        _ => None,
                    };
                    let known = self.env.all_var_names();
                    // An EXACT stdlib export beats any fuzzy guess: the real
                    // problem is almost always a missing `use`, and the old
                    // Levenshtein-only path pointed at unrelated builtins
                    // (`exec` -> "did you mean `exit`?" when the user wanted
                    // std::db's `execute`... or just hadn't imported std::db).
                    let stdlib_hits = crate::suggest::stdlib_modules_exporting(name);
                    if let Some(diag) = self.diagnostics.last_mut() {
                        let mut noted = false;
                        if let Some(note) = newcomer_note {
                            diag.notes.push(note.to_string());
                            noted = true;
                        }
                        if !noted && !stdlib_hits.is_empty() {
                            let uses: Vec<String> = stdlib_hits
                                .iter()
                                .take(2)
                                .map(|m| format!("`use std::{m}`"))
                                .collect();
                            diag.notes.push(format!(
                                "`{name}` is a std function -- add {}",
                                uses.join(" or ")
                            ));
                            noted = true;
                        }
                        if newcomer_note.is_none() {
                            // Keep the fuzzy local suggestion even alongside a
                            // stdlib hit -- the name may equally be a typo of
                            // an in-scope variable, and showing both is never
                            // wrong.
                            if let Some(suggestion) = crate::suggest::closest_match(
                                name,
                                known.iter().map(|s| s.as_str()),
                            ) {
                                diag.notes.push(format!("did you mean `{suggestion}`?"));
                                noted = true;
                            }
                        }
                        if !noted {
                            // No close match, show that variable is not in scope
                            diag.notes.push("this variable is not in scope".to_string());
                        }
                    }
                    Type::Error
                }
            }

            // Field access: object.field.
            Expr::FieldAccess {
                object,
                field,
                span,
            } => {
                let obj_ty = self.infer_expr(object);
                let obj_ty = self.engine.resolve(&obj_ty);
                // FAIL-CLOSED for fn-bearing ACTOR STATE (LEDGER item 32).
                // An actor's state is mutable storage that any handler may write
                // at any prior dispatch, so which closure sits in a fn-bearing
                // state field at a given read is genuinely not knowable
                // statically. Yield `Unknown` (which erases to `ALL`) rather than
                // the declaration's row var, which would otherwise stay forever
                // unbound and charge nothing. This is the same stance the
                // capability checker already takes for `self.<field>()` in
                // `resolve_actor_self_field_invoke_caps`, applied to the row.
                //
                // Scoped deliberately to `self` inside a handler AND to fields
                // whose type actually contains a function -- an actor's ordinary
                // data state is untouched.
                let is_actor_fn_state = matches!(
                    object.as_ref(),
                    Expr::Identifier { name, .. } if name == "self"
                ) && self.current_actor_fn_state_fields.contains(field);

                match &obj_ty {
                    Type::Struct { name, generics } => {
                        if let Some(fty) = self.env.lookup_field(name, field) {
                            let fty = fty.clone();
                            let out = self.substitute_struct_generics(name, generics, &fty);
                            if is_actor_fn_state {
                                out.with_caps_erased_to_unknown()
                            } else {
                                out
                            }
                        } else {
                            self.error_with_code(
                                format!("no field `{field}` on type `{name}`"),
                                *span,
                                kryos_errors::codes::E0106,
                            );
                            let field_names: Vec<String> = self
                                .env
                                .lookup_struct(name)
                                .map(|d| d.fields.iter().map(|(n, _)| n.clone()).collect())
                                .unwrap_or_default();
                            if let Some(s) = crate::suggest::closest_match(
                                field,
                                field_names.iter().map(|s| s.as_str()),
                            ) {
                                if let Some(diag) = self.diagnostics.last_mut() {
                                    diag.notes.push(format!("did you mean `{s}`?"));
                                }
                            }
                            Type::Error
                        }
                    }
                    Type::Tuple { elements } => {
                        // Tuple field access: t.0, t.1, etc.
                        if let Ok(idx) = field.parse::<usize>() {
                            if idx < elements.len() {
                                elements[idx].clone()
                            } else {
                                self.error(
                                    format!(
                                        "tuple index {idx} out of bounds (length {})",
                                        elements.len()
                                    ),
                                    *span,
                                );
                                Type::Error
                            }
                        } else {
                            self.error(format!("no field `{field}` on tuple type"), *span);
                            Type::Error
                        }
                    }
                    // Auto-deref: access fields through references.
                    Type::Reference { inner, .. } => {
                        if let Type::Struct { name, generics } = inner.as_ref() {
                            if let Some(fty) = self.env.lookup_field(name, field) {
                                let fty = fty.clone();
                                self.substitute_struct_generics(name, generics, &fty)
                            } else {
                                self.error(format!("no field `{field}` on type `{name}`"), *span);
                                Type::Error
                            }
                        } else {
                            self.error(
                                format!("cannot access field `{field}` on type `{obj_ty}`"),
                                *span,
                            );
                            Type::Error
                        }
                    }
                    // Auto-deref through shared (Rc-like) pointer.
                    Type::Shared { inner } => {
                        if let Type::Struct { name, generics } = inner.as_ref() {
                            if let Some(fty) = self.env.lookup_field(name, field) {
                                let fty = fty.clone();
                                self.substitute_struct_generics(name, generics, &fty)
                            } else {
                                self.error(format!("no field `{field}` on type `{name}`"), *span);
                                Type::Error
                            }
                        } else {
                            self.error(
                                format!("cannot access field `{field}` on type `{obj_ty}`"),
                                *span,
                            );
                            Type::Error
                        }
                    }
                    // Enum variant access: Color.Red resolves to the enum type.
                    Type::Enum { name, generics } => {
                        if let Some(edef) = self.env.lookup_enum(name) {
                            if edef.variants.iter().any(|(vname, _)| vname == field) {
                                Type::Enum {
                                    name: name.clone(),
                                    generics: generics.clone(),
                                }
                            } else {
                                let variants: Vec<String> =
                                    edef.variants.iter().map(|(n, _)| n.clone()).collect();
                                self.error(format!("no variant `{field}` on enum `{name}`"), *span);
                                if let Some(s) = crate::suggest::closest_match(
                                    field,
                                    variants.iter().map(|s| s.as_str()),
                                ) {
                                    if let Some(diag) = self.diagnostics.last_mut() {
                                        diag.notes.push(format!("did you mean `{s}`?"));
                                    }
                                }
                                Type::Error
                            }
                        } else {
                            self.error(
                                format!("cannot access field `{field}` on type `{obj_ty}`"),
                                *span,
                            );
                            Type::Error
                        }
                    }
                    Type::Error => Type::Error,
                    _ => {
                        self.error(
                            format!("cannot access field `{field}` on type `{obj_ty}`"),
                            *span,
                        );
                        Type::Error
                    }
                }
            }

            // Index access: object[index].
            Expr::IndexAccess {
                object,
                index,
                span,
            } => {
                let obj_ty = self.infer_expr(object);
                let idx_ty = self.infer_expr(index);
                let obj_ty = self.engine.resolve(&obj_ty);
                match &obj_ty {
                    Type::Array { element, .. } => {
                        // Index must be an integer.
                        if !self.engine.resolve(&idx_ty).is_integer() {
                            if let Err(diag) = self.engine.unify(&Type::I64, &idx_ty, *span) {
                                self.diagnostics.push(diag);
                            }
                        }
                        *element.clone()
                    }
                    Type::Map { key, value } => {
                        if let Err(diag) = self.engine.unify(key, &idx_ty, *span) {
                            self.diagnostics.push(diag);
                        }
                        *value.clone()
                    }
                    // String indexing: str[i] -> str (single character)
                    Type::Str => Type::Str,
                    Type::Error => Type::Error,
                    // Object type not yet resolved -- e.g. a generic HOF param
                    // `T` bound from a `[any]`-returning function (zip/enumerate)
                    // whose element type isn't known when the closure body is
                    // checked. Defer: treat as indexable, yielding a fresh element
                    // var unified by later use, instead of a spurious
                    // "not indexable" error on the common zip-then-map idiom.
                    Type::Var(_) => self.engine.fresh_var(),
                    _ => {
                        self.error(format!("type `{obj_ty}` is not indexable"), *span);
                        Type::Error
                    }
                }
            }

            // Binary operations.
            Expr::BinaryOp {
                op,
                left,
                right,
                span,
            } => self.check_binary_op(*op, left, right, *span),

            // Unary operations.
            Expr::UnaryOp { op, operand, span } => {
                let operand_ty = self.infer_expr(operand);
                match op {
                    UnOp::Neg => {
                        let resolved = self.engine.resolve(&operand_ty);
                        if resolved.is_numeric() || resolved.is_error() {
                            operand_ty
                        } else {
                            self.error(format!("cannot negate type `{resolved}`"), *span);
                            Type::Error
                        }
                    }
                    UnOp::Not => {
                        if let Err(diag) = self.engine.unify(&Type::Bool, &operand_ty, *span) {
                            self.diagnostics.push(diag);
                        }
                        Type::Bool
                    }
                    UnOp::BitNot => {
                        let resolved = self.engine.resolve(&operand_ty);
                        if resolved.is_integer() || resolved.is_error() {
                            operand_ty
                        } else {
                            self.error(format!("cannot bitwise-not type `{resolved}`"), *span);
                            Type::Error
                        }
                    }
                }
            }

            // Function call.
            Expr::FnCall { callee, args, span } => {
                // Extract callee name for attribute checks.
                let callee_name_str = match callee.as_ref() {
                    Expr::Identifier { name, .. } => Some(name.clone()),
                    _ => None,
                };

                // Capability-row inference (Stage 1): a direct call to a
                // GATED BUILTIN charges its capability bit by NAME (the
                // builtin's own `FunctionSig` always carries the shared,
                // permanently-empty `builtin_cap_var` -- see its doc
                // comment -- so this name-based check is the actual
                // source of truth for a builtin call, exactly mirroring
                // `kryos_capabilities::checker`'s own `collect_caps_expr`).
                if let Some(ref cname) = callee_name_str {
                    self.accumulate_builtin_call(cname);
                }

                // @deprecated: warn when calling a deprecated function.
                if let Some(ref name) = callee_name_str {
                    if self.deprecated_functions.contains(name) {
                        self.warning(format!("use of deprecated function `{name}`"), *span);
                    }
                }

                // @pure enforcement: pure functions cannot call non-pure functions or I/O builtins.
                if self.in_pure_function {
                    if let Some(ref name) = callee_name_str {
                        self.check_pure_free_call(name, *span);
                    } else {
                        // A callee that is not a plain name -- an inline/returned
                        // closure or an indexed/field function value invoked
                        // directly -- is an indirect call whose purity cannot be
                        // verified. Reject it in a @pure fn (same reasoning as a
                        // fn-typed variable in check_pure_free_call).
                        self.error(
                            "`@pure` function cannot call through an unnamed function value -- the callee's purity cannot be verified".to_string(),
                            *span,
                        );
                    }
                }

                // Bare (unqualified) enum-variant constructor: `Some(42)`,
                // `Ok(v)`, `Rect(3, 5)`. If the callee is a plain identifier
                // that is not a value/function in scope but IS an enum variant,
                // type the call as enum construction (mirrors the qualified
                // `Enum.Variant(..)` path; the MIR lowering already handles bare
                // construction via find_enum_variant).
                if let Some(ref cname) = callee_name_str {
                    if self.env.lookup_var(cname).is_none()
                        && self.env.lookup_function(cname).is_none()
                    {
                        if let Some(edef) = self.env.find_enum_by_variant(cname).cloned() {
                            if let Some((_, field_types)) =
                                edef.variants.iter().find(|(v, _)| v == cname)
                            {
                                self.check_variant_construct_arity(
                                    cname,
                                    field_types.len(),
                                    args.len(),
                                    *span,
                                );
                                let mut var_map = std::collections::HashMap::new();
                                let mut fresh_generics =
                                    Vec::with_capacity(edef.generic_var_ids.len());
                                for &old_id in &edef.generic_var_ids {
                                    let fresh = self.engine.fresh_var();
                                    if let Type::Var(new_id) = &fresh {
                                        var_map.insert(old_id, *new_id);
                                    }
                                    fresh_generics.push(fresh);
                                }
                                let mut arg_tys = Vec::with_capacity(args.len());
                                for (arg, expected_ty) in args.iter().zip(field_types.iter()) {
                                    let arg_ty = self.infer_expr(arg);
                                    let expected_instantiated = if var_map.is_empty() {
                                        expected_ty.clone()
                                    } else {
                                        self.engine.instantiate(expected_ty, &var_map)
                                    };
                                    if let Err(diag) =
                                        self.engine.unify(&expected_instantiated, &arg_ty, *span)
                                    {
                                        self.diagnostics.push(diag);
                                    }
                                    arg_tys.push(arg_ty);
                                }
                                let generics = if fresh_generics.is_empty() {
                                    self.stdlib_bridge_generics(&edef.name, cname, &arg_tys)
                                } else {
                                    fresh_generics
                                };
                                return Type::Enum {
                                    name: edef.name.clone(),
                                    generics,
                                };
                            }
                        }
                    }
                }

                let callee_ty = self.infer_expr(callee);
                let callee_ty = self.engine.resolve(&callee_ty);

                match &callee_ty {
                    Type::Function { params, ret, caps } => {
                        // Every call site charges whatever the CALLEE'S
                        // OWN TYPE carries -- direct call, call through a
                        // parameter, local, container element, or actor-
                        // state field/alias, all resolve here uniformly,
                        // because `callee_ty` is the ordinary already-
                        // resolved type the checker computed above, not a
                        // re-derivation from this call expression's own
                        // syntax. See `docs/capability-effects-spec.md` §6.
                        self.accumulate_caps(caps);
                        // Special handling for assert(): accept 1 or 2 args.
                        // assert(condition) uses a default message at codegen time.
                        let is_assert_1arg = matches!(&callee_name_str, Some(n) if n == "assert")
                            && args.len() == 1
                            && params.len() == 2;

                        // Opaque any-callable shape produced by bare `fn`
                        // type annotations: `fn(any) -> any`. Skip arity and
                        // parameter-type checking entirely. We still infer
                        // each argument so its own type errors surface.
                        let is_opaque_callable = params.len() == 1
                            && matches!(&params[0], Type::Error)
                            && matches!(ret.as_ref(), Type::Error);
                        if is_opaque_callable {
                            // map_keys matches the opaque shape (one lenient
                            // param, Error ret) but its return IS knowable:
                            // the MAP'S KEY-typed array. The Error placeholder
                            // broke the natural `let keys = map_keys(m)
                            // for k in keys { m[k] }` idiom with a bogus E0100
                            // on the re-index (workaround was annotating
                            // `let keys: [str]`).
                            if matches!(&callee_name_str, Some(n) if n == "map_keys") {
                                if let Some(arg0) = args.first() {
                                    let arg0_ty = self.infer_expr(arg0);
                                    let mt = self.engine.resolve(&arg0_ty);
                                    if let Type::Map { key, .. } = mt {
                                        return Type::Array {
                                            element: key,
                                            size: None,
                                        };
                                    }
                                }
                            }
                            // The global `reverse`/`sort` array builtins have
                            // the opaque (any->any) shape, so they hit this
                            // early return before the len-style arg gate. They
                            // operate on a KryosArray in place: a str (or any
                            // non-array) arg dereferences a KryosString as a
                            // KryosArray -> SEGFAULT, and the untyped `any`
                            // RETURN makes `let r = sort(a)  r[0]` index a
                            // non-array (crash). Reject non-array args here and
                            // return the arg's real ARRAY type so the result
                            // indexes correctly.
                            // reverse/sort mutate the array IN PLACE and
                            // return VOID (kryos_builtin_sort/reverse are
                            // `-> ()`). Reject a non-array arg (a str reached
                            // the array codegen and dereferenced a KryosString
                            // as a KryosArray -> SEGFAULT), and type the result
                            // Void so capturing it (`let r = sort(a)` /
                            // `a = sort(a)`) is a clean compile error steering
                            // to the in-place `sort(a)` form rather than a
                            // crash on a garbage void slot.
                            if matches!(&callee_name_str, Some(n) if n == "reverse" || n == "sort") {
                                if let Some(arg0) = args.first() {
                                    let arg0_ty = self.infer_expr(arg0);
                                    let at = self.engine.resolve(&arg0_ty);
                                    let fname = callee_name_str.as_deref().unwrap_or("");
                                    match &at {
                                        Type::Array { .. } | Type::Error | Type::Var(_) => {
                                            return Type::Void;
                                        }
                                        other => {
                                            let hint = if *other == Type::Str && fname == "reverse" {
                                                " -- for a string use `std::string::reverse(s)`"
                                            } else {
                                                ""
                                            };
                                            self.error(
                                                format!(
                                                    "`{fname}` expects an array; found `{other}`{hint}"
                                                ),
                                                args[0].span(),
                                            );
                                            return Type::Error;
                                        }
                                    }
                                }
                            }
                            for arg in args.iter() {
                                let _ = self.infer_expr(arg);
                            }
                            return *ret.clone();
                        }

                        if !is_assert_1arg && args.len() != params.len() {
                            let fn_name = match callee_name_str {
                                Some(ref n) => format!("`{n}`"),
                                None => "this function".to_string(),
                            };
                            self.error(
                                format!(
                                    "function {} expects {} argument{}, found {}",
                                    fn_name,
                                    params.len(),
                                    if params.len() == 1 { "" } else { "s" },
                                    args.len(),
                                ),
                                *span,
                            );
                        } else {
                            for (i, (arg, param_ty)) in
                                args.iter().zip(params.iter()).enumerate()
                            {
                                // Bidirectional inference: if arg is an un-annotated
                                // Lambda and param_ty resolves to a Function type with
                                // matching arity, push the expected param/return types
                                // down so the Lambda body can be typed against them.
                                if let Expr::Lambda {
                                    params: lparams,
                                    span: lspan,
                                    ..
                                } = arg
                                {
                                    let resolved_pty = self.engine.resolve(param_ty);
                                    if let Type::Function {
                                        params: eps,
                                        ret: er,
                                        ..
                                    } = &resolved_pty
                                    {
                                        if eps.len() == lparams.len() {
                                            self.lambda_expected_types.insert(
                                                *lspan,
                                                (eps.clone(), (**er).clone()),
                                            );
                                        }
                                    }
                                }
                                // See `dyn_container_reject_params`: a
                                // heterogeneous array literal passed DIRECTLY
                                // as a call argument for a param already
                                // rejected as `[dyn Trait]` (E0110 at the
                                // callee's own declaration) must not ALSO
                                // raise the array literal's own pairwise
                                // element-unify E0100 -- same fix as the
                                // `let x: [dyn Trait] = [A{}, B{}]` case,
                                // extended to the call-site shape the `let`
                                // fix could not reach (`FunctionSig` only
                                // keeps the resolved `Type::Error`, not the
                                // raw annotation this table stands in for).
                                if let (Some(ref cname), Expr::ArrayLiteral { span: arr_span, .. }) =
                                    (&callee_name_str, arg)
                                {
                                    if self
                                        .dyn_container_reject_params
                                        .contains(&(cname.clone(), i))
                                    {
                                        self.suppress_array_elem_unify.insert(*arr_span);
                                    }
                                }
                                let arg_ty = self.infer_expr(arg);
                                // LEDGER item 40: last point a concrete type exists before `any` erasure.
                                self.reject_untagged_scalar_into_any(param_ty, &arg_ty, arg.span());
                                if let Err(diag) = self.engine.unify(param_ty, &arg_ty, arg.span())
                                {
                                    self.diagnostics.push(diag);
                                }
                                self.check_int_literal_range(param_ty, arg, arg.span());
                                // A polymorphic builtin's opaque `Type::Error`
                                // param is DESIGNED to unify with any real
                                // value type (`unify`'s "Error unifies with
                                // anything" error-recovery escape hatch,
                                // kryos-types/src/infer.rs) -- but that escape
                                // hatch was never taught to exclude `Void`,
                                // which is not a real value at all. Passing a
                                // void-returning call's "result" as an
                                // argument to any such builtin (`to_string`,
                                // `abs`, `min`, `max`, `sort`, `reverse`, ...)
                                // silently type-checked and read whatever the
                                // erased void slot held at runtime -- verified
                                // live: `to_string(side_effect())` printed
                                // `"0"`, `abs(side_effect())` printed `0`, on
                                // both backends, zero diagnostic either way.
                                // Same shape as the `len`-specific struct/enum
                                // guard immediately below, generalized to
                                // every opaque-Error-param builtin since Void
                                // is never legitimate for any of them. A user
                                // function shadowing one of these names has
                                // typed params and never hits this arm.
                                // EXCEPTION: `coop_spawn(taskExpr)` deliberately
                                // shares the same opaque Type::Error param
                                // shape but its argument is a TASK EXPRESSION
                                // handled specially at MIR lowering (mirrors
                                // `spawn { .. }`), not a value read at the
                                // call site -- a void-returning task function
                                // is the normal, correct case there (found via
                                // `examples/async_io.kry` regressing when this
                                // check was first added; see the signature's
                                // own comment a few hundred lines below).
                                if matches!(param_ty, Type::Error)
                                    && matches!(self.engine.resolve(&arg_ty), Type::Void)
                                    && !matches!(&callee_name_str, Some(n) if n == "coop_spawn")
                                {
                                    let fn_name = callee_name_str
                                        .as_deref()
                                        .map(|n| format!("`{n}`"))
                                        .unwrap_or_else(|| "this function".to_string());
                                    self.error(
                                        format!(
                                            "cannot use the result of a void-returning call as an argument to {fn_name} -- the callee has no return value"
                                        ),
                                        arg.span(),
                                    );
                                }
                                // The builtin `len` signature accepts any type
                                // (its param is Type::Error) because it is
                                // polymorphic over str/array/map. That let a
                                // struct/enum/number slip through and read a
                                // garbage "length" from the value's first
                                // 8 bytes at runtime (a CsvRow passed to len
                                // printed a pointer). Gate the builtin only --
                                // a user function shadowing `len` has typed
                                // params and never hits this arm.
                                if matches!(&callee_name_str, Some(n) if n == "len")
                                    && matches!(param_ty, Type::Error)
                                {
                                    let resolved = self.engine.resolve(&arg_ty);
                                    let ok = matches!(
                                        resolved,
                                        Type::Str
                                            | Type::Array { .. }
                                            | Type::Map { .. }
                                            | Type::Tuple { .. }
                                            | Type::Error
                                            | Type::Var(_)
                                    );
                                    if !ok {
                                        self.error(
                                            format!(
                                                "`len` expects a str, array, or map; found `{resolved}`"
                                            ),
                                            arg.span(),
                                        );
                                    }
                                }
                            }
                        }
                        *ret.clone()
                    }
                    Type::Error => Type::Error,
                    _ => {
                        self.error(format!("type `{callee_ty}` is not callable"), *span);
                        Type::Error
                    }
                }
            }

            // Method call.
            Expr::MethodCall {
                object,
                method,
                args,
                span,
            } => {
                // If the receiver is a bare identifier that names a struct or
                // enum (not a value in scope), treat the call as a static
                // method/associated-function call. This makes `List.new()`,
                // `Map.new()` etc. work the same as `List::new()`.
                if let Expr::Identifier { name: ident, .. } = object.as_ref() {
                    let is_var = self.env.lookup_var(ident).is_some();
                    let is_type = self.env.lookup_struct(ident).is_some()
                        || self.env.lookup_enum(ident).is_some();
                    if !is_var && is_type {
                        let static_call = Expr::StaticMethodCall {
                            type_name: ident.clone(),
                            method: method.clone(),
                            args: args.clone(),
                            span: *span,
                        };
                        return self.infer_expr(&static_call);
                    }
                }
                // @pure enforcement: a pure function may not use instance-method
                // dispatch. The concrete callee's purity cannot be verified here
                // (impl-method @pure is not tracked, and a name-based check would
                // be unsound across same-named methods on different types), so
                // any side effect it performs would be silently dropped by the
                // @pure CSE/dead-call optimization. Reject outright.
                if self.in_pure_function {
                    self.error(
                        format!(
                            "`@pure` function cannot call method `{method}` \
                             (purity of a method call cannot be verified; \
                             extract the side-effect-free computation into a \
                             top-level `@pure` function, or drop `@pure`)"
                        ),
                        *span,
                    );
                }
                let obj_ty = self.infer_expr(object);
                let obj_ty = self.engine.resolve(&obj_ty);

                // Resolve method calls on a generic type variable through its
                // declared trait bounds. For `fn announce<T: Showable>(x: T)`,
                // `x.show()` typechecks because `T`'s bound is `Showable` and
                // `Showable::show(self) -> str` is in scope. Each impl resolves
                // the actual function during monomorphization in MIR lowering.
                if let Type::Var(id) = &obj_ty {
                    if let Some(bounds) = self.generic_var_bounds.get(id).cloned() {
                        for trait_name in &bounds {
                            if let Some(trait_def) =
                                self.env.lookup_trait(trait_name).cloned()
                            {
                                if let Some(sig) =
                                    trait_def.methods.iter().find(|m| m.name == *method)
                                {
                                    let expected_params: Vec<_> = if sig
                                        .params
                                        .first()
                                        .map(|(n, _)| n.as_str())
                                        == Some("self")
                                    {
                                        sig.params[1..].to_vec()
                                    } else {
                                        sig.params.clone()
                                    };
                                    if args.len() != expected_params.len() {
                                        self.error(
                                            format!(
                                                "method `{method}` expects {} arguments, found {}",
                                                expected_params.len(),
                                                args.len()
                                            ),
                                            *span,
                                        );
                                    } else {
                                        for (arg, (_, param_ty)) in
                                            args.iter().zip(expected_params.iter())
                                        {
                                            let arg_ty = self.infer_expr(arg);
                                            // LEDGER item 40: last point a concrete type exists before `any` erasure.
                                            self.reject_untagged_scalar_into_any(param_ty, &arg_ty, arg.span());
                                            if let Err(diag) =
                                                self.engine.unify(param_ty, &arg_ty, arg.span())
                                            {
                                                self.diagnostics.push(diag);
                                            }
                                        }
                                    }
                                    // Row-propagation stage gap 1/3
                                    // (docs/capability-effects-spec.md):
                                    // `T`'s bound only tells us WHICH TRAIT
                                    // a call site's concrete `T` implements
                                    // -- it does not tell us WHICH impl.
                                    // `sig` is the TRAIT's own declared
                                    // signature, whose `own_cap_var` has no
                                    // body to ever bind it (see
                                    // `Decl::Trait`'s registration, a few
                                    // hundred lines up: "no body here --
                                    // bodies live in each impl"), so it can
                                    // never be resolved to a real charge --
                                    // and even if it somehow were, it would
                                    // still only describe ONE arbitrary
                                    // impl, not the one this call site
                                    // actually reaches at runtime. Charge
                                    // this call itself as `Unknown` (never
                                    // silently free) and stamp every
                                    // capability row reachable inside the
                                    // trait's declared return type as
                                    // `Unknown` too -- a returned closure
                                    // could have come from ANY implementing
                                    // type, so its row is exactly as
                                    // unknowable as the call itself.
                                    self.accumulate_caps(&crate::ty::CapRow::unknown());
                                    return sig.ret.with_caps_erased_to_unknown();
                                }
                            }
                        }
                    }
                }

                // Handle method calls on dyn Trait objects via trait definition lookup.
                if let Type::DynTrait { ref trait_name } = obj_ty {
                    if let Some(trait_def) = self.env.lookup_trait(trait_name).cloned() {
                        if let Some(sig) = trait_def.methods.iter().find(|m| m.name == *method) {
                            // Skip 'self' parameter (first param) if present.
                            let expected_params: Vec<_> =
                                if sig.params.first().map(|(n, _)| n.as_str()) == Some("self") {
                                    sig.params[1..].to_vec()
                                } else {
                                    sig.params.clone()
                                };
                            if args.len() != expected_params.len() {
                                self.error(
                                    format!(
                                        "method `{method}` expects {} arguments, found {}",
                                        expected_params.len(),
                                        args.len()
                                    ),
                                    *span,
                                );
                            } else {
                                for (arg, (_, param_ty)) in args.iter().zip(expected_params.iter())
                                {
                                    let arg_ty = self.infer_expr(arg);
                                    // LEDGER item 40: last point a concrete type exists before `any` erasure.
                                    self.reject_untagged_scalar_into_any(param_ty, &arg_ty, arg.span());
                                    if let Err(diag) =
                                        self.engine.unify(param_ty, &arg_ty, arg.span())
                                    {
                                        self.diagnostics.push(diag);
                                    }
                                    self.check_int_literal_range(param_ty, arg, arg.span());
                                }
                            }
                            // A `dyn` method returning a by-value AGGREGATE
                            // (tuple, enum, Option, Result, or a multi-field
                            // struct) cannot be dispatched: the uniform i64
                            // dyn-thunk ABI passes ONE i64 slot back, so a
                            // multi-slot aggregate truncates to its first field.
                            // On AOT this previously emitted invalid LLVM IR
                            // (`add %Agg, 0`) -- a cryptic build failure; on the
                            // JIT it silently truncated. Reject here with a clear
                            // diagnostic + workaround so both backends agree.
                            // Scalars and heap handles (str/array/map) pass fine.
                            let ret_is_byval_agg = match &sig.ret {
                                Type::Tuple { .. }
                                | Type::Option { .. }
                                | Type::Result { .. }
                                | Type::Enum { .. } => true,
                                Type::Struct { name, .. } => self
                                    .env
                                    .lookup_struct(name)
                                    .map(|sd| sd.fields.len() > 1)
                                    .unwrap_or(false),
                                _ => false,
                            };
                            if ret_is_byval_agg {
                                self.error_with_code(
                                    format!(
                                        "`dyn {trait_name}` method `{method}` returns a by-value aggregate (`{}`), which trait-object dispatch cannot pass through its uniform handle ABI yet -- it truncates to the first field. Return a scalar or a heap handle (str/array/map), return the fields individually, or call the method via static dispatch (a concrete-typed receiver, not `dyn`).",
                                        sig.ret
                                    ),
                                    *span,
                                    kryos_errors::codes::E0110,
                                );
                                return Type::Error;
                            }
                            // Row-propagation stage gap 1
                            // (docs/capability-effects-spec.md): a `dyn
                            // Trait` VALUE could be holding any concrete
                            // implementer of `trait_name` at runtime (that
                            // is the entire point of a trait object) --
                            // `sig` is only the TRAIT's own declared
                            // signature, whose `own_cap_var` has no body to
                            // ever bind it. Charge this dispatch as
                            // `Unknown` (never silently free) and stamp
                            // every capability row reachable inside the
                            // trait's declared return type as `Unknown` too
                            // -- see the identical treatment (and full
                            // rationale) for the generic trait-bound
                            // MethodCall arm a few hundred lines up.
                            self.accumulate_caps(&crate::ty::CapRow::unknown());
                            return sig.ret.with_caps_erased_to_unknown();
                        }
                    }
                    self.error(
                        format!("no method `{method}` found on `dyn {trait_name}`"),
                        *span,
                    );
                    return Type::Error;
                }

                let type_name = match &obj_ty {
                    Type::Struct { name, .. } => Some(name.clone()),
                    Type::Enum { name, .. } => Some(name.clone()),
                    Type::Error => return Type::Error,
                    _ => None,
                };

                // Reject calling an actor handler that declares a non-void
                // return type -- actor sends are fire-and-forget (no reply
                // channel exists in the runtime), so the value can never
                // reach the caller. Loud failure here beats the old silent-0
                // (or f64-crash) behavior. See `actor_nonvoid_handlers` above.
                if let Some(ref tname) = type_name {
                    if let Some(ret_ty) = self
                        .actor_nonvoid_handlers
                        .get(&(tname.clone(), method.clone()))
                        .cloned()
                    {
                        self.error(
                            format!(
                                "actor handler `{method}` declares return type `{ret_ty}`, \
                                 but actor sends are asynchronous fire-and-forget: there is no \
                                 synchronous reply channel, so the return value can never reach \
                                 this call site (see docs/09-concurrency.md). Request-response \
                                 actors are not supported yet -- declare `{method}` with no \
                                 return type (fire-and-forget) instead."
                            ),
                            *span,
                        );
                        return Type::Error;
                    }
                }

                if let Some(ref tname) = type_name {
                    // Check if this is an enum variant constructor (e.g. Shape.Circle(3)).
                    if let Some(edef) = self.env.lookup_enum(tname).cloned() {
                        if let Some((_, field_types)) =
                            edef.variants.iter().find(|(vname, _)| vname == method)
                        {
                            self.check_variant_construct_arity(
                                method,
                                field_types.len(),
                                args.len(),
                                *span,
                            );
                            // Per-use monomorphization: fresh vars for each generic param.
                            let mut var_map = std::collections::HashMap::new();
                            let mut fresh_generics = Vec::with_capacity(edef.generic_var_ids.len());
                            for &old_id in &edef.generic_var_ids {
                                let fresh = self.engine.fresh_var();
                                if let Type::Var(new_id) = &fresh {
                                    var_map.insert(old_id, *new_id);
                                }
                                fresh_generics.push(fresh);
                            }
                            let mut arg_tys = Vec::with_capacity(args.len());
                            for (arg, expected_ty) in args.iter().zip(field_types.iter()) {
                                let arg_ty = self.infer_expr(arg);
                                let expected_instantiated = if var_map.is_empty() {
                                    expected_ty.clone()
                                } else {
                                    self.engine.instantiate(expected_ty, &var_map)
                                };
                                if let Err(diag) =
                                    self.engine
                                        .unify(&expected_instantiated, &arg_ty, arg.span())
                                {
                                    self.diagnostics.push(diag);
                                }
                                arg_tys.push(arg_ty);
                            }
                            let generics = if fresh_generics.is_empty() {
                                self.stdlib_bridge_generics(tname, method, &arg_tys)
                            } else {
                                fresh_generics
                            };
                            return Type::Enum {
                                name: tname.clone(),
                                generics,
                            };
                        }
                    }

                    if let Some(sig) = self.env.lookup_method(tname, method).cloned() {
                        // Instantiate the method signature with FRESH type vars
                        // per call, then bind them to the receiver's concrete
                        // type arguments by unifying the freshened `self`
                        // parameter against the receiver type. This fixes two
                        // problems that the old `sig.ret.clone()` had:
                        //   1. Contamination -- the impl's return var was shared
                        //      across every call, so `.get()` on Box<str> then
                        //      `let x: f64 = box_f64.get()` errored "found str".
                        //   2. Erasure -- `to_string(box_f64.get())` reported the
                        //      i64 slot and printed raw bits; the self-unify now
                        //      resolves the return to the real f64.
                        let (inst_params, inst_ret, var_map, cap_var_map) =
                            self.engine.instantiate_sig(&sig);
                        for (old_id, new_id) in &var_map {
                            if let Some(bounds) = self.generic_var_bounds.get(old_id).cloned() {
                                self.engine.set_var_bounds(*new_id, bounds);
                            }
                        }
                        let has_self =
                            sig.params.first().map(|(n, _)| n.as_str()) == Some("self");
                        let expected_params: Vec<Type> = if has_self {
                            // Unify only when the freshened self is a same-named
                            // struct/enum with matching generic arity -- an
                            // unannotated `self` (empty generics) or a shape
                            // mismatch must not inject a spurious error.
                            if let Some(self_ty) = inst_params.first() {
                                let compatible = match (self_ty, &obj_ty) {
                                    (
                                        Type::Struct { name: a, generics: ga },
                                        Type::Struct { name: b, generics: gb },
                                    )
                                    | (
                                        Type::Enum { name: a, generics: ga },
                                        Type::Enum { name: b, generics: gb },
                                    ) => a == b && ga.len() == gb.len() && !ga.is_empty(),
                                    _ => false,
                                };
                                if compatible {
                                    let _ = self.engine.unify(self_ty, &obj_ty, *span);
                                }
                            }
                            inst_params.iter().skip(1).cloned().collect()
                        } else {
                            inst_params.clone()
                        };

                        if args.len() != expected_params.len() {
                            self.error(
                                format!(
                                    "method `{method}` expects {} arguments, found {}",
                                    expected_params.len(),
                                    args.len()
                                ),
                                *span,
                            );
                        } else {
                            for (arg, param_ty) in args.iter().zip(expected_params.iter()) {
                                let arg_ty = self.infer_expr(arg);
                                // LEDGER item 40: last point a concrete type exists before `any` erasure.
                                self.reject_untagged_scalar_into_any(param_ty, &arg_ty, arg.span());
                                if let Err(diag) = self.engine.unify(param_ty, &arg_ty, arg.span())
                                {
                                    self.diagnostics.push(diag);
                                }
                            }
                        }
                        // Charge the callee's OWN row at this call site. This site
                        // resolves BOTH impl methods and ACTOR HANDLERS (an actor
                        // registers handlers through the same `lookup_method`), and
                        // it was the only major dispatch path that dropped the row --
                        // the `cap_var_map` it already computed was bound to `_` and
                        // discarded. Measured: an actor handler calling `file_read`
                        // accumulated `{fs:read}` while the `main` invoking it
                        // accumulated `{}`, so any leak behind a handler call was
                        // invisible to enforcement.
                        //
                        // ORDER MATTERS, and getting it wrong is silent: this must
                        // run AFTER the argument unification above. A handler that is
                        // row-POLYMORPHIC in a fn-typed parameter (`fn receive(self,
                        // f: fn() -> str)`) has a row that mentions that parameter's
                        // own var, and that var only binds when the argument is
                        // unified against the parameter. Charging first resolves the
                        // row while it is still open and charges nothing at all.
                        {
                            let own_row = self
                                .engine
                                .resolve_cap_row(&crate::ty::CapRow::var(sig.own_cap_var));
                            let ref_caps = self.engine.instantiate_row(&own_row, &cap_var_map);
                            if std::env::var("KRYOS_ROW_TRACE").is_ok() {
                                eprintln!(
                                    "[row] dispatch {}::{} own={} inst={} genvars={:?} map={:?}",
                                    tname,
                                    method,
                                    own_row.display(),
                                    self.engine.resolve_cap_row(&ref_caps).display(),
                                    sig.generic_cap_var_ids,
                                    cap_var_map
                                );
                            }
                            self.accumulate_caps(&ref_caps);
                        }
                        return self.engine.resolve(&inst_ret);
                    }

                    // Check if this is a Function-typed struct field being called
                    // (e.g. `t.transform(5)` where `transform: fn(i64) -> i64`).
                    //
                    // For a generic struct (`struct Box<T> { f: T }`), the field's
                    // RAW declared type is the bare generic param var (`Type::Var`),
                    // not yet substituted with the concrete type argument carried by
                    // `obj_ty`'s generics (e.g. `Box<fn() -> i64>`). Substitute the
                    // struct's declared generic vars with the receiver's concrete
                    // generic args before checking for `Type::Function`, mirroring
                    // the freshen-then-unify-then-resolve pattern the generic METHOD
                    // path above already uses for `self`.
                    let field_ty = self.env.lookup_field(tname, method).cloned().map(|raw| {
                        if let Type::Struct {
                            generics: obj_generics,
                            ..
                        } = &obj_ty
                        {
                            if let Some(def) = self.env.lookup_struct(tname) {
                                if !def.generic_var_ids.is_empty() && !obj_generics.is_empty() {
                                    let mut var_map = std::collections::HashMap::new();
                                    let mut fresh_vars =
                                        Vec::with_capacity(def.generic_var_ids.len());
                                    for &old_id in &def.generic_var_ids {
                                        let fresh = self.engine.fresh_var();
                                        if let Type::Var(new_id) = &fresh {
                                            var_map.insert(old_id, *new_id);
                                        }
                                        fresh_vars.push(fresh);
                                    }
                                    let instantiated = self.engine.instantiate(&raw, &var_map);
                                    for (fresh, concrete) in
                                        fresh_vars.iter().zip(obj_generics.iter())
                                    {
                                        let _ = self.engine.unify(fresh, concrete, *span);
                                    }
                                    return self.engine.resolve(&instantiated);
                                }
                            }
                        }
                        raw
                    });

                    if let Some(Type::Function {
                        params: fn_params,
                        ret: fn_ret,
                        caps: fn_caps,
                    }) = field_ty
                    {
                        // A call through a fn-typed struct/actor-state FIELD
                        // (`t.f()`, `self.b.f()`, an aliased `let c = b;
                        // c.f()`, or a `for x in [self.b] { x.f() }` loop
                        // variable -- this ONE resolution path is reached by
                        // every one of those syntactic routes uniformly,
                        // because none of them change `field_ty`'s STATIC
                        // TYPE, which is all this arm ever consults). Union
                        // the field's inferred row into whatever enclosing
                        // function/lambda body is making this call -- see
                        // `docs/capability-effects-spec.md` §6, the two live
                        // repros this closes: attack_container_param_alias_
                        // defeats_hotparam.kry / attack_actor_state_forloop_
                        // alias.kry.
                        self.accumulate_caps(&fn_caps);
                        // Opaque callable (bare `fn` type) accepts any arity.
                        let is_opaque = fn_params.len() == 1
                            && matches!(&fn_params[0], Type::Error)
                            && matches!(fn_ret.as_ref(), Type::Error);
                        if is_opaque {
                            for arg in args.iter() {
                                let _ = self.infer_expr(arg);
                            }
                            return *fn_ret;
                        }
                        if args.len() != fn_params.len() {
                            self.error(
                                format!(
                                    "closure field `{method}` expects {} arguments, found {}",
                                    fn_params.len(),
                                    args.len()
                                ),
                                *span,
                            );
                        } else {
                            for (arg, param_ty) in args.iter().zip(fn_params.iter()) {
                                let arg_ty = self.infer_expr(arg);
                                // LEDGER item 40: last point a concrete type exists before `any` erasure.
                                self.reject_untagged_scalar_into_any(param_ty, &arg_ty, arg.span());
                                if let Err(diag) = self.engine.unify(param_ty, &arg_ty, arg.span())
                                {
                                    self.diagnostics.push(diag);
                                }
                            }
                        }
                        return *fn_ret;
                    }
                }

                self.error_with_code(
                    format!("no method `{method}` found for type `{obj_ty}`"),
                    *span,
                    kryos_errors::codes::E0107,
                );
                // Common builtins mistaken for methods (`s.len()`, `a.push(x)`).
                const BUILTIN_FN_HINTS: &[&str] = &[
                    "len", "push", "pop", "contains", "starts_with", "ends_with",
                    "trim", "to_upper", "to_lower", "replace", "split", "join",
                    "substr", "sort", "reverse", "abs", "to_string",
                ];
                if BUILTIN_FN_HINTS.contains(&method.as_str()) {
                    if let Some(diag) = self.diagnostics.last_mut() {
                        diag.notes.push(format!(
                            "`{method}` is a global builtin, not a method -- write `{method}(value, ...)`"
                        ));
                    }
                }
                if let Type::Struct { name, .. } | Type::Enum { name, .. } = &obj_ty {
                    let methods = self.env.all_method_names(name);
                    if let Some(s) =
                        crate::suggest::closest_match(method, methods.iter().map(|s| s.as_str()))
                    {
                        if let Some(diag) = self.diagnostics.last_mut() {
                            diag.notes.push(format!("did you mean `{s}`?"));
                        }
                    }
                }
                Type::Error
            }

            Expr::StaticMethodCall {
                type_name,
                method,
                args,
                span,
            } => {
                // Enum variant constructor via `::` syntax (e.g. `Maybe::Some(10)`).
                if let Some(edef) = self.env.lookup_enum(type_name).cloned() {
                    if let Some((_, field_types)) =
                        edef.variants.iter().find(|(vname, _)| vname == method)
                    {
                        self.check_variant_construct_arity(
                            method,
                            field_types.len(),
                            args.len(),
                            *span,
                        );
                        let mut var_map = std::collections::HashMap::new();
                        let mut fresh_generics = Vec::with_capacity(edef.generic_var_ids.len());
                        for &old_id in &edef.generic_var_ids {
                            let fresh = self.engine.fresh_var();
                            if let Type::Var(new_id) = &fresh {
                                var_map.insert(old_id, *new_id);
                            }
                            fresh_generics.push(fresh);
                        }
                        let mut arg_tys = Vec::with_capacity(args.len());
                        for (arg, expected_ty) in args.iter().zip(field_types.iter()) {
                            let arg_ty = self.infer_expr(arg);
                            let expected_instantiated = if var_map.is_empty() {
                                expected_ty.clone()
                            } else {
                                self.engine.instantiate(expected_ty, &var_map)
                            };
                            if let Err(diag) =
                                self.engine
                                    .unify(&expected_instantiated, &arg_ty, arg.span())
                            {
                                self.diagnostics.push(diag);
                            }
                            arg_tys.push(arg_ty);
                        }
                        let generics = if fresh_generics.is_empty() {
                            self.stdlib_bridge_generics(type_name, method, &arg_tys)
                        } else {
                            fresh_generics
                        };
                        return Type::Enum {
                            name: type_name.clone(),
                            generics,
                        };
                    }
                }
                // Look up the method on the type (mangled as TypeName__method).
                if let Some(sig) = self.env.lookup_method(type_name, method).cloned() {
                    // @pure enforcement: same as instance-method dispatch above --
                    // an associated/static method's purity cannot be verified, so
                    // calling one from a @pure function is rejected (enum-variant
                    // construction returned earlier and is unaffected).
                    if self.in_pure_function {
                        self.error(
                            format!(
                                "`@pure` function cannot call static method \
                                 `{type_name}::{method}` (purity of a method call \
                                 cannot be verified; extract the side-effect-free \
                                 computation into a top-level `@pure` function, or \
                                 drop `@pure`)"
                            ),
                            *span,
                        );
                    }
                    // Instantiate with FRESH type vars per call site, mirroring
                    // the instance-method fix above (and the free-function /
                    // module-qualified-call paths). Without this, a generic
                    // associated/static fn (no `self` receiver, e.g. `Box::new`)
                    // reused the impl's shared generic vars across every call:
                    // the first call's concrete type bound them permanently, so
                    // a second call at a different concrete type unified against
                    // an already-bound var and was wrongly rejected with E0100.
                    let (inst_params, inst_ret, var_map, cap_var_map) =
                        self.engine.instantiate_sig(&sig);
                    for (old_id, new_id) in &var_map {
                        if let Some(bounds) = self.generic_var_bounds.get(old_id).cloned() {
                            self.engine.set_var_bounds(*new_id, bounds);
                        }
                    }
                    // Static call - skip 'self' parameter.
                    let expected_params: Vec<Type> =
                        if sig.params.first().map(|(n, _)| n.as_str()) == Some("self") {
                            inst_params.iter().skip(1).cloned().collect()
                        } else {
                            inst_params.clone()
                        };
                    if args.len() != expected_params.len() {
                        self.error(
                            format!(
                                "static method `{type_name}::{method}` expects {} arguments, found {}",
                                expected_params.len(),
                                args.len()
                            ),
                            *span,
                        );
                    } else {
                        for (arg, param_ty) in args.iter().zip(expected_params.iter()) {
                            let arg_ty = self.infer_expr(arg);
                            // LEDGER item 40: last point a concrete type exists before `any` erasure.
                            self.reject_untagged_scalar_into_any(param_ty, &arg_ty, arg.span());
                            if let Err(diag) = self.engine.unify(param_ty, &arg_ty, arg.span()) {
                                self.diagnostics.push(diag);
                            }
                        }
                    }
                    // Charge the callee's own row AFTER the arguments are
                    // unified, for the same reason as instance dispatch: a
                    // static method that is row-polymorphic in a fn-typed
                    // parameter (`Type::run(f)`) carries a row mentioning that
                    // parameter's var, and the var only binds when the argument
                    // is unified against it. Charging first -- which is what this
                    // site used to do -- resolves a still-open row and charges
                    // nothing, so a privileged closure handed to a static method
                    // was never accounted for (LEDGER item 35). The failure is
                    // SILENT: the call is checked, it simply costs zero.
                    {
                        let own_row = self
                            .engine
                            .resolve_cap_row(&crate::ty::CapRow::var(sig.own_cap_var));
                        let ref_caps = self.engine.instantiate_row(&own_row, &cap_var_map);
                        self.accumulate_caps(&ref_caps);
                    }
                    self.engine.resolve(&inst_ret)
                } else if self.env.lookup_struct(type_name).is_none()
                    && self.env.lookup_enum(type_name).is_none()
                    && self.env.lookup_function(method).is_some()
                {
                    // MODULE-qualified call: `util::add(2, 3)` (or via an
                    // alias, `u::add(..)`). `type_name` is not a type, and the
                    // module's functions are imported under their plain names
                    // -- type this as a call of the plain function. The MIR
                    // lowering already resolves it the same way; only the
                    // checker rejected it ("no method `add` on type `util`").
                    // @pure enforcement: this is a free-function call in disguise,
                    // so apply the same name-based purity check as a bare call.
                    self.check_pure_free_call(method, *span);
                    let sig = self.env.lookup_function(method).cloned().unwrap();
                    let (params, ret, var_map, cap_var_map) = self.engine.instantiate_sig(&sig);
                    for (old_id, new_id) in &var_map {
                        if let Some(bounds) = self.generic_var_bounds.get(old_id).cloned() {
                            self.engine.set_var_bounds(*new_id, bounds);
                        }
                    }
                    {
                        let own_row = self
                            .engine
                            .resolve_cap_row(&crate::ty::CapRow::var(sig.own_cap_var));
                        let ref_caps = self.engine.instantiate_row(&own_row, &cap_var_map);
                        self.accumulate_caps(&ref_caps);
                    }
                    if args.len() != params.len() {
                        self.error(
                            format!(
                                "function `{method}` expects {} arguments, found {}",
                                params.len(),
                                args.len()
                            ),
                            *span,
                        );
                    } else {
                        for (arg, param_ty) in args.iter().zip(params.iter()) {
                            let arg_ty = self.infer_expr(arg);
                            // LEDGER item 40: last point a concrete type exists before `any` erasure.
                            self.reject_untagged_scalar_into_any(param_ty, &arg_ty, arg.span());
                            if let Err(diag) = self.engine.unify(param_ty, &arg_ty, arg.span()) {
                                self.diagnostics.push(diag);
                            }
                        }
                    }
                    ret
                } else {
                    self.error(
                        format!("no method `{method}` found on type `{type_name}`"),
                        *span,
                    );
                    let methods = self.env.all_method_names(type_name);
                    if let Some(s) =
                        crate::suggest::closest_match(method, methods.iter().map(|s| s.as_str()))
                    {
                        if let Some(diag) = self.diagnostics.last_mut() {
                            diag.notes.push(format!("did you mean `{s}`?"));
                        }
                    }
                    Type::Error
                }
            }

            // Literals: array, tuple, map, struct.
            Expr::ArrayLiteral { elements, span } => {
                if elements.is_empty() {
                    Type::Array {
                        element: Box::new(self.engine.fresh_var()),
                        size: Some(0),
                    }
                } else {
                    let first_ty = self.infer_expr(&elements[0]);
                    // See `suppress_array_elem_unify`: when the enclosing
                    // declared/parameter type for THIS literal already
                    // resolved to Type::Error (e.g. `[dyn Handler]`,
                    // rejected by `reject_dyn_in_container` with E0110),
                    // forcing every element to unify with the first one is
                    // redundant noise on top of the real diagnostic -- still
                    // infer each element (so ITS OWN errors, if any, still
                    // surface), just skip the cross-element unify.
                    let skip_unify = self.suppress_array_elem_unify.contains(span);
                    for elem in &elements[1..] {
                        let elem_ty = self.infer_expr(elem);
                        if !skip_unify {
                            if let Err(diag) = self.engine.unify(&first_ty, &elem_ty, *span) {
                                self.diagnostics.push(diag);
                            }
                        }
                    }
                    Type::Array {
                        element: Box::new(first_ty),
                        size: Some(elements.len() as u64),
                    }
                }
            }
            Expr::TupleLiteral { elements, .. } => Type::Tuple {
                elements: elements.iter().map(|e| self.infer_expr(e)).collect(),
            },
            Expr::MapLiteral { entries, span } => {
                if entries.is_empty() {
                    Type::Map {
                        key: Box::new(self.engine.fresh_var()),
                        value: Box::new(self.engine.fresh_var()),
                    }
                } else {
                    let (first_key, first_val) = &entries[0];
                    let key_ty = self.infer_expr(first_key);
                    let val_ty = self.infer_expr(first_val);
                    for (k, v) in &entries[1..] {
                        let kt = self.infer_expr(k);
                        let vt = self.infer_expr(v);
                        if let Err(diag) = self.engine.unify(&key_ty, &kt, *span) {
                            self.diagnostics.push(diag);
                        }
                        if let Err(diag) = self.engine.unify(&val_ty, &vt, *span) {
                            self.diagnostics.push(diag);
                        }
                    }
                    Type::Map {
                        key: Box::new(key_ty),
                        value: Box::new(val_ty),
                    }
                }
            }
            Expr::StructLiteral { name, fields, span } => {
                // Duplicate field in the LITERAL (`Point { x: 1.0, x: 2.0 }`)
                // -- classic copy-paste typo; which value won was
                // implementation-defined before this check.
                {
                    let mut seen_fields = std::collections::HashSet::new();
                    for (fname, _) in fields {
                        if !seen_fields.insert(fname.as_str()) {
                            self.error(
                                format!(
                                    "duplicate field `{fname}` in `{name}` literal -- each field may be set only once"
                                ),
                                *span,
                            );
                        }
                    }
                }
                if self.actor_names.contains(name) {
                    self.error(
                        format!(
                            "`{name}` is an actor; construct it with `{name}()` -- actor state is private and initializes to zero"
                        ),
                        *span,
                    );
                    for (_f, e) in fields {
                        let _ = self.infer_expr(e);
                    }
                    return Type::Struct {
                        name: name.clone(),
                        generics: vec![],
                    };
                }
                if let Some(def) = self.env.lookup_struct(name).cloned() {
                    // Per-use monomorphization: build a var_map from the def's
                    // generic var IDs to fresh vars, then instantiate each
                    // field's expected type before unifying.
                    let mut var_map = std::collections::HashMap::new();
                    let mut fresh_generics = Vec::with_capacity(def.generic_var_ids.len());
                    for &old_id in &def.generic_var_ids {
                        let fresh = self.engine.fresh_var();
                        if let Type::Var(new_id) = &fresh {
                            var_map.insert(old_id, *new_id);
                        }
                        fresh_generics.push(fresh);
                    }
                    for (fname, fexpr) in fields {
                        let expr_ty = self.infer_expr(fexpr);
                        if let Some((_, expected_ty)) = def.fields.iter().find(|(n, _)| n == fname)
                        {
                            let expected_instantiated = if var_map.is_empty() {
                                expected_ty.clone()
                            } else {
                                self.engine.instantiate(expected_ty, &var_map)
                            };
                            if let Err(diag) =
                                self.engine
                                    .unify(&expected_instantiated, &expr_ty, fexpr.span())
                            {
                                self.diagnostics.push(diag);
                            }
                        } else {
                            self.error(format!("no field `{fname}` on struct `{name}`"), *span);
                        }
                    }
                    // Missing fields: every declared field must be set. Kryos
                    // has no default-value / partial-initialization feature, so
                    // an omitted field was left as zeroed/null memory -- a
                    // missing nested-struct field SEGFAULTED on first access,
                    // and a missing str/array field yielded a null handle.
                    // Reject the omission at check time (mirrors the existing
                    // extra-/unknown-field rejection above).
                    let missing: Vec<&str> = def
                        .fields
                        .iter()
                        .map(|(n, _)| n.as_str())
                        .filter(|dn| !fields.iter().any(|(fn_, _)| fn_ == dn))
                        .collect();
                    if !missing.is_empty() {
                        let list = missing
                            .iter()
                            .map(|f| format!("`{f}`"))
                            .collect::<Vec<_>>()
                            .join(", ");
                        self.error_with_code(
                            format!(
                                "missing field{} {list} in `{name}` literal -- every field must be initialized (Kryos has no default field values)",
                                if missing.len() == 1 { "" } else { "s" }
                            ),
                            *span,
                            kryos_errors::codes::E0100,
                        );
                    }
                    Type::Struct {
                        name: name.clone(),
                        generics: fresh_generics,
                    }
                } else {
                    self.error_with_code(
                        format!("unknown struct `{name}`"),
                        *span,
                        kryos_errors::codes::E0103,
                    );
                    let known = self.env.all_struct_names();
                    if let Some(suggestion) =
                        crate::suggest::closest_match(name, known.iter().map(|s| s.as_str()))
                    {
                        if let Some(diag) = self.diagnostics.last_mut() {
                            diag.notes.push(format!("did you mean `{suggestion}`?"));
                        }
                    }
                    Type::Error
                }
            }

            // Lambda.
            Expr::Lambda {
                params,
                ret_ty,
                body,
                span: lambda_span,
            } => {
                self.check_duplicate_params(params, "this closure");

                // Bidirectional inference: if an outer FnCall pushed expected
                // types for this lambda, use them for any un-annotated params
                // and the return type. This is what makes `|a, b| a + b` work
                // when passed to a function expecting `fn(i64, i64) -> i64`.
                let expected = self.lambda_expected_types.remove(lambda_span);

                let param_types: Vec<Type> = params
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        if let Some(t) = p.ty.as_ref() {
                            self.resolve_type_expr(t)
                        } else if let Some((ref eps, _)) = expected {
                            if let Some(et) = eps.get(i) {
                                et.clone()
                            } else {
                                self.engine.fresh_var()
                            }
                        } else {
                            self.engine.fresh_var()
                        }
                    })
                    .collect();

                let ret = if let Some(t) = ret_ty.as_ref() {
                    self.resolve_type_expr(t)
                } else if let Some((_, ref er)) = expected {
                    er.clone()
                } else {
                    self.engine.fresh_var()
                };

                // Set current_return_type so `return` statements inside the
                // lambda body are validated against the declared return type.
                let prev_ret = self.current_return_type.take();
                self.current_return_type = Some(ret.clone());

                // Pre-bind a pending self-recursive name (set by `Stmt::Let`
                // just before this lambda was checked) to this lambda's own
                // Function type, in the CURRENT (enclosing) scope -- i.e.
                // BEFORE pushing the body's scope below. `TypeEnv::lookup_var`
                // walks scopes innermost-to-outermost, so a call to `name`
                // inside the body falls through the body scope (which only
                // has the params) and finds this binding in the enclosing
                // scope. Consumed (take) so a nested lambda inside this body
                // does not also pick it up. The immediately-following
                // `Stmt::Let` in the caller redefines `name` in this same
                // scope with the fully-resolved type once this lambda
                // finishes checking, so nothing needs cleanup here.
                if let Some(self_name) = self.pending_self_recursive_name.take() {
                    self.env.define_var(
                        self_name,
                        Type::Function {
                            params: param_types.clone(),
                            ret: Box::new(ret.clone()),
                            // Self-recursive row polymorphism (a fixed-point
                            // HOF whose own row varies across its own
                            // recursive calls) is an explicit open item,
                            // spec §10 -- default to empty here rather than
                            // guessing; the lambda's OWN caps (computed
                            // below, after its body is walked) is what
                            // actually gets attached to the value once
                            // checking finishes.
                            caps: crate::ty::CapRow::empty(),
                        },
                    );
                }

                self.env.push_scope();
                for (param, ty) in params.iter().zip(param_types.iter()) {
                    self.env.define_var(param.name.clone(), ty.clone());
                }
                self.cap_accum_stack.push(crate::ty::CapRow::empty());
                let body_ty = self.infer_expr(body);
                let lambda_caps = self.cap_accum_stack.pop().unwrap_or_else(crate::ty::CapRow::empty);
                self.env.pop_scope();

                self.current_return_type = prev_ret;

                // Record resolved types for UN-annotated params (str/struct/
                // enum/array/... AND floats) so MIR can type the closure's
                // params instead of defaulting them to i64 -- the cause of a
                // `str` closure passed to a HOF (`fold(xs, "", |acc, x| acc+x)`)
                // miscompiling its concatenation, and of an f64 closure
                // (`map(xs, |x| x*2.0)`) lowering `*` to an integer `imul`.
                // Floats are i64-slot-passed through the uniform closure ABI;
                // both backends' env-thunks already bit-cast the i64 slot to/from
                // the real float param/return type, so typing the param is enough.
                {
                    let resolved: Vec<Option<TypeExpr>> = params
                        .iter()
                        .zip(param_types.iter())
                        .map(|(p, pt)| {
                            if p.ty.is_some() {
                                return None;
                            }
                            let r = self.engine.resolve(pt);
                            if is_flowable_param_type(&r) {
                                concrete_type_to_type_expr(&r)
                            } else {
                                None
                            }
                        })
                        .collect();
                    if resolved.iter().any(|x| x.is_some()) {
                        self.resolved_lambda_params.insert(*lambda_span, resolved);
                    }
                }

                // If the body evaluates to Void (e.g. block ending with `return`),
                // the return statements already validated against `ret`.
                // Only unify when body produces a non-void expression result.
                if body_ty != Type::Void {
                    if let Err(diag) = self.engine.unify(&ret, &body_ty, body.span()) {
                        self.diagnostics.push(diag);
                    }
                }

                self.log_fn_effect(
                    format!("<lambda@{}:{}>", lambda_span.start, lambda_span.end),
                    *lambda_span,
                    lambda_caps.clone(),
                );

                Type::Function {
                    params: param_types,
                    ret: Box::new(ret),
                    caps: lambda_caps,
                }
            }

            // If expression.
            Expr::IfExpr {
                condition,
                then_branch,
                else_branch,
                span,
            } => {
                let cond_ty = self.infer_expr(condition);
                if let Err(diag) = self.engine.unify(&Type::Bool, &cond_ty, *span) {
                    self.diagnostics.push(diag);
                }
                self.env.push_scope();
                let then_ty = self.infer_block_as_expr(then_branch);
                self.env.pop_scope();
                if let Some(else_blk) = else_branch {
                    self.env.push_scope();
                    let else_ty = self.infer_block_as_expr(else_blk);
                    self.env.pop_scope();
                    // A branch that always diverges (ends in `throw`/`return`,
                    // or all-diverging nested if/match) never produces a
                    // value -- `infer_block_as_expr` types it `void` because
                    // it falls off the end of the loop, but that's not a
                    // real value type to unify against the other branch.
                    // Exclude a divergent branch from unification and take
                    // the other (non-divergent) branch's type as the whole
                    // if-expression's type -- e.g. `if c { "pos" } else {
                    // throw "neg!" }` types as `str`, not `void`. If BOTH
                    // branches diverge, the expression itself never
                    // produces a value; degrade to `void` (bottom).
                    let then_diverges = block_diverges(then_branch);
                    let else_diverges = block_diverges(else_blk);
                    match (then_diverges, else_diverges) {
                        (true, true) => Type::Void,
                        (true, false) => else_ty,
                        (false, true) => then_ty,
                        (false, false) => {
                            if let Err(diag) = self.engine.unify(&then_ty, &else_ty, *span) {
                                self.diagnostics.push(diag);
                            }
                            then_ty
                        }
                    }
                } else {
                    // No else branch: if-expr without else is void.
                    Type::Void
                }
            }

            // Match expression.
            Expr::MatchExpr {
                subject,
                arms,
                span,
            } => {
                let subject_ty = self.infer_expr(subject);
                if arms.is_empty() {
                    return Type::Void;
                }
                // Unreachable-arm check: any arm AFTER an unguarded catch-all
                // (`_` wildcard or a bare binding `x =>`) can never run. This
                // must be a hard error, not a lint: the switch-dispatch
                // lowering is value-directed, so a specific arm placed after
                // an early `_` previously WON at runtime (`match n { _ =>
                // "wild", 0 => "zero" }` on 0 printed "zero") -- silently
                // violating first-match-wins. Rejecting the dead arm keeps
                // both backends trivially consistent.
                // A bare `Pattern::Ident` is only a catch-all when it is a
                // real BINDING -- a bare enum-VARIANT arm (`Red =>`, `Some =>`)
                // parses as Ident too but is refutable. Classify against the
                // resolved subject type, conservatively (an unresolved subject
                // never flags).
                let subj_resolved = self.engine.resolve(&subject_ty);
                let arm_catch_all: Vec<bool> = {
                    let ident_is_catch_all = |n: &str| -> bool {
                        match &subj_resolved {
                            Type::Enum { name: en, .. } => self
                                .env
                                .lookup_enum(en)
                                .map(|d| !d.variants.iter().any(|(v, _)| v == n))
                                .unwrap_or(false),
                            Type::Option { .. } => n != "Some" && n != "None",
                            Type::Result { .. } => n != "Ok" && n != "Err",
                            Type::Var(_) | Type::Error => false,
                            _ => true,
                        }
                    };
                    arms.iter()
                        .map(|arm| match &arm.pattern {
                            Pattern::Wildcard { .. } => true,
                            Pattern::Ident { name, .. } => ident_is_catch_all(name),
                            _ => false,
                        })
                        .collect()
                };
                let mut catch_all_at: Option<usize> = None;
                for (i, arm) in arms.iter().enumerate() {
                    if let Some(prev) = catch_all_at {
                        let what = if matches!(&arms[prev].pattern, Pattern::Wildcard { .. }) {
                            "a wildcard `_`".to_string()
                        } else {
                            "a binding".to_string()
                        };
                        self.error(
                            format!(
                                "unreachable match arm: {what} arm above already matches every value -- move the catch-all arm last or delete this arm"
                            ),
                            arm.body.span(),
                        );
                        break;
                    }
                    if arm.guard.is_none() && arm_catch_all[i] && i + 1 < arms.len() {
                        catch_all_at = Some(i);
                    }
                }
                // A GUARDED catch-all placed BETWEEN specific arms of an
                // ENUM/Option/Result match cannot be honored by the tag-
                // dispatch lowering (the switch jumps straight to the
                // scrutinee's tag arm, silently skipping it even when the
                // guard is true). A LEADING guarded catch-all is fine (it is
                // hoisted to an ordered pre-switch test), and scalar subjects
                // use the order-respecting sequential path. Reject the one
                // unsupported shape with guidance rather than mis-dispatch.
                if matches!(
                    subj_resolved,
                    Type::Enum { .. } | Type::Option { .. } | Type::Result { .. }
                ) {
                    let mut seen_specific = false;
                    for (i, arm) in arms.iter().enumerate() {
                        if !arm_catch_all[i] {
                            seen_specific = true;
                        } else if arm.guard.is_some() && seen_specific {
                            self.error(
                                format!(
                                    "a guarded catch-all arm between specific variant arms is not supported by enum tag dispatch yet -- move it BEFORE the variant arms (it runs as an ordered pre-check there) or restructure the guard into the variant arms"
                                ),
                                arm.body.span(),
                            );
                            break;
                        }
                    }
                }
                let mut result_ty: Option<Type> = None;
                for arm in arms {
                    self.env.push_scope();
                    self.check_arm_variant_typo(&arm.pattern, &subject_ty);
                    self.bind_pattern(&arm.pattern, &subject_ty);
                    let arm_ty = self.infer_expr(&arm.body);
                    // An arm body that diverges (always returns/throws) does
                    // not contribute to the match expression's type.  This
                    // lets the `?` operator desugar - whose Err arm is
                    // `{ return Err(e) }` of type Void - coexist with an Ok
                    // arm that yields the unwrapped value.
                    let arm_diverges = arm_body_diverges(&arm.body);
                    self.env.pop_scope();

                    if arm_diverges {
                        continue;
                    }

                    if let Some(ref first) = result_ty {
                        if let Err(diag) = self.engine.unify(first, &arm_ty, *span) {
                            self.diagnostics.push(diag);
                        }
                    } else {
                        result_ty = Some(arm_ty);
                    }
                }

                // --- Exhaustiveness check ---
                let resolved = self.engine.resolve(&subject_ty);
                let type_name = match &resolved {
                    Type::Bool => "bool".to_string(),
                    Type::I8 => "i8".to_string(),
                    Type::I16 => "i16".to_string(),
                    Type::I32 => "i32".to_string(),
                    Type::I64 => "i64".to_string(),
                    Type::I128 => "i128".to_string(),
                    Type::U8 => "u8".to_string(),
                    Type::U16 => "u16".to_string(),
                    Type::U32 => "u32".to_string(),
                    Type::U64 => "u64".to_string(),
                    Type::U128 => "u128".to_string(),
                    Type::Str => "str".to_string(),
                    Type::Char => "char".to_string(),
                    Type::Struct { name, .. } | Type::Enum { name, .. } => name.clone(),
                    // Option/Result are distinct builtin Type variants, not
                    // Type::Enum, so they were falling through to "" and skipping
                    // exhaustiveness entirely -- a match on Option/Result missing
                    // a None/Err arm (no wildcard) compiled clean and hit an
                    // unhandled case at runtime. Route them to the finite-variant
                    // exhaustiveness check.
                    Type::Option { .. } => "Option".to_string(),
                    Type::Result { .. } => "Result".to_string(),
                    _ => String::new(),
                };
                if !type_name.is_empty() {
                    let enum_def = self.env.lookup_enum(&type_name).cloned();
                    // Only GUARD-FREE arms guarantee coverage. An arm with a guard
                    // (`Pos(x) if x > 100`) may not match at runtime, so it must
                    // NOT count its variant/value as covered -- otherwise a
                    // guard-narrowed arm silently made the match look exhaustive
                    // and `Pos(5)` hit no arm at runtime. (P-03/P-04)
                    let pats: Vec<&kryos_ast::Pattern> = arms
                        .iter()
                        .filter(|a| a.guard.is_none())
                        .map(|a| &a.pattern)
                        .collect();
                    let warnings = crate::exhaustive::check_exhaustive(
                        &type_name,
                        &pats,
                        enum_def.as_ref(),
                        *span,
                    );
                    self.diagnostics.extend(warnings);
                }

                result_ty.unwrap_or(Type::Void)
            }

            // Range expression.
            Expr::RangeExpr {
                start, end, span, ..
            } => {
                let start_ty = start
                    .as_ref()
                    .map(|e| self.infer_expr(e))
                    .unwrap_or(Type::I32);
                let end_ty = end
                    .as_ref()
                    .map(|e| self.infer_expr(e))
                    .unwrap_or(Type::I32);
                if let Err(diag) = self.engine.unify(&start_ty, &end_ty, *span) {
                    self.diagnostics.push(diag);
                }
                // Range type is a struct-like type.
                Type::Struct {
                    name: "Range".to_string(),
                    generics: vec![start_ty],
                }
            }

            // Pipe expression (left |> right).
            Expr::PipeExpr { left, right, span } => {
                // Desugar to a REAL FnCall and type-check THAT, exactly as the
                // MIR lowering's `desugar_pipe` does: `a |> f` -> `f(a)` and
                // `a |> f(b, c)` -> `f(a, b, c)`. The old version only handled a
                // bare-function RHS (`a |> f`); a multi-arg target `5 |> add(10)`
                // type-checked `add(10)` standalone (arity error) and then
                // required the result to be a Function -- so any pipe with
                // explicit extra args was rejected despite compiling in MIR.
                let call = match right.as_ref() {
                    Expr::FnCall {
                        callee,
                        args,
                        span: cspan,
                    } => {
                        let mut all_args = vec![(**left).clone()];
                        all_args.extend(args.iter().cloned());
                        Expr::FnCall {
                            callee: callee.clone(),
                            args: all_args,
                            span: *cspan,
                        }
                    }
                    _ => Expr::FnCall {
                        callee: right.clone(),
                        args: vec![(**left).clone()],
                        span: *span,
                    },
                };
                self.infer_expr(&call)
            }

            // Borrow expression: &x or &mut x.
            Expr::Borrow { inner, mutable, .. } => {
                let inner_ty = self.infer_expr(inner);
                Type::Reference {
                    inner: Box::new(inner_ty),
                    mutable: *mutable,
                }
            }
            // Dereference expression: *x.
            Expr::Deref { inner, span } => {
                let deref_span = *span;
                let inner_ty = self.infer_expr(inner);
                match inner_ty {
                    Type::Reference { inner, .. } => *inner,
                    Type::Pointer { inner, .. } => {
                        // Dereferencing a RAW pointer is an unsafe operation: it
                        // can read arbitrary memory. Require an enclosing
                        // `unsafe { }` block (E0500). References/Shared are safe.
                        if self.unsafe_depth == 0 {
                            self.error_with_code(
                                "dereference of raw pointer requires an `unsafe` block"
                                    .to_string(),
                                deref_span,
                                kryos_errors::codes::E0500,
                            );
                        }
                        *inner
                    }
                    Type::Shared { inner } => *inner,
                    _ => {
                        self.error(
                            format!("cannot dereference value of type `{inner_ty}`"),
                            inner.span(),
                        );
                        Type::Error
                    }
                }
            }

            // Shared / Move / Weak expressions.
            Expr::SharedExpr { inner, .. } => {
                let inner_ty = self.infer_expr(inner);
                Type::Shared {
                    inner: Box::new(inner_ty),
                }
            }
            Expr::MoveExpr { inner, .. } => self.infer_expr(inner),
            Expr::WeakExpr { inner, .. } => {
                let inner_ty = self.infer_expr(inner);
                Type::Weak {
                    inner: Box::new(inner_ty),
                }
            }

            // Cast expression.
            Expr::Cast { expr, ty, span } => {
                // Check the source expression, then return the target type.
                let src = self.infer_expr(expr);
                // `as f32` is a legal scalar-f32 position; bypass the
                // composite-position rejection in resolve_type_expr.
                let dst = if matches!(ty, TypeExpr::Simple { name, .. } if name == "f32") {
                    Type::F32
                } else {
                    self.resolve_type_expr(ty)
                };
                // Enforce the closed cast set (docs/19-language-reference.md
                // §3.1): integer<->integer, integer<->float, bool->integer,
                // char<->integer, and reference->raw-pointer. Casting to/from
                // an aggregate or `str` was accepted and bit-reinterpreted the
                // handle -- `x as str` fed a raw i64 to string code and
                // SEGFAULTED, `struct as i64` leaked a heap pointer into
                // program output. Anything outside the scalar cast set (and
                // not a pointer/reference cast, which is unsafe territory left
                // permissive) is a hard type error. Var/Error operands are
                // left alone (inference incomplete / already reported).
                fn is_scalar_castable(t: &Type) -> bool {
                    matches!(
                        t,
                        Type::I8
                            | Type::I16
                            | Type::I32
                            | Type::I64
                            | Type::I128
                            | Type::U8
                            | Type::U16
                            | Type::U32
                            | Type::U64
                            | Type::U128
                            | Type::F32
                            | Type::F64
                            | Type::Bool
                            | Type::Char
                            | Type::USize
                            | Type::ISize
                    )
                }
                fn is_ptr_like(t: &Type) -> bool {
                    matches!(
                        t,
                        Type::Pointer { .. }
                            | Type::Reference { .. }
                            | Type::Shared { .. }
                            | Type::Weak { .. }
                    )
                }
                let rsrc = self.engine.resolve(&src);
                let rdst = self.engine.resolve(&dst);
                let inference_pending = matches!(rsrc, Type::Var(_) | Type::Error)
                    || matches!(rdst, Type::Var(_) | Type::Error);
                if !inference_pending {
                    let ptr_cast = is_ptr_like(&rsrc) || is_ptr_like(&rdst);
                    let scalar_cast =
                        is_scalar_castable(&rsrc) && is_scalar_castable(&rdst);
                    if !ptr_cast && !scalar_cast {
                        self.error_with_code(
                            format!(
                                "cannot cast `{rsrc}` to `{rdst}` -- the cast set is integer<->integer, integer<->float, bool->integer, and char<->integer; aggregate and string types cannot be cast (use a conversion function like `to_string`)"
                            ),
                            *span,
                            kryos_errors::codes::E0100,
                        );
                    }
                }
                dst
            }

            // Block expression - type is the type of the last expression.
            Expr::Block { block, .. } => {
                self.env.push_scope();
                // The block's type is its tail expression's type, computed
                // WHILE the block scope is still live -- locals bound earlier
                // in the block (e.g. `{ let a = 5; a + 1 }`) must be in scope
                // for the tail. (Previously the tail was re-inferred AFTER
                // popping the scope, so a tail referencing a block-local
                // failed with "undefined variable" -- which also broke
                // `spawn { let x = ...; use x }`.)
                let mut block_ty = Type::Void;
                let last_idx = block.stmts.len().wrapping_sub(1);
                for (i, stmt) in block.stmts.iter().enumerate() {
                    // A value-position block's tail Stmt::Expr is the block's VALUE,
                    // not a discard -- infer it directly and bypass check_stmt, so the
                    // W0400 must-use lint doesn't false-positive on a Tracked tail
                    // (mirrors infer_block_as_expr). Doing so also avoids the
                    // double-infer that the previous check_stmt + infer_expr produced.
                    if i == last_idx {
                        if let Stmt::Expr { expr, .. } = stmt {
                            block_ty = self.infer_expr(expr);
                            continue;
                        }
                        // A trailing `if ... else ...` is the block's tail
                        // VALUE (mirrors a bare if-expression). It parses as
                        // Stmt::If, so synthesize the equivalent IfExpr.
                        if let Some(if_expr) = stmt.as_value_if_expr() {
                            block_ty = self.infer_expr(&if_expr);
                            continue;
                        }
                        // A trailing `try { .. } catch e { .. }` is the
                        // block's tail VALUE too -- there is no `Expr::
                        // TryCatch` node to synthesize (try/catch is
                        // statement-only in the parser), so handle it
                        // directly. Without this, `let x = { try { 1 }
                        // catch e { 2 } }` typed as `void`, which got
                        // recorded into `resolved_let_types` and silently
                        // overrode the correct MIR-level type inference for
                        // `x` -- the LLVM backend's void-coercion fallback
                        // then substituted a literal `0` at every use site
                        // regardless of the runtime value.
                        if let Stmt::TryCatch {
                            try_block,
                            catch_name,
                            catch_block,
                            span,
                        } = stmt
                        {
                            block_ty = self.infer_try_catch_value_type(
                                try_block, catch_name, catch_block, *span,
                            );
                            continue;
                        }
                    }
                    self.check_stmt(stmt);
                }
                self.env.pop_scope();
                block_ty
            }

            // Comptime block - type is the type of the last expression.
            Expr::ComptimeBlock { body, .. } => {
                self.env.push_scope();
                let last_idx = body.stmts.len().wrapping_sub(1);
                let mut ty = Type::Void;
                for (i, stmt) in body.stmts.iter().enumerate() {
                    // Tail Stmt::Expr is the block's value, not a discard (see Expr::Block).
                    if i == last_idx {
                        if let Stmt::Expr { expr, .. } = stmt {
                            ty = self.infer_expr(expr);
                            continue;
                        }
                    }
                    // LEDGER item 42. `comptime` is NOT a compile-time evaluator
                    // (HANDOFF.md defers that past 1.0). MIR lowering keeps only
                    // the block's VALUE, so a non-tail statement inside it is
                    // handled inconsistently and SILENTLY: measured 2026-08-14,
                    // `println("INSIDE")` never reaches MIR at all and vanishes,
                    // while `n = 99` survives as `_0 = const 99_i64` and takes
                    // effect. A debug print disappearing while the mutation
                    // beside it lands is a silent-wrong-answer-shaped trap: the
                    // user reasonably concludes the block did not run, and is
                    // wrong.
                    //
                    // Rather than pick one of those two behaviours and pretend
                    // the keyword means it, reject the shapes that are not
                    // faithfully supported. Every real use in this repo (9 files:
                    // examples, the e2e/native test corpus) is
                    // `let x = comptime { <pure arithmetic> }`, which is the
                    // value form and stays legal. `let` bindings inside the block
                    // stay legal too -- they are scoped to the block and cannot
                    // be observed from outside it.
                    let offending = match stmt {
                        Stmt::Let { .. } => None,
                        Stmt::Assign { span, .. } => Some(("an assignment", *span)),
                        Stmt::Expr { span, .. } => Some(("a statement call", *span)),
                        Stmt::Return { span, .. } => Some(("a `return`", *span)),
                        Stmt::If { span, .. } => Some(("an `if` statement", *span)),
                        Stmt::For { span, .. } => Some(("a `for` loop", *span)),
                        Stmt::While { span, .. } => Some(("a `while` loop", *span)),
                        Stmt::Break { span, .. } => Some(("a `break`", *span)),
                        Stmt::Continue { span, .. } => Some(("a `continue`", *span)),
                        Stmt::Spawn { span, .. } => Some(("a `spawn`", *span)),
                        Stmt::Throw { span, .. } => Some(("a `throw`", *span)),
                        Stmt::TryCatch { span, .. } => Some(("a `try`/`catch`", *span)),
                        Stmt::DenyBlock { span, .. } => Some(("a `deny!` block", *span)),
                        _ => None,
                    };
                    if let Some((what, span)) = offending {
                        self.error_with_code(
                            format!(
                                "`comptime` does not support {what} -- it is not a compile-time evaluator yet, so a side-effecting statement here is silently dropped or silently applied depending on its shape (measured: a `println` never reaches codegen, an assignment does). Use `comptime {{ <expression> }}` for a compile-time-shaped VALUE, and move the statement outside the block"
                            ),
                            span,
                            kryos_errors::codes::E0110,
                        );
                    }
                    self.check_stmt(stmt);
                }
                self.env.pop_scope();
                ty
            }
            // Quantum blocks - check body, return void for now.
            Expr::QuantumBlock { body, .. } => {
                self.env.push_scope();
                self.check_block(body);
                self.env.pop_scope();
                Type::Void
            }

            // Unsafe block - a plain block whose type is its tail expression's,
            // but while it is being checked, raw-pointer dereference is allowed
            // (E0500 is suppressed). Semantically transparent to codegen.
            Expr::UnsafeBlock { body, .. } => {
                self.env.push_scope();
                self.unsafe_depth += 1;
                let last_idx = body.stmts.len().wrapping_sub(1);
                let mut ty = Type::Void;
                for (i, stmt) in body.stmts.iter().enumerate() {
                    if i == last_idx {
                        if let Stmt::Expr { expr, .. } = stmt {
                            ty = self.infer_expr(expr);
                            continue;
                        }
                    }
                    self.check_stmt(stmt);
                }
                self.unsafe_depth -= 1;
                self.env.pop_scope();
                ty
            }

            // Await expression - for now, just pass through the inner type.
            // A full implementation would unwrap Future<T> → T.
            Expr::Await { value, .. } => self.infer_expr(value),
        }
    }

    // ── Binary operator type checking ────────────────────────────────

    fn check_binary_op(&mut self, op: BinOp, left: &Expr, right: &Expr, span: Span) -> Type {
        // Integer bit-width for promotion; None for non-integers.
        fn int_width(t: &Type) -> Option<u8> {
            match t {
                Type::I8 | Type::U8 => Some(8),
                Type::I16 | Type::U16 => Some(16),
                Type::I32 | Type::U32 => Some(32),
                Type::I64 | Type::U64 => Some(64),
                Type::I128 | Type::U128 => Some(128),
                _ => None,
            }
        }
        // If both are concrete integer types of DIFFERENT widths, the wider
        // one is the arithmetic result (C-style promotion, no narrowing).
        fn wider_concrete_int(a: &Type, b: &Type) -> Option<Type> {
            match (int_width(a), int_width(b)) {
                (Some(wa), Some(wb)) if wa != wb => {
                    Some(if wa >= wb { a.clone() } else { b.clone() })
                }
                _ => None,
            }
        }
        let left_ty = self.infer_expr(left);
        let right_ty = self.infer_expr(right);

        match op {
            // Arithmetic: both sides must be the same numeric type.
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::Pow => {
                // Special case: array concatenation with +.
                if op == BinOp::Add {
                    let resolved_left = self.engine.resolve(&left_ty);
                    let resolved_right = self.engine.resolve(&right_ty);
                    if let (Type::Array { element: e1, .. }, Type::Array { element: e2, .. }) =
                        (&resolved_left, &resolved_right)
                    {
                        // Unify element types.
                        if let Err(diag) = self.engine.unify(e1, e2, span) {
                            self.diagnostics.push(diag);
                            return Type::Error;
                        }
                        let elem = self.engine.resolve(e1);
                        // Array concatenation always produces a dynamic array.
                        return Type::Array {
                            element: Box::new(elem),
                            size: None,
                        };
                    }
                }

                // Mixed-width integer arithmetic PROMOTES to the wider type
                // instead of unifying (which would either reject or pin both
                // to the narrower left width and truncate the result --
                // `i8 + i64` stored -65). If BOTH sides are concrete integer
                // types of different widths, skip the unify and return the
                // wider type; the codegen already widens the narrow operand
                // to the operation width.
                {
                    let rl = self.engine.resolve(&left_ty);
                    let rr = self.engine.resolve(&right_ty);
                    if let Some(wide) = wider_concrete_int(&rl, &rr) {
                        return wide;
                    }
                }
                if let Err(diag) = self.engine.unify(&left_ty, &right_ty, span) {
                    self.diagnostics.push(diag);
                    return Type::Error;
                }
                let resolved = self.engine.resolve(&left_ty);
                // Deferred numeric context: both operands are unconstrained
                // type vars (e.g. `|n| |x| x + n` - neither annotation nor
                // literal pins a type). Default to i64; the operands were
                // unified above so one binding fixes both.
                if let Type::Var(_) = resolved {
                    let _ = self.engine.unify(&resolved, &Type::I64, span);
                    return Type::I64;
                }
                if !resolved.is_numeric() && !resolved.is_error() {
                    // Special case: string concatenation with +.
                    if op == BinOp::Add && resolved == Type::Str {
                        return Type::Str;
                    }
                    let op_sym = match op {
                        BinOp::Add => "+",
                        BinOp::Sub => "-",
                        BinOp::Mul => "*",
                        BinOp::Div => "/",
                        BinOp::Mod => "%",
                        BinOp::Pow => "**",
                        _ => "?",
                    };
                    self.error(
                        format!(
                            "cannot apply `{op_sym}` to type `{resolved}`: expected a numeric type"
                        ),
                        span,
                    );
                    return Type::Error;
                }
                resolved
            }

            // Comparison: both sides same type, result is bool.
            BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => {
                if let Err(diag) = self.engine.unify(&left_ty, &right_ty, span) {
                    self.diagnostics.push(diag);
                }
                // `==`/`!=` on a struct or enum lowers to a synthesized
                // structural-equality helper (kryos-mir's
                // `ensure_struct_eq_helper`/`ensure_enum_eq_helper`) that
                // compares fields pairwise. Array/map fields don't have a
                // cheap, unsurprising structural comparison implemented yet
                // (elementwise array/map equality), so reject the comparison
                // here with a clear message rather than let it through to
                // codegen, where it would previously either silently compare
                // handles (JIT) or fail to build (AOT).
                if matches!(op, BinOp::Eq | BinOp::Neq) {
                    let resolved = self.engine.resolve(&left_ty);
                    let mut visited = std::collections::HashSet::new();
                    if self.contains_array_or_map(&resolved, &mut visited) {
                        let op_sym = if op == BinOp::Eq { "==" } else { "!=" };
                        self.error(
                            format!(
                                "cannot apply `{op_sym}` to type `{resolved}`: it has an array or map field (directly or nested) -- structural equality for array/map fields is not supported; compare those fields explicitly"
                            ),
                            span,
                        );
                        return Type::Error;
                    }
                }
                // ORDERING (< > <= >=) is defined only for scalars and
                // strings. On a struct/enum/array/map it type-checked clean
                // and codegen compared the raw HANDLES -- two equal-valued
                // structs ordered nondeterministically by heap address
                // (allocation-order dependent, differs across backends/runs).
                if matches!(op, BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq) {
                    let resolved = self.engine.resolve(&left_ty);
                    let ordered = resolved.is_numeric()
                        || matches!(
                            resolved,
                            Type::Str
                                | Type::Char
                                | Type::Bool
                                | Type::USize
                                | Type::ISize
                                | Type::Var(_)
                                | Type::Error
                        );
                    if !ordered {
                        let op_sym = match op {
                            BinOp::Lt => "<",
                            BinOp::Gt => ">",
                            BinOp::LtEq => "<=",
                            _ => ">=",
                        };
                        self.error(
                            format!(
                                "cannot apply `{op_sym}` to type `{resolved}`: ordering is defined for numbers, strings, and chars only -- compare specific fields instead"
                            ),
                            span,
                        );
                        return Type::Error;
                    }
                }
                Type::Bool
            }

            // Logical: both sides bool, result bool.
            BinOp::And | BinOp::Or => {
                if let Err(diag) = self.engine.unify(&Type::Bool, &left_ty, span) {
                    self.diagnostics.push(diag);
                }
                if let Err(diag) = self.engine.unify(&Type::Bool, &right_ty, span) {
                    self.diagnostics.push(diag);
                }
                Type::Bool
            }

            // Bitwise: both sides same integer type.
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                if let Err(diag) = self.engine.unify(&left_ty, &right_ty, span) {
                    self.diagnostics.push(diag);
                    return Type::Error;
                }
                let resolved = self.engine.resolve(&left_ty);
                // Deferred integer context - same Var-defaulting as the
                // arithmetic branch (`|n| |x| x & n`).
                if let Type::Var(_) = resolved {
                    let _ = self.engine.unify(&resolved, &Type::I64, span);
                    return Type::I64;
                }
                if !resolved.is_integer() && !resolved.is_error() {
                    let op_sym = match op {
                        BinOp::BitAnd => "&",
                        BinOp::BitOr => "|",
                        BinOp::BitXor => "^",
                        BinOp::Shl => "<<",
                        BinOp::Shr => ">>",
                        _ => "?",
                    };
                    self.error(
                        format!("cannot apply `{op_sym}` to type `{resolved}`: expected an integer type"),
                        span,
                    );
                    return Type::Error;
                }
                resolved
            }

            // Pipe is handled separately in infer_expr.
            BinOp::Pipe => {
                // Should not reach here - PipeExpr handles pipes.
                self.engine.resolve(&right_ty)
            }

            // Matrix multiply - both sides numeric.
            BinOp::MatMul => {
                if let Err(diag) = self.engine.unify(&left_ty, &right_ty, span) {
                    self.diagnostics.push(diag);
                }
                self.engine.resolve(&left_ty)
            }
        }
    }
}

// ── Public API ───────────────────────────────────────────────────────

/// Type-check a module, returning all diagnostics found.
/// True when `ty` is a type whose un-annotated closure param we want to flow
/// into MIR rather than defaulting to i64: pointer/heap-represented types
/// (i64-register-sized, no ABI change) plus floats (passed in an i64 slot
/// through the uniform closure ABI; the env-thunk bit-casts the slot to/from
/// the real float type on both backends). Plain integers/bool already work as
/// i64 and are excluded.
fn is_flowable_param_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Str
            | Type::Struct { .. }
            | Type::Enum { .. }
            | Type::Array { .. }
            | Type::Tuple { .. }
            | Type::Map { .. }
            | Type::Option { .. }
            | Type::Result { .. }
            | Type::Reference { .. }
            | Type::Shared { .. }
            | Type::Pointer { .. }
            | Type::F32
            | Type::F64
            | Type::Function { .. }
    )
}

/// Convert a resolved concrete `Type` to an AST `TypeExpr` for annotating a
/// lambda param. Returns None for unresolved vars or shapes we can't represent.
fn concrete_type_to_type_expr(ty: &Type) -> Option<TypeExpr> {
    let sp = Span::DUMMY;
    let simple = |n: &str| TypeExpr::Simple {
        name: n.to_string(),
        span: sp,
    };
    let generic = |n: &str, args: Vec<TypeExpr>| TypeExpr::Generic {
        name: n.to_string(),
        args,
        span: sp,
    };
    Some(match ty {
        Type::I8 => simple("i8"),
        Type::I16 => simple("i16"),
        Type::I32 => simple("i32"),
        Type::I64 => simple("i64"),
        Type::I128 => simple("i128"),
        Type::U8 => simple("u8"),
        Type::U16 => simple("u16"),
        Type::U32 => simple("u32"),
        Type::U64 => simple("u64"),
        Type::U128 => simple("u128"),
        Type::F32 => simple("f32"),
        Type::F64 => simple("f64"),
        Type::Bool => simple("bool"),
        Type::Char => simple("char"),
        Type::Str => simple("str"),
        Type::USize => simple("usize"),
        Type::ISize => simple("isize"),
        Type::Struct { name, generics } if generics.is_empty() => simple(name),
        Type::Enum { name, generics } if generics.is_empty() => simple(name),
        Type::Struct { name, generics } | Type::Enum { name, generics } => generic(
            name,
            generics
                .iter()
                .map(concrete_type_to_type_expr)
                .collect::<Option<Vec<_>>>()?,
        ),
        Type::Array { element, size } => TypeExpr::Array {
            element: Box::new(concrete_type_to_type_expr(element)?),
            size: *size,
            span: sp,
        },
        Type::Tuple { elements } => TypeExpr::Tuple {
            elements: elements
                .iter()
                .map(concrete_type_to_type_expr)
                .collect::<Option<Vec<_>>>()?,
            span: sp,
        },
        Type::Map { key, value } => generic(
            "map",
            vec![
                concrete_type_to_type_expr(key)?,
                concrete_type_to_type_expr(value)?,
            ],
        ),
        Type::Option { inner } => generic("Option", vec![concrete_type_to_type_expr(inner)?]),
        Type::Result { ok, err } => generic(
            "Result",
            vec![
                concrete_type_to_type_expr(ok)?,
                concrete_type_to_type_expr(err)?,
            ],
        ),
        // Closure element/param types: lets an UNTYPED `let mut fns = []`
        // holding pushed closures export its resolved `[fn(..) -> R]` element
        // type to MIR, so `fns[k]()`'s return is typed (was erased to i64 --
        // a str/f64-returning closure read out of an untyped array printed
        // raw handle bits; CLAUDE.md untyped-closure-array gotcha).
        Type::Function { params, ret, .. } => TypeExpr::Function {
            params: params
                .iter()
                .map(concrete_type_to_type_expr)
                .collect::<Option<Vec<_>>>()?,
            ret: Box::new(concrete_type_to_type_expr(ret)?),
            span: sp,
        },
        _ => return None,
    })
}

pub fn type_check(module: &Module) -> Vec<Diagnostic> {
    type_check_with_lambda_params(module).0
}

/// Type-check a module and also return the resolved types of un-annotated
/// lambda parameters (keyed by lambda span), for the MIR lowering to
/// consume.
///
/// Resource-DoS guard (LEDGER item 19, 2026-08-05): a pathological generic
/// instantiation (a generic that pairs/duplicates its own type parameter
/// across a chain of calls, e.g. `fn dup<T>(x: T) -> (T, T)` chained) makes
/// `InferenceEngine::resolve` -- called deep inside `type_check_with_lambda_
/// params_inner`'s unification passes -- deliberately PANIC with the shared
/// `kryos_errors::ResourceLimitExceeded` payload once a bounded type-tree-
/// size limit is hit, rather than let the process exhaust memory or hang
/// unresponsive with no diagnostic (see `resolve`'s own doc comment for the
/// full mechanism). This is the SINGLE point where every caller of
/// `type_check`/`type_check_with_lambda_params` (the driver's compile
/// pipeline, `check_file_with_options_full`, `check_source`, and the LSP's
/// diagnostics pass) gets that panic caught and turned into an ordinary
/// `error[E0113]` diagnostic instead of a raw Rust panic -- any OTHER panic
/// payload is a genuine internal-compiler-error and is re-raised via
/// `resume_unwind` so it still surfaces as a real crash, never silently
/// swallowed.
pub fn type_check_with_lambda_params(
    module: &Module,
) -> (
    Vec<Diagnostic>,
    std::collections::HashMap<Span, Vec<Option<TypeExpr>>>,
    std::collections::HashMap<Span, TypeExpr>,
) {
    match kryos_errors::ResourceLimitExceeded::catch(std::panic::AssertUnwindSafe(|| {
        type_check_with_lambda_params_inner(module)
    })) {
        Ok(result) => result,
        Err(limit) => (
            vec![Diagnostic::error(limit.message).with_code(kryos_errors::codes::E0113)],
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
        ),
    }
}

fn type_check_with_lambda_params_inner(
    module: &Module,
) -> (
    Vec<Diagnostic>,
    std::collections::HashMap<Span, Vec<Option<TypeExpr>>>,
    std::collections::HashMap<Span, TypeExpr>,
) {
    let mut checker = TypeChecker::new();

    // Register built-in functions that are always available.
    // println/print/eprintln accept any type - codegen converts non-string
    // args to strings via kryos_builtin_to_string at call time.
    checker.env.define_function(FunctionSig {
        name: "println".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("value".to_string(), Type::Error)],
        ret: Type::Void,
    });

    // print(value: any) -> void  (no newline)
    checker.env.define_function(FunctionSig {
        name: "print".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("value".to_string(), Type::Error)],
        ret: Type::Void,
    });

    // eprintln(value: any) -> void  (stderr + newline)
    checker.env.define_function(FunctionSig {
        name: "eprintln".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("value".to_string(), Type::Error)],
        ret: Type::Void,
    });

    // exit(code: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "exit".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("code".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // range(start, end) -> [i64]  (conceptually returns an integer sequence;
    // the MIR lowering special-cases this into a counter loop)
    checker.env.define_function(FunctionSig {
        name: "range".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("start".to_string(), Type::I64),
            ("end".to_string(), Type::I64),
        ],
        ret: Type::Array {
            element: Box::new(Type::I64),
            size: None,
        },
    });

    // len(collection) -> i64 - accepts any collection type (str, array, map).
    // Codegen passes the opaque handle to kryos_builtin_len which reads
    // the length field at offset 0 (shared across all collection types).
    checker.env.define_function(FunctionSig {
        name: "len".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("collection".to_string(), Type::Error)],
        ret: Type::I64,
    });

    // to_string(value) -> str - accepts any type.
    checker.env.define_function(FunctionSig {
        name: "to_string".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("value".to_string(), Type::Error)],
        ret: Type::Str,
    });

    // chan() -> i64  (create a new channel)
    checker.env.define_function(FunctionSig {
        name: "chan".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![],
        ret: Type::I64,
    });

    // send(ch, value) -> void
    checker.env.define_function(FunctionSig {
        name: "send".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("ch".to_string(), Type::I64),
            ("value".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // recv(ch) -> i64
    checker.env.define_function(FunctionSig {
        name: "recv".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("ch".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // chan_try_recv(ch) -> i64 status (1 data / 0 empty / -1 closed) - non-blocking.
    checker.env.define_function(FunctionSig {
        name: "chan_try_recv".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("ch".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // chan_last_recv() -> i64 value of the most recent successful try_recv on this thread.
    checker.env.define_function(FunctionSig {
        name: "chan_last_recv".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![],
        ret: Type::I64,
    });

    // file_read(path: str) -> str - read entire file to string
    checker.env.define_function(FunctionSig {
        name: "file_read".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("path".to_string(), Type::Str)],
        ret: Type::Str,
    });

    // file_write(path: str, content: str) -> i64 - write string to file (0=ok, -1=err)
    checker.env.define_function(FunctionSig {
        name: "file_write".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("path".to_string(), Type::Str),
            ("content".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });

    // file_exists(path: str) -> i64 - 1 if exists, 0 if not
    checker.env.define_function(FunctionSig {
        name: "file_exists".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("path".to_string(), Type::Str)],
        ret: Type::I64,
    });

    // append_file(path: str, content: str) -> i64 - append string to file (0=ok, -1=err)
    checker.env.define_function(FunctionSig {
        name: "append_file".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("path".to_string(), Type::Str),
            ("content".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });

    // file_size(path: str) -> i64 - byte size of file, -1 on error
    checker.env.define_function(FunctionSig {
        name: "file_size".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("path".to_string(), Type::Str)],
        ret: Type::I64,
    });

    // Legacy file IO aliases: read_file/write_file mirror file_read/file_write.
    // These exist because the LLVM codegen and several v1.x examples used the
    // *_file form; the canonical name is the file_* form.
    checker.env.define_function(FunctionSig {
        name: "read_file".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("path".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "write_file".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("path".to_string(), Type::Str),
            ("content".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });

    // create_dir(path: str) -> i64 - create directory recursively
    checker.env.define_function(FunctionSig {
        name: "create_dir".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("path".to_string(), Type::Str)],
        ret: Type::I64,
    });

    // read_line() -> str - read one line from stdin
    checker.env.define_function(FunctionSig {
        name: "read_line".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![],
        ret: Type::Str,
    });

    // map_has(m, key) -> bool. The key is lenient (Type::Error) like
    // `contains` and `map_delete`, so it works on BOTH str- and int-keyed
    // maps (the hardcoded `key: str` rejected `map_has(int_map, 1)` with
    // E0100 even though the MIR dispatches to kryos_map_has / _str by the
    // map's key type -- map_has was the one remaining holdout of the three
    // key-membership builtins).
    checker.env.define_function(FunctionSig {
        name: "map_has".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("m".to_string(), Type::Error),
            ("key".to_string(), Type::Error),
        ],
        ret: Type::Bool,
    });

    // map_delete(m, key) -> map. The key is lenient (Type::Error) like
    // `contains`, so it works on BOTH str- and int-keyed maps (a hardcoded
    // `key: str` rejected `map_delete(int_map, 1)` with E0100, even though
    // the MIR dispatches to kryos_map_delete / _str by the map's key type).
    // Returns the map handle for `m = map_delete(m, k)`.
    checker.env.define_function(FunctionSig {
        name: "map_delete".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("m".to_string(), Type::Error),
            ("key".to_string(), Type::Error),
        ],
        ret: Type::Error,
    });

    // map_keys(m) -> [str]
    checker.env.define_function(FunctionSig {
        name: "map_keys".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("m".to_string(), Type::Error)],
        ret: Type::Error,
    });

    // env_get(key: str) -> str - get environment variable
    checker.env.define_function(FunctionSig {
        name: "env_get".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("key".to_string(), Type::Str)],
        ret: Type::Str,
    });

    // time_now() -> i64 - Unix timestamp in seconds
    checker.env.define_function(FunctionSig {
        name: "time_now".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![],
        ret: Type::I64,
    });

    // sleep(ms: i64) -> void - block the current task for `ms` milliseconds
    checker.env.define_function(FunctionSig {
        name: "sleep".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("ms".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // ---------------------------------------------------------------------
    // Cooperative async executor (real interleaving via await / yield).
    //   coop_spawn(task) -> i64   register a cooperative task; returns id
    //   coop_yield()      -> void hand control to the scheduler (await point)
    //   coop_run()        -> void drive all tasks to completion
    //   coop_reset()      -> void clear the executor (queue + order log)
    //   coop_record(s)    -> void append a tag to the order log
    //   coop_order()      -> str  the recorded interleaving order
    // ---------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "coop_spawn".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        // The argument is a task expression handled specially at lowering; the
        // checker accepts any single argument type (Type::Error = wildcard).
        params: vec![("task".to_string(), Type::Error)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "coop_yield".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "coop_run".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "coop_reset".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "coop_record".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("tag".to_string(), Type::Str)],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "coop_order".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![],
        ret: Type::Str,
    });

    // close_chan(ch: chan) -> void - close a channel (further sends panic, recvs drain then 0)
    checker.env.define_function(FunctionSig {
        name: "close_chan".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("ch".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // chan_is_closed(ch: chan) -> i64 - 1 if the channel was closed, else 0.
    // Lets a producer/worker stop when its driving channel is closed.
    checker.env.define_function(FunctionSig {
        name: "chan_is_closed".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("ch".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // ---------------------------------------------------------------------
    // Low-level FFI helpers - used by stdlib `extern { ... }` blocks that
    // need raw byte access. All take/return i64 (handles or pointers cast
    // to i64) to avoid distinguishing between `ptr` and `i64` in the user
    // surface.
    // ---------------------------------------------------------------------

    // str_to_ptr(s: str) -> i64 - raw data pointer (as i64) of a string.
    checker.env.define_function(FunctionSig {
        name: "str_to_ptr".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::I64,
    });

    // arr_to_ptr(arr) -> i64 - raw handle pointer (as i64) of an array. The
    // array handle IS its header pointer, so this is the sanctioned FFI shim
    // that replaced the (now-rejected) `arr as i64` cast. Param is opaque
    // (any collection handle), same convention as `len`.
    checker.env.define_function(FunctionSig {
        name: "arr_to_ptr".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("arr".to_string(), Type::Error)],
        ret: Type::I64,
    });

    // buf_to_str(ptr: i64, len: i64) -> str - copy `len` bytes at `ptr` to a new string.
    checker.env.define_function(FunctionSig {
        name: "buf_to_str".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("ptr".to_string(), Type::I64),
            ("len".to_string(), Type::I64),
        ],
        ret: Type::Str,
    });

    // alloc(size: i64) -> i64 - allocate `size` zero-initialized bytes. Returns 0 on failure.
    checker.env.define_function(FunctionSig {
        name: "alloc".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("size".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // free_bytes(ptr: i64, size: i64) -> void - release memory from `alloc`.
    checker.env.define_function(FunctionSig {
        name: "free_bytes".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("ptr".to_string(), Type::I64),
            ("size".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // ptr_byte_at(ptr: i64, i: i64) -> i64 - read byte at offset.
    checker.env.define_function(FunctionSig {
        name: "ptr_byte_at".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("ptr".to_string(), Type::I64),
            ("i".to_string(), Type::I64),
        ],
        ret: Type::I64,
    });

    // ptr_set_byte(ptr: i64, i: i64, b: i64) -> void - write byte at offset.
    checker.env.define_function(FunctionSig {
        name: "ptr_set_byte".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("ptr".to_string(), Type::I64),
            ("i".to_string(), Type::I64),
            ("b".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // handle_to_str(handle: i64) -> str - reinterpret KryosString handle as typed str.
    checker.env.define_function(FunctionSig {
        name: "handle_to_str".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("handle".to_string(), Type::I64)],
        ret: Type::Str,
    });

    // ptr_read_i64(ptr: i64, i: i64) -> i64 - read 8-byte slot at i*8.
    checker.env.define_function(FunctionSig {
        name: "ptr_read_i64".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("ptr".to_string(), Type::I64),
            ("i".to_string(), Type::I64),
        ],
        ret: Type::I64,
    });

    // ptr_write_i64(ptr: i64, i: i64, v: i64) -> void.
    checker.env.define_function(FunctionSig {
        name: "ptr_write_i64".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("ptr".to_string(), Type::I64),
            ("i".to_string(), Type::I64),
            ("v".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // assert(condition: bool, msg: str) -> void - abort if condition is false
    checker.env.define_function(FunctionSig {
        name: "assert".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("condition".to_string(), Type::Bool),
            ("msg".to_string(), Type::Str),
        ],
        ret: Type::Void,
    });

    // assert_eq(left, right) -> void - abort if left != right, printing both
    // values. Codegen converts each argument to its stringified form via the
    // type-aware `to_string` lowering, so the runtime always sees two strings.
    // Both args use Type::Error to accept any type (matched at codegen).
    checker.env.define_function(FunctionSig {
        name: "assert_eq".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("left".to_string(), Type::Error),
            ("right".to_string(), Type::Error),
        ],
        ret: Type::Void,
    });

    // panic(msg: str) -> void - abort the process with the given message.
    // Return type is void but the call never actually returns; control
    // flow analysis treats it like a regular call for now.
    checker.env.define_function(FunctionSig {
        name: "panic".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("msg".to_string(), Type::Str)],
        ret: Type::Void,
    });

    // parse_int(s: str) -> i64 - parse string to integer (0 on failure)
    checker.env.define_function(FunctionSig {
        name: "parse_int".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::I64,
    });

    // parse_float(s: str) -> f64 - parse string to float (0.0 on failure)
    checker.env.define_function(FunctionSig {
        name: "parse_float".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::F64,
    });

    // Overflow-aware integer arithmetic. All currently operate on i64.
    // wrapping_*  : explicit 2's-complement wrap (same as default behavior)
    // checked_*   : panic on overflow with a clear message
    // saturating_*: clamp to INT64_MIN / INT64_MAX on overflow
    for op in [
        "wrapping_add",
        "wrapping_sub",
        "wrapping_mul",
        "checked_add",
        "checked_sub",
        "checked_mul",
        "saturating_add",
        "saturating_sub",
        "saturating_mul",
    ] {
        checker.env.define_function(FunctionSig {
            name: op.to_string(),
            generic_params: vec![],
            generic_var_ids: vec![],
            generic_cap_var_ids: vec![],
            own_cap_var: checker.builtin_cap_var,
            params: vec![
                ("a".to_string(), Type::I64),
                ("b".to_string(), Type::I64),
            ],
            ret: Type::I64,
        });
    }

    // type_of(value: any) -> str - returns type name (always "i64" at runtime)
    checker.env.define_function(FunctionSig {
        name: "type_of".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("value".to_string(), Type::Error)],
        ret: Type::Str,
    });

    // char_code(c: str) -> i64 - Unicode code point of first character
    checker.env.define_function(FunctionSig {
        name: "char_code".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("c".to_string(), Type::Str)],
        ret: Type::I64,
    });

    // char_from(n: i64) -> str - single-character string from code point
    checker.env.define_function(FunctionSig {
        name: "char_from".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("n".to_string(), Type::I64)],
        ret: Type::Str,
    });

    // substr(s: str, start: i64, end: i64) -> str - substring [start..end)
    checker.env.define_function(FunctionSig {
        name: "substr".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("s".to_string(), Type::Str),
            ("start".to_string(), Type::I64),
            ("end".to_string(), Type::I64),
        ],
        ret: Type::Str,
    });

    // index_of(s: str, sub: str) -> i64 - first byte offset of `sub` in `s`, or
    // -1. Fully implemented in MIR + runtime (kryos_builtin_index_of) but was
    // never registered here, so `index_of(..)` failed with E0102 despite being
    // a working builtin.
    checker.env.define_function(FunctionSig {
        name: "index_of".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("s".to_string(), Type::Str),
            ("sub".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });

    // push<T>(arr: [T], val: T) -> [T] - generic so the element type flows:
    // `let mut a = []; a = push(a, X)` infers `a: [X]`.
    let push_t = {
        let v = checker.engine.fresh_var();
        if let Type::Var(id) = v { id } else { unreachable!() }
    };
    checker.env.define_function(FunctionSig {
        name: "push".to_string(),
        generic_params: vec!["T".to_string()],
        generic_var_ids: vec![push_t],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            (
                "arr".to_string(),
                Type::Array { element: Box::new(Type::Var(push_t)), size: None },
            ),
            ("val".to_string(), Type::Var(push_t)),
        ],
        ret: Type::Array { element: Box::new(Type::Var(push_t)), size: None },
    });

    // pop<T>(arr: [T]) -> T - remove and return last element
    let pop_t = {
        let v = checker.engine.fresh_var();
        if let Type::Var(id) = v { id } else { unreachable!() }
    };
    checker.env.define_function(FunctionSig {
        name: "pop".to_string(),
        generic_params: vec!["T".to_string()],
        generic_var_ids: vec![pop_t],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![(
            "arr".to_string(),
            Type::Array { element: Box::new(Type::Var(pop_t)), size: None },
        )],
        ret: Type::Var(pop_t),
    });

    // sort(arr: any) -> any - in-place ascending sort
    checker.env.define_function(FunctionSig {
        name: "sort".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("arr".to_string(), Type::Error)],
        ret: Type::Error,
    });

    // reverse(arr: any) -> any - in-place reverse
    checker.env.define_function(FunctionSig {
        name: "reverse".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("arr".to_string(), Type::Error)],
        ret: Type::Error,
    });

    // int(x: any) -> i64 - convert to integer
    checker.env.define_function(FunctionSig {
        name: "int".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("x".to_string(), Type::Error)],
        ret: Type::I64,
    });

    // float(x: any) -> f64 - convert to float
    checker.env.define_function(FunctionSig {
        name: "float".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("x".to_string(), Type::Error)],
        ret: Type::F64,
    });

    // sqrt(x: f64) -> f64 - square root
    checker.env.define_function(FunctionSig {
        name: "sqrt".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // floor(x: f64) -> f64 - floor
    checker.env.define_function(FunctionSig {
        name: "floor".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // ceil(x: f64) -> f64 - ceiling
    checker.env.define_function(FunctionSig {
        name: "ceil".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // round(x: f64) -> f64 - round to nearest integer (ties to even)
    checker.env.define_function(FunctionSig {
        name: "round".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // abs(x: T) -> T - absolute value (polymorphic: i64 or f64)
    {
        let abs_tv = checker.engine.fresh_var();
        let abs_var_id = if let Type::Var(id) = &abs_tv {
            vec![*id]
        } else {
            vec![]
        };
        checker.env.define_function(FunctionSig {
            name: "abs".to_string(),
            generic_params: vec!["T".to_string()],
            generic_var_ids: abs_var_id,
            generic_cap_var_ids: vec![],
            own_cap_var: checker.builtin_cap_var,
            params: vec![("x".to_string(), abs_tv.clone())],
            ret: abs_tv,
        });
    }

    // min(a: T, b: T) -> T - minimum of two values (polymorphic: i64 or f64)
    {
        let min_tv = checker.engine.fresh_var();
        let min_var_id = if let Type::Var(id) = &min_tv {
            vec![*id]
        } else {
            vec![]
        };
        checker.env.define_function(FunctionSig {
            name: "min".to_string(),
            generic_params: vec!["T".to_string()],
            generic_var_ids: min_var_id,
            generic_cap_var_ids: vec![],
            own_cap_var: checker.builtin_cap_var,
            params: vec![
                ("a".to_string(), min_tv.clone()),
                ("b".to_string(), min_tv.clone()),
            ],
            ret: min_tv,
        });
    }

    // max(a: T, b: T) -> T - maximum of two values (polymorphic: i64 or f64)
    {
        let max_tv = checker.engine.fresh_var();
        let max_var_id = if let Type::Var(id) = &max_tv {
            vec![*id]
        } else {
            vec![]
        };
        checker.env.define_function(FunctionSig {
            name: "max".to_string(),
            generic_params: vec!["T".to_string()],
            generic_var_ids: max_var_id,
            generic_cap_var_ids: vec![],
            own_cap_var: checker.builtin_cap_var,
            params: vec![
                ("a".to_string(), max_tv.clone()),
                ("b".to_string(), max_tv.clone()),
            ],
            ret: max_tv,
        });
    }

    // sin(x: f64) -> f64
    checker.env.define_function(FunctionSig {
        name: "sin".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // cos(x: f64) -> f64
    checker.env.define_function(FunctionSig {
        name: "cos".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // tan(x: f64) -> f64
    checker.env.define_function(FunctionSig {
        name: "tan".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // log(x: f64) -> f64 - natural logarithm
    checker.env.define_function(FunctionSig {
        name: "log".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // log2(x: f64) -> f64
    checker.env.define_function(FunctionSig {
        name: "log2".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // log10(x: f64) -> f64
    checker.env.define_function(FunctionSig {
        name: "log10".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // pow(x: f64, y: f64) -> f64 - x to the power of y
    checker.env.define_function(FunctionSig {
        name: "pow".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("x".to_string(), Type::F64), ("y".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // abs_f(x: f64) -> f64 - absolute value for floats
    checker.env.define_function(FunctionSig {
        name: "abs_f".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("x".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // min_f(a: f64, b: f64) -> f64 - minimum of two floats
    checker.env.define_function(FunctionSig {
        name: "min_f".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("a".to_string(), Type::F64), ("b".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // max_f(a: f64, b: f64) -> f64 - maximum of two floats
    checker.env.define_function(FunctionSig {
        name: "max_f".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("a".to_string(), Type::F64), ("b".to_string(), Type::F64)],
        ret: Type::F64,
    });

    // keys(m: any) -> [str] - get map keys
    checker.env.define_function(FunctionSig {
        name: "keys".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("m".to_string(), Type::Error)],
        ret: Type::Array {
            element: Box::new(Type::Str),
            size: None,
        },
    });

    // sleep_ms(ms: i64) -> void - sleep for milliseconds
    checker.env.define_function(FunctionSig {
        name: "sleep_ms".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("ms".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // ── Byte buffer builtins ──────────────────────────────────────

    // buf_new(capacity: i64) -> i64
    checker.env.define_function(FunctionSig {
        name: "buf_new".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("capacity".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // buf_write_byte(handle: i64, byte: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_write_byte".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("handle".to_string(), Type::I64),
            ("byte".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // buf_write_i16_le(handle: i64, val: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_write_i16_le".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("handle".to_string(), Type::I64),
            ("val".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // buf_write_i32_le(handle: i64, val: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_write_i32_le".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("handle".to_string(), Type::I64),
            ("val".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // buf_write_i64_le(handle: i64, val: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_write_i64_le".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("handle".to_string(), Type::I64),
            ("val".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // buf_write_bytes(dst: i64, src: i64, len: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_write_bytes".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("dst".to_string(), Type::I64),
            ("src".to_string(), Type::I64),
            ("len".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // buf_write_str(handle: i64, s: str) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_write_str".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("handle".to_string(), Type::I64),
            ("s".to_string(), Type::Str),
        ],
        ret: Type::Void,
    });

    // buf_write_zeros(handle: i64, count: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_write_zeros".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("handle".to_string(), Type::I64),
            ("count".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // buf_len(handle: i64) -> i64
    checker.env.define_function(FunctionSig {
        name: "buf_len".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("handle".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // buf_str(handle: i64) -> str - materialize a growable KryosBuf's contents.
    checker.env.define_function(FunctionSig {
        name: "buf_str".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("handle".to_string(), Type::I64)],
        ret: Type::Str,
    });

    // buf_get_byte(handle: i64, offset: i64) -> i64
    checker.env.define_function(FunctionSig {
        name: "buf_get_byte".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("handle".to_string(), Type::I64),
            ("offset".to_string(), Type::I64),
        ],
        ret: Type::I64,
    });

    // buf_set_byte(handle: i64, offset: i64, byte: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_set_byte".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("handle".to_string(), Type::I64),
            ("offset".to_string(), Type::I64),
            ("byte".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // buf_patch_i32_le(handle: i64, offset: i64, val: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_patch_i32_le".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("handle".to_string(), Type::I64),
            ("offset".to_string(), Type::I64),
            ("val".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // buf_patch_i64_le(handle: i64, offset: i64, val: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_patch_i64_le".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("handle".to_string(), Type::I64),
            ("offset".to_string(), Type::I64),
            ("val".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // buf_write_to_file(handle: i64, path: str) -> i64
    checker.env.define_function(FunctionSig {
        name: "buf_write_to_file".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("handle".to_string(), Type::I64),
            ("path".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });

    // buf_free(handle: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "buf_free".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("handle".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // args() -> [str] - command-line arguments
    checker.env.define_function(FunctionSig {
        name: "args".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![],
        ret: Type::Array {
            element: Box::new(Type::Str),
            size: None,
        },
    });

    // ── Self-host intrinsics ─────────────────────────────────────
    //
    // The self-hosted runtime (`compiler/self-host/runtime.kry`) wraps a
    // handful of low-level intrinsics. For stage-0 builds (Rust compiler →
    // stage-1) the Rust runtime provides these as C-callable symbols. For
    // stage-1+ self-compiled builds, the codegen emits inline syscall /
    // mov / load / store instructions. Either way the type checker has
    // to know about them so `runtime.kry` passes `check`.

    // Linux x86_64 syscalls. The codegen recognises these names and emits
    // the `syscall` instruction with the appropriate argument registers.
    for (name, arity) in [
        ("syscall1", 1usize),
        ("syscall2", 2),
        ("syscall3", 3),
        ("syscall6", 6),
    ] {
        let mut params = vec![("nr".to_string(), Type::I64)];
        for i in 1..=arity {
            params.push((format!("a{i}"), Type::I64));
        }
        checker.env.define_function(FunctionSig {
            name: name.to_string(),
            generic_params: vec![],
            generic_var_ids: vec![],
            generic_cap_var_ids: vec![],
            own_cap_var: checker.builtin_cap_var,
            params,
            ret: Type::I64,
        });
    }

    // Raw memory ops. Codegen emits inline load/store instructions.
    checker.env.define_function(FunctionSig {
        name: "mem_read_byte".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("ptr".to_string(), Type::I64)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "mem_write_byte".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("ptr".to_string(), Type::I64),
            ("val".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "mem_read_i64".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("ptr".to_string(), Type::I64)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "mem_write_i64".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("ptr".to_string(), Type::I64),
            ("val".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "mem_copy".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("src".to_string(), Type::I64),
            ("dst".to_string(), Type::I64),
            ("len".to_string(), Type::I64),
        ],
        ret: Type::Void,
    });

    // String view intrinsics. A Kryos `str` is an opaque handle; these
    // expose its byte length and data pointer for low-level work. The
    // self-hosted runtime sometimes carries the handle around as raw
    // `i64`, so the intrinsic accepts either a `str` or an `i64` handle.
    checker.env.define_function(FunctionSig {
        name: "str_byte_len".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("s".to_string(), Type::Error)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "str_data_ptr".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("s".to_string(), Type::Error)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "str_from_bytes".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("ptr".to_string(), Type::I64),
            ("len".to_string(), Type::I64),
        ],
        ret: Type::Str,
    });

    // Numeric/process intrinsics.
    checker.env.define_function(FunctionSig {
        name: "__int_to_float".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("x".to_string(), Type::I64)],
        ret: Type::F64,
    });
    checker.env.define_function(FunctionSig {
        name: "__get_process_args".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![],
        ret: Type::Array {
            element: Box::new(Type::Str),
            size: None,
        },
    });

    // `__builtin_*` thin wrappers - the runtime's user-facing `len`,
    // `push`, `pop`, `range`, `map_*` functions delegate to these.
    // They mirror the existing high-level builtins but live under a
    // namespaced name so codegen can dispatch them differently from a
    // user-defined `len`/`push`.
    checker.env.define_function(FunctionSig {
        name: "__builtin_len".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("collection".to_string(), Type::Error)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "__builtin_push".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("arr".to_string(), Type::Error),
            ("val".to_string(), Type::Error),
        ],
        ret: Type::Error,
    });
    checker.env.define_function(FunctionSig {
        name: "__builtin_pop".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("arr".to_string(), Type::Error)],
        ret: Type::Error,
    });
    checker.env.define_function(FunctionSig {
        name: "__builtin_range".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("start".to_string(), Type::I64),
            ("end".to_string(), Type::I64),
        ],
        ret: Type::Array {
            element: Box::new(Type::I64),
            size: None,
        },
    });
    for name in [
        "__builtin_map_has",
        "__builtin_map_has_str",
        "__builtin_map_delete",
        "__builtin_map_delete_str",
    ] {
        checker.env.define_function(FunctionSig {
            name: name.to_string(),
            generic_params: vec![],
            generic_var_ids: vec![],
            generic_cap_var_ids: vec![],
            own_cap_var: checker.builtin_cap_var,
            params: vec![
                ("m".to_string(), Type::Error),
                ("key".to_string(), Type::Error),
            ],
            ret: Type::I64,
        });
    }
    for name in ["__builtin_map_keys", "__builtin_map_keys_str"] {
        checker.env.define_function(FunctionSig {
            name: name.to_string(),
            generic_params: vec![],
            generic_var_ids: vec![],
            generic_cap_var_ids: vec![],
            own_cap_var: checker.builtin_cap_var,
            params: vec![("m".to_string(), Type::Error)],
            ret: Type::Error,
        });
    }

    // ── String utility builtins ────────────────────────────────────

    // contains(haystack, needle) -> bool
    checker.env.define_function(FunctionSig {
        name: "contains".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        // BOTH params are lenient (Type::Error): contains works on a str
        // (substring search, needle: str), a str-keyed map (key membership,
        // needle: str), AND an int-keyed map (key membership, needle: i64).
        // The needle was previously hard-typed to str, which rejected the
        // documented `contains(m, k)` on map<i64, _> with E0100. MIR lowering
        // dispatches to kryos_map_has[_str] vs kryos_builtin_contains by the
        // first arg's type (and the map's KEY type), so leniency here is safe.
        params: vec![
            ("haystack".to_string(), Type::Error),
            ("needle".to_string(), Type::Error),
        ],
        ret: Type::Bool,
    });

    // starts_with(s: str, prefix: str) -> bool
    checker.env.define_function(FunctionSig {
        name: "starts_with".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("s".to_string(), Type::Str),
            ("prefix".to_string(), Type::Str),
        ],
        ret: Type::Bool,
    });

    // ends_with(s: str, suffix: str) -> bool
    checker.env.define_function(FunctionSig {
        name: "ends_with".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("s".to_string(), Type::Str),
            ("suffix".to_string(), Type::Str),
        ],
        ret: Type::Bool,
    });

    // trim(s: str) -> str
    checker.env.define_function(FunctionSig {
        name: "trim".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::Str,
    });

    // to_upper(s: str) -> str
    checker.env.define_function(FunctionSig {
        name: "to_upper".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::Str,
    });

    // to_lower(s: str) -> str
    checker.env.define_function(FunctionSig {
        name: "to_lower".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::Str,
    });

    // replace(s: str, from: str, to: str) -> str
    checker.env.define_function(FunctionSig {
        name: "replace".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("s".to_string(), Type::Str),
            ("from".to_string(), Type::Str),
            ("to".to_string(), Type::Str),
        ],
        ret: Type::Str,
    });

    // split(s: str, delimiter: str) -> [str]
    checker.env.define_function(FunctionSig {
        name: "split".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("s".to_string(), Type::Str),
            ("delimiter".to_string(), Type::Str),
        ],
        ret: Type::Array {
            element: Box::new(Type::Str),
            size: None,
        },
    });

    // join(arr: [str], separator: str) -> str
    checker.env.define_function(FunctionSig {
        name: "join".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            (
                "arr".to_string(),
                Type::Array {
                    element: Box::new(Type::Str),
                    size: None,
                },
            ),
            ("separator".to_string(), Type::Str),
        ],
        ret: Type::Str,
    });

    // tcp_connect(host: str, port: i64) -> i64
    checker.env.define_function(FunctionSig {
        name: "tcp_connect".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("host".to_string(), Type::Str),
            ("port".to_string(), Type::I64),
        ],
        ret: Type::I64,
    });

    // tcp_listen(host: str, port: i64) -> i64
    checker.env.define_function(FunctionSig {
        name: "tcp_listen".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("host".to_string(), Type::Str),
            ("port".to_string(), Type::I64),
        ],
        ret: Type::I64,
    });

    // tcp_accept(fd: i64) -> i64
    checker.env.define_function(FunctionSig {
        name: "tcp_accept".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("fd".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // tcp_send(fd: i64, data: str) -> i64
    checker.env.define_function(FunctionSig {
        name: "tcp_send".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("fd".to_string(), Type::I64),
            ("data".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });

    // tcp_recv(fd: i64, max_bytes: i64) -> str
    checker.env.define_function(FunctionSig {
        name: "tcp_recv".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("fd".to_string(), Type::I64),
            ("max_bytes".to_string(), Type::I64),
        ],
        ret: Type::Str,
    });

    // tcp_close(fd: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "tcp_close".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("fd".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // -----------------------------------------------------------------------
    // Async / non-blocking primitives (Gap 3 minimum viable async)
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "tcp_set_nonblocking".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("fd".to_string(), Type::I64), ("nonblocking".to_string(), Type::Bool)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "tcp_try_accept".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("listener_fd".to_string(), Type::I64)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "tcp_try_recv".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("fd".to_string(), Type::I64), ("max_bytes".to_string(), Type::I64)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "sleep_ms".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("ms".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // -----------------------------------------------------------------------
    // TLS server builtins (Gap A)
    // -----------------------------------------------------------------------
    // tls_server_config(cert_path: str, key_path: str) -> i64
    checker.env.define_function(FunctionSig {
        name: "tls_server_config".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("cert_path".to_string(), Type::Str),
            ("key_path".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });

    // tls_accept(client_tcp_fd: i64, config_handle: i64) -> i64
    checker.env.define_function(FunctionSig {
        name: "tls_accept".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("client_tcp_fd".to_string(), Type::I64),
            ("config_handle".to_string(), Type::I64),
        ],
        ret: Type::I64,
    });

    // tls_send(fd: i64, data: str) -> i64
    checker.env.define_function(FunctionSig {
        name: "tls_send".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("fd".to_string(), Type::I64),
            ("data".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });

    // tls_recv(fd: i64, max_bytes: i64) -> str
    checker.env.define_function(FunctionSig {
        name: "tls_recv".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("fd".to_string(), Type::I64),
            ("max_bytes".to_string(), Type::I64),
        ],
        ret: Type::Str,
    });

    // tls_close(fd: i64) -> void
    checker.env.define_function(FunctionSig {
        name: "tls_close".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("fd".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // -----------------------------------------------------------------------
    // PostgreSQL builtins (Gap B)
    // -----------------------------------------------------------------------
    // pg_connect(conn_str: str) -> i64
    checker.env.define_function(FunctionSig {
        name: "pg_connect".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("conn_str".to_string(), Type::Str)],
        ret: Type::I64,
    });

    // pg_exec(handle: i64, sql: str) -> i64
    checker.env.define_function(FunctionSig {
        name: "pg_exec".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("handle".to_string(), Type::I64),
            ("sql".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });

    // pg_query(handle: i64, sql: str) -> str
    checker.env.define_function(FunctionSig {
        name: "pg_query".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("handle".to_string(), Type::I64),
            ("sql".to_string(), Type::Str),
        ],
        ret: Type::Str,
    });

    // pg_close(handle: i64) -> i64
    checker.env.define_function(FunctionSig {
        name: "pg_close".to_string(),
        generic_params: vec![],
        generic_var_ids: vec![],
        generic_cap_var_ids: vec![],
        own_cap_var: checker.builtin_cap_var,
        params: vec![("handle".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // -----------------------------------------------------------------------
    // Unix domain sockets (v2.0)
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "uds_connect".to_string(), generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("path".to_string(), Type::Str)], ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "uds_bind".to_string(), generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("path".to_string(), Type::Str)], ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "uds_accept".to_string(), generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("fd".to_string(), Type::I64)], ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "uds_send".to_string(), generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("fd".to_string(), Type::I64), ("data".to_string(), Type::Str)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "uds_recv".to_string(), generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("fd".to_string(), Type::I64), ("max_bytes".to_string(), Type::I64)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "uds_close".to_string(), generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("fd".to_string(), Type::I64)], ret: Type::I64,
    });

    // -----------------------------------------------------------------------
    // WebSocket (RFC 6455) helpers (v2.0)
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "ws_accept_key".to_string(), generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("key".to_string(), Type::Str)], ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ws_encode_text".to_string(), generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("payload".to_string(), Type::Str)], ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ws_encode_binary".to_string(), generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("payload".to_string(), Type::Str)], ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ws_encode_close".to_string(), generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("code".to_string(), Type::I64)], ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ws_encode_ping".to_string(), generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("payload".to_string(), Type::Str)], ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ws_encode_pong".to_string(), generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("payload".to_string(), Type::Str)], ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ws_unmask".to_string(), generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("buf".to_string(), Type::Str),
            ("payload_off".to_string(), Type::I64),
            ("payload_len".to_string(), Type::I64),
            ("mask_off".to_string(), Type::I64),
        ],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ws_read_frame".to_string(), generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("fd".to_string(), Type::I64)], ret: Type::Str,
    });

    // -----------------------------------------------------------------------
    // JSON builtins
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "json_parse".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_stringify".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("node".to_string(), Type::I64)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "json_object".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("keys".to_string(), Type::Array { element: Box::new(Type::Str), size: None }),
            ("vals".to_string(), Type::Array { element: Box::new(Type::I64), size: None }),
        ],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_array".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("items".to_string(), Type::Array { element: Box::new(Type::I64), size: None })],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_string".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_number".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("v".to_string(), Type::F64)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_bool".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("v".to_string(), Type::Bool)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_null".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_get".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("obj".to_string(), Type::I64), ("key".to_string(), Type::Str)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_get_index".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("arr".to_string(), Type::I64), ("idx".to_string(), Type::I64)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_to_str".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("node".to_string(), Type::I64)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "json_to_int".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("node".to_string(), Type::I64)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_to_float".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("node".to_string(), Type::I64)],
        ret: Type::F64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_is_null".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("node".to_string(), Type::I64)],
        ret: Type::Bool,
    });
    checker.env.define_function(FunctionSig {
        name: "json_length".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("node".to_string(), Type::I64)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "json_type".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("node".to_string(), Type::I64)],
        ret: Type::Str,
    });

    // -----------------------------------------------------------------------
    // Crypto / hashing
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "sha256".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "sha512".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "hmac_sha256".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("key".to_string(), Type::Str), ("data".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ed25519_generate".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ed25519_public".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("pkcs8_hex".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ed25519_sign".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("pkcs8_hex".to_string(), Type::Str), ("msg".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "ed25519_verify".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("pub_hex".to_string(), Type::Str),
            ("msg".to_string(), Type::Str),
            ("sig_hex".to_string(), Type::Str),
        ],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "hex_to_base64url".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("hex".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "base64url_to_hex".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("b64url".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "pbkdf2_sha256".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("password".to_string(), Type::Str),
            ("salt_hex".to_string(), Type::Str),
            ("iters".to_string(), Type::I64),
        ],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "random_bytes".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("n".to_string(), Type::I64)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "sha1_hex".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "sha1_base64".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "base64_encode".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "base64_decode".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("s".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "chr".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("n".to_string(), Type::I64)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "byte_at".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("s".to_string(), Type::Str), ("idx".to_string(), Type::I64)],
        ret: Type::I64,
    });

    // -----------------------------------------------------------------------
    // Regex
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "regex_new".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("pattern".to_string(), Type::Str)],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "regex_match".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("re".to_string(), Type::I64), ("text".to_string(), Type::Str)],
        ret: Type::Bool,
    });
    checker.env.define_function(FunctionSig {
        name: "regex_find".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("re".to_string(), Type::I64), ("text".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "regex_find_pos".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("re".to_string(), Type::I64),
            ("text".to_string(), Type::Str),
            ("from".to_string(), Type::I64),
        ],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "regex_find_end".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("re".to_string(), Type::I64),
            ("text".to_string(), Type::Str),
            ("from".to_string(), Type::I64),
        ],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "regex_replace_all".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("re".to_string(), Type::I64),
            ("text".to_string(), Type::Str),
            ("replacement".to_string(), Type::Str),
        ],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "regex_drop".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("re".to_string(), Type::I64)],
        ret: Type::Void,
    });

    // -----------------------------------------------------------------------
    // HTTP / HTTPS
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "http_request".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("method".to_string(), Type::Str),
            ("url".to_string(), Type::Str),
            ("headers".to_string(), Type::Str),
            ("body".to_string(), Type::Str),
            ("timeout_ms".to_string(), Type::I64),
        ],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "https_get".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("url".to_string(), Type::Str)],
        ret: Type::Str,
    });

    // -----------------------------------------------------------------------
    // HTTP/2 client (Gap C) - reqwest-backed, ALPN h2 with HTTP/1.1 fallback
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "http2_get".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("url".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "http2_post".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("url".to_string(), Type::Str),
            ("body".to_string(), Type::Str),
        ],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "http2_request".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("method".to_string(), Type::Str),
            ("url".to_string(), Type::Str),
            ("headers".to_string(), Type::Str),
            ("body".to_string(), Type::Str),
        ],
        ret: Type::Str,
    });

    // -----------------------------------------------------------------------
    // WASM v0.4 web builtins (DOM / canvas / fetch / alert)
    // These compile to host imports under --backend wasm and to runtime
    // no-ops / native equivalents under cranelift/llvm (best-effort).
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "dom_set_text".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("id".to_string(), Type::Str), ("text".to_string(), Type::Str)],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "dom_get_value".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("id".to_string(), Type::Str)],
        ret: Type::Str,
    });
    checker.env.define_function(FunctionSig {
        name: "alert".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("msg".to_string(), Type::Str)],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "canvas_fill_rect".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![
            ("canvas_id".to_string(), Type::Str),
            ("x".to_string(), Type::I64),
            ("y".to_string(), Type::I64),
            ("w".to_string(), Type::I64),
            ("h".to_string(), Type::I64),
            ("color".to_string(), Type::Str),
        ],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "canvas_clear".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("canvas_id".to_string(), Type::Str)],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "fetch_text".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("url".to_string(), Type::Str)],
        ret: Type::Str,
    });

    // -----------------------------------------------------------------------
    // Time / Mutex
    // -----------------------------------------------------------------------
    checker.env.define_function(FunctionSig {
        name: "time_now_secs".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "time_now_millis".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![],
        ret: Type::I64,
    });
    // `time_millis` is the documented short alias for `time_now_millis`.
    checker.env.define_function(FunctionSig {
        name: "time_millis".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "mutex_new".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![],
        ret: Type::I64,
    });
    checker.env.define_function(FunctionSig {
        name: "mutex_lock".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("m".to_string(), Type::I64)],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "mutex_unlock".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("m".to_string(), Type::I64)],
        ret: Type::Void,
    });
    checker.env.define_function(FunctionSig {
        name: "mutex_drop".to_string(),
        generic_params: vec![], generic_var_ids: vec![], generic_cap_var_ids: vec![], own_cap_var: checker.builtin_cap_var,
        params: vec![("m".to_string(), Type::I64)],
        ret: Type::Void,
    });

    checker.check_module(module);

    // `KRYOS_DUMP_FN_EFFECTS=1` debug dump (Stage 1 -- capability-typed fn
    // values, docs/capability-effects-spec.md): print every logged
    // declared-function/lambda/actor-handler's FINAL inferred capability
    // row, fully resolved now that the whole module has finished checking
    // (every forward reference has had a chance to bind). Printed to
    // stderr so it never pollutes stdout for tools parsing normal
    // diagnostics/output.
    if checker.dump_fn_effects {
        eprint!("{}", checker.dump_fn_effects_report());
    }

    // Resolve recorded empty-array let bindings through the (now fully unified)
    // engine and convert to TypeExpr for the MIR. Only keep those whose element
    // resolved to a concrete (non-var) type.
    let let_types: std::collections::HashMap<Span, TypeExpr> = checker
        .resolved_let_types
        .iter()
        .filter_map(|(span, ty)| {
            let resolved = checker.engine.resolve(ty);
            concrete_type_to_type_expr(&resolved).map(|te| (*span, te))
        })
        .collect();
    (
        std::mem::take(&mut checker.diagnostics),
        std::mem::take(&mut checker.resolved_lambda_params),
        let_types,
    )
}

// ── Missing return analysis ─────────────────────────────────────────

/// A pattern is "binding" if it introduces any variable binding - an identifier
/// pattern, or a compound pattern (tuple/struct/enum/or) containing one. Used to
/// enforce that or-pattern alternatives are non-binding (CLAUDE.md gotcha #14):
/// binding alternatives silently produced type confusion or uninitialized reads.
/// Collect every referenced name (identifiers, call callees, method names)
/// in an expression into `out`. Used to build the interprocedural const
/// dependency graph so a cycle routed THROUGH a function call
/// (`let A = helper()` where `helper` reads `A`) is visible to cycle
/// detection, not just direct in-initializer references. Over-collection is
/// harmless (a name that is neither a const nor a fn is ignored downstream).
fn collect_names_expr(expr: &Expr, out: &mut std::collections::HashSet<String>) {
    use kryos_ast::Expr as E;
    match expr {
        E::Identifier { name, .. } => {
            out.insert(name.clone());
        }
        E::FieldAccess { object, .. } => collect_names_expr(object, out),
        E::IndexAccess { object, index, .. } => {
            collect_names_expr(object, out);
            collect_names_expr(index, out);
        }
        E::BinaryOp { left, right, .. } => {
            collect_names_expr(left, out);
            collect_names_expr(right, out);
        }
        E::UnaryOp { operand, .. } => collect_names_expr(operand, out),
        E::FnCall { callee, args, .. } => {
            collect_names_expr(callee, out);
            for a in args {
                collect_names_expr(a, out);
            }
        }
        E::MethodCall {
            object,
            method,
            args,
            ..
        } => {
            collect_names_expr(object, out);
            out.insert(method.clone());
            for a in args {
                collect_names_expr(a, out);
            }
        }
        E::StaticMethodCall {
            type_name,
            method,
            args,
            ..
        } => {
            out.insert(type_name.clone());
            out.insert(method.clone());
            for a in args {
                collect_names_expr(a, out);
            }
        }
        E::ArrayLiteral { elements, .. } | E::TupleLiteral { elements, .. } => {
            for e in elements {
                collect_names_expr(e, out);
            }
        }
        E::MapLiteral { entries, .. } => {
            for (k, v) in entries {
                collect_names_expr(k, out);
                collect_names_expr(v, out);
            }
        }
        E::StructLiteral { fields, .. } => {
            for (_, e) in fields {
                collect_names_expr(e, out);
            }
        }
        E::InterpolatedString { parts, .. } => {
            for p in parts {
                if let kryos_ast::StringPart::Expr(e) = p {
                    collect_names_expr(e, out);
                }
            }
        }
        E::Lambda { body, .. } => collect_names_expr(body, out),
        E::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            collect_names_expr(condition, out);
            collect_names_block(then_branch, out);
            if let Some(eb) = else_branch {
                collect_names_block(eb, out);
            }
        }
        E::MatchExpr { subject, arms, .. } => {
            collect_names_expr(subject, out);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    collect_names_expr(g, out);
                }
                collect_names_expr(&arm.body, out);
            }
        }
        E::RangeExpr { start, end, .. } => {
            if let Some(s) = start {
                collect_names_expr(s, out);
            }
            if let Some(e) = end {
                collect_names_expr(e, out);
            }
        }
        E::PipeExpr { left, right, .. } => {
            collect_names_expr(left, out);
            collect_names_expr(right, out);
        }
        E::Borrow { inner, .. }
        | E::Deref { inner, .. }
        | E::SharedExpr { inner, .. }
        | E::MoveExpr { inner, .. }
        | E::WeakExpr { inner, .. } => collect_names_expr(inner, out),
        E::Cast { expr, .. } => collect_names_expr(expr, out),
        E::Await { value, .. } => collect_names_expr(value, out),
        E::Block { block, .. } => collect_names_block(block, out),
        E::ComptimeBlock { body, .. }
        | E::QuantumBlock { body, .. }
        | E::UnsafeBlock { body, .. } => collect_names_block(body, out),
        _ => {}
    }
}

fn collect_names_stmt(stmt: &kryos_ast::Stmt, out: &mut std::collections::HashSet<String>) {
    use kryos_ast::Stmt as S;
    match stmt {
        S::Let { value, .. } => {
            if let Some(v) = value {
                collect_names_expr(v, out);
            }
        }
        S::Assign { target, value, .. } => {
            collect_names_expr(target, out);
            collect_names_expr(value, out);
        }
        S::Return { value, .. } => {
            if let Some(v) = value {
                collect_names_expr(v, out);
            }
        }
        S::If {
            condition,
            then_block,
            elif_clauses,
            else_block,
            ..
        } => {
            collect_names_expr(condition, out);
            collect_names_block(then_block, out);
            for (c, b) in elif_clauses {
                collect_names_expr(c, out);
                collect_names_block(b, out);
            }
            if let Some(eb) = else_block {
                collect_names_block(eb, out);
            }
        }
        S::For { iterable, body, .. } => {
            collect_names_expr(iterable, out);
            collect_names_block(body, out);
        }
        S::While {
            condition, body, ..
        } => {
            collect_names_expr(condition, out);
            collect_names_block(body, out);
        }
        S::Expr { expr, .. } | S::Spawn { expr, .. } | S::Throw { expr, .. } => {
            collect_names_expr(expr, out)
        }
        S::TryCatch {
            try_block,
            catch_block,
            ..
        } => {
            collect_names_block(try_block, out);
            collect_names_block(catch_block, out);
        }
        _ => {}
    }
}

fn collect_names_block(block: &Block, out: &mut std::collections::HashSet<String>) {
    for s in &block.stmts {
        collect_names_stmt(s, out);
    }
}

fn pattern_is_binding(p: &kryos_ast::Pattern) -> bool {
    use kryos_ast::Pattern;
    match p {
        Pattern::Ident { .. } => true,
        Pattern::Wildcard { .. } | Pattern::Literal { .. } => false,
        Pattern::Tuple { elements, .. } => elements.iter().any(pattern_is_binding),
        Pattern::Struct { fields, .. } => fields.iter().any(|(_, sp)| pattern_is_binding(sp)),
        Pattern::Enum { fields, .. } => fields.iter().any(pattern_is_binding),
        Pattern::Or { patterns, .. } => patterns.iter().any(pattern_is_binding),
    }
}

/// Returns `true` if the block is guaranteed to return a value on all
/// control flow paths. This is a conservative check - it may produce
/// false negatives (miss some paths) but never false positives.
fn block_returns(block: &Block) -> bool {
    if let Some(last) = block.stmts.last() {
        stmt_returns(last)
    } else {
        false
    }
}

/// Returns true if a match-arm body expression is guaranteed to
/// diverge (return/throw on every path) and therefore should not
/// contribute its type to the match expression's unified type.
///
/// NOTE: this is intentionally NOT implemented via `block_returns` /
/// `stmt_returns` above. Those answer "does every path produce a
/// value" -- which is also true for a plain value-producing tail
/// expression like `{ 42 }` (a `Stmt::Expr` tail counts as "returning"
/// a value there). Divergence is a different question: "does this
/// path never complete normally at all" (it only throws/returns).
/// Reusing `block_returns` here previously misclassified an all-block
/// match arm's `{ 42 }` body as divergent, so a match where every arm
/// was a block silently dropped every arm's type and fell back to
/// `void` (adding a bare sibling arm masked the bug because ANY
/// non-diverging arm supplies the result type).
fn arm_body_diverges(body: &Expr) -> bool {
    expr_diverges(body)
}

/// Returns true if a block's control flow always diverges (throws or
/// returns) on every path -- i.e. it can never complete normally with
/// a value. Used to exclude a divergent if/match branch from value-type
/// unification: a branch that only ever throws/returns contributes no
/// type, and the expression's type comes from the other branch(es).
fn block_diverges(block: &Block) -> bool {
    block.stmts.last().is_some_and(stmt_diverges)
}

fn stmt_diverges(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return { .. } => true,
        Stmt::Throw { .. } => true,
        Stmt::If {
            then_block,
            elif_clauses,
            else_block,
            ..
        } => {
            // An if/elif/else statement diverges only if EVERY branch
            // diverges, including a mandatory else (no else => it can
            // fall through without diverging).
            block_diverges(then_block)
                && elif_clauses.iter().all(|(_, b)| block_diverges(b))
                && else_block.as_ref().is_some_and(block_diverges)
        }
        Stmt::TryCatch {
            try_block,
            catch_block,
            ..
        } => block_diverges(try_block) && block_diverges(catch_block),
        Stmt::DenyBlock { body, .. } => block_diverges(body),
        // A tail expression statement diverges only if the expression
        // itself always diverges (e.g. a nested if/match whose every
        // branch throws/returns) -- a plain value expression like `42`
        // does NOT diverge, it's the block's value.
        Stmt::Expr { expr, .. } => expr_diverges(expr),
        _ => false,
    }
}

/// Returns true if an expression (a match-arm body, or a block's tail
/// expression) always diverges rather than producing a value.
fn expr_diverges(expr: &Expr) -> bool {
    match expr {
        Expr::Block { block, .. } => block_diverges(block),
        Expr::IfExpr {
            then_branch,
            else_branch,
            ..
        } => {
            block_diverges(then_branch) && else_branch.as_ref().is_some_and(block_diverges)
        }
        Expr::MatchExpr { arms, .. } => {
            !arms.is_empty() && arms.iter().all(|a| expr_diverges(&a.body))
        }
        _ => false,
    }
}

fn stmt_returns(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return { .. } => true,
        Stmt::Throw { .. } => true,
        Stmt::If {
            then_block,
            elif_clauses,
            else_block,
            ..
        } => {
            // All branches must return, including else.
            let then_ok = block_returns(then_block);
            let elifs_ok = elif_clauses.iter().all(|(_, b)| block_returns(b));
            let else_ok = else_block.as_ref().is_some_and(block_returns);
            then_ok && elifs_ok && else_ok
        }
        Stmt::TryCatch {
            try_block,
            catch_block,
            ..
        } => block_returns(try_block) && block_returns(catch_block),
        // An expression statement at the end of a block can serve as an
        // implicit return value (expression-bodied functions).
        Stmt::Expr { .. } => true,
        // A deny block returns iff its body returns on all paths.
        Stmt::DenyBlock { body, .. } => block_returns(body),
        _ => false,
    }
}
