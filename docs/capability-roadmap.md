# Capability enforcement: current state and roadmap

Kryos's defining feature is a compile-time capability system: a function's
authority over the outside world (network, filesystem, other processes, the
environment, the database, the terminal) is part of its type, checked and
propagated through the call graph. This page states honestly where enforcement
is **on by default**, where it is **opt-in**, and the plan for closing that gap.

This page has two parts. **Part 1** (below) is the design for the SOUND
long-term model — capability-typed fn values — and an honest account of why
three successive rounds of enumerating dangerous syntactic shapes could not
converge, which is the reason this design exists. **Part 2** (further down)
is the pre-existing deny-by-default rollout tracker (examples, self-host,
ecosystem, the global default), unrelated to the fn-value-laundering
problem and still accurate as written.

## Part 1 — Why three enumeration rounds failed, and the fix

### The failure, stated precisely

The capability checker's job, for any given call site, is: does the callee
require authority the caller does not hold? For a call to a NAMED function
this is easy — look up the function's own declared/inferred capability set.
The hard case is a call through a first-class **fn-value** — a closure, or a
function reference, stored in a parameter, a local, a container, a message
payload — because the callee's IDENTITY isn't known just from its name; it
has to be traced back to whatever closure was actually constructed.

Three rounds of this project attacked that tracing problem the same way:
enumerate every SHAPE through which a closure's authority could travel
(parameter → local → return → passthrough call → actor message → `spawn` →
generic instantiation → `dyn Trait` → struct field → array element → map
value → nested combinations → mutation after construction → a HOF whose
callback forwards a caller-supplied fn-value), and add a rule that traces
that specific shape. Each round closed everything it found. Each round, new
shapes were found immediately after — because a **whitelist of known-
dangerous shapes can never be complete when the attacker chooses the
shape**. This is not a claim in the abstract: the concrete history is

- Round 1 closed fn-value laundering via parameter, local, return,
  passthrough, actor, spawn, generic, dyn.
- Round 2 closed LITERAL-constructed containers, and — audited honestly —
  found that the previous round's claim about container mutation ("fails
  closed") was FALSE: a container built empty then populated via `push`/
  index-assign kept a STALE snapshot and silently asserted "no authority
  here", worse than the documented fallback.
- Round 3 closed push/map-insert mutation and a HOF whose callback is a
  NAMED function forwarding a caller-supplied fn-value. Two NEW bypasses
  were found and confirmed live within the same session: an INLINE lambda
  invoking its own bound parameter through a HOF (`map(tools, |f| f())` —
  the named-function case doesn't cover an anonymous one), and a closure
  read out of a container into an INTERMEDIATE LOCAL before being called
  (`let arr = m["k"]; arr[0]()` — chaining directly was traced, the extra
  level of local-variable indirection broke the trace).
- A fourth pass (this one), auditing the checker's ENFORCEMENT layer rather
  than trusting the "closed" list, found the enumeration's actual root
  architecture: every rule up to this point fired only when a call's
  callee could be looked up BY NAME in a table keyed on named functions
  (`hot_params`). A call whose callee was a bare local variable, with
  **zero** indirection — no parameter, no container, not even a second
  function call in between (`let r = make_secret_reader(path); r()`) — was
  never evaluated by anything, at any point across all three rounds,
  because nothing ever inspected the callee of a call unless it resolved
  to a name already known to be "hot". The same gap applied to a struct
  field read and invoked via method-call syntax (`reg.reader()`) with no
  intervening function call.

The pattern across all four rounds: **the analysis enumerates the shapes it
has thought of, and any shape it has not enumerated fails OPEN** (no
capability requirement is inferred at all, not even the conservative
`Unknown` fallback the design documents claimed). For a security property
this is unsound by construction, independent of how many rounds run.

### The fix shipped this session: invert the default

Rather than adding a fifth shape, the default was inverted: a call through a
first-class fn-value whose capability set cannot be **statically proven** to
be a subset of the caller's grant is now **rejected** (requires
`Capability::All`), not silently treated as requiring nothing. Unknown means
deny. This is sound regardless of which syntactic shapes exist or which
ones get found next, because soundness stopped depending on the enumeration
being complete.

Concretely (`kryos-capabilities/src/checker.rs`):

- `resolve_direct_invoke_caps` / `resolve_method_field_invoke_caps`: at
  every call site whose callee is NOT a resolvable named function, builtin,
  extern, actor constructor, or enum variant constructor, resolve what
  fn-value is actually being invoked (a local, a parameter, a container
  read) and require its authority directly — `Known` → exact set,
  `DependsOnParam` → deferred to the enclosing function's own call sites
  (unchanged, sound forwarding), `Unknown` → `Capability::All`.
- This runs at BOTH the enforcement layer (`check_callee_capabilities`,
  the `MethodCall` arm of `check_expr`) and the inference layer
  (`collect_caps_expr`'s `FnCall`/`MethodCall` arms), so an interior
  helper's OWN inferred capability set correctly reflects a direct
  invocation inside its body, not just a boundary function's.
- The failure mode inverts from a silent security hole to **over-
  rejection**: safe, visible (a compile error naming the call site), and
  fixable (annotate, or restructure so the value is traceable).

### The relief: precise resolution where possible, transparency for HOFs

A blanket `Unknown → all` for every unresolved fn-value call would make the
language unusable — `std::iter::map`/`filter`/`fold`/`reduce`/`find` and
any user-written HOF take a callback parameter, and an inline lambda
invoking its own bound parameter (`|f| f()`) is exactly what's needed to
map over an array of pure closures (a real, tested pattern —
`conf_generics.kry`'s `map(fns, |f| f(10))`). Measured before deciding how
to relieve it: reverting to a strict `Unknown → all` broke the full example
corpus from 91/91 to 74/91 on first pass; each failure was traced to a
specific, principled, general fix rather than a name-based carve-out:

1. **Precise resolution wherever the value's provenance IS traceable.** A
   closure bound to a local via a NAMED function call, a lambda literal, a
   container LITERAL, or (new this session) a field/index chain read out of
   a container and bound to an intermediate local, resolves to its EXACT
   capability set (often empty, for a pure closure) rather than falling to
   `Unknown`. A curried/chained call (`f(a)(b)(c)`) resolves by recursing
   through each application. A nested named function (`fn adder(y) {..}`
   declared inside another function, which the parser desugars to
   `let adder = fn(y) {..}`) — including a SELF-RECURSIVE one — resolves
   instead of tripping the fail-closed default on its own name.
2. **A NAME-collision fix that the fail-closed default exposed.** Three
   different stdlib functions are all named `find`
   (`std::iter::find`/`std::re::find`/`std::string::find`); the checker's
   internal maps are keyed by bare name (a documented, previously-harmless
   simplification). Before this fix, a WRONG declaration's parameter list
   could silently get substituted for `find`'s own during inference,
   which cost nothing when the mechanism only ever contributed authority
   optimistically. Under fail-closed, a bare-name collision made an
   unrelated declaration's params leak into `find`'s own analysis and
   forced `all`. Fixed by making the per-declaration own-parameter list
   travel WITH the function body being analyzed, instead of being
   re-looked-up by name afterward, for every OWN-parameter computation.
3. **Actor and enum-variant constructors are not fn-values.** `Account()`
   (an actor constructor) and `Some(x)`/`None()`/`Ok(x)`/`Err(e)` (enum
   variant constructors) use `Name(args)` call syntax but are not
   references to first-class functions; both are now tracked and excluded
   from the fail-closed treatment (they were never enumerated as "callable
   things" before because nothing needed to know).
4. **Transparent-forwarding lambdas — ROUND 4's shape-based version, now
   REMOVED, see "Round 5" below.** Round 4 originally resolved an inline
   lambda whose ONLY capability-relevant behavior is invoking its own bound
   parameter (`|f| f()`) against "whichever OTHER argument at the SAME call
   site the HOF iterates", identified by matching the callback's declared
   element type against another parameter's declared container type at the
   SAME declaration, first match wins. **This was unsound** — the exact
   same class of bug as rounds 1-3 (inferring authority from a SHAPE rather
   than proven data flow), just relocated to a new shape — and was deleted,
   not patched, once found. See "Round 5" below for what replaced it.

With these four in place, the example corpus is back to 91/91 (matching
the pre-fix baseline exactly), the full conformance suite (58/58),
`tests/security_gate.sh` (51 checks, including the two round-3 residuals
and 9 additional shapes found while hardening this session — direct local
invocation with zero indirection, struct-field method-call-shaped direct
invocation, chained and intermediate-local container reads,
`std::collections` Deque/Dict wrapper methods, `Option`/`Result` payloads,
a user-defined non-stdlib HOF, three-level container nesting, and
`--capabilities-mode=permissive`), and the self-host bootstrap (16/16) are
all green. See `tools/loop/LEDGER.md` for the measured before/after and the
exact commits.

### The honest residual

A container built from a genuinely NON-LITERAL source — returned from
another function, mutated inside a callee, or read out of ANOTHER
container in a way even the intermediate-local extension above cannot
follow — still resolves to `Unknown` and requires `all`. This is, and
always was, the documented conservative fallback: it fails CLOSED
(over-strict, safe), not open. It is a precision limit of a checker with no
real type-flow analysis, not a soundness hole — closing it precisely is
exactly what Part 2 below (capability-typed fn values) is for.

### Round 5 — the shape heuristic failed too, plus an unrelated scope hole

Round 4's "honest residual" section above turned out not to be the only
gap. Two more were found and fixed in round 5, both in
`kryos-capabilities/src/checker.rs`; see `tools/loop/LEDGER.md` for the full
write-up and the live repros.

**5a. The transparent-forwarding-lambda relief (item 4 above) was itself
shape-based, and shape-based inference has now failed on this exact axis
TWICE.** `find_companion_container_arg` matched a callback's declared
element type against another parameter's declared container type,
first-match-wins — an attacker passing a REAL privileged container as one
argument and an EMPTY DECOY of the identical declared shape as another made
the decoy win the match and the real container go uncharged. Deleted
outright, with no shape-based successor. The replacement,
`hot_param_companions`, is grounded in DATA FLOW instead of shape: for a
function's own directly-invoked hot callback parameter, it records — from
that function's OWN fixed source, not from anything a caller supplies —
which other parameter's element the function's body actually passes into
the callback at its internal call site (`map`'s `f(arr[i])` proves the
callback runs on `arr`'s elements, regardless of what type any OTHER
parameter happens to declare). A decoy argument at a call site cannot
change a fact about the callee's own, already-compiled body. Where no
single companion can be proven (disagreeing internal call sites, or an
argument that isn't traceable to another own-parameter), the position falls
back to `Capability::All` — the same policy as every other unresolvable
case in this checker, no exceptions carved out for convenience.

**5b. The "defer to my own caller" rule — present since round 1 — was
unsound whenever the deferring function narrows its OWN scope with `deny!`
between receiving the value and invoking it.** `fn outer(reader:
fn()->str) -> str { deny!(fs:read) { return zero_cap_tool(reader) } }`,
called from an `@capabilities(fs:read)` caller, leaked the secret with no
decoy, no generic, and no container involved — the plainest possible
forward. The deferral assumes the eventual outer call site is checked
against a scope at least as narrow as the one the deferred value is
actually used under; a `deny!` inside the SAME function breaks that
assumption, because the outer call is checked against the function's
wider ENTRY scope. Reproduced identically through a free function, an
`impl` method, and an actor message handler receiving the closure as a
message argument — this is not method- or actor-specific. Fixed with
`current_fn_entry_scope_depth`: every deferred-charge decision now checks
whether the live scope stack is DEEPER than it was when the checker
entered the current function/actor boundary; if so, a `deny!` (or any
future scope-narrowing construct) is active between entry and this call,
the deferral cannot be trusted, and `Capability::All` is required instead.
This scope check deliberately does NOT apply to the unrelated, purely
STRUCTURAL self-classification `resolve_closure_caps` runs on a fresh
lambda literal's own body (to decide whether it needs anything beyond
transparently forwarding its own parameter) — that classification is a
property of the literal alone and must stay scope-independent, or every
`map`/`filter` call over providably pure closures would spuriously require
`all` merely for running inside ANY `deny!` block, even one narrowing a
totally unrelated capability. `transparent_lambda_params` and
`structural_lambda_eval_depth` carve out exactly that sub-computation.

Both fixes were verified BOTH ways (revert, rebuild, confirm the exact
leak reappears; restore, rebuild, confirm it's gone again), and against a
battery of variants beyond the two minimal repros above: the decoy as a
MAP companion, a decoy read out of another container rather than a fresh
literal, three-or-more containers at once, the same decoy shape against
`any`/`all`/`partition`/`flat_map`-style siblings, a method receiver with
more than two array parameters, and the "defer" hole reached through an
actor message handler, a `spawn` capture, and a `dyn Trait` method — all
now REJECTED, gated in `tests/security_gate.sh`.

## Part 1b — The sound long-term model: capability-typed fn values

> The sketch below is now expanded into a full, implementable specification
> at [docs/capability-effects-spec.md](capability-effects-spec.md) —
> representation, inference, subtyping, row-polymorphism for HOFs, every
> language-feature interaction, a walk of the five named historical attacks
> through the new design, migration staging, cost, and explicit scope
> limits. Read that document for anything beyond the summary here.

Enumerating shapes cannot converge because the checker is trying to
recover, after the fact, information a real type system would have
carried with the value in the first place. The structurally sound fix is
to make a function's capability requirement part of its **type**, so
authority travels WITH the value through any container, alias, HOF, or
payload automatically — the same way Rust's `Send`/`Sync` or an effect
system's effect row travels through generic code without the compiler
having to special-case every container shape a `Send` value might sit in.

### Syntax

```kryos
// A function type now optionally carries a capability set after `@`.
fn() -> str @ {fs:read}
fn(i64) -> bool                      // no `@{...}` -- requires nothing (today's default)
fn(str) -> str @ {fs:read, fs:write}

// A declared function's inferred/declared set becomes part of its TYPE,
// not just a side-table the checker consults by name:
@capabilities(fs:read)
fn make_secret_reader(path: str) -> fn() -> str @ {fs:read} {
    return || file_read(path)
}
```

The set `{fs:read}` is a normal part of the type expression, so it
participates in every place a type already does: a variable's declared
type, a struct field's type, an array's element type, a function
parameter's type, a generic instantiation, and a `dyn Trait` method's
signature.

### Inference (avoiding an annotation burden)

The `@{...}` annotation on a function type is almost always **inferred**,
not written by hand, mirroring how `let` locals infer their type today:

- A lambda literal's inferred fn-type carries its body's actual capability
  requirement as computed today (`collect_caps_expr`), attached to the
  type rather than discarded once used.
- A named function's declared return type `fn(...) -> T @ {...}` is
  filled in by the compiler from the function's own inferred/declared set
  UNLESS the programmer writes it explicitly (in which case it is checked,
  not just trusted — a function whose body needs more than its written
  `@{...}` claims is a compile error, the same relationship `@capabilities`
  has to a function's own body today).
- Only a PUBLIC API surface needs the annotation written by hand, and only
  when its capability set is the interesting part of the contract
  (a plugin `Tool` interface declaring `fn() -> str @ {fs:read}` so a
  caller can see the ceiling on a trait object without reading every impl).

The generic-type-parameter substitution mechanism Kryos already has
(monomorphization in `kryos-mir`) is the SAME mechanism that already
substitutes `T` for a concrete type at each instantiation; the capability
set becomes one more piece of substituted metadata, unified the same way a
type variable unifies today — no new inference ALGORITHM, an extension of
the existing one.

### Subtyping: fewer capabilities is a subtype of more

A `fn(...) -> T @ {fs:read}` is usable anywhere a `fn(...) -> T @ {fs:read,
fs:write}` is expected (a function needing LESS is safely substitutable
for one permitted to need MORE) — the standard contravariant-in-
requirements subtyping rule, mechanically identical to how `net` already
satisfies a required `net:http` in the existing coarse/sub-capability
model (`Capability::satisfies`). Concretely:

```kryos
fn apply(f: fn() -> str @ {fs:read}) -> str { return f() }

@capabilities(fs:read)
fn reader() -> str { return file_read("x") }
fn pure_fn() -> str { return "constant" }   // inferred @ {} (empty)

apply(reader)   // OK: {} subset? no -- reader is @{fs:read}, matches exactly
apply(pure_fn)  // OK: pure_fn is @{} <: @{fs:read} (needs less than the param allows)
```

A caller passing a fn-value whose OWN capability set exceeds what the
parameter's type declares is a compile error at the CALL SITE — the same
`E0507` shape used today, just checked against the value's real TYPE
instead of a best-effort trace.

### Interaction with generics, `dyn`, `spawn`, and the stdlib

- **Generics.** `fn map<T, U, C: CapSet>(arr: [T], f: fn(T) -> U @ C) ->
  [U] @ C` — the callback's capability set becomes a genuine generic
  parameter, unified from the ACTUAL closure argument at each call site,
  exactly like `T`/`U` are today. `map`'s own declared/inferred set stays
  whatever it already is (typically empty); the RETURN type's `@ C`
  correctly reflects that the resulting array's later use (if it also
  stores closures) carries the same requirement forward, closing the
  "transparent forwarding" case structurally instead of via the
  `hot_param_companions` data-flow-tracing stopgap Part 1 uses today.
- **`dyn Trait`.** A trait method's signature carries its `@{...}` set as
  part of the vtable slot's type; a `dyn Trait` object built from a
  concrete impl requiring MORE than the trait's declared ceiling is
  rejected at the `dyn` coercion site (the existing attenuation check,
  applied to a new type dimension). This is where capability-typed fn
  values pay for themselves the most: today, a closure inside a `dyn`
  payload is one of the documented residuals (containers holding `dyn`
  are largely unimplemented per CLAUDE.md's known limitations); with the
  capability living in the TYPE, the vtable's own signature is the
  enforcement point, no runtime tag or call-site tracing needed.
- **`spawn`.** A spawned closure's capability requirement is checked
  against the spawning scope's grant at the `spawn` site (already true
  today via actor/spawn attenuation); with capability-typed fn values this
  becomes a plain subtyping check on the captured closure's TYPE rather
  than a separate `check_spawn_expr` code path.
- **The stdlib.** `std::iter`'s HOFs need NO signature change to stay
  capability-free for pure callers — `fn map<T, U>(arr: [T], f: fn(T) ->
  U) -> [U]` (no `@{...}` written) continues to mean "requires nothing OF
  ITS OWN, and infers/propagates whatever `f`'s actual type carries",
  which is exactly today's inferred-empty behavior, now backed by the
  type rather than a per-call heuristic. `std::collections`'
  `List<fn(...) -> T @ C>`-shaped generic containers get the SAME
  propagation for free through the existing generic-field substitution
  machinery (`struct_fields_for`), closing the residual gap documented in
  the "std::collections wraps its backing array in a struct" section
  above without a bespoke "transparent accessor" special case.

### Migration path

1. Add `@{...}` as an OPTIONAL suffix to `TypeExpr::Function` in the parser
   and AST (backward compatible: absent means "infer", not "requires
   nothing" — critical so existing `.kry` source needs zero changes).
2. Extend `kryos-types`' function-type unification to carry and unify the
   capability set alongside parameter/return types (the type-checker
   crate, not `kryos-capabilities` — this is why the fix belongs in the
   TYPE system, not the capability checker bolted on afterward).
3. Extend monomorphization (`kryos-mir`) to substitute the capability set
   the same way it substitutes `T`/`U` today.
4. Once function types carry their capability set, MOST of
   `kryos-capabilities/src/checker.rs`'s closure-tracing machinery
   (`resolve_closure_caps`, `resolve_container_path_caps`,
   `build_local_closure_caps`, `build_local_container_lits`, `hot_params`,
   `hot_param_companions`, `accumulate_hot_extra_caps`, and
   `resolve_direct_invoke_caps`) becomes REDUNDANT — replaced by an ordinary
   subtyping check against the
   already-resolved type at each call site. This is a large NET DELETION
   of checker complexity, not just an addition.
5. Ship BOTH systems side by side during migration: the type-driven check
   runs first (precise, whenever type info is available); the existing
   fail-closed heuristic checker remains as the fallback for any residual
   the type system doesn't yet cover (dynamically-constructed capability
   sets, if ever needed; incremental rollout across the stdlib before
   every signature is annotated).

### Honest scope estimate

This is a type-system change, not a checker patch, and should NOT be
rushed alongside a security fix (per this task's own instruction). Rough
sizing based on the surfaces actually touched: `kryos-ast` (1 new field on
`TypeExpr::Function`, ~1 day), `kryos-parser` (parse the `@{...}` suffix,
~1 day), `kryos-types` (thread capability sets through unification,
generic substitution, and trait/dyn signature matching — the bulk of the
work, ~2-3 weeks including the interaction with the existing generic
monomorphization pipeline), `kryos-mir`/codegen (no runtime representation
needed — capabilities are compile-time-only, erased before codegen exactly
like today, ~2-3 days to verify erasure doesn't leak), `kryos-capabilities`
(delete the now-redundant heuristic machinery once the type system covers
its cases, incremental, net negative LOC), stdlib annotation pass (mostly
automatic via inference; hand-annotating genuinely public trait-object
surfaces, ~1 week), and a full regression sweep against the security gate
+ conformance + bootstrap suites (~1 week). Total: **6-8 weeks** for a
careful, TDD, both-backends-verified rollout — not a task to fold into a
security-fix wave.



## Part 2 — Deny-by-default rollout tracker

## What is enforced today

- **The capability model is real and precise.** Each builtin maps to the exact
  authority it needs (`file_read` → `fs:read`, `http_get` → `net:http`,
  `tcp_connect` → `net:tcp`, `exec` → `process`, ...). Coarse capabilities
  grant their sub-capabilities; the reverse is rejected. Declaring less than
  you use is a compile error (`E0505`); a caller that does not declare what a
  callee needs is a compile error (`E0507`).
- **Self-scoped operations are ambient, by design.** `exit`/`abort` (terminate
  your own process) and clock reads / `sleep` are *not* gated — they cannot
  read your data, reach another system, or take unauthorized action. Gating
  them would be noise, not safety. (An agent that stalls or exits is bounded by
  `@budget`, a separate mechanism.) This mirrors how Rust's `process::exit` and
  WASI's `proc_exit` treat self-termination.
- **Under `--strict-capabilities`, enforcement is complete and load-bearing.**
  Every public example and showcase program (81/81) carries least-privilege
  `@capabilities` declarations and passes strict checking in CI. Stripping a
  program's annotations produces violations — the checker genuinely enforces,
  it is not decorative.

## Deny-by-default is here for new projects

There are now three enforcement modes (`permissive` < `inferred` < `strict`),
selectable with `--capabilities-mode` or `[capabilities] mode` in `kryos.toml`.
**`kryos new` scaffolds `mode = "inferred"`, so a fresh project is
deny-by-default from the first line of code** — both `kryos check` and
`kryos build` reject an unannotated `main` that (directly or transitively) uses
a gated builtin.

`inferred` mode is what makes deny-by-default *ergonomic*: you declare authority
once at the boundary (`main`, or a `pub` API function), and the compiler infers
every interior helper's capability set from its call graph. A ~15k-line program
needs one annotation on `main`, not one per function (the self-host compiler
itself is inferred-clean with a single `@capabilities(fs:read, fs:write,
process)` on `main`). Enforcement is sound for every DIRECT path authority can
take (direct builtin calls, method/static dispatch, a gated builtin passed BY
NAME as a first-class value) **and for closure/fn-value indirection**
through a parameter, a `let`-bound local, a return value, a chain of
passthrough calls, an actor message send, `spawn`, a generic instantiation,
`dyn Trait` dispatch, OR a CONTAINER (a struct field, an array element, or a
map value that holds a closure, including nested combinations like a struct
field holding an array of closures) — the checker resolves the SPECIFIC
closure argument at each call site (tracing through a container's
field/index structure when applicable) and requires its authority there, so
it is no longer invisible just because it flows through a value instead of a
name. See [docs/10-capabilities.md § Closure indirection, including
containers](10-capabilities.md#closure-indirection-including-containers-is-sound-read-this-if-you-are-trusting-it-with-secrets)
for the full sound surface and the one documented residual (a container
populated from a non-literal source requires `all`, the same conservative
fallback used for every other unresolvable closure provenance).

The `strict` mode (`--strict-capabilities`) remains for maximum scrutiny —
every function auditable in isolation for authority it invokes by name OR by
a resolvable fn-value argument (direct or via a container). 91/91 examples
pass it in CI (live count via `bash tests/strict_caps_examples.sh`), and
`tests/security_gate.sh` gates the closure-laundering fix directly (reject /
no-over-reject / no-cascade, plus a positive "still gated when privileged"
check, for both the direct and container shapes) rather than relying on the
example corpus to exercise it.

## The global default is now `inferred`

As of v1.0.0-beta.3 the **bare-compiler default** is `inferred`: a loose
`kryos run foo.kry` / `kryos check` / `kryos build` with no flag and no
`kryos.toml` is deny-by-default. A program that reads a file, hits the network,
or reads the environment from an unannotated `main` is rejected at compile time.
`--capabilities-mode=permissive` (on `check` and `build`) or
`[capabilities] mode = "permissive"` opts out.

Staged plan, done in order:

1. **Examples exemplary** — done. 81/81 strict; 106/106 `inferred`.
2. **New projects deny-by-default** — done. `kryos new` → `mode = "inferred"`.
3. **Self-host + stdlib capability-clean** — done. The self-host compiler is
   inferred-clean with a single boundary annotation on `main`; the stdlib
   authority wrappers are annotated so their capabilities propagate.
4. **Flip the global default to `inferred`** — done (beta.3).

### Nothing is checked permissively anymore

The `ecosystem/` packages were the last surface checked under `permissive`.
Every package entry point now declares its capabilities and the ecosystem gate
runs under the compiler default (inferred): 253/253 clean, with a handful of
intentional negative fixtures (`leaky_io`, `leak_config`, ...) excluded by
design because they exist to demonstrate a leak. **The compiler, `kryos new`
projects, the examples, the self-host compiler, and the ecosystem packages are
all deny-by-default.** The only code checked permissively is the internal
test-harness fixtures that verify compilation/runtime rather than capability
hygiene, and illustrative doc snippets.

## Migrating a program

```bash
kryos check --capabilities-mode=inferred src/main.kry   # deny-by-default
kryos check --strict-capabilities src/main.kry          # every fn must declare
```

Each error names the builtin and the capability it needs. In `inferred` mode you
usually only annotate `main` — the error names the exact (transitive) set, e.g.
`call to `do_install` requires capabilities [fs:read, fs:write]`. Add
`@capabilities(fs:read, fs:write)` to `main` and re-check; interior helpers need
nothing. In `strict` mode, annotate each function that calls a gated builtin.
See [docs/10-capabilities.md](10-capabilities.md) for the full model.
