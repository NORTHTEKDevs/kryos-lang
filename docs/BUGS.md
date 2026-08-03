# Known Bugs

> This file drifts easily because it's hand-maintained -- it once said "none
> currently tracked" while two conformance tests were actually deadlocking in
> CI. `tests/docs_status_gate.sh` now catches the most common form of drift
> (a test named here as "Active"/open that currently PASSES cleanly) and
> fails CI if this file falls out of sync with reality again -- see the note
> at the bottom of this file.

## Resolved

### `std::test::assert`'s 2-arg form was permanently shadowed by the compiler's own builtin and UNCATCHABLE (2026-08-02)

**Status**: Fixed. Regression: `tests/conformance/conf_assert_shadow_catchable.kry`
+ `tests/assert_shadow_gate.sh` (both directions' exit codes).

Both codegen backends dispatched any call literally named `assert`/
`assert_eq`/`panic` straight to the hardcoded `kryos_builtin_*` intrinsic
UNCONDITIONALLY, before the generic user-function-shadow check every other
builtin already goes through. `use std::test::{assert}` then
`assert(false, "boom")` inside a `try` printed `assertion failed: boom` and
aborted the process (exit 127) instead of throwing a catchable exception --
`catch` never ran. Fixed by adding the same shadow-check guard these three
arms were missing. Full detail + evidence: `tools/loop/LEDGER.md` item 2c
(CLOSED table).

### `[dyn Handler]` at a call site emitted a confusing `E0100` alongside the correct `E0110` (2026-08-02)

**Status**: Fixed. Regression: `tests/type_soundness.sh`
(`dyn_array_callsite_heterogeneous`).

`use_handlers([A{}, B{}])` (a heterogeneous dyn-trait array literal passed
directly as a call argument) reported both the correct `E0110` (dyn-in-
container is unsupported) and a confusing, unrelated `E0100` "expected A,
found B" from the array literal's own element-unification. The equivalent
`let x: [dyn Trait] = [A{}, B{}]` shape was already fixed; the call-site
shape needed a separate mechanism since `FunctionSig` only stores the
already-resolved `Type::Error`, not the raw annotation the `let` fix keys
off. Full detail: `tools/loop/LEDGER.md` item 4 (CLOSED table).

### A curried (2-level) generic closure return failed to build on AOT while JIT accepted it (2026-08-02)

**Status**: Fixed. Regression: `tests/conformance/conf_curried_generic_closure.kry`.

`fn curry_add<T>(a: T) -> fn(T) -> fn(T) -> T { return |b: T| (|c: T| a + b + c) }`
ran correctly on `kryos run` but failed LLVM codegen outright on
`kryos build --release` (`load %T, ptr %_1_arg` -- an unresolved generic
type parameter reaching IR emission, on BOTH the outer and inner closure,
not just the innermost one as first suspected). An explicitly-annotated
closure param naming the enclosing generic function's own type parameter
was never substituted to the concrete instantiation type. Full detail:
`tools/loop/LEDGER.md` item 8 (CLOSED table).

### Concurrency + struct receivers: two conformance tests deadlocked on `build --release` (2026-07-28)

**Status**: Fixed. Regression: `tests/conformance/conf_spinlock_mutex.kry` and
`tests/conformance/conf_errors_concurrency.kry`, both in the standard
conformance run (`tests/conformance/run_conformance.sh`).

Both tests built cleanly, ran past their first output, and then never
terminated on the LLVM/AOT backend (`kryos run`/Cranelift was unaffected).
`conf_spinlock_mutex` repeatedly printed `sync error: lock on dropped mutex`
before hanging; `conf_errors_concurrency` printed the expected `[actor
error] Adder.add: uncaught exception: negative` and then hung instead of
completing the join.

Root cause (single defect, closed both): spawn wrappers declared aggregate
captures as `ptr byval(T)` -- the by-value-in-memory ABI -- while the
runtime passes one pointer-sized word per env slot. Fixed in the commit
"fix: spawn wrappers take aggregate captures as a plain ptr, not byval".

Two process notes worth keeping, because both cost real time during the
investigation:

- A valgrind trace pointed at `__spawn_6` and a bad channel handle, and the
  working theory from that trace was that the two blockers were independent
  bugs. That was wrong -- the trace was accurate but the inference from it
  was not. The single spawn-ABI fix closed both. Prefer testing a candidate
  fix against every open failure over reasoning about whether they share a
  root.
- `kryos run` execs the compiled program as a CHILD, so plain valgrind sees
  nothing. Always `valgrind --trace-children=yes`.

Conformance is 53/53 on both backends as of this writing (`tests/conformance/`
has grown since; re-run `bash tests/conformance/run_conformance.sh` for the
authoritative current count -- don't trust a number typed into this file).

### String-in-struct ownership leak across function returns (v0.4)

**Status**: Fixed. Tracked by regression tests in
`compiler/crates/kryos-test-runner/tests/e2e/ownership/struct_with_strings_return_run.kry`
and `struct_with_strings_stress_run.kry` (1000-iter loop).
`examples/showcase/agent_runtime.kry` was rewritten to return the planner's
`Action` struct directly (with str fields) instead of using `[str]`
out-parameter slots; both JIT and `kryos build --release` are verified.

Original symptom (v0.4-era): a function returning a struct whose `str`
field was assigned from a previously-moved local would surface garbage
values, e.g. `len(action.arg_s) = 7305790164731371552` and empty string
content.

Root cause: partial-move tracking for struct field reads
(`partial_moved_locals` in `kryos-mir::lower`) did not extend to the case
where a non-copy struct local was moved wholesale into a return position
after a field had separately been moved out via field access. The current
MIR lowering tracks partial moves explicitly and the ownership analyzer
emits the correct drop / no-drop combination at scope exit.

If a new reproducer ever surfaces, please attach it to a fresh entry below
under "Active".

### `spawn` capture of a loop-local aggregate was shared across iterations (Cranelift)

**Status**: Fixed. Regression: section 7 of
`tests/conformance/conf_spawn_agg_capture_abi.kry` (a value assertion — sorts
the four received values and asserts `0 10 20 30`, not just a no-crash check).

**Backend divergence. Cranelift was wrong, LLVM AOT was correct.**

```kryos
let ch7 = chan()
let mut k = 0
while k < 4 {
    let mk = Msg.Val(k * 10)          // fresh loop-local each iteration
    spawn {
        match mk {
            Msg.Val(v) => send(ch7, v),
            Msg.Nothing => send(ch7, -1),
        }
    }
    k = k + 1
}
```

Expected `0 10 20 30` in any order (four independent captures).

- LLVM AOT: `0 20 30 10` — correct.
- Cranelift (before the fix): `30 30 30 30` — every thread observes the LAST
  iteration's value, with an occasional `10`/`20` sneaking in depending on
  scheduling.

Root cause was NOT a hoisted allocation (the initial hypothesis) — MIR shows
a fresh `Msg::variant#0(_3)` per iteration, and `RValue::EnumVariant` codegen
does `kryos_calloc` a new box each time. The real defect: the Cranelift
`Instruction::Spawn` arg-store match (`kryos-codegen-cranelift/src/codegen.rs`)
had clone/dup arms for `Str`/`Array`/`Map`/`Function`/`Shared`/`Struct` but
**no arm for `MirType::Enum`**, so an enum capture fell to the `_ => val`
default: the raw shared box pointer, un-cloned. MIR still emits the normal
`drop(_N)` right after `spawn` (the documented contract is that `spawn`
clones heap args, so the caller's local is still owned and gets dropped)
which freed the box while the spawned OS thread could still be about to read
it — and the freed allocation was immediately reused by the next iteration's
same-size `kryos_calloc`, so a thread that lost the race read whichever
iteration's value last occupied that address. The LLVM backend already had
a `MirType::Enum` arm in its equivalent spawn-arg path (boxing the aggregate
fresh), which is why it never exhibited this.

Fix: added a `MirType::Enum` arm to the Cranelift spawn arg-store match that
calls the existing `emit_enum_deep_copy` helper (already used for closure/
struct captures) to give the spawned thread an independent box, mirroring
the `MirType::Struct` arm immediately above it.

Found by `tests/conformance/conf_spawn_agg_capture_abi.kry` while adding
coverage for the spawn-wrapper ABI fix, and was a *different* bug from it —
the ABI fix was in the LLVM backend; LLVM was the backend that got this case
right.

## Active

Nothing currently tracked as an active bug in this file. The full ranked
queue of open, unfixed items (including design-note items that are
deliberately NOT one-line patches) lives in
[`tools/loop/LEDGER.md`](../tools/loop/LEDGER.md) — that file is the
authoritative, actively-maintained backlog; treat this file as a changelog
of resolved issues, not the live queue. If you find a new bug, add it to the
ledger's OPEN section (with a reproduction) rather than only here.

---

## Keeping this file honest

This file is hand-written prose, which is exactly how it drifted before (it
once claimed "none currently tracked" while two tests in this same file's
own "Active" section were deadlocking in CI, and later drifted the other
way -- claiming those same two tests were still open for weeks after the fix
that closed them landed and shipped). `tests/docs_status_gate.sh` is a
mechanical check, not full auto-generation: it scans this file's `## Active`
section for `tests/conformance/conf_*.kry` paths and runs each one; if a
test named here as an open/active bug currently exits 0 with no hang, the
gate fails and names the file to fix. It cannot catch every kind of drift
(prose claims with no associated test file, stale counts typed into
paragraphs), but it catches the exact failure mode that bit this file twice.
Run it with `bash tests/docs_status_gate.sh`; it is included in
`tools/loop/kryos-loop.sh gates`.
