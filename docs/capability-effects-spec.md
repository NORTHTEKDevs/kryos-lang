# Capability-typed function values: final specification

> Status: SPECIFICATION, not implemented. Supersedes the three independent
> drafts synthesized below and the sketch in `docs/capability-roadmap.md`
> Part 1b. Read `docs/capability-soundness.md` (24 invariants, R1-R7 failure
> history) first — this document's entire job is to make that document's
> theorem hold **by construction**, not by an ever-growing enumeration of
> resolver cases. No code changed, no rebuild performed, per task
> instruction.

## Judgment: which draft this is built from, and why

Three independent specs were produced (minimal-inference / principled-effects
/ pragmatic-migration). All three converge on the same correct core idea —
move the capability set into `Type::Function` as a first-class field and
enforce by ordinary subtyping at call sites — so the choice between them is
about representation soundness, annotation burden, and staging rigor, not
about whether the core idea is right.

**Base: the minimal-inference draft (`kryos-types::CapRow` as a bitset +
open var list).** Verified directly against the code (`kryos-types/src/ty.rs`,
`kryos-capabilities/src/model.rs`): `Type` derives `Eq, Hash` (`ty.rs:9`) and
`CapabilitySet` derives only `Debug, Clone, PartialEq, Eq` — **not** `Hash`
(`model.rs:157`), because its inner field is a `HashSet<Capability>`, and
`HashSet<T>` does not and cannot soundly implement `std::hash::Hash` (its
iteration order is unspecified). The other two drafts both embed
`CapabilitySet` directly inside a new enum (`CapReq::Closed(CapabilitySet)` /
`CapRow::Concrete(CapabilitySet)`) that they then attach, transitively, to
`Type::Function` — which will not compile once `#[derive(Hash)]` is
attempted on that enum, and manually implementing `Hash` for a `HashSet`
wrapper is the wrong fix (it would either be non-deterministic or require
sorting on every hash, silently expensive). The minimal-inference draft is
the only one of the three that caught this and designed around it: a
15-variant `Capability` enum (`Net, NetHttp, NetTcp, Io, FsRead, FsWrite,
Ffi, Compute, Crypto, Process, Env, Term, Db, Time, All` — counted directly
from `model.rs:13-45`) fits trivially in a `Copy + Eq + Hash + Ord` bitset.
This is not a stylistic preference — it is the difference between a design
that compiles and one that does not, so it is dispositive for the
representation layer (§1).

The minimal-inference draft is also the most precisely grounded of the
three in the actual unification engine (`infer.rs:176` `fresh_var`,
`:321` `unify`, `:684` `instantiate`, `:762` `instantiate_sig`, `:78`
`substitutions: HashMap<u32, Type>`), and its generalization rule — a fresh
capability-row variable is created for **any** unannotated fn-typed
parameter/return in a **declaration**, independent of whether that
declaration has ordinary type generics — is strictly more general than the
pragmatic-migration draft's rule (which ties row-variable creation to the
presence of `<T, U>` and would silently default a type-monomorphic HOF's
callback to a closed empty-set requirement unless the user writes `@{...}`
by hand — a real annotation-burden regression on exactly the kind of
non-generic callback-taking helper that is common in application code, not
just stdlib).

**Grafted from the pragmatic-migration draft:** its 8-stage migration
sequence (§8 below) is the most granular and most concretely gated of the
three — in particular Stage 2's differential harness (new inference vs. the
existing `fn_capabilities` heuristic's charge, compared call-site by
call-site across the full corpus, *before* the new mechanism is given any
enforcement authority) and Stage 6's numeric compile-time-regression budget
and IR-residue grep for the zero-ABI-impact claim. These are adopted
verbatim as the staging discipline, adapted to the base draft's
representation.

**Grafted from the principled-effects draft:** its explicit argument for
*why* a closed-bitset-or-variable representation is the right weight for
Kryos specifically (a fixed, compiler-defined, non-extensible label set —
never a Koka-style open row with a growable tail, because nothing in Kryos
ever defines a new capability label at the type level) is kept as the
representation-layer justification in §1, and its explicit contravariance-
argument phrasing (Liskov substitutability over "the set of gated
operations a value may cause") is kept as the soundness argument in §3.
Its `GenericParamKind::CapRow` / `C: CapSet` explicit-bound mechanism is
**rejected** — grep-confirmed against its own Stage 3 (migration draft's
own text: "Rewrite `std::iter::map/filter/fold/reduce/find` ... to use
`C: CapSet` explicitly") this requires a one-time hand-edit to every
stdlib HOF and every ecosystem HOF relying on the old heuristic's leniency,
which the other two drafts' fully-implicit generalization makes
unnecessary. Optional explicit row-naming for `pub`-surface documentation
is noted as a future, non-required extension in §10, not built into the
core mechanism.

Nothing below is claimed proven; every "impossible by construction" claim
must be re-verified live during implementation exactly as this repo's own
operational rule #6 requires (revert, rebuild, confirm the bug reappears;
restore, rebuild, confirm it's gone) — see the acceptance criteria attached
to each migration stage in §8.

---

## 0. The one-sentence design

Move the capability set out of the side-table the checker reconstructs from
syntax (`checker.rs`'s `hot_params`, `hot_param_companions`,
`resolve_closure_caps`, `resolve_container_path_caps`, ...) and into the
function's **type**, as a third field on `Type::Function` /
`TypeExpr::Function`. A concrete function's row is an ordinary, unifiable
bitset. A generic HOF's callback-parameter row is a **row variable** —
substituted at each call site by the exact mechanism that already
substitutes `T`/`U` type variables. Enforcement becomes one subtyping check
at each call expression against the callee's resolved type; there is no
provenance to trace because the type already carries the answer.

---

## 1. Representation

### 1.1 Surface syntax

```kryos
fn() -> str @ {fs:read}
fn(i64) -> bool                  // no `@{...}`: INFER, never "requires nothing"
fn(str) -> str @ {fs:read, fs:write}
fn() -> str @ {}                 // explicit empty: PROVEN pure, checked against the body
```

`@{...}` is a type-position suffix only (params/return of a function type,
`let` annotations, struct fields, trait method signatures) — never a
statement-level construct, so it cannot collide with the existing
statement-level `@capabilities(...)`/`@budget(...)`/`@sandbox` annotations.
Two states, never conflated:

- **Absent** — infer. The only backward-compatible reading; every `.kry`
  file in the 91-example corpus, the 66 stdlib modules, the 259-source
  ecosystem, and the self-host compiler has no `@{...}` today and must parse
  identically post-migration.
- **Explicit** (including `@{}`) — a checked upper bound: the body's
  inferred row must be `is_subset_of` what's written, or it's a compile
  error naming the excess (same relationship `@capabilities(...)` already
  has to a function body, reusing `CapabilitySet::excess_over`,
  `model.rs:270-279`, for the diagnostic).

### 1.2 AST layer (`kryos-ast/src/types.rs`)

```rust
Function {
    params: Vec<TypeExpr>,
    ret: Box<TypeExpr>,
    caps: Option<Vec<(String, Span)>>,   // None = infer; Some(vec![]) = explicit empty
    span: Span,
}
```

Raw name/span pairs, resolved later — mirrors how `Annotation.args:
Vec<String>` already stores `@capabilities(...)` arguments unresolved until
`Capability::from_str` runs. `TypeExpr` derives only `Debug, Clone,
PartialEq` today (`types.rs:3`) — adding an `Option<Vec<(String, Span)>>`
field needs nothing extra there.

### 1.3 Resolved-type layer (`kryos-types/src/ty.rs`)

This is the layer where the other two drafts' representation breaks. Since
`Type` derives `Eq, Hash` (`ty.rs:9`), and `CapabilitySet`'s
`HashSet<Capability>` cannot soundly derive `Hash`, the capability set
cannot live in `Type::Function` as a `CapabilitySet`. Kryos has 15
`Capability` variants total (`model.rs:13-45`), so the concrete part is a
plain bitset — `Copy`, trivially `Eq + Hash + Ord`, and turns every set
operation the checker needs (`union`, `is_subset_of`, `without`) into O(1)
bit ops instead of `HashSet` allocation:

```rust
// kryos-types/src/ty.rs (new)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct CapBits(u32);   // bit i = one Capability variant; 15 used, 32 available

pub type CapVarId = u32;

/// A capability row: a concrete lower bound plus zero or more still-open
/// row variables. `vars` is kept SORTED + DEDUPED as a struct invariant so
/// `#[derive(Eq, Hash)]` gives correct structural equality for free.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CapRow {
    concrete: CapBits,
    vars: Vec<CapVarId>,   // usually 0 or 1; >1 after unioning two still-open rows
}

impl CapRow {
    pub fn closed(bits: CapBits) -> Self { Self { concrete: bits, vars: vec![] } }
    pub fn empty() -> Self { Self::closed(CapBits::default()) }
    pub fn var(v: CapVarId) -> Self { Self { concrete: CapBits::default(), vars: vec![v] } }
    pub fn is_closed(&self) -> bool { self.vars.is_empty() }
    pub fn union(&self, other: &CapRow) -> CapRow { /* bits |=, vars sorted-merge-dedup */ }
    /// Only decidable when self is closed; see §3 for open-vs-open comparison.
    pub fn is_subset_of(&self, other: &CapRow) -> bool { /* self.is_closed() && bits check */ }
}
```

```rust
Function {
    params: Vec<Type>,
    ret: Box<Type>,
    caps: CapRow,
}
```

**Why not reuse `CapabilitySet` at all, even via a wrapper:** because the
mismatch is structural (a `HashSet` cannot be hashed soundly), not
cosmetic — any wrapper that keeps a `HashSet` inside a `Hash`-derived type
either fails to compile or needs a hand-written `Hash` impl whose
correctness (agreement with `PartialEq`, which is order-independent for a
set) requires sorting on every hash call. The bitset sidesteps this
entirely and is strictly cheaper. Why not a general Koka-style open row
(label + tail)? Kryos's capability label set is a small, fixed,
compiler-defined enum — no Kryos program ever defines a new capability
label at the type level — so row polymorphism here only ever needs "the
same fixed unknown set, wherever it occurs" (a set variable), never "these
effects plus possibly more appended later." A `CapRow` (closed bits + open
var list) is the correct-weight mechanism; importing row concatenation,
presence/absence constraints, or a row kind system would add complexity
nothing in Kryos's model requires.

### 1.4 Enforcement-layer bridge (`kryos-capabilities`)

`kryos-capabilities` keeps its own `Capability`/`CapabilitySet`
(`HashSet`-based; used for scope grants, `deny!` narrowing, and
diagnostics, none of which need `Hash`). It gains one pure, total
conversion pair: `CapabilitySet::from_bits(CapBits) -> CapabilitySet` and
`CapabilitySet::to_bits(&self) -> CapBits`, used only where a resolved
`Type::Function.caps` needs comparing against a live scope's
`CapabilitySet`. This keeps `kryos-capabilities` and `kryos-types` as
sibling crates with **no new `Cargo.toml` dependency edge** — `CapBits` is
the one piece of vocabulary duplicated across them, and that sync
obligation is the one deliberate, stated migration cost (§8, Stage 0).

---

## 2. Inference

### 2.1 The common case: closed row, computed exactly as today

An ordinary named function with no fn-typed parameter or return gets
`CapRow::closed(bits)` where `bits` is exactly what the existing
call-graph fixed point (`fn_capabilities`) already computes — reused
verbatim, just written into `Type::Function.caps` instead of only a
side-table keyed by name.

### 2.2 A lambda literal: closed row from its own body

The existing structural classification of a fresh lambda literal
(`resolve_closure_caps`'s `collect_caps_expr` walk) is reused unchanged.
The result becomes `CapRow::closed(bits)` on the lambda's inferred
`Type::Function`, retained on the type instead of computed once and
discarded.

### 2.3 The hard case: capability-row polymorphism

```
fn map<T, U>(arr: [T], f: fn(T) -> U) -> [U]
```

No new user-facing generic-parameter syntax (`<T, U>` stays exactly as
today; no `C: CapSet` bound). `map`'s own `FunctionSig` gains
`generic_cap_var_ids: Vec<CapVarId>` alongside the existing
`generic_var_ids: Vec<u32>` (`infer.rs`, the `instantiate_sig` region,
`:762-775`), populated automatically: **every unannotated fn-typed
parameter or return position in a declaration gets a fresh `CapVarId`**,
whether or not that declaration also has ordinary type generics — this is
the rule that keeps a type-monomorphic HOF (`fn apply(f: fn()->str) -> str
{ return f() }`) capability-polymorphic without the user writing anything,
which the pragmatic-migration draft's tie-to-`<T,U>` rule does not achieve.

- `InferenceEngine::instantiate` (`infer.rs:684-755`) gets one new arm:
  ```rust
  Type::Function { params, ret, caps } => Type::Function {
      params: params.iter().map(|p| self.instantiate(p, var_map)).collect(),
      ret: Box::new(self.instantiate(ret, var_map)),
      caps: self.instantiate_row(caps, cap_var_map),   // new, mirrors the Var(id) arm
  },
  ```
  `instantiate_row` remaps each old `CapVarId` in `caps.vars` to a fresh one
  via `cap_var_map: &HashMap<CapVarId, CapVarId>`, built alongside `var_map`
  in `instantiate_sig` from `sig.generic_cap_var_ids` — the same
  per-call-site freshening `T`/`U` already get.
- At a call site `map(xs, some_closure)`, unifying the declared parameter
  type against the actual argument's resolved type binds `C'` in a
  `cap_substitutions: HashMap<CapVarId, CapRow>` map parallel to
  `InferenceEngine.substitutions: HashMap<u32, Type>` (`infer.rs:78`).
  `unify` (`infer.rs:321`) gets one new `Type::Function` arm that, after
  unifying `params`/`ret` pairwise, also unifies `caps`: if one side is a
  bare `Var(v)`, bind `cap_substitutions[v] = other_side_row`; if both
  sides are closed and unequal, **union** rather than hard-error (this
  matters for array/collection literals mixing closures of different
  concrete rows — see §8's compatibility note).
- The return type resolves (mirroring `resolve`, `infer.rs:203-314`, which
  already chases `substitutions` recursively — add one arm chasing
  `cap_substitutions` the same way) to the real row, with zero new tracing
  code, because the row variable was never anything but an ordinary
  generic parameter that happens to range over sets instead of types.

### 2.4 Generalization scope (value restriction)

Only a function/method **declaration** with an unannotated fn-typed
parameter or return position introduces a fresh `CapVarId`. A `let`-bound
local or a lambda literal never generalizes — its row is always fully
resolved (closed) by the time its `let` finishes checking, matching ML's
value restriction and Kryos's existing non-generalization of locals. This
is why module globals and ordinary `let x = || file_read(p)` locals never
carry an open row — only a declared function signature can.

### 2.5 No annotation burden for ordinary code

A function that never takes or returns a fn-value, and is not a `pub`
trait method / `dyn`-facing signature whose ceiling is the interesting part
of the contract, needs zero `@{...}` anywhere. `@{...}` is written by hand
only where it documents a published ceiling — the same bar the roadmap
sketch already set.

---

## 3. Subtyping and variance

**Rule:** `fn(Ps) -> R @ S1 <: fn(Ps) -> R @ S2` iff `S1.is_subset_of(S2)` —
needing less is a subtype of needing more, checked contravariantly wherever
a function type itself is checked contravariantly (parameter position),
composing with the existing param-contravariant/return-covariant structural
check as one more field comparison, not a new pass.

**Soundness argument (Liskov substitutability over the set of gated
operations a value may cause):** a context that accepts a value of type
`fn(...) -> R @ S2` has, by construction, already been checked (at its own
declaration or the `deny!` scope currently in force, §7) to hold at least
`S2` before it may call that value. A value that actually needs only
`S1 ⊆ S2` is safe to substitute there, because "holds `S2`" implies "holds
`S1`." This is exactly `CapabilitySet::is_subset_of` (`model.rs:216-218`),
already used today for child-scope-attenuation and the ordinary
declared-vs-available call check, applied one level down to a first-class
value's row instead of a scope's declaration. It is the same soundness
shape as Java's checked-exception covariance on overrides, Koka/Eff effect-
row subtyping, and Rust's `Send`/`Sync` auto-trait propagation.

**Open-row vs. open-row comparison** (comparing two still-unresolved
schemes directly — e.g. a generic `impl`'s method against its trait's
generic ceiling before either is instantiated) is decided by positional
row-variable correspondence: the Nth capability-row parameter of the
impl's scheme must be `⊆` the Nth of the trait's declared ceiling scheme,
treating each unresolved `CapVarId` as an opaque, positionally-matched
placeholder — the same discipline already used for `T`/`U` positional
correspondence in a trait-impl check, applied to the parallel cap-var list.

---

## 4. Polymorphism (the hard requirement)

`map`'s declared scheme is effectively `∀ T, U, C. (arr: [T], f: fn(T) -> U
@ C) -> [U] @ C`, with `C` never written by the programmer — it is a
placeholder filled in per call site by whichever closure is passed, exactly
as `T`/`U` already are. Consequences, none requiring special-casing:

1. **`filter`/`fold`/`reduce`/`find`/user HOFs** get identical treatment —
   one fresh `CapVarId` per unannotated fn-typed parameter/return, threaded
   through automatically. The stdlib's surface syntax is **completely
   unchanged** (`fn map<T, U>(arr: [T], f: fn(T) -> U) -> [U]`) — the `@ C`
   is inferred and attached to the *resolved* type, never required in
   source. This is the annotation-burden win over the principled-effects
   draft's `C: CapSet` requirement.
2. **`|f| f()` (the transparent-forwarding-lambda case)** needs zero relief
   mechanism. Its inferred type is `fn(g: fn()->T @ C') -> T @ C'` — its
   own row IS the same row variable as its parameter `g`, unified through
   ordinary body inference of the call `g()`. When passed as `map`'s `f`,
   `C'` unifies with `map`'s `C`; at the actual call site `C` (and
   transitively `C'`) unifies with the real argument's row. One
   unification chain, no `hot_param_companions`-style data-flow heuristic.
3. **The decoy-companion attack class cannot be constructed** — see §6.3.

### 4.1 Generic containers (`List<fn(...) -> T @ C>`)

The row rides inside whatever `Struct { generics }`'s Nth type argument
resolves to — the *existing* struct-generic substitution
(`instantiate`'s `Struct`/`Enum` arms) already threads it, since a
`fn(...)->T @ C` is just one more thing a generic argument can be. No
bespoke "transparent accessor" case is needed for `std::collections`.

---

## 5. Interaction with the rest of the language

- **Generics/monomorphization.** All `CapVarId` substitution happens during
  type checking (`instantiate`/`unify` on `kryos-types::Type`), strictly
  before `kryos-mir` monomorphization runs. By the time a monomorphization
  call site is reached, every `CapRow` is already closed — monomorphization
  gains no new degree of freedom, closing the class of bug that caused the
  worst historical failures (`hot_param_companions` ran pre-monomorphization
  on the un-substituted AST and was itself sound *there*, but every
  downstream consumer re-derived shape from syntax; this design removes the
  re-derivation). This must be re-verified as a structural property during
  implementation (construct the adversarial "bare-`T`-invoked-as-function"
  case against the new design), not assumed.
- **`dyn Trait`.** A trait method's declared `@{...}` ceiling is part of the
  vtable slot's `Type::Function`. An impl coercing into `dyn Trait` is
  checked via §3's subtyping at the coercion site — one more field compared
  in the existing attenuation check. Containers of `dyn Trait` remain the
  existing `E0110` limitation (orthogonal, not fixed and not broken here).
- **Actors/actor state.** A fn-typed struct/actor-state field carries its
  row through ordinary nested field-type propagation. `self.b.f()`,
  `let x = self.b; x.f()`, and an arbitrarily-deep `let` chain of aliases
  all resolve through the SAME static type — aliasing never changes a
  type, so alias depth is irrelevant by construction. This closes
  invariant 24's disclosed residual (chained alias-of-alias) as a
  byproduct, not a targeted fix — the old checker's `current_actor_state_
  alias_locals` was bespoke one-hop provenance tracking; the type system
  has no such limit. The actor's own `@capabilities` ceiling stays a
  separate, unaffected declaration-enforced mechanism (answers "what can
  this actor's handlers do," not "what does a stored closure require");
  the two compose via one more attenuation check at actor declaration.
- **`spawn`.** A spawned closure's row is read off its already-inferred
  type at the capture site; the boundary check becomes ordinary §3
  subtyping against the spawning scope's grant — no `check_spawn_expr`-
  specific capability code path.
- **Containers, `Option`/`Result`, tuples, struct fields.** No special
  resolver of any shape. An element/payload/field's type — row included —
  propagates through the *existing* type-checker machinery that already
  correctly propagates `i64`/`str`/`T` through these forms. This is the
  single biggest simplification: today's checker enumerates container
  SHAPES (`resolve_container_path_caps` walking `PathStep`s); the new
  design has none, because reading a value out of a container is already
  an ordinary typed expression.
- **Module globals.** A top-level `let` of fn-type is never generalized
  (§2.4), so it always resolves to a closed row via ordinary inference —
  no "is this row still open at module scope" check is needed; it cannot
  be open by construction.
- **`extern`/FFI.** Not migrated into the type system. `extern` items
  backed by `kryos_*` builtins keep the existing name-based lookup
  (`required_capability_for_native_symbol`, `model.rs:520-572`) exactly as
  today — externs are FFI declarations, not first-class Kryos values with
  inferred bodies. The raw-memory TCB split (`RAW_MEMORY_BUILTINS`,
  direct-use-only, root-module-only) stays a separate, untouched pass for
  the same reason: folding it in would defeat the point of that carve-out
  (propagation must NOT happen for these primitives).

---

## 6. Enforcement — walking all five named attacks through the design

**The single enforcement rule, replacing the shape-tracing machinery:**

> At any call expression `callee(args)`, resolve `callee`'s type via the
> ordinary type checker — whatever mechanism already resolves the type of
> ANY expression. If that type is `Type::Function { caps, .. }`, require
> `caps.is_subset_of(&current_effective_scope)` (§3), naming the excess on
> failure (existing `E0507`-shape diagnostic, now sourced from the
> callee's real type instead of a heuristic trace). A `CapRow` still
> containing an unresolved `Var` at a call site is a compiler-internal bug
> (an unsolved row), never treated as empty and never guessed at.

1. **Container-alias** (`let arr = m["k"]; arr[0]()`): `m["k"]`'s type is
   `map<str, T>`'s value type; `arr[0]`'s type is ordinary array-element
   propagation. The call-site check applies directly regardless of whether
   the container came from a literal or a function call — closing
   invariant 4's "non-literal container source" residual as a side effect.
2. **Actor-state-loop-alias**, including the disclosed chained-alias
   residual: closed at any depth by §5's argument (types don't degrade
   with aliasing).
3. **Decoy companion** (`apply_from_map(decoy, real, f)`): there is no
   "which sibling argument feeds `f`" question asked at all — `f`'s row is
   resolved by unifying `f`'s declared/inferred type against the actual
   argument expression bound to `f`'s own parameter position, full stop. A
   decoy at a different parameter position is never inspected, because
   nothing in the rule ever looks at a sibling argument.
4. **Accessor-call** (`reg.reader()`): whichever way the existing type
   checker resolves method-call-syntax on a field of fn-type, the result
   is a `Type::Function` with a row, and the call-site check is identical.
5. **Tuple-index** (`t.0()`): `Tuple.elements[0]`'s type, row included,
   propagates through the existing `Tuple` substitution arm.

All five are different syntactic routes to one semantic fact — "call a
value whose type the type checker already knows." A sixth, future route
(a new capture syntax, a new binding form) is automatically covered the
moment the type checker assigns it a `Function` type with a `caps` field,
with zero new code in `kryos-capabilities`.

### 6.1 Net deletion (quantified against files actually read)

Candidates for deletion once the new mechanism carries the full corpus
(Stage 7, §8): `resolve_closure_caps`, `resolve_container_path_caps`,
`build_local_closure_caps`, `build_local_container_lits`, `hot_params`,
`hot_param_companions`, `accumulate_hot_extra_caps`,
`resolve_direct_invoke_caps`, `resolve_method_field_invoke_caps`,
`resolve_actor_self_field_invoke_caps`, `current_actor_state_alias_locals`,
`current_fn_entry_scope_depth`, `deferred_own_param_caps`,
`transparent_lambda_params`, `structural_lambda_eval_depth`. Against
`checker.rs`'s current 5,921 lines this is plausibly the majority of the
file — a large net deletion, not just an addition — but this is a
migration-time measurement (Stage 7 acceptance criterion), not a claim to
take on faith.

---

## 7. `deny!` narrowing and enforcement modes

`deny!(caps){ body }` still pushes `current_scope.capabilities.without(...)`.
§6's rule evaluates `is_subset_of` against whatever the live scope is *at
the call*, using the callee's already-fully-resolved type — there is no
"defer the charge to my caller" mechanism left for a value with a known
row, so the historical bug class (a `deny!` interposed between receiving a
capability-bearing value and invoking it, resolved against the wrong
scope) cannot recur: nothing is deferred. `current_fn_entry_scope_depth`
and `deferred_own_param_caps` become dead code — not re-patched, but
structurally unnecessary, because the category of bug (a charge resolved
against the wrong scope because it was deferred) requires a deferral
mechanism to exist, and this design has none. The only thing that still
"looks" deferred is generic row-variable substitution (§4), which is sound
generic instantiation checked once, at the actual call, against the actual
argument's type — not a trust-the-future deferral.

`strict`/`inferred`/`permissive` keep their existing meaning (which
functions are capability *boundaries*), orthogonal to and unchanged by
capability-typed fn-values. `permissive` mode must still **compute** (not
skip) an unannotated function's row, even though it skips checking whether
the function's own body stays within it — otherwise a caller checked under
`strict`/`inferred` elsewhere in the same compilation would consume a
dishonest type. This is a small, real, worth-stating distinction from
today's `permissive` behavior.

---

## 8. Migration: staged rollout, every step gate-green and bisectable

Adopts the pragmatic-migration draft's staging discipline in full,
adapted to this document's representation. Constraints: 91-example corpus,
66 stdlib modules, 259 ecosystem sources, self-hosting compiler, existing
gate chain (`tools/loop/kryos-loop.sh gates 2`, `tests/security_gate.sh`,
`compiler/self-host/test_bootstrap.sh` 16/16 alone, `tests/ecosystem_
check.sh`, `python tools/docs-examples/check.py`), and the standing rule
that touching `kryos-rt`/`kryos-stdlib-native` needs a full `cargo build
--release`, never `-p kryos-cli` alone.

**Stage 0 — AST/parser plumbing, semantically inert.**
Add `caps: Option<Vec<(String, Span)>>` to `TypeExpr::Function`; parser
accepts optional `@{...}` suffix. Off by construction: every existing
`.kry` file has no `@{...}`, parses to `None`. *Acceptance:* `cargo build
--release` clean; full example corpus (91/91) byte-identical pass/fail to
pre-change baseline; new parser-only unit tests for the `@{...}` grammar.

**Stage 1 — `kryos-types` carries `CapRow`, still a no-op.**
Add `CapBits`/`CapVarId`/`CapRow` (§1.3); add `caps: CapRow` to
`Type::Function`, defaulted via a pass-through unify arm that always
succeeds (nothing downstream reads the solved value yet). *Acceptance:*
`kryos-loop gates 2` + bootstrap 16/16 + ecosystem_check green with
IDENTICAL pass/fail results to the pre-change baseline — a byte-for-byte
gate report match, not just "still green."

**Stage 2 — row inference, still zero enforcement authority.**
Implement §2/§4's per-declaration row solving in `kryos-types` as a new
analysis, wired to nothing that enforces yet. Add `kryos check --dump-cap-
types` (debug-only) and a differential harness: for every example (91),
every ecosystem package (259), and the self-host compiler, compare "new
inferred row, unioned at each call site" against "old heuristic checker's
own resolved charge at the same call site" and assert equality. *This is
the single highest-leverage verification step in the rollout* — it proves
the new inference matches 7 rounds of hard-won heuristic ground truth
before it can affect a single accept/reject decision. *Acceptance:* 100%
agreement across the full corpus, or every mismatch triaged and fixed
*here* (never downstream).

**Stage 3 — dual-run enforcement, opt-in, hard-fail-on-disagreement.**
Behind the Stage-0 flag, run the new type-driven subset check alongside
the existing heuristic checker; require identical accept/reject on every
call site, with disagreement treated as a compiler-internal error (never
silently resolved either way). Re-run every `tests/security/attack_*.kry`
and both live-repro files named in this document's header (round-7 param-
alias, actor-state-forloop-alias) under both checkers. *Acceptance:* new
checker independently rejects every historical attack file with the OLD
checker's rejection code path temporarily disabled for that one file (the
"prove both ways" rule applied to the migration itself, file by file, not
assumed from the design).

**Stage 4 — new projects opt in by default.**
`kryos new` scaffolds `[experimental] cap_types = true`. Existing projects
unaffected. *Acceptance:* scaffold test updated; zero regression to the
existing (opt-out-by-default) corpus.

**Stage 5 — stdlib migration, module by module (66 modules).**
Each module lands as an independent, dual-run-gated commit. Per §4, the
overwhelming majority needs no hand annotation; only genuinely `pub`
HOF/trait-object surfaces get an explicit `@{...}` for documentation value.
Tracked as a checklist in `tools/loop/LEDGER.md`, mirroring the existing
deny-by-default tracker. *Acceptance per module:* dual-run agreement,
zero new `.kry` source edits required for internal (non-`pub`) code.

**Stage 6 — self-host compiler + full ecosystem (259 sources) under
dual-run, plus scale/cost proof.**
Add a synthetic depth-2048 nested-function-type fixture (existing parser
recursion-depth limit) since `CapRow` now recurses alongside `params`/
`ret` at every nesting level — re-verify, don't assume. Measure `kryos-
loop gates` wall-clock on the self-host compiler before/after; target
**<10% regression**, a checkable number, not "should be fine." Grep
generated LLVM IR / Cranelift IR for any residual runtime reference to a
capability-set value — expected zero hits — proving §9's zero-ABI claim
rather than asserting it.

**Stage 7 — flip the global default; delete the heuristic checker.**
The type-driven check becomes primary; the old checker is demoted to a
documented fallback for any residual the type system doesn't cover, then
deleted incrementally (§6.1's list), each deletion gated by N consecutive
green dual-run cycles at 100% agreement before that piece's fallback role
is removed. *Acceptance:* full 4-suite gate chain green with the old
checker's entry point compiled out entirely; `checker.rs` line count
measured and reported (expected large net decrease per §6.1, not assumed).

**Compatibility invariant enforced at every stage:** omitted `@{...}` means
infer, so zero existing `.kry` source requires a textual change to keep
compiling at any stage before Stage 5 chooses to add documentation-value
annotations on `pub` surfaces.

**What genuinely breaks, stated honestly (not "should be zero"):** not
annotation burden, but *new type errors surfacing where none existed* —
specifically an array/collection literal mixing closures with different
concrete rows (`[reader, pure_fn]`). §2.3's union-at-unification rule
(`Closed(a)` unify `Closed(b)` when neither is a bare var → `Closed(a|b)`,
not a hard-equality error) resolves this soundly (over-approximates to the
exact union of every element actually placed, same direction as the old
`Unknown -> all` fallback but now exact instead of maximal) and should make
this case *more* permissive than today, not less. This must be the first
thing measured against the 91-example corpus in Stage 2, exactly as the
roadmap's own earlier round measured "74/91 on first pass" before finding
the general fix — do not assume 91/91 on first pass here either.

---

## 9. Cost

**Compile-time.** One more field on `Type::Function`, unified/substituted
alongside `params`/`ret` at every existing unification/instantiation site
— proportional to existing type-inference cost, not a new pass. The
`CapBits` bitset makes row-union/subset O(1) machine words, cheaper than
the `HashSet`-based `CapabilitySet` operations the old checker runs today.
Measured, not assumed — Stage 6's <10% wall-clock budget is the concrete
gate.

**Runtime/ABI: none, stated loudly.** `@{...}` is erased before codegen
exactly like `@capabilities(...)` is today — nothing in the capability
model consults capabilities at runtime; the whole mechanism is static.
`CapVarId`s are type-checker-internal bookkeeping, fully resolved by the
time `kryos-mir` lowering runs (§5), never reaching codegen, never needing
a runtime tag, discriminant, or vtable slot beyond what `dyn Trait`
dispatch already carries. Stage 6's IR grep is the acceptance proof. If any
implementation step finds itself needing a runtime capability tag to make
this work, that is a deviation from this spec and a stop-and-reconsider
signal — a struct-ABI change has already defeated unrelated prior attempts
in this codebase, and this design must not become another one.

---

## 10. What this design does not guarantee

- **The raw-memory TCB split** (`RAW_MEMORY_BUILTINS`) stays the existing
  separate, non-propagating, root-module-only pass — deliberately not
  migrated, because folding it in would defeat the entire point of that
  carve-out (propagation must NOT happen for these primitives).
- **`extern`/FFI to non-`kryos_*` native symbols** stays name-based,
  unaffected — externs aren't first-class Kryos values with inferred
  bodies.
- **`dyn Trait` stored inside a container** remains the existing `E0110`
  limitation; if lifted independently later, §5's `dyn` handling composes
  with it for free, but this spec does not lift it.
- **Runtime-value-dependent capability requirements** ("this closure needs
  `fs:read` only if some boolean is true at runtime") are not supported —
  Kryos's capability model, old or new, is fully static/structural. Not a
  regression; a real limit of the whole approach.
- **Explicit, hand-named row-polymorphism syntax on a `pub` signature**
  (writing something like `fn() -> str @ {C}` to document "generic in my
  caller's capability" on a trait method, the way the principled-effects
  draft's `C: CapSet` bound would allow) is not part of this design. It
  costs nothing to add later as an optional, backward-compatible surface
  extension once the inference-only mechanism has shipped, but it is
  explicitly out of scope here — building it in from the start is exactly
  the annotation-burden tradeoff §4/Judgment rejected.
- **Row-variable self-recursion in a row-polymorphic HOF's own recursive
  definition** (a fixed-point-combinator-shaped HOF whose accumulator
  changes capability row across recursive calls) is not worked through
  here. Ordinary recursive-function type inference presumably already has
  some answer for a self-recursive function's own type variables (a
  pre-declared signature or an occurs-check-guarded fixed point); the
  row-variable case should reuse whatever that answer is, but this is an
  open item for implementation time, with an explicit-annotation escape
  hatch (`@{...}` written by hand breaks the cycle) if inference cannot
  close it.
- **The dual-run transition window's fallback checker** (§8, Stages 3-6)
  is, by definition, exactly as precise/imprecise as the old heuristic
  checker for whatever it's still covering during that window — an
  accepted, temporary, bounded cost of a staged rollout, not a permanent
  gap.
- **This is a design specification, not a mechanized proof.** Every claim
  above of the form "X becomes impossible by construction" needs
  re-verification live during implementation, the same way every prior
  round's claims were — by reverting, rebuilding, confirming the leak
  reappears, restoring, rebuilding, confirming it's gone. Nothing in this
  document is "done" until that evidence exists.

---

## Summary of concrete deliverables per crate

| Crate | Change |
|---|---|
| `kryos-ast` | `TypeExpr::Function.caps: Option<Vec<(String, Span)>>` |
| `kryos-parser` | Parse optional `@{...}` suffix on function-type expressions |
| `kryos-types` | `CapBits`, `CapVarId`, `CapRow`; `Type::Function.caps: CapRow`; row unification/instantiation/resolution threaded through `unify`/`instantiate`/`resolve`/`instantiate_sig`/`fresh_var` via a parallel `cap_substitutions` map; per-declaration row-variable solving (§2/§4) |
| `kryos-mir` | Thread `CapRow` substitution through existing monomorphization; verify erasure before codegen (Stage 6 IR grep) |
| `kryos-capabilities` | `CapabilitySet::{from_bits,to_bits}` bridge (§1.4); new type-driven subset check (§6) replacing `check_callee_capabilities`'s body; dual-run harness (Stage 3); incremental deletion of the heuristic resolvers (§6.1, Stage 7) |
| stdlib (66 modules) | Mostly zero-touch (inferred); explicit `@{...}` only on true `pub` HOF/trait-object surfaces documenting a ceiling |
| `tools/loop/LEDGER.md` | Stage checklist, mirroring the existing deny-by-default tracker |

No implementation performed; no compiler rebuild performed, per task
instruction. Files read to ground this spec (all at
`C:\Users\Krist\projects\active\kryos-lang`): `docs/capability-roadmap.md`,
`docs/capability-soundness.md`, `CLAUDE.md`,
`compiler/crates/kryos-ast/src/types.rs`,
`compiler/crates/kryos-types/src/ty.rs`,
`compiler/crates/kryos-types/src/infer.rs`,
`compiler/crates/kryos-capabilities/src/model.rs`,
`compiler/crates/kryos-capabilities/src/checker.rs` (size only, 5,921
lines).
