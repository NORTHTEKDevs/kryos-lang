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

### 1. capability escape via closure/fn-value laundering, stored in a CONTAINER (struct field / array element / map value) -- narrowed residual of a mostly-CLOSED item; see CLOSED table below for what shipped
`tests/security/cap_escape_closure_launder_container.kry` (repro below).
**The PARAMETER/local/return/passthrough-chain/actor-message/spawn/generic/
dyn-Trait shapes of this escape are CLOSED this session** (full account in
the CLOSED table). This item is what's LEFT: a closure/fn-value read back
OUT OF A CONTAINER is not traced at all.

```
struct Registry { reader: fn() -> str }

@capabilities(fs:read)
fn make_secret_reader(path: str) -> fn() -> str {
    return || file_read(path)
}

fn zero_cap_tool(reg: Registry) -> str {
    return reg.reader()
}

@capabilities(fs:read)
fn main() {
    let r = make_secret_reader("tests/security/secret_for_closure_launder.txt")
    let reg = Registry { reader: r }
    deny!(fs:read) {
        println("struct-field launder (NOT CLOSED): " + zero_cap_tool(reg))
    }
}
```
`kryos check --strict-capabilities` on this: exit=0, no diagnostic. `kryos
run`: prints the secret from INSIDE the `deny!(fs:read)` block. Identical
shape reproduces for an array (`[fn() -> str]`) and a map
(`map<str, fn() -> str>`) element -- verified live, not written up as
separate repros since it is the same root cause, not a new mechanism (this
matches the note the ORIGINAL finding already made: "a closure stored in a
struct field and read out -- `registry.reader()` -- is the same hole by a
different route").

**Root cause:** the fix's `hot_params` analysis (`kryos-capabilities/src/
checker.rs`) marks a PARAMETER hot only when its OWN declared type is
`fn(...) -> ...` (`is_fn_typed`). A parameter typed `Registry` (a struct
with a function-typed FIELD), `[fn() -> str]`, or `map<str, fn() -> str>`
never matches, so `zero_cap_tool`'s param `reg` is never marked hot, and
`resolve_closure_caps` never gets a chance to trace `reg.reader`/`arr[i]`/
`m[k]` back to the value that was written into it.

**Fix sketch for whoever picks this up:** extend `is_fn_typed` (or add a
sibling predicate) to recognize a struct type with >=1 function-typed field,
an array whose element type is a function, or a map whose value type is a
function. Extend `hot_params`'s seed pass to also detect `obj.field()` /
`arr[i]()` / `m[k]()` invocation shapes (today only a bare `p(...)` callee
counts). Extend `resolve_closure_caps` to trace a struct-literal/array-
literal/map-literal CONSTRUCTION site: union every value written into the
relevant field/element (conservative -- an array or map has no per-index
static tracking, so ANY write contributes), falling back to `Unknown` (the
same sound `Capability::All` default already used for every other
unresolvable shape) when the container is built from a non-literal source
(e.g. `push`ed in a loop, populated from another function's return, read
from yet another container). Comparable in size to the parameter-based
mechanism that just shipped; not attempted this session to keep the change
reviewable.

**Not added to `tests/security_gate.sh`** as a reject case (it would fail
today) -- it is a standing, documented, NOT-gated artifact (same status as
`tests/mem/struct_arg_leak.kry`'s design note) so a future change doesn't
silently "fix" it without re-verification, but does not turn CI permanently
red for an open item. The CLOSED shapes ARE gated (`tests/security_gate.sh`
checks #4-6).


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
  item #5, the still-open Parser analogue at 286MB). A per-struct allocation
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



### 4. `[dyn Handler]` call-site (not `let`) still gets a confusing `E0100` alongside `E0110` -- narrow, honestly-scoped residual
The `let x: [dyn Trait] = [A{}, B{}]` shape is fixed (item closed below,
see CLOSED table). The SAME symptom still reproduces when the heterogeneous
array literal is passed directly as a CALL ARGUMENT instead of through a
`let`:
```
fn use_handlers(hs: [dyn Handler]) { for h in hs { println(h.handle()) } }
fn main() { use_handlers([A{}, B{}]) }
```
still emits both `E0110` (correct) and a confusing `E0100: expected A, found
B` (noise) on the call-site line. NOT fixed here, deliberately: the `Let`
fix keys off the RAW (pre-resolution) `TypeExpr::Array{element: DynTrait}`
annotation, which is exactly what makes it precise -- `FunctionSig.params:
Vec<(String, Type)>` only stores the ALREADY-RESOLVED `Type` (dyn-in-array
already collapsed to the generic `Type::Error`), so there is no way to
distinguish "this Error came from a rejected dyn array" from "this Error
came from an unrelated unknown-type-name annotation" at the call-arg check
site without adding a reason tag to `Type::Error` or threading raw
`TypeExpr`s through `FunctionSig`. A broader fix (suppress the pairwise
unify whenever param_ty resolves to ANY `Type::Error`) was implemented,
tested, and REJECTED after measurement: it silently dropped a genuinely
useful diagnostic for the unrelated case (`let x: NotAType = [1, "two"]`
lost its "expected i64, found str" E0100, keeping only the unknown-type
error) -- proven via a stash/rebuild A-B comparison, not guessed. Ruled out
as not worth the collateral loss for a papercut-tier item. Real fix needs
either a `Type::Error` reason enum or plumbing `FunctionSig` to retain the
param's raw `TypeExpr` for this one diagnostic-quality check.

### 5. `Parser` has the same array-in-a-rebuilt-struct pattern as the closed lexer bug, unfixed
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

### 6. `any` is type-erased to a bare i64 with NO runtime type tag -- `to_string`/`format` mis-render non-i64 values -- DESIGN NOTE, NOT FIXABLE WITHOUT AN ABI CHANGE

CLAUDE.md gotcha #22. `push(args, "x")` into an `[any]`, or a `str`/`f64`
argument routed through `fn(args: [any])`, reads back through `to_string`/
`std::fmt::format` as its raw pointer/bit representation, not its logical
value. Read (not guessed) directly: `kryos-types/src/ty.rs:197` resolves the
type name `any`/`Any` straight to `Type::Error` (no type information
survives the type checker at all), and `kryos-mir/src/lower.rs:12793` lowers
it to a bare `MirType::I64` -- one 8-byte slot, zero tag bits, no
discriminant stored anywhere in the value or alongside it.

**Why this is structurally different from the two fixes closed this
session** (compound-return erasure, item 6 CLOSED below): those were a
GENERIC parameter `T`, where every concrete call site has exactly ONE
resolved type, so the fix could re-derive the real type per
MONOMORPHIZED INSTANTIATION and patch the erased slot's static type at
that instantiation. `any` has no instantiation to hang a type on -- its
entire purpose is holding DIFFERENT concrete types in the SAME slot/array
simultaneously (`[any]` may hold a `str` in index 0 and an `i64` in index
1 in the same call). There is no single per-callsite type to recover; the
type genuinely does not exist after erasure, so no amount of monomorphizing
call sites can recover it -- the value itself needs to carry a tag.

**Concrete design (not attempted -- ABI change, scoped for whoever picks
this up):** widen `any`'s runtime representation from a bare `i64` to a
2-word tagged value `{ tag: i64, payload: i64 }` (tag identifies
i64/f64/bool/str/array/map/struct-kind; payload is the existing erased
slot, reinterpreted per tag). Cost, stated honestly:
- Touches every `[any]` array element (doubles element size), every
  `fn(args: [any])` call-site marshalling, `push`/`pop`/indexing on an
  `[any]` array, and `std::fmt::format`'s own arg-walking loop (which gets
  simpler -- it can finally dispatch per-element instead of assuming i64).
- Any existing `.kryos`-compiled artifact or FFI boundary that assumes a
  bare i64 `any` slot breaks -- this is a genuine ABI break, not additive.
- Scoped narrowly to `any`/`[any]`/`fn(..: any)` -- does NOT need to touch
  the separate generic-erasure path (`T` in `impl<T>`/`fn<T>`), which the
  two fixes below already handle without any representation change,
  because a generic `T` resolves to ONE type per instantiation and `any`
  by design does not.

**Recommendation:** not worth attempting inside a wave scoped to
"fix without an ABI change" -- this genuinely needs one. Workaround
documented in CLAUDE.md gotcha #22 (build the string with `+` and
per-type `to_string` on the concretely-typed values instead of routing
through `[any]`/`format`) remains correct and is not a papercut users hit
by accident (the element-typed `std::iter` HOFs are already generic and
avoid `any` entirely, per the same gotcha).

### 7. mutated-SCALAR-capture persistence never got the "N>=2" generalization the mutated-STRUCT-capture case already has -- SILENT WRONG VALUE, NOT FIXED

`tests/known_failures/closure_mutated_capture_scalar_gaps.kry` (repro below,
3 shapes). Ranks above items 2b/2c/3/4/5/6 per this ledger's own doctrine --
a silent wrong answer outranks a crash. Found this session while hunting
"mutated captures of several kinds at once" per the closures/fn-values/
captures wave brief.

CLAUDE.md gotcha #11 documents that TWO-OR-MORE mutated STRUCT captures in
one closure now persist independently across calls (`mutated_capture_ptr_slots`,
closed `5f2386e`). The equivalent case for a mutated SCALAR (i64/f64/bool)
capture was never generalized past its ORIGINAL single-capture fix and
silently loses persistence in three shapes, reproduced live, identical on
both backends (shared MIR, not a divergence):

```
struct Ctr { base: i64 }

fn main() {
    // shape 1: two mutated scalars in one closure
    let mut a: i64 = 0
    let mut b: i64 = 0
    let bump2 = || { a = a + 1  b = b + 10  return a * 1000 + b }
    println(to_string(bump2()))  // 1010
    println(to_string(bump2()))  // want 2020, GET 1010
    println(to_string(bump2()))  // want 3030, GET 1010

    // shape 2: one mutated scalar + one mutated struct field together
    let mut scalar_cap: i64 = 0
    let mut struct_cap = Ctr { base: 100 }
    let bump_mixed = || {
        scalar_cap = scalar_cap + 1
        struct_cap.base = struct_cap.base + 1
        return scalar_cap + struct_cap.base
    }
    println(to_string(bump_mixed()))  // 102
    println(to_string(bump_mixed()))  // want 104, GET 103 (struct persisted, scalar froze)
}
```

**Root cause (read, not guessed, `kryos-mir/src/lower.rs` lambda lowering):**
a mutated capture only gets a persistent env-slot write-back when
`mutated_captures.len() == 1 && tail_value_is_identifier(body,
&mutated_captures[0])` -- the mechanism smuggles the new state back by
writing the CALL'S RETURN VALUE into the env slot after the call, which only
works when there is exactly one mutated capture and it IS (literally) the
returned value. The struct fix instead passes the capture BY POINTER
(`mutated_capture_ptr_slots`, gated on `matches!(cap_ty, Some(MirType::Struct(_)))`),
which needs no such restriction because any field mutation on any path lands
directly in the persistent block through the pointer -- but that mechanism
was never extended to scalars, which are plain SSA values (no address to
hand out) in the current representation. This also explains a THIRD shape
(in the known_failures file, not reproduced above for space): a SOLITARY
mutated scalar whose enclosing closure's tail value is NOT literally that
identifier (e.g. `let make = |tag| { count = count + 1  let n = count
return || n }` -- a common "stateful factory" idiom) never triggers the
write-back at all, so `count`'s mutation is lost across separate calls to
`make` even though only one capture is mutated.

**Why not fixed this session:** the code's own comment at the point of
restriction already states the risk explicitly ("Closures mutating more
than one capture keep working exactly as before this fix (no worse), they
just don't get the persistence fix -- a documented residual limitation
rather than risking a wrong value written to the wrong slot"). Closing this
properly needs the SAME representational change the struct fix used --
giving a mutated scalar capture its own persistent, ADDRESSABLE heap slot
and passing a pointer to it, instead of smuggling state through a return
value -- across MIR lowering AND both codegen backends (LLVM's aggregate-vs-
scalar param handling in `emit_function`'s prologue is structurally
different for a `ptr`-typed param vs a plain `i64` SSA param; Cranelift has
an equivalent split). This is a representation change, not a one-line
patch, matching the caution already documented for item 3's struct-arg
leak -- attempting it inside this wave without a full regression pass across
conformance/self-host/bootstrap risked an unreviewable change. Left as a
standing, honestly-scoped OPEN item with the fix direction stated for
whoever picks it up. Not gated (would fail today); `tests/known_failures/`
convention followed. CLAUDE.md gotcha #11 updated with this finding inline.

### 7b. NEW (found in the spawn/actors/channels/sync wave): a closure/fn-value captured by `spawn` does NOT snapshot -- a genuine cross-thread DATA RACE, silent lost updates -- NOT FIXED

`tests/known_failures/spawn_closure_shared_env_race.kry` (repro below,
verified 10/10 failures on JIT and 7/10 on AOT at 50 threads x 2000 calls
each). Ranks with item 7 per this ledger's doctrine -- a silent wrong answer
outranks a crash -- and is the SAME underlying mechanism (item 7 / CLAUDE.md
gotcha #11's mutated-scalar-capture write-back) turning into a real,
measured race the moment the closure is shared across threads instead of
called from one.

Every OTHER `spawn` capture kind is a documented, verified SNAPSHOT: str,
array, map, struct, and (as of `721a9cf`, this session's predecessor) enum
are all deep-copied at the `Instruction::Spawn` boundary so the spawned
thread privately owns its capture (`docs/09-concurrency.md`: "every capture
is a snapshot ... this holds uniformly for arrays, structs, maps, and
strings"). Read directly in both backends' `Instruction::Spawn` arg-copy
match (`kryos-codegen-cranelift/src/codegen.rs` and `kryos-codegen-llvm/src/
codegen.rs`): the `MirType::Function` / `MirType::Shared` arm is the ONE
exception -- it calls `kryos_arc_retain` on the closure's env box instead of
cloning it. Every spawned thread that captures the SAME closure value shares
the identical env allocation with the parent and every sibling thread.

**Why sharing (not just "not snapshotting") is a real bug, not merely a
documentation gap:** a closure with a single mutated scalar capture whose
tail value IS that identifier (gotcha #11's "RESOLVED" persistence case,
e.g. `let bump = || { count = count + 1  count }`) persists its state via a
NON-ATOMIC read-call-then-write-back with no lock: the generated
`{name}_env` thunk calls the closure body (which reads `env[slot]`,
computes, returns) and THEN, as a SEPARATE instruction, stores the return
value back into `env[slot]` (both backends, `if let Some(slot) =
mutated_slot { ... store ... }`). Two threads calling the same shared
closure concurrently can both read the same pre-increment `env[slot]`
before either writes back, silently losing one thread's increment --
measured directly:

```
use std::sync::{wait_group}
fn main() {
    let mut count: i64 = 0
    let bump = || { count = count + 1  count }
    let wg = wait_group()
    wg.add(50)
    let mut i = 0
    while i < 50 {
        spawn {
            let mut j = 0
            while j < 2000 { bump()  j = j + 1 }
            wg.done()
        }
        i = i + 1
    }
    wg.wait()
    let final_val = bump()   // read the CLOSURE's own state via its own return
    println("final={final_val} expected=100001")
}
```
JIT: 10/10 runs printed a value less than 100001 (observed range ~46758-
72073). AOT: 7/10 runs printed a value less than 100001 (observed range
~97745-99895 -- a narrower, still-real window, likely because LLVM's spawn
thread startup has more fixed overhead, reducing the contention window, not
because AOT is exempt). Confirmed this is unrelated to the ALREADY-
documented "outer variable stays frozen" behavior (gotcha #11's by-move
semantics: reading `count` directly after defining `bump` correctly stays
`0` forever, single-threaded, both backends, no bug there) -- this finding
is specifically about the CLOSURE's OWN persisted state (read via calling
`bump()` again, not via the outer variable), which should climb
monotonically and instead loses updates under concurrent access.

**Not fixed this session (scope discipline, matches item 3/item 7's
precedent):** two fix shapes exist, neither is a one-line patch:
(a) make `Function`/`Shared` spawn captures snapshot like every other heap
kind -- needs a per-closure-shape "deep copy this env" codegen helper
generated per closure (mirroring how `__kryos_drop_<Struct>` is generated
per struct), on both backends; or (b) make the call-plus-writeback atomic
under a per-closure lock, which taxes the common uncontended single-thread
case to fix a path most programs don't exercise. Filed with a verified,
both-backends, both-directions-of-scale repro instead of a rushed patch.
**Docs corrected the same session** (this was a real documentation gap, not
just a code gap): `docs/09-concurrency.md`'s spawn section now states the
closure/fn-value exception to the snapshot contract explicitly, and CLAUDE.md
gotcha #22 gained a matching bullet.

### 8. curried (2-level) generic closure return fails to BUILD on AOT -- JIT/AOT divergence, NOT FIXED

`tests/known_failures/closure_curried_generic_aot_crash.kry`. Found this
session while hunting "closures returned through generics" per the wave
brief.

```
fn curry_add<T>(a: T) -> fn(T) -> fn(T) -> T {
    return |b: T| (|c: T| a + b + c)
}
fn main() {
    let step1 = curry_add(1)
    let step2 = step1(2)
    println(to_string(step2(3)))
}
```
`kryos run`: prints `6`, correct. `kryos build --release`: fails LLVM
codegen outright -- `error: load operand must be a pointer to a first class
type ... load %T, ptr %_1_arg` (clang rejects the emitted `.ll`; the generic
type parameter `T` reaches LLVM IR emission UNRESOLVED for the INNER
closure). The IDENTICAL shape with a concrete type (`i64` instead of `T`,
no generics at all) builds and runs correctly on both backends -- isolates
this to generics specifically, not to currying/nesting in general (a
non-generic curry already works per `tests/conformance/conf_closures.kry`
and CLAUDE.md gotcha #11).

**Root cause:** `pending_lambda_ret_hint` (`kryos-mir/src/lower.rs`, the fix
that closed the single-level "generic function RETURNING a closure at
T=f64" item in this ledger's CLOSED table) stages a concrete
per-instantiation signature only for a lambda DIRECTLY returned by the
enclosing generic function's `Stmt::Return`. It does not recurse into a
SECOND lambda returned BY that first lambda's own body, so the innermost
closure's parameter type stays the erased/unresolved generic placeholder,
which LLVM IR emission then tries to `load` as if it were a concrete type.

**Not attempted as a fix this session:** the natural fix (make
`pending_lambda_ret_hint` propagate recursively through a chain of nested
lambda-returning-lambda bodies, not just one level) is more contained than
item 7 above, but was deprioritized in favor of thoroughly characterizing
all three findings from this wave rather than rushing a codegen change
without a full gate pass. A reasonable next step for whoever picks this up:
check whether `current_ret_ty`'s OWN nested `fn(A) -> B` structure can be
walked one level deeper when the outer lambda's body is itself a bare
lambda literal, staging a second hint for that inner lambda the same way
the outer one already gets staged. Not gated (would fail to build today).
CLAUDE.md's generic-closure-return entry updated with this finding inline.

### 9. `\|\|`-continuation parse trap also swallows closure literals silently -- previously mis-cataloged as a "block-tail closure capture scoping" bug -- DOCS CORRECTED, not a new code defect to fix

`tests/known_failures/closure_pipe_continuation_silent_wrong.kry`. Found
this session while re-verifying the "closure as the TAIL VALUE of a block
cannot capture that block's earlier let bindings" boundary named explicitly
in the wave brief -- re-verification showed the EXISTING explanation for
that boundary was wrong.

The parser has no newline-awareness at all (verified by reading
`kryos-lexer`/`kryos-parser`: tokens carry only byte-offset spans, and the
Pratt expression loop keeps consuming any token with infix binding power
regardless of a line break). CLAUDE.md's hard rule 1 already documents `-`,
`(`, `[` as trap tokens that silently continue the previous statement when
they open a new line; this session found `||` (the closure literal opener,
which doubles as boolean-or) is a FOURTH instance, and it can produce a
SILENT WRONG VALUE with zero diagnostic when both sides happen to be `bool`:

```
fn main() {
    let a: bool = false
    let b: bool = true
    let c: bool = a
    || b
    println(to_string(c))   // prints "true", not "false" -- (a || b) merged
}
```

More importantly: this is the TRUE root cause of what CLAUDE.md previously
described as a distinct closure-capture-scoping limitation ("a closure that
is the tail value of a block cannot capture that block's earlier let
bindings", cited with the exact repro `let g = { let base = "x"  || base +
"!" }` failing with `E0102`). Re-diagnosed this session: `E0102` there is
`base` self-referencing itself inside its OWN merged initializer (`let base
= ("x" || base + "!")`), nothing to do with block or closure scoping.
**Proof:** inserting any statement that cannot be continued via `||`
between the two removes the error and the closure captures cleanly --
`{ let base: str = "x"  if true { }  || base + "!" }` returns `"x!"` on
both backends (verified live, both directions). A closure literal can
capture ANY function-level or block-level `let` binding declared earlier in
the same scope; there is no scoping boundary.

**Why not filed as a code bug to fix:** this is the SAME accepted-trap
category as `-`/`(`/`[`, which CLAUDE.md's hard rule 1 already documents as
a permanent, known ASI-class hazard programmers must avoid (not a defect
awaiting a compiler fix -- fixing it generally would require adding
line-tracking to the lexer and consulting it in the Pratt continuation loop
for every binary-operator token, a cross-cutting grammar change with a wide
blast radius against existing code that intentionally relies on trailing-
operator line continuation, well outside a wave scoped to closures). The
deliverable here is the DOCS CORRECTION: CLAUDE.md gotcha #1 now lists `||`/
`|` explicitly and states the mechanism is general (not exhaustive to three
tokens); gotcha #11's "block-tail closure" entry is rewritten with the
correct root cause and the same (already-correct, now correctly explained)
workaround. `tests/known_failures/closure_pipe_continuation_silent_wrong.kry`
kept as a durable repro of the silent-value variant since it is more
dangerous than the previously-known error-shape variants (`-`/`(`/`[` all
type-cascade into visible errors in the CLAUDE.md examples; this one does
not, when both sides are `bool`).

### Ruled out this session (type system / generics / monomorphization wave) -- probed, all correct on BOTH backends, no bug found

Wrote and ran (JIT + AOT, both backends, value-asserted, not just
exit-code) minimal repros for each of the following; none reproduced a
divergence or a wrong value, so none get a ledger entry beyond this line:
deeply nested generic structs (`Box<Box<Box<f64>>>`, three levels, chained
`.get().get().get()`); a self-referential generic struct
(`Tree<T> { val: T, kids: [Tree<T>] }`) with an `f64` payload read back
through nested array-of-struct indexing; multiple instantiations of the
same generic function AND the same generic method at different concrete
types (`i64`/`str`/`f64`) interleaved in one program, including mutating
one instantiation's derived value and confirming the sibling instantiation
is untouched; a generic struct with a HEAP field (`[f64]`) read through a
`std::iter::map`/`fold` HOF chain; `Option<T>` returned from a generic
method at `T=f64` through `Some`/`None` `match` arms; a `throw` raised
from inside a generic method's body (`Box<T>.unwrap_or_throw`) caught by
the CALLER of a separate generic function, at both an `f64` and a `str`
instantiation in the same program; a generic struct with a FUNCTION-TYPED
field (`Transformer<T> { f: fn(T) -> T }`) invoked at `i64`/`str`/`f64`
instantiations; multi-parameter generics (`Pair<A, B>`) including a
`.swap()` returning `Pair<B, A>` and a heap-field-in-both-params instance
(`Pair<str, [f64]>`). All printed the correct value, byte-identical
between `kryos run` and `kryos build --release`.

### Ruled out this session (closures / fn-values / captures wave) -- re-verified boundaries hold, both backends agree, no bug found

Re-verified live (value-asserted, both `kryos run` and `kryos build
--release`) every boundary named in the wave brief, all still correct and
IDENTICAL on both backends: escaping closures snapshot heap captures at
storage time (array `len` read through a stored closure sees the pre-push
value); a captured MAP mutated by key writes through to the outer map; an
array-of-structs mutated through a NESTED field mutates the shared element;
a self-referential closure built via reassignment captures the OLD binding
(`fact(5)` returns `5`, not `120`); a closure that is the TAIL VALUE of a
block cannot capture that block's earlier `let` bindings under `E0102` --
**this last one's EXPLANATION was wrong, see item 9 above; the observable
symptom (E0102 on that exact input) is still accurate.**

Also probed and found CORRECT, no bug: closures stored directly in a struct
field literal (not via a factory function) seeing later scalar mutation;
an array of closures each capturing a distinct per-iteration loop-local;
a map of closures; closures forwarded through 2+ layers of currying with
MIXED capture kinds (str + mutated scalar via a fresh named intermediate,
not a bare tail merge) persisting correctly per-instantiation; a generic
closure-return stored in a container array at two instantiations (i64) plus
a `T=str` instantiation, read back correctly; recursion via a fn VALUE
stored in a STRUCT FIELD (`rb.f = |n| if n<=1 {1} else {n*rb.f(n-1)}`) --
unlike the documented bare-`let`-reassignment snapshot boundary, routing the
self-reference through a struct field's mutable slot DOES see the live
value and computes `fact(5) = 120` correctly on both backends (a real,
useful workaround for the documented boundary, worth adding to CLAUDE.md if
a future session wants to formalize it -- not done here to keep this wave's
docs changes to what was directly asked for); a closure captured inside a
`spawn` block via a struct field (`h.f(7)` inside `spawn { }`) returning the
correct value on both backends, consistent with the extensive spawn-closure
work already CLOSED in this ledger.

### Ruled out this session (spawn / actors / channels / sync primitives wave) -- probed hard (many-run, value-asserted, both backends), no bug found beyond item 7b above

Every repro below was run 5-10x per backend (concurrency bugs are
probabilistic; a single green run proves nothing) with a value assertion,
not just an exit code, per this session's own doctrine:

- **Every non-closure `spawn` capture kind under real cross-thread load:**
  `str`/`array`/`map` captured directly (not via a struct) into 200
  per-iteration spawn blocks with a FRESH value each iteration, read back
  through a channel -- no use-after-free, no box-reuse (the class of bug the
  enum-capture fix `721a9cf` closed), correct values 5/5 JIT + 5/5 AOT.
- **`fn` VALUE captures (a bare named-function reference, not a lambda):**
  `let f = helper` then `spawn { f(21) }` x50 threads -- the zero-capture
  `RValue::Closure` path still allocates a real ARC-boxed env (confirmed by
  it working, not by reading codegen), so `kryos_arc_retain`'s magic-sentinel
  defensive check (`kryos-rt/src/arc.rs::is_arc_ptr`) never has to fall back
  to its "static function pointer, no-op" path here. 10/10 JIT + 10/10 AOT,
  correct value every run.
- **Actor mailbox under concurrent senders:** 30 spawned threads x 1000
  messages each to ONE actor (`c.bump(1)`), read back via a reply channel
  (actor handlers with a non-void return type are now a clean COMPILE ERROR
  -- `E0110`, "actor sends are asynchronous fire-and-forget" -- a real,
  useful diagnostic already in place, not a gap). Exact count every run,
  6/6 JIT + 6/6 AOT, no lost/duplicated messages.
- **MPMC channel under real multi-producer/multi-consumer load:** 20
  producers x 500 sends, 10 consumers draining via `recv`, verified both
  exact COUNT and exact SUM (catches a duplicate that a count-only check
  would miss) -- exact 6/6 JIT.
- **`Mutex`/`Semaphore` under contention:** `Mutex` captured directly (not
  wrapped) into 10 spawned threads x 1000 lock/increment/unlock cycles --
  correct despite the Kryos-level `locked`/`dropped` bookkeeping fields
  being independently DEEP-COPIED per thread (struct capture rule), because
  the real exclusion comes from the shared native OS mutex behind the
  `handle: ptr` scalar field, not from the bookkeeping bools. `Semaphore(3)`
  with 30 competing acquirers never exceeded 3 concurrent holders. 6/6 JIT +
  6/6 AOT both primitives.
- **`ChanWaitGroup` under 40 concurrent workers:** exact count every run,
  8/8. **`ChanOnce` under 20-way contention:** fired 20/20 times (NOT once)
  -- this is NOT a new finding, it is the EXACT documented behavior in
  `chan.kry`'s own doc comment ("Not currently atomic across spawn-tasks;
  use a semaphore or external mutex for cross-task once semantics"),
  re-verified as accurate, not re-filed.
- **Per-iteration closure with a HEAP capture, shared via `spawn`, parent
  scope torn down immediately after spawning:** 2000 threads, no
  premature-free / use-after-free / double-free -- the synchronous
  `kryos_arc_retain` at the `Spawn` call site (before `kryos_spawn` starts
  the OS thread) is correctly sequenced ahead of the parent's own scope-end
  release, so refcounting holds even though the SAME box is also
  DATA-RACED by item 7b above when the closure MUTATES a capture (holding a
  reference alive under a race is a different property from serializing
  writes to it -- this item confirms the former, item 7b disproves the
  latter).
- Existing green gates re-run for flakiness, not just once: `conf_spinlock_
  mutex` and `conf_spawn_agg_capture_abi` 8/8 clean on JIT.

### Ruled out this session (stdlib correctness sweep wave, 66 modules) -- value-asserted live, no bug found beyond the fmt::format fix in CLOSED below

Per-module pass/fail table (value-asserted against hand-computed expected
outputs including edge cases, both backends where the module has real
logic -- see the fmt.kry entry in CLOSED for the one real defect this wave
found):

| Module | Verdict | What was probed |
| --- | --- | --- |
| `collections` (List/Set/Dict/Stack/Queue/Deque\<T\>) | PASS | i64-erasure caveat already honestly documented in-module; List.insert/remove index shifts, Set dedup-by-value, Dict overwrite-vs-grow, get_or default -- all correct |
| `set` (sorted `[i64]`) | PASS | insert/contains/remove binary-search boundary indices |
| `iter` | PASS | sort/sort_by/sort_by_key on `str` (the exact erasure shape that broke `List.contains`) -- correct, content-compared not pointer-compared; group_by/unique/dedup/scan/zip/unzip/windows/chunks on `str` and mismatched-length arrays |
| `math` | PASS | sqrt/cbrt/exp/ln/atan magnitude-seeded Newton convergence at extreme scales already hardened from prior sessions; not re-litigated beyond spot checks |
| `mathx` | PASS | `isqrt` near i64::MAX: `mid = (lo+hi+1)/2` looks like a classic binary-search overflow hazard (theorized, then measured) -- reproduced i64::MAX, an exact large square, and 1e18; all correct despite the interval momentarily going negative mid-search. Ruled out; wired as a regression anyway since the hazard is real even though it doesn't fire |
| `stat` | PASS | mean_x1000/variance_x1000 negative-sum truncation-toward-zero sign consistency |
| `matrix` | PASS | non-square mul (2x3 * 3x2), transpose, scale, add -- my first hand-computed "expected" value was ITSELF arithmetically wrong (58,64,126,144); the actual output (58,64,139,154) is correct -- re-verified by hand twice before ruling out |
| `tensor` | NOT DEEPLY PROBED | thin FFI wrapper over kryos-rt native tensor ops; wrapper glue (f64_to_bits round-trip, arr_data_ptr) read correct, native op correctness out of scope for a stdlib .kry sweep |
| `datetime` | PASS | from_timestamp epoch 0, 2024 leap day, 2024-03-01 rollover, year 2000, year 2100 (century non-leap), negative epochs to pre-1900, far-future epoch -- all exact |
| `duration` | PASS (already gated) | covered by `conf_stdlib_untested.kry`; not re-probed |
| `path` | PASS | join/dirname/basename/extname/stem/normalize/split/relative/with_extension/starts_with; a 40-segment join+split round trip to stress the bare (unassigned) `push()` pattern used throughout -- see the `push()` note below |
| `pathext` | PASS | normalize traversal-escape guards: bare root, Windows drive letter, UNC share -- `..` cannot pop past any of the three anchors |
| `re` | PASS | capture groups + $N/$$ replacement, zero-width match counting (find_all/replace_all/split), `^` anchor not re-matching per-slice, `escape()` round-trip, is_email/is_ipv4/is_hex validators including octet-range and anchoring, two-digit group refs ($11 vs $1+"1") |
| `crypto` | PASS (spot check) | HMAC-SHA256 RFC 2104 padding already hardened from prior sessions; `random_int`'s doc-claimed "rejection sampling" is NOT actually implemented (plain modulo on a rejection-sampling-shaped comment) -- real but negligible modulo bias for any range far below 2^63, and a theoretical i64::MIN-negation edge (1-in-2^64) that could return one value below `min`; not fixed (statistical, no single-value assertion catches it, matches this ledger's own "honestly scoped" precedent for low-severity items) |
| `hash` | PASS | crc32 against the standard "hello" (907060870) and "123456789" (3421780262 check-value) reference vectors, crc32("") == 0, fnv1a64 offset basis |
| `jwt` | PASS (already gated) | covered by `conf_stdlib_untested.kry` (tamper/alg-none/empty-secret rejection); not re-probed |
| `bytes` | PASS (already gated) | covered by `conf_stdlib_untested.kry` |
| `semver` | PASS | prerelease precedence full chain (alpha < alpha.1 < beta < beta.2 < beta.11 < rc.1 < release), numeric-not-lexicographic identifier compare (beta.2 < beta.11), malformed/extra-segment rejection |
| `histogram` | PASS (read only) | underflow/overflow/percentile-cumulative-walk logic already carries an explicit prior-session fix comment; not independently re-probed live |
| `fmt` | **1 REAL BUG, FIXED** -- see CLOSED table |
| `numfmt` | PASS (read only) | i64::MIN hex/bin/decimal_padded already hardened from prior sessions |
| `random` | PASS (read only) | range_i64 u64-span overflow fix and next_bit sign-bit fix already hardened from prior sessions |
| `slice_ops` | PASS (read only) | take/drop/partition/is_sorted/bsearch -- straightforward, no risk signature found |
| `diff_ops` | PASS (already gated) | covered by `conf_stdlib_untested.kry` |
| `fuzzy` | PASS (read only) | levenshtein/jaro/jaro_winkler already codepoint-aware (fixed from a prior byte-indexed bug per the module's own comments) |
| `probable` | PASS | `majority_vote`'s generic `pj.value == pi.value` on `Probable<str>` does CONTENT equality (not pointer identity) -- confirmed live with two distinct-object same-content strings built via runtime concatenation, not string-literal interning |
| `heap`/`queue`/`stack`/`deque`/`lru`/`bloom`/`interval`/`trie` | PASS (read only + spot-gated) | heap/queue/stack/deque/bloom/interval/trie already covered by `conf_stdlib_untested.kry`; lru read (zero-cap no-op, LRU-eviction-by-recency) but not independently live-probed this session |
| `csv` | PASS (read only) | quote-opens-only-at-field-start, doubled-quote escape, blank-line-is-zero-fields -- all carry prior-session fix comments |
| `json` | PASS (spot check) | string escape/unicode-surrogate-pair decoding and integer-exact-i64 number parsing already hardened from prior sessions; not independently re-probed live beyond reading |
| `agent`/`agent_bridge`/`backoff`/`ratelimit`/`cost`/`circuit`/`semaphore`/`db`/`os`/`process`/`io`/`fs`/`net`/`http`/`chan`/`sync`/`log`/`term`/`ffi`/`wasm`/`smtp`/`llm`/`option`/`result`/`test`/`tracked`/`strext`/`string` | NOT REACHED THIS WAVE | outside the explicitly-named priority list, or already exercised by other suites (`string`/`option`/`result`/`iter` have 18-23 existing test-file references, the highest in the repo); see "did NOT fix" below |

**A pattern probed and RULED OUT across multiple modules:** bare, unassigned
`push(arr, v)` (discarding the return value, contra the documented "always
write `arr = push(arr, v)`" convention) appears throughout `path.kry` and
`string.kry`. Theorized this could silently drop elements past a capacity
reallocation (the exact shape of the documented `let b = push(a, v)` / read
`a` footgun). Measured directly: 50 sequential bare pushes on a fresh `[i64]`
array, and a 40-segment `path::join` + `path::split` round trip -- both
correct on JIT and AOT. The array header pointer is evidently stable across
a `push`-triggered reallocation (only the internal data buffer moves), so
this specific pattern is safe as used; the DIFFERENT documented footgun
(`let b = push(a, v)` then reading the ORIGINAL variable `a`) remains real
and unrelated.

---

## CLOSED — with the evidence that closed it

| Item | Evidence |
| --- | --- |
| **`std::fmt::format` took `args: [any]` -- EVERY non-i64 argument silently rendered as its raw pointer/bit pattern instead of its value, and the function's OWN doc-comment usage example was independently broken** | Found in the stdlib correctness sweep wave while probing `fmt` (a module the priority list flagged as "silent wrong number" risk). Repro: `format("Hello, \{0\}! You are \{1\}.", ["Alice", "30"])` printed `"Hello, 140698911633600! You are 140698911633632."` -- large heap-pointer-shaped integers, not the strings -- on every run, both backends, 100% reproducible (not probabilistic). ROOT CAUSE: `any` is erased to a bare i64 with no runtime type tag (the same limitation as OPEN item #6/CLAUDE.md gotcha #22), and `format`'s `args: [any]` parameter routed every argument through that erased slot; `to_string(args[i])` then printed the slot's raw bits reinterpreted as a number -- correct-looking for an i64 argument (bits==value) and silently wrong for `str`/`f64`. A SEPARATE, compounding defect: the doc comment's own example, `format("Hello, {0}! You are {1}.", ["Alice", "30"])`, is unusable as literally written -- Kryos strings interpolate universally, so the bare `{0}`/`{1}` in that DOUBLE-QUOTED SOURCE LITERAL are consumed by the compiler itself (as the expressions `0`/`1`) before `format()` ever runs; the literal call silently becomes `format("Hello, 0! You are 1.", [...])`, which has no `{0}`/`{1}` left to substitute and returns unchanged. Confirmed live: the verbatim doc example prints `"Hello, 0! You are 1."` with zero error, on a function whose entire purpose is template substitution. FIX: changed the signature from `args: [any]` to `args: [str]` (matching `std::string::format`, a sibling function that never had this bug because it was `[str]` all along) -- callers now pre-stringify each argument with `to_string(x)`, eliminating the `any`-erasure path entirely for this function (no ABI change needed, unlike the general `any` limitation in OPEN item #6, because `format`'s signature could simply stop using `any`). Doc comment rewritten to state the interpolation caveat explicitly and show the required escaped-brace invocation (`\{0\}` or `{{0}}`). Proof both ways: `git stash` the fix, rebuild-free rerun (stdlib `.kry` is read from disk, no `cargo build` needed) -- `tests/conformance/conf_stdlib_correctness_sweep.kry` fails at the FIRST assertion (`CONF FAIL: fmt::format substitutes real str values...`, exit 1); fix restored -- same file prints `PASS`, both `kryos run` and `kryos build --release`. No prior call site in the repo used `std::fmt::format` (grep across `tests/`/`examples/` found zero references), so the signature change is a pure fix with no blast radius. Regression: `tests/conformance/conf_stdlib_correctness_sweep.kry` (also covers isqrt/normalize/crc32/datetime/matrix/semver/iter/collections edge cases from the same sweep). Gates: conformance 51/51, tier1+tier2 GREEN, bootstrap 16/16. `README.md`/`docs/BUGS.md`'s "conformance 50/50" claims corrected to 51/51 (`tests/docs_status_gate.sh` caught the drift and failed until corrected). |
| **generic method bare self-field passthrough returning a COMPOUND shape (`-> [T]`, `-> (T, i64)`) kept the erased i64-slot element for non-pointer `T` -- CLAUDE.md gotcha #17 residual** | Found the fix already drafted (uncommitted) in the working tree at session start; this session's contribution was verification, gating, and doc correction, not the original diagnosis -- recorded here honestly rather than claimed as a from-scratch find. `fn all(self: Holder<T>) -> [T] { return self.items }` at `T=f64`: `Holder<f64>.all()[0]` printed the raw i64 bit pattern of `1.5` (`4609434218613702656`), identically on both backends (shared-MIR, not a divergence) -- confirmed live via `git stash` of the diff + rebuild. ROOT CAUSE: `instance_ret_needs_monomorphization` (`kryos-mir/src/lower.rs`) only recognized a bare-struct-literal-mentioning-`T` return shape as needing per-instantiation monomorphization; a `TypeExpr::Array`/`TypeExpr::Tuple` return that merely MENTIONS `T` fell through to `false`, so a bare self-field passthrough of such a field stayed on the single erased-to-i64 compiled copy (the exemption was designed for a bare `-> T` SCALAR slot, safe to reinterpret anywhere, not a CONTAINER whose elements each need a real type). FIX: extended `instance_ret_needs_monomorphization` with `Array`/`Tuple` arms mirroring the existing `Generic` arm. Proof both ways: `git stash` the fix, rebuild -> `Holder<f64>.all()[0]` prints the bit pattern; fix restored, rebuild -> prints `1.5`, on BOTH `kryos run` and `kryos build --release`. Extended verification this session (not in the original diff): a BARE TUPLE-FIELD passthrough (`fn get_pair(self: PairHolder<T>) -> (T, i64) { return self.pair }`, not a tuple-literal-construction body) also resolves correctly post-fix -- the fix generalizes symmetrically to both container shapes, confirmed via a fresh minimal repro, both backends. Regression WIRED INTO THE GATE this session (was previously only `tests/smoke/test_generic_compound_return.kry`, which is NOT part of any gate -- `tests/smoke/` has no automated runner beyond exit-code, per its own README): added `tests/conformance/conf_generic_compound_return_f64.kry` (value-asserted, `expect()`-style, matching the existing conformance convention), which IS swept by `tests/conformance/run_conformance.sh`'s `conf_*.kry` glob and therefore by `kryos-loop.sh gates`. Gates: conformance 50/50, tier1+tier2 GREEN, bootstrap 16/16. Docs: CLAUDE.md gotcha #17's residual note rewritten from "known gap" to RESOLVED; `README.md`/`docs/BUGS.md`'s "conformance 48/48" claims corrected to 50/50 (2 new conformance files; `tests/docs_status_gate.sh` caught the drift and failed until corrected -- proof the gate itself works, not just a courtesy edit). |
| **generic function RETURNING a closure at `T=f64` integer-added the float BIT PATTERNS instead of the values -- CLAUDE.md gotcha #22 residual** | Found the fix already drafted (uncommitted) alongside the item above; same honesty note -- this session verified, gated, and documented it. `fn make_appender<T>(suffix: T) -> fn(T) -> T { return \|x\| x + suffix }` at `T=f64`: `make_appender(0.5)(2.0)` printed a ~300-digit garbage integer instead of `~2.5`, identically on both backends -- confirmed live via `git stash` + rebuild (exact garbage value reproduced: `8988465674311580...` truncated). ROOT CAUSE: the type checker's per-lambda-param type table (`lambda_param_types`) is baked from a SINGLE check pass over the unspecialized generic template, where `T` never resolves to a concrete type -- it has nothing to give MIR for a closure LITERAL that is directly `return`ed from a generic function, so the closure's own un-annotated param stayed i64-erased at every instantiation regardless of what `T` resolved to at a given call site. FIX: `pending_lambda_ret_hint` (`kryos-mir/src/lower.rs`) -- `Stmt::Return`, when its value is directly a `Lambda` literal and the ENCLOSING function's already-monomorphized `current_ret_ty` is a concrete `fn(A) -> B` of matching arity, stages that concrete per-instantiation signature; the `Expr::Lambda` codegen arm consumes it as a fallback ONLY when the type checker's own per-param resolution came up empty, so it cannot override a real annotation or a HOF-inferred param. Proof both ways: `git stash` the fix, rebuild -> the f64 instantiation prints the garbage integer (i64 instantiation and str instantiation both still correct, isolating the bug to exactly the erased-float-add path); fix restored, rebuild -> f64 prints `~2.5`, i64 and str instantiations unchanged (no regression from the added fallback), on BOTH backends. Regression WIRED INTO THE GATE this session: added `tests/conformance/conf_generic_closure_return_f64.kry` (was only in ungated `tests/smoke/`) covering all three instantiations (`i64`/`f64`/`str`) in one program so a future change can't silently regress one while "fixing" another. Gates: conformance 50/50, tier1+tier2 GREEN, bootstrap 16/16. CLAUDE.md gotcha #22's mk_appender entry rewritten from "T=f64 has a residual VALUE bug" to RESOLVED. |
| **generic struct/enum base name ending in `_` (e.g. `Box_<T>`) broke bare-passthrough instance methods -- `unresolved external symbol <method>` on BOTH backends** | Found while building `tests/fuzz`'s own generic-struct template (named `Box_` to dodge a suspected-reserved `Box`). Minimal repro: `struct Box_<T> { val: T }  impl<T> Box_<T> { fn get(self: Box_<T>) -> T { return self.val } }` then `Box_{val:"ab"}.get()` -- `kryos run` fails to LINK (`LNK2001: unresolved external symbol get`), `kryos build --release` fails to CODEGEN (`use of undefined value '@get'`); both backends fail IDENTICALLY (shared MIR, not a backend bug -- confirmed via `--emit-llvm`, not guessed). ROOT CAUSE: 6 call sites in `kryos-mir/src/lower.rs` recovered a monomorphized name's base struct via `name.split("___").next()` to fall back to the erased-fast-path `method_owners`/`impl_method_generic_info` lookup. `Box_<str>` mangles to `Box____str` (base `Box_` + the `___` separator + suffix `str` = 4 consecutive underscores); splitting on the FIRST 3-underscore run consumes one of the base's own trailing underscores, recovering `Box` instead of `Box_` -- every fallback lookup then missed and silently resolved to the BARE unmangled method name. FIX: added `mono_base_name(ctx, name)`, which recovers the base by checking ALL registered struct/enum names for a `base + "___"` PREFIX (longest wins) instead of blindly splitting -- and checks prefix matches BEFORE any exact-name shortcut, because a monomorphized instance is ALSO registered under its own full mangled name (`struct_defs` gets both), so an exact-match-first order returns the mono name as its own base and silently reintroduces the identical bug (caught by re-testing after the first attempt failed identically, not assumed correct the first time). Replaced all 6 identical `.split("___").next()` call sites. Proof both ways: pre-fix, `tests/conformance/conf_generic_underscore_name.kry` fails to even BUILD on both backends (`LNK2001: unresolved external symbol get`+`dbl`, verified via `git stash` of the fix + rebuild); post-fix, both backends print `PASS`. Regression covers a bare passthrough getter, a self-operating method (`v + v`, `str` concat) coexisting on the same base, and a generic ENUM with a trailing-underscore base name. Gates: conformance 48/48, tier1+tier2 GREEN, bootstrap 16/16. Not a differential (JIT vs AOT) bug in the end -- both backends agree by failing identically -- but found via the same minimal-repro/read-the-IR discipline the differential harness (this wave's deliverable) is built on. |
| **capability escape via closure/fn-value laundering — parameter/local/return/passthrough-chain/actor-message/spawn/generic/dyn-Trait shapes (container storage remains OPEN, item 1 above)** | Root cause: `fn_capabilities` (`kryos-capabilities/src/checker.rs`) was keyed by NAME; calling a value bound to a parameter/local of function type resolved to nothing in that map, so a closure's authority never propagated to the calling scope regardless of what it did at runtime — verified live pre-fix: a `deny!(fs:read)` block did NOT stop a closure constructed before the denial from being invoked (through a zero-capability `zero_cap_tool`) INSIDE it, printing the secret, `check --strict-capabilities` exit=0, no diagnostic. FIX: (1) `hot_params` — a structural, capability-value-independent fixed point over the whole program identifying which fn-typed PARAMETER indices are invoked, directly or by being forwarded as a bare argument into another (transitively) hot position (covers passthrough chains of any depth); (2) `fn_return_closure_caps` — a fixed point resolving what authority a closure-RETURNING function's returned value carries (a lambda literal's own body capability, a named-function reference, or — recursively — another closure-returning function's return, including a simple passthrough that depends on ITS OWN parameter, resolved against the ACTUAL argument at that call); (3) at every call site with a hot argument position, `accumulate_hot_extra_caps` resolves the SPECIFIC argument passed (`resolve_closure_caps`: a lambda literal, a `let`-bound local traced via a per-function `build_local_closure_caps` map, a named function/builtin reference, a call into (2), or — when it is one of the CURRENT function's own fn-typed parameters — deferred via `ClosureCapsResult::DependsOnParam` to that function's OWN call sites, which is what keeps a `std::iter`-HOF-shaped forwarding function requiring nothing extra) and unions that authority into the call's requirement, checked against the CALLING scope exactly like any other gated operation — so a `deny!` block, an actor's declared ceiling, or any other boundary a closure is routed through now sees the real requirement. Unresolvable provenance (a closure whose origin can't be traced at all) requires `Capability::All`, the same conservative default already documented for the raw-memory escape. Verified BOTH directions, both modes (inferred + `--strict-capabilities`): pre-fix binary compiles+runs the `deny!` repro clean and prints the secret from inside the denied scope (5/5 reproduced); post-fix binary rejects it with E0507 citing the closure argument, in both modes, while `std::iter::map/filter/fold` with a PURE closure still needs no annotation (no cascade) and the SAME HOF with a PRIVILEGED closure correctly requires the capability. Blast-radius swept live (not just the parameter case): closures forwarded through 2+ passthrough call layers, actor fire-and-forget message sends (needed a second fix — actor handlers have NO implicit `self` in their own `params`, unlike a struct `impl` method, so the method-call self-offset translation was off-by-one and silently dropped index-0 coverage until corrected), `spawn`, a generic `fn<T>`, and `dyn Trait` method dispatch are ALL closed — each individually reproduced escaping pre-fix and rejected post-fix inside a `deny!(fs:read)` block. The REJECTED naive alternative ("any call through a non-directly-named fn-typed value requires `Capability::All`") was re-verified as unusable by MEASUREMENT, not just re-assumed: 22 genuine callback-taking `std::iter` HOF signatures, ~55 raw call sites to those names across the stdlib/self-host/examples/ecosystem (a few are `std::string::find` name collisions, not the iterator HOF; dozens remain genuine) — every one would need `@capabilities(all)` under the blanket policy, none do under the shipped call-site-sensitive one. NOT closed: a closure/fn-value read back OUT OF A CONTAINER (struct field, array element, map value) — `hot_params` only recognizes a parameter whose OWN type is `fn(...) -> ...`, so `Registry{reader: fn()->str}`/`[fn()->str]`/`map<str,fn()->str>` are invisible to it; reproduced live, NOT gated, sketched as item 1 in OPEN above. Also verified: `kryos audit` still never lists `zero_cap_tool` post-fix — determined this is CORRECT, not a residual defect (audit is a pure syntactic `@capabilities(...)` scan with no inference, so it never lists ANY unannotated function, including a legitimately call-site-polymorphic one like a HOF; it was never specifically "blind to closures", it is blind to every unannotated function equally). Gates: `tests/security_gate.sh` (extended, checks #4-6: reject/no-over-reject/no-cascade + positive privileged-HOF check), conformance 47/47, tier1+tier2 GREEN, bootstrap 16/16. Docs corrected: `docs/10-capabilities.md`, `README.md`, `STABILITY.md`, `docs/capability-roadmap.md` all previously claimed NO soundness for any closure indirection; now state the precise (much larger) sound surface and the precise (much narrower) remaining gap |
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
| **bootstrap WINDOWS-ONLY exit -1 in tokenize (ex-item #2)** | NOT a fault, not Defender, not the pool allocator, not heap corruption -- confirmed by an `atexit`-hook diagnostic (`KRYOS_EXIT_TRACE=1` in fault.rs) that never fires before the -1 death, proving no `process::exit`/normal `main` return is involved; the OS kills the process directly (memory-pressure dependent). Root cause found by MEASURING MEMORY, not tracing exceptions: `Get-Process` polling showed kryos-stage1.exe peaking at 13-16GB+ (and still climbing) to tokenize a 109KB file. Cause: `Lexer { src, pos, tokens }` was rebuilt via a fresh struct LITERAL on every `lex_advance`/`lex_match_char`/`lex_emit` call (i.e. per CHARACTER, ~110K times for parser.kry, not per token) and `emit_aggregate_struct` in kryos-codegen-llvm clones/dups ANY array-typed struct FIELD unconditionally at every literal construction (elem_kind=4 for a struct-of-Token element additionally RETAINS every element) -- so each of ~110K reconstructions retained up to ~20K already-emitted tokens: O(n^2), order 1e9 atomic retains, matching the CPU hot-path symbols found via `llvm-symbolizer` against a `-g` debug rebuild (`kryos_struct_retain`/`kryos_array_dup`/`kryos_array_new` dominated `KRYOS_WATCHDOG` RVA samples). The EARLIER "Lexer NOT @copy" fix (see struct comment history) assumed a non-@copy struct's array field is merely SHARED (refcount bump) on rebuild; it is not -- `emit_aggregate_struct`'s field-clone is unconditional, not @copy-gated, so that fix never actually delivered the intended O(n) it documented. FIX: pulled `tokens` out of `Lexer` entirely into a module-level `let mut LEX_TOKENS: [Token] = []`, mutated only via `push` (never reassigned after its own initializer -- see item #2b, a SEPARATE newly-found bug where cross-function global reassignment corrupts the array). Proof: peak working set 13-16GB+ -> 286MB (tokenize alone <100MB); dose-response gone -- 8/8 clean on parser.kry (109KB) AND 8/8 clean on lower.kry (128KB, the other historically-failing file) with the OLD binary confirmed still failing 3/6 on lower.kry in the same session (prove-both-ways); `test_bootstrap.sh` 16/16 across 7 consecutive runs (was the documented 14/16 baseline); full `kryos-loop.sh gates 2` GREEN. Item #5 in OPEN is the SAME bug class, unfixed, in `Parser` (lower severity: token-granularity not character-granularity, not yet lethal) |
| **`assert_eq`-named user/stdlib calls skipped the post-call unwind check, so a caller kept running after its callee threw** | Found writing `examples/showcase/secret_agent.kry`'s value assertions with `std::test::assert_eq` (3-arg: `actual`, `expected`, `msg`). Repro: `fn main() { println("before")  assert_eq(x, y, "diff")  println("after (should NOT print)") }` with `x="AAAA"`, `y="BBBB"` -- printed BOTH `before` AND `after`, THEN the correct `kryos: uncaught exception: assertion failed: diff -- ...` to stderr, exit 101. Bisected mechanically (one variable at a time, not guessed): reproduces for ANY function literally named `assert_eq` regardless of param names/return-type annotation/import-vs-local, and does NOT reproduce under any other name -- pinned it to the name itself. Root cause (read, not guessed): `kryos-mir/src/lower.rs::is_unwind_source` and the equivalent post-call "check the thread-local exception state" filters in BOTH codegen backends (`kryos-codegen-cranelift/src/codegen.rs`'s inline `should_check` match, `kryos-codegen-llvm/src/codegen.rs::post_call_exception_check_applies`) hardcode `"assert_eq"` in their "this call can never throw, skip the check" list -- correct ONLY for the compiler's real 2-arg `assert_eq(left, right)` INTRINSIC (which lowers to `kryos_builtin_assert_eq`, a `process::abort()`-based call that never returns, so genuinely needs no check), but the exclusion didn't gate on arg count, so it ALSO wrongly excluded a 3-arg call resolving to `std::test::assert_eq` (or any user function of that name) -- a REAL function that `throw`s and returns normally. Without the check, the caller's next MIR instruction ran before anything noticed the pending exception; it only surfaced at a LATER checked boundary. Also broke `try`/`catch` routing the same way (a failing 3-arg `assert_eq` inside a `try` was not caught at all -- confirmed before/after). FIX: in all 3 sites, exclude `"assert_eq"` from the "never throws" set ONLY when `args.len() == 2` (matching the intrinsic's own dispatch condition, already present nearby in both backends), so any other arity gets the check. Proof, both ways: pre-fix the minimal repro printed `after` and pre-fix `try`/`catch` did not catch (both shown above); post-fix (`cargo build --release`, both backends) the repro prints only `before` then the exception (JIT AND AOT), the `try`/`catch` variant correctly catches and prints "caught: ...", and a genuinely-passing `assert_eq(4, 2+2, ..)` and the TRUE 2-arg intrinsic (`assert_eq(1, 2)`, no import) both still behave exactly as before (matching-value case doesn't throw; the true intrinsic still aborts immediately, only `before` prints). Gates: conformance 47/47, tier1+tier2 GREEN, bootstrap 16/16 |
| **`comptime { }` docs sold compile-time isolation/determinism in present tense; it runs at RUNTIME** | `docs/11-comptime.md` rewritten top to bottom: verified live (outer-var read, `println` fires at runtime once PER CALL with no caching, `file_read` works under ordinary capability rules -- all four directly contradicted the old doc) and reframed everything aspirational as explicitly PLANNED, not current. Also fixed the same overselling in `docs/WHY_KRYOS.md`, `README.md`, `docs/appendix/keywords.md` (which also wrongly sold `quantum`/`Qubit`/`Qureg`/`Secret` as working -- verified NOT implemented: `quantum {}` is a runtime passthrough with no quantum semantics, `Qubit`/`Qureg`/`Secret` are not even registered types, E0101). No code change -- docs only |
| **`[dyn Handler]` (as a `let`-annotated array literal) reported confusing `E0100` alongside the real `E0110`** | Array-literal element-unification (`Expr::ArrayLiteral` in `kryos-types/src/check.rs`) ignored the annotated `dyn` element type and force-unified `[A{}, B{}]`'s elements against each other, so a `dyn Trait` array (which by definition holds different concrete types) got a second "expected A, found B" diagnostic implying the fix is same-typing the elements, when the real fix is an enum. FIX: `Stmt::Let` now checks the RAW (pre-resolution) `TypeExpr` for the `[dyn Trait]` shape specifically (not "did this resolve to Type::Error", which is too broad -- see item #4 in OPEN for why that broader version was tried and reverted) and records the array literal's span in a new `suppress_array_elem_unify: HashSet<Span>`; `Expr::ArrayLiteral` consults it and skips ONLY the pairwise cross-unify (each element is still independently type-checked). Proof, both ways: pre-fix `let h: [dyn Handler] = [A{}, B{}]` emitted `E0110` + `E0100` + `E0107` (3 errors); post-fix, only `E0110` + `E0107` (2 errors) -- confirmed via a stash/rebuild A-B comparison of the exact binary. Regression-tested (`want_reject_e0110_clean`, `tests/type_soundness.sh`), which ALSO proves the fix does not regress the unrelated case (`let x: NotAType = [1, "two"]` still correctly keeps BOTH its unknown-type error and its own element-mismatch E0100). Gates: conformance 47/47, tier1+tier2 GREEN, bootstrap 16/16. Known remaining gap: the same call-ARGUMENT shape (not `let`) -- see OPEN item #4 |
| **docs/BUGS.md drifted twice: once claiming "none currently tracked" while 2 tests deadlocked, later claiming those same 2 tests were still open for weeks after the fix shipped** | File also had an exact accidental DUPLICATE of its own header + first two sections pasted back-to-back, with the duplicate's "Active" section describing `conf_spinlock_mutex`/`conf_errors_concurrency` as still-open blockers -- both now verified PASS cleanly (`kryos build --release` + run, exit 0, no hang) and were already closed in this same file's own (non-duplicate) first "Active" section and in this ledger's CLOSED table. Rewrote `docs/BUGS.md`: removed the duplication, moved both to Resolved with the real fix, corrected the stale conformance count (was 40/40 hardcoded, live count is 47/47 and growing). Added `tests/docs_status_gate.sh`, wired into `kryos-loop.sh gates` tier1: (1) scans `docs/BUGS.md`'s `## Active` section for `tests/conformance/conf_*.kry` paths and fails if any named-as-open test now passes cleanly, (2) checks `conformance N/N`-style prose claims in README.md/docs/BUGS.md/STABILITY.md against the live `tests/conformance/*.kry` file count. Proof both ways: gate FAILS when a synthetic stale "Active" entry naming a passing test is appended (verified), and FAILED for real against the pre-fix README's stale "40/40" (verified, then fixed); PASSES clean on the corrected files. Does not catch every drift shape (prose claims with no associated test file) -- a mechanical narrowing, not full auto-generation, documented as such in `docs/BUGS.md` itself |
| **Broader docs audit (same session): several other docs pages oversold unimplemented/removed features in present tense** | `docs/07-error-handling.md`'s "self-healing runtime" section (165 lines) described `@constraint`/`@fallback`/`--heal-report`/auto div-by-zero-and-index-clamp recovery in confident present tense below a single "not yet implemented" banner readers would skim past -- verified `@constraint(">= 0", "<= 100")` is a complete no-op (`clamp_percent(150.0)` returns `150`, not the doc's claimed `100`) and `--heal-report` is not a recognized CLI flag at all; rewrote the whole section in consistent future/planned tense with inline TODAY-vs-PLANNED contrasts. `docs/13-ffi.md` claimed arbitrary C-library FFI "is fully implemented" and showed `puts`/`getpid`/`getenv`/`-lsodium` linking as working examples -- verified `puts("hello from Kryos")` builds and exits 0 but prints NOTHING (silently wrong, worse than a link failure), `getpid`/`strlen` fail to link ("use of undefined value"), `[build] link` in `kryos.toml` has no effect, and the `sin`/`cos`/`pow` example that DOES work only works because those names collide with Kryos builtins (not because real FFI linking works) -- also found `kryos bindgen` DOES work despite the doc claiming the opposite. `docs/19-language-reference.md` §7.4 claimed field/index mutation through an immutable binding is rejected -- verified false (`let p = Point{..}; p.x = 9` compiles and runs; CLAUDE.md already documented this as a known-wrong line that was never fixed in the doc itself). `docs/10-capabilities.md` and `docs/capability-roadmap.md` claimed capability enforcement is "sound across every path" / "every function auditable in isolation" with no caveat -- added a prominent "Known limitation" section documenting the closure/fn-value capability escape (OPEN item #1 in this ledger) with its repro, since this is the exact security-adjacent gap the target use case (a secret-managing agent) needs disclosed, not silently omitted. Same caveat threaded into `README.md`'s capability bullet and `STABILITY.md`'s known-limitations section. All verified live against this commit's binary, not inferred from reading source. No code change for any of these -- docs only |

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

## DIFFERENTIAL FUZZ HARNESS (2026-07-31)

Built `tests/fuzz/` (`gen_fuzz.py` + `run_diff.py` + `shrink.py` +
`fuzz_gate.sh`, wired into `.github/workflows/ci.yml`): a category-templated
generator (13 categories -- int/float arithmetic+casts, string ops,
arrays, maps, scalar structs, heap-field structs, enums+match, direct
closures, `std::iter` HOF closures, generics, control flow, try/throw) that
emits deterministic `(seed, blocks)`-replayable programs, each block
printing a tagged line + folding into a checksum, diffed between `kryos run`
(Cranelift) and `kryos build --release` (LLVM). A repo-wide `tools/diff-fuzz/`
(`gen2.py`/`memsafety_fuzz.py`, already CI-wired) predates this and covers
more of the general expression grammar; this harness's distinct value is
**generics coverage (gen2.py has none) and an automatic ddmin shrinker
(gen2.py has none)** -- both new, not a duplicate of the existing tool.

**Result: 1000 generated cases (seeds 1-1000, default block counts
12-30), 0 divergences, 0.0% divergence rate.** Runtime ~1.1s/case
(build+link dominates). Shrinker self-validated against a known, still-open
divergence (`parse_float("-0.0")`'s sign, CLAUDE.md gotcha #18): given a
10-line program with that divergence buried in noise, reduced to the exact
4-line minimal repro.

**One real bug found and fixed while building the harness itself** (not by
the sweep -- by using the harness's own generic struct template, named
`Box_`): a generic struct/enum base name ending in `_` broke ALL its bare-
passthrough instance methods with `unresolved external symbol <method>` on
BOTH backends identically (shared-MIR bug, not a JIT/AOT divergence -- see
the CLOSED table entry and `tests/conformance/conf_generic_underscore_name.kry`).
Found and root-caused via the exact discipline this wave requires (minimal
repro, `--emit-llvm`, read the IR, don't guess) even though it fell outside
the harness's own stdout-diff detection (both backends fail identically, so
a stdout/exit-code differ never fires -- worth noting as a harness
limitation: it cannot see "both backends agree by being equally broken").

**Known harness limitations, stated honestly:**
- Blocks are independent (each is its own `fn` with no shared mutable
  state) for reliable shrinking and easy per-block localization -- this
  means the harness cannot find bugs that need CROSS-CATEGORY interaction
  within one call chain (e.g. a generic struct holding a closure holding an
  array holding a struct). `tools/diff-fuzz/gen2.py`'s single-program-tree
  approach is more likely to hit that shape; this harness is not a
  replacement for it.
- No concurrency/`spawn` coverage (deliberate -- avoids introducing
  non-determinism into a harness whose whole value is exact replay).
- A 0.0% divergence rate over 1000 cases is a genuinely positive signal for
  the categories covered, not proof of absence -- it means this generator's
  specific grammar didn't hit a new divergence in this sample, not that the
  categories are divergence-free in general (the still-open `parse_float
  ("-0.0")` and NaN-sign-bit divergences are proof the surface isn't fully
  clean; this generator was deliberately built to avoid re-hitting those
  CATALOGED cases rather than re-confirm them).

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
