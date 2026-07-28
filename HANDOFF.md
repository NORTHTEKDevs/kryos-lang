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
   ~79MB-per-1M-calls method leak, but that is a performance item, not a
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
   (the ~79MB/1M-call method leak), not a 1.0 blocker.
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
8. `let hs: [dyn Handler] = [Health {}, Miss {}]` reports a confusing
   `E0100 "expected Health, found Miss"` instead of the intended `E0110` —
   array-literal element unification ignores the annotated `dyn` element type.

**Explicitly deferred past 1.0:** catchable runtime panics (needs an unwinding
strategy decision, 1–2 weeks and a design commitment) and `i128`/`u128`
(nonfunctional — document as unimplemented).
