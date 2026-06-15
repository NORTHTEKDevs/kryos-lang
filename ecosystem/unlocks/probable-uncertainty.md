# Kryos Unlock: probable-uncertainty

Cluster: `std.probable` -- Confidence-Aware Values and Uncertainty Propagation

Source verified: `/c/Users/Krist/projects/active/kryos-lang/compiler/stdlib/probable.kry`
Test verified: `tests/smoke/test_probable_generic.kry`
Usage in production example: `examples/showcase/budget_analyst.kry`

---

## What is actually implemented today

`Probable<T>` is a generic struct with three fields: `value: T`, `confidence: f64`, and `source: str`. The confidence is clamped to [0.0, 1.0] on construction.

The following functions are implemented (all free functions -- generic `impl` blocks not yet supported by the checker):

- `probable(value, confidence)` -- construct
- `certain(value)` -- confidence = 1.0
- `with_source(p, source)` -- tag provenance
- `map_value(p, fn)` -- transform value; confidence is preserved (deterministic transform adds no new uncertainty)
- `is_confident(p, threshold)` -- boolean gate
- `require_confidence(p, threshold)` -- unwrap or throw
- `or_else(p, fallback)` -- unwrap or fallback at default threshold 0.5
- `combine(a, b, fn)` -- product rule: confidence = a.confidence * b.confidence
- `entropy(p)` -- Shannon entropy of the binary outcome (log2-based)
- `best_of(predictions)` -- highest confidence wins from an ensemble
- `majority_vote(predictions)` -- confidence-weighted ensemble vote; result confidence = winner's share of total

There is no operator overloading for arithmetic on `Probable<T>` -- `a + b` where `a: Probable<f64>` does NOT work. Confidence propagation through expressions requires explicit `combine()` calls. This is a real gap relative to probabilistic-programming languages.

There is no calibration tooling (no Platt scaling, no isotonic regression, no calibration curve). Confidence values are authored by the programmer or passed through from external model logprobs; the language does not verify them.

There is no distinction between aleatory uncertainty (irreducible randomness) and epistemic uncertainty (lack of knowledge). The single `f64` mixes them.

---

## Comparison to the alternatives

### vs. a plain f64 confidence field

A plain float in a struct is what most languages actually do in practice. `Probable<T>` adds:
- A named semantic (confidence is not a price, a weight, or a percentage)
- Ensemble combinators (`best_of`, `majority_vote`) that encode correct aggregation rules in the stdlib rather than being re-invented per project
- Source tagging for audit trails
- Shannon entropy as a free function
- `require_confidence` as a typed gate (throws rather than silently using a shaky value)

The gap vs. a float: none of this is prevented by a float. Developers routinely build these same helpers. The question is whether having them in the stdlib, integrated with the language's type system and AI-focused framing, changes how AI agent code is written. For agent governance, having these in the same stdlib as `std.tracked`, `std.cost`, and `@budget` matters -- they compose.

### vs. probabilistic programming languages (Stan, Pyro, Edward)

This is the "hype" boundary. Stan, Pyro, NumPyro, and similar systems do:
- Full posterior inference (MCMC, VI, SVI)
- Automatic differentiation through probability distributions
- Named random variables with defined prior/likelihood structure
- Bayesian model specification
- Calibration of predictive distributions against held-out data

`Probable<T>` does none of this. It is a confidence-tagged value container, not a probabilistic model. Calling it "probabilistic programming" would be false advertising. The correct framing is: `Probable<T>` is a first-class uncertainty label for AI output values, not a probabilistic computation substrate.

### vs. Rust effect-system crates / confidence types

Rust has no built-in uncertainty type, but the same struct is trivially expressible. Several ML inference crates return `(value, confidence)` tuples. The difference in Kryos is that `Probable<T>` is in the standard library, is generic since v4.47, and is designed to compose with the rest of the AI-focused stdlib (`std.tracked`, `std.agent`, `std.llm`). You do not have to reach for a third-party crate.

### vs. Python dataclasses with a confidence field

Python's `dataclasses.dataclass` with a `confidence: float` field is what most ML engineers actually write. Kryos offers compile-time type checking, propagation helpers in stdlib, and integration with capability enforcement. Python has the ecosystem (calibration, metrics, proper distributions). Kryos has the governance integration.

---

## Unlocks -- with honest novelty ratings

### 1. Confidence-gated decision points in agent loops

An AI agent that calls a model, wraps the answer in `Probable<T>`, and then routes based on `is_confident(answer, 0.85)` -- "if confident enough, act; if not, escalate or ask for clarification" -- is a pattern that works TODAY in Kryos and is idiomatic in the stdlib. The `budget_analyst.kry` showcase does exactly this.

Novelty: PARTIAL. The pattern exists in every ML inference system. What Kryos contributes is that confidence gating lives in the same type system as capability enforcement and budget limits. An agent that combines `@budget`, `Probable<T>`, and `@capabilities` is expressing governance constraints in one language rather than assembling them from middleware. No mainstream general-purpose language has this combination in its standard library.

Buildable: today.

### 2. Confidence-weighted ensemble arbitration in Kryos agent code

`majority_vote` and `best_of` implement two standard ensemble patterns -- confidence-weighted vote and argmax -- as stdlib functions over `[Probable<T>]`. An agent swarm where each sub-agent returns a `Probable<str>` recommendation and a coordinator picks the winner via `majority_vote` is expressible in Kryos without third-party libraries.

Novelty: PARTIAL. These patterns exist in scikit-learn and in Python agent frameworks. Kryos novelty is purely that it is in the stdlib, is statically typed, and is integrated with `std.agent`. The algorithms themselves are textbook.

Buildable: today.

### 3. Audit-ready confidence-tagged AI output (composing with std.tracked)

The `budget_analyst.kry` example shows the real compositional value: the final answer is both a `Tracked<str>` (full lineage of how it was derived) and a `Probable<str>` (confidence tag from the model's certainty). Compliance-critical applications -- financial analysis, medical triage hints, regulatory reporting -- can log not just what the model said but how confident it was and which tools were invoked to reach that answer.

Novelty: PARTIAL. OpenTelemetry traces + a confidence float in the log body achieves similar observability. The Kryos difference is that the type system forces the developer to at least acknowledge confidence (you produce a `Probable<str>`, not a bare `str`) and the lineage is embedded in the value, not wired up externally. This is cleaner than assembling it from otel + custom metadata, but it is not conceptually novel.

Buildable: today.

### 4. Uncertainty propagation through agent sub-task composition (needs language work)

The most interesting thing `Probable<T>` could do -- and does NOT yet do -- is propagate confidence automatically through expressions and function calls. In a true uncertainty-aware language, `a: Probable<f64> + b: Probable<f64>` would produce a `Probable<f64>` whose confidence is `a.confidence * b.confidence` (independence assumption) or some tighter bound. Today you must write `combine(a, b, fn (x, y) { return x + y })` explicitly.

Operator overloading on `Probable<T>` would require either:
- Trait/typeclass support in Kryos (not yet implemented -- "generic `impl` blocks not yet supported by the checker" is stated in the file header)
- Or a macro/deriving mechanism for `+`, `-`, `*`, `/` on `Probable<T>`

Until then, complex pipelines that chain 5+ uncertain values require verbose `combine` chains. This undercuts the ergonomic advantage over "just use a float."

Novelty if implemented: TRULY-NOVEL in a systems language. Stan/Pyro handle this in inference mode; no systems language in the Rust/Go/Kryos tier has operator-level uncertainty propagation in its stdlib.

Buildable: needs language work (operator overloading / trait-based dispatch on generic types).

### 5. Calibration-aware agent decisions (needs language work + runtime)

A production AI system needs to know whether its confidence values mean anything. An LLM that says "I'm 90% confident" is typically overconfident; a well-calibrated model's stated 70% should be right 70% of the time. Kryos has no calibration tooling: no ECE computation, no reliability diagram generation, no Platt scaling, no temperature scaling.

Adding calibration to `std.probable` would require:
- A `CalibrationRecord { predicted: f64, actual: bool }` accumulator
- An ECE (Expected Calibration Error) function
- Optional: isotonic regression or Platt scaling as transform functions that output a re-scaled `Probable<T>`

This is buildable in pure Kryos today as a user library (no language features required), but it is not in stdlib. Without it, confidence values in Kryos programs are assertions, not measurements.

Novelty if in stdlib: PARTIAL -- calibration is standard in ML but unusual at the language stdlib level.

Buildable: today as a user library; should be part of a future `std.probable.calibration` submodule.

---

## What NOT to claim

- `Probable<T>` is not a probabilistic programming system. Do not position it against Stan or Pyro.
- Confidence propagation through arithmetic is NOT automatic -- it requires explicit `combine()`. Do not claim "automatic uncertainty propagation" in marketing copy until operator overloading lands.
- The `combine` function uses the product rule (independence assumption). This is incorrect for correlated sources. There is no Bayesian update function, no covariance, no correlation parameter. Confidence arithmetic in Kryos is a first approximation, not a rigorous probability calculus.
- `entropy` computes Shannon entropy of the binary event (confident vs. not confident). It is not the entropy of a full probability distribution over the value space.

---

## Proposed additions to std.probable

### 1. `propagate_and(a, b)` / `propagate_or(a, b)` -- named product/sum rules

```kryos
fn propagate_and<T>(a: Probable<T>, b: Probable<T>, combine_fn: fn(T, T) -> T) -> Probable<T>
fn propagate_or<T>(a: Probable<T>, b: Probable<T>, combine_fn: fn(T, T) -> T) -> Probable<T>
```

`propagate_and` = `combine` (product rule, already exists; this is a rename for clarity).
`propagate_or` = `1 - (1-a.confidence)*(1-b.confidence)` (union rule for independent events) -- "either source being confident is enough." Useful when aggregating votes where any strong signal should count.

### 2. `threshold_map<T>(p, threshold, fn, fallback)` -- combinator for gated transforms

```kryos
fn threshold_map<T, U>(p: Probable<T>, threshold: f64, f: fn(T) -> U, fallback: U) -> U
```

If `p.confidence >= threshold`, apply `f(p.value)` and return the result. Otherwise return `fallback`. Eliminates the `if is_confident(p, t) { f(p.value) } else { fallback }` boilerplate that appears in every agent decision loop.

### 3. `downgrade<T>(p, factor)` -- explicit confidence penalty

```kryos
fn downgrade<T>(p: Probable<T>, factor: f64) -> Probable<T>
```

Multiply confidence by `factor` (clamped to [0,1]). Used when a downstream step introduces known uncertainty (e.g. "this translation introduces 20% uncertainty") without changing the value. Currently requires reconstructing the struct by hand.

### 4. `CalibrationTracker` + `ece()` -- minimal calibration support

```kryos
struct CalibrationSample {
    predicted_confidence: f64,
    was_correct: bool
}

struct CalibrationTracker {
    samples: [CalibrationSample],
    bins: i64
}

fn calibration_tracker(bins: i64) -> CalibrationTracker
fn add_sample(t: CalibrationTracker, predicted: f64, was_correct: bool) -> CalibrationTracker
fn ece(t: CalibrationTracker) -> f64   // Expected Calibration Error
fn overconfident(t: CalibrationTracker) -> bool
```

This does not require any language features not present today. It is a pure Kryos stdlib addition. Without it, developers cannot verify that their `Probable<T>` confidence values are meaningful.

### 5. `filter_confident<T>(predictions, threshold)` -- ensemble filtering

```kryos
fn filter_confident<T>(predictions: [Probable<T>], threshold: f64) -> [Probable<T>]
```

Return only elements whose confidence meets the threshold. Currently users write this manually. Common enough to belong in stdlib.

---

## Language work needed for the highest-value unlock

The biggest unlock -- operator-level uncertainty propagation -- requires trait/typeclass support for `Add`, `Sub`, `Mul`, `Div` on `Probable<T>`. The file header explicitly states "generic `impl` blocks are not yet supported by the checker." Until that lands, `Probable<T>` is an ergonomic but not automatic uncertainty system. The correct roadmap note is:

> When Kryos gains trait-based operator overloading, `Probable<T>` becomes the first systems-language stdlib type that propagates uncertainty through ordinary arithmetic expressions -- a genuine first-class novelty in the systems tier.

Everything else listed above is buildable today, in pure Kryos, with no language changes.
