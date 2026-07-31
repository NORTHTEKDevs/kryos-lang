# Kryos production ledger

The queue survives context loss; a session does not. Update this file in the
SAME commit as the work. Anything not written here is lost.

Ranked by SERIOUSNESS FOR THE INTENDED USE CASE, not by which gate is red.
Kryos is deployed as capability-attenuated infrastructure for agent tooling,
so: (breaks the capability/trust model) > (silent wrong answer) > (blocks a
green CI) > (leak) > (papercut). A silent wrong answer outranks a crash — a
crash announces itself. A trust-model hole outranks both: nothing above it in
the stack can be sound if the boundary leaks.

---

## THE LOOP

```
preflight -> select -> REPRODUCE -> bisect -> fix -> prove -> gate -> push -> ledger
```

Run `tools/loop/kryos-loop.sh preflight` first, every time. Then:

1. **SELECT** the top unblocked item below.
2. **REPRODUCE** before forming any hypothesis. `kryos-loop.sh repro <file>`.
   *No theory is allowed before a reproduction exists.* Three attributions were
   wrong this way in one session — the `@copy` arm, a "merge interaction", and
   the `param_src` branch — each from reading code instead of measuring it.
3. **BISECT MECHANICALLY.** Commit-level (`git cherry-pick` onto a worktree) or
   program reduction (cut the input until the symptom vanishes). Never
   hand-edit a hypothesis in and call the result evidence: one such edit
   silently removed more than intended and produced a confident wrong answer.
4. **FIX.**
5. **PROVE BOTH WAYS.** The test must FAIL without the fix. A gate that cannot
   fail is not a gate. Verified in both directions or it does not count.
6. **GATE** with `kryos-loop.sh gates 3`.
7. **PUSH IMMEDIATELY.** 12 commits sat unpushed once and a second agent
   independently reimplemented one of the fixes.
8. **LEDGER** — record the outcome *and everything ruled out*.

### Non-negotiables

- A self-reported "fixed" is not evidence. Only fresh command output is.
- Every gate can be green while data is silently wrong — that exact thing
  happened here (an `@copy` corruption passed conformance, no_double_free,
  bootstrap, examples, strict-caps, the e2e gate AND the IR gate). When a fix
  touches ownership, add a **value assertion**, not just a crash check.
- Measure leaks at two scales. One number proves nothing.
- Backends agreeing means the defect is in MIR; diverging means read the IR.

---

## OPEN — ranked

### 1. NEW — capability escape via closure/fn-value laundering: a "zero-capability" function can exercise ANY authority handed to it as a callback. NOT FIXED (design note only — see rationale below)
`tests/security/cap_escape_closure_launder.kry` + fixture
`tests/security/secret_for_closure_launder.txt` (repro below). **VERIFIED — the
single most serious open item, worse than the closed raw-memory escape**: it
needs no special builtin, no raw memory, no FFI — just ordinary closures,
which are one of the language's most idiomatic features (every `std::iter`
HOF and callback-style API uses them, per CLAUDE.md gotcha #19-20). It defeats
BOTH `--capabilities-mode=inferred` (the default) AND `--strict-capabilities`
("maximum scrutiny... every function auditable in isolation" per
`docs/10-capabilities.md`), and it defeats `kryos audit` too.

```
@capabilities(fs:read)
fn make_secret_reader(path: str) -> fn() -> str {
    return || file_read(path)
}

// Zero capabilities declared. Its body only ever calls a PARAMETER of
// function type -- never a NAMED builtin or NAMED function -- so it is
// invisible to fn_capabilities (keyed by NAME) in every mode.
fn zero_cap_tool(reader: fn() -> str) -> str {
    return reader()
}

@capabilities(fs:read)
fn main() {
    let reader = make_secret_reader("tests/security/secret_for_closure_launder.txt")
    println("recovered via zero_cap_tool: " + zero_cap_tool(reader))
}
```

**Proof, both ways, both modes** (`compiler/target/release/kryos.exe`, HEAD
677ecc3):
```
$ kryos check tests/security/cap_escape_closure_launder.kry              # inferred (default)
exit=0   (no diagnostics — zero_cap_tool's INFERRED set is computed empty)

$ kryos check --strict-capabilities tests/security/cap_escape_closure_launder.kry
exit=0   (no diagnostics — zero_cap_tool's DECLARED set is empty, and stays empty)

$ kryos run tests/security/cap_escape_closure_launder.kry
recovered via zero_cap_tool: TOPSECRET-CLOSURE-9f8e7d6c5b4a
exit=0

$ kryos audit tests/security/cap_escape_closure_launder.kry
== Capability inventory ==
  fs:read: 2 functions
    - make_secret_reader
    - main
                                    # zero_cap_tool is absent from the audit
                                    # entirely -- the dedicated auditing tool
                                    # also misses its true fs:read authority.
```
**Negative control (proves the checker isn't just broadly broken — it
specifically misses fn-value indirection):** the identical program with
`zero_cap_tool` calling `file_read(path)` DIRECTLY instead of through the
closure parameter is correctly REJECTED under `--strict-capabilities`:
```
error[E0505]: builtin `file_read` requires `fs:read` capability
 --> ...:2:12
  2 |     return file_read(path)
   |            ^^^^^^^^^^^^^^^ requires `fs:read`
exit=1
```

**Root cause (read, not guessed):** `kryos-capabilities/src/checker.rs`
tracks authority ONLY by NAME — `fn_capabilities: HashMap<String,
CapabilitySet>` is populated from `Decl::Function`/`Decl::Actor` names, and
every enforcement site (`enforce_callee_name`, `check_builtin_value_ref`)
looks a callee up BY NAME. Calling a value bound to a local/parameter of
function type (`reader()` where `reader: fn() -> str`) resolves to a
single-segment identifier that is never a registered builtin, extern, or
user-function NAME, so `enforce_callee_name` finds nothing and no diagnostic
fires — REGARDLESS of what the actual closure value does at runtime. The
same gap exists in `compute_inferred_capabilities`/`collect_caps_expr`:
calling a parameter contributes nothing to the caller's inferred set, so the
under-approximation is silent, not just an enforcement miss — `kryos audit`
reads the same map and inherits the blind spot. A closure LITERAL's body
(`|| file_read(path)`) is only checked ONCE, statically, against its
LEXICAL DEFINING scope (`make_secret_reader`, which legitimately has
`fs:read`) — there is no mechanism connecting that check to the scope(s) the
resulting fn-VALUE is later invoked from once it escapes as a return value,
argument, struct field, or array element. (`Expr::FieldAccess` in
`check_expr` also never checks the field name as a value-ref, so a closure
stored in a struct field and read out — `registry.reader()` — is the same
hole by a different route; not written up as a separate repro since it is
the identical root cause, not a new mechanism.)

**Why this is a DESIGN NOTE, not a patch (a fix was scoped and deliberately
NOT attempted this session):**

The only actually-sound fix is giving function TYPES a capability bound —
i.e. `fn() -> str` would need to carry (or infer) "this callable requires
`fs:read` to invoke," and every call through a value of that type would be
checked against the CALLING scope like any other gated operation. That is a
real type-system feature (capability-polymorphic / effect-polymorphic
function types), not a table entry — closer in scope to adding a new kind of
generic bound than to any of the CLOSED items above.

**The obvious cheap alternative was considered and ruled out as unsafe:**
treating a call through ANY non-directly-named fn-typed value as requiring
the caller to hold `Capability::All` (the maximally conservative, always-
sound stance: "if the checker can't prove what a callback does, assume it
can do everything"). This was NOT implemented because it cascades exactly
like the raw-memory case the TCB-split fix explicitly avoided: closures
passed to higher-order functions are pervasive and idiomatic in Kryos
(`std::iter::{map,filter,fold,reduce,scan,...}`, every callback-shaped
stdlib API, CLAUDE.md gotchas #19-20 document this at length) — blanket-
gating "calls a fn parameter" behind `all` would force nearly every HOF call
site in the stdlib and self-host compiler to declare `all`, which defeats
least-privilege as completely as the escape itself and was measured to be
the same failure mode ("closes the hole by making the language unusable")
documented for the raw-memory table-entry approach.

**A real fix is tractable but non-trivial, sketched for whoever picks this
up:** Kryos already does whole-program compilation and already computes a
fixed-point "inferred capability set" per function name
(`compute_inferred_capabilities`). The sound version needs the SAME
fixed-point computation extended to be **call-site-sensitive for fn-typed
arguments**: when a call `f(closure_expr)` passes a LAMBDA LITERAL or a
NAMED-FUNCTION reference as an argument bound to a `fn(...)`-typed
parameter, and `f`'s body calls that parameter, the capability that
particular CALL SITE of `f` requires is the union of `f`'s own direct
requirements and the PASSED closure's intrinsic requirement — not a single
global summary for `f` (which is what would be needed to keep `f` itself
capability-polymorphic, since `f` may legitimately be called elsewhere with
an innocuous closure). This is an interprocedural argument-flow analysis,
not a lookup table; it needs its own design pass (and a decision on what
happens when the flow can't be statically resolved — e.g. a closure read
out of a collection, or selected at runtime by a match/if — where the sound
answer is still "requires everything the checker can't rule out", scoped
down as narrowly as points-to precision allows).

**Ruled out as NOT the fix:** adding `reader`/generic fn-typed PARAMETER
names to `fn_capabilities` by convention, or requiring `@capabilities` on
any function with a `fn(...)`-typed parameter — neither actually computes
the real requirement per call site; both are either no-ops (an annotation
requirement with no enforced correctness) or force the same `all`-everywhere
cascade as the ruled-out blanket fix above.

**Not added to `tests/security_gate.sh`** — that gate encodes a boundary
that HOLDS; this repro is left as a standing, documented, NOT-gated artifact
(same status as `tests/mem/struct_arg_leak.kry` for the struct-leak design
note) so it is not silently "fixed" by an unrelated future change without
being re-verified, but does not turn CI permanently red for an open design
question.

### 2b. NEW (found while closing #2): global-reassignment-then-cross-function-read corrupts an array
`tests/known_failures/global_array_reassign_corrupt.kry` (repro below).
Isolated minimal repro, nothing to do with self-host:
```
struct Item { v: i64 }
let mut G: [Item] = []
fn reset() { let empty: [Item] = []  G = empty }
fn add_one(n: i64) { let it = Item{v:n}  G = push(G, it) }
fn main() { reset()  add_one(1)  println(to_string(len(G))) }
```
`kryos build --release` then run: `kryos panic: kryos_array_push: corrupt
array header ... (len=0, cap=0, elem_size=8, ref_count=1, data=0x0)`. A
top-level `let mut [T]` global works fine when only ever WRITTEN via `push`
from one call chain starting at its own initializer (proven: the same test
minus `reset()`/the extra function hop is flat). It breaks specifically when
one function REASSIGNS the global to a fresh value and a DIFFERENT function
later reads/pushes it -- the reader sees an all-zero header, i.e. either the
global's storage slot isn't actually being written by the cross-function
store, or the reader is loading a stale/wrong slot. Not yet root-caused
(stage-0 LLVM codegen for global stores, not self-host) -- lexer.kry's fix
below works around it by only ever pushing into `LEX_TOKENS`, never
reassigning it after its own top-level initializer. Worth a real fix; until
then avoid "reset a global from a helper function" as a pattern.

### 2c. NEW (found while building examples/showcase/secret_agent.kry): `std::test::assert`'s 2-arg form is permanently shadowed by the compiler's own builtin and is UNCATCHABLE -- NOT FIXED (design note)
`tests/known_failures/assert_shadow_uncatchable.kry` (repro below). Kryos has
a real, hardcoded 2-arg `assert(condition, msg)` INTRINSIC (dispatches to
`kryos_builtin_assert`, which prints and calls `std::process::abort()` --
never returns) alongside `std::test::assert(condition: bool, msg: str) ->
void` (a normal 2-arg Kryos function that builds a message and `throw`s,
meant to be catchable -- its own doc comment says "Throws with the message if
false", and `std::test::assert_no_throw`/`assert_throws` are built assuming
`assert`-family functions are ordinary throwing functions). These COLLIDE:
`kryos-codegen-{cranelift,llvm}/src/codegen.rs` dispatch any call literally
named `assert` with a nonzero arg count (`!args.is_empty()`) straight to the
intrinsic, UNCONDITIONALLY, before the generic "does the user define a
function with this exact name" shadow-check that every OTHER builtin
(`index_of`, `abs`, `len`, ... per CLAUDE.md gotcha #18, "a user function
shadowing a builtin now WINS") already goes through -- confirmed by reading
the comment directly above the generic shadow-check in both backends' call
lowering: "If the user has defined a function with this exact name, the user
definition shadows any builtin of the same name" immediately AFTER the
`assert`/`assert_eq`/`panic` special-case blocks, not before them. Since
`std::test::assert`'s signature is exactly 2 args (condition, msg), EVERY
call to it -- imported or not -- silently resolves to the intrinsic instead
of the stdlib body, no diagnostic, no warning.

```
use std::test::{assert}

fn main() {
    println("before")
    try {
        assert(false, "boom")
        println("unreachable")
    } catch (e) {
        println("caught: " + e)
    }
    println("after try/catch (should print -- assert should have been CAUGHT)")
}
```
Actual: prints `before`, then `assertion failed: boom` to stderr with NO
`kryos: uncaught exception:` prefix (proving it's `process::abort()`, not
`throw`), and the process dies -- `catch (e)` never runs, "caught: ..." and
"after try/catch" never print. `kryos_builtin_assert` is genuinely
`std::process::abort()`-based (`kryos-rt/src/builtins.rs`), so this is not a
crash from bad input -- it is the DOCUMENTED, INTENDED behavior of the real
intrinsic firing instead of the stdlib wrapper every single time.

**Why this is a design note, not a patch attempted this session:** the fix
shape is clear (move the `assert`/`assert_eq`/`panic` special-case blocks in
both codegen backends to run AFTER the generic user-shadow check, matching
how every other builtin already works) but the blast radius is unverified --
`assert`/`panic` are used pervasively across `compiler/self-host/`,
`ecosystem/*/tests/`, and every showcase/example that calls the TRUE 1-2-arg
intrinsic form (which must keep working identically, uncatchable-abort
semantics included, once no user function of that name/arity exists). That
needs its own full-gate pass, not a fix folded into an unrelated example
session. Left as a standing repro. Workaround used in `secret_agent.kry`:
avoid `std::test::assert`/`assert` entirely; use `assert_true`/`assert_ne`/
`assert_eq` (fixed this session, item below) or a locally-named helper
instead -- none of those names collide with a hardcoded intrinsic.

### 3. Struct-argument leak — ~86MB per 1M calls — DESIGN NOTE, NOT FIXED (8 attempts now ruled out)
`tests/mem/struct_arg_leak.kry`. Passing a struct with HEAP FIELDS across any
call boundary leaks its body. **Not** method-specific — a free function leaks
identically. Flat for contrast: scalar-only struct through a method, and the
same struct's fields read directly.

**Re-measured fresh this session** (`kryos-loop.sh soak`, 250k -> 1M,
Windows --release, HEAD 721a9cf, no code changed):
```
heap_field_method    10.5 MB -> 87.8 MB   LEAKS (confirms prior 25.7->95.5 shape)
scalar_method          0 MB ->  3.9 MB    FLAT
free_fn_scalar_ret   10.9 MB -> 91.7 MB   LEAKS (confirms "not method-specific")
```

**8th attempt was a design pass, not a patch — this session, per instruction, wrote
the design and DID NOT implement.** Reasoning below.

#### The mechanism a 7-attempt investigation had not yet named: two struct-drop code paths that disagree about ownership

Read (not guessed) directly from both backends this session. There are TWO
independent codegen paths that free a struct's heap fields, and only ONE of
them ever consults a struct's shared-owner count:

1. **The boxed-element path** — `__kryos_drop_<Name>` (LLVM:
   `kryos-codegen-llvm/src/codegen.rs:2118-2239`; Cranelift's equivalent named
   helper referenced at `kryos-codegen-cranelift/src/codegen.rs:7570-7588` for
   array/enum-payload struct elements). This helper calls
   `kryos_struct_release_shared` FIRST (LLVM `codegen.rs:2194-2205`) and bails
   out without freeing fields if another owner remains. This is the ONLY path
   `kryos_struct_retain`'s owner-count word (`kryos-rt/src/alloc.rs:646-670`,
   the second word of the `kryos_calloc` header) is ever checked against.
2. **The local/param/return path** — `Instruction::Drop` for a struct-typed
   local, inlined directly (LLVM `codegen.rs:3779-3797`, Cranelift
   `codegen.rs:3186-3206` -> `emit_drop_for_value` `codegen.rs:7495-7562`).
   This path calls `emit_struct_drop`/`emit_drop_for_value` **directly, with
   no call to `kryos_struct_release_shared` at all** — confirmed by reading
   both call sites end to end, not inferred. It always frees every heap field
   it finds, regardless of how many other owners exist.

A function PARAMETER, an ordinary struct `let`, and a struct RETURN value are
represented as SSA aggregates / byval copies (LLVM) or aliased raw pointers
(Cranelift) — never as a value that passes through path 1. So **any fix that
adds an owner-count retain (at a call site, at a spawn-capture site, anywhere)
is invisible to path 2**, which is exactly why attempt #7 ("give the spawned
thread its own owner count … the retain calls ARE emitted") still failed:
the retain bumped a counter that path 2's drop never reads. This generalizes
the earlier finding from "verified it still fails" to "structurally cannot
work while these two paths stay separate" — an owner-count model only
protects a struct if EVERY place that can drop that struct's fields agrees to
consult the same counter, and today most of them don't.

One correction to the "7 attempts" writeup: `conf_spinlock_mutex`'s own
structs (`SpinLock -> AtomicInt -> Mutex`) are, field-by-field,
`ptr`/`i64`/`bool` all the way down (`compiler/stdlib/sync.kry:23-27,119-122`)
— no `Str`/`Array`/`Map`/`Function`/`Shared` field anywhere in that chain, and
none of the three structs is `@copy`. Both backends' field-drop loops have an
explicit `_ => {}` fallback for scalar field types (LLVM
`codegen.rs:10542`), so dropping any of these three structs through EITHER
path is a no-op by itself — the crash was not reproduced or re-diagnosed this
session (deliberately: doing so means writing the one-line patch, which is
exactly the 8th incremental attempt this task says not to force). Flagging
this as the concrete first step for whoever attempts the real fix: run the
one-line allowlist change with `KRYOS_FREE_DIAG=1` and a debug build against
`conf_spinlock_mutex` BEFORE re-theorizing — the failure is almost certainly
in a DIFFERENT struct in the same file (`WaitGroup`/`Once`, both also
scalar-through-`AtomicInt` per `sync.kry:247-250,312-313`, so still probably
not those either) or in an interaction with the spawn deep-copy path below,
not in `SpinLock` itself. Don't re-guess this — measure it.

#### Spawn already has a THIRD, bespoke ownership model — any fix must not regress it

`Instruction::Spawn`'s struct-capture arm (LLVM `codegen.rs:3980-4002`) does
NOT use the retain/owner-count model at all. It heap-copies (`kryos_calloc`)
a fresh box and deep-clones the struct's OWN top-level `Str`/`Array`/`Map`
fields into it (`deep_copy_struct_index_clone`), while deliberately leaving
NESTED STRUCT sub-fields shared (raw pointer, not cloned) — "so an AtomicInt
inside keeps its shared cell" per the comment at `codegen.rs:3987-3996`. This
is why a spawned thread and its parent can both still see the SAME mutex/
atomic cell (required for `conf_spinlock_mutex` to mean anything) while the
thread also gets its own independent copy of any `str`/array data the struct
carries. This is a THIRD ownership policy, distinct from both drop paths
above, and it currently works (gates are green today). Any unification design
has to either (a) leave this path alone and make ordinary calls agree with
it, or (b) fold it into the new model — folding it in is strictly higher risk
because it is the one part of this file already proven correct under
concurrency.

#### Design A — uniform boxing of struct values

Every struct-typed local, param, and return becomes a pointer to a
`kryos_calloc` box carrying the shared-owner header (same layout the boxed
element path already uses), on BOTH backends. `consume_call_args` adds
`MirType::Struct` to the borrow allowlist; EVERY struct drop site (params
included — params currently never drop at all) calls `kryos_struct_retain`/
`kryos_struct_release_shared` through the SAME helper boxed elements already
use. This closes the leak and the two-path disagreement in one move, because
there is only one path left.

Cost, stated honestly:
- **ABI break on LLVM AOT.** Struct params/returns move off `byval`/`sret`
  onto a bare `ptr`, touching every call site, every `emit_aggregate_struct`
  literal, every method receiver, every generic `impl<T>` instantiation, and
  field-access GEP codegen (which currently addresses an INLINE aggregate,
  not a boxed pointer, for nested struct fields — `emit_struct_drop`'s own
  comment history at `codegen.rs:10495-10510` documents a real
  invalid-free bug from getting this distinction wrong once already).
- **A `kryos_calloc` per struct construction that currently costs zero
  allocations.** `Plain { a: 1, b: 2 }` (the flat `scalar_method` case above)
  would start allocating. The gotcha's documented hot path — `self-host
  parser.kry` threading `Parser { tokens: [Token], .. }` through every `p_*`
  call, explicitly exempted from an EARLIER, much smaller entry-copy fix
  because it OOMs stage-1 (`CLAUDE.md` gotcha #23, "Heap-bearing `@copy`
  structs as params") — is the textbook case this would make worse, not
  better, unless boxing is made conditional in a way that reintroduces the
  representation split this design exists to remove.
- **Self-host bootstrap risk.** Bootstrap already runs at the edge of what's
  survivable memory-wise (see the CLOSED "Lexer 13-16GB" entry above, and
  item #7, the still-open Parser analogue at 286MB). A per-struct allocation
  on every call in a compiler whose own IR is a struct-heavy tree is the kind
  of change that needs its own dedicated measurement pass, not a
  side-effect of a different fix.

#### Design B — unify the two drop paths without changing the ABI

Keep params/returns as SSA aggregates / aliased pointers (no ABI change).
Instead:
1. Add `MirType::Struct` to `consume_call_args`'s borrow allowlist for
   ordinary (non-spawn) calls only — the caller keeps its own scope-end drop.
2. Give callee PARAMS a real scope-end drop for struct types (today
   `param_locals` never drop — this is the other half of the leak, not just
   the caller side).
3. At the CALL SITE, before the call, emit a **recursive field-level retain**
   that walks the struct the same way `emit_struct_drop`/`emit_drop_for_value`
   already walk it for freeing (`Str`->`kryos_string_retain` equivalent,
   `Array`->array retain, nested `Struct`->recurse, scalar fields skipped) —
   this is the missing "retain" side of a traversal whose "release" side
   already exists and is exercised in production. This makes the callee's
   byval/aliased copy a genuine independent reference rather than an
   unbalanced alias, so step 2's param-drop is now correct instead of a
   double-free.
4. Leave `Instruction::Spawn`'s bespoke deep-copy path untouched — it already
   does the equivalent of steps 1-3 by hand for its one call site (the
   comment at `codegen.rs:3987-3996` is literally describing "retain top-level
   fields, share nested struct fields" already). Do not route spawn through
   the new generic retain helper; it has different (correct, tested) rules
   about what stays shared.

Cost, stated honestly:
- No ABI change, no new allocation for scalar-only or copy-avoidant structs —
  `scalar_method` and `heap_field_direct` in the repro file stay exactly as
  fast as today.
- New cost is proportional to the STRUCT'S HEAP FIELD COUNT per call, not a
  flat allocation — cheaper than Design A for the common case, more expensive
  than Design A for a struct passed through a long call chain (each hop pays
  a retain instead of one allocation shared across the chain).
- Still a real, cross-cutting change: the retain-walk codegen has to exist on
  BOTH backends and stay in lockstep with the existing drop-walk (two
  recursive traversals of the same struct shape that must never diverge — a
  divergence here is exactly the class of bug the CLOSED "computed string ->
  user fn leaked" and "spawn shared one box" entries above already show this
  codebase is prone to when a retain and a release are added in different
  places by different patches).
- Does NOT by itself explain or fix whatever broke `conf_spinlock_mutex` under
  the naive one-line version of step 1 — per the correction above, that
  failure is still unexplained and needs a direct repro (not a re-guess)
  before either design is implemented, because if it turns out to be a
  spawn/ordinary-call interaction, step 4's boundary is exactly where it
  would resurface.

#### Recommendation

Design B is the better shape: no ABI break, no bootstrap-memory risk, smaller
blast radius, and it turns the missing half of the leak (`consume_call_args`)
and the missing half of the fix (param drops) into ONE symmetric
retain/release traversal pair instead of leaving them independently patched.
Design A is the more "correct-looking" unification (one ownership model
everywhere) but its cost is concretely worse for the code this compiler
spends the most time running (self-host bootstrap, struct-heavy hot loops)
and was already ruled out in spirit by the earlier, much narrower
heap-bearing-`@copy`-param exemption in gotcha #23.

**Not implemented this session.** Rationale: 7 prior incremental attempts
already failed against this exact leak, each for a reason that looked fixed
until measured; the honest next step is a fresh, isolated repro of the
`conf_spinlock_mutex` failure mode under the one-line change (with
`KRYOS_FREE_DIAG=1`, per the measurement traps section) BEFORE writing
Design B's retain-walk, not concurrently with it. Attempting the retain-walk
without first pinning down that failure risks producing exactly an 8th
incremental, unexplained regression — the outcome this task explicitly ranks
worse than a design note. Workaround for a hot loop remains: read fields
directly (flat), keep heap data out of structs you pass, or reuse one
instance instead of constructing per iteration.



### 4. `comptime { }` runs at RUNTIME while the docs sell compile-time
Fix the docs (hours). Real compile-time evaluation is months and should not
gate 1.0.

### 5. `[dyn Handler]` reports a confusing `E0100` instead of `E0110`
Array-literal element unification ignores the annotated `dyn` element type.

### 6. Docs status sections drift because they are hand-maintained
`docs/BUGS.md` said "none currently tracked" while two tests deadlocked.
Generate from real test output.

### 7. `Parser` has the same array-in-a-rebuilt-struct pattern as the closed lexer bug, unfixed
`self-host/parser.kry`'s `advance()`-style helpers rebuild `Parser { tokens:
p.tokens, pos: p.pos + 1, errors: ..., no_struct_lit: ... }` on every token
consumed (parser.kry:148, 176, 185) -- the same shape that made `Lexer`
balloon to 16GB (closed item #2 below), just at TOKEN granularity (~20030
reconstructions x up to ~20030 retained Token elements) instead of CHARACTER
granularity (~110K reconstructions), so it costs ~O(n^2) but stays in the
"slow, not lethal" range at current file sizes -- measured 286MB peak WS
compiling parser.kry end-to-end post-fix (tokenize alone is <100MB; the
remainder is almost certainly this). Not a crash today, but the same class
of bug and will get worse as self-host files grow. Same fix shape applies:
pull `tokens` out of `Parser` into a read-only pass-through (it's never
mutated after `parser_new`, so this one doesn't even need a module global --
a plain extra parameter threaded through, or confirm `errors`/`no_struct_lit`
don't need the same treatment).

---

## CLOSED — with the evidence that closed it

| Item | Evidence |
| --- | --- |
| sret fn-value ABI | struct through a fn value returned garbage on `--release`; fixed by consulting `func_sig_aggs`. Guessing from the LLVM type string broke `Result.and_then` — enums are aggregate-shaped but returned directly |
| `bool` -> builtin ABI | `json_bool(false)` built JSON `true`; `i1` against an `(i64)` declaration left the upper 63 bits undefined |
| narrow-int args | same class for `char_from`/`char_at`; latent only because 32-bit x86-64 ops zero the upper half |
| read-only builtin args leaked | 15 builtins missing from the borrow allowlist; the consume mark is path-insensitive so one `to_upper(s)` suppressed the drop on every path. 78MB/800k -> flat |
| network `KryosString` allocator | six sites freed under a layout they were never allocated with; 360k TCP round trips 18.7MB -> 0.1MB |
| computed string -> user fn leaked | 35.4MB/400k -> 4.0MB; needed the `@copy` str-field copy as prerequisite |
| `-g` emitted an undefined string global | `kryos build -g` was broken on every platform |
| spawn wrapper `byval` ABI | System V only; closed both concurrency blockers |
| **Cranelift shared one box for a loop-local enum captured by `spawn`** | `tests/known_failures/spawn_loop_capture.kry` (now folded into `tests/conformance/conf_spawn_agg_capture_abi.kry` section 7) -- JIT printed `30 30 30 30`, AOT printed the four distinct values. Suspected mechanism (hoisted per-iteration box) was WRONG -- `--emit-mir` showed a fresh `Msg::variant#0(_3)` every iteration and `RValue::EnumVariant` codegen `kryos_calloc`s a new box each time, both correct. Real cause: the Cranelift `Instruction::Spawn` arg-store match had clone/dup arms for Str/Array/Map/Function/Shared/Struct but no `MirType::Enum` arm, so an enum capture fell to `_ => val` (raw shared pointer, no clone) while MIR's normal post-spawn `drop(_N)` still fired (spawn's documented contract is that it clones heap args) -- freeing the box while the spawned thread could still read it, and the freed slot was immediately reused by the next iteration's same-size `kryos_calloc`, so a thread that lost the race read whichever iteration's value last occupied that address. LLVM AOT already had a `MirType::Enum` arm in its equivalent path, hence no divergence there. Fix: added the missing arm calling the existing `emit_enum_deep_copy` helper (already used for closure/struct captures), mirroring the `MirType::Struct` arm immediately above it. Proof: pre-fix binary crashes (`rc=132`, illegal instruction) on the new value-assertion section every run (5/5); post-fix binary passes 8/8 JIT + 3/3 AOT, and 15/15 raw JIT runs of the original repro now print `0 10 20 30` (was nondeterministic, dominated by `30 30 30 30`). Gates green: conformance 47/47, tier1+tier2 green, bootstrap 16/16 |
| **struct `str` field read leaked, 614MB in CI** | `r.name = mk_str(i)` + `len(r.name)` in a loop: 157.7MB/2M before, 3.9MB after; the full CI workload 617.6MB -> 4.5MB. A `str` field read RETAINS and nothing balanced it. Array/map/struct field reads stay borrows -- `push` grows the shared buffer in place, so dropping those temps is the `alloc_node` double-free |
| **raw-memory capability escape** | a zero-capability program read `TOPSECRET-APIKEY` via `str_to_ptr`+`ptr_byte_at` and dereferenced +4096 without faulting. Closed with a trusted-computing-base split: raw memory requires `ffi` at DIRECT USE in user code and the requirement does not propagate, so the stdlib (which is built on these — `alloc` in 14 modules) stays usable. Guarded by `tests/security_gate.sh`, which asserts BOTH directions plus no-cascade |
| **bootstrap WINDOWS-ONLY exit -1 in tokenize (ex-item #2)** | NOT a fault, not Defender, not the pool allocator, not heap corruption -- confirmed by an `atexit`-hook diagnostic (`KRYOS_EXIT_TRACE=1` in fault.rs) that never fires before the -1 death, proving no `process::exit`/normal `main` return is involved; the OS kills the process directly (memory-pressure dependent). Root cause found by MEASURING MEMORY, not tracing exceptions: `Get-Process` polling showed kryos-stage1.exe peaking at 13-16GB+ (and still climbing) to tokenize a 109KB file. Cause: `Lexer { src, pos, tokens }` was rebuilt via a fresh struct LITERAL on every `lex_advance`/`lex_match_char`/`lex_emit` call (i.e. per CHARACTER, ~110K times for parser.kry, not per token) and `emit_aggregate_struct` in kryos-codegen-llvm clones/dups ANY array-typed struct FIELD unconditionally at every literal construction (elem_kind=4 for a struct-of-Token element additionally RETAINS every element) -- so each of ~110K reconstructions retained up to ~20K already-emitted tokens: O(n^2), order 1e9 atomic retains, matching the CPU hot-path symbols found via `llvm-symbolizer` against a `-g` debug rebuild (`kryos_struct_retain`/`kryos_array_dup`/`kryos_array_new` dominated `KRYOS_WATCHDOG` RVA samples). The EARLIER "Lexer NOT @copy" fix (see struct comment history) assumed a non-@copy struct's array field is merely SHARED (refcount bump) on rebuild; it is not -- `emit_aggregate_struct`'s field-clone is unconditional, not @copy-gated, so that fix never actually delivered the intended O(n) it documented. FIX: pulled `tokens` out of `Lexer` entirely into a module-level `let mut LEX_TOKENS: [Token] = []`, mutated only via `push` (never reassigned after its own initializer -- see item #2b, a SEPARATE newly-found bug where cross-function global reassignment corrupts the array). Proof: peak working set 13-16GB+ -> 286MB (tokenize alone <100MB); dose-response gone -- 8/8 clean on parser.kry (109KB) AND 8/8 clean on lower.kry (128KB, the other historically-failing file) with the OLD binary confirmed still failing 3/6 on lower.kry in the same session (prove-both-ways); `test_bootstrap.sh` 16/16 across 7 consecutive runs (was the documented 14/16 baseline); full `kryos-loop.sh gates 2` GREEN. Item #7 below is the SAME bug class, unfixed, in `Parser` (lower severity: token-granularity not character-granularity, not yet lethal) |
| **`assert_eq`-named user/stdlib calls skipped the post-call unwind check, so a caller kept running after its callee threw** | Found writing `examples/showcase/secret_agent.kry`'s value assertions with `std::test::assert_eq` (3-arg: `actual`, `expected`, `msg`). Repro: `fn main() { println("before")  assert_eq(x, y, "diff")  println("after (should NOT print)") }` with `x="AAAA"`, `y="BBBB"` -- printed BOTH `before` AND `after`, THEN the correct `kryos: uncaught exception: assertion failed: diff -- ...` to stderr, exit 101. Bisected mechanically (one variable at a time, not guessed): reproduces for ANY function literally named `assert_eq` regardless of param names/return-type annotation/import-vs-local, and does NOT reproduce under any other name -- pinned it to the name itself. Root cause (read, not guessed): `kryos-mir/src/lower.rs::is_unwind_source` and the equivalent post-call "check the thread-local exception state" filters in BOTH codegen backends (`kryos-codegen-cranelift/src/codegen.rs`'s inline `should_check` match, `kryos-codegen-llvm/src/codegen.rs::post_call_exception_check_applies`) hardcode `"assert_eq"` in their "this call can never throw, skip the check" list -- correct ONLY for the compiler's real 2-arg `assert_eq(left, right)` INTRINSIC (which lowers to `kryos_builtin_assert_eq`, a `process::abort()`-based call that never returns, so genuinely needs no check), but the exclusion didn't gate on arg count, so it ALSO wrongly excluded a 3-arg call resolving to `std::test::assert_eq` (or any user function of that name) -- a REAL function that `throw`s and returns normally. Without the check, the caller's next MIR instruction ran before anything noticed the pending exception; it only surfaced at a LATER checked boundary. Also broke `try`/`catch` routing the same way (a failing 3-arg `assert_eq` inside a `try` was not caught at all -- confirmed before/after). FIX: in all 3 sites, exclude `"assert_eq"` from the "never throws" set ONLY when `args.len() == 2` (matching the intrinsic's own dispatch condition, already present nearby in both backends), so any other arity gets the check. Proof, both ways: pre-fix the minimal repro printed `after` and pre-fix `try`/`catch` did not catch (both shown above); post-fix (`cargo build --release`, both backends) the repro prints only `before` then the exception (JIT AND AOT), the `try`/`catch` variant correctly catches and prints "caught: ...", and a genuinely-passing `assert_eq(4, 2+2, ..)` and the TRUE 2-arg intrinsic (`assert_eq(1, 2)`, no import) both still behave exactly as before (matching-value case doesn't throw; the true intrinsic still aborts immediately, only `before` prints). Gates: conformance 47/47, tier1+tier2 GREEN, bootstrap 16/16 |

---

## CAPABILITY SURFACE AUDIT (2026-07-29)

Enumerated all 157 builtins the LLVM codegen maps against the 82 the capability
model gates, filtered to authority-bearing names, and probed each survivor.

- **Raw memory — REAL ESCAPE, fixed.** See the closed table.
- `file_append` — looked ungated to a first-pass grep; it is gated, just in a
  different match arm than `append_file`. No gap.
- `buf_get_byte` / `buf_set_byte` — probed for out-of-bounds access. **Safe:**
  the runtime bounds-checks and returns `-1`. My first probe appeared to show a
  4096-byte over-read only because it counted the `-1` sentinel as data. Verify
  the VALUES, not just that something came back.
- `buf_write_*` — write into an owned buffer, no external authority.
- `read_line`, `time_now_*` — input and clock reads; ambient by design, matching
  the documented model.

**Minor wart, not security:** `buf_get_byte` returns `-1` out of range while
array/string indexing PANICS. Same undocumented-sentinel class as `pop([])`
returning `0`. Inconsistent, and `-1` is a plausible real byte value in signed
contexts. Worth unifying.

---

## MEASUREMENT TRAPS (each cost real time)

- **`cargo build -p kryos-cli` leaves the staticlibs stale.** Runtime edits are
  invisible to AOT programs until a full `cargo build --release`. This produced
  a wrong "no effect" reading and a wrongly "ruled out" theory. `preflight`
  checks it.
- **Bootstrap fails spuriously (rc=127, rotating modules) under load.** Run it
  alone; only a solo failure is real.
- **A control that changes the workload proves nothing.** A server that accepts
  and sends without READING looked perfectly flat — 3965 of 4000 requests were
  failing with RST.
- **`KRYOS_FREE_DIAG=1` completing while the program normally crashes means the
  crash IS corruption.** Master's `parse_int: invalid numeric input: '}'` was
  memory corruption, not a parse bug.
- **A leak needs a workload that ALLOCATES A FRESH VALUE each iteration.**
  Re-reading the SAME string 2M times looks perfectly flat even with a fully
  unbalanced retain -- the refcount climbs, nothing allocates. That false
  reading is what retired the field-read drop and cost 614MB in CI for two
  days. Vary the value, and measure the read and the overwrite TOGETHER: read
  alone 4.3MB, store alone 4.4MB, store+read 157.7MB. Either half alone
  says "no leak".
- **`kryos_string_clone` is not a deep copy.** It is a refcount bump returning
  the same pointer, identical to `kryos_string_retain`.
