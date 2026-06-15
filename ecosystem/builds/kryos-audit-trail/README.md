# kryos-audit-trail

EU AI Act Annex IV compliance reporter, in ~140 lines of pure Kryos code
(~240 with comments) and no compiler changes.

`audit_tracked(t, cost, caps, confidence, system_id, user_id)` turns any
`Tracked<T>` value into a structured JSON record that maps onto the EU AI Act
Annex IV traceability fields -- per-value lineage, capability surface, total
cost, and confidence. The provenance is the value's own causal history, not a
side-channel log: you cannot retrofit it and you cannot forget a step.

## Quickstart

```bash
kryos run src/main.kry | python3 -m json.tool    # loan-approval demo (JIT)
kryos build --release src/main.kry -o audit_demo  # native binary (LLVM AOT)
kryos run tests/test_audit.kry                   # 3 MVP unit tests
kryos run tests/test_schema.kry                  # 5 schema/cost unit tests
kryos check src/main.kry                          # type-check only
```

The demo runs and builds clean on both backends. Tests are run with `kryos run`
(each test file has a `main()` driver) because the `kryos test` runner is blocked
by an upstream `to_string(f64)` codegen bug in `std::tracked`'s `inference` -- not
a defect in this library. See SCHEMA.md "Limitations".

## API

```kryos
use audit

// Convenience: build the record straight from a Tracked<T>.
fn audit_tracked<T>(t: Tracked<T>, cost: ComputeCost, caps: [str],
                    confidence: f64, system_id: str, user_id: str) -> str

// Core: build from an already-stringified decision + its lineage.
// Use this when the verbatim decision value must appear in the record.
fn audit_record(decision: str, lineage: [LineageEntry], source: str,
                source_desc: str, cost: ComputeCost, caps: [str],
                confidence: f64, system_id: str, user_id: str) -> str
```

`confidence`: pass a value `< 0.0` (e.g. `-1.0`) to omit the field; pass
`0.0`-`1.0` to include it.

## Files

| File | Role |
|---|---|
| `src/cost_summary.kry` | `cost_to_json` -- `ComputeCost` -> JSON |
| `src/schema.kry` | `annex_iv_fields` + `annex_iv_schema_version` -- the derived compliance summary |
| `src/audit.kry` | `audit_record`, `audit_tracked`, `lineage_to_json`, `caps_to_json` |
| `src/main.kry` | loan-approval demo |
| `tests/` | `test_audit.kry` (3 MVP tests), `test_schema.kry` (5 tests) |
| `SCHEMA.md` | Annex IV field mapping + heuristics + limitations |

## How it works

JSON is assembled with the native `json_*` builtins (so no `use std::json` is
needed and there is no JsonValue/handle type collision); a JSON value is an i64
handle, exactly as the standard library builds JSON internally. The library
functions are annotated `@capabilities()` -- they are pure (no IO, no net), so a
compliance-record builder cannot accidentally exfiltrate data. Writing the
record to disk or shipping it over the network is the caller's concern and
carries the caller's own capability declarations.

## Status: MVP

Built: the core record, the four helpers, the convenience wrapper, the demo, 8
passing unit tests, and this mapping. Deliberately out of scope (post-MVP):
`audit_stream` batch records, a `caps.json` sidecar reader, schema-validation
mode, a GDPR redaction helper, and the pretty-print variant.
