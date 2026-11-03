# Capabilities

> **Measured status (2026-11-02) - read this before anything else on this page.**
> `bash tools/loop/escape_status.sh` re-runs every committed capability-escape repro under both
> enforcement modes and prints the real count: **33 still escape, 220 are rejected (253 repros)**
> (`tools/loop/check-docs-truth.sh` fails CI if the README or the LEDGER preamble drifts from that
> number). Every escape still live is named in [tools/loop/LEDGER.md](../tools/loop/LEDGER.md) with its
> measured routing in [tools/loop/ESCAPE-ROUTING.md](../tools/loop/ESCAPE-ROUTING.md). They fall in
> six groups: a closure stored into a container the callee was handed, where the container sits in a
> tuple, `Option`, `Result` or enum (item 119) or the store is reached through a factory result, a
> generic bound or a `dyn` (item 123); `deny!(fs)`, `deny!(io)` and `deny!(all)`, which the row layer
> does not match (item 121); the body of a trait default method, which the type checker never checks
> (item 122); the `deny!` verdict missing a function declared above the callee it hands a closure to
> or a store made by a callee declared below its caller (items 124, 125), or a `deny!` inside a `select`
> arm, which the type checker never visits (item 126); and a closure returned by a top-level function
> that uses a captured parameter through a helper or alongside another one (item 127); and the order the shape layer builds its per-name map in, which a
> loop-carried rebinding of an alias or of a local closure and a later lambda `let` of the same name get past (item 129); and a fn-typed field of the result of a call
> through a local closure, invoked in place, which the shape layer holds no type for (item 131; the row layer rejects it wherever it judges the `deny!`). **A count of 0
> would not mean there are no escapes.** The corpus is adversarial shapes found by directed search,
> so a green count is a floor, not a soundness proof, and the shape nobody has thought of is the one
> that matters. Today capabilities are a strong development-time discipline; they are not yet a
> boundary to run untrusted code behind. Where this page says a shape is "rejected" or the section
> below says "sound", it means that shape is checked and gated, not that no other shape leaks.

> **Implementation Status (revised after fail-closed hardening — read this
> before trusting the model with a secret):** The `@capabilities(...)`
> annotation is parsed and the compile-time capability checker
> (`kryos-capabilities` crate) is implemented. It enforces: functions must
> declare capabilities matching the stdlib modules and builtins they use,
> child scopes cannot exceed parent capabilities (attenuation), and extern
> blocks require the `ffi` capability. The actual capability variants are:
> `net` (coarse), `net:http`, `net:tcp`, `io`/`fs` (coarse, aliases),
> `fs:read`, `fs:write`, `ffi`, `compute`, `crypto`, `process`, `env`, `term`,
> `db`, `time`, `all`. Three enforcement modes are implemented —
> `permissive`, `inferred` (deny-by-default with interior inference), and
> `strict` — selectable via `--capabilities-mode` or `[capabilities] mode` in
> `kryos.toml`; `kryos new` defaults new projects to `inferred`.
>
> **Three successive hardening rounds each closed the shapes reported and
> each time new bypasses were found** (parameter/local/return/passthrough/
> actor/spawn/generic/dyn in round 1; literal-constructed containers in
> round 2; push/map-insert mutation and HOF-forwarded named functions in
> round 3) — because the underlying design enumerated known-dangerous
> SHAPES and treated anything it didn't recognize as requiring nothing. Two
> more bypasses were found immediately after round 3 (an inline lambda
> invoking its own parameter through a HOF, and a closure read out of a
> container into an intermediate local before being called), plus several
> more during the fix below (the single most basic case — a closure built
> and called directly in one function, with **zero** indirection — was
> uncatchable by any of the three rounds, because none of them evaluated the
> callee of a call at all unless it was a call BY NAME to a function already
> known to be "hot").
>
> **The default was inverted (round 4) to close every enumerated shape at
> once, but the relief mechanism that inversion shipped WITH was itself
> unsound, and a follow-on audit (round 5) found a second, unrelated
> soundness hole in the "defer to my own caller" mechanism every round back
> to round 1 has relied on. Read both before trusting this with a secret.**
>
> **Round 4** made the default itself sound: a call through a first-class
> fn-value whose capability set cannot be STATICALLY PROVEN to be a subset
> of the caller's grant now REQUIRES `Capability::All` rather than silently
> requiring nothing — "unknown" means "deny", not "allow". This closes every
> enumerated shape, plus every shape nobody had found yet, because soundness
> no longer depends on the enumeration being complete. But round 4 shipped
> ONE relief mechanism to keep ordinary `std::iter`/HOF usage
> annotation-free: an inline lambda that merely forwards its own bound
> parameter into a HOF (`map(tools, |f| f())`) was resolved against
> "whichever other argument at the same call site STRUCTURALLY supplies its
> elements" — matched by comparing the callback parameter's DECLARED element
> type against another parameter's DECLARED container type, first match
> wins. **This was the SAME class of bug as rounds 1-3, just relocated**: an
> attacker passes the REAL container carrying a privileged closure as one
> argument and an EMPTY DECOY container of the identical DECLARED SHAPE as
> another — the decoy wins the shape match and contributes nothing, and the
> real container is never charged. Confirmed live, `--strict-capabilities`,
> inside a `deny!(fs:read)` block where a direct `file_read` is correctly
> rejected: a generic `apply_to_second<T>(decoy: [T], real: [T], f: fn(T) ->
> str) -> str { return f(real[0]) }` leaked the secret with the required
> capability computed as empty.
>
> **Round 5's fix has two parts, both in `kryos-capabilities/src/checker.rs`:**
>
> 1. **Deleted the shape-based relief** (`find_companion_container_arg`) and
>    every doc claim describing it, with no shape-based replacement — shape
>    inference has now failed on this exact axis (declared-type matching)
>    twice. The ONLY relief implemented instead is `hot_param_companions`:
>    genuine per-DECLARATION data-flow tracing. For a hot callback parameter
>    invoked directly inside a function's own body (`map`'s `f(arr[i])`),
>    the checker records which of that SAME function's OTHER parameters the
>    call's actual argument expression decomposes to (`arr`, via
>    `decompose_container_path` — the same syntactic decomposition already
>    trusted everywhere else in this file). This is a property of the
>    callee's OWN, FIXED source and cannot be influenced by which argument a
>    CALLER supplies at any position, so a decoy at the call site cannot
>    change the answer — unlike the removed heuristic, which matched on the
>    CALL SITE's declared argument types, not on what the callee's body
>    actually does with them. If no single companion can be proven (multiple
>    internal call sites disagreeing, or an argument that doesn't decompose
>    to another own-parameter), the position falls back to requiring
>    `Capability::All` — no approximation, no guess.
> 2. **A second, independent, unrelated hole found while auditing every
>    other place authority gets deferred rather than charged:** the
>    long-standing rule "a hot argument that resolves to one of the CURRENT
>    function's own parameters defers the charge to THAT function's own call
>    sites" (present since round 1, needed so an ordinary passthrough HOF
>    never needs `all`) is only sound when the call site that eventually
>    supplies the parameter's real value is checked against the SAME scope
>    this invocation is running under. It is NOT sound when the CURRENT
>    function has narrowed its OWN scope with a `deny!` block between
>    receiving the parameter and invoking it: the outer call site is checked
>    against this function's wider ENTRY scope, not the narrower one
>    actually in effect at the point of invocation. Confirmed live, no
>    decoy, no generic, no container — a plain, direct forward: `fn
>    outer(reader: fn()->str) -> str { deny!(fs:read) { return
>    zero_cap_tool(reader) } }`, called from an `@capabilities(fs:read)`
>    caller, compiled clean and printed the secret from inside the denied
>    scope, identically whether `outer` is a free function, an `impl`
>    method, or an actor message handler receiving the closure as a message
>    argument. Fixed via `current_fn_entry_scope_depth`: the checker now
>    tracks the scope-stack depth at the moment it entered the function/actor
>    boundary currently being checked, and any deferred-charge decision
>    checks whether a `deny!` has pushed the live scope DEEPER than that
>    depth since — if so, the deferral is unsound and `Capability::All` is
>    required instead of empty. (A companion field,
>    `transparent_lambda_params`/`structural_lambda_eval_depth`, keeps this
>    scope check from firing on the UNRELATED, purely structural
>    self-classification `resolve_closure_caps` performs on a fresh lambda
>    literal's own body — that classification must stay scope-independent,
>    or ordinary `map`/`filter` usage over provably pure closures would
>    spuriously require `all` merely for running inside any `deny!` block at
>    all, for a totally different, unrelated capability.)
>
> See [`docs/capability-roadmap.md`](capability-roadmap.md) for the full
> analysis of why shape-based enumeration cannot converge, and the sound
> long-term design (capability-typed fn values) that would close the
> residual precision gaps (a container from a genuinely non-literal source
> still requires `all`) without any of this heuristic machinery.
>
> Runtime enforcement, audit logging, and sandboxing APIs described in some
> earlier drafts are **not yet implemented** (enforcement is entirely
> compile-time). Note: `env_get`/`env_set` require the `process` capability
> (reading the environment can exfiltrate secrets), not ambient. `kryos
> audit` is a separate, purely SYNTACTIC scan of `@capabilities(...)`
> annotations — it never runs capability inference, so it never lists any
> unannotated function (a legitimately-polymorphic helper like a
> `std::iter` HOF just as much as anything else); do not read its output as
> a complete capability inventory.

Capabilities are Kryos's security model. Every function declares exactly what system resources it needs -- filesystem access, network connections, process spawning, FFI calls. If a function tries to use something it did not declare, the program fails at compile time. Not at runtime, not with a warning -- it does not compile.

This is the opposite of how most languages work. In JavaScript or Go, any function can open a file, make a network request, or call `eval`. You only find out about unauthorized access when something goes wrong in production. Kryos inverts that: you see every capability a program uses before you run it.

## The model: three enforcement modes

Enforcement has three modes, selected with `--capabilities-mode=<mode>` on
`kryos check` / `kryos build`, or per project via `[capabilities] mode` in
`kryos.toml`. `kryos new` scaffolds `mode = "inferred"`, so **new projects are
deny-by-default from day one.**

| Mode | What it does | When |
|---|---|---|
| `permissive` | Only functions carrying a `@capabilities(...)` annotation are checked. An unannotated function is unconstrained. Capabilities are advisory. | Opt-in for scratch/legacy code via `--capabilities-mode=permissive`. |
| `inferred` | **Deny-by-default at the boundary, with interior inference.** `main` — and any annotated function — must actually hold every capability its code *transitively* uses. Interior helpers need no annotation: the compiler infers each one's capability set as the union of what it and its callees require. So an unannotated `main` that (directly or through helpers) writes a file is rejected: declare `@capabilities(fs:write)` on `main`. **Recommended, and the `kryos new` default.** | New projects; production code. |
| `strict` | Every function is checked as if annotated with exactly its declaration (empty unless declared). Every DIRECT gated builtin call from an unannotated function is an error, and so is a call through an fn-typed argument (including one read out of a struct field / array element / map value) that carries authority the caller doesn't hold. This is `--strict-capabilities`. | Maximum scrutiny; security-critical libraries. |

The key property of `inferred` mode: reading `main`'s annotation is *meant*
to tell you the program's entire authority. For direct paths — direct
builtin calls, method/static dispatch (`obj.write()`), a gated builtin
passed BY NAME as a first-class value (`apply(file_write, ...)`) — this
holds: all are accounted for. It also holds when authority flows through an
ordinary closure: a function returning `|| file_read(path)` and calling it
through an unnamed `fn`-typed parameter elsewhere is tracked back to the
SPECIFIC closure argument at each call site (not the callee's own,
correctly-empty, declaration), so `main`'s declared/inferred set still has
to cover it — and this now ALSO holds when that closure is stashed in and
read back out of a container (a struct field, array element, or map value):
see "Closure indirection, including containers" below for the full checked
surface.

```bash
kryos check --capabilities-mode=inferred src/main.kry   # deny-by-default
kryos check --strict-capabilities src/main.kry          # every fn must declare
kryos build --release --capabilities-mode=inferred src/main.kry
```

In a project, set it once in `kryos.toml` (a fresh `kryos new` already does):

```toml
[capabilities]
mode = "inferred"
```

## Closure indirection, including containers, is checked (read this if you are trusting it with secrets)

**If you are evaluating Kryos for a use case where capability enforcement is
the thing standing between untrusted code and a secret (an embedded agent
that must not exfiltrate a password, an API key, a token), read this section
first. It applies in EVERY mode, including `--strict-capabilities`.**

The checker (`kryos-capabilities/src/checker.rs`) resolves the authority
carried by a first-class function VALUE — not just a directly-named function
or builtin — at every call site that invokes it, and attributes that
authority to the CALL, not to the (correctly, permanently) empty declaration
of a generic forwarding function like `zero_cap_tool` below or `std::iter`'s
HOFs. This closes the laundering path for: a closure passed as a parameter
and invoked directly; one traced through a `let`-bound local back to the
function that constructed and returned it; one forwarded through several
layers of passthrough functions; one sent as an actor message argument
(fire-and-forget send) or handed to `spawn`; one flowing through a generic
`fn<T>`; one dispatched through a `dyn Trait` method; AND one stored in and
read back out of a CONTAINER — a struct field (`registry.reader()`), an
array element (`readers[0]()`), a map value (`handlers["x"]()`), or a nested
combination of these (a struct field holding an array of closures).

```
@capabilities(fs:read)
fn make_secret_reader(path: str) -> fn() -> str {
    return || file_read(path)
}

// Zero capabilities declared -- correctly, same as std::iter::map/filter/fold.
// Calling `reader()` no longer defeats enforcement: the checker resolves
// WHAT reader() carries at each call site and requires it there.
fn zero_cap_tool(reader: fn() -> str) -> str {
    return reader()
}

@capabilities(fs:read)
fn main() {
    let reader = make_secret_reader("secret.txt")
    deny!(fs:read) {
        println(zero_cap_tool(reader))   // REJECTED (E0507): the closure carries
                                          // fs:read, which this block just denied.
    }
}
```

**The container shape is the same violation, closed the same way.** A
plugin registry, a router table, a command dispatch map, or an event-handler
list is typically written as a struct field, array, or map of closures — the
shape a "local agent with tools" architecture actually uses — so this was
the residual the product story depended on:

```
struct Registry { reader: fn() -> str }

fn zero_cap_tool(reg: Registry) -> str {
    return reg.reader()          // container-drilling invocation: TRACED
}

@capabilities(fs:read)
fn main() {
    let r = make_secret_reader("secret.txt")
    let reg = Registry { reader: r }
    deny!(fs:read) {
        println(zero_cap_tool(reg))   // REJECTED (E0507): the closure carries
                                       // fs:read, traced through the struct field.
    }
}
```

See `tests/security/cap_escape_closure_launder_container.kry`,
`..._array.kry`, `..._map.kry`, and `..._nested.kry` for the live repros
(struct field, array element, map value, and a struct field holding an array
of closures respectively) — all four are REJECTED with E0507 and gated in
`tests/security_gate.sh` (checks #7-10). A struct/array/map "registry" of
PURE closures (no capability) still needs no annotation (`tests/
security_gate.sh` check #11) — the fix does not cascade into the legitimate
version of this exact pattern.

**CORRECTION to a previously-published claim on this page: the "populated
via `push` in a loop" case above was NOT sound.** An earlier version of this
section claimed such a container "cannot be statically traced, and the
checker requires `Capability::All` from the caller" — i.e., that it failed
CLOSED like every other unresolvable shape. That claim was never verified
against a live repro and was FALSE: the container-literal tracker
(`build_local_container_lits`) only ever looked at the container's INITIAL
`let` binding, so `let mut tools = []` followed by the canonical growable-
container idiom `tools = push(tools, reader)` (or `m["k"] = reader`, or
`arr[i] = reader`, or a struct field mutated after construction) kept the
STALE initial snapshot forever — usually an EMPTY literal. The checker did
not fall back to `Unknown`; it found a (stale) literal and confidently
resolved it to `Known(empty)`, requiring **nothing at all**. This is a live,
silent bypass, strictly worse than the documented conservative fallback,
and it is the shape a plugin registry, router table, command-dispatch map,
event-handler list, or agent tool list is actually BUILT with in practice —
exactly the architecture this language is positioned for.

**Fixed.** `build_local_container_lits` now also walks `Stmt::Assign`
(`apply_container_assign`/`rebuild_container_write` in
`kryos-capabilities/src/checker.rs`): `X = push(X, v)`, a map/array
index-assign, and a struct field-assign reaching an ALREADY-tracked
container are rebuilt precisely (the write is spliced into the tracked
literal, matching the read side's existing index-insensitive union
semantics for array/map writes). Any OTHER reassignment shape the tracker
cannot precisely characterize — an unrelated function call, a compound
assignment, a write whose path doesn't match the literal's actual shape —
now INVALIDATES the tracked entry instead of leaving it stale, so it
correctly falls through to the sound `Unknown` -> `Capability::All`
fallback described below. See `tests/security/
cap_escape_closure_launder_push.kry`, `..._map_insert.kry`,
`..._index_assign.kry`, `..._field_mutate.kry`, `..._nested_push.kry`, and
`..._map_of_arrays.kry` for the six live repros (all closed, all gated in
`tests/security_gate.sh` checks #12-19), plus a no-cascade check (#20)
proving a registry of PURE closures built the same mutating way still needs
zero annotation.

**A separate, deeper gap in the SAME session: `std::collections`
(`List`/`Stack`/`Queue`/`Deque`/`Dict`) wraps its backing array/map in a
struct and exposes it through METHOD calls (`.push(v)`, `.get(i)`,
`.peek()`, ...), not direct field/index syntax** — invisible to the
hot-parameter detector regardless of the mutation-tracking fix above,
because it only recognizes direct `field`/`index` access chains, not a call
through a helper method. Also fixed: the checker now recognizes a
"transparent accessor" method — one whose every return path is itself a
self-rooted field/index chain (`List.get`'s `return self.data[index]`,
modulo a bounds-check branch that never returns) — keyed by `(struct name,
method name)` specifically because `List.get` and `Dict.get` live in the
same stdlib module and return DIFFERENT paths; a bare-name key would let one
silently clobber the other (verified live during development). Generic
instantiation is threaded through too, so `List<fn() -> str>`'s declared
`data: [T]` field is recognized as function-bearing for THIS instantiation,
not just when a struct's field is literally typed `fn(...)`. Even with the
parameter correctly marked hot, `List.new()`/`.push()` build the list via
method calls that construct a fresh struct literal INSIDE their own bodies —
invisible to the caller-side literal tracker by design — so this shape
resolves to the conservative `Unknown` -> `Capability::All` fallback rather
than a precise capability, which is the correct, fail-CLOSED outcome. See
`tests/security/cap_escape_closure_launder_stdlib_collection.kry` (gated,
check #12-19 loop).

**A third, independent gap: a closure reaching a container's authority
through a HOF, when the HOF's callback is a NAMED function that itself
forwards a caller-supplied fn-value** (`fn invoke(f: fn() -> str) -> str {
return f() }` handed to `map(tools, invoke)`, rather than an inline lambda)
**— also fixed.** A bare reference to a named function used to be attributed
that function's own declared/inferred capability set unconditionally, which
says nothing about what a HOT parameter it forwards might carry once
applied. The checker now falls back to `Unknown` for such a reference. This
was measured for cascade against the real codebase (zero occurrences of a
bare-identifier HOF callback anywhere in `compiler/stdlib`, `compiler/
self-host`, or `examples` — every real callback is an inline lambda) and the
functionally-equivalent lambda-wrapped form was ALREADY conservatively
rejected before this fix (a pre-existing, unrelated restriction on a
lambda's own bound parameter invoking a hot-forwarding function inline), so
this closes an inconsistency rather than restricting previously-working
code. See `tests/security/cap_escape_closure_launder_hof_forward.kry`.

**One residual, by design, unchanged from every other unresolvable shape,
and now VERIFIED (not just claimed) to actually fail closed:** a container
built from a genuinely NON-LITERAL source — returned from another function,
passed as a parameter and mutated inside the callee, or read out of ANOTHER
container in a way the tracker can't follow — cannot be statically traced,
and the checker requires `Capability::All` from the caller in that case (the
same conservative fallback already used for every other closure whose
provenance can't be proven, e.g. one selected by a runtime condition). This
is not a silent gap: it fails CLOSED (over-strict, never under-strict) and
is the honest limit of static analysis, not an overlooked case. Live-tested
during this session: a container returned from a function, one passed as a
parameter and mutated inside the callee, and one only reachable through a
closure that itself captures it, all correctly require `all` rather than
silently requiring nothing.

**The naive alternative was measured, not assumed, and confirmed unusable:**
requiring `Capability::All` on any call through a non-directly-resolvable
fn-typed value (the blanket fix) forces essentially every `std::iter` HOF
call site and every callback-shaped stdlib API to declare `all` — verified
directly against `std::iter::map/filter/fold` and the self-host compiler,
which use these pervasively (CLAUDE.md gotchas #19-20). The shipped fix
instead resolves the SPECIFIC closure argument (and, for a container, the
SPECIFIC field/element path invoked) at each call site — legitimate HOF and
registry usage with a pure closure needs no annotation change
(`tests/security_gate.sh` checks #5-6 and #11 guard both directions: no
cascade, and a privileged closure through the same HOF/registry is still
correctly gated).

**Status, superseded by the fail-closed hardening below: this section
described the state after three enumeration rounds, each of which closed
the shapes reported and each time new bypasses were found in a shape the
enumeration had not covered — including, immediately after this section was
last "complete", an inline lambda invoking its own parameter through a HOF
and a closure read out of a container into an intermediate local (see
`docs/capability-roadmap.md` for why enumerating shapes here does not
converge). The checker was changed to invert the default instead: ANY call
through a first-class fn-value whose capability set is not STATICALLY
PROVEN to be a subset of the caller's grant is now REJECTED
(`Capability::All` required), regardless of the syntactic shape — closing
every enumerated shape above, the two residuals found after it, and every
shape nobody has attacked yet, in one structural change rather than a
fourth round of patches. A container from a genuinely non-literal source
(a function return, a mutated parameter, a container read out of another
container) was already documented as failing CLOSED under the old design
and still does under the new one, for the same reason. See
[`tools/loop/LEDGER.md`](../tools/loop/LEDGER.md)
for the full history of all fixes.

**Narrowed residual, found by the ASSAULT round 3 real-program sweep
(2026-08-07) and CLOSED (2026-08-08): a closure stored in a container BOUND
FROM A FACTORY FUNCTION'S RETURN, not a struct/array/map LITERAL, then read
back out and invoked DIRECTLY in the SAME function as the `deny!()`
narrowing it** — `let registry = build_registry()` followed by
`registry[idx].handler(args)` inside `deny!(fs:read) { .. }`, the ordinary
way to write a plugin registry / router table / command dispatch map.
Distinct from the container-as-PARAMETER shapes above (tracked via the
parameter's own DECLARED type, which doesn't care whether the caller's
container came from a literal or a factory call): this is a purely LOCAL-
variable shape, and `resolve_method_field_invoke_caps` (the resolver for
`obj.method(...)` when `method` is actually a fn-typed struct FIELD) only
ever recognized a root tracked as a LITERAL by `local_container_lits`
— when the root was bound from any other expression, it returned
`CapabilitySet::empty()` (charges nothing) instead of the sound
`Unknown -> all` default every other unresolvable shape in this file uses.

Closed two ways, layered:
1. **Precise, for the common case:** `fn_return_container_lits` traces a
   ZERO-PARAMETER factory function's own return statement(s) back to a
   struct/array/map literal built inside its body (including one built
   incrementally via `push`/index-assignment, and one referencing the
   factory's own local variables — a small, bounded-depth local-inlining
   substitution resolves those too), then splices that literal into the
   CALLER's `local_container_lits` as if the caller had bound it directly.
   This resolves the REAL, PRECISE capability of whatever the factory
   actually built (e.g. `[fs:read]`, not a blunt `all`) and is why a
   registry containing ONLY pure closures still needs no annotation at all
   — no cascade.
2. **Fail-closed fallback, for what can't be traced:** when the factory
   function has parameters, or its construction can't be resolved to a
   literal this way, `resolve_method_field_invoke_caps` falls back to a
   STATIC-TYPE check (`local_container_types`, resolved from an explicit
   `let` annotation or the called function's declared return type) — if the
   type positively confirms `method` names a genuine `fn(...)->...`
   field/element, the call requires `all` rather than nothing.

**Known, documented precision gap, NOT a soundness gap:**
`resolve_container_path_caps`'s `Index` step is INDEX-INSENSITIVE BY DESIGN
(it unions every element/value's authority rather than tracking which
concrete index is read) — this is pre-existing and shared by the already-
closed container-AS-PARAMETER case, not introduced by this fix. A registry
that mixes pure and privileged entries in the SAME array/map therefore
charges the union to EVERY index into it, not just the privileged one —
sound (never permits an escape) but imprecise (over-rejects a benign index
into an otherwise-mixed registry). An ALL-SAFE registry (no privileged entry
anywhere) is unaffected and still compiles clean. See
`tests/security/cap_escape_closure_launder_local_registry_index_field.kry`'s
own header comment for a live-verified example of this trade-off.

**The type checker's capability rows close the same family independently
(2026-11-01, LEDGER root R2).** The checker infers a capability row for every
function value, and `deny!` rejects a block whose row contains a denied
capability. That row layer used to drop the second closure stored into any slot:
a literal registry was typed by its first element, and the first closure stored
into a struct field, an enum payload, an annotated binding or a generic container
pinned its row. A slot is now the union of every closure stored into it, whether
the store is a literal element, a `push`, an index or field assignment, a
constructor argument, a `return` or an arm of an `if` / `match` (the value is one of
several function values), or (through a helper such as
`register(reg, name, handler)`, `reg.add(name, handler)` or `put(reg, g)`) a call,
including a call that stores into an array or map the caller handed it. A
function's parameter row is an input of its body: the body can narrow neither the
row of a closure it was passed nor the element row of a container it was handed,
so a function that builds a container from a pure function and then stores its own
closure parameter into it (`return [safe, g]`) is charged for the argument. Two
costs, both deliberate: a struct field's row is shared by every value of that
struct in the program, and a lambda's annotated parameter row is shared by every
call of that lambda, so a registry that mixes a pure and a privileged plugin (or a
lambda called with both) is judged by the union (a `deny!` of that capability that
only calls the pure one is rejected). Rejected: the
`tests/security/attack_slot_*.kry` programs (E0110). Accepted: the
`tests/security/ctl_slot_*.kry` controls, including a pure-only registry and a mixed
registry read inside a `deny!` of a capability none of its closures uses.

**The `deny!` verdict is taken when the whole module has been checked (2026-11-02,
LEDGER root R3).** A block is judged on the row its body was charged, and that row
mentions things that are bound later: the row of a parameter of the function that
holds the block (each caller supplies it), a struct field, a global or a local
array that is filled by a store further down the file or later in a loop, and a
helper or callee declared below the code that uses it. The checker used to read the
bits of the row at the block and drop whatever was still unbound, so a block that
called a closure it was handed, or read a registry filled afterwards, was judged to
need nothing. It now files the row, judges it when nothing can bind any more, and
judges it again at every reference to the function that holds the block with that
reference's argument: `outer(reader)` is rejected at the `deny!` inside `outer` when
`reader` reads a file, and `outer(pure)` is not. A helper that hands its parameter
on (a relay, a recursion, a method chain) passes the obligation on. Three costs,
all deliberate: the verdict does not depend on the order of the declarations (a
privileged `Plugin { run: .. }` literal below a `deny!` that reads another registry
of `Plugin`s now counts, exactly as it did when it sat above); a block inside a
method of a trait impl that depends on one of its parameters is judged on `all`,
because a generic bound or a `dyn` reaches the method through the trait; and a
function nothing references is not judged on its parameters (it cannot run). Still
open: the body of a trait default method is never checked (LEDGER item 122), and a
helper that stores a closure into a container it was handed is not followed through
a factory result, a generic trait bound or a `dyn` (item 123), or when the container
sits inside a tuple, an `Option`, a `Result` or an enum (item 119); `deny!(fs)`,
`deny!(io)` and `deny!(all)` are not matched against a used `fs:read` by this layer
(item 121); a function declared above the callee it hands a closure to, a store made by a callee
declared below its caller (items 124 and 125) and a `deny!` inside a `select` arm (item 126) are
not seen by it either. Rejected: the `tests/security/attack_deny_*.kry` programs (E0110).
Accepted: the `tests/security/ctl_deny_*.kry` controls.

**A call through a closure bound to a local is charged what the closure returns (2026-11-02,
LEDGER item 77, shape layer).** After `let setter = |r| { r }` the value of `setter(reader)` is `reader`,
so `f = setter(reader)` carries `reader`'s authority, not the (empty) authority of invoking `setter`.
The shape layer used to answer a call through a local with the local's own invocation authority, so
`f()` inside `deny!(fs:read)` read the file whenever the row layer was not the one judging the block
(the row layer rejects the original repros, items 77a and 77b, and every shape below wherever it judges
the block; a `deny!` inside a `select` arm is not judged by it, item 126). A closure bound to a local now
carries a return flow: the closures its body creates or names and returns (their flat authority), the
arguments it returns (replaced by the authority of the argument at each call), the parameters of an
enclosing closure or function it captures and returns, the locals it hands back (looked up when the call
is resolved, so a rebinding after the definition counts), and what it cannot trace. When the only thing
it returns is a closure literal, the flow of that closure is kept too, so `mk(a)(b)`, `let inner = mk(a)`
then `inner(b)` and `make_getter(reader)()` (item 128: a top-level function returning a closure that
returns its captured parameter) are charged what applying the result hands back, four closures deep. The
same holds for an alias of a local closure or of a top-level function and for a name bound to two
closures. What is not traced is `all`: a recursive nested `fn`, a parameter bound to a block local before
it is returned (`let q = r` then `return q`), a closure received as a parameter. Two costs, measured on
the pure twin of each shape and recorded in the LEDGER: the first two of those are rejected even when
every argument is pure. Rejected: the `tests/security/attack_shape_*.kry` programs (E0507; each puts its
`deny!` in a `select` arm so that only this layer judges it). Accepted: the
`tests/security/ctl_shape_*.kry` controls, `tests/conformance/conf_curried_generic_closure.kry` and
`tests/security/attack_generic_nested_function_type_param.kry` (a generic `wrap<T>` applied to its own
result). Still open at this layer: a closure returned by a top-level function that uses a captured
parameter through a helper, or two captured parameters (item 127).

**Every return of a closure counts (2026-11-02, LEDGER item 130), and a container read through a local
closure is traced only for a container nothing changes or aliases (item 132).** The first version of the
change above charged a call through a local closure what its visible `return`s carried: a `return r` inside
a non-final `match`, a `select` arm, a `deny!` block, a bare or `unsafe` block, an `if` or `match` bound by
`let`, or a call argument was never listed, so such a closure was charged nothing (the independent verifier
of that change ran five programs that printed the secret, and master refused each of them only by charging
the closure's own authority). Every `return` outside a nested lambda is listed now, for local closures, top-level
functions, transparent accessors and factory literals alike. A value read out of a container
(`id(reg.items[0])`, or a lookup closure `|i| { items[i] }`) and handed through a local closure is charged what
invoking the same read directly is charged when the container is a local the program never changes or
aliases: bound once by a `let` of a literal (or of a call to a factory without parameters whose returned literal
is known), read only through field and index reads whose values are named functions, builtins, lambdas, data literals
or stable function-valued locals (`let r = make_reader("literal")`, or a closure that computes from literals and its own
parameters), plus `len` and `for`. Any other read stays `all`, which is also what a named function that returns such a read
is charged and what a container PARAMETER's element is charged at the call (the pure call and the leaking call
of the same function get one answer: `fn pick(items, i) { let id = |g| { g }  return id(items[i]) }`). Rejected:
`tests/security/attack_shape_*hidden_return*.kry`, `attack_named_fn_hidden_return_direct.kry`,
`attack_select_arm_return_plain_deny.kry`, `attack_accessor_hidden_return.kry` and
`attack_shape_container_*.kry` (E0507). Accepted: the `ctl_shape_container_*.kry` controls and the
`ctl_shape_*hidden_return*_pure.kry` twins. Still open at this layer: a fn-typed field of the result of a call
through a local closure, invoked in place (item 131).

See `tests/security/cap_escape_closure_launder_local_struct_field.kry`,
`..._local_registry_index_field.kry` (the array-of-struct-field flagship
shape), `..._local_map_of_struct_field.kry`, `..._local_array_direct.kry`,
`..._local_map_direct.kry`, `..._local_nested_field_array.kry`, and
`..._local_nested_two_hop_field.kry` for the seven repros (all REJECTED,
both enforcement modes), and the two `..._control_benign.kry` siblings for
the no-cascade proof — all nine gated in `tests/security_gate.sh`.

Under strict mode, a pure function like this is fine -- it calls no capability-gated builtins:

```
fn square(x: i64) -> i64 {
    return x * x
}
```

But calling `file_read` in an unannotated function is a compile error:

```
fn bad_function() -> str {
    return file_read("secret.txt")
}
```

Error output:

```
error[E0505]: builtin `file_read` requires `fs:read` capability
 --> src/main.kry:2:12
  2 |     return file_read("secret.txt")
   |            ^^^^^^^^^^^^^^^^^^^^^^^^ requires `fs:read`
  = note: add `@capabilities(fs:read)` to the enclosing function or actor
```

## Declaring Capabilities

Use the `@capabilities(...)` attribute on a function:

```
@capabilities(fs:read)
fn load_config(path: str) -> str {
    return file_read(path)
}
```

For pure computation -- math, string manipulation, data structures -- no annotation is needed. `println` and `print` are also ambient (no capability required).

For real I/O, declare what you need:

```
@capabilities(net:http)
fn fetch_data(url: str) -> str {
    return https_get(url)   // https_get; not http_get (does not exist as a builtin)
}

@capabilities(fs:read)
fn read_config(path: str) -> str {
    return file_read(path)
}
```

### Combining Capabilities

A function can declare multiple capabilities:

```
@capabilities(net:http, fs:write)
fn download_to_file(url: str, path: str) {
    let data = https_get(url)
    file_write(path, data)
}
```

## All Capability Types

### net (coarse)

Grants all network sub-capabilities: `net:http` and `net:tcp`. Use the narrowest sub-cap that fits.

### net:http

HTTP(S) client and server operations.

```
@capabilities(net:http)
fn fetch(url: str) -> str {
    return https_get(url)
}
```

### net:tcp

Raw TCP connections, TLS, and unix-domain sockets.

```
@capabilities(net:tcp)
fn connect(host: str, port: i64) {
    tcp_connect(host, port)
}
```

### io / fs (coarse)

File I/O -- grants both `fs:read` and `fs:write`. `io` and `fs` are aliases for the same coarse capability (back-compat: `io` is the legacy spelling). Use the narrower sub-cap when possible.

### fs:read

Read-only file access.

```
@capabilities(fs:read)
fn load(path: str) -> str {
    return file_read(path)
}
```

### fs:write

Write, create, and mutate files and directories.

```
@capabilities(fs:write)
fn save(path: str, data: str) {
    file_write(path, data)
}
```

### process

Process spawning and environment variable access. Environment variables are gated here because they can contain secrets. The top-level `env_get` builtin requires this capability. Process spawning (`exec`, `spawn_process`) and `env_set` are modeled under this capability and surface through the `std::process` stdlib module.

```
@capabilities(process)
fn get_home() -> str {
    return env_get("HOME")
}
```

Note: `exit` and `abort` terminate the current process only and are **ambient** -- no capability required (same philosophy as Rust's `process::exit`).

### env

Reserved for future use as a narrower environment-variable-only split from `process`. Currently `env_get` / `env_set` map to `process`.

### ffi

Foreign function interface -- calling code written in other languages. Required on `extern` blocks.

```
@capabilities(ffi)
extern {
    fn my_c_function(x: i64) -> i64
}
```

### compute

Heavy computation including GPU dispatch and SIMD intrinsics. Pure arithmetic, string manipulation, and data structure operations are ambient and do not require `compute`.

### crypto

Cryptographic operations: hashing, signing, encrypting.

```
@capabilities(crypto)
fn hash_data(data: str) -> str {
    return sha256(data)
}
```

### term

Terminal control -- raw mode, cursor positioning, terminal size queries. The `term` capability type is recognized in `@capabilities(...)` and enforced on `use std::term::*` imports. Top-level `term_*` builtins are defined in the model but surface through the `std::term` stdlib module rather than as standalone builtins.

### db

Database access -- queries and transactions via the db stdlib module.

### time

System clock access. Currently the `time` variant is reserved for a future deterministic-execution mode. `time_now` and `sleep` are **ambient** today -- no capability required. (`time_millis` is defined in the model for future use but is not currently a top-level builtin; use `time_now` instead.)

### all

Grants every capability. Use only in top-level entry points or trusted shells. Auditable: declaring `all` is visible in every code review and capability audit.

## The Capability Hierarchy

Capabilities narrow downward -- they never elevate. If a function has `@capabilities(fs:read)`, any function it calls can have `fs:read` or less. It cannot call a function that requires `fs:write`.

```
@capabilities(fs:read)
fn safe_load(path: str) -> str {
    return file_read(path)
}

@capabilities(fs:write)
fn save(path: str, data: str) {
    file_write(path, data)
}

@capabilities(fs:read)
fn process_config(path: str) {
    let data = safe_load(path)        // OK -- same capability
    // save(path, data)               // COMPILE ERROR -- would elevate
}
```

Coarse caps satisfy their sub-caps (back-compat). A function declaring `@capabilities(net)` may call both `https_get` (needs `net:http`) and `tcp_connect` (needs `net:tcp`). The reverse does not hold: `net:http` does not grant `net:tcp`.

## Compile-Time Enforcement

The capability checker runs as a static analysis pass before codegen. Violations are errors, not warnings.

Example: calling `file_read` inside a function that declared only `net`:

```
@capabilities(net)
fn bad_function() -> str {
    return file_read("secret.txt")
}
```

Actual error output:

```
error[E0505]: builtin `file_read` requires `fs:read` capability
 --> src/main.kry:3:12
  3 |     return file_read("secret.txt")
   |            ^^^^^^^^^^^^^^^^^^^^^^^^ requires `fs:read`
  = note: add `@capabilities(fs:read)` to the enclosing function or actor
```

The error code `E0505` means "builtin requires a capability not in the declared set." Propagation errors (calling a function that has more capabilities than the caller) use `E0507`. All capability error codes (`E0501`-`E0507`) are explainable via `kryos explain <code>`.

## Builtin Function Capability Requirements

**Ambient (no capability required):**
`println`, `print`, `eprintln`, `len`, `range`, `to_string`, `abs`, `type_of`, `parse_int`, `parse_float`, `str`, `sqrt`, `sin`, `cos`, `tan`, `log`, `pow`, `floor`, `ceil`, `min`, `max`, `assert`, `push`, `pop`, `substr`, `contains`, `char_code`, `exit`, `abort`, `time_now`, `sleep`, `file_exists`

**fs:read** (or coarse `io`/`fs`):
`file_read`, `read_file`

**fs:write** (or coarse `io`/`fs`):
`file_write`, `write_file`, `create_dir`, `remove_file`, `remove_dir`, `copy_file`, `rename_file`

**net:http** (or coarse `net`):
`https_get`, `http2_get`, `http2_post`, `http2_request`

**net:tcp** (or coarse `net`):
`tcp_connect`, `tcp_listen`, `tcp_accept`, `tcp_send`, `tcp_recv`, `tls_server_config`, `tls_accept`, `tls_send`, `tls_recv`, `tls_close`, `uds_connect`, `uds_bind`, `uds_accept`, `uds_send`, `uds_recv`, `uds_close`

**net** (coarse only -- straddles connect + protocol):
`pg_connect`, `pg_exec`, `pg_query`, `pg_close`, `ws_accept_key`, `ws_encode_text`, `ws_encode_binary`, `ws_encode_close`, `ws_encode_ping`, `ws_encode_pong`, `ws_unmask`, `ws_read_frame`

**process**:
`env_get` (top-level builtin); `env_set`, `exec`, `spawn_process` are recognized by the capability checker but surface through the `std::process` stdlib module rather than as standalone builtins

**term** (via `std::term` module -- not standalone builtins):
`term_clear`, `term_raw_mode`, `term_size`

**crypto**:
`sha256`, `sha512`, `random_bytes`, `hmac_sha256`

## Real-World Patterns

### Web server with minimal privileges

```
@capabilities(net:tcp, fs:read)
fn serve(port: i64) {
    let config = file_read("config.toml")
    tcp_listen("0.0.0.0", port)
    // Can read files for config, can accept connections.
    // Cannot write files, cannot spawn processes.
}
```

### Data pipeline with read-only input

```
@capabilities(fs:read)
fn load_data(path: str) -> [str] {
    let raw = file_read(path)
    return raw.split("\n")
}

@capabilities(fs:write)
fn save_results(path: str, results: [str]) {
    file_write(path, join("\n", results))
}

@capabilities(io)
fn pipeline(input: str, output: str) {
    let data = load_data(input)
    // ... process data ...
    save_results(output, data)
}
```

The `load_data` function cannot accidentally write to disk. The `save_results` function cannot read arbitrary files. Each function has exactly the access it needs.

### Pure computation (no annotation needed)

```
fn transform(xs: [i64]) -> [i64] {
    // No annotation needed -- only uses ambient builtins.
    let mut out: [i64] = []
    for x in xs {
        out = push(out, x * 2)
    }
    return out
}
```

## Why This Matters

Most security vulnerabilities come from code doing something it was never intended to do. A logging library that makes network calls. A template engine that reads arbitrary files. A math utility that spawns processes.

Capabilities make these violations impossible. When you look at a function's `@capabilities` declaration, you know exactly what it can do. When you audit a program, you get a complete map of every capability used by every function.

This is not just defense -- it is documentation. Reading `@capabilities(fs:read)` tells you more about a function's behavior than reading its entire implementation.
