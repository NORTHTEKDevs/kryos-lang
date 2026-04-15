# std::probable

Confidence-aware values for probabilistic and AI-driven code. `Probable` wraps any value with a confidence score and an optional set of alternatives, enabling explicit handling of uncertainty without scattered null checks or ad-hoc confidence fields.

```kryos
use std::probable
```

---

## Type

```kryos
struct Probable {
    value:        any,
    confidence:   f64,
    alternatives: [any],
    alt_scores:   [f64],
    source:       str
}
```

| Field          | Description                                                        |
|----------------|--------------------------------------------------------------------|
| `value`        | The primary (highest-confidence) value                             |
| `confidence`   | Confidence in `value`, in the range `[0.0, 1.0]`                  |
| `alternatives` | Other candidate values in descending confidence order              |
| `alt_scores`   | Confidence scores corresponding to each entry in `alternatives`   |
| `source`       | Optional label identifying where this value came from              |

---

## Constructors

### probable

`probable(value: any, confidence: f64) -> Probable`

Create a `Probable` with a primary value and its confidence score.

---

### probable_certain

`probable_certain(value: any) -> Probable`

Create a `Probable` with `confidence = 1.0` and no alternatives. Use for deterministic values that must participate in probabilistic pipelines.

**Example:**
```kryos
use std::probable

let p = probable("cat", 0.87)
let certain = probable_certain(42)

println(p.value)       // "cat"
println(p.confidence)  // 0.87
```

---

## Methods

### map

`map(transform_fn: fn(any) -> any) -> Probable`

Apply `transform_fn` to the primary value and each alternative. Confidence scores are preserved.

**Example:**
```kryos
use std::probable

let score = probable(0.75, 0.9)
let percent = score.map(fn(v: f64) -> str { return (v * 100.0) + "%" })
println(percent.value)   // "75.0%"
```

---

### is_confident

`is_confident(threshold: f64) -> bool`

Return `true` if `confidence >= threshold`.

**Example:**
```kryos
use std::probable

let p = probable("spam", 0.92)
println(p.is_confident(0.9))    // true
println(p.is_confident(0.95))   // false
```

---

### require_confidence

`require_confidence(threshold: f64)`

Throw `"ConfidenceTooLow: <confidence> < <threshold>"` if `confidence < threshold`. Use as a guard before acting on uncertain values.

**Example:**
```kryos
use std::probable

let p = probable("approve", 0.6)
p.require_confidence(0.8)   // throws: ConfidenceTooLow: 0.6 < 0.8
```

---

### or_else

`or_else(fallback: any) -> any`

Return `value` if it exists, or `fallback` if the probable value is empty. Primarily useful when `value` may be null.

---

### combine

`combine(other: Probable, combine_fn: fn(any, any) -> any) -> Probable`

Combine two `Probable` values into one. The result's confidence is `min(self.confidence, other.confidence)` and its value is `combine_fn(self.value, other.value)`.

**Example:**
```kryos
use std::probable

let width  = probable(800, 0.95)
let height = probable(600, 0.88)

let area = width.combine(height, fn(w: i64, h: i64) -> i64 { return w * h })
println(area.value)       // 480000
println(area.confidence)  // 0.88
```

---

### entropy

`entropy() -> f64`

Return the Shannon entropy of the probability distribution formed by `confidence` and `alt_scores`. Higher entropy means more uncertainty.

**Example:**
```kryos
use std::probable

let certain = probable_certain("yes")
println(certain.entropy())   // 0.0

// A 50/50 prediction has maximum entropy
```

---

### explain

`explain() -> str`

Return a human-readable summary of the primary value, confidence, and top alternatives.

**Example:**
```kryos
use std::probable

let p = probable("cat", 0.72)
println(p.explain())
// value: "cat" (confidence: 72.0%)
```

---

## Ensemble Functions

### ensemble_majority_vote

`ensemble_majority_vote(predictions: [Probable]) -> Probable`

Aggregate a list of `Probable` values by majority vote. The most frequently occurring primary value wins; confidence is set to the proportion of votes it received.

**Example:**
```kryos
use std::probable

let votes = [
    probable("cat", 0.9),
    probable("cat", 0.85),
    probable("dog", 0.7)
]

let result = ensemble_majority_vote(votes)
println(result.value)       // "cat"
println(result.confidence)  // 0.6667 (2 of 3 votes)
```

---

### ensemble_best_confidence

`ensemble_best_confidence(predictions: [Probable]) -> Probable`

Return the `Probable` from `predictions` with the highest `confidence` score.

**Example:**
```kryos
use std::probable

let models = [
    probable("positive", 0.78),
    probable("positive", 0.93),
    probable("neutral",  0.65)
]

let best = ensemble_best_confidence(models)
println(best.value)       // "positive"
println(best.confidence)  // 0.93
```

---

## Complete Example

```kryos
use std::probable

// Classification pipeline
let classify = fn(text: str) -> Probable {
    // In practice: call a model
    return probable("positive", 0.88)
}

let result = classify("The product exceeded all expectations.")

if result.is_confident(0.8) {
    println("high-confidence classification: " + result.value)
} else {
    println("low confidence -- needs human review")
}

// Multi-model ensemble
let model_outputs = [
    probable("buy",  0.81),
    probable("buy",  0.76),
    probable("hold", 0.62),
    probable("buy",  0.91)
]

let consensus = ensemble_majority_vote(model_outputs)
println(consensus.explain())
// value: "buy" (confidence: 75.0%)

// Transform and combine
let price_pred  = probable(142.50, 0.85)
let volume_pred = probable(10000,  0.79)

let value_pred = price_pred.combine(
    volume_pred,
    fn(p: f64, v: i64) -> f64 { return p * v }
)

value_pred.require_confidence(0.75)
println("expected market value: " + value_pred.value)
```
