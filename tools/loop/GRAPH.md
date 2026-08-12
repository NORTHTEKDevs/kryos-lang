# Kryos completion graph

The work that must be true for Kryos to be finished, as a dependency graph.
Design + rationale: `tools/loop/COMPLETION-LOOP-DESIGN.md`.

**The invariant:** a node is green ONLY if its acceptance command exited 0 at the
current HEAD. Nothing is green by assertion. Status is derived, never typed.
Run `bash tools/loop/graph-run.sh status` — do not trust any prose, including this
file's own table of contents.

**Definition of done:** every node green in a single uninterrupted `verify-all`
at one commit.

**Failure policy:** a node that fails acceptance after 3 genuine attempts converts
to a `documented-limitation` node — the limitation goes into `docs/THREAT-MODEL.md`
and the README with a committed repro test, and the graph proceeds. A documented,
reproducible limitation is shippable; an undocumented one is not.

## Node format

    ### <id>
    deps: <comma-separated ids, or ->
    why: <one line: what this protects against>
    accept: <shell command; exit 0 == green>

Acceptance commands run from the REPO ROOT. They must be non-interactive, must not
rebuild the compiler while a gate run may be using it, and must exit non-zero on
failure rather than printing a sad message and exiting 0.

---

### docs-truth
deps: -
why: On 2026-08-10 the README claimed ONE capability residual while twelve were live, and the LEDGER ranked a fixed item as the top open escape. Both were status lines nobody re-ran. This node makes the docs answer to the harness.
accept: bash tools/loop/check-docs-truth.sh

### escape-instrument
deps: -
why: Two fixes on 2026-08-10 (Borrow/Deref passthrough, TupleLiteral in literal_field_exists) were reasoned, plausible, and did not close their targets, because nobody first measured where those calls actually route. Instrument before editing.
accept: test -f tools/loop/ESCAPE-ROUTING.md && bash tools/loop/check-routing-complete.sh

### stage2-rows
deps: escape-instrument
why: The remaining escapes cannot be closed by the shape-matcher (four failed attempts, logged in ESCAPE-ROUTING.md) and must not be closed by annotating dispatchers `all` (measured 2026-08-12: `all` cascades to every consumer of std::http/std::agent, nullifying the capability system for its flagship use case). Type-directed row enforcement is the answer and stage 1 already exists. Plan: tools/loop/STAGE2-PLAN.md.
accept: bash tools/loop/escape_status.sh | grep -q "STILL ESCAPING: 0" && bash tests/ir_signature_gate.sh >/dev/null && "$PWD/compiler/target/release/kryos.exe" check tests/conformance/conf_stdlib_wave14.kry >/dev/null

### escape-root
deps: stage2-rows
why: The remaining escapes are one root in many syntactic dresses -- enforcement resolves a callee by pattern-matching expression SHAPE, and every unmatched shape falls into a fail-OPEN default. Adding shapes one at a time has not converged across three rounds.
accept: bash tools/loop/escape_status.sh | grep -q "STILL ESCAPING: 0" && bash tests/security_gate.sh >/dev/null && bash tests/ir_signature_gate.sh >/dev/null

### threat-model
deps: escape-root
why: A language whose headline is capability safety must state precisely what that does and does not guarantee. Shipping without a stated threat model is how "capability-safe" became an unqualified claim the implementation could not back.
accept: bash tools/loop/check-threat-model.sh

### gates-full
deps: escape-root
why: The full suite plus self-host bootstrap. Bootstrap is NOT in tier1/tier2 and was broken on master for three days while every other gate stayed green.
accept: bash tools/loop/kryos-loop.sh gates 2 && bash compiler/self-host/test_bootstrap.sh | grep -q "PASS: 16 / 16"

### bench
deps: gates-full, threat-model
why: Published numbers must describe the compiler that actually ships, measured on a named machine at a named commit -- not a mid-flight build, and not a remembered figure.
accept: bash tools/loop/check-bench-current.sh

### release-ready
deps: docs-truth, escape-instrument, stage2-rows, escape-root, threat-model, gates-full, bench
why: The terminating condition. Every node green in one pass at one commit is a state that can be entered, observed and defended -- unlike "no more bugs findable".
accept: bash tools/loop/graph-run.sh verify-all --except release-ready
