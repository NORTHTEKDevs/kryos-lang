# Kryos Launch Readiness — Re-Adjudication (2026-08-07, post capability-typed-fn-value Stage 1)

**Date:** 2026-08-07
**HEAD reviewed:** `feb1991` (local `master`, matches the commit this session started from; CI for this
exact commit was **`in_progress` for 2h25m+** at review time — see §5, this is a disclosed data point,
not treated as a pass or a fail).
**Prior synthesis reviewed and superseded:** the 2026-08-06 document at HEAD `0d6b426` (preserved in
git history and in `tools/loop/LEDGER.md`; its VERDICT — LAUNCH-AS-BETA, "capability-safe" blocked — is
independently reconfirmed below, not merely carried forward on trust).

**What changed since 2026-08-06:** a real structural campaign landed (`891c406` "feat(types):
capability-typed fn values, stage 1 (representation + inference)") — the effect-type redesign this
document's own §2/§6 called "the most actionable structural fix." **This stage changed zero
enforcement.** It is representation + inference only, sitting beside the existing, unmodified
`kryos-capabilities` checker. Five more security commits landed after it (`4b2afc4`, `136b8e7`,
`6f13ccb`, `0e2aaec`, `feb1991`), each finding a **new live bypass** of the still-enforcing old
checker. Net effect on the actual accept/reject behavior users get today: **unchanged in kind, worse
in count** — the open live-bypass list grew from 4 items (2026-08-06) to at least 7 distinct open
"LIVE CAPABILITY ESCAPE" ledger entries (items 30/35-conditional-expr, 32, 33, 34, 35-static-method,
36, 37) plus a newly-confirmed forward-looking gap in the NOT-YET-ENFORCING new type system (§2a).

Everything in this section was executed live this session against the existing
`compiler/target/release/kryos.exe` (read-only, no rebuild), with `KRYOS_STDLIB_DIR` set per the
repo's own operational rule, stray `kryos.exe` processes killed first.

---

## 1. VERDICT: **LAUNCH-AS-BETA, unchanged.** The claim "capability-safe" does NOT clear and stays blocked.

Independently reproduced this session, fresh, with actual command output (not trusted from any
report), all **`rc=0` with the secret printed**, in **both** `kryos run` (inferred mode) and
`kryos check --strict-capabilities`:

| Attack file | Shape | Result |
|---|---|---|
| `attack_container_param_alias_defeats_hotparam.kry` | `let c = b; c.f()` — param-container alias | LEAK, rc=0, both modes |
| `attack_actor_state_forloop_alias.kry` | `for x in [self.b] { x.f() }` — actor-state loop alias | LEAK, rc=0, both modes |
| `attack_deny_pipe_bare_ident_call.kry` | `0 \|> reader` inside `deny!()` — pipe operator | LEAK, rc=0, both modes |
| `attack_deref_borrow_param_defeats_field_resolver.kry` | `(*b).f()` via `&`/`*` indirection | LEAK, rc=0, both modes, **AOT binary also leaks** |
| `attack_reassign_local_defeats_hotparam.kry` | `let mut g = ...; g = caller_arg; g()` | LEAK, rc=0, both modes |
| `attack_deny_bare_closure_reassign_escape.kry` | reassigned bare closure local inside `deny!()` | LEAK, rc=0, both modes |

Their `_control.kry` counterparts (the same program, direct call instead of the attack indirection)
were also re-run and correctly **REJECTED** (`E0507`, rc=1) — confirming these are real, specific
dispatch gaps, not a broken test harness or a systemic false-negative.

`tests/security_gate.sh` (the CI-wired regression corpus, does **not** yet include any of the six
files above or items 30/32-37) — re-run this session, **PASS**, all checks green. This is exactly the
"test exists, gate silent" gap the 2026-08-06 document flagged: the gate is honestly green on what it
covers and covers less than what is known-broken.

`tests/ecosystem_check.sh` and `python tools/docs-examples/check.py` — re-run this session, **PASS**
(74/74 doc examples, ecosystem check clean exit).

Not DO-NOT-LAUNCH: the repository is already public; the non-capability language surface is not
re-litigated here (see the 2026-08-06 document's own §5, carried forward unchanged, nothing new
examined this session outside the capability boundary).

**The condition is unchanged in substance from 2026-08-06, restated against the current open set:**
Kryos may launch as an explicitly-labeled pre-1.0 beta with the disclosure paragraph in §4. The claim
"capability-safe" is blocked from all launch copy until (a) every open "LIVE CAPABILITY ESCAPE" ledger
item is closed and re-verified live, AND (b) the method-level argument from the 2026-08-06 document's
§3-item-5 is satisfied — either single-point-of-enforcement over the closed value-producing grammar,
or the capability-typed-fn-value system is actually wired to enforcement (not merely inferred
alongside it) with the gap in (§2a below) closed first.

---

## 2. The soundness question, answered directly, three-way split, no padding

### (a) PROVEN

Unchanged from 2026-08-06: the exhaustive, no-wildcard `Expr` match inside `resolve_closure_caps`/
`resolve_container_path_caps` is real (independently re-confirmed present in `checker.rs` this
session) and still buys exactly what the prior document said — protection against a *new AST variant*
silently falling through an enumeration, for the two functions that are the common resolution path for
most (not all) invariants. It does not extend to dispatch-layer gaps that never reach those two
functions (items 32, 36, 37 are exactly that: a segment-count gate, a pipe-operator call-site, and a
`decompose_container_path` root that has no `Deref`/`Borrow` arm, respectively — none of the three
routes through the exhaustive match at all).

**New this session — a real, verified limit on what the capability-typed-fn-value Stage-1 work buys,
found by construction rather than assumed:** I wrote and ran a fresh probe (not in `tests/security/`
before this session) — a `dyn Trait` method that *returns* a capability-carrying closure:

```kryos
trait Provider { fn get(self: Self) -> fn() -> str }
impl Provider for RealProvider {
    fn get(self: RealProvider) -> fn() -> str { return make_secret_reader(path) }  // @capabilities(fs:read)
}
fn main() {
    let d: dyn Provider = RealProvider {}
    let f = d.get()
    let out = f()
}
```

`kryos run`/`check --strict-capabilities` **both still reject** this program (`E0507` at `d.get()`,
then again at `f()` — the OLD checker's own `Unknown -> all` fires twice, independently, so there is
**no live security regression today**; `kryos-capabilities/checker.rs` is byte-for-byte unmodified
this stage). But `KRYOS_DUMP_FN_EFFECTS=1 kryos check` on the same file shows the **new** inference's
own row for `main` as `{?C3}` — an unresolved, never-bound capability variable, not `{fs:read}` (what
a sound derivation would produce) and not `{all}` (what a safe fail-closed default would produce for
an unresolved position). **This independently confirms the exact gap flagged in this task's own
briefing**: a fn-value routed through a `dyn Trait` method call loses its row in the new type-level
system. It is currently inert (nothing consults `main`'s inferred row for enforcement), but per the
roadmap's own stated Stage-2/3 intent to make this system the enforcement mechanism, wiring it up
*before* fixing dyn-dispatch row propagation and adding an explicit "unresolved-at-generalization
means `Capability::All`, never silently drop" fallback would reproduce invariant 22's exact violation
class the whole redesign exists to eliminate. Two more novel probes this session (`Result::Ok`
payload extracted via `?` inside `deny!()`; a `while let Some(f) = ...` array-derived `Option`
destructure inside `deny!()`) both **correctly rejected**, `[all]` required, both modes — not every
new shape is a hole; these two were not.

**Confirmed this session: the old shape-matching heuristic (`find_companion_container_arg`) is
genuinely deleted, not dormant.** Grepped `kryos-capabilities/src/`: zero live references, only two
comments citing its removal for context. The sole companion-resolution path is `hot_param_companions`,
call-site-shape-blind by construction — this claim from the 2026-08-05/06 documents holds under
re-inspection.

### (b) EMPIRICALLY UNBROKEN

`tests/security_gate.sh`'s 84-check corpus and the wrapper-closure/nested-actor-state repros from
2026-08-05 — re-run live this session, **PASS**, all green, consistent with every prior session.
Docs-examples (74/74) and ecosystem_check — **PASS**. This is real evidence the checker is hardened
against a wide, specific, and growing attack surface. It is **not** evidence of soundness — see (c)
and §1's table: **the red team has still not reached dry.** Assault rounds continued past this
document's own baseline (round 5 → item 36, round 6 → item 37, both dated 2026-08-06/07, i.e. within
the last ~24h of repository time) and each found something new. State plainly, again: **more bugs are
expected.** Two of my own three novel probes this session found nothing — a genuinely negative result,
reported honestly rather than omitted — but that is two data points, not a proof the surface is
covered; the task brief's own dyn-Trait forward-looking gap (found via a *third* novel probe) shows
the pattern is not exhausted even in the parts of the system that don't enforce anything yet.

### (c) ASSUMED / UNTESTED, this session

- `bash tools/loop/kryos-loop.sh gates 2` and `compiler/self-host/test_bootstrap.sh` (alone) — **NOT
  run this session.** Both are known-expensive in this shared workspace (bootstrap alone took ~20 min
  in the 2026-08-06/07 wave per its own LEDGER entry; `gates 2` has a documented history of stalling
  under concurrent-agent contention). Skipping these is a real, disclosed gap in this session's
  coverage, not a silent one — do not read §1's PASS list as covering them. Neither is a
  capability-soundness signal specifically (both test general conformance/self-hosting), so this does
  not change the VERDICT, but it means "gates before commit" per this repo's own operational rule is
  **not fully satisfied by this session alone** and must be run before any commit that changes
  enforcement code.
- CI on HEAD `feb1991`: checked via `gh run list` — **`in_progress`, 2h25m+ elapsed** at review time
  (abnormally long; the prior four pushes on this branch show cancelled/cancelled/failure/cancelled,
  not a clean green streak). This is a genuine, disclosed yellow flag on repository health separate
  from the capability question — not itself capability-model evidence, but relevant to "is CI on HEAD
  green" which the task asked to check honestly.
- Items 33 (actor-to-actor forwarding), 34 (two-hop actor-state alias), 35-static-method, and item
  30's original accessor-call shape — **not independently re-run this session** (time-budgeted this
  session's live-fire evidence toward the six items in §1's table plus the three novel probes); their
  LEDGER entries carry their own executed evidence from the sessions that found them and are trusted
  on that basis, consistent with how the 2026-08-06 document itself trusted items outside its own
  session's direct re-run list. Flagged here explicitly rather than silently folded into "confirmed."
- Full `cargo build --release` (workspace) — not run (no rebuild performed or authorized this
  session, per the repo's own shared-workspace rule).
- FFI/extern surface combined with any of the newly-found bugs, raw-memory builtins as a bypass
  vector, and the wasm32 backend — out of scope this session, same as every prior session; not
  re-examined.

### What "provably sound" would require (unchanged, still true, now with a sharper edge)

A mechanized proof over a small core calculus PLUS a refinement argument that `checker.rs` implements
it — unchanged from 2026-08-05/06. **The sharper edge this session adds:** even the eventual
structural fix (capability-typed fn values) is not a free win — it inherits its own new dispatch
surface (dyn Trait, generic trait-bound methods) that needs the SAME "does every value-producing form
reach one point of resolution" argument the old checker never had, or it will reproduce the identical
failure class in a new codebase. The type-level redesign is still the right direction (it converts an
open-ended enumeration into a closed one, by design), but "the redesign exists" and "the redesign is
finished" are different claims, and Stage 1's own LEDGER entry already disclosed dyn-dispatch and
generic-trait-bound coverage as future work — this session's probe turns that disclosed gap from
theoretical into demonstrated (still inert, not yet a live bug, because nothing enforces off it yet).

---

## 3. Blockers ranked, each with what must be true to clear it

1. **All seven+ open "LIVE CAPABILITY ESCAPE" ledger items (30/35-conditional-expr, 32, 33, 34,
   35-static-method, 36, 37)** — the single largest blocker by count and the only one that gates the
   literal word "capability-safe." Clears when each is fixed at its stated root cause (see each
   item's own LEDGER writeup — every one already names its fix direction), `security_gate.sh` gains a
   wired check for each (closing the "test exists, gate silent" gap named in both this and the prior
   document), and every fix is proven both ways (revert, rebuild, confirm the leak returns; restore,
   rebuild, confirm reject) per this repo's own `CLAUDE.md` rule 6.
2. **The method-level argument, still not made.** Per 2026-08-06 §3 item 5, unchanged: a fifth (now
   an eighth-plus) round finding something new is the expected outcome, not a surprise, until either
   (a) a mechanical argument shows every value-producing `Expr` form passes through ONE enforcement
   point, or (b) the capability-typed-fn-value system replaces the shape-matching checker as the
   actual enforcement mechanism. Given this session's dyn-Trait finding, (b) is not yet safe to ship
   even when ready — the dyn-dispatch row-propagation gap (§2a) must close FIRST, with an explicit,
   tested policy for "a capability row still unresolved at end-of-check" (hard error or `All`, never
   silent drop), before any enforcement authority moves to the new system.
3. **CI health on HEAD.** Not itself a security blocker, but "ready to ship" requires knowing CI is
   green, and it currently is not observably so (`in_progress` 2h25m+, prior runs
   cancelled/cancelled/failure/cancelled). Clears when a CI run on the relevant HEAD completes and
   passes, or the stall is diagnosed as infrastructure (as the 2026-08-06 document found for its own
   HEAD) rather than a real regression.
4. **This session's own two skipped gates** (`gates 2`, bootstrap alone) — clears by running them
   before the next commit that touches enforcement code, per this repo's stated commit discipline.
5. Items carried forward unchanged from 2026-08-06 §3 item 6 (supply chain items 12/13/17, backend
   divergence item 15, resource-DoS items 14/25, Mutex-no-reassign hang item 31, untested surfaces
   26-29, `any` type erasure item 6, struct-argument leak item 3) — none gate "capability-safe"
   specifically, all gate "production ready" generally. Not re-examined this session; still accurate.

---

## 4. Minimum honest launch posture

**Version label:** `v0.9.0`, unchanged. Do not round up to 1.0, do not drop "beta."

**Exact wording required in README's Status section and `docs/10-capabilities.md`'s introduction**
(supersedes the 2026-08-06 wording — that paragraph named four items; the honest count today is
higher and the new type-level system, while real progress, is not itself a fix yet):

> **Capability-model status: hardening in progress, not yet a completed security guarantee.**
> Kryos's deny-by-default capability system has survived a wide adversarial sweep across 8+ red-team
> rounds and is independently re-verified against a live corpus (`tests/security_gate.sh`, 84 checks,
> plus `tests/ecosystem_check.sh` and `python tools/docs-examples/check.py`, all green as of HEAD
> `feb1991`). **At least seven distinct live bypasses of `deny!()` are currently open and unfixed**
> (`tools/loop/LEDGER.md` OPEN items 30, 32, 33, 34, 35 (two distinct bugs share this number — a
> tracking bug in its own right), 36, 37): an accessor-call or conditional-expression method receiver,
> a tuple-indexed fn value, actor-to-actor closure-parameter forwarding, a two-hop local alias of
> actor state, a hot parameter at index 0 of a static method, the pipe operator (`\|>`), and a
> `&`/`*` (borrow/deref) receiver indirection. **Do not depend on `deny!()` to contain an untrusted or
> partially-trusted code path** until these are closed and independently re-verified. A structural
> redesign (capability-typed function values, `kryos-types`) has landed as representation + inference
> only — it changes NOTHING about which programs are accepted or rejected today, and is not yet safe
> to wire to enforcement: it has its own currently-inert gap (a `dyn Trait` method return does not yet
> carry its capability row through the new inference). Track status in `tools/loop/LEDGER.md` and
> `docs/LAUNCH-READINESS.md`. **Across 8+ hardening rounds, every round that closed its own findings
> was followed by a round that found a new bypass shape.** Treat this as a standing property of the
> current design, not a bug count trending to zero.

Do not use "capability-safe" as a completed adjective in any external announcement, landing page, or
marketing description until every item in §3 clears. "Capability-aware," "deny-by-default, hardening
in progress," or the disclosure paragraph above are the accurate framings today — unchanged guidance
from every prior synthesis, because the underlying fact has not changed: the checker still has known,
open, live holes.

---

## 5. CI status, checked honestly (new this session, task explicitly required it)

`gh run list --limit 5` against HEAD `feb1991` and its four predecessors:

| Commit (message prefix) | Status | Duration |
|---|---|---|
| `feb1991` "PipeExpr call-site bypasses..." | **in_progress** | 2h25m+ (abnormal — typical runs are 15m-6h with clear completion) |
| `136b8e7` "repro item 30's decompose_container_path gap..." | completed, **cancelled** | 6h0m |
| `4b2afc4` "deny-narrowing round 2..." | completed, **cancelled** | 3h0m |
| `0d6b426` "add repro for kryos test/repl..." | completed, **failure** | 15m52s |
| `bfe5c57` "add repro for match-arm-bound fn-field..." | completed, **cancelled** | 14m7s |

Not one clean green completion in the last five pushes. The 2026-08-06 document's own precedent
(a `failure` that was infrastructure, not a real regression) means this is not automatically alarming,
but it also cannot be waved away without checking the actual failure — this session did not have
budget to dig into the `0d6b426` failure's logs or wait out the `feb1991` in-progress run. **Disclosed
as an open item (§3.3), not silently omitted and not assumed benign.**

---

## 6. Next steps, in order

1. Fix the seven-plus open capability-escape ledger items, in the order their own entries suggest
   (each names its root cause and fix direction already — this is not a research problem, it's an
   implementation backlog at this point).
2. Wire each fix's repro into `security_gate.sh` as it closes — stop the "test exists, gate silent"
   pattern from recurring an eighth time.
3. Before touching enforcement wiring for the new capability-typed-fn-value system: add `dyn Trait`
   and generic-trait-bound dispatch to that system's own test corpus, and implement an explicit,
   tested policy for an unresolved-at-end-of-check capability row (hard error, never silent `All`
   substitution done implicitly-and-untested, never a silent empty set).
4. Resolve the CI status on HEAD — either confirm infrastructure-only (as before) or fix a real
   regression; do not proceed to a launch decision with unconfirmed CI.
5. Run the two gates this session skipped (`kryos-loop.sh gates 2`, bootstrap alone) before the next
   commit that touches `kryos-capabilities` or `kryos-types`.
6. Once 1-5 are done: re-run this same live re-adjudication fresh (not a trust of this document)
   before writing "capability-safe" in any launch copy — this has now been true, and re-verified true,
   across three consecutive sessions (2026-08-05, -06, -07); expect a fourth to be necessary too.

---

## 7. Prior syntheses — retained for history, nothing lost

- **2026-08-06** (HEAD `0d6b426`): full text preserved in git history and in this file's prior
  version; superseded above. Its VERDICT (LAUNCH-AS-BETA) and its §2(a) PROVEN/§2(b)
  EMPIRICALLY-UNBROKEN framing are reconfirmed, not overturned, by this session.
- **2026-08-05** (HEAD `d29ac99`): full text preserved in git history and in
  `tools/loop/LEDGER.md`'s "FINAL LAUNCH SYNTHESIS (2026-08-05)" section. Its central methodological
  finding — a repro file without a ledger entry is not tracked, regardless of who wrote it — held
  again in the 2026-08-06 session and was not contradicted this session either (all six attack files
  in this session's §1 table already had LEDGER entries; the discipline is holding).

**The pattern across three sessions is itself the strongest single piece of evidence in this
document:** each session finds and closes nothing on its own initiative (this session fixed zero
bugs, by design — verification-only), independently re-confirms the open count is real, and finds at
least one thing the prior session's document did not fully cover (this session: the dyn-Trait
inference gap, CI health). Overstating "capability-safe" at this HEAD would be the single most
expensive error available in this repository's current state — the evidence says NOT YET, plainly,
for the third consecutive session.
