# 08 -- kryos-audit-trail: EU AI Act Compliance Reporter

**Pitch:** `audit_record(decision, system_id, user_id)` turns any `Tracked<T>` value into a structured JSON object that satisfies EU AI Act Annex IV traceability fields -- per-value lineage, capability surface, total cost, and confidence -- in ~150 lines of pure Kryos with no compiler changes.

---

## Why This Is Novel

### Novelty Rating: PARTIAL

The EU AI Act Article 13 and Annex IV require high-risk AI systems to log:
- what input data was used
- what processing steps occurred
- what human oversight mechanisms were in place
- what the system's accuracy and robustness characteristics are

Every major AI framework has some logging story. Here is an honest comparison:

| Mechanism | Who Has It | What It Lacks |
|---|---|---|
| OpenTelemetry / spans | Every cloud framework | Trace is a side-channel, not attached to the value. You log separately from computation. Easy to forget a span. |
| LangChain callbacks | Python agent frameworks | Callback fires at run level, not value level. One run = one log. Not per-output. |
| Weights & Biases / MLflow | ML experiment trackers | Run-level logging. No per-inference audit trail. No capability surface. No cost per value. |
| Wasm component model | System-level isolation | Capability surface at module boundary, not per-value. No lineage on the value itself. |
| GDPR-style audit logs | Every enterprise DB | Schema is application-defined. Nothing guarantees the log entry is causally linked to the value it describes. |

What Kryos does differently: the `Tracked<T>` type carries its own provenance. The audit record is generated from the value's `.lineage` field, which was populated causally -- each `transform`, `inference`, or `annotate` call appended an entry at the exact moment it happened. You cannot retrofit a log; the log IS the value's history. This is PARTIAL because the underlying concept (causal provenance) exists in database systems (event sourcing, append-only ledgers) and in some provenance-tracking research systems (W3C PROV). The Kryos version is notable because it is a first-class language type, not a framework convention or a separate logging call.

**TRULY-NOVEL component:** The combination of per-value lineage (`Tracked<T>`) + capability surface (what resources the producing function declared) + language-level cost (`ComputeCost`) + confidence (`Probable<T>`) in a single structured output is not available in any mainstream language today without implementing it as an entire separate compliance framework on top.

### Why Kryos Is the Right Substrate

The EU AI Act compliance requirement is not fundamentally a logging problem -- it is a provenance problem. You need to prove that a specific output came from specific inputs via specific steps. Kryos makes this structurally impossible to forget: once a value is `Tracked<T>`, every subsequent operation that produces a new value from it can call `transform` or `inference`, and the lineage is immutable on the value. Compliance is opt-in at the point where you call `tracked_source`, and then automatic through the rest of the pipeline.

---

## Kryos Primitives Used

All confirmed from source files in `compiler/stdlib/` and `docs/stdlib/`.

### std::tracked (compiler/stdlib/tracked.kry)

```kryos
struct LineageEntry {
    operation:   str,
    description: str,
    timestamp:   i64,
    metadata:    any
}

struct Tracked {
    value:              any,
    lineage:            [LineageEntry],
    source:             str,
    source_description: str
}
```

Key methods used:
- `tracked_source(value, source, description) -> Tracked` -- wrap a value with its origin
- `.transform(new_value, operation, description) -> Tracked` -- append a step
- `.inference(model, result, confidence) -> Tracked` -- record a model call
- `.annotate(operation, description) -> Tracked` -- record a checkpoint (human review, etc.)
- `.to_json() -> str` -- serialize value + full lineage to JSON
- `.explain() -> str` -- human-readable lineage chain

### std::cost (compiler/stdlib/cost.kry)

```kryos
struct ComputeCost {
    wall_time_ms: f64,
    tokens_used:  i64,
    api_calls:    i64,
    money_usd:    f64,
    energy_kwh:   f64
}
```

Used for attaching total accumulated cost to the audit record.

### std::probable (compiler/stdlib/probable.kry)

```kryos
struct Probable {
    value:        any,
    confidence:   f64,
    alternatives: [any],
    alt_scores:   [f64],
    source:       str
}
```

Used to detect whether the decision value has a confidence wrapper and, if so, include it in the audit record.

### std::agent (compiler/stdlib/agent.kry)

```kryos
struct AuditEntry {
    id:          str,
    entry_type:  str,
    description: str,
    success:     bool,
    timestamp:   i64,
    cost_usd:    f64,
    latency_ms:  f64
}
```

`AuditEntry` is the agent framework's existing action-level record. For integration with agent runs, we can accept a `[AuditEntry]` and cross-reference tool calls in the lineage.

### std::json (compiler/stdlib/json.kry)

Used to assemble the compliance JSON output without raw string concatenation:
- `json_object(keys, values) -> JsonValue`
- `json_array(items) -> JsonValue`
- `json_string(v) -> JsonValue`
- `json_number(v) -> JsonValue`
- `json_bool(v) -> JsonValue`
- `stringify(val) -> str`
- `pretty_print(val, indent) -> str`

### @capabilities (kryos-capabilities crate)

The library functions themselves are pure (no IO, no net). They can be annotated `@capabilities()` (empty) to signal this. Callers that write audit records to disk or over the network carry their own capability declarations. This is important for the compliance story: a pure audit-record builder cannot accidentally exfiltrate data.

### No Language Work Required

This library requires:
- No new compiler features
- No new stdlib modules
- No runtime changes
- No capability system extensions

Everything needed is already in the four confirmed-implemented modules above. The `@budget` attribute is NOT needed here (this library is not itself an LLM caller). The library works with values produced by budget-annotated callers but does not itself consume budget.

---

## Architecture

### Components

```
kryos-audit-trail/
  src/
    main.kry         -- entry point and demo (for kryos run)
    audit.kry        -- core library: audit_record, lineage_to_json, annex_iv_fields
    schema.kry       -- EU AI Act Annex IV field definitions and validation
    cost_summary.kry -- ComputeCost -> JSON mapper
  tests/
    test_audit.kry
    test_schema.kry
  kryos.toml
  README.md
  SCHEMA.md          -- documents which Annex IV fields map to which Kryos fields
```

### Data Model

The output of `audit_record` is a JSON object with this structure:

```json
{
  "schema_version": "eu-ai-act-annex-iv-v1",
  "generated_at": 1718300000,
  "system_id": "loan-approval-v2",
  "user_id": "user-8821",
  "decision": {
    "value": "approved",
    "source": "underwriting_model",
    "source_description": "primary loan scoring pipeline"
  },
  "lineage": [
    {
      "step": 0,
      "operation": "data_ingestion",
      "description": "raw applicant data from form submission",
      "timestamp": 1718299900,
      "metadata": null
    },
    {
      "step": 1,
      "operation": "model_inference",
      "description": "credit score model v4.2",
      "timestamp": 1718299950,
      "metadata": null
    },
    {
      "step": 2,
      "operation": "compliance_review",
      "description": "fair lending check passed",
      "timestamp": 1718299980,
      "metadata": null
    }
  ],
  "capability_surface": ["net", "db"],
  "cost": {
    "wall_time_ms": 450.0,
    "tokens_used": 1200,
    "api_calls": 2,
    "money_usd": 0.024,
    "energy_kwh": 0.0003
  },
  "confidence": 0.91,
  "annex_iv": {
    "traceability": "PASS",
    "human_oversight_recorded": true,
    "data_lineage_steps": 3,
    "model_identified": true,
    "cost_metered": true
  }
}
```

### EU AI Act Annex IV Field Mapping

| Annex IV Requirement | Kryos field | Source |
|---|---|---|
| Description of intended purpose | `system_id` | caller-supplied |
| Accuracy and robustness metrics | `confidence` | `Probable<T>.confidence` |
| Human oversight measures | lineage entries with `annotate` | `Tracked<T>.lineage` |
| Input data description | `decision.source_description` | `Tracked<T>.source_description` |
| Computational steps | `lineage[].operation` | `LineageEntry.operation` |
| Timestamps | `lineage[].timestamp` | `LineageEntry.timestamp` |
| Resource consumption | `cost` | `ComputeCost` |
| Capability surface | `capability_surface` | caps.json sidecar (from project 01) |

### Key Functions to Write

**audit.kry:**

```kryos
use std::tracked
use std::cost
use std::json
use std::datetime

// Primary entrypoint.
// decision: a Tracked value (the AI system's output)
// cost:     accumulated ComputeCost for the pipeline run
// caps:     capability strings from caps.json sidecar (or empty list)
// confidence: pass -1.0 to omit; pass 0.0-1.0 to include
// system_id, user_id: caller-supplied context
fn audit_record(
    decision:   any,
    lineage:    [LineageEntry],
    source:     str,
    source_desc: str,
    cost:       ComputeCost,
    caps:       [str],
    confidence: f64,
    system_id:  str,
    user_id:    str
) -> str {
    let now = datetime_now_unix()

    let decision_obj = json_object(
        ["value", "source", "source_description"],
        [
            json_string(to_string(decision)),
            json_string(source),
            json_string(source_desc)
        ]
    )

    let lineage_arr = lineage_to_json(lineage)
    let caps_arr    = caps_to_json(caps)
    let cost_obj    = cost_to_json(cost)
    let annex_obj   = annex_iv_fields(lineage, cost, confidence)

    let mut keys   = ["schema_version", "generated_at", "system_id",
                      "user_id", "decision", "lineage",
                      "capability_surface", "cost", "annex_iv"]
    let mut vals   = [
        json_string("eu-ai-act-annex-iv-v1"),
        json_number(now as f64),
        json_string(system_id),
        json_string(user_id),
        decision_obj,
        lineage_arr,
        caps_arr,
        cost_obj,
        annex_obj
    ]

    if confidence >= 0.0 {
        keys = push(keys, "confidence")
        vals = push(vals, json_number(confidence))
    }

    return stringify(json_object(keys, vals))
}
```

**lineage_to_json helper:**

```kryos
fn lineage_to_json(lineage: [LineageEntry]) -> JsonValue {
    let mut items: [JsonValue] = []
    let mut i = 0
    while i < len(lineage) {
        let entry = lineage[i]
        let obj = json_object(
            ["step", "operation", "description", "timestamp"],
            [
                json_number(i as f64),
                json_string(entry.operation),
                json_string(entry.description),
                json_number(entry.timestamp as f64)
            ]
        )
        items = push(items, obj)
        i = i + 1
    }
    return json_array(items)
}
```

**cost_to_json helper:**

```kryos
fn cost_to_json(c: ComputeCost) -> JsonValue {
    return json_object(
        ["wall_time_ms", "tokens_used", "api_calls", "money_usd", "energy_kwh"],
        [
            json_number(c.wall_time_ms),
            json_number(c.tokens_used as f64),
            json_number(c.api_calls as f64),
            json_number(c.money_usd),
            json_number(c.energy_kwh)
        ]
    )
}
```

**annex_iv_fields helper -- derives compliance fields from data:**

```kryos
fn annex_iv_fields(lineage: [LineageEntry], cost: ComputeCost, confidence: f64) -> JsonValue {
    // human oversight = any lineage entry whose operation contains "review" or "oversight"
    let mut human_noted = false
    let mut model_noted = false
    let mut i = 0
    while i < len(lineage) {
        let op = lineage[i].operation
        if contains(op, "review") || contains(op, "oversight") || contains(op, "human") {
            human_noted = true
        }
        if contains(op, "inference") || contains(op, "model") || contains(op, "llm") {
            model_noted = true
        }
        i = i + 1
    }

    let traceability = "PASS"
    if len(lineage) == 0 {
        traceability = "FAIL: no lineage recorded"
    }

    return json_object(
        ["traceability", "human_oversight_recorded", "data_lineage_steps",
         "model_identified", "cost_metered"],
        [
            json_string(traceability),
            json_bool(human_noted),
            json_number(len(lineage) as f64),
            json_bool(model_noted),
            json_bool(cost.money_usd > 0.0 || cost.tokens_used > 0)
        ]
    )
}
```

**Top-level convenience wrapper (takes a `Tracked` value directly):**

Note: `Tracked` stores `.value`, `.lineage`, `.source`, `.source_description` but is typed as `any` fields internally. The wrapper pattern below reads the fields and calls `audit_record`. Because `Tracked` is a struct with known field names, field access works normally.

```kryos
// Convenience: build audit record directly from a Tracked value.
// caps and cost must be supplied by the caller (Tracked does not carry them).
fn audit_tracked(
    t:          Tracked,
    cost:       ComputeCost,
    caps:       [str],
    confidence: f64,
    system_id:  str,
    user_id:    str
) -> str {
    return audit_record(
        t.value,
        t.lineage,
        t.source,
        t.source_description,
        cost,
        caps,
        confidence,
        system_id,
        user_id
    )
}
```

**Pretty-print variant for human review:**

```kryos
fn audit_record_pretty(
    t:          Tracked,
    cost:       ComputeCost,
    caps:       [str],
    confidence: f64,
    system_id:  str,
    user_id:    str
) -> str {
    let compact = audit_tracked(t, cost, caps, confidence, system_id, user_id)
    let parsed  = json::parse(compact)
    return json::pretty_print(parsed, 2)
}
```

---

## Dependency on Projects 02 and 05

The project spec lists `depends_on: 02 05`.

**Project 02 (governed-agent stdlib extension)** adds `tracked_cost(t, cost, desc) -> Tracked` which attaches a `ComputeCost` to a value's lineage as a structured metadata annotation. If project 02 is built first, callers can pass `t.lineage` and the lineage entries will already contain cost metadata. The audit library still works without project 02 -- callers just pass the cost separately.

**Project 05 (capability badging)** produces a `caps.json` sidecar for a Kryos project that lists the declared capabilities of each function. The audit library reads this sidecar (via `file_read`) to populate `capability_surface` in the record. If project 05 is not present, the caller passes an empty `[]` for caps. The audit record is still valid -- the `capability_surface` field will be `[]` rather than a populated list.

Both dependencies are additive -- the core `audit_record` function works standalone.

---

## MVP Scope

The smallest shippable slice is:

1. `audit_record(...)` function -- all fields required by Annex IV that are derivable from `Tracked` + `ComputeCost` + caller-supplied IDs. Output: compact JSON string.
2. `lineage_to_json`, `cost_to_json`, `annex_iv_fields` helpers.
3. `audit_tracked(t, cost, caps, confidence, system_id, user_id)` convenience wrapper.
4. `SCHEMA.md` -- one-page mapping of Annex IV field names to Kryos struct fields.
5. Tests: 3 cases (no lineage, short lineage, lineage with model+review steps).

This is approximately 150 lines of Kryos + 50 lines of schema documentation.

### Full Vision (post-MVP)

- `audit_stream(decisions: [Tracked], ...)` -- batch audit for a pipeline run.
- Integration with `std.agent.AuditEntry` -- merge agent tool-call log into the lineage.
- Caps sidecar reader -- read `caps.json` from the project root automatically via `file_read`.
- Schema validation mode -- `audit_validate(record_json) -> [str]` returns a list of missing required fields.
- GDPR right-to-erasure helper -- `audit_redact(record_json, fields_to_nullify) -> str`.
- Optional write-to-file: `audit_append(record_json, path)` appends NDJSON (newline-delimited JSON) to an audit log file, requiring `@capabilities(io)`.

---

## Build Plan

A fresh session can follow these steps in order. Each step is independently verifiable.

### Step 1 -- Set up the project

```bash
mkdir kryos-audit-trail
cd kryos-audit-trail
```

Create `kryos.toml`:
```toml
[package]
name = "kryos-audit-trail"
version = "0.1.0"
```

Verify:
```bash
kryos check src/main.kry
```

### Step 2 -- Write `src/cost_summary.kry`

Implement `cost_to_json(c: ComputeCost) -> JsonValue`. This is the simplest helper and has no dependencies on other library code. Write it, run `kryos check`, done.

### Step 3 -- Write `src/schema.kry`

Define the Annex IV field names as `str` constants and implement `annex_iv_fields(lineage, cost, confidence) -> JsonValue`. Unit test: pass an empty lineage, verify `traceability` is "FAIL: no lineage recorded". Pass a two-entry lineage containing "review", verify `human_oversight_recorded` is true.

### Step 4 -- Write `src/audit.kry`

Implement `lineage_to_json`, `caps_to_json`, and the full `audit_record` function. Import `std::json`, `std::datetime`, `std::tracked`. Wire the helpers from steps 2 and 3.

### Step 5 -- Write `src/main.kry` demo

```kryos
use std::tracked
use std::cost

fn main() {
    // Build a tracked decision value
    let raw = tracked_source("applicant_data", "form_upload", "raw loan application")
    let scored = raw.transform("score:712", "credit_score", "FICO v4 model")
    let reviewed = scored
        .inference("underwriting-model-v2", "approved", 0.91)
        .annotate("compliance_review", "fair lending check passed by officer")

    // Simulate accumulated cost
    let total_cost = ComputeCost {
        wall_time_ms: 450.0,
        tokens_used:  1200,
        api_calls:    2,
        money_usd:    0.024,
        energy_kwh:   0.0003
    }

    let record = audit_tracked(
        reviewed,
        total_cost,
        ["net", "db"],
        0.91,
        "loan-approval-v2",
        "user-8821"
    )

    println(record)
}
```

Run:
```bash
kryos run src/main.kry
```

Expected: valid JSON to stdout with all Annex IV fields populated.

### Step 6 -- Write tests

```kryos
// tests/test_audit.kry
use std::test
use std::tracked
use std::cost
use std::json

@test
fn test_empty_lineage_fails_traceability() {
    let t = tracked_source("x", "src", "desc")
    let c = cost_zero()
    let record = audit_tracked(t, c, [], -1.0, "sys", "usr")
    let parsed = json::parse(record)
    let annex = get(parsed, "annex_iv")
    let trace = to_str(get(annex, "traceability"))
    assert(contains(trace, "FAIL"), "empty lineage must fail traceability")
}

@test
fn test_human_review_detected() {
    let t = tracked_source("x", "src", "desc")
        .annotate("human_review", "officer signed off")
    let c = cost_zero()
    let record = audit_tracked(t, c, [], -1.0, "sys", "usr")
    let parsed = json::parse(record)
    let annex = get(parsed, "annex_iv")
    let human = to_bool(get(annex, "human_oversight_recorded"))
    assert(human == true, "review annotate should set human_oversight_recorded")
}

@test
fn test_model_inference_detected() {
    let t = tracked_source("input", "pipeline", "raw")
        .inference("my-model-v1", "output", 0.87)
    let c = cost_zero()
    let record = audit_tracked(t, c, [], 0.87, "sys", "usr")
    let parsed = json::parse(record)
    let annex = get(parsed, "annex_iv")
    assert(to_bool(get(annex, "model_identified")) == true, "inference should set model_identified")
    let conf = to_float(get(parsed, "confidence"))
    assert(conf > 0.8, "confidence should be present")
}
```

Run:
```bash
kryos test tests/test_audit.kry
```

Expected: 3/3 pass.

### Step 7 -- Write SCHEMA.md

One-page document mapping EU AI Act Annex IV points to Kryos output fields. This is the artifact a compliance auditor would reference.

---

## Success Criteria / Demo

The demo is:

```bash
kryos run src/main.kry | python3 -m json.tool
```

(or any JSON formatter -- the output is already valid JSON)

Pass criteria:
1. Output is valid JSON with no parse errors.
2. `annex_iv.traceability` is "PASS" for any input with at least one lineage step.
3. `annex_iv.human_oversight_recorded` is true when the lineage contains an `annotate` call with "review" or "human" in the operation name.
4. `annex_iv.model_identified` is true when the lineage contains an `inference` call.
5. `confidence` field appears only when a non-negative value is passed.
6. `capability_surface` is a JSON array (empty or populated).
7. All 3 unit tests pass.

---

## Risks and Honest Unknowns

### Known risks

**`Tracked` field type is `any` internally.** The `value` field on `Tracked` is typed as `any` in the current stdlib implementation. Converting it to a `str` for the JSON output uses `to_string(t.value)`. This works for primitive values (str, i64, f64, bool) but produces a runtime representation for structs (likely something like `<struct Point>`). The MVP scope handles this by documenting that `value` must be a stringifiable type, or callers should convert before wrapping in `Tracked`.

**No `datetime_now_unix()` builtin confirmed.** The spec uses `datetime_now_unix()` for the `generated_at` timestamp. Check `compiler/stdlib/datetime.kry` for the actual function name. The fallback is `0` (a known placeholder) or `env_get("SOURCE_DATE_EPOCH")` for reproducible builds. This needs verification before step 5.

**`LineageEntry.metadata` is typed `any`.** When serializing to JSON, metadata fields that are structs cannot be automatically converted. The library converts metadata to string via `to_string()` and wraps in `json_string`. If project 02's `tracked_cost` stores a `ComputeCost` struct in metadata, it will serialize as a repr string, not a nested JSON object. A follow-up version can add a `metadata_to_json` hook.

**`annex_iv_fields` heuristic is brittle.** Detecting human oversight by checking if an operation name contains "review" is a text heuristic. A caller who annotates with `"qc_pass"` would not trigger it. The MVP documents this limitation in SCHEMA.md and recommends callers use the specific operation strings `"human_review"`, `"human_oversight"`, or `"compliance_review"`.

### Honest unknowns

**Legal compliance.** This library generates a JSON record that maps to Annex IV fields. It does NOT constitute legal compliance with the EU AI Act. Actual compliance requires a notified body assessment for high-risk systems, organizational procedures, and more. The library is a technical starting point, not a certification artifact.

**Article 13 vs Annex IV.** Article 13 covers transparency obligations to users. Annex IV covers technical documentation. This library targets Annex IV (the machine-readable audit trail). A separate document template would be needed for Article 13 user-facing transparency notices.

**Schema versioning.** The EU AI Act is new (fully enforceable from August 2026 for high-risk systems). The field requirements may be refined by implementing regulations. The schema version string `"eu-ai-act-annex-iv-v1"` is a placeholder; a real implementation would track the specific delegated act version.

**Capability surface accuracy.** The `caps` parameter is caller-supplied. If project 01 (capability manifest extractor) is not used, the caller might supply an incorrect list. The library cannot verify this -- it trusts the caller. A future integration with the caps.json sidecar would make this more reliable.
