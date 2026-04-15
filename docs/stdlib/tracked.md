# std::tracked

Lineage-aware values that record every transformation applied to them. `Tracked` wraps any value with a provenance log, enabling auditable data pipelines, explainable AI outputs, and compliance-grade tracing.

```kryos
use std::tracked
```

---

## Types

### LineageEntry

A single record in a value's transformation history.

```kryos
struct LineageEntry {
    operation:   str,
    description: str,
    timestamp:   i64,
    metadata:    any
}
```

---

### Tracked

A value paired with its full provenance chain.

```kryos
struct Tracked {
    value:               any,
    lineage:             [LineageEntry],
    source:              str,
    source_description:  str
}
```

---

## Creating Tracked Values

### tracked_source

`tracked_source(value: any, source: str, description: str) -> Tracked`

Wrap `value` in a `Tracked` container, recording `source` (e.g. `"user_upload"`, `"model_inference"`) and a human-readable `description` as the origin entry.

**Example:**
```kryos
use std::tracked

let raw = tracked_source(42, "sensor_feed", "temperature reading in celsius")
```

---

## Methods

### transform

`transform(new_value: any, operation: str, description: str) -> Tracked`

Return a new `Tracked` with `new_value` and a `LineageEntry` appended for this step. The original value is unchanged.

**Example:**
```kryos
use std::tracked

let celsius = tracked_source(100.0, "sensor", "raw temperature")
let fahrenheit = celsius.transform(
    celsius.value * 9.0 / 5.0 + 32.0,
    "unit_conversion",
    "celsius to fahrenheit"
)

println(fahrenheit.value)   // 212.0
```

---

### inference

`inference(model: str, result: any, confidence: f64) -> Tracked`

Append a lineage entry describing an ML model inference step. Records `model` name, `result`, and `confidence` (0.0-1.0).

**Example:**
```kryos
use std::tracked

let input = tracked_source("Hello, world!", "user_input", "raw text")
let classified = input.inference("sentiment-v2", "positive", 0.94)
```

---

### annotate

`annotate(operation: str, description: str) -> Tracked`

Append a lineage entry without changing the value. Useful for marking review steps, approval gates, or checkpoints.

**Example:**
```kryos
use std::tracked

let val = tracked_source(99, "score_engine", "credit score")
let reviewed = val.annotate("compliance_review", "reviewed by risk team on 2026-04-14")
```

---

### explain

`explain() -> str`

Return a human-readable string describing the full lineage chain from source to current value.

**Example:**
```kryos
use std::tracked

let t = tracked_source(0, "system", "counter init")
    .transform(1, "increment", "first tick")
    .transform(2, "increment", "second tick")

println(t.explain())
// source: system -- counter init
// [1] increment -- first tick
// [2] increment -- second tick
```

---

### to_json

`to_json() -> str`

Serialize the `Tracked` value and its full lineage to a JSON string.

**Example:**
```kryos
use std::tracked

let t = tracked_source("hello", "input", "user string")
    .transform("HELLO", "uppercase", "normalize case")

println(t.to_json())
// {"value":"HELLO","source":"input","lineage":[...]}
```

---

## Complete Example

```kryos
use std::tracked

// Auditable data pipeline
let raw_price = tracked_source(49.99, "payment_gateway", "item price USD")

let after_tax = raw_price.transform(
    raw_price.value * 1.08,
    "tax_applied",
    "8% sales tax"
)

let after_discount = after_tax.transform(
    after_tax.value * 0.9,
    "discount_applied",
    "10% loyalty discount"
)

let approved = after_discount.annotate("finance_review", "approved by billing system")

println(approved.value)     // 48.5892
println(approved.explain())
// source: payment_gateway -- item price USD
// [1] tax_applied -- 8% sales tax
// [2] discount_applied -- 10% loyalty discount
// [3] finance_review -- approved by billing system

// ML pipeline with inference tracking
let document = tracked_source("Quarterly results are strong.", "doc_store", "raw report text")

let classified = document
    .inference("topic-classifier-v3", "finance", 0.91)
    .inference("sentiment-v2", "positive", 0.87)

println(classified.to_json())
```
