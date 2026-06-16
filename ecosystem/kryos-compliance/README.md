# kryos-compliance

An **EU AI Act (Annex IV) + SOC 2** compliance report generator for governed
Kryos AI systems.

Most "AI compliance" tooling asks a human to fill in a questionnaire *about* a
system. kryos-compliance does the opposite: it reads the **audit entry** an AI
action already produced -- what it did, where its inputs came from, which
capabilities it exercised, what it cost, whether a human was in the loop, which
model produced it -- and **derives** the compliance verdict mechanically. The
evidence is the action's own record, not a side-channel attestation.

It emits two frames from one entry:

- an **EU AI Act Annex IV** technical-documentation section set (traceability,
  human oversight, capability/authority surface, cost metering, risk
  management), and
- a **SOC 2 Common Criteria (CC-series)** control mapping, where each Annex IV
  concern inherits the same PASS/FAIL verdict against the CC control it
  supports.

Output is a human-facing **markdown report** and a machine-facing **JSON
summary**.

## Why Kryos

The governance facts this tool reports are the ones the Kryos toolchain makes
first-class, so the report is grounded in language-level evidence rather than
convention:

- **Capability/authority surface** is `@capabilities(net, db, ...)` -- a
  compile-time authority bound the compiler enforces, not a comment. An action
  with no declared surface is a finding.
- **Cost metering** is `ComputeCost` / `@budget(tokens, calls)` -- consumption
  is metered at the language level, so "no resource accounting" is detectable.
- **Traceability / lineage** mirrors the `Tracked<T>` provenance concept from
  [`kryos-audit-trail`](../kryos-audit-trail): the lineage *is* the value's own
  history, so an empty lineage is a hard traceability failure.

The generator itself is **pure compute**. Every library function is annotated
`@capabilities(compute)`; it touches no `io`/`net`/`process` builtin, so the
compiler *proves* the reporter's own authority is minimal. It passes
`kryos check --strict-capabilities`.

## The findings

Each finding is a binary, machine-checkable verdict over one `AuditEntry`:

| Annex IV concern        | FAIL when                                              | SOC 2 |
|-------------------------|--------------------------------------------------------|-------|
| Traceability            | no data lineage (sources) recorded                     | CC2.1 |
| Human oversight         | a **high-risk** action ran with no human in the loop   | CC1.3 |
| Model identification    | no model identifier attributed to the decision         | CC7.1 |
| Capability surface      | no capability/authority surface declared               | CC6.1 |
| Cost metering           | neither tokens nor spend recorded                      | CC7.2 |
| Risk management         | a high-risk action is missing a mandated safeguard     | CC3.2 |

Human oversight is required only for high-risk actions (EU AI Act Art. 14);
limited-risk actions pass without it. The overall verdict is PASS only when
every finding passes.

## Usage

```bash
KRYOS=/path/to/kryos.exe

# Render markdown + JSON for a compliant and a violating sample action:
$KRYOS run src/main.kry

# Redirect the markdown / JSON to files if you want artifacts:
$KRYOS run src/main.kry > report.md
```

Build an entry and report it from your own code:

```kryos
use model
use findings
use report

@capabilities(compute)
fn report_action() -> str {
    let e = audit_entry(
        "loan application approved",      // action
        "underwriting-model-v2",          // model_id
        ["form_upload", "fico:712", "underwriting-model-v2", "compliance_review"],  // sources / lineage
        ["net", "db"],                    // capabilities_used
        1200,                             // cost_tokens
        0.024,                            // cost_usd
        true,                             // human_oversight
        true                              // high_risk
    )
    let f = evaluate(e)
    return render_report(e, f)            // or report_json(e, f)
}
```

### Sample output (violation: missing lineage, no oversight on a high-risk action)

```
### 1. Traceability and Data Lineage
- **Status:** FAIL
- no data lineage recorded: the decision cannot be reconstructed from its sources
- Recorded sources: none

### 2. Human Oversight (Art. 14)
- **Status:** FAIL
- high-risk action ran with NO human oversight (Art. 14 violation)
```

```json
{ "overall": "FAIL", "passed": 3, "failed": 3, ... }
```

## Files

- `src/model.kry` -- the `AuditEntry` data model + risk-tier label.
- `src/findings.kry` -- the six PASS/FAIL findings + verdict aggregation.
- `src/soc2.kry` -- the SOC 2 CC-series control mapping.
- `src/report.kry` -- markdown report + native-`json_*` JSON summary.
- `src/main.kry` -- a compliant + a violating sample action, end to end.
- `tests/test_compliance.kry` -- the executable proof driver.

## Tests

The driver is a **`kryos run`** program (not `kryos test`): the reporter prints
`f64` costs, and the `kryos test` `@test` JIT harness mis-compiles
`to_string(f64)` even on unexecuted call sites. Each check runs
`require(cond, msg)`, which prints `FAIL: ...` and `exit(1)`s on the first
failure; reaching the pass summary is the proof.

```bash
$KRYOS check --strict-capabilities src/main.kry          # exit 0
$KRYOS check --strict-capabilities tests/test_compliance.kry  # exit 0
$KRYOS run tests/test_compliance.kry                     # exit 0, prints ALL CHECKS PASSED
```

```
PASS: compliant entry -> overall PASS, 6/6 findings pass
PASS: violation entry -> overall FAIL, traceability + oversight + risk FAIL (3 failed)
PASS: SOC 2 CC-series mapping stable (CC2.1/CC1.3/CC6.1/CC7.2/CC3.2)
PASS: JSON summary + markdown field assertions
ALL CHECKS PASSED
```

The sample set is one compliant high-risk action and one high-risk action with
its lineage missing; the driver asserts the violation is flagged on
**traceability AND human oversight** (and the derived risk-management finding),
that the compliant action passes all six findings, and asserts specific JSON and
markdown field values. The suite includes a negative control (inverting any one
expectation produces `FAIL: ...` and `exit 1`).

## Honest limits

- **This reports compliance over an entry you give it; it does not collect the
  entry.** Sources, capabilities, cost, oversight and model id must be supplied
  truthfully by the system being audited (the natural producer is the
  [`kryos-audit-trail`](../kryos-audit-trail) / `Tracked<T>` path). Garbage in,
  green out.
- **The findings are a deliberate, documented subset**, not a full Annex IV /
  SOC 2 Type II audit. They cover the mechanically-checkable governance axes
  Kryos surfaces; they are not legal advice and not an attestation.
- **High-risk is a flag on the entry, not an inferred classification.** The
  caller decides whether an action falls under EU AI Act Annex III; this tool
  enforces the *consequences* of that flag (e.g. mandatory human oversight).
- **`kryos run` (Cranelift JIT) is the supported backend** because the reporter
  prints `f64` costs and builds JSON values dynamically. Both are correct on the
  JIT; an AOT (`kryos build --release`) build is not part of the supported path.
- The SOC 2 mapping is a **one-control-per-concern** convenience map; a real SOC
  2 engagement maps evidence to multiple criteria and points of focus.
