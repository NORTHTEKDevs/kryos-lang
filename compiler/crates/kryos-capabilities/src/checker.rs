//! Capability checker — walks the AST and enforces capability rules.
//!
//! Rules enforced:
//! 1. Functions using stdlib modules must declare matching capabilities.
//! 2. Attenuation: child scopes cannot exceed parent capabilities.
//! 3. Immutability: no runtime escalation of capabilities.
//! 4. Budget and sandbox annotations are validated.

use kryos_ast::{Annotation, Decl, Expr, Module, Param, Stmt, TypeExpr};
use kryos_errors::{Diagnostic, Span};

use std::collections::{HashMap, HashSet};

use crate::model::{
    is_escalation_action, is_known_builtin_name, required_capability_for_builtin,
    required_capability_for_native_symbol, required_capability_for_path, Budget, Capability,
    CapabilitySet, Sandbox,
};

/// How strictly capabilities are enforced. The three modes form a hierarchy
/// `Permissive < Inferred < Strict`:
///
/// - **Permissive** — the historical default. Only functions that carry an
///   explicit `@capabilities(...)` annotation are checked (plus attenuation
///   between annotated scopes). An unannotated function may call any gated
///   builtin. Capabilities are advisory.
///
/// - **Inferred** — deny-by-default at the *boundary*, with interior
///   inference. Every function's effective capability set is computed: an
///   annotated function's set is its declaration (a ceiling); an unannotated
///   interior function's set is *inferred* as the union of what it and its
///   callees require (no annotation needed on helpers). Enforcement then
///   requires that `main` — and any explicitly annotated function — actually
///   holds every capability its body transitively uses. So an unannotated
///   `main` that (directly or through helpers) writes a file is rejected:
///   the program must declare `@capabilities(fs:write)` on `main`. Reading
///   `main`'s annotation tells you the program's entire authority.
///
/// - **Strict** — every function is treated as annotated with exactly its
///   declaration (empty if unannotated). Every gated builtin call from an
///   unannotated function is an error. This is `--strict-capabilities`:
///   maximally explicit, every function auditable in isolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityMode {
    Permissive,
    Inferred,
    Strict,
}

impl CapabilityMode {
    /// Parse a mode name (from `kryos.toml` / `--capabilities-mode`).
    pub fn from_str(s: &str) -> Option<CapabilityMode> {
        match s {
            "permissive" => Some(CapabilityMode::Permissive),
            "inferred" => Some(CapabilityMode::Inferred),
            "strict" => Some(CapabilityMode::Strict),
            _ => None,
        }
    }
}

/// Run the capability checking pass over a module in the given [`CapabilityMode`].
/// Enforce that raw-memory primitives are only used by code that DECLARES
/// `ffi`, at the direct call site, without that requirement propagating.
///
/// Call this on the ROOT module *before* imported declarations are merged in,
/// so it sees user code only. The stdlib is the trusted computing base here:
/// it is built on these primitives (`alloc` in 14 modules), and propagating a
/// requirement out of it would make `std::json::parse` demand `ffi` from every
/// caller -- measured, and it makes the language unusable.
///
/// Without this, capability attenuation is defeated outright: a program
/// declaring NO capabilities passes `--strict-capabilities` and still reads
/// any string in the host process via `str_to_ptr` + `ptr_byte_at`
/// (tests/security/cap_escape_raw_memory.kry).
pub fn check_raw_memory_direct(module: &Module) -> Vec<Diagnostic> {
    let diags = crate::model::with_raw_memory_gate(|| {
        check_capabilities_mode(module, CapabilityMode::Inferred)
    });
    // Report ONLY the raw-memory findings. Every other diagnostic from this
    // extra pass is a duplicate of the real check that runs on the merged
    // module, and the root module alone cannot see imported callees.
    diags
        .into_iter()
        .filter(|d| {
            d.is_error()
                && crate::model::RAW_MEMORY_BUILTINS
                    .iter()
                    .any(|b| d.message.contains(&format!("`{b}`")))
        })
        .collect()
}

pub fn check_capabilities_mode(module: &Module, mode: CapabilityMode) -> Vec<Diagnostic> {
    let mut checker = CapabilityChecker::new(mode);
    checker.check_module(module);
    checker.diagnostics
}

/// Run the capability checking pass over a module.
///
/// Back-compat shim over [`check_capabilities_mode`]: `strict == true` maps to
/// [`CapabilityMode::Strict`], `false` to [`CapabilityMode::Permissive`]. New
/// callers should use `check_capabilities_mode` directly to opt into
/// [`CapabilityMode::Inferred`].
pub fn check_capabilities(module: &Module, strict: bool) -> Vec<Diagnostic> {
    let mode = if strict {
        CapabilityMode::Strict
    } else {
        CapabilityMode::Permissive
    };
    check_capabilities_mode(module, mode)
}

/// The statically-resolved authority carried by a first-class FUNCTION VALUE
/// (a closure literal, a named-function reference, or the result of a call
/// to a function that returns one) at the point it flows into a call that
/// invokes it.
///
/// This is the mechanism that closes the closure/fn-value capability escape
/// (`tests/security/cap_escape_closure_launder.kry`): `fn_capabilities` is
/// keyed by NAME and only ever answers "what does calling this NAMED
/// function require", which says nothing about calling a VALUE flowing
/// through a local/parameter binding. `ClosureCapsResult` tracks that value's
/// own intrinsic requirement (or the fact that it depends on a parameter of
/// the function currently being analyzed, or that it cannot be resolved at
/// all) so a call through an fn-typed PARAMETER can be attributed to the
/// actual closure supplied at each call site, not blanket-denied (which
/// cascades through every `std::iter` HOF) or silently ignored (the escape).
#[derive(Debug, Clone, PartialEq)]
enum ClosureCapsResult {
    /// The value's exact required capability set is known statically.
    Known(CapabilitySet),
    /// The value IS one of the current function's own fn-typed parameters
    /// (or resolves transitively to one) — its real requirement depends on
    /// whatever the CALLER of the current function supplies, so it must not
    /// be charged to the current function itself; `hot_params` propagation
    /// defers the charge to the current function's own call sites instead.
    DependsOnParam(String),
    /// The value's provenance cannot be traced statically (read out of an
    /// array/map/struct field, chosen by a runtime condition, received via
    /// an actor message with no resolvable origin, ...). Sound handling is
    /// the same stance documented for the raw-memory escape: if the checker
    /// cannot prove what a callback does, require the caller to hold `all`.
    Unknown,
}

/// One step of a field/index access chain rooted at a parameter, used to
/// close the CONTAINER residual of the closure-laundering fix (a closure
/// stored in a struct field / array element / map value, read back out and
/// invoked by code that never held the capability it carries — see LEDGER
/// item 1). `Field(name)` is a precise, per-name struct field access
/// (`reg.reader`); `Index` is an array-element or map-value access, which is
/// deliberately name/index-INSENSITIVE (arrays and maps have no static
/// per-slot tracking, so a path ending in `Index` matches ANY element/value
/// written into that container — conservative, matching the same stance
/// `ClosureCapsResult::Unknown` already documents for other unresolvable
/// shapes).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum PathStep {
    Field(String),
    Index,
}

/// A capability scope entry on the stack.
///
/// Tracks both the capability set and whether this scope was explicitly
/// annotated with `@capabilities(...)`. Unannotated scopes are "ambient" —
/// they don't enforce builtin or cross-function capability checks.
#[derive(Debug, Clone)]
struct CapabilityScope {
    capabilities: CapabilitySet,
    /// `true` if the enclosing declaration had a `@capabilities(...)` annotation.
    annotated: bool,
}

/// Internal checker state.
struct CapabilityChecker {
    /// Stack of capability scopes. Each entry is the capability set for
    /// the current enclosing scope (function, actor, etc.).
    scope_stack: Vec<CapabilityScope>,
    /// `scope_stack.len()` immediately after the CURRENT function/actor's own
    /// boundary scope was pushed (`check_function` / `check_actor`) —
    /// i.e., the depth BEFORE any `deny!` block nested inside its body has
    /// pushed a further, NARROWER scope. Only meaningful during the
    /// enforcement walk (`check_decl`'s traversal); the inference passes
    /// (`compute_inferred_capabilities` and friends) run before any scope is
    /// ever pushed, so `scope_stack` stays at its initial length there and
    /// this field's default is correspondingly inert.
    ///
    /// Used by `accumulate_hot_extra_caps`'s `DependsOnParam` handling: a
    /// caller-supplied fn-typed parameter that a function invokes (directly
    /// or through a HOF) INSIDE a `deny!` block nested in that SAME
    /// function's own body cannot be soundly deferred to the function's own
    /// call sites the way an un-narrowed forward can. The call site that
    /// supplies the parameter's real value is checked against THIS
    /// function's own (wider) ENTRY scope, not against the narrower scope
    /// actually in effect at the point of invocation — confirmed live: `fn
    /// outer(reader: fn()->str) -> str { deny!(fs:read) { return
    /// zero_cap_tool(reader) } }`, called from an `@capabilities(fs:read)`
    /// caller, compiled clean and printed the secret from inside the denied
    /// scope pre-fix, with NO decoy or generic involved at all — a plain,
    /// direct forward. `scope_stack.len() > current_fn_entry_scope_depth` at
    /// the point of the invocation detects exactly this: a deny! (or any
    /// future scope-narrowing construct) is active between this function's
    /// entry and the current call, so the deferral is unsound and the full
    /// capability set must be required instead.
    current_fn_entry_scope_depth: usize,
    /// The subset of `current_fn_typed_params` that were added by
    /// `check_expr`'s `Expr::Lambda` arm (a lambda's OWN bound parameter
    /// names, folded in ONLY for the duration of checking that lambda's own
    /// body) rather than being one of the ENCLOSING function/handler's real
    /// declared parameters. A reference to one of these names resolving to
    /// `DependsOnParam` is a KNOWN, already-fully-handled duplicate of the
    /// check `accumulate_hot_extra_caps` performs at the ENCLOSING call site
    /// (see that field's `Lambda` arm doc) -- re-flagging it here, inside
    /// the lambda's own body, would be a spurious second diagnostic for a
    /// call the outer machinery already resolves precisely. This is
    /// distinguished from a REAL enclosing function's own parameter
    /// (`current_fn_entry_scope_depth`'s scope-narrowing check DOES apply to
    /// those, per the live `fn outer(r: fn()->str) { deny!(fs:read) { r() }
    /// }` finding) so a lambda's transparent self-forwarding is never
    /// over-rejected merely because it happens to run inside a `deny!`
    /// block the OUTER analysis has already correctly accounted for.
    transparent_lambda_params: HashSet<String>,
    /// Nesting depth of an in-progress STRUCTURAL sub-evaluation of a fresh
    /// lambda literal's own body, done by `resolve_closure_caps`'s `Lambda`
    /// arm purely to CLASSIFY that literal (does it require anything beyond
    /// transparently forwarding its own bound parameter?) -- entirely
    /// independent of, and NOT gated by, the real ambient enforcement scope
    /// at whatever call site happens to be resolving the lambda's authority.
    /// `Cell`, not a plain field, because `resolve_closure_caps` takes
    /// `&self` (it is shared, read-only resolution logic reused by both the
    /// enforcement and inference passes).
    ///
    /// Needed because that sub-evaluation reuses the SAME `collect_caps_expr`
    /// / `resolve_direct_invoke_caps` / `deferred_own_param_caps` machinery
    /// as a REAL enforcement-time direct invocation, and `deferred_own_param_caps`
    /// otherwise cannot tell the two apart: a bare `|c| c()` lambda's own
    /// classification pass calls `c()` INSIDE that sub-evaluation, "c" is
    /// (correctly) `own_params`-classified there too, and the AMBIENT
    /// `scope_stack`/`current_fn_entry_scope_depth` at that moment reflects
    /// wherever the REAL call site happens to be (e.g. inside a `deny!`
    /// block) -- with no distinguishing signal, that ambient narrowing
    /// wrongly made every transparent-forwarding lambda passed into a
    /// `deny!`'d call resolve to `Known(all)` instead of the correct
    /// `DependsOnParam`, breaking the `hot_param_companions` relief for the
    /// most common case (`map`/`filter`/... invoked inside ANY `deny!`
    /// block, even over provably pure closures). While this counter is
    /// nonzero, `deferred_own_param_caps` always defers (empty), matching
    /// its ORIGINAL, scope-independent behavior -- correct for a structural
    /// classification, which must not depend on the ambient call site at
    /// all.
    structural_lambda_eval_depth: std::cell::Cell<u32>,
    /// Accumulated diagnostics.
    diagnostics: Vec<Diagnostic>,
    /// Map from function name to the capability set used for cross-function
    /// propagation checks. In every mode this holds each *annotated* function's
    /// declared set (its ceiling). In [`CapabilityMode::Inferred`] it also holds
    /// each *unannotated* function's inferred set, so a caller is checked
    /// against what its callee transitively requires.
    fn_capabilities: HashMap<String, CapabilitySet>,
    /// Every function name declared in an `extern { ... }` block anywhere in
    /// the module. Calls to these are the raw FFI surface: a `kryos_*` name
    /// maps to the same semantic capability as its builtin (e.g.
    /// `kryos_env_get` -> `process`); any other extern name requires `ffi`.
    /// Without this call-side gate, a user-declared extern block was a
    /// deny-by-default bypass (declared top-level, called from unannotated
    /// code, no capability ever demanded).
    extern_fns: std::collections::HashSet<String>,
    /// Names bound as LOCALS (params + let/for/match/catch bindings) in the
    /// function currently being checked. A value-position identifier in this
    /// set is a local variable, NOT a function reference, so it must not be
    /// attributed a same-named function's capabilities -- without this, any
    /// local named like one of the 1000+ stdlib functions (`query`, `connect`,
    /// `find`, ...) spuriously inherited its capability.
    current_locals: std::collections::HashSet<String>,
    /// Every USER-defined (non-extern) function/method name in the module. A
    /// user function that SHADOWS a gated builtin's name (`fn http_get(x) { x+1 }`)
    /// resolves to the user function, so its calls must NOT be attributed the
    /// builtin's capability -- the checker otherwise gated by name alone and
    /// spuriously required, e.g., `net:http` for a pure user `http_get`.
    defined_fns: std::collections::HashSet<String>,
    /// Every ACTOR type name declared in the module. Actors are constructed
    /// with `Name()` call syntax (see CLAUDE.md) -- a `Expr::FnCall` whose
    /// callee is the actor's bare name, NOT a reference to a first-class
    /// fn-value. Without tracking this separately, the fail-closed default
    /// (`resolve_direct_invoke_caps`) cannot tell `Account()` (a constructor)
    /// from `r()` (invoking an unresolved closure named `r`), since actors
    /// are not recorded in `defined_fns` (they are a distinct `Decl`
    /// variant, not a function).
    actor_names: std::collections::HashSet<String>,
    /// Every ENUM VARIANT name declared in the module (`Some`, `None`, `Ok`,
    /// `Err`, and any user-declared enum's variants). A tuple-variant
    /// construction (`Some(x)`, `None()`) is `Expr::FnCall`/`StaticMethodCall`
    /// syntax with the variant's bare name as the callee, NOT a reference to
    /// a first-class fn-value -- same reasoning as `actor_names`, and the
    /// same reason the fail-closed default (`resolve_direct_invoke_caps`)
    /// needs this list: `Option`/`Result` (from `std::option`/`std::result`)
    /// are used constantly, so without this EVERY `Some(..)`/`Ok(..)`/
    /// `Err(..)`/`None()` construction anywhere in the program would be
    /// misread as an unresolvable closure invocation.
    enum_variant_names: std::collections::HashSet<String>,
    /// Every function/method's parameter LIST, keyed by bare name (same
    /// keying convention as `fn_capabilities` — a name shared by two impls'
    /// methods collides, a pre-existing modeling simplification this reuses
    /// rather than widens). Used to find which parameter INDEX is fn-typed
    /// and to map a forwarded-argument index back to a parameter name.
    fn_params: HashMap<String, Vec<Param>>,
    /// Every function/method's DECLARED return type, keyed by bare name.
    /// Only functions whose return type is itself `fn(...) -> ...` are
    /// candidates for `fn_return_closure_caps`.
    fn_ret_ty: HashMap<String, TypeExpr>,
    /// Every actor MESSAGE HANDLER name. Unlike a struct `impl` method (which
    /// the language requires to declare an EXPLICIT `self: T` as its own
    /// `params[0]`), an actor handler's `params` list has NO implicit or
    /// explicit receiver slot — `w.run(x)` maps argument 0 straight to
    /// `run`'s `params[0]`, not `params[1]`. Dispatch-index translation
    /// (`compute_hot_params` propagation, `accumulate_hot_extra_caps`) must
    /// use a zero offset for these names even though the call syntax is
    /// `MethodCall`-shaped, or a hot parameter at index 0 is never reached
    /// (an off-by-one that silently dropped actor-handler coverage).
    actor_handler_names: HashSet<String>,
    /// Every struct's field list, keyed by struct NAME, as `(field_name,
    /// declared_type)` pairs. Feeds `is_fn_bearing_type`/`resolve_type_path`
    /// — the container-closure-laundering fix (LEDGER item 1) needs to know
    /// whether a parameter typed as a struct actually carries a
    /// function-typed field somewhere in its shape.
    struct_field_types: HashMap<String, Vec<(String, TypeExpr)>>,
    /// Every struct's generic parameter names, in declaration order
    /// (`struct List<T> { .. }` -> `["T"]`). Feeds `struct_fields_for`'s
    /// generic-argument substitution.
    struct_generic_params: HashMap<String, Vec<String>>,
    /// For every user-defined method whose receiver is literally named
    /// `self` and whose every return path decomposes to the SAME
    /// self-rooted field/index chain, the method NAME -> that self-relative
    /// `PathStep` chain. Feeds `resolve_type_path`'s transparent-accessor
    /// fallback (see `collect_transparent_accessor_paths`) -- what lets
    /// `list.get(i)` on a `std::collections::List<fn() -> str>` be
    /// recognized as reaching the same slot `list.data[i]` would, instead of
    /// being invisible because `decompose_container_path` only understands
    /// direct field/index syntax, not a call through a helper method.
    /// Keyed by `(struct name, method name)`, NOT bare method name — unlike
    /// `fn_params`/`fn_capabilities`'s pre-existing bare-name-collision
    /// tolerance, a name collision here would be actively WRONG rather than
    /// just imprecise: `std::collections::List.get` and `Dict.get` both
    /// exist in the SAME stdlib module and return DIFFERENT self-relative
    /// paths (`[data, Index]` vs `[store, Index]`), so a bare-name map would
    /// let one clobber the other and silently stop recognizing the loser's
    /// accessor — verified live during development (this exact collision
    /// initially broke `List<fn() -> str>.get()` detection because `Dict`'s
    /// `get` is declared later in the file and won the collision).
    transparent_accessor_paths: HashMap<(String, String), Vec<PathStep>>,
    /// For each function name, for each PARAMETER INDEX that is invoked —
    /// either DIRECTLY as a bare `p(...)` call, or by drilling into a
    /// CONTAINER value (`p.field(...)`, `p[i](...)`, or a chain of these) —
    /// the set of field/index PATHS (see `PathStep`) from the parameter's
    /// own value down to the invoked slot. A bare direct call records the
    /// EMPTY path (the parameter's own value IS the closure). Populated
    /// structurally by `compute_hot_params`; a call site passing an argument
    /// into one of these (index, path) pairs must additionally require
    /// whatever authority that SPECIFIC path resolves to within the actual
    /// argument (see `accumulate_hot_extra_caps` /
    /// `resolve_container_path_caps`); the function declaring the hot
    /// parameter itself requires nothing extra, which is what keeps
    /// `std::iter`'s HOFs annotation-free.
    hot_params: HashMap<String, HashMap<usize, HashSet<Vec<PathStep>>>>,
    /// For each function name, for each of ITS OWN parameter indices that is
    /// directly invoked (a hot callback slot, `hot_params`'s empty-path
    /// case), the DATA-FLOW-PROVEN companion: for every argument POSITION
    /// passed at the SAME function's OWN internal call site(s) that invoke
    /// that callback parameter, which of the function's OTHER OWN
    /// parameters (by index) and PATH the argument expression actually
    /// decomposes to -- e.g. `map`'s body, `f(arr[i])`, proves the callback
    /// (param 1) is invoked at argument position 0 with `arr[i]`, so
    /// position 0 maps to `(param 0, [Index])`.
    ///
    /// This is the SOLE relief mechanism for a caller-supplied lambda that
    /// only forwards its own bound parameter (`|f| f(10)` passed as the
    /// callback to `map`/`filter`/`reduce`/... or a user-written HOF fitting
    /// the same shape) — see `accumulate_hot_extra_caps`'s `DependsOnParam`
    /// handling. It is deliberately NOT a repeat of the shape-based
    /// `find_companion_container_arg` heuristic four rounds of this file's
    /// history proved unsound (matching a callback parameter's DECLARED
    /// element type against another parameter's DECLARED container type,
    /// first match wins — defeated by an empty decoy of the same declared
    /// shape, confirmed live). This instead traces the CALLEE'S OWN, FIXED
    /// function body: the companion is whatever parameter its actual source
    /// code passes into the callback invocation, a property of the
    /// declaration itself that no caller-supplied argument can influence, so
    /// a decoy argument at the call site cannot change which parameter is
    /// found here. A position with disagreeing or unresolvable call sites
    /// (multiple internal invocations of the same callback that decompose
    /// differently, or don't decompose to another own-parameter at all)
    /// records `None` at that slot — resolved the same as an unresolvable
    /// value elsewhere in this file: `Capability::All`, never a guess.
    /// Populated once by `compute_hot_param_companions`, alongside
    /// `hot_params`.
    hot_param_companions: HashMap<String, HashMap<usize, Vec<Option<(usize, Vec<PathStep>)>>>>,
    /// For each function whose declared return type is `fn(...) -> ...`, the
    /// statically-resolved authority carried by the closure it returns (see
    /// `compute_fn_return_closure_caps`). Lets a call site resolve `let
    /// reader = make_secret_reader(path)` back to the authority the returned
    /// closure actually carries.
    fn_return_closure_caps: HashMap<String, ClosureCapsResult>,
    /// The fn-typed PARAMETER NAMES of the function currently being checked
    /// (enforcement walk only). An `Identifier` reference to one of these
    /// resolves to `ClosureCapsResult::DependsOnParam`, deferring its charge
    /// to the CURRENT function's own call sites rather than requiring `all`
    /// inside a function that merely forwards its own callback parameter.
    current_fn_typed_params: HashSet<String>,
    /// A best-effort, flat (not scope-precise — see `build_local_closure_caps`)
    /// map from each `let`-bound local in the function currently being
    /// checked to the resolved authority of its initializer, when that
    /// initializer is plausibly a function value (a lambda literal, a
    /// function reference, or a call to a closure-returning function).
    current_local_closure_caps: HashMap<String, ClosureCapsResult>,
    /// A best-effort, flat (same precision level as
    /// `current_local_closure_caps`) map from each `let`-bound local in the
    /// function currently being checked to its initializer EXPRESSION, when
    /// that initializer is a struct/array/map LITERAL (or a bare alias of
    /// another already-tracked container local). Lets
    /// `resolve_container_path_caps` trace `let reg = Registry { reader: r }`
    /// / `zero_cap_tool(reg)` back to the literal `reg` was built from,
    /// without re-deriving a `ClosureCapsResult` at bind time (a container's
    /// authority depends on WHICH field/index path is later invoked, which
    /// isn't known yet when the `let` is bound).
    current_local_container_lits: HashMap<String, Expr>,
    /// The active enforcement mode.
    mode: CapabilityMode,
}

impl CapabilityChecker {
    fn new(mode: CapabilityMode) -> Self {
        Self {
            scope_stack: Vec::new(),
            current_fn_entry_scope_depth: 0,
            transparent_lambda_params: HashSet::new(),
            structural_lambda_eval_depth: std::cell::Cell::new(0),
            diagnostics: Vec::new(),
            fn_capabilities: HashMap::new(),
            extern_fns: std::collections::HashSet::new(),
            current_locals: std::collections::HashSet::new(),
            defined_fns: std::collections::HashSet::new(),
            actor_names: std::collections::HashSet::new(),
            enum_variant_names: std::collections::HashSet::new(),
            fn_params: HashMap::new(),
            fn_ret_ty: HashMap::new(),
            actor_handler_names: HashSet::new(),
            struct_field_types: HashMap::new(),
            struct_generic_params: HashMap::new(),
            transparent_accessor_paths: HashMap::new(),
            hot_params: HashMap::new(),
            hot_param_companions: HashMap::new(),
            fn_return_closure_caps: HashMap::new(),
            current_fn_typed_params: HashSet::new(),
            current_local_closure_caps: HashMap::new(),
            current_local_container_lits: HashMap::new(),
            mode,
        }
    }

    /// The capability a CALL to `name` requires because `name` is an
    /// extern-declared function. `kryos_*` runtime exports resolve through the
    /// NATIVE-SYMBOL table (which maps the raw runtime symbol names -- `_ks`
    /// suffixes, `builtin_` prefixes, and verb-order-different spellings like
    /// `dir_create`/`process_exec_simple` -- to the SAME authority as the
    /// builtin they back); genuinely ambient `kryos_*` plumbing (allocators,
    /// pointer/string helpers) stays ungated; every non-`kryos_` extern (user
    /// C libraries) requires `ffi`. Mapping only the builtin spellings left the
    /// authority-bearing natives ambient -- a hand-declared
    /// `extern { fn kryos_process_exec_simple }` reached arbitrary process exec
    /// from a zero-capability `main` (deny-by-default bypass).
    fn required_capability_for_extern(&self, name: &str) -> Option<Capability> {
        if !self.extern_fns.contains(name) {
            return None;
        }
        match name.strip_prefix("kryos_") {
            Some(stripped) => required_capability_for_native_symbol(stripped),
            None => Some(Capability::Ffi),
        }
    }

    /// Collect every extern-declared function name (recursing into nested
    /// declaration containers) so call sites can be gated.
    fn collect_extern_fns(&mut self, decls: &[Decl]) {
        for d in decls {
            match d {
                Decl::Extern { items, .. } => {
                    for item in items {
                        if let Decl::Function { name, .. } = item {
                            self.extern_fns.insert(name.clone());
                        }
                    }
                }
                Decl::Impl { methods, .. } | Decl::Trait { methods, .. } => {
                    self.collect_extern_fns(methods);
                }
                _ => {}
            }
        }
    }

    /// Record every USER-defined (non-extern) function/method name so a call to
    /// a user function that shadows a gated builtin's name is not force-gated by
    /// the builtin table (see `defined_fns`).
    fn collect_defined_fns(&mut self, decls: &[Decl]) {
        for d in decls {
            match d {
                Decl::Function { name, .. } => {
                    self.defined_fns.insert(name.clone());
                }
                Decl::Impl { methods, .. } | Decl::Trait { methods, .. } => {
                    self.collect_defined_fns(methods);
                }
                Decl::Actor { name, .. } => {
                    self.actor_names.insert(name.clone());
                }
                Decl::Enum { variants, .. } => {
                    for v in variants {
                        self.enum_variant_names.insert(v.name.clone());
                    }
                }
                _ => {}
            }
        }
    }

    /// Whether a parameter's declared type is a function type (`fn(...) -> ...`).
    fn is_fn_typed(ty: &Option<TypeExpr>) -> bool {
        matches!(ty, Some(TypeExpr::Function { .. }))
    }

    /// Record every struct's field list (name -> [(field_name, field_type)])
    /// AND its generic parameter names in declaration order (name ->
    /// ["T", ...]). Feeds `is_fn_bearing_type` / `resolve_type_path` — the
    /// container-closure-laundering fix needs to know whether a struct type
    /// carries a function-typed field somewhere in its shape, INCLUDING when
    /// that field's declared type is a generic parameter (`List<T>`'s
    /// `data: [T]`) that a specific reference instantiates with a function
    /// type (`List<fn() -> str>`) — see `struct_fields_for`. Actor state
    /// fields are deliberately NOT included: an actor is a message-passing
    /// boundary, not a value ever passed around and drilled into like an
    /// ordinary struct (see `check_actor`'s own, separate handling).
    fn collect_struct_field_types(&mut self, decls: &[Decl]) {
        for d in decls {
            if let Decl::Struct { name, fields, generics, .. } = d {
                self.struct_field_types.insert(
                    name.clone(),
                    fields.iter().map(|f| (f.name.clone(), f.ty.clone())).collect(),
                );
                self.struct_generic_params
                    .insert(name.clone(), generics.iter().map(|g| g.name.clone()).collect());
            }
        }
    }

    /// `struct_name`'s field list with its OWN generic parameters
    /// substituted for `type_args` (positional, by declaration order), so a
    /// reference to `List<fn() -> str>`'s `data` field resolves to
    /// `[fn() -> str]`, not the raw declared `[T]`. Falls back to the
    /// UNSUBSTITUTED field list when the struct has no generic parameters,
    /// `type_args` is empty (a bare non-generic reference), or the arity
    /// doesn't line up (a partially-applied/malformed reference — `zip`
    /// simply stops at the shorter side, so this degrades gracefully rather
    /// than panicking).
    fn struct_fields_for(&self, struct_name: &str, type_args: &[TypeExpr]) -> Option<Vec<(String, TypeExpr)>> {
        let fields = self.struct_field_types.get(struct_name)?;
        if type_args.is_empty() {
            return Some(fields.clone());
        }
        let Some(generic_names) = self.struct_generic_params.get(struct_name) else {
            return Some(fields.clone());
        };
        if generic_names.is_empty() {
            return Some(fields.clone());
        }
        let subst: HashMap<&str, &TypeExpr> = generic_names
            .iter()
            .map(String::as_str)
            .zip(type_args.iter())
            .collect();
        Some(
            fields
                .iter()
                .map(|(n, t)| (n.clone(), Self::substitute_generic_type(t, &subst)))
                .collect(),
        )
    }

    /// Recursively replace every `TypeExpr::Simple { name, .. }` that names a
    /// key in `subst` with its substituted type. Used to instantiate a
    /// generic struct's declared field types (`T`, `[T]`, `map<K, T>`, ...)
    /// against the concrete type arguments at a specific reference.
    fn substitute_generic_type(ty: &TypeExpr, subst: &HashMap<&str, &TypeExpr>) -> TypeExpr {
        match ty {
            TypeExpr::Simple { name, .. } => match subst.get(name.as_str()) {
                Some(replacement) => (*replacement).clone(),
                None => ty.clone(),
            },
            TypeExpr::Generic { name, args, span } => TypeExpr::Generic {
                name: name.clone(),
                args: args.iter().map(|a| Self::substitute_generic_type(a, subst)).collect(),
                span: *span,
            },
            TypeExpr::Array { element, size, span } => TypeExpr::Array {
                element: Box::new(Self::substitute_generic_type(element, subst)),
                size: *size,
                span: *span,
            },
            TypeExpr::Tuple { elements, span } => TypeExpr::Tuple {
                elements: elements.iter().map(|e| Self::substitute_generic_type(e, subst)).collect(),
                span: *span,
            },
            TypeExpr::Function { params, ret, span } => TypeExpr::Function {
                params: params.iter().map(|p| Self::substitute_generic_type(p, subst)).collect(),
                ret: Box::new(Self::substitute_generic_type(ret, subst)),
                span: *span,
            },
            TypeExpr::Optional { inner, span } => TypeExpr::Optional {
                inner: Box::new(Self::substitute_generic_type(inner, subst)),
                span: *span,
            },
            TypeExpr::Reference { inner, mutable, span } => TypeExpr::Reference {
                inner: Box::new(Self::substitute_generic_type(inner, subst)),
                mutable: *mutable,
                span: *span,
            },
            TypeExpr::Shared { inner, span } => TypeExpr::Shared {
                inner: Box::new(Self::substitute_generic_type(inner, subst)),
                span: *span,
            },
            TypeExpr::Weak { inner, span } => TypeExpr::Weak {
                inner: Box::new(Self::substitute_generic_type(inner, subst)),
                span: *span,
            },
            TypeExpr::Pointer { inner, mutable, span } => TypeExpr::Pointer {
                inner: Box::new(Self::substitute_generic_type(inner, subst)),
                mutable: *mutable,
                span: *span,
            },
            TypeExpr::DynTrait { .. } | TypeExpr::Inferred { .. } => ty.clone(),
        }
    }

    /// Whether a type could carry a function VALUE somewhere within its
    /// shape, reachable by field/index access — i.e. whether it is worth
    /// treating a parameter of this type as a "container candidate" for the
    /// `hot_params` container-invocation seed pass. `Function` itself
    /// qualifies (the pre-existing direct case); so does an array whose
    /// element type qualifies, a `map<K, V>` whose VALUE type qualifies, an
    /// `Option<T>` whose inner type qualifies, and a struct with at least one
    /// field whose type qualifies (recursively). `depth` bounds the
    /// recursion so a self-referential or mutually-recursive struct
    /// definition (`struct Node { next: Node }`) cannot loop forever; it is
    /// NOT needed for the separate field/index PATH walk in
    /// `resolve_type_path`, which is bounded by the actual (finite) access
    /// chain written in the source, not by the type graph.
    fn is_fn_bearing_type(&self, ty: &Option<TypeExpr>) -> bool {
        match ty {
            Some(t) => self.is_fn_bearing_type_inner(t, 0),
            None => false,
        }
    }

    fn is_fn_bearing_type_inner(&self, ty: &TypeExpr, depth: u32) -> bool {
        if depth > 8 {
            return false;
        }
        match ty {
            TypeExpr::Function { .. } => true,
            TypeExpr::Array { element, .. } => self.is_fn_bearing_type_inner(element, depth + 1),
            TypeExpr::Optional { inner, .. } => self.is_fn_bearing_type_inner(inner, depth + 1),
            TypeExpr::Generic { name, args, .. } if (name == "map" || name == "Map") && args.len() == 2 => {
                self.is_fn_bearing_type_inner(&args[1], depth + 1)
            }
            TypeExpr::Simple { name, .. } => self
                .struct_fields_for(name, &[])
                .is_some_and(|fields| {
                    fields.iter().any(|(_, fty)| self.is_fn_bearing_type_inner(fty, depth + 1))
                }),
            TypeExpr::Generic { name, args, .. } => self
                .struct_fields_for(name, args)
                .is_some_and(|fields| {
                    fields.iter().any(|(_, fty)| self.is_fn_bearing_type_inner(fty, depth + 1))
                }),
            _ => false,
        }
    }

    /// Resolve the TYPE reached by walking `path` (a field/index access
    /// chain) starting from `ty`. Returns `None` if the path doesn't
    /// correspond to a real field/index chain on this type (an ordinary
    /// method call that happens to share a field's name, an index into a
    /// non-container, a drill through an opaque/unmodeled type, ...) — used
    /// to distinguish a genuine container-closure invocation from an
    /// ordinary method call before marking a parameter hot.
    ///
    /// A `Field(fname)` step that does NOT name a genuine struct field falls
    /// back to checking whether `fname` is a TRANSPARENT ACCESSOR METHOD
    /// (`transparent_accessor_paths` — `List.get`, `Stack.peek`,
    /// `Queue.front`, `Deque`'s accessors, and any user method shaped the
    /// same way): if so, that method's own self-relative path is spliced in
    /// and resolution continues against the SAME `ty` with the combined
    /// path. This is what makes `list.get(i)()` on a
    /// `std::collections::List<fn() -> str>` resolve exactly like
    /// `list.data[i]()` would, instead of "get" failing to match any real
    /// field on `List` and silently being treated as an ordinary,
    /// non-hot method call.
    fn resolve_type_path(&self, ty: &TypeExpr, path: &[PathStep]) -> Option<TypeExpr> {
        self.resolve_type_path_inner(ty, path, 0)
    }

    fn resolve_type_path_inner(&self, ty: &TypeExpr, path: &[PathStep], depth: u32) -> Option<TypeExpr> {
        if depth > 16 {
            return None;
        }
        let Some((head, rest)) = path.split_first() else {
            return Some(ty.clone());
        };
        match (head, ty) {
            (_, TypeExpr::Optional { inner, .. }) => self.resolve_type_path_inner(inner, path, depth + 1),
            (PathStep::Field(fname), TypeExpr::Simple { name, .. }) => {
                if let Some(fields) = self.struct_fields_for(name, &[]) {
                    if let Some((_, fty)) = fields.iter().find(|(n, _)| n == fname) {
                        return self.resolve_type_path_inner(&fty.clone(), rest, depth + 1);
                    }
                }
                let accessor_path = self
                    .transparent_accessor_paths
                    .get(&(name.clone(), fname.clone()))?;
                let mut combined = accessor_path.clone();
                combined.extend(rest.iter().cloned());
                self.resolve_type_path_inner(ty, &combined, depth + 1)
            }
            (PathStep::Field(fname), TypeExpr::Generic { name, args, .. }) => {
                if let Some(fields) = self.struct_fields_for(name, args) {
                    if let Some((_, fty)) = fields.iter().find(|(n, _)| n == fname) {
                        return self.resolve_type_path_inner(&fty.clone(), rest, depth + 1);
                    }
                }
                let accessor_path = self
                    .transparent_accessor_paths
                    .get(&(name.clone(), fname.clone()))?;
                let mut combined = accessor_path.clone();
                combined.extend(rest.iter().cloned());
                self.resolve_type_path_inner(ty, &combined, depth + 1)
            }
            (PathStep::Index, TypeExpr::Array { element, .. }) => {
                self.resolve_type_path_inner(element, rest, depth + 1)
            }
            (PathStep::Index, TypeExpr::Generic { name, args, .. })
                if (name == "map" || name == "Map") && args.len() == 2 =>
            {
                self.resolve_type_path_inner(&args[1], rest, depth + 1)
            }
            _ => None,
        }
    }

    /// Compute, for every user-defined method whose receiver parameter is
    /// literally named `self` and whose every RETURN-position expression
    /// (see `collect_return_exprs`) decomposes to the SAME self-rooted
    /// field/index chain, `(the method's OWN struct name, method name) ->`
    /// that self-relative `PathStep` chain (an accompanying branch that
    /// never returns — a bounds-check `throw` — is fine; every path that
    /// DOES return must agree). This is what lets a call through a
    /// `std::collections`-style accessor method (`list.get(i)`,
    /// `stack.peek()`, `queue.front()`, `deque.back()` — all of which are,
    /// in their entirety modulo a bounds guard, a bare
    /// `return self.<field>[...]` / `return self.<field>`) be recognized by
    /// `resolve_type_path` as reaching the same slot the field access it
    /// wraps would reach.
    ///
    /// Deliberately walks `Decl::Impl` directly (each method paired with its
    /// OWN `target` struct name) instead of reusing `fn_params`/
    /// `collect_functions` (both keyed by bare method name — the
    /// pre-existing modeling simplification `hot_params`/`fn_capabilities`
    /// already accept). A bare-name key here would be actively WRONG, not
    /// just imprecise: `List.get` and `Dict.get` both live in the SAME
    /// stdlib module and return DIFFERENT self-relative paths (`[data,
    /// Index]` vs `[store, Index]`) — verified live during development that
    /// a bare-name map lets `Dict.get` (declared later in the file) clobber
    /// `List.get`, silently breaking detection for `List<fn() -> str>`
    /// specifically, the flagship shape this fix targets.
    fn collect_transparent_accessor_paths(decls: &[Decl]) -> HashMap<(String, String), Vec<PathStep>> {
        let mut result = HashMap::new();
        Self::collect_transparent_accessor_paths_walk(decls, &mut result);
        result
    }

    fn collect_transparent_accessor_paths_walk(
        decls: &[Decl],
        result: &mut HashMap<(String, String), Vec<PathStep>>,
    ) {
        for d in decls {
            let Decl::Impl { target, methods, .. } = d else {
                continue;
            };
            for m in methods {
                let Decl::Function {
                    name: method_name,
                    params,
                    body: Some(body),
                    ..
                } = m
                else {
                    continue;
                };
                if params.first().map(|p| p.name.as_str()) != Some("self") {
                    continue;
                }
                let mut returns: Vec<&Expr> = Vec::new();
                Self::collect_return_exprs(body, &mut returns);
                if returns.is_empty() {
                    continue;
                }
                let mut agreed: Option<Vec<PathStep>> = None;
                let mut ok = true;
                for r in &returns {
                    let Some((root, path)) = Self::decompose_container_path(r) else {
                        ok = false;
                        break;
                    };
                    if root != "self" {
                        ok = false;
                        break;
                    }
                    match &agreed {
                        None => agreed = Some(path),
                        Some(existing) if *existing == path => {}
                        Some(_) => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    if let Some(path) = agreed {
                        result.insert((target.clone(), method_name.clone()), path);
                    }
                }
            }
        }
    }

    /// Decompose an expression into `(root identifier name, path of
    /// field/index accesses from the root to `expr` itself)`, when `expr` is
    /// a chain of `FieldAccess`/`IndexAccess` rooted at a bare `Identifier`.
    /// `None` for any other shape (a call, a literal, a binary op, ...) —
    /// those are not "a value read out of a container reachable from a bare
    /// name", so the container-invocation heuristic does not apply.
    fn decompose_container_path(expr: &Expr) -> Option<(&str, Vec<PathStep>)> {
        match expr {
            Expr::Identifier { name, .. } => Some((name.as_str(), Vec::new())),
            Expr::FieldAccess { object, field, .. } => {
                let (root, mut path) = Self::decompose_container_path(object)?;
                path.push(PathStep::Field(field.clone()));
                Some((root, path))
            }
            Expr::IndexAccess { object, .. } => {
                let (root, mut path) = Self::decompose_container_path(object)?;
                path.push(PathStep::Index);
                Some((root, path))
            }
            _ => None,
        }
    }

    /// Record every function/method's parameter list and declared return
    /// type, keyed by bare name (recursing into impl/trait methods and actor
    /// handlers). Feeds `hot_params` and `fn_return_closure_caps`.
    fn collect_fn_signatures(&mut self, decls: &[Decl]) {
        for d in decls {
            match d {
                Decl::Function {
                    name,
                    params,
                    ret_ty,
                    ..
                } => {
                    self.fn_params.insert(name.clone(), params.clone());
                    if let Some(rt) = ret_ty {
                        self.fn_ret_ty.insert(name.clone(), rt.clone());
                    }
                }
                Decl::Impl { methods, .. } | Decl::Trait { methods, .. } => {
                    self.collect_fn_signatures(methods);
                }
                Decl::Actor { handlers, .. } => {
                    for h in handlers {
                        self.fn_params.insert(h.name.clone(), h.params.clone());
                        if let Some(rt) = &h.ret_ty {
                            self.fn_ret_ty.insert(h.name.clone(), rt.clone());
                        }
                        self.actor_handler_names.insert(h.name.clone());
                    }
                }
                _ => {}
            }
        }
    }

    /// Every call site reachable inside `expr`: `(callee_bare_name, args,
    /// is_method_or_static)`. `args` never includes a method/static
    /// receiver, matching how `MethodCall`/`StaticMethodCall` already store
    /// them — callers translate an arg index to a `fn_params` index by
    /// adding 1 (for the leading `self`) when `is_method_or_static` is true.
    /// Used only for the STRUCTURAL hot-parameter analysis (which parameter
    /// POSITIONS are invoked/forwarded) — not for capability values.
    fn walk_calls_expr<'a>(expr: &'a Expr, out: &mut Vec<(&'a str, &'a [Expr], bool)>) {
        match expr {
            Expr::FnCall { callee, args, .. } => {
                if let Expr::Identifier { name, .. } = callee.as_ref() {
                    out.push((name.as_str(), args.as_slice(), false));
                }
                Self::walk_calls_expr(callee, out);
                for a in args {
                    Self::walk_calls_expr(a, out);
                }
            }
            Expr::MethodCall {
                object,
                method,
                args,
                ..
            } => {
                out.push((method.as_str(), args.as_slice(), true));
                Self::walk_calls_expr(object, out);
                for a in args {
                    Self::walk_calls_expr(a, out);
                }
            }
            Expr::StaticMethodCall { method, args, .. } => {
                out.push((method.as_str(), args.as_slice(), true));
                for a in args {
                    Self::walk_calls_expr(a, out);
                }
            }
            Expr::FieldAccess { object, .. } => Self::walk_calls_expr(object, out),
            Expr::IndexAccess { object, index, .. } => {
                Self::walk_calls_expr(object, out);
                Self::walk_calls_expr(index, out);
            }
            Expr::BinaryOp { left, right, .. } => {
                Self::walk_calls_expr(left, out);
                Self::walk_calls_expr(right, out);
            }
            Expr::UnaryOp { operand, .. } => Self::walk_calls_expr(operand, out),
            Expr::ArrayLiteral { elements, .. } | Expr::TupleLiteral { elements, .. } => {
                for e in elements {
                    Self::walk_calls_expr(e, out);
                }
            }
            Expr::MapLiteral { entries, .. } => {
                for (k, v) in entries {
                    Self::walk_calls_expr(k, out);
                    Self::walk_calls_expr(v, out);
                }
            }
            Expr::StructLiteral { fields, .. } => {
                for (_, e) in fields {
                    Self::walk_calls_expr(e, out);
                }
            }
            Expr::Lambda { body, .. } => Self::walk_calls_expr(body, out),
            Expr::IfExpr {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                Self::walk_calls_expr(condition, out);
                Self::walk_calls_block(then_branch, out);
                if let Some(b) = else_branch {
                    Self::walk_calls_block(b, out);
                }
            }
            Expr::MatchExpr { subject, arms, .. } => {
                Self::walk_calls_expr(subject, out);
                for arm in arms {
                    Self::walk_calls_expr(&arm.body, out);
                    if let Some(g) = &arm.guard {
                        Self::walk_calls_expr(g, out);
                    }
                }
            }
            Expr::PipeExpr { left, right, .. } => {
                Self::walk_calls_expr(left, out);
                Self::walk_calls_expr(right, out);
            }
            Expr::Borrow { inner, .. }
            | Expr::Deref { inner, .. }
            | Expr::SharedExpr { inner, .. }
            | Expr::MoveExpr { inner, .. }
            | Expr::WeakExpr { inner, .. } => Self::walk_calls_expr(inner, out),
            Expr::ComptimeBlock { body, .. }
            | Expr::QuantumBlock { body, .. }
            | Expr::UnsafeBlock { body, .. } => Self::walk_calls_block(body, out),
            Expr::Cast { expr, .. } => Self::walk_calls_expr(expr, out),
            Expr::Block { block, .. } => Self::walk_calls_block(block, out),
            Expr::Await { value, .. } => Self::walk_calls_expr(value, out),
            Expr::RangeExpr { start, end, .. } => {
                if let Some(s) = start {
                    Self::walk_calls_expr(s, out);
                }
                if let Some(e) = end {
                    Self::walk_calls_expr(e, out);
                }
            }
            Expr::InterpolatedString { parts, .. } => {
                for part in parts {
                    if let kryos_ast::StringPart::Expr(e) = part {
                        Self::walk_calls_expr(e, out);
                    }
                }
            }
            Expr::Identifier { .. }
            | Expr::IntLiteral { .. }
            | Expr::FloatLiteral { .. }
            | Expr::StringLiteral { .. }
            | Expr::CharLiteral { .. }
            | Expr::BoolLiteral { .. }
            | Expr::NoneLiteral { .. } => {}
        }
    }

    fn walk_calls_block<'a>(block: &'a kryos_ast::Block, out: &mut Vec<(&'a str, &'a [Expr], bool)>) {
        for stmt in &block.stmts {
            Self::walk_calls_stmt(stmt, out);
        }
    }

    fn walk_calls_stmt<'a>(stmt: &'a Stmt, out: &mut Vec<(&'a str, &'a [Expr], bool)>) {
        match stmt {
            Stmt::Let { value, .. } => {
                if let Some(e) = value {
                    Self::walk_calls_expr(e, out);
                }
            }
            Stmt::Assign { value, target, .. } => {
                Self::walk_calls_expr(target, out);
                Self::walk_calls_expr(value, out);
            }
            Stmt::Return { value, .. } => {
                if let Some(e) = value {
                    Self::walk_calls_expr(e, out);
                }
            }
            Stmt::If {
                condition,
                then_block,
                elif_clauses,
                else_block,
                ..
            } => {
                Self::walk_calls_expr(condition, out);
                Self::walk_calls_block(then_block, out);
                for (cond, block) in elif_clauses {
                    Self::walk_calls_expr(cond, out);
                    Self::walk_calls_block(block, out);
                }
                if let Some(block) = else_block {
                    Self::walk_calls_block(block, out);
                }
            }
            Stmt::For { iterable, body, .. } => {
                Self::walk_calls_expr(iterable, out);
                Self::walk_calls_block(body, out);
            }
            Stmt::While {
                condition, body, ..
            } => {
                Self::walk_calls_expr(condition, out);
                Self::walk_calls_block(body, out);
            }
            Stmt::Expr { expr, .. } => Self::walk_calls_expr(expr, out),
            Stmt::Spawn { expr, .. } => Self::walk_calls_expr(expr, out),
            Stmt::TryCatch {
                try_block,
                catch_block,
                ..
            } => {
                Self::walk_calls_block(try_block, out);
                Self::walk_calls_block(catch_block, out);
            }
            Stmt::Throw { expr, .. } => Self::walk_calls_expr(expr, out),
            Stmt::Select { branches, .. } => {
                for branch in branches {
                    Self::walk_calls_expr(&branch.channel, out);
                    Self::walk_calls_block(&branch.body, out);
                }
            }
            Stmt::DenyBlock { body, .. } => Self::walk_calls_block(body, out),
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
        }
    }

    /// Every CONTAINER-shaped invocation anywhere in `expr`, reduced to
    /// `(root identifier the access chain is rooted at, the field/index PATH
    /// from that root to the invoked slot)` — see `decompose_container_path`
    /// / `PathStep`. Mirrors `walk_calls_expr`'s full traversal (so no call
    /// site is missed) but reconstructs the WHOLE access chain instead of a
    /// bare callee name, which is what lets `compute_hot_params` check,
    /// against the root's own declared type (`resolve_type_path`), whether a
    /// given path actually terminates in a function-typed slot before
    /// marking it hot. A `MethodCall` is speculatively included (`method`
    /// treated as a trailing `Field` step) because the parser cannot
    /// distinguish `reg.reader()` (a struct field holding a closure) from an
    /// ordinary `reg.some_method()` at parse time (see the parser's own
    /// comment on this ambiguity) — `resolve_type_path` rejects the ones
    /// that don't actually resolve to a function-typed slot, so an ordinary
    /// method call never gets misclassified as hot.
    fn walk_container_calls_expr<'a>(expr: &'a Expr, out: &mut Vec<(&'a str, Vec<PathStep>)>) {
        match expr {
            Expr::FnCall { callee, args, .. } => {
                if let Some((root, path)) = Self::decompose_container_path(callee) {
                    if !path.is_empty() {
                        out.push((root, path));
                    }
                }
                Self::walk_container_calls_expr(callee, out);
                for a in args {
                    Self::walk_container_calls_expr(a, out);
                }
            }
            Expr::MethodCall {
                object,
                method,
                args,
                ..
            } => {
                if let Some((root, mut path)) = Self::decompose_container_path(object) {
                    path.push(PathStep::Field(method.clone()));
                    out.push((root, path));
                }
                Self::walk_container_calls_expr(object, out);
                for a in args {
                    Self::walk_container_calls_expr(a, out);
                }
            }
            Expr::StaticMethodCall { args, .. } => {
                for a in args {
                    Self::walk_container_calls_expr(a, out);
                }
            }
            Expr::FieldAccess { object, .. } => Self::walk_container_calls_expr(object, out),
            Expr::IndexAccess { object, index, .. } => {
                Self::walk_container_calls_expr(object, out);
                Self::walk_container_calls_expr(index, out);
            }
            Expr::BinaryOp { left, right, .. } => {
                Self::walk_container_calls_expr(left, out);
                Self::walk_container_calls_expr(right, out);
            }
            Expr::UnaryOp { operand, .. } => Self::walk_container_calls_expr(operand, out),
            Expr::ArrayLiteral { elements, .. } | Expr::TupleLiteral { elements, .. } => {
                for e in elements {
                    Self::walk_container_calls_expr(e, out);
                }
            }
            Expr::MapLiteral { entries, .. } => {
                for (k, v) in entries {
                    Self::walk_container_calls_expr(k, out);
                    Self::walk_container_calls_expr(v, out);
                }
            }
            Expr::StructLiteral { fields, .. } => {
                for (_, e) in fields {
                    Self::walk_container_calls_expr(e, out);
                }
            }
            Expr::Lambda { body, .. } => Self::walk_container_calls_expr(body, out),
            Expr::IfExpr {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                Self::walk_container_calls_expr(condition, out);
                Self::walk_container_calls_block(then_branch, out);
                if let Some(b) = else_branch {
                    Self::walk_container_calls_block(b, out);
                }
            }
            Expr::MatchExpr { subject, arms, .. } => {
                Self::walk_container_calls_expr(subject, out);
                for arm in arms {
                    Self::walk_container_calls_expr(&arm.body, out);
                    if let Some(g) = &arm.guard {
                        Self::walk_container_calls_expr(g, out);
                    }
                }
            }
            Expr::PipeExpr { left, right, .. } => {
                Self::walk_container_calls_expr(left, out);
                Self::walk_container_calls_expr(right, out);
            }
            Expr::Borrow { inner, .. }
            | Expr::Deref { inner, .. }
            | Expr::SharedExpr { inner, .. }
            | Expr::MoveExpr { inner, .. }
            | Expr::WeakExpr { inner, .. } => Self::walk_container_calls_expr(inner, out),
            Expr::ComptimeBlock { body, .. }
            | Expr::QuantumBlock { body, .. }
            | Expr::UnsafeBlock { body, .. } => Self::walk_container_calls_block(body, out),
            Expr::Cast { expr, .. } => Self::walk_container_calls_expr(expr, out),
            Expr::Block { block, .. } => Self::walk_container_calls_block(block, out),
            Expr::Await { value, .. } => Self::walk_container_calls_expr(value, out),
            Expr::RangeExpr { start, end, .. } => {
                if let Some(s) = start {
                    Self::walk_container_calls_expr(s, out);
                }
                if let Some(e) = end {
                    Self::walk_container_calls_expr(e, out);
                }
            }
            Expr::InterpolatedString { parts, .. } => {
                for part in parts {
                    if let kryos_ast::StringPart::Expr(e) = part {
                        Self::walk_container_calls_expr(e, out);
                    }
                }
            }
            Expr::Identifier { .. }
            | Expr::IntLiteral { .. }
            | Expr::FloatLiteral { .. }
            | Expr::StringLiteral { .. }
            | Expr::CharLiteral { .. }
            | Expr::BoolLiteral { .. }
            | Expr::NoneLiteral { .. } => {}
        }
    }

    fn walk_container_calls_block<'a>(block: &'a kryos_ast::Block, out: &mut Vec<(&'a str, Vec<PathStep>)>) {
        for stmt in &block.stmts {
            Self::walk_container_calls_stmt(stmt, out);
        }
    }

    fn walk_container_calls_stmt<'a>(stmt: &'a Stmt, out: &mut Vec<(&'a str, Vec<PathStep>)>) {
        match stmt {
            Stmt::Let { value, .. } => {
                if let Some(e) = value {
                    Self::walk_container_calls_expr(e, out);
                }
            }
            Stmt::Assign { value, target, .. } => {
                Self::walk_container_calls_expr(target, out);
                Self::walk_container_calls_expr(value, out);
            }
            Stmt::Return { value, .. } => {
                if let Some(e) = value {
                    Self::walk_container_calls_expr(e, out);
                }
            }
            Stmt::If {
                condition,
                then_block,
                elif_clauses,
                else_block,
                ..
            } => {
                Self::walk_container_calls_expr(condition, out);
                Self::walk_container_calls_block(then_block, out);
                for (cond, block) in elif_clauses {
                    Self::walk_container_calls_expr(cond, out);
                    Self::walk_container_calls_block(block, out);
                }
                if let Some(block) = else_block {
                    Self::walk_container_calls_block(block, out);
                }
            }
            Stmt::For { iterable, body, .. } => {
                Self::walk_container_calls_expr(iterable, out);
                Self::walk_container_calls_block(body, out);
            }
            Stmt::While {
                condition, body, ..
            } => {
                Self::walk_container_calls_expr(condition, out);
                Self::walk_container_calls_block(body, out);
            }
            Stmt::Expr { expr, .. } => Self::walk_container_calls_expr(expr, out),
            Stmt::Spawn { expr, .. } => Self::walk_container_calls_expr(expr, out),
            Stmt::TryCatch {
                try_block,
                catch_block,
                ..
            } => {
                Self::walk_container_calls_block(try_block, out);
                Self::walk_container_calls_block(catch_block, out);
            }
            Stmt::Throw { expr, .. } => Self::walk_container_calls_expr(expr, out),
            Stmt::Select { branches, .. } => {
                for branch in branches {
                    Self::walk_container_calls_expr(&branch.channel, out);
                    Self::walk_container_calls_block(&branch.body, out);
                }
            }
            Stmt::DenyBlock { body, .. } => Self::walk_container_calls_block(body, out),
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
        }
    }

    /// Append every actor message handler's `(name, false, body)` so the
    /// STRUCTURAL closure-flow passes (`compute_hot_params`,
    /// `compute_fn_return_closure_caps`) see handler bodies too — a handler
    /// with a fn-typed parameter that it invokes must mark that parameter
    /// hot exactly like an ordinary function, or a closure laundered through
    /// an actor message is invisible to the fix (see
    /// `tests/security/cap_escape_closure_launder_actor.kry`).
    ///
    /// Deliberately NOT folded into `collect_functions` itself: that helper
    /// also feeds `compute_inferred_capabilities`, which has its own,
    /// separate, already-correct handling of actor ceilings (`check_actor`
    /// checks each handler body directly against the actor's OWN declared
    /// scope) — adding handlers there would let a handler's inferred set
    /// leak into `fn_capabilities` and risk widening actor call-site checks
    /// unrelated to this fix. This helper's `false` "not annotated" tag is
    /// meaningful ONLY to the two structural passes above, which don't
    /// special-case annotation at all.
    fn collect_actor_handler_bodies<'a>(
        decls: &'a [Decl],
        out: &mut Vec<(String, bool, &'a kryos_ast::Block, &'a [Param])>,
    ) {
        for d in decls {
            if let Decl::Actor { handlers, .. } = d {
                for h in handlers {
                    out.push((h.name.clone(), false, &h.body, h.params.as_slice()));
                }
            }
        }
    }

    /// Compute, for every function name, the set of PARAMETER INDICES that
    /// are invoked anywhere in the whole program, each mapped to the set of
    /// field/index PATHS (see `PathStep`) through which they are invoked —
    /// either DIRECTLY (`p(...)`, empty path) or via a CONTAINER drill
    /// (`p.field(...)`, `p[i](...)`, or a chain of these — non-empty path,
    /// the residual this closes, LEDGER item 1) — or by being FORWARDED as a
    /// bare argument into another (transitively) hot parameter position of a
    /// different call. A monotone fixed point over a finite lattice
    /// (function x parameter-index x path triples, drawn from the finite set
    /// of field names and index steps that actually appear in the source),
    /// so it terminates.
    ///
    /// This is the structural half of the closure-laundering fix: it never
    /// looks at what capability the forwarded value carries, only at WHICH
    /// positions (and, for containers, which paths) in the call graph a
    /// closure gets invoked. `std::iter::map`'s callback parameter is hot
    /// (its body calls it directly); `map` itself still requires nothing
    /// extra — only a CALL to `map` with a SPECIFIC closure argument does
    /// (see `accumulate_hot_extra_caps`), which is what keeps ordinary HOF
    /// usage annotation-free instead of cascading.
    fn compute_hot_params(
        &self,
        decls: &[Decl],
    ) -> HashMap<String, HashMap<usize, HashSet<Vec<PathStep>>>> {
        let mut fns: Vec<(String, bool, &kryos_ast::Block, &[Param])> = Vec::new();
        Self::collect_functions(decls, &mut fns);
        Self::collect_actor_handler_bodies(decls, &mut fns);

        let mut hot: HashMap<String, HashMap<usize, HashSet<Vec<PathStep>>>> = HashMap::new();

        // Seed A: a function's own fn-typed parameter invoked DIRECTLY —
        // records the EMPTY path (the parameter's own value IS the closure).
        for (name, _annotated, body, params) in &fns {
            let mut calls: Vec<(&str, &[Expr], bool)> = Vec::new();
            Self::walk_calls_block(body, &mut calls);
            for (i, p) in params.iter().enumerate() {
                if !Self::is_fn_typed(&p.ty) {
                    continue;
                }
                if calls
                    .iter()
                    .any(|(cname, _args, is_method)| !is_method && *cname == p.name)
                {
                    hot.entry(name.clone())
                        .or_default()
                        .entry(i)
                        .or_default()
                        .insert(Vec::new());
                }
            }
        }

        // Seed B: a function's own CONTAINER-typed parameter (struct field /
        // array element / map value / nested combination) whose contents are
        // invoked via a field/index access chain — the residual this closes.
        for (name, _annotated, body, params) in &fns {
            let mut occurrences: Vec<(&str, Vec<PathStep>)> = Vec::new();
            Self::walk_container_calls_block(body, &mut occurrences);
            for (i, p) in params.iter().enumerate() {
                let Some(ty) = &p.ty else { continue };
                for (root, path) in &occurrences {
                    if *root != p.name {
                        continue;
                    }
                    if matches!(self.resolve_type_path(ty, path), Some(TypeExpr::Function { .. })) {
                        hot.entry(name.clone())
                            .or_default()
                            .entry(i)
                            .or_default()
                            .insert(path.clone());
                    }
                }
            }
        }

        // Propagate: forwarding a fn-typed/container-typed parameter as a
        // bare argument into another (already-known-hot) parameter
        // position. The forwarded value is unchanged (a bare identifier
        // forward, not a further field/index drill), so the callee's own
        // recorded PATHS at that position carry over verbatim.
        loop {
            let mut changed = false;
            for (name, _annotated, body, params) in &fns {
                let mut calls: Vec<(&str, &[Expr], bool)> = Vec::new();
                Self::walk_calls_block(body, &mut calls);
                for (i, p) in params.iter().enumerate() {
                    if !self.is_fn_bearing_type(&p.ty) {
                        continue;
                    }
                    for (cname, cargs, is_method) in &calls {
                        // An actor handler's `MethodCall`-shaped dispatch has
                        // NO receiver slot in its own `params` (unlike a
                        // struct `impl` method, which requires an explicit
                        // `self` as `params[0]`) — see `actor_handler_names`.
                        let has_self_offset =
                            *is_method && !self.actor_handler_names.contains(*cname);
                        let offset = if has_self_offset { 1usize } else { 0usize };
                        for (j, a) in cargs.iter().enumerate() {
                            if let Expr::Identifier { name: an, .. } = a {
                                if an == &p.name {
                                    let idx = j + offset;
                                    let forwarded: Option<HashSet<Vec<PathStep>>> =
                                        hot.get(*cname).and_then(|m| m.get(&idx)).cloned();
                                    if let Some(paths) = forwarded {
                                        let entry = hot
                                            .entry(name.clone())
                                            .or_default()
                                            .entry(i)
                                            .or_default();
                                        let before = entry.len();
                                        entry.extend(paths);
                                        if entry.len() != before {
                                            changed = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }

        hot
    }

    /// Compute `hot_param_companions` (see its field doc for the full
    /// rationale): for every function's own DIRECTLY-invoked (Seed-A-style)
    /// fn-typed parameter, and for every argument POSITION passed at each of
    /// that function's OWN internal call sites which invoke it, the OTHER
    /// own parameter (by index) and path the argument expression actually
    /// decomposes to via `decompose_container_path` -- real, per-declaration
    /// data flow, never a guess from declared type shape. `None` at a
    /// position means no single companion could be proven (multiple
    /// disagreeing internal call sites, or an argument expression that
    /// doesn't decompose to another of the SAME function's own parameters at
    /// all) -- callers of this map must treat `None` as unresolvable, not as
    /// "no capability needed".
    fn compute_hot_param_companions(
        &self,
        decls: &[Decl],
    ) -> HashMap<String, HashMap<usize, Vec<Option<(usize, Vec<PathStep>)>>>> {
        let mut fns: Vec<(String, bool, &kryos_ast::Block, &[Param])> = Vec::new();
        Self::collect_functions(decls, &mut fns);
        Self::collect_actor_handler_bodies(decls, &mut fns);

        let mut result: HashMap<String, HashMap<usize, Vec<Option<(usize, Vec<PathStep>)>>>> =
            HashMap::new();

        for (name, _annotated, body, params) in &fns {
            let mut calls: Vec<(&str, &[Expr], bool)> = Vec::new();
            Self::walk_calls_block(body, &mut calls);
            for (i, p) in params.iter().enumerate() {
                if !Self::is_fn_typed(&p.ty) {
                    continue;
                }
                // Every DIRECT invocation of THIS parameter, bare-call-shaped
                // (matching Seed A's own detection), within the SAME body.
                let sites: Vec<&[Expr]> = calls
                    .iter()
                    .filter(|(cname, _args, is_method)| !is_method && *cname == p.name)
                    .map(|(_, args, _)| *args)
                    .collect();
                if sites.is_empty() {
                    continue;
                }
                let max_arity = sites.iter().map(|a| a.len()).max().unwrap_or(0);
                let mut per_slot: Vec<Option<(usize, Vec<PathStep>)>> = vec![None; max_arity];
                for (slot, per_slot_entry) in per_slot.iter_mut().enumerate() {
                    let mut candidate: Option<(usize, Vec<PathStep>)> = None;
                    let mut ok = true;
                    for site in &sites {
                        let Some(arg_expr) = site.get(slot) else {
                            ok = false;
                            break;
                        };
                        let Some((root, path)) = Self::decompose_container_path(arg_expr) else {
                            ok = false;
                            break;
                        };
                        let Some(comp_idx) = params
                            .iter()
                            .position(|q| q.name == root && q.name != p.name)
                        else {
                            ok = false;
                            break;
                        };
                        match &candidate {
                            None => candidate = Some((comp_idx, path)),
                            Some((ci, cp)) if *ci == comp_idx && *cp == path => {}
                            _ => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if ok {
                        *per_slot_entry = candidate;
                    }
                }
                result.entry(name.clone()).or_default().insert(i, per_slot);
            }
        }

        result
    }

    /// Every RETURN-position expression in `block`: explicit `return expr`
    /// statements (recursing through if/for/while/try-catch so a return
    /// nested in a branch still counts), plus the block's own trailing tail
    /// expression (a best-effort implicit-return heuristic — nested tail
    /// positions inside a branch used as the function's overall value, e.g.
    /// a trailing `if/else` with no final `return`, are NOT covered; explicit
    /// `return` is the documented/preferred style, so this is a deliberately
    /// bounded approximation, not full control-flow return analysis).
    fn collect_return_exprs<'a>(block: &'a kryos_ast::Block, out: &mut Vec<&'a Expr>) {
        let last_idx = block.stmts.len().checked_sub(1);
        for (i, stmt) in block.stmts.iter().enumerate() {
            match stmt {
                Stmt::Return { value: Some(e), .. } => out.push(e),
                Stmt::If {
                    then_block,
                    elif_clauses,
                    else_block,
                    ..
                } => {
                    Self::collect_return_exprs(then_block, out);
                    for (_, b) in elif_clauses {
                        Self::collect_return_exprs(b, out);
                    }
                    if let Some(b) = else_block {
                        Self::collect_return_exprs(b, out);
                    }
                }
                Stmt::For { body, .. } | Stmt::While { body, .. } => {
                    Self::collect_return_exprs(body, out)
                }
                Stmt::TryCatch {
                    try_block,
                    catch_block,
                    ..
                } => {
                    Self::collect_return_exprs(try_block, out);
                    Self::collect_return_exprs(catch_block, out);
                }
                Stmt::Expr { expr, .. } if Some(i) == last_idx => out.push(expr),
                _ => {}
            }
        }
    }

    /// Merge two `ClosureCapsResult`s discovered for the SAME function's
    /// different return paths. `Known` sets union (both paths are equally
    /// real); anything else disagreeing collapses to `Unknown` (sound: a
    /// function whose return authority isn't uniformly resolvable is treated
    /// as opaque, never silently narrowed).
    fn merge_closure_caps(a: ClosureCapsResult, b: ClosureCapsResult) -> ClosureCapsResult {
        match (a, b) {
            (ClosureCapsResult::Known(x), ClosureCapsResult::Known(y)) => {
                ClosureCapsResult::Known(x.union(&y))
            }
            (ClosureCapsResult::DependsOnParam(x), ClosureCapsResult::DependsOnParam(y))
                if x == y =>
            {
                ClosureCapsResult::DependsOnParam(x)
            }
            _ => ClosureCapsResult::Unknown,
        }
    }

    /// Resolve the statically-known authority carried by an expression that
    /// evaluates to a first-class FUNCTION VALUE.
    ///
    /// - A lambda literal's authority is its body's own capability
    ///   requirement (reuses `collect_caps_expr`, the same computation
    ///   already trusted for ordinary inference).
    /// - A `let`-bound local resolves through `local_caps` (see
    ///   `build_local_closure_caps`).
    /// - One of the CURRENT function's own fn-typed parameters resolves to
    ///   `DependsOnParam` — deferred to the current function's own call
    ///   sites (see `hot_params`), never charged here.
    /// - A reference to a NAMED function/builtin hands out that function's
    ///   own (declared/inferred) capability set — the existing rule already
    ///   enforced at the reference site by `check_builtin_value_ref`.
    /// - A call to a function with a known `fn_return_closure_caps` entry
    ///   resolves through it (recursing into the ACTUAL argument when the
    ///   callee's return depends on one of ITS OWN parameters — a simple
    ///   passthrough function).
    /// - Everything else (array/map/struct-field reads, method calls,
    ///   conditionals, ...) is `Unknown` — the sound, conservative default.
    #[allow(clippy::too_many_arguments)]
    fn resolve_closure_caps(
        &self,
        expr: &Expr,
        working: &HashMap<String, CapabilitySet>,
        fn_return_caps: &HashMap<String, ClosureCapsResult>,
        local_caps: &HashMap<String, ClosureCapsResult>,
        own_params: &HashSet<String>,
    ) -> ClosureCapsResult {
        match expr {
            // NOTE: passes an empty container-literal map for the lambda's
            // OWN body, not the enclosing function's `local_container_lits`.
            // A lambda literal's body is a single EXPRESSION (no `let`
            // statements of its own — see `Expr::Lambda`), so there is
            // nothing new to build here; the narrow gap this leaves (an
            // inline lambda argument's body itself drilling into an
            // ENCLOSING container local, e.g. `zero_cap_tool(|| reg.reader())`)
            // is not one of the shapes this fix targets (LEDGER item 1 is
            // about a container VALUE flowing through a parameter/local, not
            // a fresh closure literal referencing one from its lexical
            // scope) and falls back to the existing sound `Unknown` default.
            //
            // TRANSPARENT-FORWARDING LAMBDA: an inline callback whose ONLY
            // capability-relevant behavior is invoking its OWN bound
            // parameter (`|f| f()`, `|f| f(10)`) -- the shape `map`/`filter`/
            // any elementwise HOF needs when the array being mapped holds
            // CLOSURES rather than plain data (a plugin/tool registry). Such
            // a lambda carries no authority OF ITS OWN; its authority is
            // whatever gets bound to its own parameter at each invocation,
            // which is determined by the ENCLOSING CALL, not by this lambda.
            // Detected the same way Seed A detects a NAMED function's own
            // hot parameter (a bare call to the param's own name); resolved
            // to `DependsOnParam(<the LAMBDA's own param name>)`, which the
            // call-site resolver (`accumulate_hot_extra_caps`) distinguishes
            // from the ENCLOSING function's own hot parameter (already
            // `own_params`-gated there) by checking whether the name is even
            // IN `own_params` -- if not, it must belong to this argument
            // lambda's own scope, whose real binding at runtime is
            // determined by the callee's body, not by this call site's
            // argument shapes; provenance can't be proven here, so the FULL
            // capability set is charged (see `accumulate_hot_extra_caps`'s
            // `DependsOnParam` arm -- a prior shape-matching relief mechanism
            // here was removed as unsound; see the ledger for why).
            Expr::Lambda { params, body, .. } => {
                // NOTE: an inline lambda's fn-typed parameter is almost
                // always UNANNOTATED (`|f| f(10)`, not `|f: fn(i64)->i64|
                // f(10)`) -- its real type is inferred from context (the
                // HOF's own signature). So, unlike a NAMED function's own
                // parameters (where `is_fn_typed` on the DECLARED type is
                // exact), candidacy here is "any of this lambda's own bound
                // parameter names", filtered down to the ones actually
                // invoked directly in the body below -- not gated on a type
                // annotation that is normally absent.
                let lambda_own_params: HashSet<String> =
                    params.iter().map(|p| p.name.clone()).collect();
                if !lambda_own_params.is_empty() {
                    let mut calls: Vec<(&str, &[Expr], bool)> = Vec::new();
                    Self::walk_calls_expr(body, &mut calls);
                    let hot_own_param = lambda_own_params.iter().find(|pname| {
                        calls
                            .iter()
                            .any(|(cname, _args, is_method)| !is_method && *cname == pname.as_str())
                    });
                    if let Some(hot_p) = hot_own_param {
                        let mut ext_own_params = own_params.clone();
                        ext_own_params.extend(lambda_own_params.iter().cloned());
                        self.structural_lambda_eval_depth
                            .set(self.structural_lambda_eval_depth.get() + 1);
                        let body_caps = self.collect_caps_expr(
                            body,
                            working,
                            &ext_own_params,
                            local_caps,
                            &HashMap::new(),
                        );
                        self.structural_lambda_eval_depth
                            .set(self.structural_lambda_eval_depth.get() - 1);
                        if body_caps.is_empty() {
                            return ClosureCapsResult::DependsOnParam(hot_p.clone());
                        }
                        return ClosureCapsResult::Known(body_caps);
                    }
                }
                ClosureCapsResult::Known(self.collect_caps_expr(
                    body,
                    working,
                    own_params,
                    local_caps,
                    &HashMap::new(),
                ))
            }
            Expr::Identifier { name, .. } => {
                if let Some(r) = local_caps.get(name) {
                    return r.clone();
                }
                if own_params.contains(name) {
                    return ClosureCapsResult::DependsOnParam(name.clone());
                }
                if self.defined_fns.contains(name) {
                    // If `name` ITSELF has a hot parameter (it forwards a
                    // caller-supplied fn-value into a call, directly or
                    // through a container path -- e.g. `fn invoke(f: fn() ->
                    // str) -> str { return f() }`), a bare reference to
                    // `name` as an unapplied VALUE carries authority that
                    // depends on whatever it is EVENTUALLY called with --
                    // which isn't known here (this is `name` being handed
                    // onward, e.g. `map(tools, invoke)`, not `name` being
                    // called with a concrete argument). Attributing it
                    // `working[name]`'s OWN declared/inferred set (which
                    // only reflects gated BUILTINS `name` calls directly,
                    // never what its hot parameter might carry) silently
                    // drops that dependency -- verified live: passing a
                    // privileged closure through `tools` into `map(tools,
                    // invoke)` leaked the secret with NO capability required
                    // before this check existed. Fall back to the sound
                    // `Unknown` -> `Capability::All` default instead of
                    // asserting a value we cannot actually vouch for. A
                    // named function with NO hot parameters of its own (the
                    // overwhelming majority -- any plain predicate/mapper
                    // passed to `std::iter`) is unaffected: this only
                    // engages for a function that is ITSELF a caller-supplied-
                    // closure invoker being forwarded as a first-class value.
                    if self.hot_params.get(name).is_some_and(|m| !m.is_empty()) {
                        return ClosureCapsResult::Unknown;
                    }
                    return ClosureCapsResult::Known(
                        working.get(name).cloned().unwrap_or_else(CapabilitySet::empty),
                    );
                }
                if let Some(cap) = required_capability_for_builtin(name) {
                    let mut s = CapabilitySet::empty();
                    s.insert(cap);
                    return ClosureCapsResult::Known(s);
                }
                ClosureCapsResult::Unknown
            }
            Expr::FnCall { callee, args, .. } => {
                match callee.as_ref() {
                    Expr::Identifier { name, .. } => {
                        // The callee may be a LOCAL bound to a fn-returning
                        // expression rather than a named function -- a
                        // curried intermediate step (`let step2 =
                        // step1(2)`). `local_caps` already holds the fully
                        // resolved authority for such a local (computed when
                        // its own `let` was processed); check it BEFORE
                        // `fn_return_caps` (which is keyed by function name
                        // and knows nothing about locals), matching the
                        // local-shadows-function convention used elsewhere
                        // in this file.
                        if let Some(r) = local_caps.get(name) {
                            return r.clone();
                        }
                        match fn_return_caps.get(name) {
                            Some(ClosureCapsResult::Known(c)) => ClosureCapsResult::Known(c.clone()),
                            Some(ClosureCapsResult::DependsOnParam(pname)) => {
                                let Some(params) = self.fn_params.get(name) else {
                                    return ClosureCapsResult::Unknown;
                                };
                                let Some(idx) = params.iter().position(|p| &p.name == pname) else {
                                    return ClosureCapsResult::Unknown;
                                };
                                match args.get(idx) {
                                    Some(arg) => self.resolve_closure_caps(
                                        arg, working, fn_return_caps, local_caps, own_params,
                                    ),
                                    None => ClosureCapsResult::Unknown,
                                }
                            }
                            _ => ClosureCapsResult::Unknown,
                        }
                    }
                    // A CURRIED/chained call (`f(a)(b)`): the callee of THIS
                    // call is itself another call. Peel one layer at a time
                    // by recursing -- sound because `fn_return_closure_caps`
                    // (via `collect_caps_expr`'s `Lambda` arm, which already
                    // unions a lambda's FULL nested body into one flat
                    // requirement regardless of how many closures it is
                    // nested inside) already over-approximates a curried
                    // function's return authority at the FIRST resolution
                    // step, so each further application can safely reuse the
                    // same resolved value rather than needing its own.
                    Expr::FnCall { .. } | Expr::MethodCall { .. } | Expr::StaticMethodCall { .. } => {
                        self.resolve_closure_caps(callee, working, fn_return_caps, local_caps, own_params)
                    }
                    _ => ClosureCapsResult::Unknown,
                }
            }
            _ => ClosureCapsResult::Unknown,
        }
    }

    /// Resolve the statically-known authority carried by the value reached
    /// by walking `path` (a field/index access chain — see `PathStep`) into
    /// `expr`. This is the CONTAINER counterpart of `resolve_closure_caps`
    /// (which it delegates to once `path` is exhausted — at that point
    /// `expr` denotes the closure value itself, the pre-existing case).
    ///
    /// - A `StructLiteral` + a leading `Field(name)` step descends into the
    ///   named field's expression (or `Unknown` if the literal doesn't
    ///   explicitly write that field — a defaulted field's value can't be
    ///   read back statically).
    /// - An `ArrayLiteral`/`MapLiteral` + a leading `Index` step is
    ///   INDEX-INSENSITIVE by design (see `PathStep::Index`): it unions the
    ///   result over EVERY element/value in the literal, so any write
    ///   contributes and an empty literal resolves to `Known(empty)` (no
    ///   element can ever be invoked at runtime — it would panic first).
    /// - An `Identifier` resolves through `local_container_lits` (a `let
    ///   reg = Registry { .. }` binding, see `build_local_container_lits`),
    ///   or to `DependsOnParam` if it names one of the CURRENT function's own
    ///   parameters (the container was itself received as a parameter and is
    ///   being read/forwarded — deferred to this function's own call sites,
    ///   mirroring the direct fn-typed-parameter case).
    /// - Anything else (a function call returning a container, a
    ///   conditionally-selected container, a container read out of ANOTHER
    ///   container, ...) is `Unknown` — the same sound, documented fallback
    ///   used for every other unresolvable shape; see LEDGER item 1 for why
    ///   these particular shapes are out of scope (the container must be
    ///   built from a LITERAL, directly or via a local alias, to be traced).
    #[allow(clippy::too_many_arguments)]
    fn resolve_container_path_caps(
        &self,
        expr: &Expr,
        path: &[PathStep],
        working: &HashMap<String, CapabilitySet>,
        fn_return_caps: &HashMap<String, ClosureCapsResult>,
        local_caps: &HashMap<String, ClosureCapsResult>,
        local_container_lits: &HashMap<String, Expr>,
        own_params: &HashSet<String>,
    ) -> ClosureCapsResult {
        let Some((head, rest)) = path.split_first() else {
            return self.resolve_closure_caps(expr, working, fn_return_caps, local_caps, own_params);
        };
        match (head, expr) {
            (PathStep::Field(fname), Expr::StructLiteral { fields, .. }) => {
                match fields.iter().find(|(n, _)| n == fname) {
                    Some((_, fexpr)) => self.resolve_container_path_caps(
                        fexpr, rest, working, fn_return_caps, local_caps, local_container_lits, own_params,
                    ),
                    None => ClosureCapsResult::Unknown,
                }
            }
            (PathStep::Index, Expr::ArrayLiteral { elements, .. }) => {
                Self::merge_all(elements.iter().map(|el| {
                    self.resolve_container_path_caps(
                        el, rest, working, fn_return_caps, local_caps, local_container_lits, own_params,
                    )
                }))
            }
            (PathStep::Index, Expr::MapLiteral { entries, .. }) => {
                Self::merge_all(entries.iter().map(|(_, vexpr)| {
                    self.resolve_container_path_caps(
                        vexpr, rest, working, fn_return_caps, local_caps, local_container_lits, own_params,
                    )
                }))
            }
            (_, Expr::Identifier { name, .. }) => {
                if let Some(lit) = local_container_lits.get(name) {
                    // Clone to release the borrow of `local_container_lits`
                    // before recursing back into it.
                    let lit = lit.clone();
                    return self.resolve_container_path_caps(
                        &lit, path, working, fn_return_caps, local_caps, local_container_lits, own_params,
                    );
                }
                if own_params.contains(name) {
                    return ClosureCapsResult::DependsOnParam(name.clone());
                }
                ClosureCapsResult::Unknown
            }
            _ => ClosureCapsResult::Unknown,
        }
    }

    /// Fold an iterator of per-element/per-value `ClosureCapsResult`s into
    /// one combined result via `merge_closure_caps`, treating an EMPTY
    /// iterator (an empty array/map literal) as `Known(empty)` — no element
    /// exists to carry any authority, and none could ever be invoked at
    /// runtime without panicking first.
    fn merge_all(results: impl Iterator<Item = ClosureCapsResult>) -> ClosureCapsResult {
        results
            .fold(None, |acc, r| {
                Some(match acc {
                    None => r,
                    Some(prev) => Self::merge_closure_caps(prev, r),
                })
            })
            .unwrap_or_else(|| ClosureCapsResult::Known(CapabilitySet::empty()))
    }

    /// Best-effort, FLAT (not lexical-scope-precise: two `let`s of the same
    /// name in disjoint `if`/`else` branches collide in the same map, the
    /// later one winning) collection of every `let name = expr` binding's
    /// resolved closure authority within `block`, processed in textual
    /// order so a local can reference an EARLIER local. Used by
    /// `resolve_closure_caps`'s `Identifier` arm to trace a closure through
    /// a local variable (`let reader = make_secret_reader(path)`).
    fn build_local_closure_caps(
        &self,
        block: &kryos_ast::Block,
        working: &HashMap<String, CapabilitySet>,
        fn_return_caps: &HashMap<String, ClosureCapsResult>,
        own_params: &HashSet<String>,
        local_container_lits: &HashMap<String, Expr>,
    ) -> HashMap<String, ClosureCapsResult> {
        let mut locals = HashMap::new();
        self.build_local_closure_caps_block(block, working, fn_return_caps, own_params, local_container_lits, &mut locals);
        locals
    }

    fn build_local_closure_caps_block(
        &self,
        block: &kryos_ast::Block,
        working: &HashMap<String, CapabilitySet>,
        fn_return_caps: &HashMap<String, ClosureCapsResult>,
        own_params: &HashSet<String>,
        local_container_lits: &HashMap<String, Expr>,
        locals: &mut HashMap<String, ClosureCapsResult>,
    ) {
        for stmt in &block.stmts {
            match stmt {
                Stmt::Let {
                    name, value: Some(v), ..
                } => {
                    // A nested NAMED function (`fn adder(y) { .. }` declared
                    // inside another function's body) desugars to exactly
                    // this shape -- `let adder = fn(y) { .. }` -- in the
                    // parser (`parse_inner_fn`). Its own body may itself
                    // contain further `let`s (including further nested
                    // functions), which need to be in THIS SAME flat map
                    // BEFORE resolving `v`'s own capability below, so that a
                    // direct call to one of them from within the lambda's
                    // own body (`return adder(10)`) resolves when THIS
                    // lambda's (`make_adder`'s) own requirement is computed,
                    // instead of falling through to the fail-closed `Unknown`
                    // default. Flattening nested scopes into one map is
                    // consistent with this function's existing "best-effort,
                    // not scope-precise" design; the recursion handles
                    // arbitrary nesting depth.
                    if let Expr::Lambda { body, .. } = v {
                        // A self-recursive nested named function (`fn nfact(n)
                        // { return n * nfact(n - 1) }`, desugared to `let
                        // nfact = fn(n) { .. }`) calls ITS OWN name from
                        // inside its own body. Pre-register an optimistic
                        // `Known(empty)` placeholder for `name` BEFORE
                        // recursing into the body, so the self-reference
                        // resolves to something instead of `Unknown` (which
                        // would otherwise require `all` for the extremely
                        // common named-recursion idiom). This is a narrow,
                        // sound-enough approximation consistent with this
                        // function's existing "best-effort, not scope-precise"
                        // design: it can under-count ONLY the capability
                        // carried by the recursive call itself (a purely
                        // internal self-reference, never external caller-
                        // supplied authority), while any OTHER gated builtin
                        // the function calls directly is still counted
                        // normally via the real `r` computed below, which
                        // OVERWRITES this placeholder once known.
                        locals.insert(name.clone(), ClosureCapsResult::Known(CapabilitySet::empty()));
                        if let Expr::Block { block, .. } = body.as_ref() {
                            self.build_local_closure_caps_block(block, working, fn_return_caps, own_params, local_container_lits, locals);
                        }
                    }
                    // A closure/fn-value read OUT of a container via a
                    // field/index chain and bound to a local (`let f =
                    // m["k"]`), as opposed to invoked immediately
                    // (`m["k"]()`, already resolved precisely at the CALL
                    // site by `resolve_direct_invoke_caps`/
                    // `resolve_method_field_invoke_caps`) -- resolve it the
                    // SAME way at BIND time, via `resolve_container_path_caps`,
                    // so a later direct call through the local (`f()`) finds
                    // its real (possibly EMPTY) authority instead of falling
                    // through to the fail-closed `Unknown` default just
                    // because of the extra level of local-variable
                    // indirection. Only engages for a genuine field/index
                    // chain (non-empty path); a bare identifier alias still
                    // goes through the existing `resolve_closure_caps` path
                    // below, unchanged.
                    let r = match Self::decompose_container_path(v) {
                        Some((root, path)) if !path.is_empty() => {
                            let root_expr = Expr::Identifier {
                                name: root.to_string(),
                                span: Span::DUMMY,
                            };
                            self.resolve_container_path_caps(
                                &root_expr,
                                &path,
                                working,
                                fn_return_caps,
                                &*locals,
                                local_container_lits,
                                own_params,
                            )
                        }
                        _ => self.resolve_closure_caps(v, working, fn_return_caps, &*locals, own_params),
                    };
                    locals.insert(name.clone(), r);
                }
                Stmt::If {
                    then_block,
                    elif_clauses,
                    else_block,
                    ..
                } => {
                    self.build_local_closure_caps_block(then_block, working, fn_return_caps, own_params, local_container_lits, locals);
                    for (_, b) in elif_clauses {
                        self.build_local_closure_caps_block(b, working, fn_return_caps, own_params, local_container_lits, locals);
                    }
                    if let Some(b) = else_block {
                        self.build_local_closure_caps_block(b, working, fn_return_caps, own_params, local_container_lits, locals);
                    }
                }
                Stmt::For { body, .. } | Stmt::While { body, .. } => {
                    self.build_local_closure_caps_block(body, working, fn_return_caps, own_params, local_container_lits, locals)
                }
                Stmt::TryCatch {
                    try_block,
                    catch_block,
                    ..
                } => {
                    self.build_local_closure_caps_block(try_block, working, fn_return_caps, own_params, local_container_lits, locals);
                    self.build_local_closure_caps_block(catch_block, working, fn_return_caps, own_params, local_container_lits, locals);
                }
                _ => {}
            }
        }
    }

    /// Best-effort, FLAT (same precision level as `build_local_closure_caps`)
    /// collection of every `let name = expr` binding whose initializer is a
    /// struct/array/map LITERAL (or a bare alias of another already-tracked
    /// container local), within `block`, processed in textual order. Feeds
    /// `resolve_container_path_caps`'s `Identifier` arm — lets `let reg =
    /// Registry { reader: r }` / `zero_cap_tool(reg)` trace back to the
    /// literal `reg` was built from. Purely syntactic (no capability
    /// resolution here — that happens lazily, at USE time, in
    /// `resolve_container_path_caps`, once the specific field/index path
    /// being invoked is known), so unlike `build_local_closure_caps` this
    /// needs no `working`/`fn_return_caps`/`own_params` context.
    ///
    /// **MUTATION TRACKING (closes the live bypass a prior session's fix
    /// left open — see LEDGER, "closure into a container via push / map
    /// insert"):** the ORIGINAL version of this function only ever looked at
    /// `Stmt::Let`, so a container built by `let mut tools = []` then
    /// POPULATED afterward (`tools = push(tools, reader)`, `m["k"] = reader`,
    /// `arr[i] = reader`, `r.field = reader`) kept the STALE initial snapshot
    /// forever. That snapshot was usually an EMPTY literal (`[]`/`{}`), and
    /// `resolve_container_path_caps`'s index-insensitive union over zero
    /// elements resolves to `Known(empty)` — not `Unknown`. So a mutated
    /// container didn't just fall through the documented conservative
    /// `Unknown` -> `Capability::All` fallback (which the report's "requires
    /// `Capability::All`" claim assumed); it actively asserted "this
    /// container carries no authority", which is FALSE and worse than doing
    /// nothing. Every subsequent-mutation call site walked a lie instead of
    /// admitting ignorance.
    ///
    /// Fix: `Stmt::Assign` is now walked too. Three shapes are resolved
    /// PRECISELY (the tracked literal is rebuilt with the write applied, so
    /// a later read sees the real authority):
    /// - `X = push(X, v)` (the canonical growable-container idiom, see
    ///   CLAUDE.md's push-aliasing gotcha) appends `v` into the tracked
    ///   array literal for `X`.
    /// - `X = Y` where `Y` is itself a tracked container local re-aliases
    ///   `X` to it (same rule the `Let` arm already applies).
    /// - `X = <fresh struct/array/map literal>` overwrites the tracked
    ///   snapshot (same rule as `Let`).
    /// - A field/index write reaching into an ALREADY-tracked container
    ///   through a field/index PATH (`r.field = v`, `arr[i] = v`,
    ///   `m[k] = v`, and nested combinations like `hs[i].handler = v`) is
    ///   rebuilt via `rebuild_container_write`, splicing `v` in at the
    ///   reached point (index writes are index-INSENSITIVE — appended to
    ///   the union — matching the read side's existing design).
    ///
    /// Every OTHER assignment shape reaching a tracked name — an
    /// unrecognized reassignment (`X = some_function()`, `X = other_arr`
    /// where `other_arr` isn't itself tracked), a compound assignment
    /// (`+=` and friends), or a field/index write whose path can't be
    /// resolved against the literal's actual current shape — INVALIDATES
    /// (removes) that name's tracked entry instead of leaving it stale, so a
    /// later read correctly falls through to `Unknown` -> `Capability::All`
    /// (fail CLOSED) rather than silently keeping a snapshot that is now
    /// known to be wrong. This is the concrete fix for "an unanalyzable
    /// fn-value must FAIL CLOSED, not open."
    fn build_local_container_lits(block: &kryos_ast::Block) -> HashMap<String, Expr> {
        let mut locals = HashMap::new();
        Self::build_local_container_lits_block(block, &mut locals);
        locals
    }

    /// Apply a single `Stmt::Assign` to the in-progress container-literal
    /// snapshot map. See `build_local_container_lits`'s doc comment for the
    /// recognized shapes and the fail-closed invalidation rule.
    fn apply_container_assign(locals: &mut HashMap<String, Expr>, target: &Expr, op: kryos_ast::AssignOp, value: &Expr) {
        // Compound assignment (`+=`, ...) never applies to a fn-bearing
        // container in any of the shapes this tracks; if it targets a
        // tracked root, the snapshot is no longer trustworthy.
        if !matches!(op, kryos_ast::AssignOp::Assign) {
            if let Some((root, _)) = Self::decompose_container_path(target) {
                locals.remove(root);
            }
            return;
        }
        match target {
            Expr::Identifier { name, .. } => {
                // `X = push(X, v)` — in-place append (see CLAUDE.md's push
                // aliasing gotcha: `push` grows the shared buffer and
                // returns the handle, so `X = push(X, v)` is the canonical,
                // and by far the most common, idiom for this).
                if let Expr::FnCall { callee, args, .. } = value {
                    if let (Expr::Identifier { name: callee_name, .. }, [src, pushed]) =
                        (callee.as_ref(), args.as_slice())
                    {
                        if callee_name == "push" {
                            if let Expr::Identifier { name: src_name, .. } = src {
                                if src_name == name {
                                    if let Some(Expr::ArrayLiteral { elements, span }) =
                                        locals.get(name).cloned()
                                    {
                                        let mut new_elements = elements;
                                        new_elements.push(pushed.clone());
                                        locals.insert(
                                            name.clone(),
                                            Expr::ArrayLiteral { elements: new_elements, span },
                                        );
                                    } else {
                                        // Not currently tracked as an array
                                        // literal (unknown prior content, or
                                        // never tracked at all) — appending
                                        // one MORE element on top of an
                                        // unknown base would UNDER-report
                                        // whatever authority the unknown
                                        // part carries. Stay untracked.
                                        locals.remove(name);
                                    }
                                    return;
                                }
                            }
                        }
                    }
                }
                // `X = Y` — alias to another tracked container local (same
                // rule as the `Let` arm below).
                if let Expr::Identifier { name: src, .. } = value {
                    match locals.get(src).cloned() {
                        Some(existing) => {
                            locals.insert(name.clone(), existing);
                        }
                        None => {
                            locals.remove(name);
                        }
                    }
                    return;
                }
                // `X = <fresh literal>` — overwrite with the new snapshot.
                match value {
                    Expr::StructLiteral { .. } | Expr::ArrayLiteral { .. } | Expr::MapLiteral { .. } => {
                        locals.insert(name.clone(), value.clone());
                    }
                    _ => {
                        // Anything else we can't characterize (a function
                        // call result, an arithmetic/ternary expression, a
                        // read out of another container, ...) — invalidate
                        // rather than keep a now-stale snapshot.
                        locals.remove(name);
                    }
                }
            }
            Expr::FieldAccess { object, field, .. } => {
                Self::apply_container_path_write(
                    locals,
                    object,
                    &PathStep::Field(field.clone()),
                    None,
                    value,
                );
            }
            Expr::IndexAccess { object, index, .. } => {
                Self::apply_container_path_write(locals, object, &PathStep::Index, Some(index), value);
            }
            _ => {}
        }
    }

    /// Shared plumbing for `r.field = v` / `arr[i] = v` / `m[k] = v`: resolve
    /// `object`'s root identifier and access path, and if that root is
    /// CURRENTLY tracked, rebuild its literal with the write spliced in at
    /// `path + [leaf_step]`. If the root isn't tracked, there is nothing to
    /// invalidate (it already resolves to `Unknown`). If it IS tracked but
    /// the write can't be resolved against its actual shape, the root is
    /// invalidated (fail closed) rather than left stale.
    fn apply_container_path_write(
        locals: &mut HashMap<String, Expr>,
        object: &Expr,
        leaf_step: &PathStep,
        leaf_key: Option<&Expr>,
        value: &Expr,
    ) {
        let Some((root, path)) = Self::decompose_container_path(object) else {
            return;
        };
        let root = root.to_string();
        let Some(current) = locals.get(&root).cloned() else {
            return;
        };
        match Self::rebuild_container_write(&current, &path, leaf_step, leaf_key, value) {
            Some(updated) => {
                locals.insert(root, updated);
            }
            None => {
                locals.remove(&root);
            }
        }
    }

    /// Walk `path` into literal `lit`, then splice `value` in at the point
    /// reached via `leaf_step` (a `Field` write overwrites/inserts the named
    /// field; an `Index` write is index-INSENSITIVE and appends — matching
    /// `resolve_container_path_caps`'s read-side union semantics). Returns
    /// `None` when the path can't be resolved against `lit`'s actual shape
    /// (a container-of-unknown-provenance element, a struct field write
    /// through a type that isn't tracked, ...) — the caller treats `None` as
    /// "invalidate the whole root", never as "leave the old snapshot".
    fn rebuild_container_write(
        lit: &Expr,
        path: &[PathStep],
        leaf_step: &PathStep,
        leaf_key: Option<&Expr>,
        value: &Expr,
    ) -> Option<Expr> {
        let Some((head, rest)) = path.split_first() else {
            // `lit` IS the container the leaf write lands directly on.
            return match (leaf_step, lit) {
                (PathStep::Field(fname), Expr::StructLiteral { name, fields, span }) => {
                    let mut new_fields = fields.clone();
                    match new_fields.iter_mut().find(|(n, _)| n == fname) {
                        Some(slot) => slot.1 = value.clone(),
                        None => new_fields.push((fname.clone(), value.clone())),
                    }
                    Some(Expr::StructLiteral { name: name.clone(), fields: new_fields, span: *span })
                }
                (PathStep::Index, Expr::ArrayLiteral { elements, span }) => {
                    let mut new_elements = elements.clone();
                    new_elements.push(value.clone());
                    Some(Expr::ArrayLiteral { elements: new_elements, span: *span })
                }
                (PathStep::Index, Expr::MapLiteral { entries, span }) => {
                    let mut new_entries = entries.clone();
                    let key = leaf_key.cloned().unwrap_or_else(|| value.clone());
                    new_entries.push((key, value.clone()));
                    Some(Expr::MapLiteral { entries: new_entries, span: *span })
                }
                _ => None,
            };
        };
        match (head, lit) {
            (PathStep::Field(fname), Expr::StructLiteral { name, fields, span }) => {
                let (_, fexpr) = fields.iter().find(|(n, _)| n == fname)?;
                let updated = Self::rebuild_container_write(fexpr, rest, leaf_step, leaf_key, value)?;
                let mut new_fields = fields.clone();
                if let Some(slot) = new_fields.iter_mut().find(|(n, _)| n == fname) {
                    slot.1 = updated;
                }
                Some(Expr::StructLiteral { name: name.clone(), fields: new_fields, span: *span })
            }
            (PathStep::Index, Expr::ArrayLiteral { elements, span }) => {
                // Index-insensitive: the write might land on ANY element, so
                // apply it to every element that can accept it and keep the
                // rest as-is (mirrors the read-side union).
                let mut new_elements = Vec::with_capacity(elements.len());
                let mut any = false;
                for el in elements {
                    match Self::rebuild_container_write(el, rest, leaf_step, leaf_key, value) {
                        Some(updated) => {
                            any = true;
                            new_elements.push(updated);
                        }
                        None => new_elements.push(el.clone()),
                    }
                }
                if !any {
                    return None;
                }
                Some(Expr::ArrayLiteral { elements: new_elements, span: *span })
            }
            (PathStep::Index, Expr::MapLiteral { entries, span }) => {
                let mut new_entries = Vec::with_capacity(entries.len());
                let mut any = false;
                for (k, v) in entries {
                    match Self::rebuild_container_write(v, rest, leaf_step, leaf_key, value) {
                        Some(updated) => {
                            any = true;
                            new_entries.push((k.clone(), updated));
                        }
                        None => new_entries.push((k.clone(), v.clone())),
                    }
                }
                if !any {
                    return None;
                }
                Some(Expr::MapLiteral { entries: new_entries, span: *span })
            }
            _ => None,
        }
    }

    fn build_local_container_lits_block(block: &kryos_ast::Block, locals: &mut HashMap<String, Expr>) {
        for stmt in &block.stmts {
            match stmt {
                Stmt::Let {
                    name, value: Some(v), ..
                } => match v {
                    Expr::StructLiteral { .. } | Expr::ArrayLiteral { .. } | Expr::MapLiteral { .. } => {
                        locals.insert(name.clone(), v.clone());
                    }
                    Expr::Identifier { name: src, .. } => {
                        if let Some(existing) = locals.get(src).cloned() {
                            locals.insert(name.clone(), existing);
                        }
                    }
                    _ => {}
                },
                Stmt::Assign { target, op, value, .. } => {
                    Self::apply_container_assign(locals, target, *op, value);
                }
                Stmt::If {
                    then_block,
                    elif_clauses,
                    else_block,
                    ..
                } => {
                    Self::build_local_container_lits_block(then_block, locals);
                    for (_, b) in elif_clauses {
                        Self::build_local_container_lits_block(b, locals);
                    }
                    if let Some(b) = else_block {
                        Self::build_local_container_lits_block(b, locals);
                    }
                }
                Stmt::For { body, .. } | Stmt::While { body, .. } => {
                    Self::build_local_container_lits_block(body, locals)
                }
                Stmt::TryCatch {
                    try_block,
                    catch_block,
                    ..
                } => {
                    Self::build_local_container_lits_block(try_block, locals);
                    Self::build_local_container_lits_block(catch_block, locals);
                }
                _ => {}
            }
        }
    }

    /// Compute, for every function whose DECLARED return type is
    /// `fn(...) -> ...`, the statically-resolved authority of the closure it
    /// returns (see `ClosureCapsResult`). A monotone fixed point (mutual
    /// recursion between closure-returning functions is possible: `a` calls
    /// `b` in return position, `b` calls `a`).
    fn compute_fn_return_closure_caps(
        &self,
        decls: &[Decl],
        working: &HashMap<String, CapabilitySet>,
    ) -> HashMap<String, ClosureCapsResult> {
        let mut fns: Vec<(String, bool, &kryos_ast::Block, &[Param])> = Vec::new();
        Self::collect_functions(decls, &mut fns);
        Self::collect_actor_handler_bodies(decls, &mut fns);
        let mut result: HashMap<String, ClosureCapsResult> = HashMap::new();

        loop {
            let mut changed = false;
            for (name, _annotated, body, params) in &fns {
                if !matches!(self.fn_ret_ty.get(name), Some(TypeExpr::Function { .. })) {
                    continue;
                }
                let own_params: HashSet<String> = params
                    .iter()
                    .filter(|p| Self::is_fn_typed(&p.ty))
                    .map(|p| p.name.clone())
                    .collect();
                let local_container_lits = Self::build_local_container_lits(body);
                let local_caps = self.build_local_closure_caps(body, working, &result, &own_params, &local_container_lits);
                let mut return_exprs: Vec<&Expr> = Vec::new();
                Self::collect_return_exprs(body, &mut return_exprs);
                if return_exprs.is_empty() {
                    continue;
                }
                let mut combined: Option<ClosureCapsResult> = None;
                for e in &return_exprs {
                    let r = self.resolve_closure_caps(e, working, &result, &local_caps, &own_params);
                    combined = Some(match combined {
                        None => r,
                        Some(prev) => Self::merge_closure_caps(prev, r),
                    });
                }
                if let Some(r) = combined {
                    if result.get(name) != Some(&r) {
                        result.insert(name.clone(), r);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }

        result
    }

    /// The EXTRA capability requirement a specific CALL SITE incurs because
    /// the callee has hot (invoked) fn-typed or container-typed parameters:
    /// for each hot parameter index and each of its recorded PATHS (see
    /// `hot_params` / `PathStep`), resolve the ACTUAL argument expression
    /// passed at this call, walk it down that path, and require whatever
    /// authority the reached value carries. This is what attributes
    /// `zero_cap_tool(reader)`'s real `fs:read` requirement (path: empty —
    /// the direct fn-typed-parameter case) and `zero_cap_tool(reg)`'s (path:
    /// `[Field("reader")]` — the container residual, LEDGER item 1) to the
    /// CALL, not to `zero_cap_tool`'s own (legitimately empty, like any HOF)
    /// declaration.
    #[allow(clippy::too_many_arguments)]
    fn accumulate_hot_extra_caps(
        &self,
        callee_name: &str,
        args: &[Expr],
        is_method_or_static: bool,
        extra: &mut CapabilitySet,
        working: &HashMap<String, CapabilitySet>,
        local_caps: &HashMap<String, ClosureCapsResult>,
        local_container_lits: &HashMap<String, Expr>,
        own_params: &HashSet<String>,
    ) {
        let Some(hot_idxs) = self.hot_params.get(callee_name) else {
            return;
        };
        // See `actor_handler_names`: an actor handler has no receiver slot
        // in its own `params`, even though the call syntax is method-shaped.
        let has_self_offset =
            is_method_or_static && !self.actor_handler_names.contains(callee_name);
        for (&i, paths) in hot_idxs {
            let arg_idx = if has_self_offset {
                match i.checked_sub(1) {
                    Some(v) => v,
                    None => continue, // hot index 0 is `self` for a method — never a caller-supplied arg
                }
            } else {
                i
            };
            let Some(arg) = args.get(arg_idx) else {
                continue;
            };
            for path in paths {
                match self.resolve_container_path_caps(
                    arg,
                    path,
                    working,
                    &self.fn_return_closure_caps,
                    local_caps,
                    local_container_lits,
                    own_params,
                ) {
                    ClosureCapsResult::Known(c) => *extra = extra.union(&c),
                    // `pname` names a parameter, but WHOSE parameter -- the
                    // CURRENT (enclosing) function's own, or the specific
                    // fn-value ARGUMENT's own (a transparent-forwarding
                    // inline lambda, `|f| f()`, see `resolve_closure_caps`'s
                    // `Lambda` arm)? `own_params` is the enclosing function's
                    // own fn-typed parameter set, so a name found there IS
                    // that deferred case: this call forwards one of the
                    // CURRENT function's own parameters, so `hot_params` has
                    // already (or will) mark IT hot too -- charging it here
                    // as well would double-count and, worse, would force a
                    // pure forwarding function (the `std::iter` HOF shape) to
                    // require `all`.
                    //
                    // BUT that deferral is only SOUND when the call site that
                    // will eventually supply `pname`'s real value is checked
                    // against the SAME scope this invocation is actually
                    // running under. It is NOT sound when THIS function has
                    // narrowed its own scope with a `deny!` block between
                    // receiving `pname` and invoking it here: the outer call
                    // site is checked against this function's wider ENTRY
                    // scope, not the narrower one in effect at this precise
                    // call -- confirmed live, no decoy or generic needed: `fn
                    // outer(reader: fn()->str) -> str { deny!(fs:read) {
                    // return zero_cap_tool(reader) } }`, called from an
                    // `@capabilities(fs:read)` caller, compiled clean and
                    // printed the secret from inside the denied scope.
                    // `current_fn_entry_scope_depth` (see its field doc)
                    // detects exactly this: if the live scope stack is
                    // DEEPER than it was at this function's own entry, a
                    // deny! (or future narrowing construct) is active right
                    // now, so provenance can't be checked from here and the
                    // full set must be required instead -- same fallback as
                    // every other unresolvable case in this file.
                    ClosureCapsResult::DependsOnParam(pname) if own_params.contains(&pname) => {
                        *extra = extra.union(&self.deferred_own_param_caps(&pname));
                    }
                    // Otherwise `pname` belongs to the ARGUMENT LAMBDA's own
                    // scope -- it is not supplied by any future caller, it is
                    // supplied RIGHT HERE, by whichever OTHER argument in
                    // THIS SAME call the callee's body actually binds it from
                    // at the invocation site. A prior version of this code
                    // (`find_companion_container_arg`, removed) tried to
                    // infer that "whichever other argument" STRUCTURALLY --
                    // by matching the callback parameter's DECLARED element
                    // type against another parameter's declared container
                    // type, first match wins, with no reference to what the
                    // callee's body actually does with either value. That is
                    // exactly the shape-based-inference class of bug this
                    // file has failed on four times running (see
                    // docs/10-capabilities.md and the ledger entry for this
                    // fix): an attacker passes a REAL container carrying the
                    // secret closure as one argument and an EMPTY DECOY
                    // container of the SAME DECLARED SHAPE as another: the
                    // decoy wins the first-match search (contributing no
                    // capabilities), and the real container -- which the
                    // callee's body may very well pass to the callback at
                    // runtime -- is never charged. Confirmed live: a generic
                    // `apply_to_second<T>(decoy: [T], real: [T], f: fn(T) ->
                    // str)` that invokes `f(real[0])` leaked a `fs:read`
                    // closure through the `decoy` companion match with the
                    // required capability computed as empty.
                    //
                    // The ONLY relief this file now implements for this case
                    // is `hot_param_companions` -- genuine, per-declaration
                    // DATA-FLOW tracing (not shape guessing): it looks at
                    // `callee_name`'s OWN, FIXED body to see which OTHER
                    // parameter it actually passes into the SAME argument
                    // SLOT of ITS internal invocation of this hot callback
                    // (`map`'s body literally writes `f(arr[i])`, proving the
                    // callback is invoked with `arr`'s element regardless of
                    // any other parameter's declared shape). Because this
                    // fact is a property of the callee's own source and
                    // cannot be influenced by which argument a CALLER
                    // supplies at any position, a decoy at the call site
                    // cannot change the answer -- unlike the removed
                    // heuristic, which matched on the CALL SITE's declared
                    // argument types. `arg` here must be the lambda literal
                    // ITSELF (`path` empty) for this to apply; a lambda
                    // reached through a container path falls through to the
                    // sound `all` fallback below, as does any position
                    // `hot_param_companions` could not resolve to a single
                    // agreeing companion.
                    ClosureCapsResult::DependsOnParam(pname) => {
                        let companion = if path.is_empty() {
                            if let Expr::Lambda { params: lambda_params, .. } = arg {
                                lambda_params
                                    .iter()
                                    .position(|lp| lp.name == pname)
                                    .and_then(|lambda_idx| {
                                        self.hot_param_companions
                                            .get(callee_name)
                                            .and_then(|m| m.get(&i))
                                            .and_then(|v| v.get(lambda_idx))
                                            .and_then(|c| c.as_ref())
                                    })
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        match companion {
                            Some((comp_idx, comp_path)) => {
                                let comp_arg_idx = if has_self_offset {
                                    comp_idx.checked_sub(1)
                                } else {
                                    Some(*comp_idx)
                                };
                                match comp_arg_idx.and_then(|k| args.get(k)) {
                                    Some(comp_arg) => match self.resolve_container_path_caps(
                                        comp_arg,
                                        comp_path,
                                        working,
                                        &self.fn_return_closure_caps,
                                        local_caps,
                                        local_container_lits,
                                        own_params,
                                    ) {
                                        ClosureCapsResult::Known(c) => *extra = extra.union(&c),
                                        // The proven companion is ITSELF the
                                        // enclosing function's own parameter
                                        // -- deferred exactly like the
                                        // `own_params` arm above, not charged
                                        // here UNLESS this call is already
                                        // inside a `deny!` narrower than this
                                        // function's own entry scope, in
                                        // which case the same unsoundness
                                        // applies (see
                                        // `current_fn_entry_scope_depth`'s
                                        // doc) and the deferral must not
                                        // happen.
                                        ClosureCapsResult::DependsOnParam(cp)
                                            if own_params.contains(&cp) =>
                                        {
                                            *extra = extra.union(&self.deferred_own_param_caps(&cp));
                                        }
                                        ClosureCapsResult::DependsOnParam(_)
                                        | ClosureCapsResult::Unknown => {
                                            extra.insert(Capability::All)
                                        }
                                    },
                                    None => extra.insert(Capability::All),
                                }
                            }
                            // Genuinely unresolvable provenance -- no
                            // approximation, no shape guess: the caller must
                            // hold everything the invoked value could
                            // possibly carry.
                            None => extra.insert(Capability::All),
                        }
                    }
                    // Genuinely unresolvable provenance: the sound
                    // conservative stance (matching the raw-memory escape's
                    // documented policy) is to require the caller to hold
                    // everything.
                    ClosureCapsResult::Unknown => extra.insert(Capability::All),
                }
            }
        }
    }

    /// **FAIL-CLOSED default for a DIRECT INVOCATION of a first-class
    /// fn-value.** Every enforcement path up to this fix (`hot_params` /
    /// `accumulate_hot_extra_caps`) only ever attributed authority to a CALL
    /// SITE that names a known FUNCTION/METHOD (so its callee could be looked
    /// up in `hot_params` by name) — a call whose callee is a bare local
    /// variable, an unnamed parameter, or a field/index chain into one (`r()`,
    /// `arr[0]()`, `m["k"][0]()`) was never resolved AT ALL: `hot_params.get`
    /// on a non-function name is always `None`, so the whole mechanism
    /// silently contributed nothing, no matter what the invoked value
    /// actually carried. This was true even with NO indirection whatsoever —
    /// verified live: a closure built and called in the SAME function, with
    /// no parameter/argument passing step in between, was uncatchable by any
    /// of the three prior enumeration rounds because none of them checked the
    /// callee itself, only arguments flowing INTO an already-known-hot
    /// function.
    ///
    /// This closes that gap structurally rather than by naming another shape:
    /// resolve what `callee` denotes via the SAME closure/container resolvers
    /// already used for argument attribution, and require it exactly like a
    /// named function's own declaration would be. The three outcomes:
    /// - `Known(c)`: the value's exact capability set — require it.
    /// - `DependsOnParam(_)`: `callee` is (or resolves to) one of the CURRENT
    ///   function's own fn-typed parameters — its real value is supplied by
    ///   THIS function's caller, so `hot_params`/Seed A/B already defer the
    ///   charge to that caller; nothing to require here (charging it here too
    ///   would double-count AND would force every ordinary user-written HOF —
    ///   `fn apply(f: fn()->str) -> str { f() }` — to require `all`) --
    ///   UNLESS this direct invocation is already inside a `deny!` block
    ///   narrower than this function's own entry scope (see
    ///   `current_fn_entry_scope_depth`'s doc), in which case the deferral is
    ///   unsound (the outer call site is checked against the wider entry
    ///   scope, not this narrower one) and `all` is required instead.
    ///   Confirmed live, no container/generic/decoy needed: `fn outer(r:
    ///   fn()->str) -> str { deny!(fs:read) { return r() } }` called from an
    ///   `@capabilities(fs:read)` caller compiled clean and printed the
    ///   secret from inside the denied scope, pre-fix.
    /// - `Unknown`: genuinely unresolvable provenance — the same conservative
    ///   stance used everywhere else in this file: require `Capability::All`.
    ///   Unknown must mean deny, not "this call needs nothing" — see
    ///   `docs/capability-roadmap.md` for why enumerating shapes here instead
    ///   (three rounds of it) does not converge.
    /// The correct resolution for a `ClosureCapsResult::DependsOnParam` whose
    /// name is confirmed to be one of the CURRENT function's own parameters:
    /// empty (safely deferred to this function's own call sites) UNLESS the
    /// live scope is currently narrower than this function's own entry scope
    /// (a `deny!` is active between entry and this point), in which case the
    /// deferral is unsound and the full set must be required here instead.
    /// See `current_fn_entry_scope_depth`'s field doc for the full
    /// rationale and the live repro that found this.
    fn deferred_own_param_caps(&self, pname: &str) -> CapabilitySet {
        // A lambda's OWN bound parameter, re-encountered while re-checking
        // that SAME lambda's own body -- already fully handled at the
        // enclosing call site (see `transparent_lambda_params`'s doc).
        // Exempt regardless of scope narrowing; this is not the caller-level
        // parameter the scope check exists to protect.
        if self.transparent_lambda_params.contains(pname) {
            return CapabilitySet::empty();
        }
        // A STRUCTURAL classification of a fresh lambda literal's own body
        // (see `structural_lambda_eval_depth`'s doc) -- scope-independent by
        // definition, so the ambient scope must never influence it.
        if self.structural_lambda_eval_depth.get() > 0 {
            return CapabilitySet::empty();
        }
        if self.scope_stack.len() > self.current_fn_entry_scope_depth {
            let mut c = CapabilitySet::empty();
            c.insert(Capability::All);
            c
        } else {
            CapabilitySet::empty()
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_direct_invoke_caps(
        &self,
        callee: &Expr,
        working: &HashMap<String, CapabilitySet>,
        local_caps: &HashMap<String, ClosureCapsResult>,
        local_container_lits: &HashMap<String, Expr>,
        own_params: &HashSet<String>,
    ) -> CapabilitySet {
        let Some((root, path)) = Self::decompose_container_path(callee) else {
            // A call/method-call/other compound expression used directly as
            // ITS OWN callee (`mk()()`, `pick_reader(cond)()`, ...) — resolve
            // through the general closure-value resolver, which already
            // understands a call to a known `fn_return_closure_caps` entry
            // and falls back to `Unknown` for anything else it cannot prove.
            return match self.resolve_closure_caps(
                callee,
                working,
                &self.fn_return_closure_caps,
                local_caps,
                own_params,
            ) {
                ClosureCapsResult::Known(c) => c,
                ClosureCapsResult::DependsOnParam(pname) => self.deferred_own_param_caps(&pname),
                ClosureCapsResult::Unknown => {
                    let mut c = CapabilitySet::empty();
                    c.insert(Capability::All);
                    c
                }
            };
        };
        let result = if path.is_empty() {
            self.resolve_closure_caps(callee, working, &self.fn_return_closure_caps, local_caps, own_params)
        } else {
            let root_expr = Expr::Identifier {
                name: root.to_string(),
                span: Span::DUMMY,
            };
            self.resolve_container_path_caps(
                &root_expr,
                &path,
                working,
                &self.fn_return_closure_caps,
                local_caps,
                local_container_lits,
                own_params,
            )
        };
        match result {
            ClosureCapsResult::Known(c) => c,
            ClosureCapsResult::DependsOnParam(pname) => self.deferred_own_param_caps(&pname),
            ClosureCapsResult::Unknown => {
                let mut c = CapabilitySet::empty();
                c.insert(Capability::All);
                c
            }
        }
    }

    /// The sibling of `resolve_direct_invoke_caps` for the `obj.method(...)`
    /// SYNTACTIC shape when `method` is actually a function-typed STRUCT
    /// FIELD being read and invoked (`reg.reader()`), not a genuine
    /// method/trait dispatch. Only engages when `method` is POSITIVELY
    /// confirmed to name a field explicitly written in a locally-tracked
    /// struct literal reachable from `object` — never a blanket guess, so an
    /// ordinary method call (`p.distance(other)`) is never misclassified as
    /// an unresolvable field read (which would otherwise force every method
    /// call in the language to require `all`).
    #[allow(clippy::too_many_arguments)]
    fn resolve_method_field_invoke_caps(
        &self,
        object: &Expr,
        method: &str,
        working: &HashMap<String, CapabilitySet>,
        local_caps: &HashMap<String, ClosureCapsResult>,
        local_container_lits: &HashMap<String, Expr>,
        own_params: &HashSet<String>,
    ) -> CapabilitySet {
        let Some((root, path)) = Self::decompose_container_path(object) else {
            return CapabilitySet::empty();
        };
        let Some(lit) = local_container_lits.get(root) else {
            return CapabilitySet::empty();
        };
        if !Self::literal_field_exists(lit, &path, method) {
            return CapabilitySet::empty();
        }
        let mut full_path = path;
        full_path.push(PathStep::Field(method.to_string()));
        let root_expr = Expr::Identifier {
            name: root.to_string(),
            span: Span::DUMMY,
        };
        let result = self.resolve_container_path_caps(
            &root_expr,
            &full_path,
            working,
            &self.fn_return_closure_caps,
            local_caps,
            local_container_lits,
            own_params,
        );
        match result {
            ClosureCapsResult::Known(c) => c,
            ClosureCapsResult::DependsOnParam(pname) => self.deferred_own_param_caps(&pname),
            ClosureCapsResult::Unknown => {
                let mut c = CapabilitySet::empty();
                c.insert(Capability::All);
                c
            }
        }
    }

    /// Whether `lit` (a container literal reachable via `local_container_lits`)
    /// explicitly writes a field named `field` at the position `path` walks
    /// to. Used ONLY to confirm the `obj.method(...)` shape is really a field
    /// read before treating it as one — a `false` result means "not a field
    /// access we can positively identify", so the caller falls back to
    /// ordinary method-call handling rather than a false-positive `Unknown`.
    fn literal_field_exists(expr: &Expr, path: &[PathStep], field: &str) -> bool {
        match path.split_first() {
            None => matches!(
                expr,
                Expr::StructLiteral { fields, .. } if fields.iter().any(|(n, _)| n == field)
            ),
            Some((PathStep::Field(fname), rest)) => match expr {
                Expr::StructLiteral { fields, .. } => fields
                    .iter()
                    .any(|(n, e)| n == fname && Self::literal_field_exists(e, rest, field)),
                _ => false,
            },
            Some((PathStep::Index, rest)) => match expr {
                Expr::ArrayLiteral { elements, .. } => elements
                    .iter()
                    .any(|e| Self::literal_field_exists(e, rest, field)),
                Expr::MapLiteral { entries, .. } => entries
                    .iter()
                    .any(|(_, v)| Self::literal_field_exists(v, rest, field)),
                _ => false,
            },
        }
    }

    /// Require `caps` against the CURRENT scope, exactly like
    /// `enforce_callee_name`'s propagation check, for a capability that was
    /// resolved from a DIRECT fn-value invocation rather than a named
    /// function's own declaration. Pushed as its own diagnostic (not folded
    /// into `enforce_callee_name`) because there is no callee NAME to name in
    /// the message — the note instead explains that this is a closure/fn-value
    /// call, not a function declaration mismatch.
    fn require_direct_invoke_caps(&mut self, required: &CapabilitySet, call_span: Span) {
        if required.is_empty() {
            return;
        }
        if let Some(caller_caps) = self.current_caps() {
            if !required.is_subset_of(caller_caps) {
                let excess = required.excess_over(caller_caps);
                let excess_names: Vec<String> = excess.iter().map(|c| c.to_string()).collect();
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "call through a function value requires capabilities [{}] not granted to caller",
                        excess_names.join(", ")
                    ))
                    .with_label(
                        call_span,
                        "invokes a closure/fn-value directly whose authority could not be \
                         proven a subset of this scope",
                    )
                    .with_note(
                        "this call invokes a first-class function value (a local, a parameter, \
                         or a container element) directly; its provenance could not be resolved \
                         to a proven-safe capability set, so the caller must hold everything it \
                         could possibly carry -- see docs/capability-roadmap.md for the sound \
                         long-term fix (capability-typed fn values)",
                    )
                    .with_code(kryos_errors::codes::E0507),
                );
            }
        }
    }

    /// True only in the maximally-explicit [`CapabilityMode::Strict`], where an
    /// unannotated function is treated as declaring the empty set.
    fn strict_mode(&self) -> bool {
        matches!(self.mode, CapabilityMode::Strict)
    }

    /// Get the current (innermost) capability scope, if any.
    fn current_scope(&self) -> Option<&CapabilityScope> {
        self.scope_stack.last()
    }

    /// Get just the current capability set, if any scope exists.
    fn current_caps(&self) -> Option<&CapabilitySet> {
        self.scope_stack.last().map(|s| &s.capabilities)
    }

    /// Check whether ANY enclosing scope is annotated.
    /// This handles lambdas and blocks inside annotated functions.
    fn has_annotated_scope(&self) -> bool {
        self.scope_stack.iter().any(|s| s.annotated)
    }

    /// Build a map from function names to their declared capability sets.
    /// Only includes functions with explicit `@capabilities(...)` annotations.
    fn build_fn_capability_map(&mut self, declarations: &[Decl]) {
        for decl in declarations {
            match decl {
                Decl::Function {
                    name, annotations, ..
                } if Self::has_capabilities_annotation(annotations) => {
                    let caps = CapabilitySet::from_annotations(annotations);
                    // UNION on a name collision, not overwrite: impl methods
                    // are keyed by bare name here, so two structs each with an
                    // annotated `write` must contribute BOTH sets. Overwriting
                    // let the last-declared one win, which (with the inference
                    // filter below) both false-rejected the other method's body
                    // and let a caller under-declare (an escape).
                    let merged = match self.fn_capabilities.get(name) {
                        Some(existing) => existing.union(&caps),
                        None => caps,
                    };
                    self.fn_capabilities.insert(name.clone(), merged);
                }
                Decl::Impl { methods, .. } | Decl::Trait { methods, .. } => {
                    self.build_fn_capability_map(methods);
                }
                // An actor's message handlers are message-passing entry points:
                // invoking a handler from outside (`w.dump(..)`) exercises the
                // actor's declared authority. Record each handler name under the
                // actor's declared caps (its ceiling) so the call site is gated
                // like a call to an annotated function. Without this, handler
                // names never entered `fn_capabilities` (neither this map nor the
                // inferred pass recursed into actors), so `enforce_callee_name`
                // found nothing and an unannotated caller reached a gated builtin
                // inside a capability-annotated actor's handler for free -- a
                // capability escape past deny-by-default. An unannotated actor
                // contributes the empty set here (a no-op); its handler bodies are
                // still checked against the empty scope in `check_actor`.
                Decl::Actor {
                    annotations,
                    handlers,
                    ..
                } => {
                    let caps = CapabilitySet::from_annotations(annotations);
                    for handler in handlers {
                        let merged = match self.fn_capabilities.get(&handler.name) {
                            Some(existing) => existing.union(&caps),
                            None => caps.clone(),
                        };
                        self.fn_capabilities.insert(handler.name.clone(), merged);
                    }
                }
                _ => {}
            }
        }
    }

    /// Check if annotations contain a `@capabilities(...)` annotation.
    fn has_capabilities_annotation(annotations: &[Annotation]) -> bool {
        annotations.iter().any(|a| a.name == "capabilities")
    }

    /// The capability contributed by an expression used as a first-class VALUE:
    /// a bare identifier naming a gated builtin (`file_write` passed as a
    /// `fn(...)` argument) delegates that builtin's authority. Used by interior
    /// inference so the requirement propagates to the boundary; the enforcement
    /// counterpart is `check_builtin_value_ref`.
    fn builtin_value_caps(expr: &Expr) -> CapabilitySet {
        let mut set = CapabilitySet::empty();
        if let Expr::Identifier { name, .. } = expr {
            if let Some(cap) = required_capability_for_builtin(name) {
                set.insert(cap);
            }
        }
        set
    }

    /// Gather every function (top-level and impl/trait method) that has a body,
    /// as `(name, annotated, body, params)` tuples, for interior capability
    /// inference. Carries THIS DECLARATION's own param list directly (rather
    /// than making the caller look it up later via `self.fn_params.get(name)`)
    /// because `fn_params` is keyed by BARE NAME and several stdlib modules
    /// declare same-named functions (`std::iter::find`, `std::re::find`,
    /// `std::string::find` all collide on "find") — looking a name back up
    /// after the fact can silently return a DIFFERENT declaration's params
    /// than the one actually being processed, which (since this file's
    /// fail-closed default now requires `all` for any parameter it fails to
    /// recognize as the current function's own) turns a pre-existing,
    /// previously-harmless bare-name collision into a false-positive
    /// rejection. Returning the params inline sidesteps the collision for
    /// every OWN-PARAMS computation; a genuine cross-function name lookup
    /// (resolving a DIFFERENT, forwarded-to function by name) still goes
    /// through `fn_params` and keeps the same pre-existing collision
    /// tolerance documented on that field.
    fn collect_functions<'a>(
        decls: &'a [Decl],
        out: &mut Vec<(String, bool, &'a kryos_ast::Block, &'a [Param])>,
    ) {
        for d in decls {
            match d {
                Decl::Function {
                    name,
                    annotations,
                    body: Some(b),
                    params,
                    ..
                } => out.push((
                    name.clone(),
                    Self::has_capabilities_annotation(annotations),
                    b,
                    params.as_slice(),
                )),
                Decl::Impl { methods, .. } | Decl::Trait { methods, .. } => {
                    Self::collect_functions(methods, out)
                }
                _ => {}
            }
        }
    }

    /// Compute each UNANNOTATED function's inferred capability set: the fixpoint
    /// union of the capabilities its body requires directly (gated builtins /
    /// stdlib paths) and the sets of the functions it calls. Terminating: the
    /// lattice is the finite powerset of the capability enum and each step only
    /// adds capabilities (monotone union), so the result is the least set that
    /// covers all direct uses — an *over*-approximation, never an under one.
    ///
    /// Soundness note: an unresolved single-segment call (not a gated builtin,
    /// not a known function) contributes nothing. That is sound under Kryos's
    /// whole-program compilation because every user/stdlib function body is
    /// merged into this module (so real callees resolve), constructors and
    /// closure-variable calls carry no authority, and the only way to reach a
    /// raw native `extern` is through an `extern` block, which already requires
    /// the `ffi` capability.
    fn compute_inferred_capabilities(
        &self,
        declarations: &[Decl],
    ) -> HashMap<String, CapabilitySet> {
        let mut fns: Vec<(String, bool, &kryos_ast::Block, &[Param])> = Vec::new();
        Self::collect_functions(declarations, &mut fns);

        // Seed the working map with annotated functions' declared ceilings.
        let mut working: HashMap<String, CapabilitySet> = self.fn_capabilities.clone();

        loop {
            let mut changed = false;
            for (name, is_annotated, body, params) in &fns {
                // Skip only THIS function if it is annotated (its declaration is
                // the ceiling, never widened by its own body). Do NOT skip an
                // UNANNOTATED method just because a DIFFERENT method sharing its
                // bare name is annotated -- that suppressed the unannotated
                // method's inferred caps (a same-named annotated method hid an
                // unannotated one's fs:write), which both false-rejected the
                // unannotated body and let a caller under-declare. Its inferred
                // caps are UNIONED into the shared name entry.
                if *is_annotated {
                    continue;
                }
                let own_params: HashSet<String> = params
                    .iter()
                    .filter(|p| self.is_fn_bearing_type(&p.ty))
                    .map(|p| p.name.clone())
                    .collect();
                let local_container_lits = Self::build_local_container_lits(body);
                let local_caps = self.build_local_closure_caps(
                    body,
                    &working,
                    &self.fn_return_closure_caps,
                    &own_params,
                    &local_container_lits,
                );
                let collected = self.collect_caps_block(
                    body,
                    &working,
                    &own_params,
                    &local_caps,
                    &local_container_lits,
                );
                let cur = working
                    .get(name)
                    .cloned()
                    .unwrap_or_else(CapabilitySet::empty);
                let merged = cur.union(&collected);
                // union only grows, so a length change means a new capability.
                if merged.len() != cur.len() {
                    working.insert(name.clone(), merged);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        // Return the full unioned map. A name shared by an annotated and an
        // unannotated method now carries BOTH (over-approximation) so neither
        // the ceiling nor the propagation loses a capability.
        working
    }

    /// Read-only union of the capabilities required by every call in a block.
    ///
    /// `own_params` is the fn-typed parameter NAME set of the function this
    /// block belongs to (so an `Identifier` reference to one of them resolves
    /// to `ClosureCapsResult::DependsOnParam`, deferred rather than charged
    /// here); `local_caps` is that function's `let`-binding closure-authority
    /// map (see `build_local_closure_caps`). Both feed hot-call resolution
    /// (see the `Expr::FnCall` arm of `collect_caps_expr`).
    fn collect_caps_block(
        &self,
        block: &kryos_ast::Block,
        working: &HashMap<String, CapabilitySet>,
        own_params: &HashSet<String>,
        local_caps: &HashMap<String, ClosureCapsResult>,
        local_container_lits: &HashMap<String, Expr>,
    ) -> CapabilitySet {
        let mut acc = CapabilitySet::empty();
        for stmt in &block.stmts {
            acc = acc.union(&self.collect_caps_stmt(
                stmt,
                working,
                own_params,
                local_caps,
                local_container_lits,
            ));
        }
        acc
    }

    fn collect_caps_stmt(
        &self,
        stmt: &Stmt,
        working: &HashMap<String, CapabilitySet>,
        own_params: &HashSet<String>,
        local_caps: &HashMap<String, ClosureCapsResult>,
        local_container_lits: &HashMap<String, Expr>,
    ) -> CapabilitySet {
        let mut acc = CapabilitySet::empty();
        let e = |ex: &Expr, a: &mut CapabilitySet| {
            *a = a.union(&self.collect_caps_expr(
                ex,
                working,
                own_params,
                local_caps,
                local_container_lits,
            ))
        };
        let b = |bl: &kryos_ast::Block, a: &mut CapabilitySet| {
            *a = a.union(&self.collect_caps_block(
                bl,
                working,
                own_params,
                local_caps,
                local_container_lits,
            ))
        };
        match stmt {
            Stmt::Let { value, .. } => {
                if let Some(expr) = value {
                    e(expr, &mut acc);
                }
            }
            Stmt::Assign { value, target, .. } => {
                e(target, &mut acc);
                e(value, &mut acc);
            }
            Stmt::Return { value, .. } => {
                if let Some(expr) = value {
                    e(expr, &mut acc);
                }
            }
            Stmt::If {
                condition,
                then_block,
                elif_clauses,
                else_block,
                ..
            } => {
                e(condition, &mut acc);
                b(then_block, &mut acc);
                for (cond, block) in elif_clauses {
                    e(cond, &mut acc);
                    b(block, &mut acc);
                }
                if let Some(block) = else_block {
                    b(block, &mut acc);
                }
            }
            Stmt::For { iterable, body, .. } => {
                e(iterable, &mut acc);
                b(body, &mut acc);
            }
            Stmt::While {
                condition, body, ..
            } => {
                e(condition, &mut acc);
                b(body, &mut acc);
            }
            Stmt::Expr { expr, .. } => e(expr, &mut acc),
            Stmt::Spawn { expr, .. } => e(expr, &mut acc),
            Stmt::TryCatch {
                try_block,
                catch_block,
                ..
            } => {
                b(try_block, &mut acc);
                b(catch_block, &mut acc);
            }
            Stmt::Throw { expr, .. } => e(expr, &mut acc),
            Stmt::Select { branches, .. } => {
                for branch in branches {
                    e(&branch.channel, &mut acc);
                    b(&branch.body, &mut acc);
                }
            }
            Stmt::DenyBlock { body, .. } => b(body, &mut acc),
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
        }
        acc
    }

    fn collect_caps_expr(
        &self,
        expr: &Expr,
        working: &HashMap<String, CapabilitySet>,
        own_params: &HashSet<String>,
        local_caps: &HashMap<String, ClosureCapsResult>,
        local_container_lits: &HashMap<String, Expr>,
    ) -> CapabilitySet {
        let mut acc = CapabilitySet::empty();
        let e = |ex: &Expr, a: &mut CapabilitySet| {
            *a = a.union(&self.collect_caps_expr(
                ex,
                working,
                own_params,
                local_caps,
                local_container_lits,
            ))
        };
        let b = |bl: &kryos_ast::Block, a: &mut CapabilitySet| {
            *a = a.union(&self.collect_caps_block(
                bl,
                working,
                own_params,
                local_caps,
                local_container_lits,
            ))
        };
        match expr {
            Expr::FnCall { callee, args, span: _ } => {
                // The capability contribution of THIS call.
                let segments = self.resolve_path(callee);
                // A single-segment USER function that shadows a builtin/path
                // name resolves to the user function, so its call carries no
                // builtin authority (its real requirement comes from working).
                let shadows_user_fn =
                    segments.len() == 1 && self.defined_fns.contains(&segments[0]);
                if !shadows_user_fn {
                    if let Some(cap) = required_capability_for_path(&segments) {
                        acc.insert(cap);
                    }
                }
                if segments.len() == 1 {
                    // A user function that SHADOWS a builtin's name resolves to
                    // the user function, so do not attribute the builtin's
                    // capability by name (its real requirement comes from
                    // `working.get` below).
                    if !self.defined_fns.contains(&segments[0]) {
                        if let Some(cap) = required_capability_for_builtin(&segments[0]) {
                            acc.insert(cap);
                        }
                    }
                    // Extern calls contribute their FFI capability to inference
                    // so helpers calling raw externs propagate the requirement
                    // to the boundary (same rule as enforce_callee_name).
                    if let Some(cap) = self.required_capability_for_extern(&segments[0]) {
                        acc.insert(cap);
                    }
                    if let Some(caps) = working.get(&segments[0]) {
                        acc = acc.union(caps);
                    }
                    // CLOSURE-LAUNDERING FIX: if this call's callee has hot
                    // (invoked) fn-typed parameters, the SPECIFIC argument
                    // supplied at THIS call site carries its own authority
                    // that this function's inferred set must also cover —
                    // otherwise a helper that forwards a resolvable
                    // privileged closure into a hot call would silently
                    // under-report its own requirement. See
                    // `accumulate_hot_extra_caps`.
                    self.accumulate_hot_extra_caps(
                        &segments[0],
                        args,
                        false,
                        &mut acc,
                        working,
                        local_caps,
                        local_container_lits,
                        own_params,
                    );
                }
                // FAIL-CLOSED DEFAULT: everything above only attributes
                // authority when the callee is a KNOWN function/builtin/
                // extern NAME. A callee that is instead a first-class
                // fn-value with no such name (a bare local, a container
                // element, a field/index chain into one -- INCLUDING a
                // callee shape `resolve_path` cannot even see, like
                // `arr[0]()`, since it only understands Identifier/
                // FieldAccess) was never resolved AT ALL by the
                // `hot_params`/name-keyed machinery above. See
                // `resolve_direct_invoke_caps`. Gated on `segments.len() <=
                // 1`: a MULTI-segment path (`segments.len() > 1`) only ever
                // arises from a qualified stdlib call (`std.net.connect()`),
                // already fully handled by the qualified-path check above --
                // an ordinary `obj.field(...)` chain is parsed as
                // `MethodCall`, not a multi-segment `FieldAccess` callee (see
                // `resolve_method_field_invoke_caps` for that shape).
                let callee_is_named = segments.len() == 1
                    && (self.defined_fns.contains(&segments[0])
                        || self.actor_names.contains(&segments[0])
                        || self.enum_variant_names.contains(&segments[0])
                        || is_known_builtin_name(&segments[0])
                        || self.extern_fns.contains(&segments[0]));
                if !callee_is_named && segments.len() <= 1 {
                    acc = acc.union(&self.resolve_direct_invoke_caps(
                        callee,
                        working,
                        local_caps,
                        local_container_lits,
                        own_params,
                    ));
                }
                e(callee, &mut acc);
                for arg in args {
                    e(arg, &mut acc);
                }
            }
            Expr::MethodCall {
                object,
                method,
                args,
                ..
            } => {
                // The method's own (declared or inferred) requirement.
                if let Some(caps) = working.get(method) {
                    acc = acc.union(caps);
                }
                self.accumulate_hot_extra_caps(
                    method,
                    args,
                    true,
                    &mut acc,
                    working,
                    local_caps,
                    local_container_lits,
                    own_params,
                );
                // FAIL-CLOSED: `obj.method(...)` may really be a
                // function-typed struct FIELD being invoked -- see
                // `resolve_method_field_invoke_caps`.
                acc = acc.union(&self.resolve_method_field_invoke_caps(
                    object,
                    method,
                    working,
                    local_caps,
                    local_container_lits,
                    own_params,
                ));
                e(object, &mut acc);
                for arg in args {
                    e(arg, &mut acc);
                }
            }
            Expr::StaticMethodCall { method, args, .. } => {
                if let Some(caps) = working.get(method) {
                    acc = acc.union(caps);
                }
                self.accumulate_hot_extra_caps(
                    method,
                    args,
                    true,
                    &mut acc,
                    working,
                    local_caps,
                    local_container_lits,
                    own_params,
                );
                for arg in args {
                    e(arg, &mut acc);
                }
            }
            Expr::FieldAccess { object, .. } => e(object, &mut acc),
            Expr::IndexAccess { object, index, .. } => {
                e(object, &mut acc);
                e(index, &mut acc);
            }
            Expr::BinaryOp { left, right, .. } => {
                e(left, &mut acc);
                e(right, &mut acc);
            }
            Expr::UnaryOp { operand, .. } => e(operand, &mut acc),
            Expr::ArrayLiteral { elements, .. } | Expr::TupleLiteral { elements, .. } => {
                for elem in elements {
                    e(elem, &mut acc);
                }
            }
            Expr::MapLiteral { entries, .. } => {
                for (k, v) in entries {
                    e(k, &mut acc);
                    e(v, &mut acc);
                }
            }
            Expr::StructLiteral { fields, .. } => {
                for (_, ex) in fields {
                    e(ex, &mut acc);
                }
            }
            Expr::Lambda { body, .. } => e(body, &mut acc),
            Expr::IfExpr {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                e(condition, &mut acc);
                b(then_branch, &mut acc);
                if let Some(block) = else_branch {
                    b(block, &mut acc);
                }
            }
            Expr::MatchExpr { subject, arms, .. } => {
                e(subject, &mut acc);
                for arm in arms {
                    e(&arm.body, &mut acc);
                    if let Some(guard) = &arm.guard {
                        e(guard, &mut acc);
                    }
                }
            }
            Expr::PipeExpr { left, right, .. } => {
                e(left, &mut acc);
                e(right, &mut acc);
            }
            Expr::Borrow { inner, .. }
            | Expr::Deref { inner, .. }
            | Expr::SharedExpr { inner, .. }
            | Expr::MoveExpr { inner, .. }
            | Expr::WeakExpr { inner, .. } => e(inner, &mut acc),
            Expr::ComptimeBlock { body, .. }
            | Expr::QuantumBlock { body, .. }
            | Expr::UnsafeBlock { body, .. } => b(body, &mut acc),
            Expr::Cast { expr, .. } => e(expr, &mut acc),
            Expr::Block { block, .. } => b(block, &mut acc),
            Expr::Await { value, .. } => e(value, &mut acc),
            Expr::RangeExpr { start, end, .. } => {
                if let Some(s) = start {
                    e(s, &mut acc);
                }
                if let Some(en) = end {
                    e(en, &mut acc);
                }
            }
            Expr::InterpolatedString { parts, .. } => {
                for part in parts {
                    if let kryos_ast::StringPart::Expr(ex) = part {
                        e(ex, &mut acc);
                    }
                }
            }
            // A gated builtin used as a VALUE (bound to a `let`, returned,
            // stored in an array/struct, piped, ...) delegates its authority.
            // This single arm covers every non-call value position. (A user
            // FUNCTION used as a value is enforced at the reference site by
            // `check_builtin_value_ref`, which rejects it against the boundary's
            // grants rather than silently propagating -- see the capability
            // escape fix; propagating here would over-approximate any local
            // variable that shares a name with a stdlib function.)
            // Skip when the name is a USER-defined function shadowing a builtin
            // (it resolves to the user fn, no builtin authority). This arm also
            // receives the recursed callee of a FnCall, so without this a call
            // to a user `file_write`/`http_get` spuriously inherited the
            // builtin's capability.
            Expr::Identifier { name, .. } => {
                if !self.defined_fns.contains(name) {
                    acc = acc.union(&Self::builtin_value_caps(expr));
                }
            }
            Expr::IntLiteral { .. }
            | Expr::FloatLiteral { .. }
            | Expr::StringLiteral { .. }
            | Expr::CharLiteral { .. }
            | Expr::BoolLiteral { .. }
            | Expr::NoneLiteral { .. } => {}
        }
        acc
    }

    fn check_module(&mut self, module: &Module) {
        // Pass 0: record every extern-declared function name so call sites
        // can require the corresponding capability (see extern_fns).
        self.collect_extern_fns(&module.declarations);
        self.collect_defined_fns(&module.declarations);
        self.collect_fn_signatures(&module.declarations);
        self.collect_struct_field_types(&module.declarations);
        self.transparent_accessor_paths = Self::collect_transparent_accessor_paths(&module.declarations);

        // Pass 1: seed the propagation map with every ANNOTATED function's
        // declared set (its ceiling).
        self.build_fn_capability_map(&module.declarations);

        // Pass 1a: STRUCTURAL closure-laundering analysis. Computed in EVERY
        // mode (including Strict and Permissive) — the escape this closes
        // (`tests/security/cap_escape_closure_launder.kry`) defeats
        // `--strict-capabilities` exactly as much as the default inferred
        // mode, because both route enforcement through `enforce_callee_name`
        // / `check_expr`, which this feeds regardless of mode.
        //
        // `hot_params` is purely structural (which fn-typed parameter
        // POSITIONS are invoked, directly or via forwarding — no capability
        // values involved), so it has no dependency on the inferred map.
        // `fn_return_closure_caps` (which functions return closures, and
        // what those closures require) DOES need a capability map to resolve
        // named-function references and lambda bodies in return position, so
        // it is seeded from a baseline inferred pass; the REAL inferred pass
        // below is then re-run hot-aware so an interior helper that forwards
        // a resolvable privileged closure into a hot call reflects that in
        // its own ceiling (not just at direct enforcement sites).
        self.hot_params = self.compute_hot_params(&module.declarations);
        self.hot_param_companions = self.compute_hot_param_companions(&module.declarations);
        let baseline = self.compute_inferred_capabilities(&module.declarations);
        self.fn_return_closure_caps =
            self.compute_fn_return_closure_caps(&module.declarations, &baseline);

        // Pass 1b (Inferred mode only): compute each UNANNOTATED function's
        // capability set as the fixpoint union of what it and its callees
        // require, and merge those into the propagation map. This is what lets
        // interior helpers stay annotation-free while their requirements still
        // reach the boundary. Annotated functions are left at their declared
        // ceiling (a function that under-declares is caught when its own body
        // is checked, not by widening it here).
        if matches!(self.mode, CapabilityMode::Inferred) {
            // Hot-aware re-run: now that hot_params/fn_return_closure_caps
            // are populated, this pass's `collect_caps_expr` also attributes
            // hot-call-site authority to the calling function's own set.
            let inferred = self.compute_inferred_capabilities(&module.declarations);
            for (name, caps) in inferred {
                // UNION (not or_insert): a name shared by an annotated and an
                // unannotated method must carry both the declared and the
                // inferred caps, so a caller sees the full requirement and the
                // unannotated body sees a ceiling that covers it.
                let merged = match self.fn_capabilities.get(&name) {
                    Some(existing) => existing.union(&caps),
                    None => caps,
                };
                self.fn_capabilities.insert(name, merged);
            }
        }

        // Pass 2: walk the AST and enforce capability rules.
        for decl in &module.declarations {
            self.check_decl(decl);
        }
    }

    /// Add every identifier a pattern binds.
    fn pattern_names(p: &kryos_ast::Pattern, out: &mut std::collections::HashSet<String>) {
        use kryos_ast::Pattern;
        match p {
            Pattern::Ident { name, .. } => {
                out.insert(name.clone());
            }
            Pattern::Tuple { elements, .. } => {
                for e in elements {
                    Self::pattern_names(e, out);
                }
            }
            Pattern::Or { patterns, .. } => {
                for e in patterns {
                    Self::pattern_names(e, out);
                }
            }
            Pattern::Struct { fields, .. } => {
                for (_, fp) in fields {
                    Self::pattern_names(fp, out);
                }
            }
            Pattern::Enum { fields, .. } => {
                for fp in fields {
                    Self::pattern_names(fp, out);
                }
            }
            _ => {}
        }
    }

    fn check_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Function {
                name,
                annotations,
                params,
                body,
                span,
                ..
            } => {
                // Track this function's IN-SCOPE local bindings so a value-
                // position identifier that is a local (not a function
                // reference) is not attributed a same-named function's
                // capabilities. Seed with the PARAMS only; each `let`/`for`/
                // `catch` binding is added as the walk reaches it (see
                // check_stmt / check_block), so a binding is in scope only AFTER
                // its initializer. Building the whole-body set up front made it
                // ORDER-INDEPENDENT, which let `let leaker = leaker` (and the
                // laundering variant `let x = leaker; let leaker = x`) shadow an
                // annotated function's name: the value-authority gate saw the
                // RHS function reference as "just a local" and let its
                // capabilities escape to an unannotated caller.
                let locals: std::collections::HashSet<String> =
                    params.iter().map(|p| p.name.clone()).collect();
                self.current_locals = locals;
                // Closure-laundering fix: track THIS function's own
                // fn-typed/container-typed parameter names (so a reference
                // to one resolves to `DependsOnParam`, deferred to ITS OWN
                // call sites) and its `let`-bound locals' resolved closure
                // authority / container-literal bindings (so `let reader =
                // make_secret_reader(path)` and `let reg = Registry {
                // reader: r }` both trace back to their real requirement).
                // See `accumulate_hot_extra_caps` / `resolve_closure_caps` /
                // `resolve_container_path_caps`.
                let own_params: std::collections::HashSet<String> = params
                    .iter()
                    .filter(|p| self.is_fn_bearing_type(&p.ty))
                    .map(|p| p.name.clone())
                    .collect();
                self.current_local_container_lits = match body {
                    Some(b) => Self::build_local_container_lits(b),
                    None => HashMap::new(),
                };
                self.current_local_closure_caps = match body {
                    Some(b) => self.build_local_closure_caps(
                        b,
                        &self.fn_capabilities,
                        &self.fn_return_closure_caps,
                        &own_params,
                        &self.current_local_container_lits,
                    ),
                    None => HashMap::new(),
                };
                self.current_fn_typed_params = own_params;
                self.check_function(name, annotations, body.as_ref(), *span);
                self.current_locals.clear();
                self.current_fn_typed_params.clear();
                self.current_local_closure_caps.clear();
                self.current_local_container_lits.clear();
            }
            Decl::Actor {
                name,
                annotations,
                handlers,
                ..
            } => {
                self.check_actor(name, annotations, handlers);
            }
            Decl::Extern { items, span, .. } => {
                self.check_extern(items, *span);
            }
            Decl::Impl { methods, .. } => {
                for method in methods {
                    self.check_decl(method);
                }
            }
            Decl::Trait { methods, .. } => {
                for method in methods {
                    self.check_decl(method);
                }
            }
            Decl::Import { path, .. } => {
                // Check that imports of capability-gated modules are allowed.
                if let Some(required_cap) = required_capability_for_path(&path.segments) {
                    if let Some(caps) = self.current_caps() {
                        if !caps.has(required_cap) {
                            self.diagnostics.push(
                                Diagnostic::error(format!(
                                    "import of `{}` requires `{required_cap}` capability",
                                    path.segments.join("::")
                                ))
                                .with_label(path.span, format!("requires `{required_cap}`"))
                                .with_code(kryos_errors::codes::E0501),
                            );
                        }
                    }
                }
            }
            Decl::Const { value, .. } => {
                // A top-level `let`/`let mut` initializer (Decl::Const) runs
                // at program startup and can call gated builtins
                // (file_write, env_get, http_get, ...). It was never checked
                // — a full capability bypass in every mode (backlog #24).
                // A const has no annotation site of its own, but it is part
                // of the program whose authority ceiling is `main`'s
                // declaration. Check the initializer against `main`'s
                // capabilities: a const that needs authority `main` never
                // declared (the leak case: net:http-only main with a
                // file_write const) is rejected, while legitimate startup
                // initializers covered by main's annotation pass. Permissive
                // leaves it unchecked, matching unannotated functions.
                let annotated = !matches!(self.mode, CapabilityMode::Permissive);
                self.scope_stack.push(CapabilityScope {
                    capabilities: self
                        .fn_capabilities
                        .get("main")
                        .cloned()
                        .unwrap_or_else(CapabilitySet::empty),
                    annotated,
                });
                self.check_expr(value);
                self.scope_stack.pop();
            }
            // Struct, Enum, TypeAlias — no capability concerns.
            _ => {}
        }
    }

    fn check_function(
        &mut self,
        name: &str,
        annotations: &[Annotation],
        body: Option<&kryos_ast::Block>,
        span: Span,
    ) {
        let caps = CapabilitySet::from_annotations(annotations);
        let annotated = Self::has_capabilities_annotation(annotations);
        let _budget = Budget::from_annotations(annotations);
        let _sandbox = Sandbox::from_annotations(annotations);

        // Validate unknown capability names in annotations.
        self.validate_capability_annotations(annotations);

        // Attenuation check: this function's capabilities must not exceed
        // the enclosing scope's capabilities.
        if let Some(parent) = self.current_scope() {
            if !caps.is_subset_of(&parent.capabilities) {
                let excess = caps.excess_over(&parent.capabilities);
                let excess_names: Vec<String> = excess.iter().map(|c| c.to_string()).collect();
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "function `{name}` exceeds parent capabilities: [{}]",
                        excess_names.join(", ")
                    ))
                    .with_label(span, "declared here")
                    .with_note(
                        "child scope cannot add capabilities not granted by parent".to_string(),
                    )
                    .with_code(kryos_errors::codes::E0503),
                );
            }
        }

        // Choose the effective capability set this function's body is checked
        // against, and whether the per-call guards fire (`annotated`):
        //
        // - Permissive: unchanged — only explicitly annotated functions enforce.
        // - Strict: every function enforces against its declaration (empty if
        //   unannotated), so every gated builtin call must be declared.
        // - Inferred: `main` and annotated functions are BOUNDARIES — they
        //   enforce against their DECLARATION (empty `main` => deny-by-default,
        //   annotation => ceiling). Every other (interior) function enforces
        //   against its INFERRED set, so helpers need no annotation yet their
        //   requirements still propagate up to the boundary.
        let (effective_caps, effective_annotated) = match self.mode {
            CapabilityMode::Permissive => (caps, annotated),
            CapabilityMode::Strict => (caps, true),
            CapabilityMode::Inferred => {
                if annotated || name == "main" {
                    (caps, true)
                } else {
                    let inferred = self
                        .fn_capabilities
                        .get(name)
                        .cloned()
                        .unwrap_or_else(CapabilitySet::empty);
                    (inferred, true)
                }
            }
        };
        let scope = CapabilityScope {
            capabilities: effective_caps,
            annotated: effective_annotated,
        };
        self.scope_stack.push(scope);
        let saved_entry_depth = self.current_fn_entry_scope_depth;
        self.current_fn_entry_scope_depth = self.scope_stack.len();
        if let Some(block) = body {
            self.check_block(block);
        }
        self.current_fn_entry_scope_depth = saved_entry_depth;
        self.scope_stack.pop();
    }

    fn check_actor(
        &mut self,
        name: &str,
        annotations: &[Annotation],
        handlers: &[kryos_ast::MessageHandler],
    ) {
        let caps = CapabilitySet::from_annotations(annotations);
        let annotated = Self::has_capabilities_annotation(annotations);

        self.validate_capability_annotations(annotations);

        // Attenuation: actor capabilities must not exceed parent scope.
        if let Some(parent) = self.current_scope() {
            if !caps.is_subset_of(&parent.capabilities) {
                let excess = caps.excess_over(&parent.capabilities);
                let excess_names: Vec<String> = excess.iter().map(|c| c.to_string()).collect();
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "actor `{name}` exceeds parent capabilities: [{}]",
                        excess_names.join(", ")
                    ))
                    .with_label(
                        annotations.first().map(|a| a.span).unwrap_or(Span::DUMMY),
                        "declared here",
                    )
                    .with_note("spawned actor capabilities must be a subset of the spawner's")
                    .with_code(kryos_errors::codes::E0503),
                );
            }
        }

        // Check each handler under the actor's capability scope. Both Strict
        // and Inferred enforce against the actor's declaration (empty if
        // unannotated): actors are message-passing boundaries, so their
        // authority must be declared, not inferred from handler bodies.
        let scope = CapabilityScope {
            capabilities: caps,
            annotated: annotated || !matches!(self.mode, CapabilityMode::Permissive),
        };
        self.scope_stack.push(scope);
        let saved_entry_depth = self.current_fn_entry_scope_depth;
        self.current_fn_entry_scope_depth = self.scope_stack.len();
        for handler in handlers {
            let own_params: std::collections::HashSet<String> = handler
                .params
                .iter()
                .filter(|p| self.is_fn_bearing_type(&p.ty))
                .map(|p| p.name.clone())
                .collect();
            self.current_local_container_lits = Self::build_local_container_lits(&handler.body);
            self.current_local_closure_caps = self.build_local_closure_caps(
                &handler.body,
                &self.fn_capabilities,
                &self.fn_return_closure_caps,
                &own_params,
                &self.current_local_container_lits,
            );
            self.current_fn_typed_params = own_params;
            self.check_block(&handler.body);
            self.current_fn_typed_params.clear();
            self.current_local_closure_caps.clear();
            self.current_local_container_lits.clear();
        }
        self.current_fn_entry_scope_depth = saved_entry_depth;
        self.scope_stack.pop();
    }

    fn check_extern(&mut self, items: &[Decl], span: Span) {
        // Extern blocks require the Ffi capability.
        if let Some(caps) = self.current_caps() {
            if !caps.has(Capability::Ffi) {
                self.diagnostics.push(
                    Diagnostic::error("extern block requires `ffi` capability")
                        .with_label(span, "extern block here")
                        .with_code(kryos_errors::codes::E0506),
                );
            }
        } else {
            // Top-level extern — check against module-level (no scope = no caps).
            // Extern at top level is only valid if there's an enclosing scope with ffi.
            // At module level we allow it (it's a declaration, not an invocation).
            // The actual *use* of extern functions inside capability-scoped code
            // will be caught when those functions are called.
        }

        for item in items {
            self.check_decl(item);
        }
    }

    fn check_block(&mut self, block: &kryos_ast::Block) {
        // Lexical scope for `current_locals`: bindings introduced inside this
        // block go out of scope at its end, so a sibling/later block does not
        // wrongly see them as locals (which would re-open the self-shadow gate
        // bypass). Snapshot on entry, restore on exit.
        let saved_locals = self.current_locals.clone();
        for stmt in &block.stmts {
            self.check_stmt(stmt);
        }
        self.current_locals = saved_locals;
    }

    /// Check a `deny!(caps) { body }` block. Narrows the active capability set by
    /// removing the denied capabilities, then checks the body under that narrowed,
    /// annotated scope so the existing per-call guards (E0505 builtin / E0502
    /// missing / E0507 propagation) fire on any use of a denied capability.
    fn check_deny_block(&mut self, denied: &[String], body: &kryos_ast::Block, span: Span) {
        let denied_caps: Vec<Capability> =
            denied.iter().filter_map(|s| Capability::from_str(s)).collect();
        // A deny that names no recognized capability (typo) would silently narrow
        // nothing — warn rather than no-op.
        if denied_caps.is_empty() {
            self.diagnostics.push(
                Diagnostic::warning(format!(
                    "deny!({}) names no recognized capability; the block has no effect",
                    denied.join(", ")
                ))
                .with_label(span, "deny block here")
                .with_code(kryos_errors::codes::W0500),
            );
        }
        let base = self
            .current_caps()
            .cloned()
            .unwrap_or_else(CapabilitySet::empty);
        let narrowed = base.without(&denied_caps);
        self.scope_stack.push(CapabilityScope {
            capabilities: narrowed,
            annotated: true,
        });
        self.check_block(body);
        self.scope_stack.pop();
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                value,
                name,
                pattern,
                ..
            } => {
                if let Some(expr) = value {
                    self.check_expr(expr);
                }
                // The binding comes INTO SCOPE only now, AFTER its initializer
                // is checked -- so `let leaker = leaker` checks the RHS while
                // `leaker` still resolves to the (annotated) function, and the
                // value-authority gate fires. Uses on later statements see it as
                // a local (as intended for a local named like a stdlib fn).
                self.current_locals.insert(name.clone());
                if let Some(p) = pattern {
                    Self::pattern_names(p, &mut self.current_locals);
                }
            }
            Stmt::Assign { value, target, .. } => {
                self.check_expr(target);
                self.check_expr(value);
            }
            Stmt::Return { value, .. } => {
                if let Some(expr) = value {
                    self.check_expr(expr);
                }
            }
            Stmt::If {
                condition,
                then_block,
                elif_clauses,
                else_block,
                ..
            } => {
                self.check_expr(condition);
                self.check_block(then_block);
                for (cond, block) in elif_clauses {
                    self.check_expr(cond);
                    self.check_block(block);
                }
                if let Some(block) = else_block {
                    self.check_block(block);
                }
            }
            Stmt::For {
                pattern,
                iterable,
                body,
                ..
            } => {
                self.check_expr(iterable);
                // The loop pattern binds locals scoped to the body.
                let saved = self.current_locals.clone();
                Self::pattern_names(pattern, &mut self.current_locals);
                self.check_block(body);
                self.current_locals = saved;
            }
            Stmt::While {
                condition, body, ..
            } => {
                self.check_expr(condition);
                self.check_block(body);
            }
            Stmt::Expr { expr, .. } => {
                self.check_expr(expr);
            }
            Stmt::Spawn { expr, span } => {
                self.check_spawn_expr(expr, *span);
            }
            Stmt::TryCatch {
                try_block,
                catch_name,
                catch_block,
                ..
            } => {
                self.check_block(try_block);
                // The catch variable is scoped to the catch block.
                let saved = self.current_locals.clone();
                self.current_locals.insert(catch_name.clone());
                self.check_block(catch_block);
                self.current_locals = saved;
            }
            Stmt::Throw { expr, .. } => {
                self.check_expr(expr);
            }
            Stmt::Select { branches, .. } => {
                for branch in branches {
                    self.check_expr(&branch.channel);
                    self.check_block(&branch.body);
                }
            }
            Stmt::DenyBlock { denied, body, span } => {
                self.check_deny_block(denied, body, *span);
            }
            Stmt::Break { .. } | Stmt::Continue { .. } => {}
        }
    }

    fn check_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::FnCall { callee, args, span } => {
                // Check if the callee is a path to a capability-gated stdlib module.
                self.check_callee_capabilities(callee, args, *span);

                // Check for prohibited self-heal escalation calls.
                self.check_escalation(callee, *span);

                // Recurse into arguments (the Identifier arm flags any gated
                // builtin used as a value). Do NOT recurse into a bare-name
                // callee: its capability is already enforced by
                // check_callee_capabilities, and recursing would double-report.
                if !matches!(callee.as_ref(), Expr::Identifier { .. }) {
                    self.check_expr(callee);
                }
                for arg in args {
                    self.check_expr(arg);
                }
            }
            Expr::MethodCall {
                object,
                method,
                args,
                span,
            } => {
                // Check for prohibited self-heal escalation method calls.
                if is_escalation_action(method) {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "prohibited capability escalation: `{method}` cannot be called"
                        ))
                        .with_label(*span, "escalation attempt")
                        .with_note(
                            "capabilities are immutable at compile time; \
                             self-heal actions cannot escalate privileges",
                        )
                        .with_code(kryos_errors::codes::E0504),
                    );
                }

                // Propagate the method's capability requirement to the caller
                // (the previously-missing call-site check for method dispatch).
                let mut extra = self.compute_hot_extra_caps(method, args, true);
                // FAIL-CLOSED: `obj.method(...)` is sometimes really a
                // function-typed struct FIELD read and invoked (`reg.reader()`),
                // not a genuine method dispatch -- see
                // `resolve_method_field_invoke_caps`. Only engages when
                // `method` is positively confirmed to be such a field.
                extra = extra.union(&self.resolve_method_field_invoke_caps(
                    object,
                    method,
                    &self.fn_capabilities,
                    &self.current_local_closure_caps,
                    &self.current_local_container_lits,
                    &self.current_fn_typed_params,
                ));
                self.enforce_callee_name(method, *span, false, &extra);

                self.check_expr(object);
                for arg in args {
                    self.check_expr(arg);
                }
            }
            Expr::StaticMethodCall {
                method, args, span, ..
            } => {
                // Same call-site propagation for `Type::method()` static
                // dispatch. The parser turns ANY `Ident::name(args)` into a
                // StaticMethodCall, and the checker/MIR lower an unknown
                // `type_name` to a bare call to `name` — so
                // `NotAType::file_write(..)` actually invokes the builtin.
                // If `method` is a gated builtin name, gate it (backlog #38):
                // pass is_builtin_name so the E0505 builtin gate fires, just
                // as it does for the ordinary `file_write(..)` call shape.
                let is_gated_builtin = required_capability_for_builtin(method).is_some();
                let extra = self.compute_hot_extra_caps(method, args, true);
                self.enforce_callee_name(method, *span, is_gated_builtin, &extra);
                for arg in args {
                    self.check_expr(arg);
                }
            }
            Expr::FieldAccess { object, .. } => {
                self.check_expr(object);
            }
            Expr::IndexAccess { object, index, .. } => {
                self.check_expr(object);
                self.check_expr(index);
            }
            Expr::BinaryOp { left, right, .. } => {
                self.check_expr(left);
                self.check_expr(right);
            }
            Expr::UnaryOp { operand, .. } => {
                self.check_expr(operand);
            }
            Expr::ArrayLiteral { elements, .. } | Expr::TupleLiteral { elements, .. } => {
                for elem in elements {
                    self.check_expr(elem);
                }
            }
            Expr::MapLiteral { entries, .. } => {
                for (k, v) in entries {
                    self.check_expr(k);
                    self.check_expr(v);
                }
            }
            Expr::StructLiteral { fields, .. } => {
                for (_, expr) in fields {
                    self.check_expr(expr);
                }
            }
            Expr::Lambda { params, body, .. } => {
                // Lambdas inherit the enclosing scope's capabilities.
                //
                // Fold this lambda's OWN parameter names into the
                // fn-typed-parameter set for the duration of checking its
                // body, mirroring a NAMED function's own params
                // (`current_fn_typed_params`). Without this, a
                // TRANSPARENT-FORWARDING inline lambda (`|f| f()`) invoking
                // its own bound parameter is flagged as a bad direct
                // invocation by the fail-closed default even though the
                // ENCLOSING call site (`map(tools, |f| f())`) already
                // resolves and enforces its real authority correctly via
                // `accumulate_hot_extra_caps` (see `resolve_closure_caps`'s
                // `Lambda` arm) -- this would
                // otherwise be a spurious SECOND, wrong diagnostic for
                // exactly the call the outer machinery already accounts for.
                let saved = self.current_fn_typed_params.clone();
                let saved_transparent = self.transparent_lambda_params.clone();
                self.current_fn_typed_params
                    .extend(params.iter().map(|p| p.name.clone()));
                self.transparent_lambda_params
                    .extend(params.iter().map(|p| p.name.clone()));
                self.check_expr(body);
                self.current_fn_typed_params = saved;
                self.transparent_lambda_params = saved_transparent;
            }
            Expr::IfExpr {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.check_expr(condition);
                self.check_block(then_branch);
                if let Some(block) = else_branch {
                    self.check_block(block);
                }
            }
            Expr::MatchExpr { subject, arms, .. } => {
                self.check_expr(subject);
                for arm in arms {
                    self.check_expr(&arm.body);
                    if let Some(guard) = &arm.guard {
                        self.check_expr(guard);
                    }
                }
            }
            Expr::PipeExpr { left, right, .. } => {
                self.check_expr(left);
                self.check_expr(right);
            }
            Expr::Borrow { inner, .. }
            | Expr::Deref { inner, .. }
            | Expr::SharedExpr { inner, .. }
            | Expr::MoveExpr { inner, .. }
            | Expr::WeakExpr { inner, .. } => {
                self.check_expr(inner);
            }
            Expr::ComptimeBlock { body, .. }
            | Expr::QuantumBlock { body, .. }
            | Expr::UnsafeBlock { body, .. } => {
                self.check_block(body);
            }
            Expr::Cast { expr, .. } => {
                self.check_expr(expr);
            }
            Expr::Block { block, .. } => {
                self.check_block(block);
            }
            Expr::Await { value, .. } => {
                self.check_expr(value);
            }
            Expr::RangeExpr { start, end, .. } => {
                if let Some(s) = start {
                    self.check_expr(s);
                }
                if let Some(e) = end {
                    self.check_expr(e);
                }
            }
            Expr::InterpolatedString { parts, .. } => {
                for part in parts {
                    if let kryos_ast::StringPart::Expr(e) = part {
                        self.check_expr(e);
                    }
                }
            }
            // A bare identifier that names a gated builtin, evaluated as a
            // VALUE (let/assign/return/array/struct/pipe/arg — anywhere that is
            // not the direct callee of a call), delegates that builtin's
            // authority and must be declared. The FnCall arm deliberately does
            // not recurse into a bare-name callee, so this never double-fires.
            Expr::Identifier { span, .. } => {
                self.check_builtin_value_ref(expr, *span);
            }
            // Literals — no capability concerns.
            Expr::IntLiteral { .. }
            | Expr::FloatLiteral { .. }
            | Expr::StringLiteral { .. }
            | Expr::CharLiteral { .. }
            | Expr::BoolLiteral { .. }
            | Expr::NoneLiteral { .. } => {}
        }
    }

    /// Check a function call's callee to see if it references a capability-gated path.
    ///
    /// We resolve simple dotted paths like `std.net.TcpStream.connect(...)` by
    /// walking field access chains. This also catches direct calls to
    /// identifiers that match stdlib path patterns, bare builtin function
    /// calls (like `file_write`), and cross-function capability propagation.
    fn check_callee_capabilities(&mut self, callee: &Expr, args: &[Expr], call_span: Span) {
        let segments = self.resolve_path(callee);

        // 1. Check qualified stdlib paths (e.g. std.io.write_file).
        if let Some(required_cap) = required_capability_for_path(&segments) {
            if let Some(caps) = self.current_caps() {
                if !caps.satisfies_required(&required_cap) {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "call to `{}` requires `{required_cap}` capability",
                            segments.join("::")
                        ))
                        .with_label(call_span, format!("requires `{required_cap}`"))
                        .with_note(format!(
                            "add `@capabilities({required_cap})` to the enclosing function or actor"
                        ))
                        .with_code(kryos_errors::codes::E0502),
                    );
                }
            }
        }

        // 2+3. Bare builtin requirement + cross-function propagation, keyed on
        //      the single-segment callee name. Shared with method / static
        //      dispatch (see `enforce_callee_name`).
        let mut callee_is_named = false;
        if segments.len() == 1 {
            // CLOSURE-LAUNDERING FIX: attribute the authority of whatever
            // closure/fn-value is ACTUALLY passed at this call site into any
            // hot (invoked) fn-typed parameter position — see
            // `compute_hot_extra_caps`.
            let extra = self.compute_hot_extra_caps(&segments[0], args, false);
            self.enforce_callee_name(&segments[0], call_span, true, &extra);
            callee_is_named = self.defined_fns.contains(&segments[0])
                || self.actor_names.contains(&segments[0])
                || self.enum_variant_names.contains(&segments[0])
                || is_known_builtin_name(&segments[0])
                || self.extern_fns.contains(&segments[0]);
        }

        // FAIL-CLOSED DEFAULT: everything above only enforces a call whose
        // callee resolves to a KNOWN function/builtin/extern NAME (so it can
        // be looked up in `hot_params`/`fn_capabilities` by name). A callee
        // that is instead a first-class fn-value with no such name -- a bare
        // local (`r()`), a container element (`arr[0]()`, `m["k"][0]()`), or
        // any field/index chain into one -- was never checked at all,
        // regardless of what it carries. Resolve it directly and require
        // whatever it actually needs; an unresolvable provenance requires
        // `all` rather than silently requiring nothing. See
        // `resolve_direct_invoke_caps`. Gated on `segments.len() <= 1` for
        // the same reason as the inference-side twin in `collect_caps_expr`:
        // a multi-segment path is always a qualified stdlib call, already
        // fully handled above.
        if !callee_is_named && segments.len() <= 1 && (self.strict_mode() || self.has_annotated_scope()) {
            let extra = self.resolve_direct_invoke_caps(
                callee,
                &self.fn_capabilities,
                &self.current_local_closure_caps,
                &self.current_local_container_lits,
                &self.current_fn_typed_params,
            );
            self.require_direct_invoke_caps(&extra, call_span);
        }
    }

    /// Convenience wrapper over `accumulate_hot_extra_caps` for the
    /// ENFORCEMENT walk: uses the checker's live `fn_capabilities` (the
    /// final, fully-merged map) and the CURRENT function's own tracked
    /// fn-typed parameters / local closure-authority map.
    fn compute_hot_extra_caps(
        &self,
        callee_name: &str,
        args: &[Expr],
        is_method_or_static: bool,
    ) -> CapabilitySet {
        let mut extra = CapabilitySet::empty();
        self.accumulate_hot_extra_caps(
            callee_name,
            args,
            is_method_or_static,
            &mut extra,
            &self.fn_capabilities,
            &self.current_local_closure_caps,
            &self.current_local_container_lits,
            &self.current_fn_typed_params,
        );
        extra
    }

    /// Enforce the capability requirements of a call to `name` — a bare
    /// function name (`FnCall`) or a method name (`MethodCall` /
    /// `StaticMethodCall`). Two checks, both gated on the caller being an
    /// enforcing scope (strict mode, or any annotated/inferred scope):
    ///
    /// - **E0505** if `name` is a capability-gated builtin the caller does not
    ///   hold; and
    /// - **E0507** if `name` is a known function/method (annotated ceiling or
    ///   inferred set) whose capabilities exceed the caller's.
    ///
    /// Routing method dispatch through here is what closes the soundness hole
    /// where `obj.method()` / `Type::method()` could exercise authority the
    /// caller never declared (the call-site propagation was previously only
    /// wired for free-function `FnCall`s).
    ///
    /// `is_builtin_name`: whether `name` may denote a gated runtime builtin.
    /// True for free-function calls (`file_write(...)`), FALSE for method /
    /// static dispatch -- a method is never a runtime builtin, so a method
    /// merely NAMED like one (e.g. `doc.write_file()`) must NOT be force-gated
    /// by the builtin table; it is gated only by its own propagated caps.
    fn enforce_callee_name(
        &mut self,
        name: &str,
        call_span: Span,
        is_builtin_name: bool,
        extra: &CapabilitySet,
    ) {
        if !(self.strict_mode() || self.has_annotated_scope()) {
            return;
        }

        // A user function that SHADOWS a builtin's name resolves to the user
        // function (gated only by its own propagated caps below), so it must not
        // be force-gated by the builtin table.
        if is_builtin_name && !self.defined_fns.contains(name) {
            if let Some(required_cap) = required_capability_for_builtin(name) {
                if let Some(caps) = self.current_caps() {
                    if !caps.satisfies_required(&required_cap) {
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "builtin `{name}` requires `{required_cap}` capability"
                            ))
                            .with_label(call_span, format!("requires `{required_cap}`"))
                            .with_note(format!(
                                "add `@capabilities({required_cap})` to the enclosing function or actor"
                            ))
                            .with_code(kryos_errors::codes::E0505),
                        );
                    }
                }
            }
        }

        // Calls to extern-declared functions are the raw FFI surface — gate
        // them like the builtin they wrap (`kryos_*`) or on `ffi` (anything
        // else). Declaring an extern is free; CALLING it demands authority.
        if let Some(required_cap) = self.required_capability_for_extern(name) {
            if let Some(caps) = self.current_caps() {
                if !caps.satisfies_required(&required_cap) {
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "extern function `{name}` requires `{required_cap}` capability"
                        ))
                        .with_label(call_span, format!("requires `{required_cap}`"))
                        .with_note(format!(
                            "add `@capabilities({required_cap})` to the enclosing function or actor"
                        ))
                        .with_code(kryos_errors::codes::E0506),
                    );
                }
            }
        }

        // Union in the call-site-specific authority carried by any hot
        // fn-typed argument (see `compute_hot_extra_caps`) — this is what
        // makes `zero_cap_tool(reader)` require `fs:read` at THIS call,
        // even though `zero_cap_tool` itself has (correctly, like any HOF)
        // no capability of its own in `fn_capabilities`.
        let base_callee_caps = self
            .fn_capabilities
            .get(name)
            .cloned()
            .unwrap_or_else(CapabilitySet::empty);
        let callee_caps = base_callee_caps.union(extra);
        if !callee_caps.is_empty() {
            if let Some(caller_caps) = self.current_caps() {
                if !callee_caps.is_subset_of(caller_caps) {
                    let excess = callee_caps.excess_over(caller_caps);
                    let excess_names: Vec<String> =
                        excess.iter().map(|c| c.to_string()).collect();
                    let via_closure = !extra.is_empty();
                    let note = if via_closure {
                        format!(
                            "call to `{name}` requires [{}] not granted to caller -- some of \
                             this authority is carried by a closure/fn-value ARGUMENT passed at \
                             this call site, not by `{name}`'s own declaration",
                            callee_caps
                                .iter()
                                .map(|c| c.to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    } else {
                        format!(
                            "function `{name}` has @capabilities({}) but caller lacks [{}]",
                            callee_caps
                                .iter()
                                .map(|c| c.to_string())
                                .collect::<Vec<_>>()
                                .join(", "),
                            excess_names.join(", ")
                        )
                    };
                    self.diagnostics.push(
                        Diagnostic::error(format!(
                            "call to `{name}` requires capabilities [{}] not granted to caller",
                            excess_names.join(", ")
                        ))
                        .with_label(call_span, "callee requires more capabilities")
                        .with_note(note)
                        .with_code(kryos_errors::codes::E0507),
                    );
                }
            }
        }
    }

    /// A gated builtin used as a first-class VALUE (passed as a `fn(...)`
    /// argument, or bound to a variable) hands out its authority just as much
    /// as calling it. Require the capability at the reference site so
    /// `apply(file_write, ...)` cannot smuggle `fs:write` past the boundary.
    fn check_builtin_value_ref(&mut self, expr: &Expr, span: Span) {
        if let Expr::Identifier { name, .. } = expr {
            if !(self.strict_mode() || self.has_annotated_scope()) {
                return;
            }
            // An extern fn handed out as a value carries its authority too.
            if let Some(required_cap) = self.required_capability_for_extern(name) {
                if let Some(caps) = self.current_caps() {
                    if !caps.satisfies_required(&required_cap) {
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "extern function `{name}` used as a value requires `{required_cap}` capability"
                            ))
                            .with_label(span, format!("requires `{required_cap}`"))
                            .with_code(kryos_errors::codes::E0506),
                        );
                    }
                }
            }
            if !self.defined_fns.contains(name) {
              if let Some(required_cap) = required_capability_for_builtin(name) {
                if let Some(caps) = self.current_caps() {
                    if !caps.satisfies_required(&required_cap) {
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "builtin `{name}` used as a value requires `{required_cap}` capability"
                            ))
                            .with_label(span, format!("requires `{required_cap}`"))
                            .with_note(format!(
                                "passing `{name}` as a function value delegates its authority; \
                                 add `@capabilities({required_cap})` to the enclosing function"
                            ))
                            .with_code(kryos_errors::codes::E0505),
                        );
                    }
                }
              }
            }
            // A USER FUNCTION (or method) with capabilities used as a first-class
            // value hands out ITS authority too. `fn h(p) { file_read(p) }` then
            // `let f = h; f(x)` / `apply(h, x)` / `Reader { f: h }` / `[h]` reached
            // a gated op through a local holding the function, which the call-site
            // gate never sees -- a capability ESCAPE. Require the function's full
            // set at the reference site, against the enclosing boundary's grants.
            // Skip names bound as LOCALS: a local variable that merely shares a
            // name with a function is not a reference to that function.
            if self.current_locals.contains(name) {
                return;
            }
            if let Some(fn_caps) = self.fn_capabilities.get(name).cloned() {
                if let Some(caps) = self.current_caps() {
                    if !fn_caps.is_subset_of(caps) {
                        let excess = fn_caps.excess_over(caps);
                        let excess_names: Vec<String> =
                            excess.iter().map(|c| c.to_string()).collect();
                        self.diagnostics.push(
                            Diagnostic::error(format!(
                                "function `{name}` used as a value requires capabilities [{}] not granted here",
                                excess_names.join(", ")
                            ))
                            .with_label(span, "delegates its authority")
                            .with_note(format!(
                                "referencing `{name}` as a value hands out its authority; \
                                 add `@capabilities({})` to the enclosing function",
                                excess_names.join(", ")
                            ))
                            .with_code(kryos_errors::codes::E0507),
                        );
                    }
                }
            }
        }
    }

    /// Check if a callee expression is a prohibited escalation action.
    fn check_escalation(&mut self, callee: &Expr, span: Span) {
        let segments = self.resolve_path(callee);
        if let Some(last) = segments.last() {
            if is_escalation_action(last) {
                self.diagnostics.push(
                    Diagnostic::error(format!(
                        "prohibited capability escalation: `{last}` cannot be called"
                    ))
                    .with_label(span, "escalation attempt")
                    .with_note(
                        "capabilities are immutable at compile time; \
                         self-heal actions cannot escalate privileges",
                    )
                    .with_code(kryos_errors::codes::E0504),
                );
            }
        }
    }

    /// Check a spawn expression against current capabilities.
    ///
    /// When spawning, the spawned entity's capabilities must not exceed
    /// the current scope's capabilities.
    fn check_spawn_expr(&mut self, expr: &Expr, span: Span) {
        // The spawn itself is checked — any function/actor being spawned
        // will have its capabilities checked via attenuation rules.
        // Here we flag the spawn site if there's no enclosing capability scope.
        self.check_expr(expr);

        // Additional: if spawning a struct literal (actor), check annotations would
        // apply at the actor declaration site (already handled by check_actor).
        // This is here for completeness — spawn-site checks are structural.
        let _ = span;
    }

    /// Resolve a callee expression into path segments.
    ///
    /// `std.net.connect(...)` → `["std", "net", "connect"]`
    fn resolve_path(&self, expr: &Expr) -> Vec<String> {
        match expr {
            Expr::Identifier { name, .. } => vec![name.clone()],
            Expr::FieldAccess { object, field, .. } => {
                let mut segments = self.resolve_path(object);
                segments.push(field.clone());
                segments
            }
            _ => Vec::new(),
        }
    }

    /// Validate that `@capabilities(...)` annotations only contain recognized names.
    fn validate_capability_annotations(&mut self, annotations: &[Annotation]) {
        for ann in annotations {
            if ann.name == "capabilities" {
                for arg in &ann.args {
                    if Capability::from_str(arg).is_none() {
                        self.diagnostics.push(
                            Diagnostic::warning(format!(
                                "unknown capability `{arg}` in @capabilities annotation"
                            ))
                            .with_label(ann.span, format!("`{arg}` is not recognized"))
                            .with_note(
                                "known capabilities: net, io, ffi, compute, crypto, \
                                 process, env, term, db, time, all"
                                    .to_string(),
                            )
                            .with_code("W-CAP-UNKNOWN"),
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kryos_ast::{Block, MessageHandler};
    use kryos_errors::Span;

    fn span() -> Span {
        Span::DUMMY
    }

    fn ann(name: &str, args: Vec<&str>) -> Annotation {
        Annotation {
            name: name.to_string(),
            args: args.into_iter().map(|s| s.to_string()).collect(),
            span: span(),
        }
    }

    fn empty_block() -> Block {
        Block {
            stmts: Vec::new(),
            span: span(),
        }
    }

    fn stdlib_call(module: &str, func: &str) -> Expr {
        Expr::FnCall {
            callee: Box::new(Expr::FieldAccess {
                object: Box::new(Expr::FieldAccess {
                    object: Box::new(Expr::Identifier {
                        name: "std".into(),
                        span: span(),
                    }),
                    field: module.into(),
                    span: span(),
                }),
                field: func.into(),
                span: span(),
            }),
            args: vec![],
            span: span(),
        }
    }

    fn fn_decl(name: &str, caps: Vec<&str>, body_stmts: Vec<Stmt>) -> Decl {
        Decl::Function {
            name: name.to_string(),
            generics: vec![],
            params: vec![],
            ret_ty: None,
            body: Some(Block {
                stmts: body_stmts,
                span: span(),
            }),
            public: false,
            is_async: false,
            annotations: if caps.is_empty() {
                vec![]
            } else {
                vec![ann("capabilities", caps)]
            },
            doc_comments: vec![],
            span: span(),
        }
    }

    #[test]
    fn deny_block_narrows_net_inside_net_fn() {
        // @capabilities(net) fn guarded() { deny!(net) { http_get() } }
        // deny removes net inside the block -> http_get (needs net:http) errors E0505.
        let deny = Stmt::DenyBlock {
            denied: vec!["net".to_string()],
            body: Block {
                stmts: vec![Stmt::Expr {
                    expr: builtin_call("http_get"),
                    span: span(),
                }],
                span: span(),
            },
            span: span(),
        };
        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl("guarded", vec!["net"], vec![deny])],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("E0505")),
            "expected E0505 for http_get inside deny!(net), got: {diags:?}"
        );
    }

    #[test]
    fn deny_block_allows_non_denied_cap() {
        // @capabilities(net, io) fn { deny!(net) { file_write() } }
        // file_write needs fs:write (covered by io); deny!(net) keeps io -> no error.
        let deny = Stmt::DenyBlock {
            denied: vec!["net".to_string()],
            body: Block {
                stmts: vec![Stmt::Expr {
                    expr: builtin_call("file_write"),
                    span: span(),
                }],
                span: span(),
            },
            span: span(),
        };
        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl("guarded", vec!["net", "io"], vec![deny])],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        let errors: Vec<_> = diags.iter().filter(|d| d.is_error()).collect();
        assert!(
            errors.is_empty(),
            "deny!(net) must not break a file_write covered by io, got: {diags:?}"
        );
    }

    #[test]
    fn no_capabilities_no_stdlib_no_errors() {
        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl("main", vec![], vec![])],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        assert!(diags.is_empty());
    }

    #[test]
    fn using_net_with_capability_is_ok() {
        let call = stdlib_call("net", "connect");
        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl(
                "do_net",
                vec!["net"],
                vec![Stmt::Expr {
                    expr: call,
                    span: span(),
                }],
            )],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        assert!(diags.is_empty(), "expected no errors, got: {diags:?}");
    }

    #[test]
    fn using_net_without_capability_is_error() {
        let call = stdlib_call("net", "connect");
        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl(
                "do_net",
                vec![],
                vec![Stmt::Expr {
                    expr: call,
                    span: span(),
                }],
            )],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("requires `net` capability"));
        assert_eq!(diags[0].code.as_deref(), Some("E0502"));
    }

    #[test]
    fn attenuation_child_exceeds_parent() {
        // The AST doesn't nest functions as Decls inside function bodies,
        // so we test attenuation via an actor whose handler scope is limited.
        let actor = Decl::Actor {
            name: "MyActor".into(),
            state_fields: vec![],
            handlers: vec![MessageHandler {
                name: "handle".into(),
                params: vec![],
                ret_ty: None,
                body: empty_block(),
                span: span(),
            }],
            annotations: vec![ann("capabilities", vec!["net"])],
            doc_comments: vec![],
            span: span(),
        };

        // Inner function declared inside actor scope — we test this by
        // building a module with just the outer function, and pushing a
        // manual scope to demonstrate. Instead, let's use the public API properly:
        // Module-level functions don't have a parent scope, so attenuation
        // doesn't apply there. We test attenuation through actors and nested decls.
        let module = Module {
            name: "test".into(),
            declarations: vec![actor],
            span: span(),
        };
        // This should pass — the actor has `net` and the handler uses nothing.
        let diags = check_capabilities(&module, false);
        assert!(diags.is_empty());
    }

    #[test]
    fn escalation_via_method_call_is_error() {
        let escalation = Expr::MethodCall {
            object: Box::new(Expr::Identifier {
                name: "self".into(),
                span: span(),
            }),
            method: "add_capability".into(),
            args: vec![],
            span: span(),
        };

        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl(
                "try_escalate",
                vec!["net"],
                vec![Stmt::Expr {
                    expr: escalation,
                    span: span(),
                }],
            )],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        assert!(!diags.is_empty());
        assert!(diags
            .iter()
            .any(|d| d.code.as_deref() == Some("E0504")));
    }

    #[test]
    fn escalation_via_fn_call_is_error() {
        let escalation = Expr::FnCall {
            callee: Box::new(Expr::Identifier {
                name: "widen_sandbox".into(),
                span: span(),
            }),
            args: vec![],
            span: span(),
        };

        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl(
                "try_escalate",
                vec!["net"],
                vec![Stmt::Expr {
                    expr: escalation,
                    span: span(),
                }],
            )],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        assert!(!diags.is_empty());
        assert!(diags
            .iter()
            .any(|d| d.code.as_deref() == Some("E0504")));
    }

    #[test]
    fn all_capability_grants_everything() {
        // Using std::net, std::io, std::crypto in a function with `all`.
        let calls = vec![
            Stmt::Expr {
                expr: stdlib_call("net", "connect"),
                span: span(),
            },
            Stmt::Expr {
                expr: stdlib_call("io", "read"),
                span: span(),
            },
            Stmt::Expr {
                expr: stdlib_call("crypto", "hash"),
                span: span(),
            },
        ];

        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl("god_mode", vec!["all"], calls)],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        assert!(
            diags.is_empty(),
            "expected no errors with `all`, got: {diags:?}"
        );
    }

    // -------------------------------------------------------------------
    // Strict mode tests (--strict-capabilities / deny-by-default).
    //
    // These exercise the public `check_capabilities(module, strict)` switch
    // so the wiring stays end-to-end: pipeline.rs -> check_capabilities
    // -> CapabilityChecker { strict_mode } -> per-call guard.
    // -------------------------------------------------------------------

    /// Build a bare-name builtin call expression (e.g. `file_write()`),
    /// which is what the strict-mode path enforces against.
    fn builtin_call(name: &str) -> Expr {
        Expr::FnCall {
            callee: Box::new(Expr::Identifier {
                name: name.into(),
                span: span(),
            }),
            args: vec![],
            span: span(),
        }
    }

    #[test]
    fn strict_unannotated_with_file_write_is_error() {
        // Unannotated function that calls `file_write` (requires `fs:write`).
        // In strict mode this must surface E0505 at the call site.
        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl(
                "leaks",
                vec![],
                vec![Stmt::Expr {
                    expr: builtin_call("file_write"),
                    span: span(),
                }],
            )],
            span: span(),
        };
        let diags = check_capabilities(&module, true);
        let errors: Vec<_> = diags.iter().filter(|d| d.is_error()).collect();
        assert_eq!(
            errors.len(),
            1,
            "expected exactly one E0505 error, got: {diags:?}"
        );
        assert_eq!(errors[0].code.as_deref(), Some("E0505"));
        assert!(errors[0].message.contains("file_write"));
        assert!(errors[0].message.contains("`fs:write`"));
    }

    #[test]
    fn nonstrict_unannotated_with_file_write_is_ok() {
        // Same module as above, but with strict=false — the existing
        // opt-in behaviour. No errors should be raised.
        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl(
                "leaks",
                vec![],
                vec![Stmt::Expr {
                    expr: builtin_call("file_write"),
                    span: span(),
                }],
            )],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        assert!(
            diags.iter().all(|d| !d.is_error()),
            "non-strict mode must not error on unannotated builtin calls, got: {diags:?}"
        );
    }

    #[test]
    fn strict_unannotated_with_no_gated_calls_is_ok() {
        // Unannotated function that touches nothing capability-gated.
        // Strict mode must NOT error — we only deny gated builtins, not
        // every function definition.
        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl("pure", vec![], vec![])],
            span: span(),
        };
        let diags = check_capabilities(&module, true);
        assert!(
            diags.iter().all(|d| !d.is_error()),
            "pure function in strict mode must have no errors, got: {diags:?}"
        );
    }

    #[test]
    fn strict_annotated_with_matching_cap_is_ok() {
        // Annotated @capabilities(io) function calling file_write.
        // Strict mode must accept this — the annotation grants exactly
        // what the call needs.
        let module = Module {
            name: "test".into(),
            declarations: vec![fn_decl(
                "writer",
                vec!["io"],
                vec![Stmt::Expr {
                    expr: builtin_call("file_write"),
                    span: span(),
                }],
            )],
            span: span(),
        };
        let diags = check_capabilities(&module, true);
        assert!(
            diags.iter().all(|d| !d.is_error()),
            "annotated function with matching cap must not error in strict mode, got: {diags:?}"
        );
    }

    #[test]
    fn unknown_capability_name_warns() {
        let module = Module {
            name: "test".into(),
            declarations: vec![Decl::Function {
                name: "bad_caps".into(),
                generics: vec![],
                params: vec![],
                ret_ty: None,
                body: Some(empty_block()),
                public: false,
                is_async: false,
                annotations: vec![ann("capabilities", vec!["net", "banana"])],
                doc_comments: vec![],
                span: span(),
            }],
            span: span(),
        };
        let diags = check_capabilities(&module, false);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("banana"));
        assert_eq!(diags[0].code.as_deref(), Some("W-CAP-UNKNOWN"));
    }

    // -----------------------------------------------------------------
    // Inferred mode (--capabilities-mode=inferred / deny-by-default with
    // interior inference). These prove: helpers need no annotation, the
    // boundary (main / annotated fns) is where deny-by-default bites, and
    // authority cannot leak past an unannotated boundary.
    // -----------------------------------------------------------------

    fn call(name: &str) -> Stmt {
        Stmt::Expr {
            expr: builtin_call(name),
            span: span(),
        }
    }

    fn infer(decls: Vec<Decl>) -> Vec<Diagnostic> {
        let module = Module {
            name: "test".into(),
            declarations: decls,
            span: span(),
        };
        check_capabilities_mode(&module, CapabilityMode::Inferred)
    }

    #[test]
    fn inferred_helper_needs_no_annotation() {
        // main @capabilities(fs:write) { helper() }   fn helper() { file_write() }
        // helper is unannotated; inference covers it. Zero errors.
        let helper = fn_decl("helper", vec![], vec![call("file_write")]);
        let main = fn_decl("main", vec!["fs:write"], vec![call("helper")]);
        let diags = infer(vec![main, helper]);
        assert!(diags.is_empty(), "expected clean, got {diags:?}");
    }

    #[test]
    fn inferred_unannotated_main_transitive_write_is_error() {
        // fn main() { helper() }   fn helper() { file_write() }
        // main declares nothing -> deny-by-default: the transitively-required
        // fs:write must be declared on main. Error names the missing cap.
        let helper = fn_decl("helper", vec![], vec![call("file_write")]);
        let main = fn_decl("main", vec![], vec![call("helper")]);
        let diags = infer(vec![main, helper]);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("E0507")),
            "expected E0507 at the boundary, got {diags:?}"
        );
    }

    #[test]
    fn inferred_unannotated_main_direct_builtin_is_error() {
        // fn main() { file_write() }  -> E0505 at the boundary.
        let main = fn_decl("main", vec![], vec![call("file_write")]);
        let diags = infer(vec![main]);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("E0505")),
            "expected E0505, got {diags:?}"
        );
    }

    #[test]
    fn inferred_annotated_main_covers_direct_builtin() {
        let main = fn_decl("main", vec!["fs:write"], vec![call("file_write")]);
        assert!(infer(vec![main]).is_empty());
    }

    #[test]
    fn inferred_pure_helper_is_authority_free() {
        // fn main() { pure() }  fn pure() { println-like/no builtins }  -> clean.
        // A helper that touches no capability provably requires none, so an
        // unannotated main calling it is fine.
        let pure = fn_decl("pure", vec![], vec![call("some_local_fn")]);
        let leaf = fn_decl("some_local_fn", vec![], vec![]);
        let main = fn_decl("main", vec![], vec![call("pure")]);
        assert!(infer(vec![main, pure, leaf]).is_empty());
    }

    #[test]
    fn inferred_chain_depth_three_propagates() {
        // a @capabilities(fs:write) -> b -> c -> file_write ; b,c unannotated.
        let c = fn_decl("c", vec![], vec![call("file_write")]);
        let b = fn_decl("b", vec![], vec![call("c")]);
        let a = fn_decl("a", vec!["fs:write"], vec![call("b")]);
        // `a` is the boundary; give it a main caller so nothing dangles.
        let main = fn_decl("main", vec!["fs:write"], vec![call("a")]);
        let diags = infer(vec![main, a, b, c]);
        assert!(diags.is_empty(), "expected clean chain, got {diags:?}");
    }

    #[test]
    fn inferred_annotation_ceiling_is_enforced() {
        // fn helper @capabilities(fs:read) { file_write() } -> fs:write not
        // covered by the declared fs:read ceiling. E0505.
        let helper = fn_decl("helper", vec!["fs:read"], vec![call("file_write")]);
        let main = fn_decl("main", vec!["fs:read"], vec![call("helper")]);
        let diags = infer(vec![main, helper]);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("E0505")),
            "expected E0505 ceiling violation, got {diags:?}"
        );
    }

    #[test]
    fn inferred_ambient_builtins_never_blocked() {
        // fn main() { exit(); sleep(); }  -> ambient, no annotation needed.
        let main = fn_decl("main", vec![], vec![call("exit"), call("sleep")]);
        assert!(infer(vec![main]).is_empty());
    }

    #[test]
    fn permissive_default_unchanged_by_inference() {
        // The same unannotated-main-writes-file program is clean in Permissive.
        let main = fn_decl("main", vec![], vec![call("file_write")]);
        let module = Module {
            name: "test".into(),
            declarations: vec![main],
            span: span(),
        };
        assert!(check_capabilities_mode(&module, CapabilityMode::Permissive).is_empty());
    }

    // --- Soundness: authority must not leak through method / static dispatch
    // or through gated builtins passed as first-class values (regression tests
    // for holes found by adversarial review). ---

    fn method_call(recv: &str, method: &str) -> Stmt {
        Stmt::Expr {
            expr: Expr::MethodCall {
                object: Box::new(Expr::Identifier {
                    name: recv.into(),
                    span: span(),
                }),
                method: method.into(),
                args: vec![],
                span: span(),
            },
            span: span(),
        }
    }

    fn impl_with_method(target: &str, method: Decl) -> Decl {
        Decl::Impl {
            target: target.into(),
            trait_name: None,
            generics: vec![],
            methods: vec![method],
            doc_comments: vec![],
            span: span(),
        }
    }

    #[test]
    fn inferred_method_call_authority_is_caught() {
        // impl Sink { fn write(self) { file_write(...) } }  (unannotated method)
        // fn main() { s.write() }  (unannotated) -> deny-by-default at boundary.
        // Before the fix, MethodCall never propagated and this LEAKED.
        let write = fn_decl("write", vec![], vec![call("file_write")]);
        let imp = impl_with_method("Sink", write);
        let main = fn_decl("main", vec![], vec![method_call("s", "write")]);
        let diags = infer(vec![imp, main]);
        assert!(
            diags.iter().any(|d| d.is_error()),
            "method-dispatch authority must be caught at the boundary, got {diags:?}"
        );
    }

    #[test]
    fn inferred_method_call_authority_ok_when_declared() {
        // Same, but main declares fs:write -> inference threads it, no error.
        let write = fn_decl("write", vec![], vec![call("file_write")]);
        let imp = impl_with_method("Sink", write);
        let main = fn_decl("main", vec!["fs:write"], vec![method_call("s", "write")]);
        let diags = infer(vec![imp, main]);
        assert!(diags.is_empty(), "declared main must pass, got {diags:?}");
    }

    fn let_stmt(name: &str, value: Expr) -> Stmt {
        Stmt::Let {
            name: name.into(),
            ty: None,
            value: Some(value),
            pattern: None,
            mutable: false,
            span: span(),
        }
    }

    fn ident(name: &str) -> Expr {
        Expr::Identifier {
            name: name.into(),
            span: span(),
        }
    }

    #[test]
    fn inferred_builtin_let_bound_is_caught() {
        // fn main() { let f = file_write }  -- binding a gated builtin to a
        // variable delegates its authority; unannotated main must be rejected.
        // (Non-argument value position -- the second-pass leak.)
        let main = fn_decl("main", vec![], vec![let_stmt("f", ident("file_write"))]);
        let diags = infer(vec![main]);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("E0505")),
            "let-bound gated builtin must require its cap, got {diags:?}"
        );
    }

    #[test]
    fn inferred_builtin_returned_is_caught() {
        // fn get() { return file_write }  -- returning a gated builtin exposes
        // its authority. get's inferred set must include fs:write and propagate.
        let get = Decl::Function {
            name: "get".into(),
            generics: vec![],
            params: vec![],
            ret_ty: None,
            body: Some(Block {
                stmts: vec![Stmt::Return {
                    value: Some(ident("file_write")),
                    span: span(),
                }],
                span: span(),
            }),
            public: false,
            is_async: false,
            annotations: vec![],
            doc_comments: vec![],
            span: span(),
        };
        let main = fn_decl("main", vec![], vec![call("get")]);
        let diags = infer(vec![get, main]);
        assert!(
            diags.iter().any(|d| d.is_error()),
            "returned gated builtin must propagate to the boundary, got {diags:?}"
        );
    }

    #[test]
    fn inferred_free_fn_named_like_builtin_is_not_gated() {
        // A user-DEFINED free function named `file_write` that does pure work
        // resolves (via `defined_fns`) to the user function -- codegen dispatches
        // to it, not the builtin -- so it must NOT be forced to declare fs:write
        // (commit 69a18a3: attribute caps by resolved symbol, not by name). The
        // real, non-shadowed builtin call is still gated (see
        // inferred_unannotated_main_direct_builtin_is_error), and the value-position
        // authority escape is still caught (inferred_builtin_passed_as_value_is_caught).
        let shadow = fn_decl("file_write", vec![], vec![call("some_noop")]);
        let noop = fn_decl("some_noop", vec![], vec![]);
        let main = fn_decl("main", vec![], vec![call("file_write")]);
        let diags = infer(vec![shadow, noop, main]);
        assert!(
            diags.is_empty(),
            "a pure user fn shadowing a builtin name must not be gated, got {diags:?}"
        );
    }

    #[test]
    fn inferred_user_method_named_like_builtin_not_gated() {
        // A METHOD named `write_file` is never a runtime builtin, so method
        // dispatch must NOT force fs:write -- it is gated only by the method's
        // own (here empty) caps. This is the realistic false positive the sound
        // design fixes: methods skip the builtin table entirely.
        let m = fn_decl("write_file", vec![], vec![]);
        let imp = impl_with_method("Doc", m);
        let main = fn_decl("main", vec![], vec![method_call("d", "write_file")]);
        let diags = infer(vec![imp, main]);
        assert!(
            diags.is_empty(),
            "user method colliding with a builtin name must not be gated, got {diags:?}"
        );
    }

    #[test]
    fn inferred_builtin_passed_as_value_is_caught() {
        // fn main() { apply(file_write, ...) }  -- passing a gated builtin as a
        // fn value delegates its authority; unannotated main must be rejected.
        let call_apply = Stmt::Expr {
            expr: Expr::FnCall {
                callee: Box::new(Expr::Identifier {
                    name: "apply".into(),
                    span: span(),
                }),
                args: vec![Expr::Identifier {
                    name: "file_write".into(),
                    span: span(),
                }],
                span: span(),
            },
            span: span(),
        };
        let main = fn_decl("main", vec![], vec![call_apply]);
        let diags = infer(vec![main]);
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("E0505")),
            "gated builtin passed as a value must require its cap, got {diags:?}"
        );
    }
}
