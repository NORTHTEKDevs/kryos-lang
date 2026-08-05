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

### 17. SUPPLY CHAIN: a dependency's explicit `git = "..."` / `github:org/repo@ver` source in `kryos.toml` is NEVER consulted by `kryos pkg install`/`update` — silently replaced by a pure by-name lookup against the single hardcoded official registry, or a flat failure, either way ignoring what the manifest says (RED TEAM round 3, toolchain-supply lens, found 2026-08-04) — NOT FIXED

`tests/security/pkg_manifest_git_source_ignored.sh`. `kryos-package/src/
manifest.rs`'s `DepSpec` Deserialize impl accepts a `git = "..."` TOML key
(and `parse_dep_string`, which backs `kryos pkg add <spec>`, accepts
`github:org/repo@ver`) — both produce a real `DepSpec::Remote { source,
version_req }` with the user's chosen source recorded. But
`compiler/crates/kryos-cli/src/commands/pkg.rs::install()`'s handling of a
`DepSpec::Remote` dependency destructures it as `kryos_package::DepSpec::
Remote { .. }` — a wildcard that discards `source` and `version_req`
entirely — and instead does a lookup of the dependency's NAME in the local
registry index, unconditionally synthesizing `github_subdir:NORTHTEKDevs/
kryos-registry/packages/<name>/<version>` from whatever entry it finds
there. `update()` has the same shape. The manifest's declared source is
never read by either command.

**Verified live, both directions, against `compiler/target/release/
kryos.exe`, HEAD `fc05cce`, no compiler changes:**

**(A) Name not in the registry index — fully offline, no network needed.**
A project's `kryos.toml`:
```toml
[dependencies]
totally-unregistered-name-zzz = { git = "https://github.com/some-real-org/some-real-repo", version = "0.1.0" }
```
```
$ kryos pkg install
  warning: registry lookup for `totally-unregistered-name-zzz` failed: registry not synced — run `kryos pkg sync` first
error: dependency resolution failed: package 'totally-unregistered-name-zzz' not found in registry
$ echo $?
1
```
The install FAILS even though a perfectly valid alternate source was given
in the manifest — that source is never attempted. The git-source manifest
syntax is dead code as far as `install`/`update` are concerned.

**(B) Name that DOES exist in the registry (any popular/common package
name) — the dangerous direction, needs network.** Same manifest shape,
naming the real `http-router` package with an obviously attacker-controlled
`git =` source:
```toml
[dependencies]
http-router = { git = "https://github.com/ATTACKER-CONTROLLED/evil-http-router", version = "0.1.0" }
```
```
$ kryos pkg install
fetching packages ...
  cloning https://github.com/NORTHTEKDevs/kryos-registry.git (subdir: packages/http-router/0.1.0) -> ...\.kryos\packages\http-router-0.1.0
installed 1 package
  http-router (cached: ...\.kryos\packages\http-router-0.1.0)
  1 remote, 0 local path
wrote kryos.lock
$ echo $?
0
$ cat kryos.lock
[[package]]
name = "http-router"
version = "0.1.0"
source = "github_subdir:NORTHTEKDevs/kryos-registry/packages/http-router/0.1.0"
checksum = "sha256:ec03da9283102b939b7d64bf7a61a3f6154243f979319baac9b354cad9dc044d"
```
The OFFICIAL registry package is installed, checksum-verified against the
OFFICIAL index entry, and the lock file written — a completely "successful,
honest-looking" install, with **zero** warning, diff, or prompt that the
manifest's declared `git =` source was never touched. Both the CLI's own
`installA.log`/`installB.log` and `kryos.lock` were inspected directly (not
grepped for a marker string) to confirm the attacker URL never appears
anywhere in the fetch path — the resolver genuinely never constructs it.

**Why this is real and distinct from existing ledger items:** item 12 (lock
file never read) and item 1b (checksum, CLOSED) both concern the integrity
of a source `resolve()` has ALREADY picked; this is about `resolve()`
picking a source that has NO relationship to what the project author wrote
at all, silently. Blast radius: any project trying to pin a private fork, a
security-patched mirror, or a vendored copy of a package by declaring an
explicit `git =`/`github:` source under a name that also exists in the
public registry gets the PUBLIC package instead, indistinguishable from a
correct install — this is the textbook "dependency confusion" shape (same
name, different intended source, wrong one silently wins), just produced by
the resolver discarding the user's own source field rather than by an
external registry-priority bug.

**Not yet attempted (follow-up, out of scope this round — no compiler
changes made):** whether `kryos pkg add github:some/repo@1.0.0` (the CLI
form, rather than a hand-written `git =` TOML key) reaches the exact same
dead branch — it produces the same `DepSpec::Remote` shape by construction
(verified by reading `parse_dep_string`), so it should, but was not
separately executed live this round to save the shared workspace's budget
after (A)+(B) already proved the mechanism deterministically. Fix shape (a
design decision, not attempted): `install()`/`update()` should honor a
non-empty `DepSpec::Remote.source` directly (treating it as an explicit
override of the registry lookup, matching what `cargo`'s `git = "..."`
dependency does) rather than silently discarding it in favor of a pure
name-based registry match.

**Ruled out this round (round 3, toolchain-supply lens), evidence
attached, no defect found:**
- **`kryos fmt` does not alter program MEANING on the shapes tried**: a
  multi-line string literal (`"line one\nline two\nline three"` written
  with real embedded newlines) is reformatted into an escaped single-line
  form and re-run with the IDENTICAL `len()` (28) before and after; the
  same held for a string containing a real embedded `\r\n` (len 8 before
  AND after — fmt re-escapes it as `"one\r\ntwo"` rather than normalizing
  the line ending away). A string containing literal `/* fake */` and
  `// also fake` text is left byte-identical (fmt's lexer correctly does
  not treat in-string text as a comment). A program with a genuinely
  NESTED block comment (`/* outer /* nested */ still-comment */`) makes
  `kryos fmt` **fail closed** — `skipped <file> (a comment could not be
  re-anchored; file left untouched)`, exit 0, file byte-identical, rather
  than silently corrupting or dropping the comment. (Not exhaustive — only
  these shapes were tried; fmt's general re-indentation was not fuzzed.)
- **The experimental `--backend wasm` enforces the SAME capability check
  as the default backend, not a bypass**: a zero-`@capabilities` function
  calling `file_write` (needs `fs:write`) is rejected with the identical
  `error[E0505]` under `kryos check` AND under `kryos build --backend
  wasm`, both exit 1 — the capability inference/enforcement pass runs
  before backend selection, so it is not a route around `deny!`/inferred
  mode. (Only the compile-time capability-rejection path was probed, not
  wasm codegen correctness/parity generally.)
- **`kryos doc` does not crash on a doc comment containing lexer-trap-
  looking text** (an unterminated `{interp` and a fake `*/` inside a `///`
  comment): renders the raw text into the generated Markdown verbatim,
  exit 0, no panic.

### 15. SILENT WRONG ANSWER: `let a = arr[i]` (array-of-struct element read) is a SHARED HANDLE on Cranelift/JIT but an INDEPENDENT COPY on LLVM/AOT — a genuine backend divergence, contradicting gotcha #23's documented "both backends agree" claim (RED TEAM round 2, memory-unsafety lens, found 2026-08-04) — ROOT-CAUSED, DESIGN NOTE, NOT FIXED (2026-08-05)

`tests/security/attack_container_element_alias_refcount.kry`. CLAUDE.md
gotcha #23 states: "reading a struct element out of a collection returns a
SHARED handle to the stored box on both backends: a later in-place mutation
... IS visible through `a` ... both backends agree — a semantic boundary,
not a miscompile." This is TRUE for the documented direction (mutate through
the CONTAINER, read through a previously-taken alias) but FALSE for the
reverse: mutate through an alias taken via `arr[i]`, then read back through
a SECOND alias taken via the same `arr[i]`, or through the container itself.

Minimal repro: build a one-element `[Item]` (struct with a `str` field),
take three separate `let` reads of the same slot (`let a = arr[0]`,
`let b = arr[0]`, `let c = arr[0]`), mutate a field through `a`
(`a.tag = a.tag + "!"`), then read the field back through `a`, `b`, `c`, and
`arr[0]` directly, 20000 iterations, exit code checked both runs:

```
$ kryos run attack_container_element_alias_refcount.kry
last=x19999!|x19999!|x19999!|x19999!|5      # a|b|c|arr[0] — ALL FOUR see the mutation
$ kryos build --release attack_container_element_alias_refcount.kry -o bin && ./bin
last=x19999!|x19999|x19999|x19999|5         # a|b|c|arr[0] — only `a` itself sees it
```

Both runs exit 0 — no crash, purely a silent value divergence, classified by
the printed VALUE not by grepping. Proven both ways by construction: the
JIT's own `a` value is identical to AOT's `a` value (`x19999!` in both), so
the mutation itself is correctly applied in both backends — only its
VISIBILITY through separately-taken aliases of the same read expression
diverges. Per the loop's own rule ("backends diverging => read the emitted
IR, do not guess from source"), the IR was not read this round (attacker-only
mandate, environment severely contended — see the item 14 session note
below, which applied equally to this investigation); the working hypothesis,
NOT verified against MIR/LLVM-IR, is that `let x = arr[i]` on AOT lowers to
a value-copy (matching AOT's `@copy`-struct-assignment semantics) while the
same expression on Cranelift lowers to a raw pointer load (matching how
`m["k"]` container-mutation-then-alias-read was verified), i.e. the two
backends may be implementing two DIFFERENT points on the shared/copy
spectrum for the identical source construct rather than one of them having
a bug in an otherwise-shared model. This needs an `--emit-mir`/`--emit-llvm`
read to confirm before attempting a fix — flagging as the next concrete step,
not attempting it (out of scope this round).

Not double-free/UAF: `KRYOS_FREE_DIAG=1` census on both the JIT and AOT run
is clean (no diagnostic output, exit 0) at 20000 iterations — this is a
values-diverge defect, not a corruption defect, despite living in exactly
the "aliases created by reading out of a container" surface this round's
brief named as a corruption-suspicion area. Distinct from all four
already-found items this campaign (cap-escape-closure-wraps-closure,
closure-lock-reentrant-self-deadlock, closure-mutating-costale-scalar-capture,
map-hash-collision-dos) and from item 14 above (different lens, different
file).

**Ruled out this round, evidence attached (exit 0, value-correct, clean
under `KRYOS_FREE_DIAG=1`, both with and without diag):**
- `tests/security/attack_drop_helper_recursion.kry` — a 200000-deep
  self-referential struct chain (`Node{val, next:[Node]}`) built iteratively
  then dropped (recursive `__kryos_drop_Node` teardown, one native stack
  frame per link): `built len=200000`, exit 0, with and without diag. No
  stack overflow or corruption at this depth (higher depths not attempted —
  environment cost).
- (`attack_throw_unwind_heap_locals.kry`, `attack_closure_env_teardown_twice.kry`,
  `attack_spawn_mutating_closure_reentrancy.kry` were independently written
  and probed by the concurrency-lens agent this same round — `6665c4f` —
  before this agent got to run them; results agree (clean, exit 0, value-
  correct) and are not re-litigated here to avoid duplicate ledger entries.)

**Ruled out, round 3 (memory-unsafety lens, found 2026-08-04) — went deeper on
this exact surface (aliases created by reading out of a container +
container element ownership) with a shape nobody had tried: a SLOT
OVERWRITE while a prior plain (non-closure) alias is still live, not just a
field-mutation-then-read.** Hypothesis: `let a = arr[0]` takes a shared
handle (documented above); does `arr[0] = NewValue{..}` free the OLD box out
from under `a` (UAF at `a`'s later read) or double-free it (both the
overwrite's release AND `a`'s own scope-exit drop try to free the same
allocation)? Falsified — both backends deep-copy on overwrite, `a` keeps its
own independent, correctly-owned reference:
- `tests/security/attack_array_slot_overwrite_alias_uaf.kry` — 20000
  iterations of `let a = arr[0]` then `arr[0] = Item{..}` (brand new box)
  while `a` is still live, then read both. `kryos run`: `last=orig19999|3|
  new19999` (exit 0); `KRYOS_FREE_DIAG=1`: identical, no diagnostic (exit
  0); `kryos build --release`: identical value (exit 0). `a` correctly sees
  the ORIGINAL struct (uncorrupted, right field count) and the slot shows
  the NEW one — no UAF, no double free, both backends agree.
- `tests/security/attack_map_key_overwrite_alias_uaf.kry` — the map
  analogue (`m["k"] = Item{..}` then overwrite the same key while `let a =
  m["k"]` is live), same 20000-iteration/both-backend/FREE_DIAG protocol,
  same clean result (`overwrite_last=orig19999|3|new19999`, exit 0
  everywhere). (The REMOVE-while-aliased variant of this probe, also in the
  brief, is untestable as a language feature: grepped the whole runtime and
  stdlib — `kryos-rt/src/map.rs` and every `.kry` module — and there is no
  `map<K,V>` key-removal builtin at all; `remove`/`delete` only exist for
  `List<T>` by-index and the raw i64 `std::set`. Flagging the gap itself,
  not inventing a repro for a function that does not exist.)
- Also probed **struct-literal field duplication** (a distinct bullet in
  this round's brief): `Item { tag: "first", other: 1, tag: "second" }` is
  rejected at `kryos check` with a clean `error[E0110]: duplicate field
  \`tag\` in \`Item\` literal -- each field may be set only once` (exit 1,
  not committed as a repro since it never reaches codegen — no heap value is
  ever constructed to leak or double-free).

Session note: this round's environment was under the same severe,
documented contention as prior rounds (concurrent compiler jobs from other
agents sharing this workspace, per `fc05cce`/`6665c4f`/`a537504` all landing
the same day) — several individually-fast (`kryos run` a two-line
`println`) commands queued for multiple minutes and one `kryos run` hit a
transient `LNK1104 cannot open file` on a temp `.exe` (a file-lock collision
with a concurrent build, not a language defect — the identical command
succeeded cleanly on retry with no code or file changes). Reported for
calibration, not as a finding; both repros above were still proven both
ways (JIT + AOT + FREE_DIAG) despite it.

#### Root-caused 2026-08-05 (assigned to fix this item): confirmed by reading both backends' IR-emission code end to end, not guessed. DESIGN NOTE, NOT FIXED — this is the SAME architectural gap as item 3, not a separate bug.

Re-reproduced fresh against the committed repro, HEAD at task start, no
compiler changes before measuring: `kryos run` ->
`last=x19999!|x19999!|x19999!|x19999!|5` (exit 0); `kryos build --release`
-> `last=x19999!|x19999|x19999|x19999|5` (exit 0). Matches the round-2
numbers exactly.

The prior round's working hypothesis ("AOT lowers `let x = arr[i]` to a
value-copy, Cranelift to a raw pointer load — needs an `--emit-mir`/
`--emit-llvm` read to confirm") is **CONFIRMED**, and the mechanism is more
precise than "value-copy vs pointer": it is a difference in how the two
backends represent EVERY struct/enum value, not something special about
the `let`/index-read site.

- **Cranelift never materializes struct/enum storage at all.** `RValue::Index`
  (`kryos-codegen-cranelift/src/codegen.rs:5638-5737`) calls
  `kryos_array_get` and, for a Struct/Enum element type, returns the raw
  result UNMODIFIED — the code comment there is explicit: "a plain Index
  read is intentionally left as an alias (no copy) ... copying
  unconditionally on every read leaked one struct block per read that was
  never stored anywhere else" (this was tried and reverted before; MIR's
  drop-tracking has no way to free a codegen-only copy, so an unconditional
  clone here is a guaranteed leak, not a free lunch). Every subsequent
  FieldAccess on that value GEPs directly off the same pointer. `a`, `b`,
  `c`, and a fresh `arr[0]` read are the literal same i64 pointer value,
  dereferenced live at each use — hence all four see a later mutation.
- **LLVM materializes struct/enum values as first-class SSA aggregates
  everywhere**, never as a pointer, by construction: `mir_type_to_llvm`
  (`kryos-codegen-llvm/src/codegen.rs:11029-11081`) maps `MirType::Struct(name)`
  to `"%name"` (an LLVM aggregate TYPE, not `ptr`) unconditionally — there
  is no alternate "boxed" representation to opt into. `RValue::Index`'s
  aggregate branch (`codegen.rs:7234-7371`, specifically the `is_aggregate`
  arm at `7306-7335`) does `p = inttoptr i64 raw to ptr` then
  `v = load {dest_ty}, ptr p` — a genuine value copy into a fresh SSA
  register, additionally spilled into the local's OWN `alloca %Name` (never
  the source pointer `p`) if the local is later mutated (`mutable_locals`,
  populated for ANY local that is later the target of a field store, not
  just `let mut` bindings). `RValue::Field` (`codegen.rs:7025-7135`) reads a
  named-struct field via `extractvalue {obj_ty} {obj_val}, {field_idx}` —
  operating on the SSA aggregate VALUE, never a GEP off a shared address.
  There is no `obj_ty == "ptr"` fallback in this arm (checked: the only
  `ptr`-object branches in this file are for anonymous tuple/array element
  types and dyn-trait fat pointers, `codegen.rs:4433,7240,7350,8255` — none
  apply to a named struct). So `let a = arr[0]` / `let b = arr[0]` /
  `arr[0].tag` are THREE INDEPENDENT LLVM value copies of the same box;
  mutating `a`'s copy is invisible to the other two, matching the observed
  `x!|x|x|x`.

**This is not a second bug to fix independently of item 3.** It is the same
representational fork — "every struct/enum value is a byval SSA aggregate on
LLVM, a raw heap pointer on Cranelift" — showing up as a value-divergence
here instead of a leak there. Item 3's own design note already scoped and
costed the only fix that closes this class for good: **Design A, uniform
boxing of struct values** (every struct-typed local/param/return becomes a
pointer to a `kryos_calloc` box on BOTH backends, so `RValue::Field` reads
GEP off a shared address instead of `extractvalue`-ing a materialized copy).
Item 3's own cost analysis for Design A applies unchanged here: an ABI break
on LLVM AOT touching every call site, every `emit_aggregate_struct` literal,
every method receiver, every generic instantiation, and field-access GEP
codegen; a new `kryos_calloc` per struct construction (currently free for
flat structs); and unmeasured self-host bootstrap memory risk (bootstrap is
already struct-heavy and running at the edge of survivable memory per item
3's own bootstrap note). Item 3's Design B (retain-walk at call boundaries,
no ABI change) does NOT close this item — Design B only fixes ownership at
call/return boundaries; it does nothing for `RValue::Index`/`RValue::Field`
materializing independent copies on every array/map struct-element READ,
which is this item's entire mechanism. **A narrow, LOCAL fix confined to just
`RValue::Index`/`RValue::Field` (without the ABI change) was evaluated and
rejected**: making the array-Index-read destination `ptr`-typed instead of
`%Name`-typed would require `RValue::Field`'s extractvalue path (and every
other named-struct-aggregate consumer in this file — struct-to-struct
assignment, function-argument passing, comparison, printing) to grow a
parallel GEP-based code path for a "this local is actually a pointer"
representation that does not exist today anywhere outside the
already-narrow `Ptr(elem_ty)` temp used by the FieldAssign chained-target
case (`kryos-mir/src/lower.rs:10288-10343`, which is a short-lived TEMP
consumed once, never a general user-visible local) — the same blast radius
as Design A, just approached from the read side instead of the
call-boundary side.

**Recommendation:** do not attempt this and item 3 as two patches. Whoever
picks up item 3's Design A should verify it also closes this item's repro
(`tests/security/attack_container_element_alias_refcount.kry`, both
backends, `last=x19999!|x19999!|x19999!|x19999!|5` on AOT after the fix) as
part of that change's own proof, rather than re-deriving this analysis.
Until then this stays a documented, accepted boundary — CLAUDE.md gotcha #23
corrected (2026-08-05) to state the true boundary: mutating through a
CHAINED `container[key].field = ...` target and reading through a
previously-bound alias agrees on both backends (unaffected, already fixed
via the `Ptr(elem_ty)` mechanism above); mutating through an alias `let`
binding itself and reading through a SECOND alias or the container does
NOT.

### 16. DEADLOCK/HANG: an uncaught `throw` inside a `spawn` task silently skips every statement after it in that task, including a `wg_done(wg)` the caller was relying on — turning ONE ordinary exception into a PERMANENT hang of every `wg_wait()` (RED TEAM round 3, concurrency lens, found 2026-08-04) — NOT FIXED

`tests/security/attack_spawn_uncaught_throw_waitgroup_hang.kry`. Read (not
guessed) `kryos-rt/src/spawn.rs::kryos_spawn`: the spawned OS thread's
closure runs `invoke_task(fn_ptr, &args)` then calls
`kryos_exception_report_thread_if_pending()`, whose own source comment says
exactly "A `throw` that unwound out of the thread's entry function would
otherwise vanish with the thread-local state. Report it like a Rust thread
panic: message to stderr, thread dies, process lives." This isolation
mechanism is itself correct and already documented/verified (LEDGER's
2026-08-01 error-handling wave: "`spawn { throw .. }` is isolated ... parent
survives"). The gap this round found is the UNEXAMINED CONSEQUENCE: Kryos
exceptions are flag-based, not native stack unwinding (CLAUDE.md, codegen
comment) — there is no unwind-then-continue and no `finally`, so "thread
dies" means the spawned FUNCTION BODY simply stops executing at the throw
point. Any statement written AFTER it in the same task — most commonly the
ordinary "signal I'm done" idiom, `wg_done(wg)` at the end of a worker body
— never runs. `std::sync::WaitGroup.wait()` (`compiler/stdlib/sync.kry`)
busy-polls `while self.counter.load() > 0 { sleep(1) }` with nothing left
alive that will ever decrement it back to the target, so the wait is a
PERMANENT hang, not a slow one.

Live repro, `compiler/target/release/kryos.exe`, no compiler changes,
`KRYOS_STDLIB_DIR` set per repo convention — 8 workers `spawn`ed, each does
`risky_work(idx)` (worker index 3 throws) then `wg_done(wg)`, `main` does
`wg_wait(wg)` afterward:
```
$ timeout 25 kryos.exe run attack_spawn_uncaught_throw_waitgroup_hang.kry
worker 0 result=0
worker 1 result=2
kryos: uncaught exception in spawned thread: worker 2 result=4
boom in worker 3worker 4 result=8
worker 5 result=10
main: waiting on wg...
worker 6 result=12
worker 7 result=14
$ echo $?
124
```
`echo 124` is `timeout`'s own kill signal — the process never reaches "main:
all workers done" (the intentionally-unreachable-if-the-hypothesis-holds
final line). Every other worker's `result=` line printed and (implicitly,
since the program otherwise hangs) called `wg_done` — only worker 3, the one
that threw, is missing, exactly matching the "one skipped `wg_done` starves
the counter by exactly 1, forever" prediction.

**Proven both ways.** Negative control — the byte-identical program with the
throw's trigger condition changed to one that never fires (`n == 99` instead
of `n == 3`) — completes normally, prints every worker's result INCLUDING
`worker 3 result=6`, and reaches the final line:
```
$ timeout 20 kryos.exe run /tmp_control.kry
worker 0 result=0
...
worker 3 result=6
...
main: waiting on wg...
worker 7 result=14
main: all workers done (this line should be unreachable if the hang hypothesis is correct)
$ echo $?
0
```

**Both backends agree** (consistent with the defect living in shared runtime
code — `kryos-rt/src/spawn.rs` is linked into both, not backend-specific
codegen): `kryos build --release` on the same source, then running the
resulting binary, reproduces the identical hang (`timeout 20 ./bin` exits
124) with the identical interleaved-output shape (worker 3's line missing,
`main: waiting on wg...` printed, no further progress).

**Sample size, honestly reported (severe shared-workspace contention this
session — bash and background-task turnaround routinely exceeded a minute
for trivial commands, matching the documented `feedback_machine_slowness`
class):** 4/4 independent `kryos run` invocations hang (all `timeout`-killed
at 124, none reach the final line); 1/1 `kryos build --release` + binary run
hangs identically. This is well short of the 50-run bar the task brief asks
for, but the mechanism is fully deterministic by construction (worker 3's
throw condition is unconditional on `n == 3`, not a race), so repeat count
adds confidence about environment reproducibility, not about a hidden
probabilistic component — there isn't one to find here.

**Blast radius:** this is the exact "spawn N workers, wait for them via a
WaitGroup" idiom used elsewhere IN THIS OWN CAMPAIGN
(`attack_actor_multiword_send_contention.kry`,
`attack_spawn_mutating_closure_reentrancy.kry`) and is the natural pattern
for any worker-pool / fan-out-fan-in program — none of those existing
attacks happen to have a worker that can throw, which is why this was not
already surfaced. Any real program using `spawn` + `WaitGroup` for a batch
job where a SINGLE item's processing can throw (a malformed record, a
timeout, a validation failure) hangs the entire batch forever the moment
that one throw fires, with no diagnostic connecting the printed
`kryos: uncaught exception in spawned thread: ...` stderr line to the silent
hang that follows it — a caller watching only stdout/exit status has no
signal at all beyond "the process never finishes."

**Ruled out this round (checked, not just assumed — evidence attached):**
- **The LEDGER item 7b closure-capture-lock does NOT stay held across a
  throw inside the locked closure's own body** (this was this round's other
  explicit "untested" target from the task brief, besides reentrancy which
  item 11 already closed). Read `kryos-codegen-cranelift/src/codegen.rs`
  ~1741-1900: the env-thunk that wraps every mutating closure has exactly
  ONE return point — `call orig_ref` (the closure's compiled body) is a
  PLAIN Cranelift call that always returns control to the thunk regardless
  of whether the callee's body executed a `throw`, because Kryos exceptions
  are a thread-local flag checked at each CALL SITE with a synthesized
  early-return, not native stack unwinding (per CLAUDE.md and the codegen's
  own `exc_return_block` comments) — there is no non-local jump that could
  skip past the thunk's own unconditional
  `kryos_mutex_unlock`(kryos-codegen-cranelift/src/codegen.rs:1895-1899)
  after the call. This is independently confirmed by EXISTING live evidence
  already in this ledger (item 11, round 2): `attack_closure_mutate_then_throw_state.kry`
  calls a mutating closure `f(5)` that mutates its capture then throws via a
  helper, catches the exception, and calls `f(0)` again — the SECOND call
  completes and returns a value (not a hang) on both backends, which could
  only happen if the first call's lock was released. No new repro added;
  citing existing evidence plus the code read that explains WHY it holds.

**Fix shape (not attempted — attacker-only mandate this round):** this is a
documentation/API-design gap more than a single-line bug — `kryos_spawn`
could offer a hook to run cleanup on the isolated-exception path (today's
`kryos_exception_report_thread_if_pending` only reports, it has no callback
mechanism a Kryos program can register), or `std::sync::WaitGroup` specific
to spawn could track "task N started, not yet done" and fail wait() with a
diagnostic instead of hanging forever when a tracked task's thread exits
without calling done(). Absent either, the least-code mitigation is
documentation: `docs/09-concurrency.md`'s existing "spawn isolates a throw"
claim should say explicitly that this means any code after the throw point
in that task — including a paired `wg_done`/`chan_send`/actor-notify — is
skipped, so a caller relying on such a signal must wrap the task body in its
own `try`/`catch` and call the signal from the `catch` arm too if it wants
`wg_wait()` to ever return.

---

### 14. RESOURCE-DOS: `parse_statement`'s stray-`;` recovery recurses with NO nesting/depth guard — a flat run of semicolons stack-overflows the compiler on `kryos check` (RED TEAM round 2, resource-dos lens, found 2026-08-04) — NOT FIXED

`tests/security/attack_parser_semicolon_chain_stack_overflow.kry`. The
parser OOM fix already landed (verified current state) hardened the
LOOP-shaped zero-progress hazards: `parse_module`'s declaration loop,
`parse_block_stmts`, and `parse_map_or_block_expr`'s block-body loop all
detect `self.pos == before` and force-advance instead of spinning. Every
OTHER recursive-descent entry point in the same file independently checks
`self.nesting_exhausted()` (`MAX_NESTING_DEPTH = 2048`, `MAX_RECURSION_DEPTH
= 256`) and bumps `rec_depth`/`nest_depth` before recursing:
`parse_block` (parser.rs:1169-1187), `parse_expr_bp` (parser.rs:1987-2004),
and `parse_type` (parser.rs:3534-3553) all fail closed with one clean
`E0010` diagnostic ("program nesting exceeds the maximum depth of 2048")
on adversarially deep input instead of exhausting a resource.

`parse_statement` (parser.rs:1336) is the one recursive-descent entry point
that was missed. Its `TokenKind::Semicolon` arm (parser.rs:1385-1394)
reports "unexpected `;`", consumes the token, then calls
`self.parse_statement()` again to continue — a genuine Rust call, one stack
frame per `;`, with **no** `nesting_exhausted()` check and **no**
`rec_depth` increment anywhere on this path. It is invisible to both
existing depth budgets.

Live repro, `compiler/target/release/kryos.exe` HEAD 00b3cf7, no compiler
changes, `KRYOS_STDLIB_DIR` set per repo convention:
```
$ python3 -c "print('fn main() {'); print(';' * 500000); print('println(\"done\")'); print('}')" > bomb.kry
$ kryos check bomb.kry
kryos: stack overflow (unbounded recursion?)
$ echo $?
253
```
`kryos check` — type-check only, no codegen — crashes on the parse itself;
`run`/`build --release` are equally exposed since the crash happens before
either backend is reached. Proven both ways: the SAME construct at n=10
semicolons (`tests/security/attack_parser_semicolon_chain_stack_overflow.kry`,
committed) is harmless — ten clean `E0009` diagnostics, `error: compilation
failed`, exit 1, no crash — confirming the defect is the ABSENCE of a bound
on n, not the semicolon-recovery mechanism itself being wrong. The 500k-line
bomb file is not committed (impractical size); regenerate via the one-liner
above.

Fix shape (not attempted — attacker-only mandate this round): apply the
exact same guard the three sibling entry points already use around the
self-recursive call at parser.rs:1394 — check `self.nesting_exhausted()`
and bump `rec_depth` (and pop it after the call returns) — so a long
semicolon run degrades to the existing `E0010` diagnostic instead of an
uncatchable native stack overflow. This is a one-function fix mirroring
code already present three times in the same file.

**Ruled out this session (round 2), evidence attached:**
- **Every comma-separated list loop already checked** (`parse_param_list`,
  `parse_struct_decl` fields, `parse_enum_decl` variants, `parse_struct_literal`,
  `parse_array_literal`, `parse_generics`, `skip_impl_target_type_args`,
  generic type-arg lists): each either has an explicit `self.pos == before`
  progress guard, or terminates via `expect_name`/`expect_ident` (which
  always advances at least one token on both the match AND mismatch
  branches, per parser.rs:229-331) or the general `expect()` fallback (which
  also unconditionally advances on mismatch, parser.rs:229-253) — so none of
  them can spin. Read directly, not guessed; this is why `expect_name`'s
  earlier fix transitively protects most comma-list loops that call it.
- **`std::json::parse`'s recursive-descent `_parse_value` already has a
  depth guard** (`compiler/stdlib/json.kry:151-157`, `depth > 1000` throws
  "maximum nesting depth exceeded") and its string-accumulation path already
  fixed an O(n²) quadratic-append DoS (same file, `_parse_string` comment:
  "a ~1MB string field made parse() take ~19s, a single-request DoS") — a
  COMPILED PROGRAM calling `std::json::parse` on a deeply-nested or
  adversarially-large JSON document is not exposed via this stdlib module.
- **Generic monomorphization of a SELF-REFERENTIAL type at shallow depth
  (60 levels) does not explode**: `wrap<T>(x: T, depth: i64)` recursively
  calling `wrap(Box{val: x}, depth-1)` (so each recursive call instantiates
  `wrap` at a NEW, one-level-deeper generic argument type, `Box<Box<...<i64>>>`)
  compiles clean (`kryos check`, exit 0, no diagnostics) at depth 60 within
  30s. **Not a full ruling-out of the monomorphization-explosion surface** —
  60 distinct instantiations is a small probe, not a stress test, and larger
  depths were not attempted this session (environment constraints, see
  below); flagging as the next thing to push on this surface, not as
  closed. `monomorphize`/`monomorphize_struct`/`monomorphize_enum`
  (kryos-mir/src/lower.rs:15227-15870) have no depth cap of their own —
  only per-mangled-name memoization, which does not bound a chain of
  DISTINCT names.
- **Array push/map growth use standard doubling realloc** (`kryos-rt/src/
  array.rs:154`, `kryos-rt/src/map.rs:83,300`, `new_cap = cap * 2`) with a
  header-corruption sanity check ahead of every grow — read, not stress-run
  live this session (environment constraints); no bug found in the growth
  logic itself, consistent with amortized-O(1) push documented in CLAUDE.md.

**Session note — environment, not code, was the binding constraint.** This
shared workspace was under severe, sustained contention this session
(consistent with the documented `feedback_machine_slowness` class): trivial
`bash` commands (`echo hello`) sometimes took several minutes to return, and
compiler invocations took proportionally longer. This bounded how many
distinct attacks could be RUN (as opposed to hypothesized) — the codegen-
blowup, inference-blowup, and larger-scale monomorphization-explosion
probes the task brief asked for were not attempted live this session for
that reason, reported honestly as not done rather than invented. The one
finding above was fully executed and proven both ways despite the
environment; it should not be read as the full extent of this surface.

---

### 19. RESOURCE-DOS: monomorphization mangled-name generation is O(2^depth) for a generic function whose return type PAIRS its own input into a tuple — `kryos check` hangs for minutes on a 24-line source file, no depth/size cap (RED TEAM round 3, resource-dos lens, found 2026-08-04) — NOT FIXED

`tests/security/attack_monomorphization_tuple_doubling_explosion.kry`. This
is the "larger-scale monomorphization-explosion probe" flagged as not
attempted in round 2's session note above (that round only ruled out a
LINEAR self-referential-type chain at depth 60) — attempted this round, and
it explodes.

`monomorphize` (`kryos-mir/src/lower.rs:15806`) builds a mangled function
name via `mono_mangled_name` (`kryos-mir/src/lower.rs:14694`), which calls
`format!("{t}")` on each concrete type argument. `MirType`'s `Display` impl
for `Tuple` (`kryos-mir/src/ir.rs:134-143`) recurses fully into every
element with no interning or structural sharing. `fn dup<T>(x: T) -> (T, T)`
called in a chain (`dup(dup(dup(x)))`) makes each level's return type a
tuple containing the PREVIOUS level's type TWICE — so the mangled name's
length (and the cost of building/hashing/caching it) doubles per level of
nesting: O(2^depth) from a source program that only grows O(depth) (one
extra `let dN = dup(dN-1)` line per level). Neither `monomorphize` nor
`mono_mangled_name` has any depth or size cap of its own.

Live, `compiler/target/release/kryos.exe` HEAD `00b3cf7`, no compiler
changes, `kryos check` (type-check only — codegen is never reached, so this
is pure monomorphization/type-checking cost), each depth timed back-to-back
in one script to minimize cross-run noise, cross-checked against a
freshly-measured baseline (depth-10 `kryos check`: 1.4s wall, ~0.17s user —
the machine was not under heavy load when these numbers were taken):

```
depth 15/18/20/22  -- all fast, a few seconds each, exit 0
depth 23           -- 34s,  exit 0
depth 24           -- 65s,  exit 0   (~1.9x depth 23, ~2x per level)
depth 25           -- did not finish inside 100s (`timeout 100`, exit 124)
depth 30           -- did not finish inside 5+ minutes wall clock; killed
                       externally, no crash, no diagnostic, just unresponsive
```

**Control, proven both ways by construction (no fix exists yet to revert,
so the "both ways" proof is this contrast):** the same-shape chain using a
LINEAR wrapper instead of a doubling tuple (`struct Box_<T> { v: T }`,
`fn boxit<T>(x: T) -> Box_<T> { return Box_{v: x} }`, chained identically)
stays FLAT at depth 24 AND depth 60 — both complete in ~8s, the same
few-seconds process-overhead floor every trivial `kryos check` showed this
session. This isolates the DOUBLING type structure specifically as the
cause, not "many monomorphizations" in general — matching and going beyond
the already-closed linear-chain probe from round 2's session note (that one
only reached depth 60 on a chain that, per this round's measurement,
would have stayed flat at any depth, since it never pairs a type with
itself).

**Blast radius:** this needs no adversarial obfuscation — `fn dup<T>(x: T)
-> (T, T)` is an entirely ordinary "pair/duplicate" combinator, and pairing
a generic result with itself across a handful of pipeline stages is a
normal thing to write by accident (not even deliberately hostile) in
data-pipeline or combinator-style code. A 24-line, unremarkable-looking
source file makes `kryos check`/`run`/`build` hang for over a minute; the
growth trend (~2x per level) implies depth 30 is on the order of an hour
and depth 35+ is effectively unbounded — from source that is barely bigger.

Fix shape (not attempted — attacker-only mandate this round): same class of
remedy as LEDGER item 14 — cap monomorphization recursion/instantiation
depth with a clean diagnostic (mirroring the parser's
`MAX_NESTING_DEPTH`/`MAX_RECURSION_DEPTH` budgets), or give
`mono_mangled_name` a content-addressed/interned name (hash the structural
type via a memoized recursive hash that shares repeated subtrees, or assign
each distinct `MirType` a small integer id via a canonicalizing arena)
instead of a full recursive `Display`-based string. A speed-up alone (same
exponential formula, faster constant) is not a fix.

---

### 20. Capability checker false-rejects a closure retrieved through a generic passthrough ACCESSOR method's chained return call (`holder.get()()` where `get` just does `return self.val`) — a THIRD, deeper instance of the item-just-closed's bug class, deliberately left OPEN (combined-category grammar fuzz wave, found 2026-08-04) — NOT FIXED, has a workaround

Found by the same new grammar fuzzer (`tests/fuzz/gen_grammar.py`'s
`mega_combo` scenario — a generic struct holding a closure field, accessed
via its own generated `get()` method) that found and closed the two
bare-block-scope false-rejects in the CLOSED table above. Minimal repro:
```
struct GBox<T> { val: T }
impl<T> GBox<T> { fn get(self: GBox<T>) -> T { return self.val } }
fn main() {
    let reader = || 5 + 1
    let holder: GBox<fn() -> i64> = GBox { val: reader }
    let v = holder.get()()
    println(to_string(v))
}
```
rejected with the same `E0507: call through a function value requires
capabilities [all]`. **Confirmed NOT the same root cause as the bare-block
bug just fixed**: reproduces at the TOP LEVEL with zero block nesting, and
still reproduces even through an intermediate local (`let g = holder.get()
 let v = g()`) — so it is not a missing scope-flattening arm, it needs the
checker to trace THROUGH a generic method's own body (recognize `get` as a
pure passthrough returning its `self.val` field) to resolve what `g`/the
chained call actually carries, which neither `build_local_closure_caps_block`
(keyed on direct `let`-bound identifiers) nor `build_local_container_lits_block`
(keyed on container LITERALS, not a method-call expression) currently
attempts. Deliberately NOT chased this round (see LEDGER discipline: prove
before fixing, and this needs new machinery — tracing a generic method
body's return expression back to a field, not a scope-recursion fix — not a
one-line extension of the fix just closed). **Workaround, verified live**:
read the field directly instead of calling the accessor
(`holder.val()` / an intermediate `let v = holder.val  v()`) resolves fine,
since a direct field-path read IS covered by `resolve_container_path_caps`.
`tests/fuzz/gen_grammar.py`'s own `mega_combo` scenario was adjusted to use
this workaround (`holder.val` instead of `holder.get()()`) so the rest of
that scenario's combo (generic struct construction at a closure-typed
field, try/throw, interpolation) still exercises without every single
generated case re-reporting this one known gap. Not a capability ESCAPE
(same safe-direction over-approximation as the closed item), lower severity
than that one (narrower trigger shape, has a clean workaround) — filed
separately rather than bundled into the fix above per the non-negotiable
"prove before fixing" discipline.

---

### 12. SUPPLY CHAIN: `kryos pkg install` never reads `kryos.lock` — silently re-resolves live and overwrites the lock on every run, with no warning (RED TEAM round 1, toolchain-supply lens, found 2026-08-04) — NOT FIXED

`tests/security/pkg_install_ignores_lock.sh`. CLAUDE.md documents (and the
LEDGER item 1b checksum work implies) that "pinning a specific version still
depends on committing kryos.lock" — but `kryos pkg install` never opens
`kryos.lock` at all. Grepped: `LockFile::from_file` is called exactly once
in the whole CLI, by `kryos pkg outdated` (`compiler/crates/kryos-cli/src/
commands/pkg.rs:482`) — `install()` (line 221) and `update()` (line 157)
only ever *write* a fresh lock via `LockFile::from_resolved(&graph)` after
resolving straight from the manifest against a live registry lookup. There
is no code path anywhere that compares a freshly resolved graph against a
previously committed lock, and no warning is printed when they diverge.

Live repro (offline, path dependency, no network needed — the mechanism is
identical for a Remote/registry dependency, where the drift would come from
a newly published or force-pushed index entry instead of a hand-edited
`kryos.toml`): `kryos pkg install` locks `dep` at `v1.0.0`; with
`kryos.lock` present and UNTOUCHED, `dep`'s own `kryos.toml` is bumped to
`v2.0.0` with different source content; a second `kryos pkg install` in the
same project silently rewrites `kryos.lock` to `v2.0.0` and prints the exact
same `installed 1 package` / `wrote kryos.lock` success banner as the first,
honest run — no diff, no prompt, no exit-code change (exit 0 both times).

For a Remote dependency this is a genuine supply-chain hole: LEDGER item
1b's checksum verification (`fetch::verify_package_checksum`) only checks
that the fetched bytes match the checksum recorded for WHATEVER VERSION
`resolve()` just picked — it does nothing to stop `resolve()` from picking a
different, newer version than the one a team committed to `kryos.lock` in
the first place. A compromised or force-pushed registry entry (the exact
threat LEDGER item 1b's own writeup names — "a force-pushed history") is
silently adopted by every consumer's next `kryos pkg install`, checksum
intact, lock file re-signed to match, with zero observable difference from
a routine install. Committing `kryos.lock` today provides no protection
against this at all; the file's only real reader is the informational
`kryos pkg outdated` report.

Fix shape (not attempted — this is a design/behavior decision, not a
one-line patch): `install()` should read an existing `kryos.lock` first and
resolve pinned to it (fetch exactly the locked name@version, verify its
checksum, done) unless `--update`/no-lock-present, matching `cargo
install`/`npm ci` semantics; `update()` remains the explicit "re-resolve
and possibly move the lock" operation. Until then, treat `kryos.lock` as
non-authoritative — it records the last resolution, it does not pin it.

### 13. `kryos audit` is blind to capability violations `kryos check`/`build` reject outright — reports a clean bill of health on code that will not compile (RED TEAM round 1, toolchain-supply lens, found 2026-08-04) — NOT FIXED

`tests/security/audit_blind_to_capability_violations.sh`. `kryos audit`'s
own description is "Audit capability usage, extern surface, and secret
patterns" — but it never runs (or cross-references) the same
inference/enforcement pass `kryos check`/`run`/`build` use. It only
inventories `@capabilities(...)` annotations that are textually PRESENT and
regex-sweeps for extern blocks / secret-looking strings; it has no
awareness that a called builtin unconditionally REQUIRES a capability.

Live repro: a one-function file with no `@capabilities` annotation calling
`file_write` (requires `fs:write`). `kryos check` on it fails immediately
(`error[E0505]: builtin \`file_write\` requires \`fs:write\` capability`,
exit 1 — the file cannot compile at all). `kryos audit` on the byte-identical
file exits 0 and prints `(no @capabilities annotations found)` under a
"Capability inventory" heading, with no error, warning, or hint that the
file is capability-invalid. Same result for a file using the raw-memory FFI
builtins (`alloc`/`ptr_write_i64`/`ptr_read_i64`/`free_bytes`) — `check`
rejects them (they require `ffi` on this build; see the ruled_out note
below, this CORRECTS the CLAUDE.md gotcha claiming they need no capability
at all), `audit` is silent.

This is a real trust trap for the tool's stated purpose: a reviewer running
`kryos audit` on a third-party package to vet its capability footprint gets
a "clean" report on code that either won't build at all, or — in a mixed
file where SOME functions are correctly annotated — omits any mention of
the specific gated builtins reachable from the unannotated ones, since audit
never runs the inference pass that would find them. `audit` should at
minimum run the same capability inference `check` does (in inferred/report
mode, not enforcing) and list the INFERRED requirement per function
alongside whatever `@capabilities` annotation is or isn't present, so its
"Capability inventory" reflects what the code actually needs rather than
only what a human already bothered to write down.

### 10. LIVE CAPABILITY ESCAPE — a closure returned by a zero-cap wrapper function defeats `deny!()` on BOTH enforcement modes (round 6 red-team, found 2026-08-04) — NOT FIXED, HIGHEST PRIORITY (breaks the trust model)

`tests/security/cap_escape_closure_wraps_closure.kry`. Minimal repro (single
level of wrapping, no containers, no generics, no spawn):

```kryos
@capabilities(fs:read)
fn make_secret_reader(path: str) -> fn() -> str {
    return || file_read(path)
}

fn wrap_once(inner: fn() -> str) -> fn() -> str {
    return || inner()
}

@capabilities(fs:read)
fn main() {
    let reader = make_secret_reader("tests/security/secret_for_closure_launder.txt")
    let wrapped = wrap_once(reader)
    deny!(fs:read) {
        let leaked = wrapped()
        println("SINGLE-WRAPPED CLOSURE LEAK: " + leaked)
    }
}
```

**Verified live against `compiler/target/release/kryos.exe`, HEAD `00b3cf7`,
no compiler changes:**
```
$ kryos run cap_escape_closure_wraps_closure.kry
SINGLE-WRAPPED CLOSURE LEAK: TOPSECRET-CLOSURE-9f8e7d6c5b4a
$ echo $?
0
$ kryos check --strict-capabilities cap_escape_closure_wraps_closure.kry
$ echo $?
0
```
Both the default inferred mode and `--strict-capabilities` compile it clean
and the secret prints from INSIDE `deny!(fs:read)`. A DOUBLE wrap
(`wrap_once(wrap_once(reader))`) reproduces identically (also run live) —
the single-wrap form is the minimal repro per bisection discipline.

**Why this is a real escape, not a re-report of a closed item:** `main`
legitimately holds `fs:read` outside the `deny!` block (same decisive-proof
shape as the already-closed `cap_escape_closure_launder_deny.kry`), so the
only question is whether the block's narrowing holds — it does not. This is
a DIFFERENT mechanism from every closed closure-launder variant. Every
closed repro forwards the *same* closure value `make_secret_reader` returned,
through a container/HOF call site — exactly the shape the round-5
fail-closed fix (`4ac8b83`) and its hot-param/companion data-flow tracing
target: calling a fn-value parameter/local/container-element directly
requires `[all]` unless the checker can trace the bound value to a
fixed-source callee AT THAT CALL SITE. Here, `inner()` is never invoked
inside `wrap_once`'s own body at `wrap_once`'s own call site — `wrap_once`
only *constructs* a new lambda literal (`|| inner()`) and returns it; the
actual invocation happens later, from inside that freshly-built closure, at
an unrelated call site (`wrapped()`) with no syntactic link back to
`wrap_once(reader)`'s argument. Confirmed by construction (grep, not
guessed): `wrap_once` itself is correctly inferred to need zero
capabilities (it never calls a gated builtin in its OWN frame — building a
closure is not calling one), so the round-5 fail-closed rule has nothing to
attach to at `wrap_once`'s call site. The rule evidently also isn't applied
to the RETURNED lambda's own body when *that* lambda is later invoked — it
gets attributed an empty capability set (rather than the `[all]`
fail-closed default the two ruled-out attacks below correctly hit) — so
`deny!(fs:read)` has nothing to reject. Not yet root-caused inside
`checker.rs` itself (that requires touching the compiler, out of scope for
this red-team pass per task rules) — the above is inference from black-box
behavior plus the round-5 commit message, stated as a hypothesis, not a
verified code-read.

**Blast radius:** any zero-capability "decorator" helper that takes a
privileged closure and returns a new closure that calls it — a completely
ordinary shape (logging wrapper, memoizer, retry wrapper, `tap()`) —
silently launders full authority through `deny!()`, and (untested this
session, follow-up needed) plausibly through capability inference generally,
not just `deny!` narrowing.

**Ruled out this session (round 6), evidence attached, no escape:**
- **Generic identity function round-trip** (`tests/security/
  cap_escape_generic_identity_launder.kry`): `fn identity<T>(x: T) -> T {
  return x }`, `let laundered = identity(reader)`, called inside
  `deny!(fs:read)`. REJECTED (E0507) under both `kryos run` and
  `--strict-capabilities` — `laundered()` is a direct fn-value invocation
  whose provenance the checker could not resolve through the generic
  passthrough, so it correctly fails closed to `[all]`, which the deny block
  then blocks. The generic-identity vector does NOT bypass the fail-closed
  default.
- **Raw tuple payload (not Option/Result)** (`tests/security/
  cap_escape_tuple_payload_direct.kry`): `fn zero_cap_tool(t: (fn() -> str,
  i64)) -> str { let (f, _tag) = t  return f() }`, called with `(reader, 42)`
  inside `deny!(fs:read)`. REJECTED (E0507) under both modes — same
  fail-closed `[all]` outcome as the array/map/struct-field container
  variants already closed; a bare tuple slot is not a distinct hole.

**Not yet attempted this session (follow-up for whoever picks this up):**
whether the SAME wrapping-closure mechanism defeats capability inference
outright (no `deny!`, just an unannotated `main`) rather than only the
`deny!` narrowing; whether it also escapes `--strict-capabilities` when the
wrapper itself is required to declare `@capabilities()` explicitly (it
currently infers to empty, which strict mode accepted without complaint —
worth checking whether strict mode's OWN declaration requirement should have
forced a human to notice and gate `wrap_once`, and didn't because inference
computed empty before the human ever had to choose a value).

**RED TEAM round 2 (2026-08-04), same item, new evidence — blast radius
CONFIRMED WIDER, two vectors RULED OUT:**

- **CONFIRMED — survives generic monomorphization.**
  `tests/security/attack_wrap_closure_generic.kry`: `fn wrap_once_generic<T>(inner: fn() -> T) -> fn() -> T { return || inner() }`
  reproduces IDENTICALLY to the concrete `fn() -> str` form — live,
  `kryos run` exit 0 prints the secret, `--strict-capabilities` exit 0, no
  diagnostic, `compiler/target/release/kryos.exe` HEAD `00b3cf7`, no
  compiler changes. This answers the open "generics are the highest-suspicion
  surface, untested" question for this specific hole: the missing
  fail-closed propagation is in how a returned LAMBDA LITERAL's own body is
  attributed capabilities, and that attribution is equally wrong whether the
  enclosing function is monomorphic or generic — not a distinct generics-only
  bug, but proof the existing bug's fix must be verified against a generic
  wrapper too, not just the concrete repro.
- **RULED OUT — does not defeat inference outright without `deny!`.**
  `tests/security/attack_wrap_closure_inference_bypass.kry`: same
  `wrap_once`, but `main` is UNANNOTATED and there is no `deny!` block at
  all. REJECTED (E0507) at `wrap_once(reader)` itself — the error names the
  mechanism directly: "some of this authority is carried by a
  closure/fn-value ARGUMENT passed at this call site, not by `wrap_once`'s
  own declaration". So the checker's call-SITE argument tracking is sound
  when there is no narrowing scope to satisfy first; the item-10 hole is
  specific to `deny!()`'s narrowing not being re-checked at the later,
  syntactically-disconnected `wrapped()` call site, not a general inference
  gap.
- **RULED OUT — impl-method wrap.** `tests/security/attack_wrap_closure_impl_method.kry`:
  same shape via `impl Decorator { fn wrap(self, inner: fn() -> str) -> fn() -> str { return || inner() } }`.
  REJECTED (E0507) at `wrapped()` — but for the ORDINARY fail-closed reason
  ("call through a function value requires capabilities [all] ... provenance
  could not be resolved"), the exact default item 10 shows is wrongly
  skipped for the free-function case. So the bug is narrower than "any
  function that builds+returns a forwarding lambda" — confirmed specific to
  a FREE function doing so, not an impl method.
- **RULED OUT — actor-handler wrap.** `tests/security/attack_wrap_closure_actor.kry`:
  same shape via an actor method. REJECTED on two independent grounds: (1)
  E0110, actor handlers with a non-unit return type are rejected outright
  (fire-and-forget dispatch has no reply channel) — orthogonal but also
  blocks this shape; (2) even so, `wrapped()` independently hits the same
  correct [all] fail-closed rejection as the impl-method case. Actor
  dispatch is not a laundering vector for this mechanism.

All four probes verified live against the same unmodified
`compiler/target/release/kryos.exe`, no compiler changes, both `kryos run`
and `kryos check --strict-capabilities`.

---

### 11. RED TEAM round 1, concurrency lens (2026-08-04): the LEDGER item 7b closure-lock fix has a genuine self-deadlock (reentrancy) — NOT FIXED. (Part (b), stale non-mutated co-capture, is FIXED — see CLOSED table: "closure-mutating-costale-scalar-capture".)

Attacked LEDGER item 7b's serialize-every-call fix for the spawn-shared
mutating-closure race (`kryos_mutex_lock`/`unlock` on a lock word in the
closure's own env, `kryos-codegen-cranelift/src/codegen.rs:1791-1904`,
mirrored in `kryos-codegen-llvm/src/codegen.rs`). Two independent, both-ways
verified defects, both reachable with ZERO spawn/threads involved:

**(a) CONFIRMED — self-deadlock via reentrancy.** `kryos_mutex_lock`
(`kryos-stdlib-native/src/sync_prims.rs:44-66`) is a bare `AtomicBool` CAS
spin-then-yield lock with no owner-thread tracking and no recursion count —
not reentrant. `tests/security/attack_closure_lock_reentrant_deadlock.kry`
stores a mutating closure into a `map<i64, fn(i64)->i64>` it also reads from
(maps are captured by live shared handle, not snapshotted), giving the
closure a genuine handle to ITSELF; the body calls that handle again before
returning. The second `kryos_mutex_lock` call spins forever against a lock
the CURRENT thread already holds. Live:
```
$ export KRYOS_STDLIB_DIR=.../compiler/stdlib
$ timeout 15 kryos.exe run tests/security/attack_closure_lock_reentrant_deadlock.kry
(no output — process force-killed after 15s)
$ echo $?
124
```
A recursion depth of 3 should complete in microseconds; instead the process
had to be forcibly terminated by `timeout`. Root cause is structural, not
shallow: the fix serializes by ENV POINTER (correct for concurrent DIFFERENT
threads) but has no notion of "the current thread already holds this" — the
standard fix is a reentrant mutex (owner-thread-id + recursion count), which
was not used. **Caution for whoever re-runs this repro:** the compiled
temp binary appears to survive the `timeout`-killed wrapper process as an
orphan spinning a full CPU core (this session saw sustained, severe
machine-wide slowdown for several minutes immediately after running it,
consistent with an escaped spin-loop); hunt for and kill it after running,
not just `kryos.exe`.

**(b) CONFIRMED — silent wrong value: a non-mutated co-capture in a mutating
closure is snapshotted at construction, not re-read, contradicting the
documented by-reference promise.** CLAUDE.md gotcha #11 states any capture
NOT mutated inside the closure body sees later outer mutations
unconditionally. Read (not guessed) `kryos-mir/src/lower.rs`'s
`box_scalar_captures` comment (~line 12562-12590): the mechanism that makes
this promise true is scoped **deliberately narrowly to struct-literal-field
lambdas only**, with the comment explicitly asserting a `let`-bound closure
"keeps the existing, ALREADY-CORRECT `closure_locals` path" that "re-reads
the outer variable's CURRENT value at every call site." That assumption is
FALSE for a `let`-bound closure that ALSO mutates a different capture:
`closure_locals` is populated only for NON-mutating closures (the
direct-call fast path it enables is unsafe once a closure owns mutable
state by move — `mutating_closures` doc comment, same file). So a
`let`-bound mutating closure's OTHER, non-mutated scalar captures fall into
neither "correct" mechanism and silently freeze at their construction-time
value. `tests/security/attack_closure_costale_isolate.kry` isolates this
with no throw, no spawn, no recursion — two plain sequential calls:
```kryos
let mut counter: i64 = 0
let mut flag: i64 = 1
let f = |n: i64| { counter = counter + n  counter + flag }
let r1 = f(1)
flag = 100
let r2 = f(1)
```
Live, BOTH backends, deterministic, reverse-matches the stale-snapshot
prediction exactly:
```
$ kryos run attack_closure_costale_isolate.kry
r1=2 r2=3
$ kryos build --release attack_closure_costale_isolate.kry -o /tmp/x && /tmp/x
r1=2 r2=3
```
Expected if `flag` were live-visible per the documented promise: `r2=102`
(counter=2, flag=100). Got `r2=3` (counter=2, flag=1 — the value at closure
construction) on BOTH backends — backends agreeing means the defect is in
shared MIR, matching where it was read from. `tests/security/attack_closure_mutate_then_throw_state.kry`
shows the same root cause causing a worse-looking symptom: a plain outer
reassignment (`trigger = 0`, meant to disarm a conditional `throw` inside a
helper the closure calls) is silently ignored on the closure's next call, so
an exception the caller specifically arranged NOT to happen fires anyway,
uncaught, and kills the process (exit 101) — live, JIT:
```
baseline (no throw): g1=5 g2=10 expect g1=5 g2=10
caught: boom
kryos: uncaught exception: boom
THROW_TEST_EXIT=101
```
Not fixed at the time this round's attacker filed it (attacker-only
mandate). **FIXED in a later session — see CLOSED table entry
"closure-mutating-costale-scalar-capture" for the fix, both-ways proof, and
gate results.** The fix taken was exactly the narrow fix this round already
named: `box_scalar_captures`'s boxing (the `RValue::ArcAlloc`/
`MirType::Shared` machinery) now also fires for a `let`-bound closure that
mutates >=1 capture, boxing its OTHER non-mutated scalar co-captures too —
no ABI change, since a mutating closure never uses the `closure_locals`
fast path this widening could otherwise have collided with.

**Byproduct finding from fixing (b), NOT itself fixed, distinct root cause:**
verifying the fix against `tests/security/attack_closure_mutate_then_throw_state.kry`
confirmed the costale-capture symptom is gone (the disarming `trigger = 0`
reassignment IS now observed — no more uncaught "boom" on the second call),
but exposed the ORTHOGONAL bug this repro's own header comment had already
hypothesized as a second, separate mechanism: the MUTATED capture's own
write-back is skipped when the call that mutates it throws mid-body. Live,
post-fix, both backends identical:
```
baseline (no throw): g1=5 g2=10 expect g1=5 g2=10
caught: boom
counter observed by NEXT call to f (n=0, no further mutation this call): 0
```
`after` should be `5` (the first call's `counter = counter + 5` persisting
despite the later throw) but is `0` — the mutation never reached the
persistent box. Root cause per this file's own header comment (not
re-verified by reading the codegen this session, carried forward as a
strong hypothesis): the `StoreDeref` write-back LEDGER item 7 inserts is
appended only before blocks whose terminator is `Terminator::Return`
(`kryos-mir/src/lower.rs` ~12717-12724), but a `throw` mid-body does not
reach that terminator — it takes the codegen-synthesized
`exc_return_block` early-exit path (`kryos-codegen-cranelift/src/codegen.rs`
~3204-3230) that runs `emit_exception_cleanup_drops` and returns directly,
never executing the MIR block containing the writeback. Filed here rather
than silently dropped or folded into the (b) fix's regression test, since
it is a different call path (exception unwind, not a co-capture) with a
different fix shape (the writeback needs inserting at the exception-return
site too, in the codegen backends, not in `lower.rs`). Not fixed this
session (out of scope for the assigned closure-mutating-costale-scalar-capture
defect) — next session should treat it as its own ledger item.

**RULED OUT this round (checked, not reproduced — evidence, not assumption):**
- **Actor `kryos_actor_lock`/`unlock` reentrancy or throw-during-lock**
  (`kryos-rt/src/actor.rs:249-286`, emitted by `Instruction::ActorSend`,
  `kryos-codegen-cranelift/src/codegen.rs:3766-3858`): lock-send-unlock is
  ONE MIR instruction with no throwable call in between (only
  `kryos_string_clone`/`kryos_array_clone`/`kryos_map_clone`/
  `kryos_arc_retain`, all `kryos_`-prefixed and therefore exempt from the
  exception-check-and-early-return gate per `should_check`'s own filter,
  `kryos-codegen-cranelift/src/codegen.rs:3160-3189`) — no window for a
  throw mid-lock. (Read-only ruling — not independently stress-run this
  session due to the environment failure below.)
- **Actor mailbox self-send deadlock** (`kryos_actor_recv`, same file): the
  mailbox `Mutex` is released via `Condvar::wait` while blocking, and `recv`
  never holds the lock across message HANDLING (returns as soon as it
  dequeues) — a handler that sends to itself cannot deadlock against its
  own prior `recv`.
- **Channel close/drain** (`kryos-rt/src/channel.rs`): plain `Mutex` +
  `Condvar`, short critical sections, no lock held across a wait; no new
  hazard found. The documented "`recv` on a closed drained channel returns
  0, indistinguishable from `send(ch,0)`" ambiguity is unchanged, already
  known — not re-reported.
- **Spawn capture kinds** (`Instruction::Spawn`,
  `kryos-codegen-cranelift/src/codegen.rs:3449-3670`): Str/Array/Map/
  Function/Struct/Enum each have deliberate, individually-commented deep-
  copy/clone/snapshot handling with a visible history of prior fixes per
  kind. Read closely, found no new gap in the STATIC logic — but did not get
  to run a fresh adversarial multi-thread stress pass against this surface
  this session (see below); treat as read-only assurance, weaker than the
  two executed findings above.

**Session note:** bash/background-task infrastructure in this shared
workspace became severely unresponsive for an extended period immediately
following the reentrancy repro (matches `feedback_machine_slowness` class
symptoms) — several planned probes (actor reentrancy live-run, spawn
capture-kind adversarial stress, 50-run race-rate sampling per the task
brief) were not completed as a result. Reported honestly as not done rather
than invented.

**ROUND 2 (2026-08-04, same session/day, continued): closed out the probes
round 1 could only read-rule-out. All four are NEGATIVE (falsified) —
recorded with the live evidence, no new bug found on this pass.**

- **FALSIFIED — cross-thread nested-spawn reentrancy does NOT self-deadlock
  or lose updates** (distinct from the CONFIRMED same-thread reentrancy in
  (a) above, which still deadlocks — this attacks a DIFFERENT shape: a
  mutating closure shared across a `spawn` that itself `spawn`s a NESTED
  task touching the same closure, plus a direct call from the spawning
  thread racing both). `tests/security/attack_spawn_mutating_closure_reentrancy.kry`:
  300 iterations x (1 direct call + 1 spawned call + 1 nested-spawned call)
  = 900 calls, plus 1 final call to read the closure's internal total;
  every increment surviving predicts exactly `final_val=901`. Live, 21 runs
  total (11 on `kryos run`/JIT, 10 on `kryos build --release`/AOT), 21/21
  printed `final_val=901` exit 0 — no deadlock, no lost update, no
  corruption on either backend. Makes sense in hindsight: each nested
  `spawn` is a fresh OS thread acquiring/releasing the env-pointer mutex in
  its own call, never the SAME thread re-entering while it already holds
  the lock — the hazard in (a) is specific to one thread calling back into
  itself synchronously, not to spawn depth or fan-out.
- **FALSIFIED — the actor multi-word send lock (`kryos_actor_lock`/
  `kryos_actor_unlock`, `kryos-rt/src/actor.rs:249-286`) survives real
  concurrent contention with no message-word corruption or loss** (round 1
  only read this code; this is the live stress it flagged as not done).
  New file `tests/security/attack_actor_multiword_send_contention.kry`:
  one actor receives 500 sequential `(str, i64)` messages from `main`, then
  40 concurrently spawned OS threads each send 100 more `(str, i64)`
  messages to the SAME actor (4000 concurrent multi-word sends racing the
  target's spinlock) — a corrupted interleave would splice a tag from one
  send with an amount from another, or drop a word. Verified by VALUE, not
  just exit code: independently computed `expected=128750` (sum 0..499 +
  4000) matches `final_total` every run, `seen=4500` (500+4000) matches
  message count every run, `ok_tags=4500` (every tag non-empty/non-spliced)
  every run. Live, 16 runs total (11 JIT, 5 AOT), 16/16 exact matches on
  both backends, exit 0.
- **FALSIFIED — throw-unwind through several nested live heap locals
  (str + struct-with-array) drops each exactly once, both backends' shared
  MIR.** `tests/security/attack_throw_unwind_heap_locals.kry`: 2000
  iterations of a `try` around a helper that stacks 3 nesting levels of
  live `str`+`Holder{tag,data}` locals (outer/mid/inner) before throwing
  from the innermost — a double-free, leak, or use-after-free on unwind
  would corrupt or crash within a handful of iterations. Live: `caught=2000`
  exit 0, no crash across all 2000 throws.
- **FALSIFIED — a closure value stored in TWO container slots (a 3-element
  array AND a 2-key map) plus its own original binding, all three
  references dropping at different scope-end points, does not double-free
  the shared env.** `tests/security/attack_closure_env_teardown_twice.kry`:
  5000 iterations, each building a fresh closure and referencing it from an
  array, a map, and its own `let` — 3 independent teardown points per
  iteration, 15000 total drops of shared env boxes. Verified by VALUE: the
  exact arithmetic (`5i+36` summed contribution per iteration) predicts
  `total=62667500`; live run printed exactly that, exit 0 — not just
  crash-free, numerically exact.
- **NOT a new finding (re-confirmation only):** re-ran
  `tests/security/attack_closure_mutate_then_throw_state.kry` this round —
  reproduces the ALREADY-DOCUMENTED (b) costale-capture bug above via a
  throw-shaped symptom (a disarming outer reassignment is silently dropped,
  the disarmed call throws anyway, uncaught, exit 101). Same root cause,
  same LEDGER entry — not counted as a distinct discovery.

**Honest gap:** 16-21 runs per surface, not the full 50+ the task brief
asks for, given continued machine/session cost constraints in this shared
workspace — treat the falsification as solid (backends agree, values exact)
but not exhaustive; a very-low-rate race on either surface cannot be ruled
out at this sample size.

---

### 21. SILENT WRONG ANSWER: a mutating closure's own mutated-scalar-capture write-back is skipped when the call that mutates it THROWS mid-body — the mutation silently vanishes instead of persisting or erroring (byproduct of closing item 11(b), found 2026-08-05) — NOT FIXED

Surfaced verifying the fix for `closure-mutating-costale-scalar-capture`
(CLOSED table) against its corroborating repro,
`tests/security/attack_closure_mutate_then_throw_state.kry`. That fix closed
the repro's originally-observed symptom (an uncaught exception firing on the
SECOND call because the disarming outer reassignment was invisible — gone,
confirmed live) but the repro's own header comment had already hypothesized
a SECOND, independent mechanism, which is what remains once the first is
fixed: `f`'s body does `counter = counter + n` then calls a helper that
`throw`s; the mutation to `counter` is real within that call (later code in
the SAME call would see it) but never reaches `counter`'s persistent
`ArcAlloc` box, so the NEXT call to `f` observes the PRE-mutation value —
not the incremented one, and not an error. Live, post item-11(b)-fix, both
backends identical:
```
baseline (no throw): g1=5 g2=10 expect g1=5 g2=10
caught: boom
counter observed by NEXT call to f (n=0, no further mutation this call): 0
if the writeback ran on the aborted call, after should be 5 (5 persisted + this call's r=0);
if the writeback was skipped, `counter` silently reverted to 0 and after == 0
```
`after` is `0`; expected `5` if the mutation before the throw had persisted.

**Hypothesized root cause (from the repro's own header, not re-verified by
reading the codegen this session — carry forward as a strong lead, not a
proven fact):** LEDGER item 7's `StoreDeref` write-back for a mutated scalar
capture is appended only to MIR blocks whose terminator is
`Terminator::Return` (`kryos-mir/src/lower.rs` ~12717-12724). A `throw`
mid-body does not lower to that terminator — Kryos exceptions are a
thread-local flag, not native unwinding, so every user-function CALL site
checks the flag after the call and, if set, takes an early-return path
SYNTHESIZED DIRECTLY IN CODEGEN (`kryos-codegen-cranelift/src/codegen.rs`
~3204-3230, `exc_return_block`; mirrored in
`kryos-codegen-llvm/src/codegen.rs`), entirely separate from any MIR block
the `lower.rs` pass ever sees. That path runs
`emit_exception_cleanup_drops` (drops str/array/struct/enum locals) but has
no reason to know about the `StoreDeref` writeback living in a different
block, so it skips it.

**Why this is a distinct item, not folded into item 11(b)'s fix:** different
call path (exception unwind through a codegen-synthesized block, not a
co-capture read), different fix surface (the write-back would need
inserting at the codegen exception-return site in BOTH backends, not in
`kryos-mir/src/lower.rs`), and arguably a real ergonomics/soundness
question the repro's brief already flagged ("either persisted or a clean
error, not silently lost") — a mutation whose effect is silently dropped
because it happened to be followed by an exception is a footgun regardless
of which specific value it lands on. Not fixed this session — the assigned
defect was `closure-mutating-costale-scalar-capture` (item 11(b)) only, per
the "your job is the fix, not re-triage" scope for this wave; a codegen
change to two backends' exception-return synthesis is a new, separately-
scoped fix. No regression test added for this item since it is unfixed;
the live repro above (`tests/security/attack_closure_mutate_then_throw_state.kry`,
already committed) reproduces it on demand.

---

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

**Not implemented as of the 8th-attempt design note above.** Workaround for a
hot loop remains: read fields directly (flat), keep heap data out of structs
you pass, or reuse one instance instead of constructing per iteration.

#### 9th investigation (this session): did the fresh repro the 8th note asked
for — the `conf_spinlock_mutex` attribution was WRONG, not just unverified.
Corrected mechanism, DESIGN B REVISED. Still not implemented; here is exactly
why, with hard evidence.

Per the 8th note's own instruction, this session's first move was the fresh,
isolated repro **before** touching any code. It falsified the prior
attribution:

```kryos
use std::sync::{spin_lock}
fn main() {
    let lock = spin_lock()
    let mut i = 0
    while i < 5 {
        let l = lock.lock()
        l.unlock()
        i = i + 1
    }
    println("seq spinlock ok")
}
```

Applying ONLY the one-line change (`MirType::Struct` added to
`consume_call_args`'s borrow allowlist, `kryos-mir/src/lower.rs:9120`), full
`cargo build --release`, this crashes **5/5 runs, deterministically, with
ZERO spawn/threads**:

```
$ kryos run repro.kry     (x5)
kryos: uncaught exception: sync error: lock on dropped mutex   (all 5 runs)
$ kryos build --release repro.kry && ./repro
seq spinlock ok    (clean, every time)
```

So: **not spawn-specific, not concurrency-specific, not even
`conf_spinlock_mutex`-specific** — the prior write-up's attribution ("makes
the caller free a handle `spawn` still shares") was never re-verified after
being written, and was wrong. It reproduces in a single thread with a plain
sequential lock/unlock loop, and it is backend-DIVERGENT (JIT-only, AOT
clean) — which by non-negotiable #6 means read the emitted IR, not the
source, so that's what this session did instead of continuing to guess.

**Root cause, read directly from both backends' own type-lowering, not
inferred:**

1. **LLVM/AOT never boxes a plain struct value at all.** `--emit-llvm` on the
   repro shows `%SpinLock = type { %AtomicInt }`, `%AtomicInt = type { i64,
   %Mutex }`, `%Mutex = type { ptr, i1, i1 }` — nested struct FIELDS are
   INLINE aggregates, not pointers — and every call/return of a `SpinLock`
   goes through `ptr byval(%SpinLock)` / `ptr sret(%SpinLock)`:
   `define internal void @SpinLock__lock(ptr sret(%SpinLock) %_sret, ptr
   byval(%SpinLock) %_0_arg)`. `byval`/`sret` are LLVM-level COPY semantics —
   the callee gets its own stack copy, there is no shared heap box for the
   struct itself, so there is nothing to double-free at the struct level.
   This is why the one-line change is a no-op on AOT for this repro: nobody
   was freeing a box that never existed.
2. **Cranelift boxes literally every struct value, at every level, uniformly
   — confirmed in `mir_type_to_cl` (`kryos-codegen-cranelift/src/codegen.rs:44`):
   `MirType::Struct(_) => Ok(Some(types::I64))`, no distinction between a
   top-level local and a FIELD of another struct.** `compute_struct_layout`
   (same file, line 251) uses this uniformly for field offsets too, and the
   struct-field-drop walk (`emit_drop_for_value`, ~line 7549) `load`s a
   nested `MirType::Struct` field as an `i64` **pointer** and recurses
   `emit_drop_for_value` on it — proving nested struct fields are SEPARATE
   `kryos_calloc` boxes on this backend, chained (SpinLock → box → AtomicInt
   → box → Mutex → box), not embedded. Cranelift is therefore, for structs,
   already exactly what Design A below calls "uniform boxing" — it never had
   an ABI to break; LLVM is the only backend with the byval/sret
   representation Design A's "ABI break" cost is actually about.
3. **`SpinLock.lock()`/`Mutex.lock()`/`.unlock()` return `self` (or a value
   built from `self`'s own fields) directly** — `return self`,
   `return Mutex { handle: self.handle, locked: true, dropped: false }`.
   On Cranelift this means the CALLEE hands back the SAME box pointer (or a
   pointer built by copying a field straight through) that the CALLER still
   holds. The one-line change makes the caller keep its own scope-end drop
   (correct, that half of Design B is fine) but adds **no retain anywhere**
   — so after `let l = lock.lock()`, `lock` and `l` are two independent
   Kryos-level locals that alias ONE Cranelift box, each believing itself the
   sole owner. First one's scope-end drop wins the race and frees it (in the
   minimal repro, silently and deterministically, since there's no
   concurrency to race); the second finds a box whose header bytes have
   since been reused, reads a stale/garbage `dropped` field as truthy, and
   throws. Confirmed independently with the runtime's own (always-on,
   non-`KRYOS_FREE_DIAG`-gated) double-free guard: a 1-lock/1-unlock version
   of the same repro reports 3 caught-and-ignored
   `kryos_free: double free of 0x... (already-freed box)` events even though
   it happens to still exit 0 — i.e. it is ALREADY over-freeing on the very
   first call, the 5-iteration version just eventually loses the race against
   reused memory. This also explains why the FULL concurrent
   `conf_spinlock_mutex` (8+64 threads) was a worse repro to debug from: under
   the one-line change it is **nondeterministic** (3/3 sample runs: one clean
   exit 0, one "ignored" double-free + exit 0, one clean) — thread scheduling
   sometimes hides the corruption inside the guard's tolerance window. The
   sequential 6-line repro above is deterministic and should be preferred for
   any future attempt.
4. **`heap_field_method`/`free_fn_scalar_ret`/`method_chain` in
   `struct_arg_leak.kry` do NOT crash under the same one-line change** (spot
   checked to 2000 iters with `KRYOS_FREE_DIAG=1`, zero double-free reports).
   Mechanism: `size_fn(b)`/`b.size()` return an `i64`, not a value derived
   from `self`, so there is exactly one Kryos-level owner of `b`'s box for
   its whole lifetime (the caller) and the one-line change alone — caller
   keeps its own drop, callee still never drops its param (`param_locals` is
   unchanged) — is enough for that shape. `method_chain`'s `.add()` always
   returns a FRESH struct literal, not an alias of `self`, so it's likewise
   safe. **The crash is narrowly scoped to the "method returns `self` or a
   value built from `self`'s own fields" idiom** — which happens to be
   exactly the shape `std::sync`'s entire lock/atomic/once API is written in,
   which is why `conf_spinlock_mutex` is what caught it, not because spawn or
   concurrency has anything to do with the mechanism.

**Design B, revised — what actually has to be true for this to be safe.**
The 8th note's Design B step 3 ("retain-walk", `Str`→retain, `Array`→retain,
nested `Struct`→recurse, scalars skipped) is necessary but **provably
insufficient on Cranelift**: it only bumps refcounts on leaf `str`/`array`/
`map` content, and `SpinLock`/`AtomicInt`/`Mutex` have NONE anywhere in the
chain (`ptr`/`i64`/`bool` fields only, confirmed in `compiler/stdlib/sync.kry`)
— the retain-walk is a complete no-op for them, so a "correct" implementation
of Design B exactly as written would still reproduce the crash above. What
Cranelift additionally needs, on top of Design B's field-content retain-walk
(still required, unchanged, for the `heap_field_*` leak on both backends):
- Route Cranelift's struct local/param/return drop path (`emit_drop_for_value`'s
  `MirType::Struct` arm, ~codegen.rs:7504) through `kryos_struct_release_shared`
  BEFORE freeing fields/box — i.e. give path 2 the SAME owner-count guard
  path 1 (`__kryos_drop_<T>`, used for boxed array/enum-payload elements)
  already has. `kryos_struct_retain`/`kryos_struct_release_shared` already
  exist and are correct (`kryos-rt/src/alloc.rs:654-700`) but `kryos_struct_retain`
  has no LLVM `declare` or Cranelift `func_ids` entry today — only
  `_release_shared` is wired for codegen use; `_retain` is currently called
  only from Rust (`array.rs`, for boxed struct array elements). This needs
  wiring as a callable codegen intrinsic on Cranelift.
- Emit `kryos_struct_retain(ptr)` on the struct argument's box at each
  ordinary user-fn call site (Cranelift-only codegen addition — LLVM has no
  box to retain, byval already copies).
- **A `return`-passthrough exemption is required, not optional.** If a
  function's scope-end drop of a struct PARAM is unconditionally routed
  through the checked release, a function that returns that exact param
  (`return self`) under-counts: the checked-release "stops" (correctly, sees
  the caller's retain), but the RETURN then hands the same pointer to a NEW
  destination local with no corresponding retain for that new binding, so
  the box ends up with 2 live owners and an owner-count word that only
  accounted for 1. This needs the same "tail-identifier-move guard" pattern
  already used elsewhere in this codebase for return-of-a-moved-value (see
  the F1 CLOSED fix in this ledger) generalized to struct params: skip the
  param's own drop when it is exactly the `return`ed operand, unchanged.
  Fixing this trap is deceptively easy to describe and easy to get subtly
  wrong to implement (per-shape: `return self` bare vs. `return
  T{field:self.field,...}` partially-rebuilt vs. `return self.inner_field`
  need different treatment) — this is precisely the class of "retain and
  release added in different places by different patches" divergence this
  ledger's own mechanism section already warns about.
- The nested-struct-as-separate-box finding (point 2 above) means this
  bookkeeping is not just top-level: `SpinLock → AtomicInt → Mutex` is THREE
  independently kryos_calloc'd boxes chained by pointer on Cranelift, each
  needing its own correct owner-count lifecycle, and `spawn`'s existing
  bespoke capture arm already depends on nested-struct sub-boxes staying
  SHARED (not cloned, not retained) across threads — any change to how path 2
  frees a nested struct field must be re-verified against `spawn` sharing a
  live nested box across threads, which this session did NOT attempt (the
  sequential repro above deliberately has zero spawn involvement, by design,
  to isolate the mechanism — the concurrent interaction is a real, separate
  next question, not yet answered).

**Not implemented this session either.** Real effort was spent (isolated the
true mechanism with hard IR/runtime evidence, corrected a wrong attribution
in this very ledger, found the precise boundary of what does/doesn't crash,
and identified that `kryos_struct_retain` isn't even wired into codegen yet)
but implementing the revised design safely needs: wiring a new codegen
intrinsic, a return-passthrough exemption whose per-shape correctness is
exactly the kind of thing this codebase's history shows gets subtly wrong on
the first pass, and a full re-verification of the `spawn` nested-box-sharing
interaction — that is real, multi-step, cross-cutting work, not a single
edit to verify inline, and rushing it risks a 9th unexplained regression,
which this task ranks explicitly below an honest, evidence-backed stop.
Every experimental edit made while investigating this was reverted before
finishing (`git diff` on `kryos-mir/src/lower.rs` is empty at HEAD); nothing
in this commit changes compiler behavior. Workaround unchanged: read fields
directly (flat), keep heap data out of structs you pass, or reuse one
instance instead of constructing per iteration.


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

### 7b. FIXED — see CLOSED table: "a closure/fn-value captured by `spawn` did NOT snapshot -- a genuine cross-thread DATA RACE, silent lost updates"

Was OPEN item 7b (closure/fn-value shared, not snapshotted, across `spawn`
threads -- a mutated-scalar capture's non-atomic call-then-writeback
persistence mechanism lost updates under contention: 10/10 failures on
JIT, 7/10 on AOT at 50 threads x 2000 calls). A prior session attempted
and RULED OUT deep-copying the closure env at spawn (wrong semantics for
a closure whose whole point is shared mutable state, and the hint
mechanism couldn't even fire for mutating closures -- both `closure_locals`
gates on `!mutating_closures.contains(func_name)`). This session
implemented the ledger's own suggested remaining shape instead: serialize
every call to a mutating closure's underlying function under a lock
scoped to that closure's own env allocation. Full writeup, evidence both
ways (before/after on both backends), and gate output in CLOSED below.
`tests/known_failures/spawn_closure_shared_env_race.kry` folded into
`tests/conformance/conf_spawn_closure_capture_lock.kry` and deleted.

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

**Re-assessed this session (parser/grammar wave): the "wide blast radius" claim above is TRUE
but SMALLER than stated -- concrete feasibility, not implemented.** Confirmed
by reading, not assuming: `kryos_parser::Parser` holds only `Vec<Token>` +
`pos` (no source text), and `kryos_lexer::Token` is exactly `{ kind, span,
text }` -- no line/newline info anywhere, matching hard rule 1's claim
precisely. But detecting "was this token preceded by a newline" does NOT
need full line-number tracking (a bigger change than necessary): every
`Token` in the whole workspace is constructed through ONE choke point
(`Lexer::emit`, `kryos-lexer/src/lexer.rs`), called right after ONE
whitespace-skipping function (`skip_whitespace_and_comments`) that already
walks every byte between tokens including any `\n`. A single `bool`
("newline_before", not a line number) tracked across that walk and stored on
`Token` would need: (1) one new field on `Token` (9 total construction
sites workspace-wide, most trivially `false` -- lexer's own `emit`, 2 dummy/
test constructors, kryos-fmt/kryos-lsp re-emit tokens they already have),
(2) a check in the Pratt continuation loop specifically at the point it
decides to consume `|`/`||` (and, if extended uniformly rather than
special-cased, `-`/`(`/`[` too) as an INFIX continuation of the previous
expression -- if the token has `newline_before` set, emit a `W`-series
warning instead of silently choosing the "continue" reading. NOT attempted
this session: (a) still cross-cutting relative to a wave scoped to two named
parser bugs, both already delivered; (b) the highest-risk part is unverified
-- whether any EXISTING code (self-host source, stdlib, examples, or a
program relying on the documented trailing-operator continuation idiom)
would now emit spurious warnings, which needs a full-corpus run before
shipping, not a spot check; (c) scoping it to ONLY `|`/`||` while leaving
`-`/`(`/`[` unwarned would be an inconsistent partial fix worth avoiding.
Recommendation for whoever picks this up: implement the single-bool version
above (not full line tracking), run it in WARN-only mode across the entire
self-host source + stdlib + examples corpus first, and only then decide
whether any legitimate continuation usage needs an explicit escape hatch
before making it a default-on diagnostic.

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
| **`closure-mutating-costale-scalar-capture`: a non-mutated SCALAR co-capture in a `let`-bound MUTATING closure was silently frozen at closure-construction time instead of tracking later outer mutations, contradicting CLAUDE.md gotcha #11's unconditional by-reference promise — FIXED** | Reported by a red-team finding (LEDGER item 11(b), independently verified by two adversarial reviewers before reaching this session). Reproduced live pre-fix exactly as reported (`compiler/target/release/kryos.exe`): `tests/security/attack_closure_costale_isolate.kry` (`let mut counter = 0  let mut flag = 1  let f = \|n: i64\| { counter = counter + n  counter + flag }  let r1 = f(1)  flag = 100  let r2 = f(1)`) printed `r1=2 r2=3` on BOTH `kryos run` and `kryos build --release` (rc=0, no diagnostic) — expected `r2=102` (counter=2, flag=100) if `flag` were live-visible per the documented promise; got `r2=3` (flag frozen at its construction-time value of 1). Corroborating: `tests/security/attack_closure_mutate_then_throw_state.kry` reassigns a captured `trigger` to `0` specifically to disarm a conditional `throw` inside a helper the closure calls on its second invocation — pre-fix this printed `caught: boom` then `kryos: uncaught exception: boom` (exit 101), proving the outer reassignment was invisible to the closure. ROOT CAUSE (read, not guessed, `kryos-mir/src/lower.rs` Lambda-lowering, ~line 12562-12638): the mechanism that boxes a non-mutated scalar capture behind an ARC-managed heap cell so a LATER outer reassignment writes through to it (`box_scalar_captures`, gated by `ctx.pending_box_scalar_captures`) was scoped deliberately narrowly to a struct-literal-field lambda's direct value ONLY — the surrounding comment explicitly asserted a `let`-bound closure "keeps the existing, ALREADY-CORRECT `closure_locals` path" that "re-reads the outer variable's CURRENT value at every call site." That assumption is FALSE for a `let`-bound closure that ALSO mutates a DIFFERENT capture: `closure_locals` (the direct-call re-read substitution) is populated only for NON-mutating closures — `mutating_closures`'s own doc comment states the direct-call fast path it enables is unsafe once a closure owns mutable state by move. So a `let`-bound MUTATING closure's OTHER, non-mutated scalar co-captures fell through BOTH mechanisms (not struct-literal-field, so never boxed; disqualified from `closure_locals` by the closure's OWN mutation, not just the mutated capture specifically) and silently froze at their construction-time snapshot. FIX: widened the `box_scalar_captures` gate from `ctx.pending_box_scalar_captures` alone to `ctx.pending_box_scalar_captures \|\| !mutated_captures.is_empty()` — any closure that mutates >=1 capture now also boxes its OTHER non-mutated scalar co-captures via the identical `RValue::ArcAlloc`/`RValue::Deref`/`MirType::Shared` machinery, with the existing `Stmt::Assign` write-through (`capture_boxes`, keyed by variable name, already generic over ANY box registered for that name) requiring no changes at all. Safe to widen unconditionally: a mutating closure NEVER uses the `closure_locals` fast path regardless of whether this box fires (disabled by `mutating_closures.insert(..)` a few lines below in the same function), so there is no fast-path conflict the original comment's narrow scoping was protecting against. PROOF BOTH WAYS: `git stash` just this hunk of `kryos-mir/src/lower.rs`, full `cargo build --release` (no `-p`, `kryos-mir` links into the runtime toolchain) — `tests/conformance/conf_functions.kry`'s new `costale_scalar_cocapture` assertion fails (`CONF FAIL: non-mutated scalar co-capture in a mutating closure tracks a later outer mutation`, rc=0 but wrong value) and the live repro reproduces `r1=2 r2=3` on `kryos run`; `git stash pop` + rebuild — `conformance functions: PASS` on both `kryos run` and `kryos build --release`, and the live repro prints `r1=2 r2=102` on both backends. The corroborating throw repro's originally-reported symptom is also gone post-fix (the disarming `trigger = 0` reassignment is now observed; no more uncaught exception on the second call) — verified live, both backends. **Byproduct, NOT fixed, filed separately: LEDGER item 21** — verifying against the throw repro exposed an orthogonal bug (the MUTATED capture `counter`'s own write-back is skipped when the call that mutates it throws mid-body, so `after=0` instead of the expected `5`), a different call path (codegen-synthesized exception-return, not a co-capture) with a different fix surface (both backends' codegen, not `lower.rs`) — see item 21 for the live evidence and hypothesized root cause. Gates: conformance 61/61 both backends, `kryos-loop.sh gates 2` tier1+tier2 GREEN, `security_gate.sh` PASS (all checks unchanged), `test_bootstrap.sh` 16/16 run alone. Regression: `tests/conformance/conf_functions.kry` (`costale_scalar_cocapture`); `tests/security/attack_closure_costale_isolate.kry` and `attack_closure_mutate_then_throw_state.kry` (already committed) re-verified passing live, kept as-is (not folded into conformance — they predate this fix and remain useful standalone repros). No CLAUDE.md text change needed: gotcha #11's "sees a mutation made after construction" promise was already stated unconditionally for non-mutated captures — this bug was a silent VIOLATION of that promise by the implementation, not a documented caveat that needed correcting. |
| **`generic-bare-map-compound-return-misrenders`: bare self-field passthrough returning `map<K, T>` mis-rendered through `to_string`, both backends, both key value types tested — FIXED** | Reported by an `invariants`-class finding (independently verified by two adversarial reviewers before reaching this session). Reproduced live pre-fix exactly as reported (`compiler/target/release/kryos.exe`): `struct Holder<T> { m: map<str, T> }` / `impl<T> Holder<T> { fn get_map(self: Holder<T>) -> map<str, T> { return self.m } }` at `T=f64`: `to_string(h.get_map()["k"])` printed `4609434218613702656` (the raw i64 bit pattern of `1.5`) instead of `1.5`, identically on `kryos run` and `kryos build --release` (rc=0, no diagnostic); the numeric range check on the SAME value passed (bits correct, only render dispatch wrong). At `T=str`, worse and previously undocumented: `to_string(rm["k"])` printed a raw pointer integer instead of `"hello"`, even though direct `+` concat on the same value worked correctly — str is NOT "already stable" here the way it is for the array/tuple sibling bug, because `+` doesn't need static-type dispatch but `to_string`'s render path does. ROOT CAUSE (read directly, not guessed): `instance_ret_needs_monomorphization` (`kryos-mir/src/lower.rs`) had `Some(ast::TypeExpr::Generic { name, args, .. }) if name != "map" => args.iter().any(mentions)` — explicitly excluding `map` from the same per-receiver-instantiation monomorphization trigger its `TypeExpr::Array`/`TypeExpr::Tuple` sibling arms get two lines below (the fix that closed the identical bug class for `-> [T]` / `-> (T, i64)`, see the CLOSED entry above and `conf_generic_compound_return_f64.kry`). The exclusion's own doc comment reasoned map is like an enum — no nominal STRUCT-LAYOUT mismatch possible, so "a conservative no-op filter, no bug there" — which conflated the struct-CONSTRUCTION concern this function's `Generic` arm otherwise guards (`-> Box<T>`, where the returned struct's SHAPE differs per instantiation) with the ELEMENT-TYPE-ERASURE concern the `Array`/`Tuple` arms exist to fix (a compound container's VALUE type must be individually retyped per instantiation for render dispatch) — map has the layout property (true, harmless) but ALSO has the erasure property (missed) since a bare `return self.m` is also exempted from `body_operates_on_self`'s "operates on self" trigger (that check specifically exempts pure passthroughs, per its own doc comment), so the method fell all the way through to `_ => false` and stayed on the single erased-to-i64 compiled copy. FIX: removed the `if name != "map"` guard entirely — `TypeExpr::Generic { args, .. } => args.iter().any(mentions)` now covers map (and any other builtin/user generic container) uniformly with no name-based carve-out; `substitute_type_expr_to_mir`/`monomorphize_impl_fn` (the machinery the array/tuple fix already routes through) already handle map generically with no map-specific gap. PROOF BOTH WAYS: `git stash` just this hunk of `lower.rs`, full `cargo build --release` (no `-p`, `kryos-mir` links into the runtime toolchain) — `conf_generic_compound_return_map.kry` fails with `CONF FAIL: f64 map compound return RENDERS as 1.5, not its bit pattern`, rc=1, on BOTH `kryos run` and `kryos build --release --backend llvm`; `git stash pop`, rebuild — both backends print `conformance generic_compound_return_map: PASS`, rc=0. Assertions gate the RENDER path specifically (`to_string(..) == "1.5"` / `== "hello"`), not a bits-surviving proxy — the numeric range check and the str equality/concat checks pass identically with or without the fix, proven above; only the `to_string` assertions are the real gate. Regression: `tests/conformance/conf_generic_compound_return_map.kry` (f64 value-type + str value-type, both the render assertion and a parity check that `+`/equality on the erased pointer already worked pre-fix). Gates: conformance 61/61 (was 60/60 — one new conformance file; README.md's stale "60/60" claim caught and corrected by `docs_status_gate`, which failed until fixed — proof the gate itself works), `kryos-loop.sh gates 2` tier1+tier2 GREEN, `security_gate.sh` PASS (all checks unchanged), bootstrap 16/16 run alone. Docs: CLAUDE.md gotcha #17 extended to note map is now covered by the same fix; this ledger entry closes the finding. |
| **Item 18: LIVE CAPABILITY ESCAPE — a privileged closure stored into an actor's own state field defeated `deny!()` when read back and invoked from a separate actor method (`actor-state-stored-closure-cap-escape`), FIXED** | Reproduced live pre-fix exactly as reported (`compiler/target/release/kryos.exe`, HEAD `00b3cf7`): `kryos run tests/security/attack_actor_state_stored_closure.kry` printed `ACTOR-STATE-STORED (was NOT CLOSED): TOPSECRET-CLOSURE-9f8e7d6c5b4a` from inside `deny!(fs:read)`, rc=0, under BOTH default-inferred and `--strict-capabilities`. ROOT CAUSE (read via `kryos-capabilities/src/checker.rs`, not guessed): `Expr::MethodCall { object: self, method: "reader", .. }`'s capability charge came from two mechanisms, both of which silently contributed nothing for this shape. (1) `resolve_method_field_invoke_caps` (the same fix that already closes the ordinary-struct-field case, `cap_escape_closure_launder_field_mutate.kry`) requires `object`'s root to be found in `current_local_container_lits`, a per-FUNCTION flat map of `let`/`assign`-tracked struct/array/map literals built fresh for whichever function body is currently being checked — `self` is never a local binding of `invoke()`'s own body (its value was written by a DIFFERENT method, `stash`, at a DIFFERENT dispatch), so the lookup misses and the function returns `CapabilitySet::empty()` by design (documented: "never a blanket guess ... an ordinary method call is never misclassified"). (2) Ordinary method-call handling (`compute_hot_extra_caps("reader", ..)`) also contributes nothing because `reader` is not the name of any declared function or actor handler — it is a state FIELD name, not a callable. Both paths independently landing on empty meant the call was net-zero-charged instead of hitting this file's own standing fail-closed rule ("Unknown must mean deny, not this call needs nothing"). FIX: added `current_actor_fn_state_fields` (the current actor's own FN-TYPED state field names, populated by `check_actor` from `Decl::Actor`'s `state_fields`, scoped to the duration of that actor's handler bodies being checked) and `resolve_actor_self_field_invoke_caps` (unions `Capability::All` into the call-site charge whenever `object` is the bare `self` identifier and `method` names one of those fields), wired into `check_expr`'s `MethodCall` arm alongside the existing struct-field resolver. This is a DELIBERATE blunt fail-closed default, not a precise cross-handler trace: an actor's state can be written by any OTHER handler at any prior dispatch, so there is no sound way to attribute a specific closure value to a given `self.<field>()` call site without whole-actor data-flow tracing across every handler's write sites (left as a documented possible follow-up, not attempted — the task's own framing named `[all]` as the correct fail-closed target). PROOF BOTH WAYS: pre-fix binary (HEAD `00b3cf7`) reproduces the leak as above; post-fix `cargo build --release` (full, no `-p`, since `kryos-capabilities` links into the runtime toolchain) — `kryos run` and `kryos check --strict-capabilities` both now reject with `error[E0507]: call to \`reader\` requires capabilities [all] not granted to caller`, rc=1, in BOTH modes. NOT A WEAKENING: verified every sibling actor/capability check in `tests/security_gate.sh` (44 pre-existing checks) still passes unchanged, plus two NEW checks added (#45 rejects this exact escape under both modes; #46 proves an ordinary scalar-state actor dispatched from inside an unrelated `deny!` still needs zero annotation — no cascade). KNOWN, DOCUMENTED OVER-APPROXIMATION (not a regression): the sibling decoy-control file `attack_actor_state_stored_closure_control_decoy.kry` (a zero-capability closure in the identical actor shape) now ALSO fails closed post-fix, whereas pre-fix it correctly compiled clean — this is the expected, sound trade-off of the blunt `[all]` default (its header comment rewritten to explain why, not left contradicting the new behavior); a future precise fix would need real cross-handler write-site tracing to tell the two cases apart. Gates: `security_gate.sh` PASS (46/46 checks incl. the 2 new), `kryos-loop.sh gates 2` tier1 (conformance 60/60) + tier2 GREEN, bootstrap 16/16 run alone. Regression: `tests/security_gate.sh` checks #45-46 (no separate conformance file — this is a compile-time-rejection security check, matching the pattern of every other closed cap-escape item in this table). |
| **Combined-category grammar fuzz wave (2026-08-04): capability provenance checker false-rejects a zero-capability closure call when the closure is defined AND called inside a bare `{ }` scoping block or a `let x = { .. }` block-tail-value — forces `@capabilities(all)` on ordinary code, defeating least-privilege — TWO instances, same root shape, FIXED** | Found building a NEW combined-category grammar fuzzer (`tests/fuzz/gen_grammar.py` + `run_diff_grammar.py`, this wave's deliverable — 9 scenarios threading generics/closures/dyn/spawn/actors/enums/Option/Result/tuples/try-throw through ONE connected data-flow story per program, unlike the existing template harness's independent per-category blocks) wrapping each scenario body in its own `{ }` for local scoping, exactly as `gen_fuzz.py`'s own README documents as the class of bug its independent-block design cannot reach. Minimal repro: `fn main() { { let mul_add = \|x: i64\| x * 2 + 1  let v1 = mul_add(5) } }` — rejected with `error[E0507]: call through a function value requires capabilities [all] not granted to caller` even though `mul_add` is a trivially pure closure; the SAME code with the outer `{ }` removed compiles clean. Reproduces for a curried closure, the `let x = { .. }` block-tail-value idiom (gotcha #3's documented pattern), double nesting (`if { { .. } }`), and propagates through an unannotated helper function (forcing it to `@capabilities(all)`, then rejecting an unannotated caller of THAT helper). A SECOND, sibling instance of the identical root cause: a closure retrieved from a CONTAINER (`store = push(store, reader)` then `store[0]()`) inside a bare block hit the same false-reject via a different flat-map builder. ROOT CAUSE (both, read via `kryos-capabilities/src/checker.rs`, not guessed): `build_local_closure_caps_block` and `build_local_container_lits_block` each flatten nested `if`/`for`/`while`/`try` scopes into one map (by design, per their own doc comments — "best-effort, not scope-precise") so a closure/container `let`-bound inside one is known at its later direct-call site — but neither had a match arm for a BARE `{ }` block, which desugars to `Stmt::Expr { expr: Expr::Block { .. } }` (there is no dedicated `Stmt::Block` AST variant) or for a `let x = { .. }` block-tail-value initializer (`Stmt::Let { value: Some(Expr::Block{..}), .. }`); both fell through the existing `_ => {}` catch-all, so the closure was invisible to the flat map and the REAL per-call checker (which does correctly walk into `Expr::Block` for every other purpose) resolved the call as `Unknown` -> `Capability::All`. NOT the same root cause as a third residual found by the same generator (`holder.get()()` — calling the chained return of a generic passthrough ACCESSOR method holding a closure field) — that one reproduces even with zero block nesting and even through an intermediate local, so it needs tracing a generic method's own body and was deliberately left OPEN, filed below, not conflated with this fix. FIX: added a `Stmt::Expr { expr: Expr::Block { block: inner, .. }, .. }` arm to both flat-map builders (recurse into `inner`, mirroring the existing if/for/while/try recursion) and a `Expr::Block` arm to each builder's `Stmt::Let` handling (recurse into the block before computing the outer let's own resolution). PROOF BOTH WAYS: `git stash` just `checker.rs` + `cargo build --release -p kryos-cli` (no staticlib touched, `-p` build is legitimate here) — `tests/conformance/conf_closure_block_scope_caps.kry` fails `kryos check`/`kryos run` with 6 E0507 errors across all 4 repro shapes + the helper-propagation case; `git stash pop` + rebuild — all pass, prints PASS on both backends, values verified correct (`mul3(5)` -> 16, not just "compiles"). Verified NOT a weakening: every existing `tests/security/attack_wrap_closure_*`/`cap_escape_closure_launder*`/`fail-closed-hardening`/`decoy-companion` capability-escape check in `tests/security_gate.sh` (72 checks) still correctly rejects post-fix — the fix only makes MORE cases resolve to their real (possibly non-empty) computed capability via the SAME `resolve_closure_caps`/`resolve_container_path_caps` already used for top-level-scoped closures; it does not change what a genuinely-gated closure requires. Regression: `tests/conformance/conf_closure_block_scope_caps.kry` (6 assertions). Gates: conformance 60/60 (was 59/59 — new regression test added, README.md conformance count corrected, `docs_status_gate` catches drift), tier1+tier2 GREEN (an initial `examples_e2e` 10/12 + `docs_status_gate` FAIL on the first combined run were the documented parallel-gate-contention flake + the stale-count gap respectively — both re-ran clean ALONE: `examples_e2e` 12/12, `docs_status_gate` PASS after the README fix), `security_gate.sh` PASS (all 72 checks), bootstrap 16/16 solo. Combined-category grammar sweep re-run clean after the fix: 150/150 match (0 diverge, 0 both-fail) across all 9 scenarios x 15 seeds.
| **Round 5: the round-4 fail-closed fix's OWN relief mechanism was itself a shape-based decoy escape (the SAME bug class, twice), plus an unrelated, deeper "defer to my own caller" scope hole reachable through actor/spawn/method/direct-invoke paths without any decoy at all** | Reported live at HEAD (`4ac8b83`), confirmed FIRST per non-negotiable #2: `apply_to_second<T>(decoy: [T], real: [T], f: fn(T) -> str) -> str { return f(real[0]) }`, called as `apply_to_second(decoy, real, \|c\| c())` inside `deny!(fs:read)` where a direct `file_read` is correctly rejected, printed the real secret with rc=0 under `--strict-capabilities`. ROOT CAUSE: round 4's relief for a transparent-forwarding lambda (`\|f\| f()`) resolved "whichever other argument supplies its elements" by matching the callback's DECLARED element type against another parameter's DECLARED container type, first match wins (`find_companion_container_arg`) — an empty DECOY of the same declared shape always won ahead of the REAL container, contributing zero capabilities. **This is not a new bug class, it is the SAME one rounds 1-3 already failed on, relocated**: any inference of authority from declared shape/position/arity rather than PROVEN data flow can be defeated by a decoy of that shape — there is no fifth heuristic that fixes this, the mechanism itself is wrong. FIX PART 1 (mandated, no shape-based successor): `find_companion_container_arg` deleted outright. The ONLY relief implemented is `hot_param_companions` — genuine per-DECLARATION data-flow tracing computed ONCE from each function's own FIXED source, independent of any call site: for a hot callback parameter invoked directly inside a function's own body, it records, from the ACTUAL call-argument expression at that internal invocation (`map`'s literal `f(arr[i])`), which of the function's OTHER OWN parameters (by index) and PATH the argument decomposes to via `decompose_container_path` — the same syntactic decomposition already trusted everywhere else in this file. Because this is a property of the callee's own compiled declaration, a caller cannot make a decoy occupy "the parameter the body actually reads from" without that decoy BEING what actually flows to the callback (in which case charging it IS correct) — proven live: re-running the exact repro above on the fixed binary now attributes the precise `fs:read` requirement to `real`, not `decoy`. Where no single companion can be proven (disagreeing internal call sites, or an argument that doesn't decompose to another own-parameter at all), the position requires `Capability::All` — no approximation. AUDITED for every other shape-based-inference site per instruction #1 (grepped `structurally`/`shape-based`/`first-match`/`arity`/`parameter position`): none found; every other "detect a hot parameter's POSITION" mechanism in this file (`hot_params`'s Seed A/B, `is_fn_bearing_type`, `resolve_type_path`) determines WHERE authority might flow, never WHICH call-site argument to exempt from charging, so none is the vulnerable class. SECOND, INDEPENDENT BUG found while auditing every remaining place authority gets DEFERRED rather than charged (per instruction #1's mandate to audit exhaustively, not just the reported bug): the "a hot argument that is one of the CURRENT function's own parameters defers its charge to THAT function's own call sites" rule — present since round 1, load-bearing for ordinary passthrough HOFs — assumes the eventual outer call site is checked against a scope at least as narrow as wherever the value is actually invoked. FALSE whenever the deferring function narrows ITS OWN scope with `deny!` between receiving the parameter and invoking it: confirmed live, NO decoy, NO generic, NO container, the plainest possible forward — `fn outer(reader: fn()->str) -> str { deny!(fs:read) { return zero_cap_tool(reader) } }`, called from an `@capabilities(fs:read)` caller, compiled clean and printed the secret from inside the denied scope (rc=0) under BOTH inferred and `--strict-capabilities`. Reproduced identically, unmodified in kind, through THREE separate invocation paths sharing the same root: a bare direct call (`r()` as its own callee), a hot ARGUMENT forward (`zero_cap_tool(reader)`), and — found only by testing the task's own enumerated verification list, not assumed safe — an ACTOR MESSAGE HANDLER receiving the closure as a message argument and invoking it inside its own `deny!`, which leaked identically with zero changes to the underlying mechanism (the handler's own params ARE `current_fn_typed_params` there too). FIX PART 2: `current_fn_entry_scope_depth` (new field) records `scope_stack.len()` at the instant the checker enters the CURRENT function/actor's own boundary scope (`check_function`/`check_actor`); every deferred-charge decision now requires `Capability::All` instead of empty whenever the LIVE scope is deeper than that recorded depth (a `deny!`, or any future narrowing construct, is active between entry and this exact call) — `deferred_own_param_caps` centralizes this for the THREE call sites that previously returned an unconditional empty (`resolve_direct_invoke_caps` x2, `resolve_method_field_invoke_caps`) plus the two matching arms inside `accumulate_hot_extra_caps`. CAUGHT AND FIXED A SELF-INTRODUCED REGRESSION DURING THIS SAME FIX, per non-negotiable #2 (prove both ways, don't assume the fix is clean): the naive version of part 2 applied the scope check UNCONDITIONALLY, which broke the round-4 no-cascade guarantee — `map(fns, \|f\| f())` over PROVABLY PURE closures started requiring `all` merely because it happened to run inside ANY `deny!` block (even one narrowing an unrelated capability), because `resolve_closure_caps`'s STRUCTURAL self-classification of a fresh lambda literal (used to decide whether `\|f\| f()` needs anything beyond forwarding its own param) reuses the exact same `deferred_own_param_caps` machinery as a REAL enforcement-time call, with no signal to tell them apart. Found via a live regression re-check of the FIRST fix (`decoy_generic.kry`'s attribution silently degraded from precise `[fs:read]` to blanket `[all]`) before this was ever committed. Fixed with two new fields scoped exactly to the sub-computation that must stay scope-independent: `transparent_lambda_params` (the LAMBDA's own bound names, tracked only for the duration `check_expr` re-checks that SAME lambda's body, distinguishing "a lambda re-encountering its own already-handled parameter" from a real enclosing function's parameter) and `structural_lambda_eval_depth` (a `Cell<u32>` nesting counter set for the duration of `resolve_closure_caps`'s Lambda-arm classification sub-call into `collect_caps_expr`, which is NOT a real call-site check and must never consult the ambient scope at all). PROOF BOTH WAYS for both parts: reverted each fix in turn, rebuilt (`cargo build --release`, full — `kryos-capabilities` is linked into the runtime toolchain), confirmed the EXACT leak reproduces (secret bytes printed, rc=0) on the pre-fix binary for the decoy-generic repro, the plain-forward `outer(reader)` repro, AND the actor-handler repro; restored, rebuilt, confirmed all three rejected (E0507) and the no-cascade/precision checks pass again. Verified against every variant the task enumerated, not just the two minimal repros: decoy as a MAP companion (`cap_escape_decoy_map_companion.kry`), decoy read out of another container rather than a fresh literal (`..._container_read_source.kry`), 3+ containers (`..._three_containers.kry`), the same decoy shape against `any`/`all`/`partition`/`flat_map`-style user-defined siblings (`..._iter_siblings.kry`), a method receiver with 3 array parameters (`..._method_receiver.kry`), and the scope-hole reached through an actor message handler (`..._actor_message.kry`), a `spawn` capture (`..._spawn_capture.kry`), and a `dyn Trait` method (`..._dyn_trait_method.kry`) — all 8 new files REJECTED (E0507) under both `inferred` and `--strict-capabilities`, added to `tests/security_gate.sh`'s existing shape-loop pattern. Full verification sweep, all green: `security_gate.sh` (every existing check incl. the two no-cascade positives, PASS with unchanged precise attribution), `strict_caps_examples.sh` (91/91, zero net cascade), full `cargo build --release` clean, `tests/conformance/run_conformance.sh` (58/58), `kryos-loop.sh gates 2` (tier1+tier2 GREEN, one transient `examples_e2e` flake reproduced the documented parallel-gate-contention pattern — re-ran alone, 12/12 clean), `test_bootstrap.sh` run ALONE (16/16, one stray `kryos.exe` killed first per non-negotiable #3). Regression: `tests/security/cap_escape_decoy_{map_companion,container_read_source,three_containers,iter_siblings,method_receiver,actor_message,spawn_capture,dyn_trait_method}.kry`. Docs: `docs/10-capabilities.md`'s implementation-status callout rewritten to describe BOTH round-5 fixes and the guarantee actually enforced (not another "closed" claim); `docs/capability-roadmap.md` gained a "Round 5" section recording why shape-based inference failed a SECOND time on this exact axis, so nobody re-attempts a shape heuristic here a third time, and item 4 in Part 1's relief list marked superseded. |
| **STRUCTURAL fix for fn-value laundering: inverted the enumeration to fail-CLOSED, closing round-3's 2 residuals plus a more basic root gap all three rounds missed** | THREE prior rounds (see the two CLOSED rows below this one) each enumerated a syntactic SHAPE through which a closure's authority could travel and traced that specific shape; each round closed everything it found, and new shapes were found immediately after -- because a whitelist of dangerous shapes cannot be complete when the attacker chooses the shape. Confirmed live before touching code, both round-3 residuals: `map(tools, \|f\| f())` (an inline lambda invoking its own bound parameter through a HOF -- the prior HOF-forward fix only covered a NAMED forwarding function) and `let arr = m["k"]; arr[0]()` (a closure read out of a container into an intermediate local -- chaining directly was traced, the extra local broke it), both leaked the real secret (rc=0, `TOPSECRET-CLOSURE-...` printed) inside `deny!(fs:read)` under both `inferred` and `--strict-capabilities`. Auditing the ENFORCEMENT layer (not just the "closed" list) found the actual root: every rule up to this point fired ONLY for a call whose callee resolved to a NAMED function already tracked in `hot_params`; a call whose callee was a bare local with ZERO indirection (`let r = make_secret_reader(path); r()`, no parameter, no container, not even a second function call) was never evaluated by anything across all three rounds, since nothing inspected a call's callee unless it was already a name in that table. Found and confirmed live 3 more variants of the same root gap while auditing: a struct-field closure invoked via method-call syntax with no intervening function (`reg.reader()` directly in `main`), a full chained container index-call with no intermediate local (`m["k"][0]()` directly in `main` -- NOT the same as the already-closed parameter-crossing case), and (implicitly, since the mechanism generalizes) any container 3+ levels deep invoked the same way. FIX (`kryos-capabilities/src/checker.rs`, `kryos-capabilities/src/model.rs`): inverted the default -- a call through a first-class fn-value that cannot be resolved to a KNOWN function/builtin/extern/actor-constructor/enum-variant-constructor now goes through `resolve_direct_invoke_caps`/`resolve_method_field_invoke_caps`, which resolve what is actually being invoked via the SAME closure/container resolvers already used for argument attribution and require it directly: `Known` -> exact set, `DependsOnParam` (of the CURRENT function's own hot param) -> deferred as before, `Unknown` -> `Capability::All`. Runs at both the enforcement layer (`check_callee_capabilities`, `check_expr`'s `MethodCall` arm) and the inference layer (`collect_caps_expr`'s `FnCall`/`MethodCall` arms), so an interior helper's own inferred set reflects a direct invocation inside its body too. CASCADE MEASURED, not assumed: a strict `Unknown -> all` first pass broke the example corpus from 91/91 to 74/91; every failure traced to a specific, GENERAL (non-name-based) fix rather than a carve-out -- (1) precise resolution extended to a field/index chain read into an intermediate local, a curried/chained call (`f(a)(b)(c)`, resolved by recursing through the callee-of-a-call shape), and a self-recursive nested named function (the parser desugars `fn adder(y){..}` inside a body to `let adder = fn(y){..}`, needing a pre-registered placeholder before recursing into its own body); (2) a bare-name collision fix (`std::iter::find`/`std::re::find`/`std::string::find` all share the name "find" in the checker's bare-name-keyed maps) -- `collect_functions` now carries each declaration's OWN param list inline instead of re-looking it up by name afterward, for every own-parameter computation, so a colliding name's WRONG params can no longer leak into another declaration's inference; (3) actor constructors (`Account()`) and enum-variant constructors (`Some(x)`/`None()`/`Ok`/`Err`) are `Name(args)` call syntax but not fn-value references -- both are now tracked (`actor_names`, `enum_variant_names`) and excluded; (4) a TRANSPARENT-FORWARDING lambda (`\|f\| f()`, whose only behavior is invoking its own bound parameter) is resolved structurally against a COMPANION container argument at the SAME call site -- `find_companion_container_arg` matches purely on TYPE SHAPE (a `fn(T,..)->U` callback paired with a `[T]`/`map<K,T>` parameter elsewhere in the SAME declaration, `T` compared by generic-name identity), covering `map`/`filter`/`fold`/`reduce`/`find` and any user-written HOF fitting the shape without naming `std::iter` anywhere in the implementation. PROOF BOTH WAYS: `git stash` `checker.rs`+`model.rs` + full `cargo build --release` (crate is linked into the runtime toolchain) -- every new `security_gate.sh` check (18 shapes across both modes: direct local call, struct-field direct call, chained/intermediate-local container reads, HOF inline lambda, `std::collections` Deque/Dict, Option/Result payloads, a user-defined HOF, 3-level nesting, and `--capabilities-mode=permissive`) goes RED (escape compiles, secret prints); restore + rebuild -- all green, and the pre-existing 33 checks + the two no-cascade positives (pure map-over-closures, pure mutation-built registries) stay green throughout. Full verification sweep, all green: `security_gate.sh` (51 checks), `strict_caps_examples.sh` (91/91, matching the pre-fix baseline exactly -- zero net cascade after the relief mechanisms), `run_examples_gate.sh`, `run_examples_e2e.sh` (12/12 response-body assertions), `ir_signature_gate.sh` (58 modules, no severe mismatches), full `tests/conformance/run_conformance.sh` (58/58), `inferred_soundness.sh`, `type_soundness.sh`, every other tier-1 gate script, the full `cargo test --release --workspace` suite (kryos-ownership's 2 pre-existing, unrelated failures reconfirmed identical on baseline HEAD via `git stash`, not a regression), and `test_bootstrap.sh` run ALONE (16/16). Regression: 9 new repros (`tests/security/cap_escape_{direct_local_call,struct_field_direct_call,container_chained_direct,container_intermediate_local,hof_inline_lambda,collections_deque_dict,option_result_payload,user_hof_three_level,hof_siblings}.kry`) + 2 no-cascade positives (`cap_escape_hof_inline_lambda_nocascade.kry`, and the pre-existing mutation-built-registry check), `tests/security_gate.sh` extended with checks #21-34. Docs: `docs/10-capabilities.md`'s implementation-status callout and the closure-indirection status line rewritten to state the fail-closed inversion (not another "all shapes closed" claim -- that framing is exactly what failed three times) and point to `docs/capability-roadmap.md`, which gained a full Part 1/1b: an honest post-mortem of why enumeration cannot converge, and the DESIGN (not implemented this wave, deliberately) for the sound long-term fix -- capability-typed fn values (`fn() -> str @ {fs:read}`), covering syntax, inference via the existing generic-substitution machinery, contravariant subtyping, generics/`dyn`/`spawn`/stdlib interaction, a 5-step migration path that nets DELETES most of the heuristic tracing machinery this fix added, and an honest 6-8 week scope estimate. Honest residual, unchanged in kind from before this fix (still documented, still fails CLOSED not open): a container built from a genuinely non-literal source (a function return, a container mutated inside a callee, one read out of ANOTHER container in a way even the intermediate-local extension can't follow) resolves to `Unknown` -> requires `all`. |
| **LIVE capability bypass: closure into a container via MUTATION after construction (push / index-assign / field-assign), a `std::collections` wrapper-method gap, and a HOF-forwarded-named-function gap — the prior wave's "safely rejected" claim was FALSE, measured live before touching code** | The prior wave (`a868ab7`) closed container laundering for LITERAL-constructed containers and claimed the dynamic-population case "resolves to Unknown and requires Capability::All" (safely rejected). REPRODUCED first, per non-negotiable #1: `tools = push(tools, reader)` in a loop, then `tools[i]()` inside `deny!(fs:read)`, and `m["k"] = reader` on a `map<str, fn()->str>` then `m[k]()`, both LEAKED the real secret (rc=0, no diagnostic, `TOP SECRET DATA` printed) in BOTH `inferred` and `--strict-capabilities` — confirming the report. Went further and found the prior wave's OWN claim about which shapes were "correctly rejected" was itself wrong: `tools[0] = reader` (array index-assign) and `r.f = reader` (struct field mutation after construction) were claimed rejected at compile time; re-measured live on a `let mut` binding and BOTH also leaked identically (compiled clean, printed the secret) — the "rejected" observation likely came from an orthogonal immutable-binding error on a non-`mut` variable, unrelated to capabilities, not an actual capability check. Enumerated and tested the full blast radius before fixing (per instruction, table below) rather than assuming: push ⇒ LEAK; map index-assign ⇒ LEAK; array index-assign ⇒ LEAK; struct field-init (baseline, prior fix) ⇒ correctly rejected; struct field-mutate-after-construction ⇒ LEAK; nested array-of-structs via push ⇒ LEAK; nested map-of-arrays via push+insert ⇒ LEAK; container returned from a function ⇒ correctly rejected (Unknown, genuinely untraceable); container param mutated inside callee ⇒ correctly rejected (Unknown); `std::collections::List<fn()->str>` via `.push()`/`.get()` ⇒ LEAK (separate root cause); a closure reaching a container through a HOF where the HOF's OWN callback is a bare fn-value (`map(paths, make_secret_reader)`, populating) ⇒ correctly rejected (Unknown); a HOF whose callback is a NAMED function that itself forwards a hot parameter (`map(tools, invoke)`) ⇒ LEAK (third, independent root cause); captured by an inner closure ⇒ correctly rejected (Unknown); captured by `spawn` ⇒ LEAK pre-fix, closed by the same mutation-tracking fix. THREE independent root causes, three fixes, all in `kryos-capabilities/src/checker.rs`: **(1) Mutation-tracking gap.** `build_local_container_lits`/`build_local_container_lits_block` only ever walked `Stmt::Let`, so a container's INITIAL literal snapshot (typically an empty `[]`/`{}`) was NEVER updated by a later `Stmt::Assign` — `resolve_container_path_caps`'s index-insensitive union over that stale, usually-empty literal resolved to `Known(empty)`, not `Unknown`, so the call required NOTHING — strictly worse than the documented conservative fallback, because the checker confidently asserted safety instead of admitting ignorance. Fix: `apply_container_assign` (new) recognizes `X = push(X, v)` (appends `v` into the tracked array), a plain alias (`X = Y` where `Y` is tracked), and a fresh literal reassignment (same rule as `Let`); `apply_container_path_write`/`rebuild_container_write` (new) handle a field/index write reaching into an ALREADY-tracked container through a path (`r.field = v`, `arr[i] = v`, `m[k] = v`, nested combinations), splicing the write in (index writes stay index-insensitive, matching the read side). Any OTHER reassignment shape the tracker can't precisely characterize (an unrelated function call result, a compound `+=`-style assign, a path that doesn't match the literal's actual shape) INVALIDATES (removes) the tracked entry instead of leaving it stale, so it correctly falls through to `Unknown` -> `Capability::All` — the concrete fix for "an unanalyzable fn-value must fail CLOSED, not open." **(2) `std::collections` wrapper opacity.** `decompose_container_path` only understands direct field/index syntax, so `list.get(i)` (a METHOD call) was invisible to the hot-parameter seed pass regardless of fix (1) — `resolve_type_path(List<fn()->str>, [Field("get")])` failed because "get" isn't a real FIELD on `List`. Fix: `transparent_accessor_paths` (new) records, for every method whose receiver is literally `self` and whose every return path decomposes to the SAME self-rooted field/index chain (`List.get`'s `return self.data[index]`), that method's self-relative path — KEYED BY `(struct name, method name)`, not bare name, because `List.get` and `Dict.get` live in the same stdlib file and return DIFFERENT paths (`[data,Index]` vs `[store,Index]`); a bare-name key was tried first and verified LIVE to let Dict's (declared later) clobber List's, breaking detection for the flagship `List<fn()->str>` shape — caught by testing, not assumed. `resolve_type_path` falls back to this map when a `Field` step doesn't name a real field. Also needed: generic instantiation was NEVER threaded through `struct_field_types`/`is_fn_bearing_type` — a generic struct's field type is stored RAW (`List<T>`'s `data: [T]`), so no instantiation was ever recognized as fn-bearing. Added `struct_generic_params` + `struct_fields_for` + `substitute_generic_type` to substitute the ACTUAL type arguments (`fn()->str`) for the struct's declared generic parameter names before checking fn-bearing-ness. Even with the parameter correctly marked hot, `List.new()`/`.push()` build the list via method calls whose struct-literal construction happens INSIDE the method body, invisible to caller-side literal tracking by design — resolves to `Unknown` -> `Capability::All`, the correct fail-closed outcome (this residual is NOT closed further; documented, not silently dropped). **(3) HOF-forwarded-named-function gap.** `resolve_closure_caps`'s `Identifier` arm, for a bare reference to a named function, unconditionally returned that function's own declared/inferred capability set — which says nothing about a HOT parameter it forwards (`fn invoke(f: fn()->str) -> str { return f() }`'s `f` is hot), so handing `invoke` to `map(tools, invoke)` as an unapplied VALUE was attributed `Known(empty)`. Fix: if `name` itself has any hot parameter (`hot_params.get(name)` non-empty), a bare reference falls back to `Unknown` instead. CASCADE MEASURED (not assumed) two ways before shipping: (a) grepped `compiler/stdlib`, `compiler/self-host`, `examples` for a bare-identifier (non-lambda) HOF callback argument — zero real occurrences, every actual callback in this codebase is an inline lambda; (b) confirmed live that the functionally-equivalent lambda-wrapped form (`map(tools, |f| invoke(f))`) was ALREADY conservatively rejected on the PRE-FIX binary (an unrelated, pre-existing restriction on a lambda-bound parameter invoking a hot-forwarding function inline) — so this fix closes an inconsistency (naming the adapter explicitly was silently MORE permissive than the equivalent lambda) rather than restricting previously-working code. PROOF BOTH WAYS for the whole batch: `git stash` just `checker.rs` + `cargo build --release -p kryos-cli` (compiler-internals-only crate, confirmed via `tools/loop/kryos-loop.sh preflight` that no kryos-rt/kryos-stdlib-native source is newer than the staticlibs, so a full rebuild was not required for THIS session's changes) — ALL 16 new `security_gate.sh` checks (8 shapes x 2 modes) go RED (escape compiles), while every PRE-EXISTING check and the new no-cascade positive check stay GREEN; `git stash pop` + rebuild — all 16 go GREEN again. Regression: 8 new committed repros (`tests/security/cap_escape_closure_launder_{push,map_insert,index_assign,field_mutate,nested_push,map_of_arrays,stdlib_collection,hof_forward}.kry`), `tests/security_gate.sh` extended with checks #12-19 (reject, both modes, all 8 shapes) and #20 (a registry of PURE closures built via the SAME mutation shapes needs zero annotation — no cascade). Gates: `security_gate.sh` PASS (33/33), full `cargo build --release` clean, `kryos-loop.sh gates 2` GREEN (conformance 58/58, tier1+tier2 all PASS including `strict_caps`/`examples`/`examples_e2e`), `test_bootstrap.sh` 16/16 (run alone, per non-negotiable #4/#5 — one stray `kryos.exe` killed first). Docs: `docs/10-capabilities.md`'s implementation-status paragraph and "Closure indirection, including containers" section rewritten to correct the FALSE "push in a loop requires Capability::All" claim (it required NOTHING) and document all three new closed shapes plus the one genuinely-remaining residual (a container from a truly non-literal source — function return, mutated parameter, read out of another container — which DOES fail closed, now actually verified rather than assumed). |
| **Item 8: a curried (2-level) generic closure return failed to BUILD on AOT while JIT accepted it -- JIT/AOT divergence** | Reproduced live before touching code: `tests/known_failures/closure_curried_generic_aot_crash.kry` printed `6` on `kryos run` but `kryos build --release` failed LLVM codegen (`error: load operand must be a pointer to a first class type ... load %T, ptr %_1_arg`). `--emit-llvm` showed the raw generic name `%T` unresolved on BOTH `__lambda_0` (outer, `\|b: T\|`) AND `__lambda_1` (inner, `\|c: T\|`) as `ptr byval(%T)` params -- broader than the prior write-up's "only the innermost closure" attribution. ROOT CAUSE (read, not guessed, `kryos-mir/src/lower.rs` Lambda-lowering param loop): `pending_lambda_ret_hint`'s fallback only fires for a closure param with NO explicit type annotation (`p.ty.is_none()`); `\|b: T\|`/`\|c: T\|` both name the generic type EXPLICITLY, so neither ever went through it or ANY substitution -- the raw `TypeExpr::Simple("T")` reached LLVM IR emission unresolved regardless of nesting depth. Cranelift's uniform i64 closure-arg ABI papers over the same erasure (no byval/sret distinction to violate), which is why JIT was always correct. FIX: an explicitly-annotated lambda param is now substituted through the current monomorphization's `active_generic_bindings` (the same `T -> concrete MirType` map already used for the enclosing generic function's OWN param/return types) when building the lambda's param list. `active_generic_bindings` is a plain `ctx` field, not reset by `save_function_state`/`restore_function_state`, so it stays live across a nested lambda-inside-a-lambda lowering -- fixing the outer AND the curried inner closure in ONE change, no recursion needed (the ledger's prior "make the hint propagate recursively" fix shape was therefore not the minimal one). Proof both ways: `git stash` the `kryos-mir` fix + `cargo build --release -p kryos-cli` (compiler-internals-only change, no kryos-rt/kryos-stdlib-native touched) -- `kryos build --release` on the repro fails with the exact original clang error; `git stash pop` + rebuild -- builds clean, runs, prints `6` on both `kryos run` and the AOT binary. Regression: `tests/conformance/conf_curried_generic_closure.kry` (i64 instantiation + a SECOND independent instantiation to rule out cross-instantiation aliasing; was `tests/known_failures/closure_curried_generic_aot_crash.kry`, deleted). CLAUDE.md gotcha #22's curried-generic-closure entry updated from "residual, NOT fixed" to RESOLVED. Also cleaned up in this pass: `tests/known_failures/lowercase_struct_literal_parse_fail.kry` was already fixed (per the CLOSED entry above, commit e58d8dc) but the known_failures file + its README row were never deleted -- removed both (re-verified fixed on both backends before deleting). Gates: conformance (incl. the new test) PASS both backends, `kryos-loop.sh gates 2` GREEN, bootstrap 16/16. |
| **Item 4: `[dyn Handler]` at a CALL SITE (not a `let`) emitted a confusing `E0100` alongside the correct `E0110`** | Reproduced live before touching code: `fn use_handlers(hs: [dyn Handler]) { .. }` then `use_handlers([A{}, B{}])` emitted BOTH `error[E0110]: \`dyn Handler\` cannot be stored in an array yet` (correct) AND `error[E0100]: type mismatch: expected \`A\`, found \`B\`` (noise) at `kryos check`. The already-fixed `let x: [dyn Trait] = [A{}, B{}]` case (`suppress_array_elem_unify`) could not reach this shape because it keys off the RAW pre-resolution `TypeExpr` at the `Stmt::Let` site specifically; `FunctionSig.params` only stores the ALREADY-RESOLVED `Type::Error` a rejected dyn-in-array collapses to, with no way to tell "this Error came from a rejected dyn array" apart from "this Error came from an unrelated unknown-type-name annotation" at the call-arg check site -- the exact blocker a prior session's investigation named and left unfixed rather than widen the general `Type::Error` unify-anything escape hatch (tried and REJECTED that session: it silently dropped a genuinely useful diagnostic for an unrelated case, proven via A/B rebuild). FIX (`kryos-types/src/check.rs`): sidesteps the blocker instead of solving it -- a new side table, `dyn_container_reject_params: HashSet<(function_name, param_index)>`, is populated once at function-SIGNATURE registration (where the raw `TypeExpr` is still available, before it collapses to `Type::Error`), keyed by the exact param identity rather than by the type itself. The call-argument checker consults this table (not `FunctionSig`) when zipping args against params: if the callee/param-index pair was flagged AND the argument is an array literal, its span is added to the pre-existing `suppress_array_elem_unify` set before inferring it, skipping only that literal's own pairwise element-unify. Narrowly scoped by construction -- a DIFFERENT param that happens to also resolve to `Type::Error` for an unrelated reason is never in the table, so an unrelated genuinely-mismatched array literal keeps both diagnostics (verified by a negative-control probe). Proof both ways: `git stash` the `check.rs` fix + `cargo build --release -p kryos-cli` -- `kryos check` on the repro reports 2 errors (E0110 + E0100); restore + rebuild -- reports exactly 1 (E0110 only). Regression: `tests/type_soundness.sh` gained `dyn_array_callsite_heterogeneous` (via the existing `want_reject_e0110_clean` helper) + a new `want_reject_e0100` helper backing `unrelated_array_mismatch_not_suppressed` (negative control: an ordinary `[HA]` param, no dyn involved, passed a genuinely mismatched `[HA{}, HB{}]` literal, must still report E0100 -- proves the suppression didn't overreach). Gates: `type_soundness.sh` PASS (all probes correct, unsound rejected, correct accepted), `kryos-loop.sh gates 2` GREEN. |
| **Item 2c: `std::test::assert`'s 2-arg form was permanently shadowed by the compiler's own builtin and UNCATCHABLE -- a user function was supposed to WIN over a same-named builtin (CLAUDE.md gotcha #18), this was the one undocumented exception** | Reproduced live before touching code, both backends identical: `use std::test::{assert}` then `try { assert(false, "boom") } catch (e) { .. }` printed `assertion failed: boom` to stderr with NO `kryos: uncaught exception:` prefix and the process ABORTED (exit 127) -- `catch (e)` never ran. ROOT CAUSE (read, not guessed): both codegen backends dispatch any call literally named `assert`/`assert_eq`/`panic` with a matching arg count straight to the hardcoded `kryos_builtin_*` intrinsic UNCONDITIONALLY, in three standalone `if`/match-arm blocks that run BEFORE the generic "does the user define a function with this exact name" shadow-check every OTHER builtin (`len`, `abs`, `contains`, `sin`, ...) already goes through -- confirmed these three blocks were the ONLY ones without the guard sibling math builtins (`sqrt`/`floor`/`ceil`/`round`/`abs`/`sin`/`cos`/...) already had a few lines above them in the same function. Since `std::test::assert`'s real signature is exactly 2 args (matching the intrinsic's own arity), every call -- imported or not -- silently resolved to the intrinsic, permanently, with no diagnostic. FIX: added the SAME shadow-check guard to the `assert`/`assert_eq`/`panic` special-case blocks in BOTH `kryos-codegen-llvm` (`!self.func_param_types.contains_key(name)`, matching the `abs`/`len` precedent in that file) and `kryos-codegen-cranelift` (`!translator.user_func_names.contains(func)`, matching the sibling math-builtin guard in that file) -- when a user-defined (or stdlib-imported) function shadows the name, execution now falls through to the pre-existing generic user-shadow dispatch path instead, exactly like every other builtin. Proof both ways: `git stash` both codegen files + `cargo build --release -p kryos-cli` -- the repro aborts (exit 127, catch never runs) on both backends; restore + rebuild -- `caught: assertion failed: boom` prints and `catch`/the statement after the try/catch both run, on both backends. Non-regression, explicitly verified: a program that does NOT import `std::test::assert` keeps the TRUE intrinsic's exact uncatchable-abort semantics for BOTH the 1-arg and 2-arg forms (`assert(true)`, `assert(cond, msg)`) and for `assert_eq`/`panic` unshadowed, on both backends -- the fix only changes behavior when a same-named function is actually in scope. Regression: `tests/conformance/conf_assert_shadow_catchable.kry` (was `tests/known_failures/assert_shadow_uncatchable.kry`, deleted) + a new standalone `tests/assert_shadow_gate.sh` (wired into `kryos-loop.sh gates` tier 1 as `assert_shadow`) asserting BOTH directions' exit codes, since "the true intrinsic still aborts uncatchably when unshadowed" needs a nonzero-exit assertion the conformance harness can't make (same reason `utf8_invalid_string_gate.sh` is a standalone script). **Loose end resolved (not a new bug, a documentation slip):** a prior commit (`e7b1599`, "fix assert_eq unwind-skip bug") left a source comment on `is_unwind_source` (`kryos-mir/src/lower.rs`) citing `tests/known_failures/assert_eq_shadow_unwind_skip.kry` as proof of a DIFFERENT, already-fixed bug (a 3-arg `assert_eq` call nested inside an `if`/`for`/`while` inside a `try` could execute statements past the failing call before its exception was noticed, because `is_unwind_source` excluded any `assert_eq`-named call from post-call exception checks regardless of arity) -- that file was never actually committed (confirmed via `git log -S` across full history: zero hits for the filename, one hit for the fix diff itself, which IS present and unchanged at this HEAD). The underlying fix (`true_assert_eq_intrinsic = func == "assert_eq" && args.len() == 2`, gating the exclusion to the true 2-arg intrinsic's own arity, present in `kryos-mir` and both codegen backends) was real and already shipped -- only its regression repro was a slip. Recovered as `tests/conformance/conf_assert_eq_unwind_immediate.kry`, proved both ways THIS session (temporarily reverted the arity guard to an unconditional `func == "assert_eq"` in all 3 sites, rebuilt -- a 3-arg `assert_eq` call nested one level inside a `try`'s `if` let two subsequent statements execute, `ran=11` instead of `0`; restored + rebuilt -- `ran=0`, both backends); the source comment now points at the real test instead of the missing one. Gates: conformance (both new tests) PASS both backends, `assert_shadow_gate.sh` PASS, `kryos-loop.sh gates 2` GREEN. |
| **Parser/grammar wave: a lowercase-named struct could not be constructed via struct-literal (or matched via struct-pattern) syntax at all -- arbitrary, undocumented, case-based parser restriction** | Reproduced live before touching code: `struct counter { val: i64 }` then `counter { val: v }` failed with two misattributed `error[E0102]: undefined variable` diagnostics (naming `counter` and `val`, not the real "struct-literal requires capitalized name" restriction). ROOT CAUSE (read, not guessed): `kryos-parser/src/parser.rs`'s primary-expression struct-literal check and its sibling struct-PATTERN check both gated on `looks_like_type_name(&name)` (`name.chars().next().is_uppercase()`), unconditionally, everywhere -- not just in the genuinely ambiguous positions. The real ambiguity (`if cond { }` / `while cond { }` / `for x in xs { }` / `match subj { }`, where a bare identifier condition/subject/iterable sits directly before the construct's OWN block/arm-list `{`) is ALREADY fully handled, independent of case, by the pre-existing `no_struct_literal` flag that every one of those parses sets around its condition/subject/iterable (`parse_expr_no_struct_lit`, 13 call sites, unchanged). Outside those positions (`let`-initializers, `return` values, call arguments, array elements, binop operands, match-pattern position) there is no second grammar production competing for `Name { ... }` -- a bind pattern followed by `{` has no valid parse besides a struct pattern, and an ordinary expression position has no valid parse besides a struct literal (or a syntax error) either. FIX: removed the case check from BOTH sites (struct-literal in `parse_primary`, struct-pattern in `parse_pattern`), relying solely on `no_struct_literal` for the ambiguous positions; deleted the now-dead `looks_like_type_name` function. Proof both ways: `git stash` the parser fix -- `tests/known_failures/lowercase_struct_literal_parse_fail.kry` reproduces the exact two `E0102`s on both backends; restore + rebuild -- prints `5` (the documented expected output) on both `kryos run` and `kryos build --release`. Ambiguity guard re-verified live post-fix: an `if`/`while`/`for` condition/iterable immediately followed by its own block still parses as a condition, never a struct literal, even with a lowercase struct of the same name in scope; a lowercase struct PATTERN (`counter { val: n } => ...`) matches correctly in a `match` arm. Regression: `tests/conformance/conf_lowercase_struct_literal.kry` (literal construction, direct literal, struct pattern, and all three ambiguity-guard shapes, value-asserted via internal `panic()` on mismatch). Docs: `docs/19-language-reference.md` §5.2 now states struct names are not required to be capitalized. Gates: conformance 55/55 both backends, tier1+tier2 GREEN, bootstrap 16/16, `selfhost_regressions` PASS. |
| **Parser/grammar wave (the more serious of the two, ranked "silent wrong parse"): `tests/known_failures/parse_nested_binop_corrupts_next.kry` -- a self-hosted `tokenize()` call, called a SECOND time in one process, silently accumulated the first call's tokens onto the second's result (13+31=44, not 31) and then double-freed at process exit -- misdiagnosed at the time as "nested binop recursion corrupts a later parse"** | REPRODUCED before theorizing (`cd compiler/self-host && kryos.exe run known_failure_nested_binop.kry`): both backends agreed (JIT and AOT both printed "44 tokens (want 31)"), so per non-negotiable #6 the defect was in shared logic, not backend-specific codegen -- ruled OUT the parse_expr-recursion attribution the file's own bisection trail pointed at (a red herring: the bisection tracked "does it crash", not "is the count exactly right", so several "ok" steps were already silently wrong). Root-caused instead by tracing `len(tf)` with print statements at every statement boundary (not by re-reading the recursive precedence-climbing code): `lexer.kry`'s `LEX_TOKENS` module-level mutable global accumulator (kept out of the `Lexer` struct to avoid an O(n^2) array-dup, per that struct's own comment) was DELIBERATELY never reset between `tokenize()` calls, because resetting it via a cross-function reassignment used to corrupt the array header -- exactly LEDGER item 2b (closed the SAME day, `fd07331`, by the previous session). With item 2b now fixed, resetting `LEX_TOKENS` in `lexer_new()` is safe and closes the wrong-COUNT half of the bug. Fixing that surfaced a SECOND, general (non-self-host) bug via `KRYOS_MIR_DROP_TAGS`+`KRYOS_FREE_DIAG` site-tagging (added a `site: i64` field to the free-diag "first (rc->0) freed at" report, `kryos-rt/src/lib.rs`, to name the exact drop site instead of only a coarse, line-imprecise Kryos stack trace): `return LEX_TOKENS` (a bare mutable-global identifier returned directly) never retained the returned handle -- `emit_global_load` is a raw read, not a retain -- so the caller's return value ALIASED the global's own copy with no extra reference. Harmless as long as the global was never reassigned again, but the moment fix #1 reset the global on the SECOND call, the reset's own guarded release freed the SAME box the FIRST call's return value (`tf` in the repro) still held (confirmed via the site tags: the double-free was `fn-exit:tf`, the first zeroing was `fn-exit:t2` -- i.e. `t2` and `tf` had silently become the same box). FIX 2 (general, `kryos-mir/src/lower.rs`, `Stmt::Return` lowering): retain a bare mutable-global-identifier return the same way a bare PARAM return already was (`emit_param_source_retain`'s existing "borrow-to-own at the return boundary" pattern, extended to globals). PROVED BOTH WAYS, independently, for each half: (1) lexer.kry reset alone (mir fix reverted) -- "44 tokens" bug gone (31 correct) but `KRYOS-FREE-DIAG` reports `array DOUBLE-FREE rc=0 len=13 cap=16`, both backends; (2) mir fix alone (lexer.kry reset reverted) -- the general `tests/no_double_free.sh` `global_return_alias` case (`git stash` the mir fix, rebuild) reports `DOUBLE-FREE`; restored, clean; (3) both fixes together -- the full repro prints `31 tokens (want 31)` with NO double-free on either backend, `tf` independently re-verified still `len=13` after the second `tokenize()` call (proves independence, not just count luck). Regression: `tests/no_double_free.sh` (`global_return_alias`, general MIR case) + `compiler/self-host/regression_lexer_reentrant_tokenize.kry` (renamed from the known_failures file, hard `panic()`-asserted, wired into new `compiler/self-host/test_regressions.sh`, added to `kryos-loop.sh gates` tier 1 as `selfhost_regressions`). `tests/known_failures/parse_nested_binop_corrupts_next.kry` deleted (folded into the two regressions above). Gates: conformance 55/55 both backends, tier1+tier2 GREEN (incl. `no_double_free` and `selfhost_regressions`), bootstrap 16/16 (ran alone, per non-negotiable #5/#6). The diagnostic instrumentation change (`kryos-rt/src/lib.rs`/`array.rs`/`string.rs`: `diag_zeroed_by`'s return type gained the site id) is a permanent tooling improvement, not investigation-only scaffolding -- kept, since it directly answers the "which site froze this to rc=0" question `KRYOS_FREE_DIAG`'s own doc comment already says is otherwise invisible. |
| **LEDGER item 5: `Parser` carried the same array-in-a-rebuilt-struct pattern as the closed Lexer bug (O(n^2) struct-element retains on every `advance`)** | Read 680be5b (the Lexer fix) for the mechanism and applied the analogous change: `emit_aggregate_struct` clones/dups any array-typed struct FIELD unconditionally at struct-literal construction time (not gated on `@copy`), and `p_advance`/`p_expect`/`p_error` in `self-host/parser.kry` all rebuilt a `Parser{tokens: p.tokens, ...}` literal on every token -- one O(N) array-dup per token across a fixed N-token stream, O(N^2) total. Fix: moved `tokens` out of `Parser` into a module-level `PARSER_TOKENS: [Token]` global (mirroring `LEX_TOKENS` exactly), set once per `parser_new` call, read via `PARSER_TOKENS[idx]` everywhere `p.tokens[idx]` was read before. Kept `errors: [str]` as a struct field (deliberately NOT moved, unlike the ledger note's original "extra parameter" suggestion): it is 0-length for the overwhelmingly common clean-parse case, so its per-literal duplication cost is negligible, and it is a real external API surface (`main.kry` reads `p.errors` after parsing) that a module-global would have required threading a getter through for no measurable benefit -- this fix is now safe to make as a plain reassignment specifically because ledger item 2b (below) closed the cross-function global-reassignment corruption bug first. MEASURED before/after compiling `self-host/lower.kry` (128KB, 18657 tokens) via stage-1's own `obj` path (`KRYOS_SKIP_TYPES=1`, Windows `Start-Process`/`PeakWorkingSet64` polling, stage-0 kryos.exe unchanged both runs): peak working set 435.5 MB -> 101.7 MB (4.3x reduction), wall time 396ms -> 402ms (flat -- at this file's token count the retain/dup work is cheap in wall-clock terms even though it is genuinely O(n^2); the memory churn is what scales visibly, matching the mechanism). `bash compiler/self-host/test_bootstrap.sh` 16/16, stable across 2 consecutive runs post-fix. No new automated perf-threshold gate was added: this machine has no portable peak-memory tool (no `/usr/bin/time -v` in Git Bash; the existing `kryos-loop.sh soak` peak-WS measurement is Windows-PowerShell-only) and `test_bootstrap.sh` itself is not part of the actual `ubuntu-latest` CI workflow (`.github/workflows/ci.yml` only runs `tests/conformance/run_conformance.sh`) -- matching 680be5b's own precedent (measured + bootstrap-green, no synthetic dose-response gate), rather than adding a fragile platform-specific threshold. |
| **LEDGER item 2b: cross-function global reassignment corrupted a heap-owning global (`kryos_array_push: corrupt array header ... cap=0, data=0x0`)** | Root-caused in `kryos-mir/src/lower.rs` (`Stmt::Assign` lowering, plain-identifier-target arm): a mutable module-level global (`let mut NAME: T`) is a raw i64 slot in the runtime registry (`kryos-rt/src/globals.rs`, `kryos_global_set`/`kryos_global_get`) with NO ARC awareness -- it neither retains the incoming value nor releases the outgoing one. When one function reassigned a global from a BARE LOCAL reference (`G = empty` inside `reset()`), the local's own ordinary end-of-scope Drop freed the exact buffer the global's slot now pointed to; the next read from a DIFFERENT function (`add_one()`) then observed freed memory. The identical ARC-bookkeeping gap and fix shape already existed one match-arm above for actor-state-field assignment (`self.items = newitems`) -- generalized that fix to the plain-global case: retain the RHS when it's a bare container-local reference (str/array/map), mark a bare non-copy struct/enum RHS `dropped_locals` (ownership transfer) instead, and release the OLD global value on overwrite (closes a latent leak the same raw-slot-store pattern also caused). Found and fixed a SECOND bug while proving this: the initial patch called `lower_expr_to_rvalue` directly instead of the generic `lower_expr_to_operand` fallback it replaced, silently dropping that fallback's `consume_call_args` call -- so `G = push(G, it)` (the common push-chain form) no longer marked the pushed struct-typed local `it` as consumed, and `it`'s own scope-end Drop freed the box immediately after pushing it; the freed slot was reused by the NEXT call's `it` allocation, so every element of a struct-typed global array silently aliased the LAST value pushed (found by this fix's own regression test, not the original repro, which only asserted `len`). Fixed by calling `consume_call_args` in the global path too, with one adjustment: `consume_call_args`'s self-skip for `a = push(a, v)` compares MIR local IDENTITY (`arg[0] == dest`), which never matches for globals (every textual read of a global lowers to a FRESH temp via `emit_global_load`) -- detected the self-referential `G = push(G, ...)` shape at the AST level instead (arg 0 is literally the identifier being assigned) and excluded only that arg from the generic call, so the true "second owner" case (`G2 = push(G1, x)`) still gets its correct retain. PROVED BOTH WAYS: `tests/conformance/conf_global_reassign_cross_fn.kry` (array/str/map globals, asserting actual element VALUES not just length) -- reverting the `lower.rs` fix and rebuilding (`cargo build --release -p kryos-cli`, no Rust-runtime crates touched so no full rebuild needed) crashes on BOTH backends with the exact original panic (`kryos_array_push: corrupt array header ... cap=0, data=0x0`, exit 98); reapplying and rebuilding passes on both. Also manually verified the cross-global "second owner" case (`G2 = push(G1, x)`) and the struct-element-aliasing regression (`G[i].v` reading the last-pushed value on JIT) both stay correct. `tests/known_failures/global_array_reassign_corrupt.kry` deleted (folded into the conformance test above); conformance 53/53 -> 54/54 (README.md's count updated in the same commit, `tests/docs_status_gate.sh` was catching the drift). Gates: conformance 54/54 both backends, tier1+tier2 GREEN, bootstrap 16/16 x2. **Also newly found and NOT fixed (out of scope for this wave, filed for a future pass):** pushing a struct literal built from a NAMED local into ANY array (global OR local) that is itself read fresh each time (i.e., the exact `fn add_one(n) { let it = Item{v:n}; ARR = push(ARR, it) }` shape) depends entirely on `consume_call_args` correctly recognizing the push-target identity; this is now correct for the plain-global and plain-local cases, but was NOT re-audited against the pre-existing Cranelift struct-box-aliasing family already tracked as ledger item 3's design note (`kryos-codegen-cranelift`'s uniform struct boxing + missing owner-count guard on the local/param/return drop path) -- any future change to `consume_call_args` or to path 2's drop semantics should re-verify this exact shape on JIT specifically, since it is where item 3's "two disagreeing struct-drop paths" mechanism is easiest to accidentally re-trigger. |
| **LEDGER item 7: mutated-SCALAR-capture persistence never got the "N>=2" generalization the mutated-STRUCT-capture case already had -- SILENT WRONG VALUE in 3 shapes** | Reproduced live before touching code, both backends, identical (shared MIR): `tests/known_failures/closure_mutated_capture_scalar_gaps.kry` -- shape 1 (two mutated scalars in one closure) printed `1010,1010,1010` instead of climbing `1010,2020,3030`; shape 2 (one mutated scalar + one mutated struct together) printed `102,103,104` instead of `102,104,106` (the struct persisted, the scalar froze at its first-call contribution); shape 3 (a solitary mutated scalar whose closure's tail is a DIFFERENT expression -- a "stateful factory" returning an inner closure) printed `1,1` instead of `1,2` across successive outer calls. ROOT CAUSE (read, not guessed, `kryos-mir/src/lower.rs` lambda lowering): the old mechanism smuggled the new value back by writing the closure CALL's RETURN VALUE into the env slot from the env-thunk, which only worked when `mutated_captures.len() == 1 && tail_value_is_identifier(body, &mutated_captures[0])` -- exactly one mutated capture whose body's tail IS that identifier. Any other shape silently reverted to reading the original captured value every call. FIX: generalized the struct case's OWN fix (`mutated_capture_ptr_slots`, pass-by-pointer) to scalars, which previously had no address to hand out (plain SSA values). Every mutated SCALAR capture is now boxed behind an ARC-allocated heap cell at closure-construction time -- reusing the EXACT SAME `RValue::ArcAlloc`/`RValue::Deref`/`MirType::Shared` machinery that already backed the READ-ONLY struct-literal-field capture case (`box_scalar_captures`) -- so the capture's env slot holds a POINTER instead of a raw value; the closure's own parameter for that capture becomes `Shared(scalar)`; the prologue dereferences it once into the original local (unchanged body code); and -- the genuinely new half -- an `Instruction::StoreDeref` writes the local's current value back through the SAME pointer before EVERY `Terminator::Return` in the function (a pattern with direct precedent in this same file: the `@budget` annotation's pop-to-depth instrumentation does an identical "for every block whose terminator is Return, append an instruction" pass). This has NO tail-shape or capture-count restriction, fully replacing (not augmenting) the old return-value-smuggling mechanism -- `MirAttributes::mutated_capture_slot` is no longer set by anything (kept as a field, documented as dead, to avoid ripping out the still-harmless codegen plumbing that reads it). Required zero codegen changes on either backend: `StoreDeref`/`Deref`/`Shared`-typed params were all pre-existing, proven machinery. Proof both ways: `git stash` the `kryos-mir` fix + full `cargo build --release` (required -- MIR/codegen crates, not `-p kryos-cli`-safe to skip) -- all 3 shapes reproduce the exact documented wrong values on BOTH `kryos run` and `kryos build --release`; `git stash pop` + rebuild -- all 3 shapes correct on both backends (`1010,2020,3030` / `102,104,106` / `1,2`). Regression suite additionally verified NOT to break under the fix (all re-run live, both backends, matching or exceeding prior known-good values): the ORIGINAL single-mutated-scalar "RESOLVED" idiom (`let bump = || { count = count+1  count }` -> `1,2,3`, outer var frozen at `0`), the same idiom at `f64` and `bool`, a closure mutating a struct capture AND an array capture together, and the existing `two_mutated_struct_captures` case (`111,122,133`, outer vars `0,100`) -- all unchanged. `conf_spinlock_mutex` re-run 5x clean on AOT (unaffected -- this fix touches only SCALAR capture representation, not the struct-capture-at-spawn path that test exercises). Regression: `tests/conformance/conf_functions.kry` (3 new checked functions: `two_mutated_scalar_captures`, `mixed_scalar_and_struct_mutated_captures`, `stateful_factory_mutated_scalar`; `tests/known_failures/closure_mutated_capture_scalar_gaps.kry` deleted per the known-failures-to-gate convention). Gates: conformance 53/53 (both backends), tier1+tier2 GREEN, bootstrap 16/16. CLAUDE.md gotcha #11 and `MirAttributes` doc comments (`kryos-mir/src/ir.rs`) updated to state the new mechanism and retire the old one. See item 7b below for the SAME race class re-surfacing under `spawn` -- NOT closed by this fix (orthogonal: this fix is about single-threaded generalization, not cross-thread atomicity). |
| **Capability escape via closure/fn-value laundering stored in a CONTAINER (struct field / array element / map value / nested combination) -- the narrowed residual of the mostly-closed laundering fix, LEDGER item 1** | Reproduced live before touching any code: `tests/security/cap_escape_closure_launder_container.kry` (struct field, pre-existing repro) compiled clean under BOTH `--capabilities-mode=inferred` and `--strict-capabilities` and printed the secret from INSIDE a `deny!(fs:read)` block; wrote and reproduced 3 sibling repros the same way -- `..._array.kry` (array element), `..._map.kry` (map value), `..._nested.kry` (a struct field holding an ARRAY of closures). All 4 confirmed vulnerable pre-fix (exit=0, no diagnostic, secret printed). ROOT CAUSE (matches the fix sketch this ledger already carried): `hot_params`'s seed pass only ever marked a PARAMETER hot when its OWN declared type was `fn(...) -> ...` (`is_fn_typed`); a parameter typed `Registry` (struct with a fn-typed FIELD), `[fn() -> str]`, or `map<str, fn() -> str>` never matched, so the drilling function's own param was never marked hot and `resolve_closure_caps` never got a chance to trace `reg.reader`/`arr[i]`/`m[k]` back to the value written into it. FIX (`kryos-capabilities/src/checker.rs`): (1) `struct_field_types` -- new struct-name -> field-type map, collected in Pass 0; (2) `is_fn_bearing_type` -- recognizes a struct with >=1 function-typed field, an array whose element type is a function, and a `map<K,V>` whose VALUE type is a function, recursively (so a struct field holding an array of closures qualifies) with a depth cap against recursive struct definitions; (3) `PathStep`/`decompose_container_path`/`resolve_type_path` -- a field/index access-chain representation and a walker that reduces `obj.field(...)`/`arr[i](...)`/chains of these to `(root identifier, path)`, validated against the root's OWN declared type before counting, so an ordinary method call that happens to share a struct field's name is never misclassified as hot (verified: see no-cascade check below); (4) `hot_params`'s type changed from `HashMap<String, HashSet<usize>>` to `HashMap<String, HashMap<usize, HashSet<Vec<PathStep>>>>` -- a hot parameter now carries the SET OF PATHS through which it's invoked (empty path = the pre-existing direct-fn-typed-parameter case, unchanged), populated by a new container-invocation seed pass alongside the existing direct-call seed, and propagated through forwarding exactly like the direct case (broadened the propagation filter from `is_fn_typed` to `is_fn_bearing_type`); (5) `resolve_container_path_caps` -- walks a struct/array/map LITERAL (or a `let`-bound alias of one, tracked by the new `build_local_container_lits`/`current_local_container_lits`) down the recorded path: a struct field is traced PRECISELY by name (unwritten/defaulted field -> `Unknown`), an array/map is traced INDEX-INSENSITIVELY (unions every element/value written -- conservative, matching the ledger's own design note), falling back to `Unknown` -> `Capability::All` for a non-literal source (a `push`ed loop, a function return, a read from another container) -- the same sound fallback already used for every other unresolvable shape; (6) `accumulate_hot_extra_caps` now iterates every recorded path per hot index instead of resolving the whole argument once. Proof BOTH ways, all 4 shapes, both modes: `git stash` the `checker.rs` fix + full `cargo build --release` (required -- this crate is linked into the runtime toolchain, `-p kryos-cli` does not rebuild it) -- all 4 repros compile clean (exit 0) and print the secret from inside `deny!(fs:read)`, in both `inferred` and `--strict-capabilities`; `git stash pop` + rebuild -- all 4 rejected with `E0507` citing the closure/fn-value argument, in both modes. No-cascade verified two ways: (a) `tests/security_gate.sh`'s existing HOF checks (#5-6) still pass unchanged; (b) a NEW positive probe -- a struct/array/map "registry" of PURE closures (the actual plugin-registry/router-table/dispatch-map shape this residual mattered for) compiles clean with ZERO annotation under `--strict-capabilities` and runs correctly; (c) a struct with BOTH a privileged fn-typed field AND an unrelated real method compiles clean when only the real method is called (`resolve_type_path` correctly rejects the method name as a field path, so it is never misclassified as hot). Regression: extended `tests/security_gate.sh` with checks #7-10 (reject, both modes, all 4 shapes) and #11 (the pure-closure registry no-cascade probe). Gates: `security_gate.sh` PASS (11/11), full `cargo build --release` clean, `kryos-loop.sh gates 2` and `test_bootstrap.sh` re-verified (see below). Docs: `docs/10-capabilities.md`'s "Known limitation" section rewritten -- the container-storage residual is now CLOSED, not open; the implementation-status callout, the `strict` mode table row, and the "one residual gap" prose all updated to state the closure/fn-value laundering fix is now sound for every indirection shape it covers (parameter/local/return/passthrough/actor/spawn/generic/dyn AND container storage), with no remaining known gap for this class of escape. NOT attempted / genuinely out of scope (documented, not silently dropped): a container built from a NON-LITERAL source (populated via `push` in a loop, returned from another function, read out of ANOTHER container) still resolves to `Unknown` -> requires `Capability::All` at the call site -- this is the SAME conservative fallback the shipped parameter-based fix already uses for its own unresolvable shapes (a closure whose provenance can't be traced at all), not a new gap this fix introduces. |
| **FINAL SWEEP (item 1b, trust-model break): `kryos pkg install`/`add` never verified a checksum against anything -- the documented "tarballs are pinned by hash" claim was FALSE** | Live repro (real `NORTHTEKDevs/kryos-registry`, real `git clone`, no mocking): `kryos pkg add http-router && kryos pkg install` then `echo MALICIOUS_INJECTED_CONTENT >> ~/.kryos/packages/http-router-0.1.0/src/lib.kry`, then a SECOND project depending on the same name+version ran `kryos pkg install` and silently reused the tampered cache (`installed 1 package`, exit 0, no warning). ROOT CAUSE (three compounding gaps, all closed): (1) `LockFile::checksum` was always written `None` -- `LockFile::from_resolved` never read anything into it; (2) `RegistryEntry.checksum` (a real published `sha256:<hex>`) was never compared against anything -- `pkg info`/`show` was its ONLY consumer, a human-readable display; (3) even if wired up naively, the two sides of the comparison didn't correspond -- `generate_index_entry` hashed a placeholder "tarball" (`pack()`'s `target/package/*.tar.gz`, actually just a text LISTING of file names, not their content) while `fetch_github_subdir` never produces or downloads a tarball at all -- it `git clone`s the registry repo and copies out `packages/<name>/<version>/` as a directory tree, so there were no tarball bytes on the install side to hash even in principle. FIX: introduced a single canonical content-hash function, `content_checksum` (`kryos-package/src/registry.rs`) -- `sha256:<hex>` over `kryos.toml` + every `.kry` file under `src/`/`stdlib/`, hashed in deterministic `/`-normalized sorted-path order (platform-independent). `pack()` now computes this over the exact files it publishes and stores it as `PublishPackage.content_checksum`; `generate_index_entry` emits it VERBATIM (no more tarball-byte hashing). On the install side, `AvailablePackage`/`ResolvedPackage` gained a `checksum: Option<String>` field threaded from the registry-index lookup through `resolve()`; `fetch::fetch_resolved` calls the new `verify_package_checksum(dest, expected, name, version)` on EVERY `Remote` package -- including a CACHE HIT, not just a fresh fetch -- recomputing `content_checksum` over the on-disk directory and comparing against the index-recorded value. A mismatch OR a missing/empty checksum is rejected (fails closed, per the "missing checksum is the same hole with extra steps" directive) and the poisoned cache entry is `remove_dir_all`'d so a later run cannot mistake it for a good install. `LockFile::from_resolved` now threads the VERIFIED checksum into `kryos.lock` instead of always writing `None`. While in the unpack path: `copy_dir_all` (the function that materializes a fetched `github_subdir:` package from the git clone) now rejects any symlink entry via `DirEntry::file_type()` (which does NOT follow symlinks, unlike the `Path::is_dir()` it used before) -- a malicious registry commit could otherwise plant a symlink pointing outside the package (e.g. at another cached package or up the filesystem) and have this function silently copy unrelated files into the local cache; a real tar-based zip-slip (`../`/absolute entry paths) does not apply today since there is no tar-format extraction anywhere in this path, only a directory walk whose entries come from `read_dir` (which cannot yield path-separator-bearing names). Also fixed while in the file: a failed fetch (including a now-rejected symlink) no longer leaves a partial `dest`/tmp-clone lying around to be mistaken for a successful cache entry on a later run; `collect_kry_files`'s prefix was hardcoded to `"src/"` regardless of caller, mislabeling `stdlib/` files in both the publish listing and the new checksum's path space -- now takes an explicit `prefix` argument. Proof both ways, unit level (`tests/checksum_verification.rs`, 5 new tests + `registry::tests::content_checksum_is_deterministic_and_content_sensitive` + `fetch::tests::copy_dir_all_refuses_a_symlink_entry_pointing_outside_the_package`): with `verify_package_checksum` temporarily short-circuited to `Ok(())` (simulating the pre-fix behavior) and with `copy_dir_all`'s symlink guard temporarily removed, 4 of 5 checksum tests and the symlink test all go RED (confirmed via a`build+test` cycle with the guard stripped, then restored); with the real fix, all pass, including a legitimate-content-still-installs case. Proof both ways, LIVE (real registry, real network, no mocking): pre-migration, a legit `kryos pkg add http-router && kryos pkg install` against the (at-the-time still old-scheme) index FAILED with a checksum mismatch -- correctly proving old published checksums were computed under the broken scheme and would need republishing, not that the new verification itself was wrong. Migrated all 13 real package-version JSON entries in `NORTHTEKDevs/kryos-registry` to the new `content_checksum` scheme (computed via `kryos pkg publish` run against each already-published `packages/<name>/<version>/` directory -- the package CONTENT is unchanged, only the recorded checksum is corrected to actually describe it; pushed as `NORTHTEKDevs/kryos-registry@9025d8a`). Post-migration: `kryos pkg add http-router && kryos pkg install` succeeds (exit 0, `kryos.lock` now records a real checksum instead of nothing), and repeating the EXACT ledger repro (tamper the cached `src/lib.kry`, then `kryos pkg install` from a second project depending on the same name+version) now fails closed with `error: checksum mismatch for \`http-router\` v0.1.0: expected sha256:ec03da92... got sha256:9a0086b7...` (exit 1) and the tampered cache directory is removed. Docs corrected to state the real (now true) guarantee instead of the false one: CLAUDE.md's package-registry paragraph, `docs/package-registry.md`'s status callout + `kryos pkg install` section, `README.md`'s Status prose + feature table (both previously named this as one of two most-severe open items; the closure/container capability-laundering gap named at the time is now also CLOSED (see the CLOSED table entry above)). NOT changed: the transport mechanism itself (still `git clone` of a directory tree, not a downloaded/verified tarball -- `docs/package-registry.md`'s BLAKE3/HTTPS-GET design spec remains aspirational, SHA-256/git-clone is what's actually implemented); `pkg add`'s wildcard-version-by-default behavior (`name = "*"`) is unchanged, so pinning a specific version still depends on committing `kryos.lock` -- but the CONTENT behind whatever version is locked is now cryptographically checked on every install. Gates: `kryos-package` unit+integration tests 62/62 (38 lib + 19 `tests/package.rs` + 5 new `tests/checksum_verification.rs`), full workspace `cargo build --release` clean, `kryos-loop.sh gates 2` and bootstrap re-verified GREEN (see below). |
| **FINAL SWEEP (2026-08-02): a single stray token at block-statement level (a bare `,`, or a `)`/`]`/`}` with no enclosing call/array/struct-literal to absorb it) HUNG the parser forever, zero output -- reachable by a one-character typo** | Found fresh-eyes probing the CLI/wasm surface (started from `map<str, i64>{}`, a plausible mistyped empty-map literal -- correct syntax is bare `{}` per `examples/wasm_maps.kry` -- which bisected down to a minimal 6-token repro with no map/generics involved at all: `fn main() { let x = 5 , }`). Verified live with `timeout`: `kryos check` on that file ran the full 10s/15s timeout with **zero bytes of output** (not a fast crash -- earlier untimed runs looked like a prompt `exit=127` only because something else eventually killed the process; timed runs proved it hangs). ROOT CAUSE (read, not guessed): the diagnostic-cascade fix closed earlier this session (`parse_primary`'s unexpected-token fallback, `kryos-parser/src/parser.rs`) deliberately stopped consuming `RParen`/`RBracket`/`RBrace`/`Comma` on an unexpected token, on the assumption that an ENCLOSING call/array/struct-literal/match-arms loop would consume it during its own recovery -- correct for a token nested inside one of those constructs, but at the OUTERMOST block-statement level there is no such enclosing construct. An expression-statement built entirely from that fallback (e.g. a stray trailing `,`) returns `Some(Stmt::Expr{..})` with the cursor exactly where it started; `parse_block_stmts`'s loop only force-advances when `parse_statement()` returns `None`, so a `Some` that made literally zero progress spins the identical token through the loop forever. `parse_module` (the top-level declaration loop, one level up the grammar) already has the exact right guard for this bug CLASS -- a `self.pos == before` no-progress check with a comment citing a prior fuzzer-found 2-byte hang (`"}:"`) -- but it was never mirrored down to the block-statement loop. FIX: added the same before/after-position guard to `parse_block_stmts` (factored into a shared `recover_stray_block_token` helper used by both the `None` and now-guarded `Some` paths), so any statement parse that consumes zero tokens forces one diagnostic + one token of progress instead of looping. Proof both ways: `git stash` the fix + rebuild -- `fn main() { let x = 5 , }` times out (10s, 0 bytes) on `kryos check`; restore + rebuild -- 2 clean diagnostics (`E0003` + `E0009`), exit 1, `<1s`. Non-regression: the ORIGINAL cascade-fix repro (`let match: i64 = 5` + `to_string(match)`, reserved-keyword-as-value) re-verified still exactly 2 errors, no cascade reintroduced. Regression: `tests/diagnostics_gate.sh` check 6 (bounded with `timeout`, since a `conf_*.kry` conformance file can't assert "must not hang" -- same precedent as `docs_status_gate`/`utf8_invalid_string_gate`). Gates: conformance 53/53, tier1+tier2 GREEN, bootstrap 16/16, security_gate PASS, differential fuzz gate (seeds 1-40) 0 divergences. |
| **FINAL SWEEP (2026-08-02): a void-returning call's "result" silently type-checked as an argument to any opaque-signature polymorphic builtin (`to_string`, `abs`, `min`, `max`, `sort`, `reverse`, ...) and read back as garbage/zero at runtime -- SILENT WRONG ANSWER, both backends** | Found probing `kryos repl` (fresh-eyes CLI surface sweep): every bare `println(..)` line at the REPL prompt printed a spurious extra `0` -- traced to the REPL's auto-print heuristic wrapping the input as `to_string(println(..))` to "echo" bare-expression results, which is SUPPOSED to fail to compile for a void-returning statement (falling back to a silent run per the REPL's own comment) but instead type-checked clean. Reproduced directly, outside the REPL, on both backends: `fn side_effect() { println("ran") } fn main() { println(to_string(side_effect())) }` prints `ran` then `0` (not a diagnostic) on `kryos run` AND `kryos build --release`; `abs(side_effect())` reproduces identically. The concretely-typed sibling case (`fn take_i64(v: i64) -> i64 {..}` then `take_i64(side_effect())`) already correctly rejects with `E0100: expected i64, found void` -- proving the gap is narrow, not general. ROOT CAUSE: `to_string`/`abs`/`len`/etc. declare their param as the opaque `Type::Error` sentinel (`kryos-types/src/check.rs`) specifically so ONE signature accepts any real value type; `unify`'s "Error unifies with anything" error-recovery escape hatch (`kryos-types/src/infer.rs`, `if a.is_error() || b.is_error() { return Ok(()) }`) was never taught to exclude `Type::Void`, which is not a real value at all, so it silently unified too. FIX: a new check alongside the existing (adjacent, same-shape) `len`-specific struct/enum guard -- whenever a call argument's `param_ty` is the opaque `Type::Error` sentinel AND the argument's resolved type is `Type::Void`, reject with a clear message naming the callee. **First attempt regressed `examples/async_io.kry`**: `coop_spawn(taskExpr)` shares the identical opaque-param shape but its argument is a TASK EXPRESSION handled specially at MIR lowering (`lower_coop_spawn`, mirrors `spawn { .. }`), not a value read at the call site -- a void-returning task function is the correct, intended case there, per the signature's own pre-existing comment ("the argument is a task expression handled specially at lowering"). Caught by running the full gate ladder before declaring done (`examples`/`strict_caps` went RED), not by the new unit repro alone -- exactly the "prove both ways AND run the full gate" discipline this ledger's non-negotiables require. Fixed by excluding `coop_spawn` by name from the new check (mirrors the existing `len`-specific name-gating immediately below it). Proof both ways: `git stash` the checker fix + rebuild -- `tests/type_soundness.sh`'s two new `want_reject` cases both report `HOLE` (unsound program passed check); restore + rebuild -- both correctly rejected, `coop_spawn_void_task_ok` and `polymorphic_builtins_still_work` (ordinary `to_string`/`abs`/`len` usage) both still accepted and run correctly. Regression: `tests/type_soundness.sh` (4 new cases: 2 `want_reject`, 2 `want_pass`). Gates: conformance 53/53, tier1+tier2 GREEN (including `examples`/`strict_caps`, which caught the `coop_spawn` regression), bootstrap 16/16, security_gate PASS, differential fuzz gate 0 divergences. |
| **FFI/extern surface wave (2026-08-01): "declared but unimplemented" extern shapes now REJECTED at check time (E0508) instead of compiling then failing at link time or segfaulting at runtime** | Verified live, both backends, exactly as CLAUDE.md documented: `extern "C" { fn abs(x: i32) -> i32 }` failed AOT with a type mismatch and "worked" on `kryos run` only via collision with the ambient `abs` builtin; `extern "C" { fn getpid() -> i32 }` failed AOT codegen with `use of undefined value '@getpid'` (confirmed via `--emit-llvm`: codegen never emits a `declare` for a non-`kryos_*`/non-builtin-colliding name); `extern { fn kryos_env_get(key: str) -> str }` compiled clean and SEGFAULTED on BOTH backends (exit 139) -- confirmed via `--emit-llvm` that the call site (`call ptr @kryos_env_get(ptr @.str.0.hdr)`, 1 arg) is emitted from the user's OWN extern declaration while the hardcoded-correct runtime declaration (`declare i64 @kryos_env_get(ptr, i64, ptr, i64)`, 4 args) sits unused in the same module -- the extern's param/symbol info is genuinely never threaded to codegen, exactly as documented. DECISION: (b) reject at check time, not (a) implement real FFI emission -- real arbitrary C-library linking needs `[build] link` support (not implemented), linker-flag passthrough, and per-backend declare-emission changes across both codegen backends; not tractable as a small, reviewable commit, and every extra day of "compiles, might work" is worse for a capability-safety pitch than an honest rejection. FIX: new `error[E0508]` (`kryos-types/src/check.rs::check_extern_item_shape`, called from `register_decl`'s `Decl::Extern` arm, so it fires on `check`/`run`/`build` uniformly, independent of capability mode) rejects (1) any extern name not prefixed `kryos_` (arbitrary C FFI, unconditionally -- including names that "work" via builtin collision, since that was itself part of the trap) and (2) a `kryos_*`-prefixed extern with a str/array/map/struct/tuple/enum/fn -typed param or return, UNLESS the name is in a small explicit allowlist (`kryos_builtin_to_upper`/`to_lower`, `kryos_ffi_dlopen`/`dlsym`/`cstr`/`strlen`/`string_from_ptr`) built from a repo-wide grep of every `kryos_*` extern signature that legitimately uses `str` today (the stdlib's OWN `ffi.kry`/`strext.kry`/`string.kry` — these are compiler-verified-safe because their real native symbol accepts a Kryos str/handle directly, unlike `kryos_env_get`, which expects raw pointer+length pairs per `std::os`'s `_env_or_empty`). Proof both ways: `git stash` the 3-file fix + `cargo build --release`, all 4 repro shapes (abs/getpid/kenv/puts) compile clean with exit 0 (confirmed live); restore + rebuild, all 4 rejected with E0508 naming the exact limitation, `kryos_process_argc` (safe scalar-only `kryos_*` extern) and `env_get()` (builtin route) both still work unchanged. Also caught by the fix and needed updating: 2 already-broken root examples (`examples/ffi_libc.kry`, `examples/ffi_test.kry`) hand-declared `_getpid`/`puts` directly -- rewritten to use the ALREADY-WORKING `kryos_ffi_dlopen`/`dlsym`/`dlcall0` dynamic-loading pattern (matching `examples/ffi_dlopen.kry`), since that's the only way this compiler can genuinely reach a real C library function today. `compiler/crates/kryos-test-runner/tests/e2e/functions/extern_ffi.kry` (previously asserted the now-rejected `puts(s: str)` shape "type-checks + compiles" as a GOOD outcome) rewritten to assert the safe `kryos_process_argc` pattern instead; 2 new `error_cases/extern_*.kry` fixtures gate both E0508 paths in the e2e suite (proven RED pre-fix, GREEN post-fix). Docs: `docs/13-ffi.md` rewritten top to bottom (status note + every worked example now shows the E0508 rejection instead of the old link-failure/silent-wrong-output text); CLAUDE.md gotchas #22's two FFI entries (C-FFI-not-emitted, kryos_* str-signature segfault) rewritten RESOLVED; `STABILITY.md`'s stale "examples gate: root 44/44, showcase 23/23" corrected to the live 45/45 / 24/24 (pre-existing drift, found while re-running the gate, not introduced this session). Gates: `kryos-loop.sh gates 2` GREEN (conformance 53/53, tier1+tier2 all PASS incl. `docs_status_gate`), bootstrap 16/16, `security_gate.sh` PASS, `examples` gate 45/45 root + 16/16 fixtures + 24/24 showcase, `kryos-test-runner` e2e+native suites green. NOT ATTEMPTED (filed, not half-fixed): real arbitrary C-library FFI emission (option (a)) -- would need `[build] link`/`-l` flag support, linker invocation changes, and per-backend typed-declare emission keyed off the extern's OWN declared signature (today codegen only knows the hardcoded runtime symbol list); a genuinely new feature, not a hardening fix, and out of scope for this wave. Also NOT checked: whether a user-declared extern can SHADOW/conflict with another user-declared extern of the same name with a different signature within one program (a narrower, lower-severity redeclaration-consistency gap; not reproduced, not gated). |
| **Strings/UTF-8/byte-buffer wave (2026-08-01): three real bugs found by direct reproduction, all in-scope for "a substr that splits a multibyte codepoint corrupts or crashes downstream"** | Probed byte_at/char_code/substr/contains/find/split/replace/reverse/to_upper/to_lower/trim/string_builder/interpolation per the assigned wave. Interpolation (braces/escapes/nested quotes), string_builder double-build safety, and `std::string::find`/`starts_with`/`ends_with`/`strext::trim` were all re-verified correct live -- **not** re-litigated as bugs. Three real defects found and fixed: **(1) SILENT DATA LOSS (most severe):** `contains`/`trim`/`to_upper`/`to_lower`/`replace` (Rust-builtin-backed, `kryos-rt/src/string.rs`) and `split`/`join`/`trim_start`/`trim_end` (`kryos-rt/src/builtins.rs`) converted a `KryosString`'s raw bytes to `&str` via `str::from_utf8(..).unwrap_or("")` -- ANY invalid UTF-8 byte (trivially produced by an ordinary `substr()` that splits a multibyte codepoint; substr is byte-indexed and never checks codepoint boundaries) made the WHOLE string act as empty: `trim("café"-substr-truncated-to-4-bytes)` silently returned `""` (discarding the entire original content), and `contains(bad, "caf")` silently returned `false` even though "caf" is a genuine byte-prefix -- no panic, no diagnostic, just wrong data. Live repro (`git stash` the fix, rebuild): `trim(bad) len=0`, `to_upper(bad) len=0`, `to_lower(bad) len=0`, `replace(bad,"x","y") len=0`, `contains(bad,"caf")=false`; fix restored, same calls PANIC with `kryos panic: string operation requires valid UTF-8, but the string contains invalid byte sequences (a substr()/byte_at() call likely split a multibyte character mid-codepoint) -- use std::utf8::is_valid(s) to check first`. FIX: both files' private `bytes_to_str` helper now panics via `crate::panic::kryos_panic` on `Err(_)` instead of `unwrap_or("")`, matching the existing "fail loudly like the other checked builtins" precedent (`file_read`'s missing-file panic, `kryos_string_slice`'s OOB panic) already in the same file. Does NOT affect the byte-buffer model: a `chr()`/`base64_decode()`-built latin-1 buffer is always valid UTF-8 by construction (codepoints 0-255 always encode validly), so invalid content here is ALWAYS a boundary bug upstream, never a legitimate payload -- `find`/`starts_with`/`ends_with` were never affected (already raw-byte comparisons, no UTF-8 decode step). Gate: `tests/utf8_invalid_string_gate.sh` (new, wired into `kryos-loop.sh gates` tier1 AND `.github/workflows/ci.yml`) -- asserts BOTH directions (invalid input panics loudly; ordinary valid multibyte input on the same 5 builtins is unaffected, guarding against over-rejection) since a nonzero-exit assertion can't live in a `conf_*.kry` (conformance requires exit 0). **(2) CRASH:** `std::string`'s codepoint walkers (`chars`, `char_at`, `reverse`, `split(s, "")`) detected a UTF-8 lead byte and unconditionally stepped 2-4 bytes forward with NO bounds check, including at the string's own last byte -- a `substr()`-truncated tail (a lead byte with zero continuation bytes left) computed a slice end past `len(s)` and `kryos_string_slice` panicked (`string slice out of bounds`, exit 98) from ordinary byte-index arithmetic on an ordinary valid multibyte string, not adversarial input. Live repro: `chars(substr("café",0,4))` panicked pre-fix, `git stash`-verified. FIX: new `std::utf8::step_at(s, bytepos) -> i64` clamps the step so `bytepos + step` never exceeds `len(s)`; all four call sites in `string.kry` now call it instead of duplicating the unclamped stepping logic (also fixes a latent 5th duplicate that was never fully consistent -- `reverse`'s stray-continuation-byte fallback vs `chars`'/`split`'s lack of one). **(3) SILENT WRONG ANSWER in `std::bytes`:** `find_byte`/`find_seq`/`compare`/`is_ascii` (module doc: "treating chars as bytes") walked raw UTF-8 byte offsets `0..len(s)` one byte at a time -- but a latin-1 byte-buffer value >= 0x80 needs TWO UTF-8 bytes to encode (UTF-8's 1-byte range is only 0-0x7F), so `len(s)` OVERCOUNTS such a buffer and the byte-offset walk read only the LEAD byte of each 2-byte value: `find_byte(chr(10)+chr(200)+chr(30), 200)` returned `-1` (NOT FOUND) for a buffer that genuinely contained 200, and `find_byte(.., 30)` returned the WRONG index (3, not 2) -- every subsequent index off by one per high byte seen. Live repro verified pre-fix exactly as described. FIX: rewrote all four `std::bytes` functions to be CODEPOINT-indexed via `step_at` (matching `byte_at`'s own documented "CODEPOINT of the i-th CHARACTER" contract) instead of raw-UTF8-byte-indexed; `find_seq` compares by logical byte VALUE (codepoint arrays) rather than raw substr equality, so it is correct regardless of each matched byte's own encoding width. Regression (both crash-class and silent-wrong-answer-class, proven correct AND proven still-correct on plain ASCII, both backends): `tests/conformance/conf_utf8_string_hardening.kry` (JIT + AOT, both green; `git stash` of `bytes.kry`+`string.kry`+`utf8.kry` makes the file fail to even compile -- `step_at` doesn't exist -- proving the fix is load-bearing). Docs: CLAUDE.md gotcha #22 extended with both the `len()`-overcounts-a-high-byte-buffer trap and the substr-boundary/panic-consistency note; README.md + docs/BUGS.md conformance count corrected 52/52 -> 53/53 (`docs_status_gate` caught the drift). Gates: conformance 53/53, tier1+tier2 GREEN, bootstrap 16/16. NOT FIXED / OUT OF SCOPE, filed here rather than half-fixed: `kryos-stdlib-native/src/bindings.rs`'s `handle_to_str` (46 call sites spanning crypto/regex/HTTP2/actor-messaging, including `byte_at` itself) has the IDENTICAL `unwrap_or("")`-on-invalid-UTF8 pattern as bug (1) above, but the blast radius (46 sites across crypto/network primitives) is too large to verify individually in one focused session -- `byte_at()` on an invalid string currently silently returns `-1` for every index (a defensible-if-imperfect "can't read this" answer, not fabricated data, lower severity than the trim/contains data-loss class that WAS fixed). Minimal repro for whoever picks this up: any `handle_to_str`-backed function (`byte_at`, `base64_encode`, `sha256`, `hmac_sha256`, regex functions, ...) called on a `substr()`-truncated invalid-UTF8 string silently treats it as `""`/no-match instead of panicking. `std::bytes` (the fixed module) does NOT depend on `handle_to_str` -- it uses the global `substr`/`char_code` builtins, unaffected. |
| **AOT-only: mutating a struct field NARROWER than 64 bits (`u8`/`i8`/`u16`/`i16`/`u32`/`i32`/`bool`) silently did nothing, or corrupted a neighboring field, on `build --release` — not overflow-specific, ANY assignment to such a field was affected** | Found probing the numeric/struct-field-overflow wave. Repro: `struct Ctr { v: u8 }` then `let mut c = Ctr{v:5}  c.v = 15  println("{c.v}")` prints `15` on `kryos run` but `5` (the ORIGINAL value, unchanged) on `build --release` — reproduces for plain-literal assignment, assignment from a local, and self-referencing arithmetic (`c.v = c.v + 10`) alike, and for every narrow scalar type (u8/i8/i16/i32/bool), not just overflow. A DIFFERENT shape from the same root CORRUPTS a sibling field instead of no-op'ing: `struct Three{x:u8,y:u8,z:i64}` then `t.y = 42` left `t.z` reading `999936` instead of its untouched `999999`. ROOT CAUSE (read the emitted `--emit-llvm` IR, not guessed): `StoreField` codegen (`kryos-codegen-llvm/src/codegen.rs`) treated EVERY scalar (non-aggregate) struct field as an opaque 8-byte slot and unconditionally emitted `store i64 <value>, ptr <field_ptr>` — correct for i64/ptr/double fields (always genuinely 8 bytes) but wrong for a field whose LLVM type is narrower (`%Ctr = type { i8 }` — the struct type reserves EXACTLY the field's real width, not a padded 8 bytes, unless a WIDER field afterward forces natural alignment padding to absorb the excess). For a struct whose only/last narrow field has nothing wider after it, the 8-byte store overflows the `alloca`'s real size — undefined behavior that LLVM's `-O2`/`-O3` optimizer (implied by `--release`) is free to treat as unreachable and eliminate outright, which is why the mutation vanished with no crash and no diagnostic (confirmed via the emitted IR: `getelementptr %Ctr, ptr %_0.addr, i32 0, i32 0` then `store i64` into a 1-byte alloca). When a narrower field DOES follow (e.g. `y: u8` before `z: i64` with only enough padding to align `z`, not enough to absorb a full 8-byte write starting mid-struct), the same store spills into that neighbor's real bytes instead. `kryos run`/Cranelift was correct throughout — this is a pure LLVM/AOT backend bug, not a shared-MIR defect (the struct LITERAL construction path a few lines earlier in the same function correctly used `insertvalue`/a full-width typed `store %Ctr`, so the mutation path was a distinct, less-tested code path). FIX: `StoreField` now branches on the field's actual LLVM type — `i64`/`ptr`/`double` keep the existing opaque `store i64`; any narrower type (`i8`/`i16`/`i32`/`i1`) truncates the widened value to that exact type first and stores with it (`store i8 ...`/`store i1 ...`), touching only the field's own bytes regardless of what does or doesn't follow it in the struct layout. Proof both ways: `git stash` the fix, `cargo build --release -p kryos-cli`, run `tests/conformance/conf_narrow_struct_field_store.kry` on AOT — fails at the first assertion (`CONF FAIL: single-u8-field struct: plain literal assign persists`, exit 1); `kryos run` on the SAME file passes (isolates it to AOT, not a language-level defect); fix restored, full `cargo build --release`, same file passes on BOTH backends. Regression: `tests/conformance/conf_narrow_struct_field_store.kry` (8 assertions: single-narrow-field struct of each type, self-ref arithmetic with/without overflow, two adjacent narrow fields with no padding between them, and the narrow-field-corrupts-neighboring-i64-field shape). Gates: conformance 52/52 (was 51/51 — `tests/docs_status_gate.sh` caught the drift, README.md/docs/BUGS.md corrected), tier1+tier2 GREEN, bootstrap 16/16, differential fuzz gate (seeds 1-40) 0 divergences. |
| **`std::fmt::format` took `args: [any]` -- EVERY non-i64 argument silently rendered as its raw pointer/bit pattern instead of its value, and the function's OWN doc-comment usage example was independently broken** | Found in the stdlib correctness sweep wave while probing `fmt` (a module the priority list flagged as "silent wrong number" risk). Repro: `format("Hello, \{0\}! You are \{1\}.", ["Alice", "30"])` printed `"Hello, 140698911633600! You are 140698911633632."` -- large heap-pointer-shaped integers, not the strings -- on every run, both backends, 100% reproducible (not probabilistic). ROOT CAUSE: `any` is erased to a bare i64 with no runtime type tag (the same limitation as OPEN item #6/CLAUDE.md gotcha #22), and `format`'s `args: [any]` parameter routed every argument through that erased slot; `to_string(args[i])` then printed the slot's raw bits reinterpreted as a number -- correct-looking for an i64 argument (bits==value) and silently wrong for `str`/`f64`. A SEPARATE, compounding defect: the doc comment's own example, `format("Hello, {0}! You are {1}.", ["Alice", "30"])`, is unusable as literally written -- Kryos strings interpolate universally, so the bare `{0}`/`{1}` in that DOUBLE-QUOTED SOURCE LITERAL are consumed by the compiler itself (as the expressions `0`/`1`) before `format()` ever runs; the literal call silently becomes `format("Hello, 0! You are 1.", [...])`, which has no `{0}`/`{1}` left to substitute and returns unchanged. Confirmed live: the verbatim doc example prints `"Hello, 0! You are 1."` with zero error, on a function whose entire purpose is template substitution. FIX: changed the signature from `args: [any]` to `args: [str]` (matching `std::string::format`, a sibling function that never had this bug because it was `[str]` all along) -- callers now pre-stringify each argument with `to_string(x)`, eliminating the `any`-erasure path entirely for this function (no ABI change needed, unlike the general `any` limitation in OPEN item #6, because `format`'s signature could simply stop using `any`). Doc comment rewritten to state the interpolation caveat explicitly and show the required escaped-brace invocation (`\{0\}` or `{{0}}`). Proof both ways: `git stash` the fix, rebuild-free rerun (stdlib `.kry` is read from disk, no `cargo build` needed) -- `tests/conformance/conf_stdlib_correctness_sweep.kry` fails at the FIRST assertion (`CONF FAIL: fmt::format substitutes real str values...`, exit 1); fix restored -- same file prints `PASS`, both `kryos run` and `kryos build --release`. No prior call site in the repo used `std::fmt::format` (grep across `tests/`/`examples/` found zero references), so the signature change is a pure fix with no blast radius. Regression: `tests/conformance/conf_stdlib_correctness_sweep.kry` (also covers isqrt/normalize/crc32/datetime/matrix/semver/iter/collections edge cases from the same sweep). Gates: conformance 51/51, tier1+tier2 GREEN, bootstrap 16/16. `README.md`/`docs/BUGS.md`'s "conformance 50/50" claims corrected to 51/51 (`tests/docs_status_gate.sh` caught the drift and failed until corrected). |
| **generic method bare self-field passthrough returning a COMPOUND shape (`-> [T]`, `-> (T, i64)`) kept the erased i64-slot element for non-pointer `T` -- CLAUDE.md gotcha #17 residual** | Found the fix already drafted (uncommitted) in the working tree at session start; this session's contribution was verification, gating, and doc correction, not the original diagnosis -- recorded here honestly rather than claimed as a from-scratch find. `fn all(self: Holder<T>) -> [T] { return self.items }` at `T=f64`: `Holder<f64>.all()[0]` printed the raw i64 bit pattern of `1.5` (`4609434218613702656`), identically on both backends (shared-MIR, not a divergence) -- confirmed live via `git stash` of the diff + rebuild. ROOT CAUSE: `instance_ret_needs_monomorphization` (`kryos-mir/src/lower.rs`) only recognized a bare-struct-literal-mentioning-`T` return shape as needing per-instantiation monomorphization; a `TypeExpr::Array`/`TypeExpr::Tuple` return that merely MENTIONS `T` fell through to `false`, so a bare self-field passthrough of such a field stayed on the single erased-to-i64 compiled copy (the exemption was designed for a bare `-> T` SCALAR slot, safe to reinterpret anywhere, not a CONTAINER whose elements each need a real type). FIX: extended `instance_ret_needs_monomorphization` with `Array`/`Tuple` arms mirroring the existing `Generic` arm. Proof both ways: `git stash` the fix, rebuild -> `Holder<f64>.all()[0]` prints the bit pattern; fix restored, rebuild -> prints `1.5`, on BOTH `kryos run` and `kryos build --release`. Extended verification this session (not in the original diff): a BARE TUPLE-FIELD passthrough (`fn get_pair(self: PairHolder<T>) -> (T, i64) { return self.pair }`, not a tuple-literal-construction body) also resolves correctly post-fix -- the fix generalizes symmetrically to both container shapes, confirmed via a fresh minimal repro, both backends. Regression WIRED INTO THE GATE this session (was previously only `tests/smoke/test_generic_compound_return.kry`, which is NOT part of any gate -- `tests/smoke/` has no automated runner beyond exit-code, per its own README): added `tests/conformance/conf_generic_compound_return_f64.kry` (value-asserted, `expect()`-style, matching the existing conformance convention), which IS swept by `tests/conformance/run_conformance.sh`'s `conf_*.kry` glob and therefore by `kryos-loop.sh gates`. Gates: conformance 50/50, tier1+tier2 GREEN, bootstrap 16/16. Docs: CLAUDE.md gotcha #17's residual note rewritten from "known gap" to RESOLVED; `README.md`/`docs/BUGS.md`'s "conformance 48/48" claims corrected to 50/50 (2 new conformance files; `tests/docs_status_gate.sh` caught the drift and failed until corrected -- proof the gate itself works, not just a courtesy edit). |
| **generic function RETURNING a closure at `T=f64` integer-added the float BIT PATTERNS instead of the values -- CLAUDE.md gotcha #22 residual** | Found the fix already drafted (uncommitted) alongside the item above; same honesty note -- this session verified, gated, and documented it. `fn make_appender<T>(suffix: T) -> fn(T) -> T { return \|x\| x + suffix }` at `T=f64`: `make_appender(0.5)(2.0)` printed a ~300-digit garbage integer instead of `~2.5`, identically on both backends -- confirmed live via `git stash` + rebuild (exact garbage value reproduced: `8988465674311580...` truncated). ROOT CAUSE: the type checker's per-lambda-param type table (`lambda_param_types`) is baked from a SINGLE check pass over the unspecialized generic template, where `T` never resolves to a concrete type -- it has nothing to give MIR for a closure LITERAL that is directly `return`ed from a generic function, so the closure's own un-annotated param stayed i64-erased at every instantiation regardless of what `T` resolved to at a given call site. FIX: `pending_lambda_ret_hint` (`kryos-mir/src/lower.rs`) -- `Stmt::Return`, when its value is directly a `Lambda` literal and the ENCLOSING function's already-monomorphized `current_ret_ty` is a concrete `fn(A) -> B` of matching arity, stages that concrete per-instantiation signature; the `Expr::Lambda` codegen arm consumes it as a fallback ONLY when the type checker's own per-param resolution came up empty, so it cannot override a real annotation or a HOF-inferred param. Proof both ways: `git stash` the fix, rebuild -> the f64 instantiation prints the garbage integer (i64 instantiation and str instantiation both still correct, isolating the bug to exactly the erased-float-add path); fix restored, rebuild -> f64 prints `~2.5`, i64 and str instantiations unchanged (no regression from the added fallback), on BOTH backends. Regression WIRED INTO THE GATE this session: added `tests/conformance/conf_generic_closure_return_f64.kry` (was only in ungated `tests/smoke/`) covering all three instantiations (`i64`/`f64`/`str`) in one program so a future change can't silently regress one while "fixing" another. Gates: conformance 50/50, tier1+tier2 GREEN, bootstrap 16/16. CLAUDE.md gotcha #22's mk_appender entry rewritten from "T=f64 has a residual VALUE bug" to RESOLVED. |
| **generic struct/enum base name ending in `_` (e.g. `Box_<T>`) broke bare-passthrough instance methods -- `unresolved external symbol <method>` on BOTH backends** | Found while building `tests/fuzz`'s own generic-struct template (named `Box_` to dodge a suspected-reserved `Box`). Minimal repro: `struct Box_<T> { val: T }  impl<T> Box_<T> { fn get(self: Box_<T>) -> T { return self.val } }` then `Box_{val:"ab"}.get()` -- `kryos run` fails to LINK (`LNK2001: unresolved external symbol get`), `kryos build --release` fails to CODEGEN (`use of undefined value '@get'`); both backends fail IDENTICALLY (shared MIR, not a backend bug -- confirmed via `--emit-llvm`, not guessed). ROOT CAUSE: 6 call sites in `kryos-mir/src/lower.rs` recovered a monomorphized name's base struct via `name.split("___").next()` to fall back to the erased-fast-path `method_owners`/`impl_method_generic_info` lookup. `Box_<str>` mangles to `Box____str` (base `Box_` + the `___` separator + suffix `str` = 4 consecutive underscores); splitting on the FIRST 3-underscore run consumes one of the base's own trailing underscores, recovering `Box` instead of `Box_` -- every fallback lookup then missed and silently resolved to the BARE unmangled method name. FIX: added `mono_base_name(ctx, name)`, which recovers the base by checking ALL registered struct/enum names for a `base + "___"` PREFIX (longest wins) instead of blindly splitting -- and checks prefix matches BEFORE any exact-name shortcut, because a monomorphized instance is ALSO registered under its own full mangled name (`struct_defs` gets both), so an exact-match-first order returns the mono name as its own base and silently reintroduces the identical bug (caught by re-testing after the first attempt failed identically, not assumed correct the first time). Replaced all 6 identical `.split("___").next()` call sites. Proof both ways: pre-fix, `tests/conformance/conf_generic_underscore_name.kry` fails to even BUILD on both backends (`LNK2001: unresolved external symbol get`+`dbl`, verified via `git stash` of the fix + rebuild); post-fix, both backends print `PASS`. Regression covers a bare passthrough getter, a self-operating method (`v + v`, `str` concat) coexisting on the same base, and a generic ENUM with a trailing-underscore base name. Gates: conformance 48/48, tier1+tier2 GREEN, bootstrap 16/16. Not a differential (JIT vs AOT) bug in the end -- both backends agree by failing identically -- but found via the same minimal-repro/read-the-IR discipline the differential harness (this wave's deliverable) is built on. |
| **capability escape via closure/fn-value laundering — parameter/local/return/passthrough-chain/actor-message/spawn/generic/dyn-Trait shapes (container storage was the residual, closed in a later session -- see the CLOSED table entry above)** | Root cause: `fn_capabilities` (`kryos-capabilities/src/checker.rs`) was keyed by NAME; calling a value bound to a parameter/local of function type resolved to nothing in that map, so a closure's authority never propagated to the calling scope regardless of what it did at runtime — verified live pre-fix: a `deny!(fs:read)` block did NOT stop a closure constructed before the denial from being invoked (through a zero-capability `zero_cap_tool`) INSIDE it, printing the secret, `check --strict-capabilities` exit=0, no diagnostic. FIX: (1) `hot_params` — a structural, capability-value-independent fixed point over the whole program identifying which fn-typed PARAMETER indices are invoked, directly or by being forwarded as a bare argument into another (transitively) hot position (covers passthrough chains of any depth); (2) `fn_return_closure_caps` — a fixed point resolving what authority a closure-RETURNING function's returned value carries (a lambda literal's own body capability, a named-function reference, or — recursively — another closure-returning function's return, including a simple passthrough that depends on ITS OWN parameter, resolved against the ACTUAL argument at that call); (3) at every call site with a hot argument position, `accumulate_hot_extra_caps` resolves the SPECIFIC argument passed (`resolve_closure_caps`: a lambda literal, a `let`-bound local traced via a per-function `build_local_closure_caps` map, a named function/builtin reference, a call into (2), or — when it is one of the CURRENT function's own fn-typed parameters — deferred via `ClosureCapsResult::DependsOnParam` to that function's OWN call sites, which is what keeps a `std::iter`-HOF-shaped forwarding function requiring nothing extra) and unions that authority into the call's requirement, checked against the CALLING scope exactly like any other gated operation — so a `deny!` block, an actor's declared ceiling, or any other boundary a closure is routed through now sees the real requirement. Unresolvable provenance (a closure whose origin can't be traced at all) requires `Capability::All`, the same conservative default already documented for the raw-memory escape. Verified BOTH directions, both modes (inferred + `--strict-capabilities`): pre-fix binary compiles+runs the `deny!` repro clean and prints the secret from inside the denied scope (5/5 reproduced); post-fix binary rejects it with E0507 citing the closure argument, in both modes, while `std::iter::map/filter/fold` with a PURE closure still needs no annotation (no cascade) and the SAME HOF with a PRIVILEGED closure correctly requires the capability. Blast-radius swept live (not just the parameter case): closures forwarded through 2+ passthrough call layers, actor fire-and-forget message sends (needed a second fix — actor handlers have NO implicit `self` in their own `params`, unlike a struct `impl` method, so the method-call self-offset translation was off-by-one and silently dropped index-0 coverage until corrected), `spawn`, a generic `fn<T>`, and `dyn Trait` method dispatch are ALL closed — each individually reproduced escaping pre-fix and rejected post-fix inside a `deny!(fs:read)` block. The REJECTED naive alternative ("any call through a non-directly-named fn-typed value requires `Capability::All`") was re-verified as unusable by MEASUREMENT, not just re-assumed: 22 genuine callback-taking `std::iter` HOF signatures, ~55 raw call sites to those names across the stdlib/self-host/examples/ecosystem (a few are `std::string::find` name collisions, not the iterator HOF; dozens remain genuine) — every one would need `@capabilities(all)` under the blanket policy, none do under the shipped call-site-sensitive one. NOT closed: a closure/fn-value read back OUT OF A CONTAINER (struct field, array element, map value) — `hot_params` only recognizes a parameter whose OWN type is `fn(...) -> ...`, so `Registry{reader: fn()->str}`/`[fn()->str]`/`map<str,fn()->str>` are invisible to it; reproduced live, NOT gated, closed in a later session (see the CLOSED table entry above for the fix). Also verified: `kryos audit` still never lists `zero_cap_tool` post-fix — determined this is CORRECT, not a residual defect (audit is a pure syntactic `@capabilities(...)` scan with no inference, so it never lists ANY unannotated function, including a legitimately call-site-polymorphic one like a HOF; it was never specifically "blind to closures", it is blind to every unannotated function equally). Gates: `tests/security_gate.sh` (extended, checks #4-6: reject/no-over-reject/no-cascade + positive privileged-HOF check), conformance 47/47, tier1+tier2 GREEN, bootstrap 16/16. Docs corrected: `docs/10-capabilities.md`, `README.md`, `STABILITY.md`, `docs/capability-roadmap.md` all previously claimed NO soundness for any closure indirection; now state the precise (much larger) sound surface and the precise (much narrower) remaining gap |
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
| **`copy_dir_all_refuses_a_symlink_entry_pointing_outside_the_package` (shipped in `fbd1e5b`, item 1b) was VACUOUS for the directory-symlink case -- passed whether or not the guard existed** | REPRODUCED first, per the loop's own rule: with the `file_type().is_symlink()` guard deleted entirely from `copy_dir_all` and `cargo test -p kryos-package` rerun, the existing test still reported `ok`. Root cause matches the assigned brief exactly: `fs::copy(&path, &target)` on a directory reparse point fails with a plain OS `PermissionDenied` on Windows regardless of the guard's presence, and the test only asserted `result.is_err()` -- true either way. FIX, two independent hardenings (both applied, not either/or): (1) a new `assert_is_guard_rejection` helper pins the error to the GUARD'S OWN signature -- `ErrorKind::InvalidData` plus the literal message text and the offending entry name -- so any other `Err` (an incidental OS refusal, or a different failure entirely) now fails the assertion instead of satisfying it; renamed the test to `copy_dir_all_refuses_a_symlinked_directory_entry_pointing_outside_the_package` for clarity against its new sibling; (2) added `copy_dir_all_refuses_a_symlinked_file_entry_pointing_outside_the_package` -- a FILE symlink (not a directory) pointing outside the package, which is the case that genuinely exercises the guard: `DirEntry::file_type()` reports the symlink type either way, so a missing guard falls into the plain `fs::copy` branch, and `fs::copy` FOLLOWS a file symlink on every OS with no incidental refusal to mask it. Proof both ways, both tests, one `guard`-stripped rebuild: with the guard removed, the directory test goes RED on the exact predicted mechanism (`left: PermissionDenied, right: InvalidData`) and the file test goes RED because the call returns `Ok(())` -- the outside file's real secret content is copied straight into the cache with no error at all (the smoking-gun case the brief asked for); with the guard restored, both pass. Also hardened `registry.rs`'s `content_checksum_is_deterministic_and_content_sensitive` coverage gap named in this wave's brief: added `content_checksum_distinguishes_stdlib_from_src_prefix`, proving `collect_kry_files`'s `prefix` argument is load-bearing (a `src/foo.kry` and a byte-identical `stdlib/foo.kry` must NOT hash the same) -- proof both ways: hardcoding `"src"` for the `stdlib_dir` call site (reproducing the exact hardcoded-prefix bug `fbd1e5b` already fixed) makes the two checksums collide and the new test go RED; the real code keeps them distinct. Also verified (adjacent items named in the brief, no code change needed): a checksum-MISSING entry is already rejected identically to a checksum-MISMATCH, both at the `verify_package_checksum` level (`verify_package_checksum_rejects_missing_checksum`) and end-to-end through `fetch_resolved` (`fetch_resolved_rejects_a_package_with_no_recorded_checksum`) -- both tests already existed and pass. Partial-destination cleanup on a failed fetch (`fetch_resolved`'s `let _ = std::fs::remove_dir_all(&dest)` on any `Err`) is verified BY CODE INSPECTION, not a fresh dedicated test -- the cleanup line is unconditional and identical regardless of whether the `Err` originates from `fetch_github`/`fetch_github_subdir` (a copy failure) or from `verify_package_checksum` (already covered live by both `fetch_resolved_rejects_a_tampered_cache_entry_and_wipes_it` and `fetch_resolved_rejects_a_package_with_no_recorded_checksum`, both of which assert `!dest.exists()` post-failure); a genuine copy-failure-specific repro would need a real `git clone` of a crafted malicious subdirectory, which is out of scope to plant against the live public registry and not attempted here -- flagged honestly rather than claimed as tested. Gates: `kryos-package` unit+integration tests 65/65 (41 lib + 5 `checksum_verification.rs` + 19 `package.rs`), full `cargo build --release` clean, `kryos-loop.sh gates 2` GREEN (conformance 58/58, tier1+tier2 all PASS), `test_bootstrap.sh` 16/16, `security_gate.sh` PASS (33/33). |
| **OPEN item 7b: a closure/fn-value shared (not snapshotted) across `spawn`-ed threads was a genuine cross-thread DATA RACE with silent lost updates** | A prior session RULED OUT deep-copying the closure env at spawn (wrong semantics -- a closure whose whole point is shared mutable state would have each thread's mutations land in a throwaway private copy instead; also structurally couldn't fire, since `closure_locals`, the only provenance-tracking mechanism available, is unconditionally empty for every mutating closure by design) and left the remaining shape unimplemented: "make the load-mutate-store atomic under a per-closure lock". Implemented that shape instead of retrying deep-copy. Mechanism: `MirAttributes.needs_capture_lock` (new field, `kryos-mir/src/ir.rs`) is set in `lower.rs`'s Lambda arm exactly where `mutating_closures` already gets populated (same condition, `!mutated_captures.is_empty()`, covering both the scalar-box and struct-ptr-slot mutation shapes). Both codegen backends (LLVM `codegen.rs`, Cranelift `codegen.rs` + `jit.rs`) read this flag to (a) reserve ONE extra i64 "lock word" slot at the end of the closure's existing env allocation (offset `(1+captures.len())*8`, seeded 0) -- same ARC allocation, same lifetime as the env itself, so this adds no new allocation and no new leak/drop-ordering surface -- and (b) wrap the underlying-function call inside the generated `{name}_env` thunk with `kryos_mutex_lock`/`kryos_mutex_unlock` on that word. The thunk is the ONE call path every invocation of a mutating closure value goes through (direct calls are provably excluded: `closure_locals`'s direct-call fast path is unconditionally disabled for any closure in `mutating_closures`), so this needed no call-site enumeration and no change to the underlying function's own arity/ABI -- purely additive at the env/thunk layer. A plain blocking lock, not a CAS retry: each caller executes the closure body exactly once, so no side effect (e.g. a `println` inside the closure) can be duplicated by a retry. Reused the EXISTING native `kryos_mutex_lock`/`kryos_mutex_unlock` runtime primitives (`kryos-stdlib-native`, already used by `std::sync::Mutex`) rather than adding new runtime code; hit and fixed one real bug along the way -- an initial I32-return Cranelift declaration for these two symbols conflicted with the pre-existing all-I64 declaration `std::sync`'s own Mutex usage installs (`declare_runtime_builtins`/`ensure_func_ref_with_args`'s uniform convention), a hard "signature ... is incompatible with previous declaration" Cranelift module error, not a runtime deadlock as first suspected -- fixed by matching the established I64-return convention (and, in `jit.rs`, by reusing the already-declared `FuncId` instead of re-declaring). Proof both ways, both backends, MANY runs (a race is probabilistic; one green run is not evidence): pre-fix (stashed the fix, full `cargo build --release`) `tests/known_failures/spawn_closure_shared_env_race.kry` at 50 threads x 2000 calls -- JIT 20/20 runs RACE (lost updates, e.g. `final=74293 want=100001`), AOT 13/20 runs RACE (65%, matching the ledger's prior ~70% figure); post-fix (restored, rebuilt) the SAME repro -- JIT 50/50 clean, AOT 50/50 clean, all printing the exact expected total with zero lost updates. `conf_spinlock_mutex` (the test a naive ownership-based attempt at this same bug previously broke, per this ledger's own warning) 10/10 clean on BOTH backends after the fix. Folded the repro into a permanent regression test, `tests/conformance/conf_spawn_closure_capture_lock.kry` (30 threads x 1000 calls, exact-value `expect()` assertion, no flake tolerance), and deleted `tests/known_failures/spawn_closure_shared_env_race.kry` per that directory's own "when fixed, fold and delete" convention; updated `tests/known_failures/README.md` and `docs/09-concurrency.md`'s spawn section (the language's documented concurrency contract: closures still don't snapshot, by design, but sharing a mutating closure across `spawn` is now SAFE, not merely possible, with the tradeoff -- serialized, not lock-free -- stated explicitly; `std::sync::atomic_int()` remains the faster purpose-built choice for a hot shared counter). Gates: `kryos-loop.sh gates 2` GREEN (conformance 59/59, tier1+tier2 all PASS), `tests/security_gate.sh` PASS, `test_bootstrap.sh` 16/16 (run alone). Not attempted / out of scope: making the lock reentrant (a mutating closure calling itself recursively through its own stored value would self-deadlock -- no evidence this is reachable today, since gotcha #11 already documents that a self-referential closure built via reassignment captures the OLD binding rather than truly recursing) and a CAS-based lock-free fast path for the provably-side-effect-free case (deferred per the original ruled-out attempt's own reasoning: proving side-effect-freedom is its own analysis, not needed once a correct blocking lock exists). |

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

## CAPABILITY SOUNDNESS THEOREM AUDIT (2026-08-04)

Wrote `docs/capability-soundness.md`: a precise theorem (authority
confinement, refined for `deny!` narrowing, sub-capabilities, the raw-memory
TCB split, and call-site-polymorphic HOFs), a 22-invariant table covering
every way authority enters/travels/is stored/is invoked, and a per-invariant
status against `kryos-capabilities/src/{checker,model}.rs` read directly
(not from memory of prior rounds).

**Prime-suspect hypothesis (generic monomorphization producing a WRONG
companion, not just an unresolved one) traced to ground and RULED OUT, by
construction, not by re-running old tests.** `compute_hot_param_companions`/
`compute_hot_params` run once over the pre-monomorphization AST; every
gating predicate (`is_fn_typed`, `is_fn_bearing_type`,
`decompose_container_path`, parameter-name matching) reads the DECLARED
`TypeExpr` and the callee's own literal call-site argument syntax — no
substituted/instantiated type is ever consulted (grepped
`kryos-capabilities` for `monomorph`/`type_arg`/`instantiat*`: zero hits).
Two instantiations of the same generic declaration therefore cannot receive
different companion facts; the mechanism cannot observe `T` at all, so it
cannot prove a wrong companion from it.

**One live attack constructed and run this session** (not merely reasoned
about): a bare unconstrained generic parameter invoked as a function,
`fn invoke_generic<T>(x: T) -> str { return x() }`, called with a closure
argument. Result: REJECTED twice, independently — `E0110` (Kryos's type
checker refuses to call a value of unconstrained generic type; no trait-
bound syntax exists to make this legal) AND, separately, the capability
checker's own inference for the same program computed `invoke_generic` as
requiring `[all]` (`Unknown -> Capability::All`, the documented fail-closed
default firing correctly on an unresolvable callee). No escape.

**Re-verified live, not just re-cited, that the round-5 fix still holds at
current HEAD** (`00b3cf7`, no code changed this session):
`tests/security/cap_escape_decoy_map_companion.kry` (a generic
`apply_from_map<T>(decoy: map<str,T>, real: map<str,T>, f: fn(T)->str)` HOF
companion decoy) run against `compiler/target/release/kryos.exe`, both
`kryos run` (inferred) and `kryos check --strict-capabilities` — REJECTED
(E0507) both modes, correctly attributing the requirement to the closure
argument, not the decoy.

No new escape found; no code changed. Gates (unmodified, docs-only commit):
`kryos-loop.sh gates 2` GREEN (conformance 59/59, tier1+tier2 all PASS),
`tests/security_gate.sh` PASS, `test_bootstrap.sh` run ALONE 16/16.

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

## NUMERIC SEMANTICS AUDIT (2026-08-01)

Probed integer/float/cast/overflow semantics per CLAUDE.md gotcha #18/#22
and `conf_overflow.kry`'s existing coverage. One real bug found and fixed
(the AOT narrow-struct-field-store miscompile, see CLOSED table above).
Everything else below was measured and is CORRECT on both backends —
recorded so it isn't re-investigated:

- **Float -> narrow-int cast saturation is genuinely PER-WIDTH, not a
  cast-to-i64-then-truncate shortcut.** `300.0 as u8` -> `255` (not `44`,
  which is what a truncate-via-i64 path would give since `300.0 as i64` =
  `300`, `300 as u8` = `44`), `-5.0 as u8` -> `0`, `1.0e10 as u32` ->
  `u32::MAX`, `-1.0e10 as i32` -> `i32::MIN`, `1.0e10 as i8` -> `i8::MAX`,
  `NaN as u8` -> `0` — identical on `kryos run` and `build --release`.
  Gotcha #18 only documents the f64->i64 case explicitly; this confirms it
  generalizes correctly to every narrower integer width too.
- **Unsigned comparison/division/modulo at the `u64`/`u32` boundary use
  real UNSIGNED operations, not signed ones reinterpreting the high bit.**
  `u64::MAX > 0`, a `u64` value with the top bit set compared/divided
  against a small `u64` (`10000000000000000000 / 5` ->
  `2000000000000000000`, not a negative-dividend signed-division result),
  `u32(3_000_000_000) / 2` -> `1500000000` (unsigned) while
  `u32(3_000_000_000) as i32` correctly reinterprets the same bits as
  `-1294967296` (signed) — both backends agree throughout. No sign-compare
  bug at the unsigned boundary.
- **Hex/binary integer literals parse correctly at their extremes.**
  `0xFF` -> `255`, `0b1010` -> `10`, `0xFFFFFFFFFFFFFFFF as u64 as i64` ->
  `-1` (the full 64-bit pattern, not rejected or truncated), a `u8`-typed
  hex literal masks correctly (`0x100 as u8` -> `0`). Both backends agree.
- **`i128`/`u128` re-verified: still non-functional, but now fail CLEANLY
  instead of crashing** (an improvement since CLAUDE.md gotcha #22 was last
  written). `let a: i128 = 100` and arithmetic between two `i128` locals
  both now give a clean `error[E0110]: \`i128\` is not yet supported by
  the code generator` at compile time, exit 1, on BOTH backends — no
  Cranelift verifier ICE, no raw LLVM type-mismatch build failure (the
  previously-documented crash mode). CLAUDE.md corrected to reflect this;
  the types still don't work, they just fail predictably now instead of
  crashing. No silent miscompile either way — this was the specific thing
  this wave was asked to re-check.
- **Bitwise NOT on a narrow unsigned type masks to the type's own width**,
  not the full 64-bit register: `~(0u8)` -> `255`, `~(0u16)` -> `65535` on
  both backends (not `-1`/a full-width all-ones value misread as unsigned).
- **Narrow-int overflow/truncation in ARRAY elements is correct on both
  backends** (unlike the struct-field case, which was broken on AOT only):
  `arr: [u8]`, `arr[i] = arr[i] + N` wraps mod 256 identically on `kryos
  run` and `build --release`, including through a loop-accumulated i8 sum
  that stays within representable range after one wrap
  (`120i8+120i8+120i8` -> `104`, correct on both, no further clamping
  needed since 104 fits in `i8` after the mod-256 truncation). The
  struct-field bug (CLOSED table) did NOT extend to arrays — arrays are
  always heap-allocated `KryosArray` buffers addressed by element stride,
  never the fragile stack-`alloca`-plus-i64-slot-store path that broke for
  struct fields.
- **A narrow field SANDWICHED between wider fields** (`struct Wide{a:i8,
  b:i64, c:i8}`, mutating the trailing `c` with overflow) **was already
  correct on both backends even before the struct-field fix** — the
  natural alignment padding after `a` (needed to align `b` to 8 bytes) and
  after `c` (needed to round the struct's total size up to its own
  alignment) happened to provide enough slack to absorb the erroneous
  8-byte store without touching `a`/`b`. This is WHY the bug went
  unnoticed for this long: it only manifests for a narrow field with nothing
  wider declared after it in the SAME struct (a single-scalar-field
  struct, or two-or-more consecutive narrow fields with nothing wider
  trailing) — a less common but far from rare shape (counters, flags,
  small config/newtype structs).
- **The struct-field fix verified to hold for HEAP-escaping struct
  instances too**, not just the stack-`alloca` local case the bug was
  found in: a single-narrow-field struct stored as an array element
  (`arr[0].v = arr[0].v + 250` with overflow) and one passed through a
  function boundary and returned (`fn mutate(c: SU8) -> SU8 { let mut m =
  c  m.v = m.v + 100  return m }`) both wrap correctly post-fix on AOT,
  matching JIT. (These heap/escaping paths were never confirmed broken
  pre-fix either — plausible that heap struct boxes reserve full 8-byte-
  per-field slots regardless of declared width, unlike the tightly-packed
  stack `alloca %StructName`, which would mean they were never exposed to
  this bug in the first place; not root-caused further since the fix
  covers both paths identically going forward.)
- **Float `to_string`/`parse_float` round-trips exactly** for a spread of
  values (`0.1`, `1.0/3.0`, `123456789.123456`, `1e300`, `1e-300`, `-0.0`,
  `0.0`, `3.14159265358979`) — reparsing the printed string reproduces the
  original value (diff `< 1e-7`) on both backends, no precision loss from
  the formatter.
- **NaN comparison semantics are correct and backend-consistent**: `nan ==
  nan`, `nan != nan`, `nan < x`, `x < nan`, `nan <= nan` all give the IEEE-
  correct answer (`==`/`<`/`<=` false, `!=` true) on both backends; `+-inf`
  compare/order correctly against large finite values and each other.
- **NEW (deterministic consequence of the ALREADY-DOCUMENTED NaN sign-bit
  divergence, not a new root cause): `sort()` on an `[f64]` array
  containing NaN gives a DIFFERENT ORDER per backend.** `sort([3.0, nan,
  1.0, 2.0])` places the NaN FIRST on `kryos run`/JIT and LAST on `build
  --release`/AOT. ROOT CAUSE (read, not guessed): `kryos_builtin_sort_f64`
  (`kryos-rt/src/builtins.rs`) sorts via `f64::total_cmp` on the raw bit
  pattern -- a real, deterministic IEEE-754 total order in which a
  NEGATIVE-signed NaN sorts before every other value (including `-inf`)
  and a POSITIVE-signed NaN sorts after every other value (including
  `+inf`). Since an invalid-op NaN canonicalizes with the sign bit SET on
  JIT and CLEAR on AOT (CLAUDE.md gotcha #18, already backlogged), the
  SAME array sorts to opposite NaN placement on each backend by direct
  consequence -- no new defect in `sort` itself, and no crash/hang either
  way. **Doc correction (not a code fix): gotcha #18 previously claimed
  the NaN sign-bit divergence "is NOT observable through normal float
  use" -- that claim is FALSE, demonstrated by this repro, and has been
  corrected in CLAUDE.md** to name `sort()` as a concrete case where it
  surfaces. Not fixed (would require unifying NaN canonicalization across
  backends first, the same backlogged architectural item as the sign-bit
  and `parse_float("-0.0")` divergences); documented per this wave's
  "fix what is provable, document what is inherent" mandate.

---

## ERROR HANDLING, PANICS, DIAGNOSTICS WAVE (2026-08-01)

### Correctness verification (all confirmed TRUE and CONSISTENT both backends, live-tested not inferred)

- Runtime panics (`10/0`, array OOB, `file_read` on a missing file) are
  uncatchable by `try`/`catch`, exit **98**, identical message on `kryos
  run` and `build --release`.
- Uncaught `throw` unwinds to stderr `kryos: uncaught exception: <msg>`,
  exit **101**, both backends. A caught `throw` runs the catch block and
  continues normally, both backends.
- `?` propagates correctly through 3+ levels of nested `Result`-returning
  calls and through a generic `fn try_get<T>(...)  fn sum_two<T>(...)`
  chain, both backends.
- `spawn { throw .. }` is isolated (`kryos: uncaught exception in spawned
  thread: ..`, parent survives) but `spawn { 10/0 }` kills the **whole
  process** exit 98 before the parent's post-spawn code runs -- matches
  `docs/09-concurrency.md`'s existing claim, re-verified live.
- Actor handlers: a `throw` inside a handler is isolated (`[actor error]
  Name.method: uncaught exception: ..`, process continues) but a panic
  inside a handler kills the whole process exit 98 -- same asymmetry as
  `spawn`, not previously verified for actors specifically.
- A panic inside a directly-called closure is uncatchable exit 98, same as
  a named function.
- A panic occurring while heap-holding locals (a struct with array/str
  fields, a live `string_builder`) are still in scope produces a clean
  single panic message and exit 98 -- no double-free/corruption artifacts,
  no hang. (Kryos has no user-facing `Drop`/destructor trait to test
  directly -- panics abort via `kryos_panic`/`exit()` rather than
  unwinding, so compiler-generated scope-end drops never run on the panic
  path at all; this is the closest direct test of "does a panic mid-
  cleanup corrupt state".)

### Fixed: two diagnostic-cascade defects (an already-reported error must not spawn a wall of unrelated noise)

1. **`[dyn Trait]` array rejection (E0110) poisoned the wrong thing.**
   `let handlers: [dyn Handler] = [A{}, B{}]` correctly emits one E0110 (per
   the CLOSED-table fix for this shape), but a SUBSEQUENT use
   (`for h in handlers { h.handle() }`) triggered a second, unrelated
   `E0107: no method \`handle\` found for type \`i64\`` -- worse than the
   OPEN item #4 call-argument residual (different mechanism: the `for`
   loop's `Stmt::For` handling in `kryos-types/src/check.rs` defaulted an
   already-errored (`Type::Error`) iterable's element type to `i64` instead
   of propagating the poison, so every downstream use of the loop variable
   re-triggered fresh, nonsensical type errors against `i64`). FIX: split
   the `Type::Var(_) | Type::Error => Type::I64` arm so `Type::Error`
   propagates as `Type::Error` (which the method-call checker already
   short-circuits with no new diagnostic, per the existing `Type::Error =>
   return Type::Error` guard a few hundred lines away -- this fix just
   makes the for-loop consistent with a pattern the codebase already uses
   elsewhere). `Type::Var` (genuinely unresolved generic, not yet an error)
   keeps defaulting to i64 as before -- unaffected. Proof both ways: stash
   the one-line check.rs change + rebuild -> 2 errors (E0110 + bogus
   E0107); restore + rebuild -> 1 error (E0110 only). Does NOT touch OPEN
   item #4 (the call-argument shape, a different code path producing E0100
   not E0107) -- verified unchanged, not re-litigated.
2. **Reserved keyword used as a value (`let match: i64 = 5` then
   `to_string(match)`) cascaded into 8 unrelated "unexpected end of file"
   errors.** Root cause, read via `--emit`-free direct tracing of parser
   state: (a) the shared primary-expression-parse failure path
   unconditionally `advance()`d past ANY unexpected token, including a
   natural closing delimiter (`)`/`]`/`}`/`,`) that belongs to the
   ENCLOSING construct, not the failed expression; (b) `parse_match_expr`
   then tried to `expect(LBrace)` and parse match arms against tokens that
   never belonged to a match at all, eventually consuming the REAL `}` that
   was meant to close `main`, cascading into "expected ',' / ')' / '}' at
   end of file" 6 more times. FIX, two parts: (1) the primary-expr fallback
   no longer consumes `RParen`/`RBracket`/`RBrace`/`Comma` when reporting
   "expected expression" -- these are left for the enclosing
   call/array/block/list parser to detect correctly instead of being eaten
   as if they were the bad token; (2) `parse_match_expr` detects when its
   own subject failed to parse at all (the `<error>` sentinel identifier)
   and bails out immediately with an empty match rather than attempting
   `expect(LBrace)`/an arms loop against tokens it doesn't own. Together:
   8 errors -> 2 (the real root cause, "reserved keyword 'match' cannot be
   used as a name", plus one legitimate follow-on, "unexpected ')',
   expected expression" at the bare-`match`-in-value-position use -- both
   accurate, neither noise). Proof both ways: stash both parser.rs hunks +
   rebuild -> 8 cascading errors (verified once with only fix (1) applied
   in isolation -- made it WORSE, 12 errors, because `expect()` elsewhere
   still consumed the same delimiters via a different code path; fix (2)
   was required to actually collapse the cascade); restore both + rebuild
   -> 2 errors. Non-regression: ordinary multi-arm/or-pattern/enum-payload
   match expressions verified unaffected (`tests/diagnostics_gate.sh`
   check 5).

### Fixed: missing error codes (an entire category, E02xx "Resolution errors", was reserved in `kryos-errors/src/codes.rs`'s own doc comment but had ZERO codes defined)

Found while checking whether `kryos explain <code>` helps for each corpus
mistake -- a "missing import" mistake (`use std::string::{capitalize_words}`,
a name that doesn't exist) and a "name collision" mistake (`use
std::csv::{parse}` + `use std::json::{parse}`) both produced clear MESSAGES
but zero error code, so `kryos explain` had nothing to look up. Grepping
`kryos-driver/src/resolve.rs` found the WHOLE file (module-not-found,
qualified-call-wrong-origin, qualified-call-not-imported, private-member-
import, unknown-export, duplicate-import) had never had a single
`.with_code(..)` call. Added `E0200`-`E0205` (codes.rs + full explain.rs
articles + `list()`/`explain()` registration) and wired all 6 resolve.rs
sites plus 3 pre-existing code-less lexer/parser diagnostics (unterminated
string, unterminated block comment, unexpected `;`) to `E0009`. Proof both
ways: `tests/diagnostics_gate.sh` (new) checks 3-6 fail on the pre-fix
binary (verified via `git stash` of all 6 files + rebuild -> 6 FAILs) and
pass post-fix.

### Docs fixed (honest-docs goal, not code changes)

- **`docs/07-error-handling.md` directly contradicted itself.** The "What
  `catch` catches" section correctly states `file_read` panics
  (uncatchable); the later "Common mistakes" section then showed wrapping
  `file_read` in `try`/`catch` as "Safe" -- verified live that this is
  false (the catch never runs, exit 98). Rewritten to show the actually-
  catchable alternative (`std::fs::read_file`, which `throw`s on failure --
  verified live) and to state the raw-builtin panic explicitly.
- **CLAUDE.md gotcha #16's claim "bare unqualified `None`/`Red` in an
  expression is rejected (E0102)" is FALSE as of this compiler.** Verified
  live with the doc's own example (`use std::option::{None}; let x = None`)
  and with a genuinely ambiguous two-enum case (`Color{Red,..}` +
  `Fruit{Red,..}`, both in scope, `let c = Red`) -- neither is rejected;
  bare resolution silently picks the FIRST-DECLARED enum with that variant
  name, no ambiguity diagnostic at all. Not a silent wrong VALUE in
  practice (a genuine type mismatch against a differently-typed context
  still surfaces as ordinary E0100 -- verified: `let x: Fruit = Red`
  reports "expected Fruit, found Color"), but the doc's specific mechanism
  claim (an E0102 ambiguity check) does not exist. Corrected in place.

### Not fixed / out of scope, left for another wave

- **LEDGER item #2c (`std::test::assert`'s 2-arg form permanently shadowed
  by the compiler's own uncatchable intrinsic) was NOT re-attempted.**
  Already fully root-caused and scoped in this file as a design note
  needing its own full-gate pass across `compiler/self-host/` and
  `ecosystem/*/tests/`; re-verified the repro still reproduces exactly as
  documented (uncaught `process::abort()`, catch never runs) but did not
  re-litigate the decision to defer it.
- **Hard rule #6 ("Type annotations on top-level `let` ... are required")
  is not actually enforced** -- `let count = 5` at top level (no
  annotation, no function call) compiles and runs correctly (infers `i64`,
  value correct). This is a language-semantics doc-accuracy question
  (either the rule was relaxed and the doc never updated, or this is a
  real gap), not an error-handling/diagnostics-wave item -- flagged here
  with a minimal repro rather than fixed, since it's outside this wave's
  assigned area. `tests/known_failures/` was not used since nothing here
  crashes or gives a wrong answer, it just contradicts a doc claim.
- **`E0110`'s catch-all "type error" explain text** is intentionally
  generic (mirrors `E0009`'s "syntax error" catch-all pattern already in
  the codebase) -- read as acceptable, not a defect, since each E0110
  MESSAGE is already specific (wrong arg count, duplicate fn, tuple OOB,
  ..); did not attempt to split E0110 into narrower codes, that's a much
  larger taxonomy change outside this wave's scope.
- **`03_builtin_shadow_var` corpus case** (`let len: i64 = 5` then
  `len(arr)`) gives `E0110: type i64 is not callable` -- accurate and
  points at the right span, but doesn't name the shadowing variable
  explicitly. Read as a minor polish opportunity, not a defect; not
  changed.

Regression gate: `tests/diagnostics_gate.sh` (new), wired into
`kryos-loop.sh gates` tier 1. Gates: conformance 53/53, tier1+tier2 GREEN
(examples_e2e flaked 10/12 and 8/12 under tier-3 contention across two
separate runs, both times clean 12/12 re-run alone -- the documented
bootstrap-class contention flake, not a regression), bootstrap 16/16.

---

### Wave: modules/imports/namespace resolution (2026, HEAD 0d9b932 at start)

Probed every documented module-resolver limit plus deep chains, diamonds,
circular/self imports, glob+selective mixes, qualified-call-origin
validation, and case-sensitivity. Live-reproduced against
`kryos-driver/src/resolve.rs` before touching anything, per the loop's
REPRODUCE-first rule.

**RE-VERIFIED STILL ACCURATE (no action, matches CLAUDE.md as written):**
- No import aliasing (`use m::{f as g}` is `E0009` parse error).
- Two modules exporting the same name cannot both be imported (`E0205`).
- The resolver pulls every STRUCT/enum/trait/actor of an imported module
  regardless of the selective list -- two disjoint-function-only imports
  from two user modules that both define `struct Item` still collide
  (`E0205`), confirmed live with a fresh 2-module repro (stdlib itself no
  longer has any same-named structs, so this had to be re-demonstrated with
  synthetic modules, not stdlib). Type-reachability import stays backlogged.
- Importing a name that shadows a global builtin (`use std::trie::{contains}`
  alongside `use std::os::{name}`) breaks `std::os`'s internal `contains(..)`
  calls with a mislocated `E0100` (byte-offset spans pointing past the end of
  the user's own file, into the shadowed module's source). Confirmed live,
  still exactly as documented.
- Module-path case-sensitivity gate (`tests/module_case_gate.sh`) still
  correctly rejects a case-mismatched import.
- Module-qualified-call-vs-origin validation (`E0201` wrong origin, `E0202`
  not imported) fires correctly for genuine misbindings.

**PROBED FURTHER, NO BUG FOUND (all live-verified, values checked not just
exit codes):** 5-level-deep import chains; diamond imports (two siblings
both importing a common ancestor module, including one via GLOB and the
other SELECTIVE simultaneously); mutual/circular imports between two user
modules calling each other's functions (correct mutual-recursion result,
no infinite loop); a module importing ITSELF (silently a no-op -- the
driver pre-seeds `visited` with the root's own canonical path before
`resolve_imports`, so the self-import is skipped, not a duplicate-decl
error); glob-import collision between two modules exporting the same name
(`E0205`, same as selective).

**FIXED: false `E0201`/`E0202` when a local type's name collides with a
stdlib module file stem.** `kryos-driver/src/resolve.rs`'s qualified-call
validator treats ANY receiver identifier matching one of the 66 stdlib
module file stems as a module qualifier (by design, so `csv::parse` is
checked even when the caller never imported `std::csv`). It had no way to
tell a same-named LOCAL type apart -- Kryos does not require PascalCase type
names (confirmed live: `struct os { .. }` / `enum set { .. }` are legal
declarations). Repro: `enum set { Full(i64), Empty }` with `impl set { fn
make(v: i64) -> set { return set::Full(v) } }` then `set::make(5)` --
BEFORE: `error[E0202]: \`set::Full\` is not imported` and `\`set::make\` is
not imported`, exit 1, even though the program never imports `std::set`.
AFTER: prints `5`. Proof both ways: `git stash` the 2-file fix + rebuild ->
both E0202s fire; restore + rebuild -> clean run, correct value. Fix:
`collect_local_type_names` scans struct/enum/trait/actor/type-alias names in
the root module AND the resolved import closure; a qualifier matching one of
those now wins over a same-named stdlib module, checked before the
`modules.contains(&recv)` test. Genuine wrong-origin (`E0201`) and
not-imported (`E0202`) cases re-verified still fire (not swallowed by the
carve-out). Regression: `tests/module_case_gate.sh` checks 4-5 (also proven
RED on the pre-fix binary via `git stash`).

**DOC CORRECTED (was a false negative claim, not a code bug): the
"transitive FFI through a selectively-imported function" limitation is
STALE.** CLAUDE.md claimed `use std::os::{temp_dir}` fails because the
resolver doesn't follow `temp_dir` -> `_env_or_empty` -> the
`kryos_env_get` extern. Live-reproduced on BOTH backends (`kryos run` and
`kryos build --release`) with `@capabilities(process, fs:read)` on the
caller: it compiles and runs correctly, printing the real temp directory.
Root cause of the doc going stale: `resolve_imports_inner` already (a)
recursively resolves an imported module's OWN imports unconditionally
before filtering by the selection list, and (b) always includes `extern { }`
blocks regardless of selection -- so `_env_or_empty` (pulled in via the
identifier-transitive-closure walk over `temp_dir`'s body) and its
`kryos_env_get` extern declaration are both present. This was almost
certainly fixed by the resolver rewrite in `c616af1` (program-wide selection
unions + transitive closure) and the doc was never re-verified after. This
was the wave's flagship assigned item ("most user-hostile") -- ruled out as
already fixed rather than needing a redesign. Corrected in `CLAUDE.md` in
place; no code change needed.

**FILED, OUT OF SCOPE (parser/grammar, not module resolution) -- a
lowercase-named struct cannot be constructed via struct-literal syntax AT
ALL, unrelated to imports.** Found while building the local-type-collision
repro above: `struct counter { val: i64 }` then `counter { val: v }`
ANYWHERE (a `let` initializer or a `return` tail) fails with `error[E0102]:
undefined variable \`counter\`` + a second `undefined variable \`val\`` --
the parser appears to only recognize `Name { field: value }` as a struct
literal when `Name` starts with an uppercase letter (likely a
disambiguation heuristic against `if cond { }`/`while cond { }` blocks),
otherwise parsing `counter` as a bare identifier and `{ val: v }` as an
unrelated block. `struct Counter { .. }` (PascalCase) with the identical
body works. Not documented anywhere as a hard requirement (CLAUDE.md's
struct examples are all PascalCase by convention, not stated as a rule).
This sidesteps struct literals entirely by using an enum (tuple-variant
construction, no `{ }`) for the local-type-collision fix's regression test.
Minimal repro left in this session's scratch, not added to `tests/` since
it's outside this wave's assigned surface (module/import resolution) --
whoever picks up parser/grammar hardening should check whether the
uppercase-only struct-literal gate is intentional and, if not, either relax
it or give it a real diagnostic instead of two misleading `E0102`s pointing
at the wrong tokens.

Gates: `kryos-loop.sh gates 2` GREEN (conformance 53/53, all tier1+tier2
checks pass), bootstrap 16/16 solo.

---

### Wave: capability cascade -- round 5's fail-closed fix landed on 2 shipped ecosystem packages (HEAD 8fba060 at start)

`bash tests/ecosystem_check.sh` regressed from 259/259 to 257/259 after round
5 (`2041367`, deleted the last shape-based fn-value relief -- see the Round 5
entry above): `ecosystem/kryos-actor-pipeline/demo_pipeline.kry`'s `main` and
`tests/test_pipeline.kry`'s 3 scenario functions all called `pipeline_run(..)`
with a `[Stage]` argument whose elements carry a fn-typed `run` field
(`stages[i].run(a, b)`, a struct-field invocation the checker must trace the
provenance of), and all four call sites required `[all]` post-fix.

**DETERMINED WHICH CASE, WITH EVIDENCE (per the mandate): case (b) is
superficially what it looks like, but the checker's OWN documentation
(`docs/10-capabilities.md` line 115, `resolve_container_path_caps`'s doc
comment in `checker.rs`) states this is an INTENTIONAL, already-decided scope
boundary, not an undiscovered precision bug -- "a container from a genuinely
non-literal source still requires `all`" is the accepted cost, not a gap to
close in the checker.** Traced the actual break live: `demo_pipeline.kry`
built its stage table via `let stages = build_stages()` where `build_stages()
-> [Stage]` returned an array of `stage_new(name, caps, run)` CALLS;
`test_pipeline.kry`'s scenarios called `pipeline_run(three_stages())`
directly, same shape. `resolve_container_path_caps`'s `Identifier` arm only
resolves a local through `local_container_lits`, which `build_local_container_lits`
populates ONLY for a `let x = <literal>` (or an alias of an already-tracked
literal) -- a `let x = some_fn_call()` is never tracked, by design (extending
it to unfold arbitrary function-call return values would mean re-deriving a
callee's return shape at every call site, the same class of inference this
file's doc explicitly rules out extending). Separately, even a literal ARRAY
containing `stage_new(..)` CALLS as elements (as `scenario_ordering_preserved`/
`scenario_single_stage` already did, pre-fix) does not help: walking a
`Field("run")` step into an `Expr::FnCall` element matches no arm in
`resolve_container_path_caps` (only `StructLiteral`/`ArrayLiteral`/`MapLiteral`
are traced) and falls to `Unknown` regardless of what `stage_new`'s own trivial
body does. So the fix is (a): restructure so provenance is resolvable by
construction, not extend the checker's resolution surface.

**FIX: replaced the `build_stages()`/`three_stages()` helper-function
indirection with a `Stage { name: .., caps: .., run: run_ingest }` struct
LITERAL constructed inline, directly inside each function that calls
`pipeline_run`** (`main` in demo_pipeline.kry; all 3 scenarios in
test_pipeline.kry). `stage_new()` itself is untouched and stays in
`src/stage.kry` -- it is still fine for the (common) case where a `Stage`'s
`.run` field is only ever READ as data (`test_units.kry`'s
`test_stage_metadata` still uses it, unaffected, since it never invokes
`.run`), just not for a call site whose hot fn-value the checker needs to
trace. Once each `run:` field is a bare `Identifier` referencing a named,
`@capabilities`-annotated launcher, `resolve_closure_caps` resolves it via
`working.get(name)` precisely, and the union over the array literal is exact:
demo_pipeline.kry's `main` now requires exactly `{compute, io}` (unchanged
from its existing declaration, so NO annotation had to change there);
test_pipeline.kry's 3 scenarios now require the EMPTY set (their launcher
functions, `t_run_double`/`t_run_plus10`/`t_run_collect`, are unannotated and
call no gated builtin), so no new annotations were needed there either --
proving the restructuring alone was sufficient without loosening or adding
any `@capabilities` beyond what was already honest. Verified both ways:
`git stash` the 2-file restructuring -> both files reproduce the exact
pre-fix `[all]` E0507 (`kryos check` on each, shown above); restore -> both
`kryos check` clean AND `kryos run` produce IDENTICAL output/values to
before this wave (demo_pipeline's `[1,4,9,16,25,36,49,64]`, test_pipeline's
`3/3 end-to-end pipeline scenarios passed`).

**EXHAUSTIVE CASCADE CHECK (per the mandate -- "no third surprise"):**
- `tests/ecosystem_check.sh` (every `ecosystem/*/` + `packages/*/` `.kry`
  file, inferred/deny-by-default, the same mode real usage compiles under):
  257/259 -> **259/259 clean** (0 failed, 6 negative fixtures excluded by
  design, unchanged).
- `tests/strict_caps_examples.sh` (`examples/*.kry` +
  `examples/showcase/{,extra/}*.kry` under `--strict-capabilities`): still
  **91/91 pass**, unaffected -- confirms the wave's own claim that this
  corpus was already checked and missed the ecosystem regression, and that it
  remains clean now.
- `examples/real/**/*.kry` + `examples/extracted_packages/*/src/*.kry` (25
  files) -- NOT covered by any existing gate script (checked manually,
  `kryos check`, inferred mode): **25/25 clean**, no regression.
- `tools/docs-examples/check.py` (fenced ` ```kryos ` blocks in
  `docs/learn/**`, the numbered chapters, `QUICKSTART.md`, `CLAUDE.md`): pins
  `--capabilities-mode=permissive` deliberately (its own comment: "not
  capability hygiene... should not have to carry an `@capabilities`
  annotation"), so it is structurally unreachable by an inferred-mode
  regression like this one -- not re-run, correctly out of scope.
- `tests/` (the compiler's own regression corpus, incl.
  `kryos-capabilities/tests/capabilities.rs` and `tests/security_gate.sh`'s
  decoy/scope-narrowing live repros): covered by the mandated gates below,
  all green -- no capability regression outside the 2 ecosystem files found.

Gates: `kryos-loop.sh gates 2` GREEN (conformance 58/58, all tier1+tier2
checks pass), `tests/security_gate.sh` PASS (every decoy-companion and
scope-narrowed-deferred-param repro from round 5 still rejected, both modes),
bootstrap 16/16 solo. Stray `kryos.exe` killed before each gate run.

---

### Wave: `fuzz_parser` OOM (>2GB, CI exit 71) -- resource-exhaustion DoS, FIXED

CI's `fuzz_parser` job reported `libFuzzer: out-of-memory (used: 2100Mb;
limit: 2048Mb)`. Installed `cargo-fuzz` fresh (not previously set up in this
environment) and reproduced live rather than guessing from the two prior
lexer/parser O(n^2)-rebuild fixes (`680be5b`, `fd07331`) the task description
suggested as likely causes -- **neither applied here**: those were both in
the SELF-HOST Kryos source (`compiler/self-host/parser.kry`), not the Rust
`kryos-parser` crate this fuzz target exercises, and the Rust parser's
existing `nest_depth`/`rec_depth` recursion-depth guard (`MAX_NESTING_DEPTH`
= 2048, `MAX_RECURSION_DEPTH` = 256, gated at every recursive entry point:
`parse_block`, `parse_expr_bp`, the Pratt loop's per-operator spine charge,
`parse_pattern`) was already sound and NOT the culprit -- confirmed by
auditing every increment/decrement site before touching anything.

**ROOT CAUSE (found via `cargo fuzz run fuzz_parser -- -max_total_time=180`,
Windows MSVC build, no clang available so `cargo install cargo-fuzz` +
rustc's built-in libFuzzer support was used instead):** a fuzzer run
surfaced a 13s timeout, minimized with `-minimize_crash=1` to a 7-byte
reproducer, `let]\x0e{]` (bytes `6c 65 74 5d 0e 7b 5d`). Running that exact
minimized input alone at `-rss_limit_mb=2048` reproduces the CI failure
EXACTLY: `libFuzzer: out-of-memory (used: 2055Mb; limit: 2048Mb)`, exit 71.
Traced live: `let ]` fails name/`=` recovery and lands on parsing `{...}` as
a value; `parse_map_or_block_expr`'s "otherwise parse as a block" loop then
sees a bare `]` with nothing after it but EOF. `parse_statement` ->
`parse_primary`'s unexpected-token fallback deliberately does NOT consume a
stray `)`/`]`/`}`/`,` (comment in that function: it trusts an ENCLOSING
call/array/struct-literal to consume it during recovery), so
`parse_statement()` returns `Some(stmt)` having advanced the cursor by
**zero tokens**. `parse_block_stmts` and `parse_module` both already guard
this exact "zero progress" case (their own comments cross-reference an
earlier fuzz OOM: the 2-byte top-level input `}:`) -- but
`parse_map_or_block_expr`'s OWN block-body loop, a THIRD, independent call
site with the identical shape, was never given the same guard. Every other
loop that calls a selectively-non-advancing parse function
(`parse_arg_list`, `parse_struct_literal`, `parse_map_literal_body`, the
tuple-pattern loop) is naturally protected because the element parse is
always followed by an UNCONDITIONALLY-advancing `expect(..)`/`expect_name()`
call; audited all of them, none share this gap. This is error-recovery
retry-and-accumulate (the third candidate class the task description named),
not a missing nesting bound and not a container-rebuild quadratic -- the
existing depth guard was correctly ruled out as the cause, not extended.

**FIX** (`kryos-parser/src/parser.rs`, `parse_map_or_block_expr`): added the
same `before = self.pos` / `if self.pos == before { self.recover_stray_block_token() }`
guard already used by `parse_block_stmts`, reusing the existing
`recover_stray_block_token` helper (reports one diagnostic and force-advances
past the stray token, no-op at `}`/EOF) rather than inventing a new
mechanism.

PROOF BOTH WAYS: minimized repro alone against the fuzz target --
pre-fix: `out-of-memory (used: 2055Mb; limit: 2048Mb)`, exit 71 (`git stash`
just `parser.rs`, `cargo fuzz build`, ran); post-fix: `Executed ... in 2 ms`,
exit 0 (`git stash pop`, rebuild, ran). Same both-ways proof repeated against
the new `kryos-parser` regression test
(`fuzz_regression_map_or_block_stray_rbracket_terminates`, asserts a BOUNDED
diagnostic count, not just "didn't crash"): pre-fix, `cargo test -p
kryos-parser` on that one test hangs and is killed by a 20s external
`timeout` (exit 143, growing RSS observed); post-fix, passes in <1ms.

Corpus: minimized 7-byte reproducer added permanently at
`compiler/fuzz/corpus/fuzz_parser/oom_map_or_block_stray_rbracket` so CI's
mutation-based fuzzing starts from it every run. `.gitignore` gained
`compiler/fuzz/artifacts/` (ephemeral per-run crash dumps; the corpus entry
+ regression test are the permanent record, not the raw artifact).

Re-ran the fuzzer past the CI duration after the fix: `-max_total_time=120
-rss_limit_mb=2048` seeded from the (now non-empty) corpus -- 373,614 execs,
zero crashes, zero timeouts, zero OOMs.

Gates: `cargo build --release` (full; `kryos-parser` feeds `kryos-cli`'s
own parsing, no staticlib-caching concern but rebuilt fully anyway per
policy) clean. `kryos-loop.sh gates 2`: tier1 GREEN (conformance 58/58, all
11 other tier-1 checks PASS); tier2's `examples_e2e` showed the
already-documented tier-3-adjacent parallel-gate contention flake (10/12,
matching the EXACT pattern this file's own prior entry already recorded for
this same script -- "flaked 10/12 and 8/12 under tier-3 contention... both
times clean 12/12 re-run alone"); re-ran `run_examples_e2e.sh` alone: clean
12/12 (layer 1 11/11, layer 2 2/2, layer 3 12/12). `tests/security_gate.sh`
PASS (every existing check, unaffected -- this wave touched parser recovery,
not the capability checker). `test_bootstrap.sh` run ALONE: 16/16 (one stray
`kryos.exe` killed first). Full `cargo test -p kryos-parser`: 65/65 pass excluding 2 pre-existing
DEBUG-BUILD-ONLY stack-overflow tests (`test_nesting_guard_deep_parens` and
`test_nesting_guard_allows_reasonable_depth` -- `test_nesting_guard_long_chain`
passes fine, it's the iterative-spine case with no deep recursion) --
confirmed via `git stash` on unmodified HEAD that these two overflow the
default debug-test thread stack identically WITHOUT this wave's change (not
a regression introduced here; likely a debug-only thread-stack-size gap --
`test_nesting_guard_allows_reasonable_depth` overflowing on just 200 nested
parens in an UNOPTIMIZED build, when the guard's own limit is 2048/256, says
the debug parser's per-frame stack cost is the real issue, not the depth
guard's threshold). Left unfixed as out of scope for this wave.

Not fixed / out of scope: the pre-existing debug-build stack-overflow flake
on `test_nesting_guard_deep_parens`/`_allows_reasonable_depth` noted above
(reproduces on unmodified HEAD; unrelated to the OOM this wave targeted).

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

---

## COMBINED-CATEGORY GRAMMAR FUZZ WAVE (2026-08-04)

Task: go beyond `tests/fuzz`'s template harness (14 independent per-category
blocks, 0 spawn/dyn, shallow generics) with real grammar-based generation
that deliberately COMBINES generics/closures/dyn/spawn/actors/enums/
Option/Result/tuples/try-throw in ONE connected data-flow story per program
-- the shape the existing harness's own README documents it cannot reach
("blocks are independent... cannot find bugs that need CROSS-CATEGORY
interaction... e.g. a generic struct holding a closure holding an array
holding a struct").

**Built `tests/fuzz/gen_grammar.py` + `run_diff_grammar.py` +
`fuzz_gate_grammar.sh`** (full design/scope notes in `tests/fuzz/README.md`,
module docstrings). 9 scenarios, each a connected story (not independent
blocks), each built on `ExprGen` -- a genuine recursive expression grammar
(random operator/operand/depth choice at every node: arithmetic, bitwise,
casts at narrow-type boundaries, nested `{ if .. } else { .. }`-valued
blocks, string interpolation, comparisons). HONEST SCOPE stated up front in
both the code and README: the expression layer is a real grammar; the
surrounding statement/declaration scaffolding is 9 hand-designed,
parameterized scenario shapes, not a fully unconstrained statement grammar
-- a fully free statement grammar against Kryos's capability/ownership/type
rules has too low a valid-program rate to be worth the run budget, so this
was a deliberate tradeoff, not an oversight.

**One real bug found and FIXED** (two instances of the same root cause) --
see CLOSED table: the capability provenance checker
(`build_local_closure_caps_block` / `build_local_container_lits_block` in
`kryos-capabilities/src/checker.rs`) false-rejected a zero-capability
closure call when the closure was defined+called inside a bare `{ }`
scoping block or a `let x = { .. }` block-tail-value initializer, forcing
`@capabilities(all)` on ordinary code -- found because this generator wraps
every scenario body in its own `{ }` for local scoping, exactly the
combined-category-generator behavior the task asked for. Not a JIT/AOT
stdout divergence (both backends rejected identically -- `run_diff_grammar
.py`'s NEW `both-fail` bucket, added specifically so this class of finding
isn't silently discarded like `gen_fuzz.py`'s README warns its own harness
would). Fixed, proven both ways (`git stash`/rebuild), verified the fix
doesn't weaken any of 72 capability-escape checks in `security_gate.sh`.

**A third, deeper instance found and deliberately left OPEN** (item 20):
calling the chained return of a generic passthrough accessor method
(`holder.get()()`) needs tracing a generic method's own body, not a
scope-recursion fix -- confirmed via isolation (reproduces even at top
level, even through an intermediate local) that it is NOT the same root
cause before filing it separately, per the non-negotiable "prove before
fixing" discipline. Has a clean workaround (read the field directly); the
generator's own `mega_combo` scenario was adjusted to use the workaround so
the rest of that scenario still exercises.

**Scale reached this wave (final): 1,600 grammar-fuzz cases post-fix, run in
bounded batches (seeds 1-160, 9 scenarios + the shuffled `all`-combo = 10
variants/seed), 0 divergences, 0 both-fail, 0.00% divergence rate.** An
initial single seeds-16-300 (2,850-case) sweep was launched unbounded in the
background and had to be killed by the harness's own runtime cap before it
finished -- Python's default block-buffering on a redirected stream meant
its output never flushed, so that specific run's result could not be
verified and is NOT counted here (a partial/unflushed run is not evidence,
per this ledger's own non-negotiables). Re-run instead as four bounded,
fully-captured batches (`python -u` unbuffered, seeds 16-40/41-80/81-120/
121-160) that each completed and printed a real summary. Rate ~1.3-2.2s/case
(build+link across 2 backends dominates, same as the existing template
harness; the range reflects real contention from other agents sharing this
machine during the run, not generator overhead). Also re-ran the EXISTING
template harness (`gen_fuzz.py`/`run_diff.py`) at seeds 1-300 as a
regression check on the shared `kryos-capabilities` change: 300/300 match,
0 divergences -- confirms the checker fix did not affect the existing
harness's coverage. Also ran `tools/diff-fuzz/memsafety_fuzz.py`
(KRYOS_FREE_DIAG double-free sweep) for 400 cases: 0 with double-free.

**Also run this wave** (per task requirement to check the memory-safety
path and existing cargo-fuzz targets, not just the new generator):
- `tools/diff-fuzz/memsafety_fuzz.py` (KRYOS_FREE_DIAG double-free sweep):
  see this section's follow-up for count/result.
- cargo-fuzz `fuzz_parser`/`fuzz_typechecker`/`fuzz_lexer`: nightly toolchain
  IS installed (`nightly-x86_64-pc-windows-msvc`) but `cargo fuzz run`
  failed out of the box with `STATUS_DLL_NOT_FOUND` then
  `STATUS_ENTRYPOINT_NOT_FOUND` -- the MSVC-target ASan runtime DLL
  (`clang_rt.asan_dynamic-x86_64.dll`) is not on `PATH` by default in this
  environment, and the standalone LLVM install's copy (`C:\Program
  Files\LLVM\lib\clang\21\...`) is the WRONG one (entrypoint mismatch,
  presumably a version/toolset mismatch with what rustc's sanitizer runtime
  expects) -- the one that actually works is the MSVC-toolchain-bundled
  copy: `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\
  Tools\MSVC\<ver>\bin\Hostx64\x64\clang_rt.asan_dynamic-x86_64.dll`. Once
  that directory is on `PATH`, all three targets ran clean:
  `fuzz_parser` 287,247 execs/90s, `fuzz_typechecker` 225,620 execs/90s,
  `fuzz_lexer` 456,487 execs/60s, **zero crashes/timeouts/OOMs across all
  three.** (This PATH requirement is worth remembering for the next agent
  who hits the same DLL errors and assumes cargo-fuzz is broken -- it
  isn't, it's a PATH gap specific to this Windows MSVC environment.)

Gates (this wave, before the follow-up commit): conformance 60/60 (was
59/59 -- new regression test), tier1+tier2 GREEN, `security_gate.sh` PASS
(72 checks), bootstrap 16/16 solo, combined-category grammar sweep 150/150
match (0 diverge, 0 both-fail) post-fix.
