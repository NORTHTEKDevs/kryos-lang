# Kryos Launch Readiness — Final Synthesis

**Date:** 2026-08-05
**HEAD reviewed:** `d29ac99` (CI green 9/9, re-confirmed live via `gh run list`)
**Inputs:** four independent reviewer passes (production-certifier, production-realism,
devil's-advocate, completeness-critic) over one hardening campaign (invariant analyses x4,
red-team x3 rounds, differential fuzzing), plus direct re-execution of the disputed claims
against the existing `compiler/target/release/kryos.exe` (read-only, no rebuild) during this
synthesis.

This document adjudicates disagreement rather than averaging it. Where reviewers disagreed
on a fact (not a judgment call), the fact was re-executed live and the result is stated below
with its exact command and output.

---

## 0. A finding about the evidence itself, verified before anything else

The campaign's own upward-reported summary ("CAMPAIGN EVIDENCE") disagreed with
`tools/loop/LEDGER.md` — the authoritative live document — **in both directions**, on
trust-model-adjacent items, in the same session:

| Claim in the campaign summary | LEDGER.md status | Re-verified live this session |
|---|---|---|
| `container-element-alias-backend-divergence`: **CONFIRMED FIXED** | Item 15: **ROOT-CAUSED, DESIGN NOTE, NOT FIXED (2026-08-05)** | **LEDGER is correct.** `kryos run` on `tests/security/attack_container_element_alias_refcount.kry` prints `x19999!\|x19999!\|x19999!\|x19999!\|5`; `kryos build --release` on the same file prints `x19999!\|x19999\|x19999\|x19999\|5`. The mutation is visible through every alias on JIT and only through the first alias on AOT — a live, reproducible backend divergence, exactly as LEDGER states. The evidence pack's "CONFIRMED FIXED" claim is **false**. |
| `actor-state-stored-closure-cap-escape`: **NOT CONFIRMED** | LEDGER CLOSED table, item 18: **FIXED**, `security_gate.sh` 46/46 incl. 2 new checks | **LEDGER is correct.** `kryos run tests/security/attack_actor_state_stored_closure.kry` now exits 1 with `error[E0507]: call to \`reader\` requires capabilities [all] not granted to caller`, both default and `--strict-capabilities` modes. The evidence pack's "NOT CONFIRMED" claim is **false** — this one is actually closed. |
| Item 10 (`cap_escape_closure_wraps_closure`, the wrapper-closure escape) — **absent from the campaign summary entirely** | Item 10: **NOT FIXED, HIGHEST PRIORITY (breaks the trust model)** | **Confirmed live, this is real and current.** See §1 below. This is the single most consequential fact in the whole campaign and it did not appear anywhere in the summary handed to the reviewers. |

**Why this matters more than any individual bug:** three of the four reviewers built their
verdict partly from the summary, not from LEDGER.md directly. Reviewer 2 (production-realism)
and reviewer 4 (completeness-critic) caught the discrepancy by reading LEDGER.md directly, as
instructed. Reviewer 1 (production-certifier) caught item 10 by independently re-running the
repro rather than trusting either document. Reviewer 3 (devil's-advocate) inherited the
summary's framing and consequently argued about the *wrong* closed/open item (it treated the
actor-state escape as the live one; the actual live one is the wrapper-closure escape). The
process lesson, not just the code lesson: **a status summary must be diffed against the
authoritative ledger before it is reported upward, every time** — this campaign's own
non-negotiable #1 ("a self-reported 'fixed' is not evidence") applies to campaign-summary
generation itself, not just to individual bug fixes.

---

> **UPDATE (2026-08-05, structural-completeness wave):** LEDGER item 10 — the specific
> blocker this document's condition names in §1 and §3.1 — is **CLOSED**, re-verified live:
> `kryos check`/`kryos check --strict-capabilities tests/security/
> cap_escape_closure_wraps_closure.kry` now both exit 1 with `E0507`, both directions proven
> (leaks on the pre-fix binary, rejects on the post-fix binary). Two further live escapes in
> the SAME closure-provenance family were found and closed the same wave (a one-hop-deeper
> variant of the already-closed item 18, and its aliased-local sibling) — see
> `tools/loop/LEDGER.md`'s structural-completeness-wave entry and
> `docs/capability-soundness.md`'s top update note for full evidence. **This clears the
> specific condition in §1** ("blocked from all launch copy until LEDGER item 10 is closed and
> re-verified live"). It does NOT by itself upgrade the overall verdict below beyond that one
> named condition — blockers #2-8 in §3 (evidence-pack process discipline, item 15's backend
> divergence, the spawn-hang/parser-DoS/monomorphization/supply-chain/any-erasure items) are
> unrelated to the capability-provenance closure family this wave addressed and were not
> re-examined this session; a full re-adjudication of the launch verdict needs a fresh
> synthesis pass over all of §3, not just this one blocker.

## 1. VERDICT: **LAUNCH-AS-BETA** — conditional, with one marketing claim hard-blocked

Not LAUNCH: the language cannot be presented as "production ready" or "capability-safe" (a
completed guarantee) while a live, unfixed, trivially-reachable trust-model bypass exists,
independently reproduced by two reviewers and by this synthesis.

Not DO-NOT-LAUNCH (full stop): the repository is already public. Most of the language —
type system, ownership/ARC, generics, concurrency primitives, both native backends,
self-hosting — is genuinely solid, heavily tested, and gated green (conformance, CI,
bootstrap all independently re-verified live, not trusted from self-report). Withholding
an honest beta label does not make the existing public code any safer; it only means the
gap goes undisclosed. Disclosure is the higher-value move available right now.

**The condition:** Kryos may launch as an explicitly-labeled, pre-1.0 beta with a disclosed
limitations section (§4). The specific claim **"capability-safe"** — used as a completed,
load-bearing guarantee (not as "capability-*aware*, hardening in progress") — is blocked from
all launch copy until LEDGER item 10 is closed and re-verified live. This is a narrower,
more precise gate than a blanket ship/no-ship: it says exactly which sentence cannot be
written yet, not that nothing can ship.

This adjudicates the four reviewers as follows: production-certifier (NO-SHIP) and
production-realism (BLOCK) are right about every fact and right that "production ready" +
"capability-safe" cannot be claimed today — but their verdicts are calibrated to a
zero-tolerance production SLA (payment/auth-adjacent web services), a harsher bar than a
disclosed pre-1.0 language beta needs. Devil's-advocate's PROCEED-WITH-CHANGES is the closest
to correct in shape (beta + gate the specific claim) but was reasoning from the wrong open
item (see §0) — its prescription is retained, its factual premise is corrected here.
Completeness-critic's four new gaps (§5) are real and are added to the disclosed-limitations
list and the ledger; none of them independently changes the verdict, because item 10 alone
already caps it at LAUNCH-AS-BETA.

---

## 2. The soundness question, answered directly

The task asked for the capability model to be "provably sound." Here is the honest
three-way split, with no hedging:

### (a) What is PROVEN

**Nothing.** There is no stated theorem with mechanically checked invariants, and no gate
that cannot pass while the property is violated. `docs/capability-soundness.md` states an
informal theorem and audits 22 invariants against the checker's source — this is a serious,
well-executed code audit, but it is not a proof: it is prose reasoning about code, checked
by a human (this session) reading the same code, not by a proof assistant or an exhaustive
symbolic check. The clearest evidence that it is not a proof: the theorem is currently
**false** — item 10 is a live counterexample in the exact baseline the document audits, and
both `kryos check` (default mode) and `kryos check --strict-capabilities` — the two
mechanical gates that exist — pass on the counterexample program. A gate that can pass while
the property it names is violated is not evidence of that property; it is evidence the gate
doesn't cover that shape.

### (b) What is EMPIRICALLY UNBROKEN

The capability model survived real, executed attacks across several **distinct** classes,
across 6 red-team rounds plus this campaign's invariant analysis (2 new attack programs).
Attack classes that were tried and did **not** succeed (verified fail-closed, live, this
baseline):
- Closure-laundering through a **container or HOF call site** (the original round-5 class —
  a struct field, array element, map value, or generic-HOF argument holding a closure) — fixed
  and now actively rejected (`resolve_closure_caps`/`resolve_container_path_caps`).
- **impl-method wrap** (`tests/security/attack_wrap_closure_impl_method.kry`) — ruled out,
  rejects correctly.
- **Actor-handler wrap** (`tests/security/attack_wrap_closure_actor.kry`) — ruled out, rejects
  correctly.
- **Raw-memory / unsafe pointer arithmetic** as a capability bypass — found real once, fixed,
  now gated.
- **Generic identity function round-trip** and **raw tuple payload (not Option/Result)** —
  ruled out, do not defeat inference.
- **FFI/extern surface** (E0506) — gated; declaring an extern is free, calling one that backs
  a `kryos_*` builtin charges the builtin's capability.
- **Actor state field storing a privileged closure**, read back and invoked from a different
  handler (item 18) — found real, now fixed and gated (`security_gate.sh` 46/46, re-verified
  live this session: rc=1 both modes).

That is a genuinely wide, genuinely adversarial sweep, and most of it holds. It is evidence
of "hardened against a broad attack surface," which is a real and valuable property. It is
not evidence of soundness — a sweep, however wide, proves only that the specific things tried
did not work; it says nothing about the next shape not yet tried, which is exactly what item
10 was (a wrapper closure that never invokes its argument inside its OWN body at its OWN call
site — the checker's fail-closed default triggers only through mechanisms tied to a
*call site*, and this shape has none).

### (c) What is ASSUMED or UNTESTED

- The full sub-capability lattice's `Capability::satisfies` implementation was matched
  against its call sites and CLAUDE.md's documented table, not read line-by-line this
  campaign (explicitly flagged by the capability reviewer as out of scope this pass).
- Generic monomorphization is flagged by this campaign's own analysis as "carried 3 of the 4
  historical capability bugs" and "the highest-suspicion surface" — and has still not had a
  dedicated adversarial fuzzing pass combining capabilities with deep generic instantiation.
- `spawn` mutating-closure reentrancy and throw-during-unwind interaction with capability
  checking: untested, explicitly out of scope every round so far.
- Any chain of 2+ wrapper closures combined with generics and/or containers together: item 10
  is the 1-level case; the combinatorial space beyond it (generic wrapper returning a wrapper
  returning a wrapper, each parametrized differently) has not been swept.

### What "provably sound" would actually require, and the cost

A real proof needs two layers, not one:
1. **A mechanized soundness proof** (progress + preservation style) over a small core
   calculus capturing closures, capability sets, and `deny!` narrowing — done in a proof
   assistant (Coq/Lean/Isabelle) or as a fully explicit hand proof with a precise typing
   judgment and closure-provenance model. This is PL-research-scoped work: realistically
   2-6 months for someone with type-theory background, more if starting the calculus from
   scratch. It would prove the *design* sound.
2. **A refinement argument (or, more practically, exhaustive fuzz-based cross-checking) that
   the actual Rust implementation (`checker.rs`) matches the calculus.** This matters because
   item 10 is exactly the kind of gap a calculus proof does NOT catch by itself — it is an
   *implementation* gap ("the returned lambda's own body is attributed an empty capability set
   instead of the fail-closed default"), not a flaw in the informal theorem's shape. A proof
   about an idealized calculus does not, by itself, prove the compiler that compiles real
   `.kry` files is correct.

**The more immediately actionable path**, already identified in this codebase's own design
notes and not yet built: implement **capability-typed function values**
(`fn() -> str @ {fs:read}`), so provenance is carried in the *type* rather than re-inferred
per call site by a checker that must enumerate every shape a value can arrive through. This
closes item 10's entire bug *class* structurally (a wrapper closure's return type would
correctly propagate `@{fs:read}` through ordinary type-checking, with no new call-site
pattern-matching needed) rather than patching case-by-case. Estimated at a few weeks of
focused compiler work per the existing design note — not a formal proof, but the highest-
leverage step toward one, and the step that would have prevented item 10 by construction.

---

## 3. Blockers, ranked

Each entry: what must be true to clear it.

1. **LEDGER item 10 — live capability escape (wrapper closure defeats `deny!()`, both modes).
   CLOSED 2026-08-05 (structural-completeness wave).** `tests/security/
   cap_escape_closure_wraps_closure.kry` now exits 1 (`E0507`) under both `kryos check` and
   `kryos check --strict-capabilities`, re-verified live, proven both directions (pre-fix
   binary reproduces the leak; post-fix binary rejects). Wired into `security_gate.sh` checks
   #47-50 alongside two further live escapes in the same family found and closed the same
   wave. See `tools/loop/LEDGER.md`'s structural-completeness-wave CLOSED entry and
   `docs/capability-soundness.md` invariants 23-24 for the fix and full evidence. **Was
   blocking:** any "capability-safe" claim in launch copy — that specific condition is now
   cleared.
2. **Evidence-pack accuracy (§0).** Two false claims (one over-claim, one under-claim) reached
   this synthesis from the campaign's own summary. **Blocks:** trusting any future
   "CONFIRMED FIXED" / "NOT CONFIRMED" claim about a capability/trust-model item without an
   independent re-run. **Clears when:** a process step is added — diff any capability/security
   status claim against `tools/loop/LEDGER.md` before it is reported upward — not a code fix,
   a discipline fix.
3. **LEDGER item 15 — backend semantic divergence (array-of-struct element alias).**
   Re-verified live this session: JIT and AOT disagree on whether a mutation through one alias
   is visible through a second alias of the same read. Now accurately disclosed in CLAUDE.md
   (already says "NOT FIXED" as of 2026-08-05) and in this document. **Blocks:** any
   "both backends agree" / "deterministic across backends" claim. **Clears when:** either
   backend's representation is unified (documented as sharing the same architectural root as
   item 3, the struct-argument leak — a representation/ABI change, not a point fix) or the
   divergence is permanently and prominently disclosed (already true; does not itself block a
   disclosed beta, only blocks the "backends agree" claim specifically).
4. **LEDGER item 16 — uncaught `throw` inside `spawn` permanently hangs every `wg_wait()`.**
   Ordinary worker-pool idiom, no diagnostic, no timeout. **Clears when:** the failure
   propagates to the WaitGroup (poison it, or run pending waiters with an error state) or an
   enforced timeout exists; until then, `docs/09-concurrency.md` must carry an explicit
   warning on this exact combination.
5. **LEDGER items 14 + 22 (new, below) — parser resource-DoS, two distinct mechanisms.** Item
   14: unguarded stray-`;` recursion stack-overflows the compiler (crash, exit 253). Item 22:
   the EXISTING `MAX_NESTING_DEPTH`/`MAX_RECURSION_DEPTH` guards bound stack depth correctly
   but not total WORK — 9 independent grammar constructs hang indefinitely just below the
   documented ceiling. **Clears when:** both get a bound with a clean diagnostic, mirroring
   the parser's own existing guard pattern (per the certifier's note, item 14 is a one-function
   fix); item 22 additionally needs the guard's *purpose* extended from stack-safety to
   work-boundedness.
6. **LEDGER item 19 + item 23 (new, below) — unbounded monomorphization, two distinct
   mechanisms.** Item 19: mangled-name generation is O(2^depth) for a type-doubling generic
   chain. Item 23 (new): a self-recursive, type-*growing* generic instantiation is completely
   unbounded (3.2GB+ in 15s on an 8-line program, still climbing). **Clears when:** either a
   depth/size cap with diagnostic (matching the parser's pattern) or a content-addressed/
   interned mangled-name scheme lands for both.
7. **LEDGER items 12, 13, 17 — `kryos pkg`/`kryos audit` supply-chain integrity gaps** (lockfile
   never consulted and silently overwritten; `git =` manifest source ignored; `audit` reports
   clean on code that will not compile). **Clears when:** fixed, or `kryos pkg`/`kryos audit`
   are explicitly labeled "not yet a trust boundary — pin and vendor manually" in user-facing
   docs before any beta recommends using them beyond a toy project.
8. **LEDGER item 24 (new, below) — `bool` into `any`.** Fails the AOT build outright and
   silently misrenders on JIT (`1` instead of `true`) — a crash+silent-wrong-answer combination
   for a completely ordinary shape (`fn log_event(args: [any])`). **Clears when:** fixed, or
   folded explicitly into the existing `any`-erasure warning in CLAUDE.md/docs with this exact
   failure mode named (today's docs mention render mis-formatting for `any`, not an outright
   AOT build failure).

None of these require the struct-ABI change that items 3 (struct-arg leak) and `any` erasure
need — all are scoped, single-mechanism fixes, consistent with how the ~60 prior defects in
LEDGER's CLOSED table were closed.

---

## 4. Minimum honest launch posture

**Version label:** `v0.9.0` (unchanged — this is already the repo's own accurate pre-1.0
label; do not round up to "1.0" or drop "beta"). Existing badge/README status text is correct
on this point and was not changed.

**The exact wording a user must see before adopting the capability model as a security
boundary** (place in README's Status section — already added this session — and in
`docs/10-capabilities.md`'s introduction):

> **Capability-model status: hardening in progress, not yet a completed security guarantee.**
> Kryos's deny-by-default capability system has survived a wide adversarial sweep — closure
> laundering through containers and higher-order functions, actor-state storage, impl-method
> and actor-handler wrapping, raw-memory paths, and the FFI/extern surface are all fail-closed
> and gated in CI (`tests/security_gate.sh`). **One live bypass is currently open and
> unfixed:** a closure returned by an ordinary zero-capability wrapper function defeats
> `deny!()` under every enforcement mode, including `--strict-capabilities`
> (`tools/loop/LEDGER.md` item 10). Do not depend on `deny!()` to contain an untrusted or
> partially-trusted code path that might use this exact shape (a decorator, logger, retry
> wrapper, or middleware pattern that returns a closure without calling it inside its own
> body) until this item is closed and independently re-verified. Track status in
> `tools/loop/LEDGER.md` and `docs/LAUNCH-READINESS.md`.

A security claim this project cannot fully defend must not appear in launch copy: do not use
"capability-safe" as a completed adjective (e.g. "Kryos is a capability-safe language") in any
external announcement, landing page, or marketing description until item 10 closes. "Capability-
aware," "deny-by-default, hardening in progress," or the disclosure paragraph above are the
accurate framings today.

---

## 5. Additional gaps found this synthesis (completeness-critic's findings, verified and
   carried forward — not launch-blocking individually, but must be disclosed)

- **CLI dev-tooling surface has zero test coverage.** Verified: grepping `tests/` for any of
  `lsp, dap, repl, bench, profile, coverage, watch, workspace, trace, diff, changelog, cheat,
  tree, lint, doc_serve, bindgen, pack, eval, config, manifest` returns 0 hits for every one,
  across ~20 subcommands checked. `kryos lsp`/`kryos dap` parse untrusted editor/debugger
  protocol input — a distinct, unaudited attack surface from anything fuzzed this campaign
  (which scoped entirely to the `.kry` source-language grammar). See LEDGER item 26.
- **wasm32 backend is effectively unaudited.** 11 hand-written smoke probes plus a CI
  smoke job exist; it was explicitly excluded from this campaign's new differential fuzzer,
  `security_gate.sh`, and all leak/race testing, despite being documented in CLAUDE.md as a
  first-class target alongside the two native backends. See LEDGER item 27.
- **The campaign's own new security/fuzz corpus runs on Linux CI only.** Verified directly in
  `.github/workflows/ci.yml`: `security_gate.sh` and `fuzz_gate_grammar.sh` appear only in the
  `ubuntu-latest` job (line 159, line 226); the `macos-14` job (line 515) and `windows-latest`
  job run a different, smaller smoke set. See LEDGER item 28.
- **`compiler/stdlib/smtp.kry` and `compiler/stdlib/term.kry` have never been compiled by
  anything.** Verified: 0 references to `std::smtp`/`std::term` anywhere in `tests/`,
  `ecosystem/`, or `examples/`. `docs/stdlib/term.md`'s 19 fenced code examples are also never
  executed — `tools/docs-examples/check.py`'s glob (verified directly) excludes
  `docs/stdlib/*.md`. See LEDGER item 29.

---

## 6. What to do next, in order

1. Wire `tests/security/cap_escape_closure_wraps_closure.kry` into `security_gate.sh` as a
   known-failing (must-stay-red-until-fixed) check — hours of work, prevents the gap from
   silently widening while item 10 is being fixed.
2. Fix LEDGER item 10 (or land the capability-typed-fn-value design, which closes it and its
   whole class at once) — this is the single blocker on the "capability-safe" claim.
3. Add the process step from §0/blocker 2: any future capability/security status report must
   be diffed against `tools/loop/LEDGER.md` before being reported upward.
4. Fix items 14 and 22 together (same guard family, parser resource bounds) — the certifier's
   own read is that item 14 is a one-function mirror of an existing pattern.
5. Fix items 19 and 23 together (same guard family, monomorphization resource bounds).
6. Fix item 16 (spawn/throw/WaitGroup hang) or ship the documented warning immediately if the
   fix is not ready by beta launch.
7. Fix or clearly gate items 12, 13, 17 (`kryos pkg`/`kryos audit` supply-chain gaps) before
   recommending `kryos pkg` for anything beyond a toy project.
8. Fix item 24 (`bool` into `any`) or fold its exact failure mode into the existing `any`-
   erasure documentation.
9. Land at least a reduced-iteration `security_gate.sh` + `fuzz_gate_grammar.sh` pass on the
   macOS and Windows CI jobs (item 28), and a minimal `kryos check`/compile smoke test for
   `smtp`/`term` (item 29), before claiming "CI green on all platforms" in any launch copy.
10. Once items 10, 14, 16, 19, 22, 23 are closed and re-verified live (not self-reported), and
    items 12/13/17/24/26/27/28/29 are either fixed or accurately disclosed, re-run this same
    synthesis process (four independent reviewers + live re-execution of every disputed claim)
    before removing the "capability-model hardening in progress" language and using
    "capability-safe" as a completed claim.
