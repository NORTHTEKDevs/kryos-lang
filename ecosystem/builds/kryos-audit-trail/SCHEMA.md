# SCHEMA.md -- EU AI Act Annex IV field mapping

`kryos-audit-trail` turns a Kryos `Tracked<T>` value into a JSON record whose
fields map onto the technical-documentation points the EU AI Act (Annex IV,
read with Article 13) requires high-risk AI systems to keep. Provenance is not
written to a side-channel log: the `lineage` array **is** the value's own causal
history, populated by every `transform` / `inference` / `annotate` call at the
moment it happened.

> This library produces a machine-readable technical artifact. It is **not** a
> certificate of legal compliance. Actual compliance for a high-risk system
> additionally requires a notified-body assessment, organizational procedures,
> and an Article 13 user-facing transparency notice. See "Honest scope" below.

## Record shape

```json
{
  "schema_version": "eu-ai-act-annex-iv-v1",
  "generated_at": 1781508934,
  "system_id": "loan-approval-v2",
  "user_id": "user-8821",
  "decision": { "value": "...", "source": "form_upload", "source_description": "..." },
  "lineage": [ { "step": 0, "operation": "source", "description": "...", "timestamp": 1781508934 } ],
  "capability_surface": ["net", "db"],
  "cost": { "wall_time_ms": 450, "tokens_used": 1200, "api_calls": 2, "money_usd": 0.024, "energy_kwh": 0.0003 },
  "annex_iv": { "traceability": "PASS", "human_oversight_recorded": true, "data_lineage_steps": 4, "model_identified": true, "cost_metered": true },
  "confidence": 0.91
}
```

## Field mapping

| EU AI Act Annex IV / Art. 13 requirement | Record field | Kryos source |
|---|---|---|
| Intended purpose / system identification | `system_id` | caller-supplied |
| Affected user / subject of decision | `user_id` | caller-supplied |
| The system output (decision) | `decision.value` | `Tracked<str>.value` (faithful) or caller string via `audit_record` |
| Input-data source | `decision.source` | `Tracked<T>.source` |
| Input-data description | `decision.source_description` | `Tracked<T>.source_description` |
| Computational / processing steps | `lineage[].operation` | `LineageEntry.operation` |
| Step rationale / detail | `lineage[].description` | `LineageEntry.description` |
| Step ordering | `lineage[].step` | array index (gap-free) |
| Timestamps of steps | `lineage[].timestamp` | `LineageEntry.timestamp` |
| Resource consumption | `cost.*` | `std::cost.ComputeCost` |
| Operational boundaries / capability surface | `capability_surface` | caller-supplied `[str]` (caps.json sidecar, project 05) |
| Accuracy / robustness metric | `confidence` | `Probable<T>.confidence` or caller (omitted if `< 0.0`) |
| Traceability of outputs to inputs | `annex_iv.traceability` | derived from `lineage` |
| Human-oversight measures | `annex_iv.human_oversight_recorded` | derived from `lineage` operations |
| Logging of computational steps | `annex_iv.data_lineage_steps` | `len(lineage)` |
| Model / algorithm identification | `annex_iv.model_identified` | derived from `lineage` operations |
| Resource metering present | `annex_iv.cost_metered` | derived from `cost` |
| Record schema identity | `schema_version` | constant `eu-ai-act-annex-iv-v1` |
| Record generation time | `generated_at` | `std::datetime.timestamp()` (Unix seconds) |

## Derived `annex_iv` heuristics

| Field | Rule |
|---|---|
| `traceability` | `"PASS"` when `len(lineage) >= 1`, else `"FAIL: no lineage recorded"` |
| `human_oversight_recorded` | `true` when any `lineage[].operation` contains `review`, `oversight`, or `human` |
| `model_identified` | `true` when any `lineage[].operation` contains `inference`, `model`, or `llm` |
| `data_lineage_steps` | `len(lineage)` |
| `cost_metered` | `true` when any opted-in consumption dimension is non-zero: `money_usd > 0.0`, `tokens_used > 0`, `api_calls > 0`, or `energy_kwh > 0.0`. `wall_time_ms` is excluded -- elapsed time is incidental latency (almost always non-zero), not evidence that resource use was recorded |

`inference(...)` always records `operation = "inference"`, so a model inference
step is detected automatically. For human oversight, annotate with one of the
recommended operation strings below.

### Recommended operation strings (the heuristic is intentionally simple)

- Human oversight: `"human_review"`, `"human_oversight"`, `"compliance_review"`
- Model step: prefer `inference(...)` (sets `"inference"`); or use an operation
  name containing `model` / `llm`.

The detection is a **case-sensitive substring match** over the operation name.
A caller who annotates with, e.g., `"qc_pass"` or `"Review"` (capital R) will not
trip the human-oversight heuristic. Use the recommended strings.

## Limitations (current toolchain, Kryos v4.43)

- **Decisions are typed `Tracked<str>`.** `audit_tracked` takes a concrete
  `Tracked<str>` (not a generic `Tracked<T>`), so the decision value is read
  faithfully -- `decision.value` is the literal decision string on both the
  Cranelift JIT (`kryos run`) and the LLVM AOT (`kryos build --release`) backends.
  This is the natural type for a decision (a label like `"approved"` / `"denied"`).
  For a non-string decision, stringify it at the source --
  `tracked_source(to_string(score), ...)` -- or call `audit_record(decision_str,
  lineage, ...)` directly. (A generic `Tracked<T>` parameter would erase the value
  to an i64 slot and emit an opaque pointer, and would also break AOT codegen --
  hence the concrete type.)
- **`kryos test` is blocked by an upstream codegen bug; tests run via `kryos
  run`.** The `kryos test` runner eagerly JIT-compiles every function in every
  imported module, and the polymorphic `to_string(f64)` mis-compiles under that
  eager path (verifier: "arg 0 has type f64, expected i64"). This trips
  `std::tracked`'s `inference` (`"confidence=" + to_string(confidence)`), which
  any user of `inference()` pulls in -- it is not a defect in this library, and
  `std::tracked`/`std::cost` are otherwise fine under `kryos test`. The tests are
  therefore driven by a `main()` in each test file and run with `kryos run
  tests/test_audit.kry` / `kryos run tests/test_schema.kry`, which use
  reachability-based compilation. The `@test` annotations remain so plain `kryos
  test` works verbatim once the upstream `to_string(f64)` codegen bug is fixed.
- **Whole-number floats serialize as bare integers.** `json_stringify` omits the
  decimal for integral `f64` values, so `confidence: 0.0` renders as `0`,
  `confidence: 1.0` as `1`, and `wall_time_ms: 450.0` as `450`. This is valid
  JSON (RFC 8259 draws no integer/number distinction) and a `{"type":"number"}`
  JSON Schema accepts it, but a strict deserializer that treats JSON integers and
  floats as distinct types should parse `confidence` and the `cost.*` fields as
  floats. Fractional values (`0.91`, `0.024`) are unaffected.

## Honest scope

This targets Annex IV (the machine-readable technical record), not Article 13's
user-facing transparency notice. `schema_version` is a placeholder until the
relevant delegated/implementing act pins the exact field set (high-risk
obligations are enforceable from August 2026). The `capability_surface` is
trusted as caller-supplied; integrate project 05's `caps.json` sidecar for a
verified surface.
