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

## The critical path: struct-method receiver representation

This is the long pole (**2–4 weeks**) and it is believed to be the single root
cause of BOTH remaining blockers.

**Blocker 1 — `conf_spinlock_mutex`.** Repeats `sync error: lock on dropped
mutex` from spawned threads, then deadlocks (verified alive past 8 minutes,
output frozen at 8 lines). The mutex box is released while workers still hold a
reference: a use-after-free reachable from ordinary `Mutex` use. This is the
issue CLAUDE.md gotcha #22 documents as needing a representation/ABI change,
and it is also behind the ~79MB-per-1M-calls method leak.

**Blocker 2 — `conf_errors_concurrency`.** Narrowed further, and the valgrind
run **refuted** the shared-root hypothesis. Full detail in `docs/BUGS.md`. The
runtime reporter and the shared MIR are both proven correct and the recovery
block demonstrably runs. The single memory error is a bad channel handle inside
a SPAWNED closure (`kryos_chan_send` reading 28 bytes outside any live block,
reached from `__spawn_6`), not a freed actor receiver.

So **do not assume one ABI fix closes both blockers** — I initially thought it
would and the evidence says otherwise. Treat blocker 2 as an independent bug in
how a spawned closure captures a channel handle. It is also probably much
smaller than blocker 1.

**Next concrete action:** bisect `conf_errors_concurrency` by section to find
which `spawn` becomes `__spawn_6`, then inspect how the channel handle enters
that closure's environment.

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

1. ~~Valgrind triage of blocker 2~~ **done** — refuted the shared root; blocker 2
   is an independent spawned-closure channel-handle bug. Bisect it next; it looks
   smaller than blocker 1 and may be days rather than weeks.
2. Receiver representation/ABI change — 2–4 weeks, unblocks both blockers.
3. Box layout behind one shared helper — 2–3 days.
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
