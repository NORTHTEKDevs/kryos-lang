# Handoff — pre-1.0 readiness work, 2026-07-28

Branch: `fix/aggregate-sret-and-box-header`. Everything below is committed.
Written so a fresh session can pick up without re-deriving anything.

---

## What changed on this branch

| Commit | What |
| --- | --- |
| `d9b41d6b` | **LLVM/AOT:** aggregate return through a fn value now gets a real `sret` destination |
| `335551ee` | **Cranelift/JIT:** struct/enum boxes carry the allocation header (`kryos_calloc`/`kryos_free`) |
| `1968380d` | **ecosystem:** `JsonValue::Int` arm in kryos-fmt; capability annotations in the pipeline demo — this gate was **red on master** |
| `37c3b743` | conformance runner time-boxes each program; `docs/BUGS.md` tracks the two real hangs |
| `07c694f7` | dropped the stale `quinn-proto` lock entry (closes a high Dependabot alert) |
| `c561b40d` | relabel `1.0.0-rc.2` → `0.9.0`; STABILITY.md stops claiming a clean sweep |
| `184a580a` | **differential fuzzer** now generates indirect calls + aggregate returns |

## Environment notes that cost real time to discover

- **`kryos run` does not JIT in-process.** It AOT-compiles to a temp binary and
  `exec`s it as a CHILD. Plain `valgrind`/`gdb` therefore see nothing. Always
  `valgrind --trace-children=yes`.
- **`kryos_free` must be declared with its real VOID signature at the earliest
  declaration point** in the Cranelift backend's `compile_module_with_options`.
  `ensure_func_ref_with_args` falls back to a generic `(i64,..) -> i64` for an
  unseen name, and Cranelift then rejects the later void redeclaration
  (`signature ... is incompatible with previous declaration`).
- Kryos strings need `{{` / `}}` for literal braces.
- `cargo fmt --all -- --check` shows wide pre-existing drift (a rustfmt version
  difference, not anyone's edits). **CI runs no fmt or clippy job** — do not run
  `cargo fmt` on the repo, it produces a huge unrelated diff.
- `tests/test_harness_check.sh` hardcodes `target/release/kryos.exe` and so
  cannot pass on Linux. `tests/module_case_gate.sh` genuinely fails (a wrong-case
  `use std::String` is not rejected — a portability bug on case-insensitive
  filesystems). **Neither script is wired into `ci.yml`.** Both pre-existing.

## Current test state

Conformance: **39/41 both backends.** The 2 failures are the release blockers
below; both pass under Cranelift and hang under LLVM AOT.

Green: type-soundness, inferred-soundness, no-double-free, examples
(45/45 + 16/16 + 16/16 fixtures + 23/23 showcase + 2/2 cap-rejection),
strict-caps 90/90, package selftests 3/3, encoding, match-exhaustiveness,
concurrency-smoke, ecosystem **259/259**, `cargo test` for kryos-rt +
both codegen crates.

**Differential fuzzer with the new indirect/aggregate coverage: 45 programs,
0 flagged (0 ICE, 0 divergence, 0 AOT build failures).** This is the most
useful new data point — the expanded fuzzer did not turn up a fresh bug tail,
which shifts the schedule risk down substantially.

---

## Both 1.0 blockers are FIXED

Done on 2026-07-28, in commit "fix: spawn wrappers take aggregate captures as a
plain ptr, not byval". `conf_spinlock_mutex` and `conf_errors_concurrency` were
ONE bug, not two: `param_agg_ty` correctly decided that an aggregate capture in
a `__spawn_`/`__coopspawn_` wrapper needs the runtime's one-word ABI, but the
emitter rendered that as `ptr byval(T)` — the by-value-in-memory ABI, which on
x86-64 consumes no integer register. The wrapper read its enum off the stack and
took the next param from the first integer register, i.e. env slot 0 (the boxed
enum pointer). A `send` then used that pointer as its channel handle and the
matching `recv` blocked forever.

**Conformance is now 40/40 on both backends.** Concurrency-smoke and
no-double-free are clean; each blocker passes 3/3 consecutive AOT runs.

### Two process lessons, both mine

1. I read the valgrind trace correctly and then drew the wrong conclusion from
   it — I recorded that the blockers were independent bugs. Testing the
   candidate fix against BOTH open failures is what found the truth. Do that
   before theorising about shared roots.
2. The struct-method receiver representation (CLAUDE.md gotcha #22) is
   **no longer on the 1.0 critical path.** It is still worth doing for the
   struct-argument leak (see the correction below -- it is NOT method-specific),
   but that is a performance item, not a
   correctness blocker. My earlier 2–4 week estimate for it was gating the
   whole schedule; it should not.

## Structural fix worth doing alongside

Both bugs fixed on this branch were the same class: **an invariant introduced on
one path and not propagated to every consumer.** The `sret` ABI decision lived
in `emit_function` while `func_ret_types` kept the logical type; the
shared-owner header lived in `kryos_calloc` while Cranelift kept calling libc.

Two backends independently deciding box layout is a latent bug generator. Put
the layout behind one shared helper both backends call, and add debug asserts at
ABI boundaries. Precedent to follow: the dyn-trait aggregate-return case is
blocked by an explicit `E0110` rather than silently miscompiling — the fn-value
path had no such guard, which is exactly why it miscompiled silently.
**Guard every ABI boundary that isn't proven.**

## Remaining queue (my ordering)

1. ~~Both concurrency blockers~~ **done** — one spawn-wrapper ABI fix closed both.
2. ~~Conformance test pinning the spawn-wrapper ABI~~ **done** —
   `tests/conformance/conf_spawn_agg_capture_abi.kry`, 6 sections, green on both
   backends. The load-bearing shape is an aggregate capture FOLLOWED BY a scalar
   that is used; capturing an aggregate alone cannot detect the slot shift.
3. **NEW BUG, found by that test.** Cranelift shares ONE box for a loop-local
   aggregate captured by `spawn`, so every thread reads the last iteration's
   value (`30 30 30 30` instead of `0 10 20 30`). LLVM AOT is correct. Repro
   `tests/known_failures/spawn_loop_capture.kry`, details in docs/BUGS.md. This
   is a silent wrong answer, not a hang. Likely the per-iteration box is
   hoisted out of the loop in the Cranelift capture-boxing path — start there.
4. Box layout behind one shared helper — 2–3 days.
5. Receiver representation/ABI change — 2–4 weeks. **Now a performance item**
   (the struct-argument leak, ~86MB/1M -- see the correction below), not a
   1.0 blocker.
4. Capability-gate the raw-memory builtins — 2–4 days. Currently ungated, which
   undercuts the central pitch under `--strict-capabilities`.
5. Generate the docs' status sections from real test output rather than by hand.
   `docs/BUGS.md` had said `Active: (none currently tracked)` while two tests
   deadlocked, and `STABILITY.md` §5 opened by claiming no architectural
   failures — both now corrected, but by hand, so they will drift again.
   (Note: CLAUDE.md's `conf_stdlib_wave14` entry is NOT stale — it already
   records the array-dup struct-element fix that turned it into a pass. I
   mis-flagged this earlier in the session; it is accurate as written.)
6. Fix `tests/module_case_gate.sh`'s finding, and either make
   `test_harness_check.sh` portable or mark it Windows-only. Wire both into CI.
7. `comptime {}` runs at runtime while the docs sell it as compile-time. Fix the
   docs (hours); implementing real compile-time eval is months and should not
   gate 1.0.
8. ~~`let hs: [dyn Handler] = [Health {}, Miss {}]` reports a confusing
   `E0100 "expected Health, found Miss"` instead of the intended `E0110` —
   array-literal element unification ignores the annotated `dyn` element type.~~
   **done** — both the `let` shape and the same symptom at a CALL SITE
   (`use_handlers([A{}, B{}])`) now report E0110 alone; see LEDGER item 4.

**Explicitly deferred past 1.0:** catchable runtime panics (needs an unwinding
strategy decision, 1–2 weeks and a design commitment) and `i128`/`u128`
(nonfunctional — document as unimplemented).


---

# Merge, 2026-07-28 — this branch integrated with the leak/ABI line

Both lines branched from `ac45392` and neither was pushed, so they never saw
each other. Integrated on `integrate/leaks-abi-and-concurrency`. Nothing from
either side was dropped except one genuine duplicate.

## The duplicate, and why a clean auto-merge was the hazard

**The sret fn-value bug was fixed twice, independently and almost identically.**
`d9b41d6` here and `3d8b2b0` on the other line both looked up `func_sig_aggs`,
allocated via `kryos_arc_alloc`, and passed `ptr sret(agg)`. Only the placement
differed. Git auto-merged BOTH arms with no conflict: this branch's matches
first, so the other became unreachable dead code behind an `else if ...
.is_some()` that can never be true. The duplicate arm was deleted, this
branch's kept. Worth remembering — the two codegen files reported "auto-merging"
and only `CLAUDE.md` conflicted, which reads like a safe merge and was not.

## Why the two lines disagreed about test state

Platform. This branch ran on Linux and reported `conf_spinlock_mutex` and
`conf_errors_concurrency` hanging under LLVM AOT; the other line ran on Windows
and had them green the whole time. The `ptr byval(T)` spawn-capture bug is
System V-specific — a by-value-in-memory aggregate consumes no integer register
on x86-64 SysV, shifting every later parameter. Windows x64 uses a different
convention and did not manifest it. **Both sets of results were correct and
neither validated the other platform.** Same caveat applies in reverse:
`module_case_gate.sh` fails on Linux (a wrong-case `use std::String` is not
rejected on a case-insensitive filesystem) and passes on Windows.

## Correction to the struct-receiver characterization

The remaining-queue item above described this as a "method leak". Measured, it
is **not** method-specific: a free function leaks identically. The trigger is a
struct with HEAP FIELDS crossing any call boundary, roughly 85 bytes per call.
Flat for comparison: a struct with only scalar fields through a method, and the
same struct's fields read directly without a call. Repro and the full
rule-out list: `tests/mem/struct_arg_leak.kry`. Still open; still not a 1.0
blocker.

## Verified here on Windows after the merge

- conformance **45/45** both backends (43 + this branch's `conf_fnval_agg_return`
  and `conf_spawn_agg_capture_abi`)
- both concurrency blockers and both new tests: **3/3 consecutive AOT runs each**
- no-double-free, type-soundness, inferred-soundness, match-exhaustiveness,
  concurrency-smoke, module-case, bootstrap 16/16 (serial), examples,
  strict-caps 90/90, examples-e2e 12/12 response-body assertions, ir-signatures
- leak repros still flat: computed-argument and TCP round-trip (360k round
  trips, 0MB delta). Struct-argument leak still present at 86.2MB/1M, as expected.
- **this branch's newly-filed loop-capture bug independently reproduced**: JIT
  prints `30 30 30 30`, AOT prints the four distinct values. Report accurate.

## Build gotcha that cost the other line hours

`cargo build -p kryos-cli` does **not** regenerate `kryos_rt.lib` /
`kryos_stdlib_native.lib`, the staticlib archives an AOT-compiled program
links — only the rlibs the compiler itself uses. Any measurement of a
kryos-rt or kryos-stdlib-native change after a `-p` build silently tests the
old runtime. Run a full `cargo build --release` before measuring anything in
those crates.
