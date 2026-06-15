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
kryos run tests/test_postmvp.kry                 # 4 post-MVP unit tests
kryos check src/main.kry                          # type-check only
```

The demo runs and builds clean on both backends. Tests are run with `kryos run`
(each test file has a `main()` driver) because the `kryos test` runner is blocked
by an upstream `to_string(f64)` codegen bug in `std::tracked`'s `inference` -- not
a defect in this library. See SCHEMA.md "Limitations".

## API

```kryos
use audit

// Convenience: build the record straight from a Tracked<str>. The decision
// value is read faithfully (concrete type, not generic). Both backends.
fn audit_tracked(t: Tracked<str>, cost: ComputeCost, caps: [str],
                 confidence: f64, system_id: str, user_id: str) -> str

// Core: build from an already-stringified decision + its lineage.
// Use this when the decision is not a str (stringify it yourself first).
fn audit_record(decision: str, lineage: [LineageEntry], source: str,
                source_desc: str, cost: ComputeCost, caps: [str],
                confidence: f64, system_id: str, user_id: str) -> str
```

`confidence`: pass a value `< 0.0` (e.g. `-1.0`) to omit the field; pass
`0.0`-`1.0` to include it.

### Post-MVP helpers

```kryos
use audit_stream    // batch a pipeline run into one {records:[...]} object
fn audit_stream(decisions: [Tracked<str>], cost: ComputeCost, caps: [str],
                confidence: f64, system_id: str, user_id: str) -> str

use caps_sidecar    // read project-05's capability badge to feed `caps`
fn read_caps_sidecar(path: str) -> [str]     // @capabilities(io)

use validate        // post-hoc validation + GDPR right-to-erasure (both pure)
fn audit_validate(record_json: str) -> [str]              // [] = valid
fn audit_redact(record_json: str, fields_to_nullify: [str]) -> str
```

`read_caps_sidecar` accepts either `["net","db"]` or `{"capabilities":[...]}`
(project 05's flat badge), returning `[]` for a missing/malformed file. It is
the only IO-touching function; the core builders stay pure. `audit_redact`
nullifies the named **top-level** fields (nullify the whole `decision` object
for nested erasure).

## Files

| File | Role |
|---|---|
| `src/cost_summary.kry` | `cost_to_json` -- `ComputeCost` -> JSON |
| `src/schema.kry` | `annex_iv_fields` + `annex_iv_schema_version` -- the derived compliance summary |
| `src/audit.kry` | `audit_record`, `audit_record_handle`, `audit_tracked`, `lineage_to_json`, `caps_to_json` |
| `src/audit_stream.kry` | `audit_stream` -- batch records for a pipeline run |
| `src/caps_sidecar.kry` | `read_caps_sidecar` -- read project-05's capability badge (io) |
| `src/validate.kry` | `audit_validate`, `audit_redact` -- validation + GDPR redaction |
| `src/main.kry` | loan-approval demo |
| `tests/` | `test_audit.kry` (3), `test_schema.kry` (5), `test_postmvp.kry` (4) |
| `SCHEMA.md` | Annex IV field mapping + heuristics + limitations |

## How it works

JSON is assembled with the native `json_*` builtins (so no `use std::json` is
needed and there is no JsonValue/handle type collision); a JSON value is an i64
handle, exactly as the standard library builds JSON internally. The library
functions are annotated `@capabilities()` -- they are pure (no IO, no net), so a
compliance-record builder cannot accidentally exfiltrate data. Writing the
record to disk or shipping it over the network is the caller's concern and
carries the caller's own capability declarations.

## Status

Built: the core record + helpers + convenience wrapper, the demo, and the
post-MVP set -- `audit_stream` (batch), `read_caps_sidecar` (caps.json badge
reader), `audit_validate` (schema validation), and `audit_redact` (GDPR
right-to-erasure). 12 passing unit tests (3 audit + 5 schema + 4 post-MVP) and
this mapping. Still out of scope: a `pretty_print` variant, nested-field
redaction, and live integration with project 05's published badge (the reader
consumes the badge format; 05 produces it).
