# Kryos Launch Readiness — Re-Adjudication (2026-08-16)

**Date:** 2026-08-16
**HEAD reviewed:** `dbea2da` (local `master`)
**Prior synthesis reviewed and superseded:** the 2026-08-07 document at HEAD `feb1991` (preserved in
git history), whose VERDICT was LAUNCH-AS-BETA with **at least seven distinct open live capability
bypasses**. That count is now zero, measured fresh this session, not carried forward on trust — see
Section 1.

**Everything in this document was executed live this session** against a `compiler/target/release/kryos.exe`
that is current with HEAD (verified: no source file under the compiler crates is newer than the
binary's mtime), with stray `kryos.exe` processes killed before and after every gate, gates run
strictly one at a time (never concurrently — this repo's own operational rule, violating it produces
spurious `rc=127` contention failures). Every number below is copy-pasted from real command output
captured this session, not summarized from memory or trusted from a prior document.

---

## 1. VERDICT

**NOT YET 1.0. Ship as v0.9.0-beta, unchanged from VERSIONING.md's own standing bar.** The
capability-safety claim that blocked every prior synthesis (2026-08-05 through -07) is **no longer
blocked** — the last live escape closed 2026-08-13 and the count has stayed at zero through today's
independent re-measurement. What still blocks a `1.0.0` tag is not a defect this session found; it is
the bar `VERSIONING.md` already set for itself and has not yet met: **"cut only after external users
have run real workloads against it. Not before."** Kryos has one user. That has not changed.

Read that as two separate questions, because this repo has repeatedly conflated them and then had to
walk the conflation back:

- **"Is the engineering done and gated?"** — Yes, extensively, as of this session. See Section 2.
- **"Is it 1.0?"** — No. Zero external validation is a fact about exposure, not about code quality,
  and no gate in this repository measures it. See Section 4.

---

## 2. What is PROVEN, this session, with the command and the real output

Every gate in the repo's assigned ladder was run to completion. None were skipped, none were
sampled, none were trusted from a prior session. Full ladder, in the order run:

| # | Gate | Command | Result |
|---|---|---|---|
| 1 | Capability-escape corpus | `bash tools/loop/escape_status.sh` | **STILL ESCAPING: 0, now-rejected: 19, missing: 0** (grown from 17 named shapes as of the 2026-08-13 item-33 fix to 19 as of this HEAD — two more shapes were added to the corpus and both are rejected) |
| 2 | Security gate | `bash tests/security_gate.sh` | **PASS**, all checks green |
| 3 | IR signature gate | `bash tests/ir_signature_gate.sh` | **PASS** — 65 emitted modules, 0 severe mismatches |
| 4 | Strict-capabilities examples | `bash tests/strict_caps_examples.sh` | **PASS — 91/91** |
| 5 | Inferred-mode soundness | `bash tests/inferred_soundness.sh` | **PASS** — all probes correct |
| 6 | Cascade detector | `kryos.exe check tests/conformance/conf_stdlib_wave14.kry` | **rc=0** |
| 7 | Spec conformance suite | `bash tests/conformance/run_conformance.sh` | **65/65 PASS**, both backends |
| 8 | Type soundness | `bash tests/type_soundness.sh` | **PASS** |
| 9 | Backend divergence pins | `bash tests/backend_divergence_pins.sh` | **PASS** |
| 10 | Diagnostics gate | `bash tests/diagnostics_gate.sh` | **PASS** |
| 11 | Parser nesting-depth guard | `bash tests/parser_nesting_gate.sh` | **PASS** — bounded-time, false-positive-free |
| 12 | Concurrency smoke | `bash tests/concurrency_smoke.sh` | **PASS** — no deadlock |
| 13 | No-double-free | `bash tests/no_double_free.sh` | **PASS** |
| 14 | Match exhaustiveness | `bash tests/match_exhaustiveness.sh` | **PASS** |
| 15 | Stdlib compile gate | `bash tests/stdlib_compile_gate.sh` | **PASS — 66/66 modules** |
| 16 | CLI smoke gate | `bash tests/cli_smoke_gate.sh` | **PASS — 38/38 subcommands** |
| 17 | WASM differential gate | `bash tests/wasm_differential_gate.sh` | **PASS — 62/62 compiled programs match native** (1 correctly refused as out-of-subset, not a failure) |
| 18 | Authority surface gate | `bash tests/authority_surface_gate.sh` | **PASS — 0 ungated, 0 ungrantable** across 82 capability-classified builtins |
| 19 | Capability matrix gate | `bash tests/capability_matrix_gate.sh` | **PASS — 0 escapes across 75 enumerated shapes**, both modes. SOUNDNESS 75/75; PRECISION 34/75 (see Section 3 — documented, deliberate, not a failure condition) |
| 20 | JIT symbols gate | `bash tests/jit_symbols_gate.sh` | **PASS — 396/396** runtime symbols reachable |
| 21 | Package selftests | `bash tests/package_selftests.sh` | **PASS — 3/3** |
| 22 | **Ecosystem check (this wave's primary target)** | `bash tests/ecosystem_check.sh` | **PASS — 259/259 clean**, 0 failed, 6 negative fixtures excluded by design. Run to completion this session (~11 min under machine contention) — three prior sessions' attempts had died to contention, not to a real failure; this is the first completed run against current HEAD. |
| 23 | Examples gate | `bash tests/run_examples_gate.sh` | **PASS** — root 45/45, fixtures 16/16, showcase 24/24, capability-rejection 3/3, project OK |
| 24 | Self-host whole-program gate | `bash tests/selfhost_wholeprogram_gate.sh` | **PASS** — 46s (ceiling 200s) |
| 25 | Bootstrap (run alone, per operational rule) | `bash compiler/self-host/test_bootstrap.sh` | **PASS — 16/16 modules** |
| 26 | Docs-truth gate | `bash tools/loop/check-docs-truth.sh` | **PASS** — measured 0 live escapes; README and LEDGER preamble both state 0; re-verified after this session's doc edits. See Section 5 for a caveat on this gate's own internal coverage. |
| 27 | Docs status gate | `bash tests/docs_status_gate.sh` | **PASS** — live conformance count (65) matches README/docs/BUGS.md, no stale claims found |
| 28 | Acceptance (the repo's own "production-ready" definition) | `bash tests/acceptance.sh` | **PASS, 4/4** — [1/4] tier-1 gate ladder GREEN (18 gates incl. a fresh conformance run), [2/4] security gate PASS, [3/4] self-host reentrant-tokenize corruption repro PASS, [4/4] self-host mini-parser end-to-end PASS |

**28/28 gates green.** This is the broadest simultaneous-green measurement this repository has on
record — every prior synthesis document found at least one open item at this depth of checking.

---

## 3. The capability-safety question, answered directly

### (a) What closed, and how

The last live escape (LEDGER item 33 — a closure forwarded actor-to-actor as a message parameter,
escaping both `inferred` and `--strict-capabilities` modes) was closed 2026-08-13. Root cause was
**not** what the OPEN entry originally blamed (a `has_self_offset` bug in
`kryos-capabilities/checker.rs`) — that was a confident, source-read diagnosis that turned out wrong.
The actual mechanism was two compounding defects in the newer `kryos-types` row checker: (1) actor
handler bodies re-resolved their own parameter types instead of binding from the already-registered
signature, minting a second, unconnected capability-row variable; (2) a declaration-order forward
reference left a callee's row unbound at the point a caller snapshotted it. Fixed by binding every
handler's row in a pre-pass before the real walk. Full evidence, including the decisive
declaration-order control (same source, same types, opposite order — one correctly rejected, one not,
proving the bug was order-sensitivity, not a missing check) is in `tools/loop/LEDGER.md`'s CLOSED
table.

### (b) Two independent methods now agree at zero

- **Directed search** (`escape_status.sh`): 19 hand-found adversarial shapes, all rejected, both modes.
- **Combinatorial enumeration** (`capability_matrix_gate.sh`): 75 shapes generated by crossing SOURCE
  x CONTAINER x TRANSPORT axes systematically rather than by hand — a materially different method,
  reducing the chance both miss the same hole by construction. 75/75 rejected, both modes.

Neither is a soundness proof. Both are real, and their agreement is stronger evidence than either
alone — the 2026-08-05/06/07 syntheses' own standard for "provably sound" (a mechanized proof over a
small core calculus, plus a refinement argument that the checker implements it) is still not met and
is not claimed here.

### (c) The one documented, non-security cost of getting (a) and (b) to zero

Closing every escape required a fail-closed default: any closure/fn-value whose capability provenance
cannot be resolved (e.g., read out of a struct field, array element, or map value) is charged
`Capability::All` rather than assumed safe. Measured for the first time in the 2026-08-14
capability-matrix wave: **41 of the 75 enumerated shapes are legitimate, zero-authority (pure-closure)
programs that now also require `@capabilities(all)` to compile** — the fail-closed stance does not
distinguish a privileged closure in a container from a pure one in the same shape. This is LEDGER item
41, explicitly re-evaluated and left as-is on 2026-08-15: an experiment to narrow it
(`KRYOS_NO_HOTPARAM_ALL=1`) was measured against the full attack corpus and found **2 real escapes**
that the shipped default correctly rejects — narrowing precision reopened soundness. The shipped
default trades convenience for safety, deliberately, with the trade quantified rather than hidden.
This is a real usability cost for callback-heavy container-shaped code, not a security defect, and
should be described to users as such — not smoothed over as "capabilities just work."

### (d) What "provably sound" would still require

Unchanged from every prior synthesis: a mechanized proof over a small core calculus, plus a refinement
argument that `kryos-capabilities`/`kryos-types` implement it. Nobody has done this. The honest claim
today is **"hardened against a wide, dual-method adversarial sweep, currently at zero known escapes,
not formally proven."** That is a materially stronger claim than any prior session could make, and it
is still short of "proven."

---

## 4. Why this is not "1.0.0", stated plainly

Per `VERSIONING.md` (unchanged, and correct as written):

> **1.0.0** — cut only after external users have run real workloads against it. Not before.

Nothing this session measured bears on that bar, because nothing this session did constitutes external
exposure. The engineering-completeness bar (this repo's own `tests/acceptance.sh`, whose docstring
says it exits 0 "ONLY when kryos-lang is production-ready by the definition in
`tools/loop/LEDGER.md`") is **met** — see Section 2, row 28. That is a real, narrower, internally-defined bar
about gate health, and it should not be equivocated with the `VERSIONING.md` bar about field exposure.
Both are true simultaneously and are not in tension: **internally production-ready, not yet
externally validated.**

**Version label:** `v0.9.0`, unchanged. Do not round up to `1.0.0`.

---

## 5. CI, verified honestly (this task explicitly required not overclaiming here)

**What is verifiable without running CI, verified this session:**

- `.github/workflows/ci.yml` parses as valid YAML (`python3 -c "import yaml; yaml.safe_load(open(...))"`
  — succeeded, 9 jobs: `build-and-test`, `wasm-smoke`, `selfhost-stage1`, `docs-examples`,
  `registry-smoke`, `quickstart-e2e`, `fuzz`, `build-and-test-macos`, `build-and-test-windows`).
- Every script path the workflow invokes (24 distinct `bash <path>.sh` / `python3 <path>.py`
  references, resolved against each step's `working-directory`) **exists in the repository** — checked
  file-by-file, zero missing. This matters concretely: the task brief flagged that the Windows job now
  runs `security_gate.sh`, `stdlib_compile_gate.sh`, `cli_smoke_gate.sh`, `authority_surface_gate.sh`,
  and `jit_symbols_gate.sh` for the first time, and every one of those paths resolves.
- None of the 24 referenced scripts are git-tracked with the executable bit (`git ls-files --stage`
  shows `100644` for all of them, not `100755`). **This does not block CI**: every single invocation in
  `ci.yml` explicitly runs the script through its interpreter (`bash script.sh`, `python3 script.py`),
  never as a bare `./script.sh`, so the missing `+x` bit is inert on a Linux/macOS runner. Confirmed by
  grep across the whole file — zero bare-executable invocations of a test script exist.

**What is NOT verifiable from this environment, and is NOT claimed:**

- **Whether a CI run on this exact HEAD (or any recent HEAD) has actually completed green.** This
  session did not have network/`gh` access exercised for that purpose, and the task instructions
  explicitly direct not to claim CI is green without running it. Prior syntheses (2026-08-06/07) found
  CI runs `cancelled`/`cancelled`/`failure`/`in_progress` — not a clean streak — and this session has no
  fresher data point than that. **Static validity (YAML parses, scripts exist, no bad interpreter
  invocation) is necessary but not sufficient for "CI is green."** Treat CI health as **unverified,
  pending a real run**, not as passing.

---

## 6. What remains genuinely OPEN (from tools/loop/LEDGER.md's OPEN section — only three headers remain)

1. **Item 41 — precision cost.** See Section 3(c). Deliberate, quantified, not a bug. Re-evaluated
   2026-08-15 and kept as-is after the narrowing experiment reopened 2 real escapes.
2. **Item 3 — struct-argument leak, ~86MB per 1M calls.** A struct with heap-bearing fields crossing a
   call boundary leaks; not method-specific (a free function leaks identically). DESIGN NOTE, NOT
   FIXED — 8 distinct fix attempts have been ruled out this repo's history. Performance issue, not a
   correctness or security blocker. Workaround: keep heap data out of structs crossing call
   boundaries, or read fields directly in a hot loop instead of passing/returning the struct.
3. **Item 6 — `any` type erasure.** `any` is a bare i64 with no runtime type tag; a *direct* (not
   container) `any` slot holding a `str`/`f64` mis-renders on `to_string`/`format`. DESIGN NOTE, NOT
   FIXABLE WITHOUT AN ABI CHANGE. The dangerous container-shaped variant (bool/f64/f32/str routed
   through an `[any]`/`map<_,any>` slot) is a clean compile-time rejection (LEDGER items 40/40b) — only
   the narrower direct-slot `str` case still silently mis-renders, and it is documented as such in
   `CLAUDE.md`.

Other documented, non-blocking limitations (STABILITY.md Section 5/6, verified still accurate this session,
no changes needed): turbofish struct construction unsupported; `panic` not catchable by `try`/`catch`
(`throw` is, by design); program nesting capped at 256/2048 (deliberate anti-DoS bound, not a
usability limit any legitimate program approaches); AOT needs a host C toolchain; `kryos fmt` drops
non-doc comments; the `wasm32` backend is a documented compute-focused subset, not full parity.

Explicitly deferred past 1.0 (HANDOFF.md, still accurate): catchable runtime panics (needs an
unwinding-strategy design decision); `i128`/`u128` (nonfunctional, and using them is now a clean
`E0110` compile error rather than a crash — the "document as unimplemented" half of that deferral is
done, the implementation half remains deferred).

---

## 7. A tooling gap found and disclosed, not silently fixed

While verifying `tools/loop/check-docs-truth.sh` (Section 2 row 26), inspection of its own source found that
its third check — "no LEDGER item may sit in OPEN while its own repro is actually rejected" (the exact
failure mode that put a fixed item at the top of OPEN on 2026-08-10) — **has never executed**. Its
`sed` range pattern is `/^## OPEN — ranked/,/^## CLOSED/p` using an em dash (`—`); the actual LEDGER
header is `## OPEN - ranked` with a plain hyphen (`-`). The range never matches, so the loop body runs
zero iterations and the check trivially "passes" every time by doing nothing.

**Not fixed this session, deliberately, and the reason is itself a finding worth recording:** a naive
one-character fix (em dash to hyphen) would introduce a *new* false-positive. The escape corpus's own
item labels `41a`/`41b` (both legitimately `fixed`) share a leading number with the unrelated,
legitimately-still-open LEDGER item **41** (the precision-cost design note, Section 3(c)/6.1) — the check's
regex `^${item}[a-z]?[[:space:]].*fixed$` would then match `41a`/`41b`'s "fixed" lines against OPEN
item `41`'s bare number and report a spurious FAIL against an item that is correctly open. A correct
fix needs the item-number matching disambiguated (e.g., requiring a word boundary that also excludes a
different corpus's coincidentally-numbered entries), which is more than a one-line change and was not
attempted here to avoid shipping an unverified fix to a verification script under time pressure.
**Net effect: `check-docs-truth.sh` reports PASS honestly for the check it does run (the escape-count
cross-check, Section 2 row 26), but the OPEN-item-freshness self-check is currently inert.** Flagged here so
it does not get rediscovered from scratch.

---

## 8. Minimum honest launch posture

**Version label:** `v0.9.0`. Do not round up to 1.0, do not drop "beta."

**Capability-model status, accurate as of this measurement:** Kryos's deny-by-default capability
system has survived a directed adversarial sweep (19 named shapes) AND an independent combinatorial
enumeration (75 systematically-generated shapes), and is currently at **zero known live escapes** —
verifiable on demand via `bash tools/loop/escape_status.sh` and `bash tests/capability_matrix_gate.sh`.
This is a materially different, stronger position than every prior synthesis document recorded (which
ranged from "4 open" to "7+ open" across 2026-08-05 through -07). It is still not a formal soundness
proof, and the fail-closed default that got it to zero has a real, quantified usability cost (Section 3(c)).
**Do not claim "formally proven capability-safe."** "Capability-safe against a broad, dual-method
adversarial sweep, zero known live escapes as of 2026-08-16" is the accurate, defensible framing.

**1.0 status:** not yet. The blocker is exposure (one user), not a defect this session found. Ship
`v0.9.0` as a feature-complete, extensively-gated beta — which is exactly what `README.md` and
`VERSIONING.md` already say — and cut `1.0.0` when real external usage exists to validate against,
per `VERSIONING.md`'s own already-correct standard.

---

## 9. Prior syntheses — retained for history, nothing lost

- **2026-08-07** (HEAD `feb1991`): VERDICT was LAUNCH-AS-BETA with 7+ open live capability bypasses
  and CI observed `in_progress` 2h25m+ with a non-green recent history. Superseded above — the
  bypasses are now all closed (Section 2 row 1, Section 3), CI health remains unverified either way (Section 5, unchanged
  posture: verify what's verifiable, do not claim green).
- **2026-08-06** (HEAD `0d6b426`) and **2026-08-05** (HEAD `d29ac99`): full text preserved in git
  history and in `tools/loop/LEDGER.md`'s "FINAL LAUNCH SYNTHESIS" section. Their central
  methodological finding — a repro file without a LEDGER entry is not tracked, regardless of who wrote
  it — held again through every session since, including this one (every gate cited above already
  existed with a wired LEDGER entry; no new untracked repro was found or needed this session, because
  this session's job was verification breadth, not new adversarial search).

**The pattern across six sessions is itself evidence, in both directions.** Sessions 1-3 (Aug 5-7)
each found something new and none closed anything on their own initiative. Sessions since (culminating
in the 2026-08-13 item-33 fix and this session's 28/28 confirmation) show the opposite trend: the
open count went to zero and has *stayed* at zero across an independent re-measurement using a
different method. Continuing to say "not yet capability-safe" after this measurement would now be
**understating** the evidence, the same way saying "capability-safe, full stop" after 2026-08-05 would
have been **overstating** it. Both errors cost credibility; this document tries to name the actual
state precisely instead: zero known escapes, not formally proven, one usability cost, one user, not
1.0 yet.
