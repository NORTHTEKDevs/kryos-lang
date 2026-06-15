# 10 -- kryos-calibration: CalibrationTracker + ECE in std.probable

**One-line pitch:** Add `CalibrationSample`, `CalibrationTracker`, and `ece()` to `std.probable` so that confidence values in `Probable<T>` are measured quantities with a provable calibration error, not programmer assertions.

---

## Why This Is Novel (Honest Novelty Rating)

### The core claim

`Probable<T>` already exists in the Kryos stdlib. It lets every value carry a confidence score. The problem: nothing in the language checks whether those confidence scores are accurate. A function can return `probable(result, 0.9)` and the 0.9 is a guess. Without measurement, `Probable<T>` is expressive but not trustworthy.

Expected Calibration Error (ECE) is the standard metric for measuring whether a model's stated confidence matches its empirical accuracy. If a model says 0.9 confidence on 100 predictions, and only 70 are correct, its ECE is non-zero -- it is overconfident. ECE closes the loop: it turns "this value says it is 90% confident" into "we measured that confidence values in the 0.85-0.95 range were correct 70% of the time."

The claim of this project is specific: no systems language includes ECE as a stdlib concept directly alongside its confidence type. The two belong together. If `Probable<T>` is in `std.probable`, then `CalibrationTracker` and `ece()` belong there too.

### Novelty ratings per axis

**`CalibrationTracker` + `ece()` in the same stdlib module as `Probable<T>` -- PARTIAL, with a meaningful distinction**

Python has `sklearn.calibration.calibration_curve`. NumPy/SciPy have the building blocks. Hugging Face `evaluate` has an ECE metric. These all exist. The partial distinction is:

1. In every existing system, ECE is in a ML framework or stats library, not in the confidence type's own module. The confidence type (`float`) and the calibration tool are in completely different packages with no conceptual link. Kryos puts them in the same `std.probable` module because they are conceptually inseparable. A `Probable<T>` value is only meaningful after you have measured its calibration.

2. No systems language (Rust, C++, Go, Zig, Swift) has either concept at the stdlib level. They all treat probability as a raw float. The fact that Kryos already has `Probable<T>` as a first-class type is what makes adding ECE to the same module coherent rather than arbitrary.

**Novelty rating: PARTIAL.** The ECE algorithm is 10-line textbook math. The novelty is placing it inside the language's confidence type module, establishing calibration measurement as a language-level concern rather than a framework concern.

### Who else does it

- Python / scikit-learn: `calibration_curve`, `CalibrationDisplay`. In a separate ML-specific library, not in the core language.
- Elixir / Nx: `Scholar.Metrics.Classification.expected_calibration_error`. In a third-party ML library.
- Julia: `MLJ` has calibration tools. Again, separate package.
- No other systems language stdlib has any calibration concept at all.

### Why Kryos is the right substrate

Because `Probable<T>` is already in `std.probable`. The substrate already has the type. Adding `CalibrationTracker` to the same module is the natural completion: here is the confidence type, here is how you measure whether it is accurate. Together they let a Kryos program produce AI outputs and prove -- within the program, using stdlib types, with no external ML framework -- that its confidence claims are calibrated. That is the governance argument: not just "this output has confidence 0.85" but "we have measured that our 0.85-confidence outputs are correct 84% of the time."

For kryos-bench-governed (project 07), this closes a stated gap in that spec: "The ECE calculation will reflect calibration of the heuristic, not of the model." Adding `CalibrationTracker` to stdlib lets project 07 call `ece()` on its scored results and report a real calibration number rather than a two-bucket heuristic.

---

## Which Kryos Primitives This Uses

### `std::probable` -- the target module

Source: `compiler/stdlib/probable.kry`.

`CalibrationTracker` and `ece()` are added directly to this file. The existing `Probable<T>` struct (`value: T, confidence: f64, source: str`) is the type that calibration measures. No new types are imported; this is a pure extension of the existing module.

### Language features used

- Structs (`@copy` on both new structs -- they contain only scalar fields: `f64`, `i64`, `bool`).
- Array `[CalibrationSample]` with `push` builtin.
- Arithmetic builtins: `abs`, `sqrt` (available without import per CLAUDE.md).
- Numeric casts: `(n as f64)`.
- `fn` free functions (generic `impl` blocks on generic structs are not yet fully supported; the existing `std.probable` uses free functions throughout, and this spec follows the same pattern).

### Language work needed first

None. Every feature used here -- `@copy` structs, `[T]` arrays, `push`, `abs`, `(x as f64)`, free `fn` at module level -- is confirmed working in both the Cranelift JIT and the LLVM AOT backend per CLAUDE.md and the existing stdlib sources.

**Confirmed runtime builtins available without import:** `abs`, `len`, `push`, `to_string`, `time_now` (used in tracked.kry line 31 -- confirmed present in runtime).

**Honest current limitations (none block this project):**

- Generic `impl<T>` blocks on generic structs have partial support. `CalibrationTracker` and `CalibrationSample` contain no type parameters, so this limitation does not apply.
- `@copy` is valid here because both new structs contain only scalar fields (`f64`, `i64`, `bool`). No heap-bearing fields.
- `ece()` is pure arithmetic. No I/O, no network, no capabilities required.

---

## Architecture

### New types added to `stdlib/probable.kry`

```kryos
/// One observed outcome: what confidence was stated, and whether the
/// prediction was actually correct.
@copy
struct CalibrationSample {
    predicted: f64,
    correct: bool
}

/// Accumulates CalibrationSamples and computes ECE over them.
/// bins: number of equal-width confidence buckets (default 10).
struct CalibrationTracker {
    samples: [CalibrationSample],
    bins: i64
}
```

`CalibrationSample` is `@copy` because both fields are scalar. `CalibrationTracker` contains a `[CalibrationSample]` array (heap-bearing), so it is NOT `@copy` -- pass it as a move value. Following the functional pattern of the existing stdlib, every operation returns a new `CalibrationTracker` rather than mutating in place. This avoids the borrow/alias complexity flagged in CLAUDE.md gotcha 23.

### Constructor

```kryos
/// Create a new CalibrationTracker with a given number of bins.
/// bins = 10 gives deciles (0.0-0.1, 0.1-0.2, ..., 0.9-1.0).
fn calibration_tracker(bins: i64) -> CalibrationTracker {
    if bins < 2 { bins = 2 }
    return CalibrationTracker { samples: [], bins: bins }
}
```

Note: `bins` parameter is `i64`. The guard `if bins < 2 { bins = 2 }` uses a reassignment -- declare the parameter as `let mut bins: i64 = bins` inside the function body, or check conditionally. The CLAUDE.md warns that function parameters are not `let mut` by default in all cases. Safe pattern: copy to a local first.

Corrected form:

```kryos
fn calibration_tracker(bins: i64) -> CalibrationTracker {
    let mut b = bins
    if b < 2 { b = 2 }
    return CalibrationTracker { samples: [], bins: b }
}
```

### Sample accumulation

```kryos
/// Record one prediction outcome. confidence is the value from Probable<T>.confidence.
/// correct is true if the prediction matched the ground truth.
fn add_sample(tracker: CalibrationTracker, confidence: f64, correct: bool) -> CalibrationTracker {
    let mut c = confidence
    if c < 0.0 { c = 0.0 }
    if c > 1.0 { c = 1.0 }
    let sample = CalibrationSample { predicted: c, correct: correct }
    return CalibrationTracker {
        samples: push(tracker.samples, sample),
        bins: tracker.bins
    }
}
```

### ECE computation

ECE = sum over bins of: (fraction of total samples in bin) * |average confidence in bin - accuracy in bin|

```kryos
/// Compute Expected Calibration Error over all recorded samples.
/// Returns a value in [0.0, 1.0]. Lower is better. 0.0 = perfectly calibrated.
/// Returns 0.0 if no samples have been recorded.
fn ece(tracker: CalibrationTracker) -> f64 {
    let n = len(tracker.samples)
    if n == 0 {
        return 0.0
    }
    let bin_width = 1.0 / (tracker.bins as f64)
    let mut total_ece = 0.0
    let mut b = 0
    while b < tracker.bins {
        let lo = (b as f64) * bin_width
        let hi = lo + bin_width
        let mut bin_count = 0
        let mut bin_correct = 0
        let mut bin_conf_sum = 0.0
        let mut i = 0
        while i < n {
            let s = tracker.samples[i]
            // Include sample in bin if its confidence falls in [lo, hi).
            // For the last bin, also include hi == 1.0.
            let in_bin = if b == tracker.bins - 1 {
                s.predicted >= lo and s.predicted <= hi
            } else {
                s.predicted >= lo and s.predicted < hi
            }
            if in_bin {
                bin_count = bin_count + 1
                bin_conf_sum = bin_conf_sum + s.predicted
                if s.correct { bin_correct = bin_correct + 1 }
            }
            i = i + 1
        }
        if bin_count > 0 {
            let avg_conf = bin_conf_sum / (bin_count as f64)
            let acc = (bin_correct as f64) / (bin_count as f64)
            let weight = (bin_count as f64) / (n as f64)
            let diff = avg_conf - acc
            let abs_diff = if diff < 0.0 { 0.0 - diff } else { diff }
            total_ece = total_ece + weight * abs_diff
        }
        b = b + 1
    }
    return total_ece
}
```

Note on `abs`: CLAUDE.md states `abs` is a polymorphic builtin available without import. However, `abs` on a local `f64` variable may not resolve correctly in all compiler builds (it is listed as working on `i64` and `f64` builtin, but the f64 import from `std::math` shadows the builtin). Using an explicit inline `if diff < 0.0 { 0.0 - diff } else { diff }` is safer and avoids any ambiguity. The code above uses this pattern.

### Convenience: sample from a Probable value directly

```kryos
/// Helper: record a Probable<T> outcome without unwrapping confidence manually.
/// Takes the confidence from p and the caller-supplied correct flag.
fn add_probable_sample<T>(tracker: CalibrationTracker, p: Probable<T>, correct: bool) -> CalibrationTracker {
    return add_sample(tracker, p.confidence, correct)
}
```

Note: this function is generic over `T`. As of v4.47, `Probable<T>` is generic and the existing free functions in `probable.kry` are generic. `add_probable_sample<T>` follows the same pattern as `map_value<T>` in the existing file. Confirm this compiles with `kryos check` before proceeding; if the checker rejects the generic free function, the fallback is to pass `p.confidence` explicitly at the call site (trivial one-liner).

### Summary helper

```kryos
/// Human-readable calibration summary: sample count, ECE, and per-bin breakdown.
fn calibration_summary(tracker: CalibrationTracker) -> str {
    let n = len(tracker.samples)
    let error = ece(tracker)
    let mut out = "CalibrationTracker: " + to_string(n) + " samples, ECE=" + to_string(error) + "\n"
    let bin_width = 1.0 / (tracker.bins as f64)
    let mut b = 0
    while b < tracker.bins {
        let lo = (b as f64) * bin_width
        let hi = lo + bin_width
        let mut bin_count = 0
        let mut bin_correct = 0
        let mut bin_conf_sum = 0.0
        let mut i = 0
        while i < n {
            let s = tracker.samples[i]
            let in_bin = if b == tracker.bins - 1 {
                s.predicted >= lo and s.predicted <= hi
            } else {
                s.predicted >= lo and s.predicted < hi
            }
            if in_bin {
                bin_count = bin_count + 1
                bin_conf_sum = bin_conf_sum + s.predicted
                if s.correct { bin_correct = bin_correct + 1 }
            }
            i = i + 1
        }
        if bin_count > 0 {
            let avg_conf = bin_conf_sum / (bin_count as f64)
            let acc = (bin_correct as f64) / (bin_count as f64)
            out = out + "  [" + to_string(lo) + "-" + to_string(hi) + "] "
            out = out + "n=" + to_string(bin_count)
            out = out + " avg_conf=" + to_string(avg_conf)
            out = out + " acc=" + to_string(acc) + "\n"
        }
        b = b + 1
    }
    return out
}
```

Note: `calibration_summary` duplicates the binning loop from `ece()`. This is intentional: Kryos does not have closures that can capture mutable state for the inner scan, and the CLAUDE.md doctrine prefers three similar lines over a premature abstraction. The alternative is to compute ECE inside `calibration_summary` and avoid a second pass -- but that trades clarity for brevity. For ~100 samples (the target scale of MVP), two O(N*B) passes are fast.

---

## Data Model Summary

```
CalibrationSample {
    predicted: f64        // confidence value from Probable<T>.confidence
    correct: bool         // was the prediction actually right?
}

CalibrationTracker {
    samples: [CalibrationSample]   // all recorded outcomes
    bins: i64                      // number of ECE bins (default 10)
}

ece(tracker) -> f64               // Expected Calibration Error in [0.0, 1.0]
```

The total new code in `std.probable` is approximately 80 lines: two structs (~8 lines), `calibration_tracker` (~6 lines), `add_sample` (~10 lines), `ece` (~35 lines), `add_probable_sample` (~3 lines), `calibration_summary` (~30 lines). The existing `probable.kry` is 150 lines; this extends it to approximately 230 lines.

---

## MVP Scope vs Full Vision

### MVP (smallest shippable slice, today)

Modify `compiler/stdlib/probable.kry` to add:

1. `CalibrationSample` struct.
2. `CalibrationTracker` struct.
3. `calibration_tracker(bins)` constructor.
4. `add_sample(tracker, confidence, correct)` -- returns new tracker.
5. `ece(tracker)` -- returns f64.

Total: approximately 60 lines. Write a smoke-test file (`calibration_smoke.kry`) that:
- Creates a tracker with 10 bins.
- Adds 100 samples: for each, `predicted = 0.7`, `correct = (i % 10 < 7)` (70 out of 100 correct).
- Calls `ece(tracker)` and asserts the result is near 0.0 (within 0.05).

This is the stated success criterion: a perfectly calibrated predictor that says 0.7 and is correct 70% of the time should have ECE near zero.

Do NOT implement `add_probable_sample` or `calibration_summary` in the MVP. Add them after the smoke test passes.

### Full vision (post-MVP)

- `add_probable_sample<T>` convenience wrapper.
- `calibration_summary(tracker)` text report with per-bin breakdown.
- Integration test: use `std::llm::chat()` results wrapped in `Probable<str>`, feed outcomes into `CalibrationTracker`, report ECE. This connects calibration measurement to the actual LLM integration.
- Integration with kryos-bench-governed (project 07): replace the two-bucket ECE heuristic in `score_run()` with `calibration_tracker(10)` + `ece()`. This makes project 07's ECE number a real measurement rather than a simplification.
- Optional: `from_bench_results(results: [BenchResult], cases: [BenchCase]) -> CalibrationTracker` -- constructs a tracker from a benchmark run, bridging the two projects.

---

## Build Plan (ordered steps for a fresh session)

**Step 0: Verify toolchain**

```bash
kryos --version
```

Confirm >= v2.3.0. No external dependencies or API keys needed for this project.

**Step 1: Read the existing `probable.kry`**

```bash
cat compiler/stdlib/probable.kry
```

Familiarize yourself with the exact struct and function patterns used. The new code must match the existing style: free functions, no semicolons, `@copy` on scalar-only structs, `let mut` for mutation, explicit `return` on all paths.

**Step 2: Add the two structs at the top of `probable.kry`**

Insert after the existing `use std::math::{log2}` import and before the `Probable<T>` struct definition. This keeps calibration types visible early in the file.

```kryos
@copy
struct CalibrationSample {
    predicted: f64,
    correct: bool
}

struct CalibrationTracker {
    samples: [CalibrationSample],
    bins: i64
}
```

**Step 3: Add the three core functions**

Append after the existing `majority_vote` function at the bottom of the file:

- `calibration_tracker(bins: i64) -> CalibrationTracker`
- `add_sample(tracker: CalibrationTracker, confidence: f64, correct: bool) -> CalibrationTracker`
- `ece(tracker: CalibrationTracker) -> f64`

Use the implementations from the Architecture section above verbatim. Do not add `calibration_summary` yet.

**Step 4: Type-check the modified stdlib file**

```bash
kryos check compiler/stdlib/probable.kry
```

If the compiler resolves stdlib files individually, this may not work directly. Alternative:

```bash
kryos check calibration_smoke.kry
```

This will type-check `probable.kry` transitively when it resolves the `use std::probable::*` import.

Fix any `E0101` (unknown type), `E0102` (undefined variable), or `E0100` (type mismatch) errors before proceeding.

**Step 5: Write the smoke test**

Create `calibration_smoke.kry` in the project root:

```kryos
use std::probable::{calibration_tracker, add_sample, ece}

fn main() {
    // Build a perfectly calibrated predictor at confidence 0.7:
    // 100 predictions, 70 correct. ECE should be near 0.0.
    let mut tracker = calibration_tracker(10)
    let mut i = 0
    while i < 100 {
        let is_correct = (i % 10) < 7
        tracker = add_sample(tracker, 0.7, is_correct)
        i = i + 1
    }
    let error = ece(tracker)
    println("ECE (expect near 0.0): " + to_string(error))
    // The 0.7 confidence falls in the 0.6-0.7 or 0.7-0.8 bin depending on edge handling.
    // With 70/100 correct and all samples at exactly 0.7, the predicted == actual,
    // so calibration error is 0.0 (or extremely close -- floating point rounding).
    if error > 0.05 {
        throw "ECE too high: " + to_string(error) + " expected < 0.05"
    }
    println("PASS: ECE within expected range")
}
```

**Step 6: Run the smoke test**

```bash
kryos run calibration_smoke.kry
```

Expected output:
```
ECE (expect near 0.0): 0.0
PASS: ECE within expected range
```

If the ECE is slightly non-zero due to bin boundary placement (e.g. 0.7 lands in the 0.7-0.8 bin where avg_conf = 0.7 and acc = 70/100 = 0.7 exactly), the result should still be 0.0 or within float precision. The `> 0.05` guard allows for any realistic floating-point deviation.

**Step 7: Test an overconfident predictor**

Add a second test case to `calibration_smoke.kry`:

```kryos
    // Overconfident predictor: says 0.9 but is only 60% correct.
    let mut oc_tracker = calibration_tracker(10)
    let mut j = 0
    while j < 100 {
        let is_correct = (j % 10) < 6
        oc_tracker = add_sample(oc_tracker, 0.9, is_correct)
        j = j + 1
    }
    let oc_error = ece(oc_tracker)
    println("Overconfident ECE (expect ~0.30): " + to_string(oc_error))
    if oc_error < 0.20 {
        throw "Overconfident ECE too low: " + to_string(oc_error) + " expected >= 0.20"
    }
    println("PASS: overconfident ECE detected")
```

Expected: ECE near 0.30 (avg_conf = 0.9, acc = 0.6, difference = 0.3, weight = 1.0 since all samples are in the same bin).

**Step 8: Add `calibration_summary` (optional, post-smoke)**

Once the core passes, add `calibration_summary` and update `calibration_smoke.kry` to print it for the overconfident tracker. Verify the per-bin table shows the gap.

**Step 9: Run the existing `kryos test` suite**

```bash
kryos test
```

Confirm no regressions in the rest of the stdlib. The additions are append-only to `probable.kry`; they cannot break existing functions.

**Step 10: Update the `use` documentation comment at the top of `probable.kry`**

The existing comment block reads:
```
//   use std::probable::{Probable, probable, certain, or_else}
```

Update it to add the new exports:
```
//   use std::probable::{CalibrationSample, CalibrationTracker,
//                       calibration_tracker, add_sample, ece,
//                       calibration_summary}
```

---

## Success Criteria / How to Demo

### Pass criteria

- [ ] `kryos check calibration_smoke.kry` exits 0 with no errors.
- [ ] `kryos run calibration_smoke.kry` prints `PASS: ECE within expected range`.
- [ ] With 100 samples at confidence 0.7, 70 correct: `ece(tracker)` returns a value in `[0.0, 0.05]`.
- [ ] With 100 samples at confidence 0.9, 60 correct: `ece(tracker)` returns a value in `[0.20, 0.35]`.
- [ ] `kryos test` passes with no regressions.

### The demo (2 minutes)

1. Show `probable.kry` -- point to the existing `Probable<T>` struct with its `confidence: f64` field. Explain the problem: nothing enforces or measures whether that field is accurate.

2. Show `add_sample` and `ece()`. Explain: you feed in `(predicted_confidence, was_correct)` pairs. `ece()` bins them and computes the weighted absolute deviation between stated confidence and actual accuracy.

3. Run the smoke test: `kryos run calibration_smoke.kry`. Show the ECE=0.0 for the perfectly calibrated predictor, and the ECE~0.30 for the overconfident one.

4. The pitch: "Other languages have ECE in ML libraries. Kryos has it in the same file as the confidence type. If you use `Probable<T>`, you have `CalibrationTracker` right there, same import. Confidence is a measured quantity in Kryos, not a programmer assertion."

5. Optional: show `calibration_summary(oc_tracker)` output -- a per-bin table that shows exactly which confidence range is miscalibrated and by how much. This is the kind of output an auditor or regulator would want to see alongside an AI system's predictions.

---

## Risks and Honest Unknowns

**Risk 1: `@copy` on `CalibrationSample` (LOW)**

`CalibrationSample { predicted: f64, correct: bool }` contains only scalar fields. `@copy` is correct here. The bool field: CLAUDE.md lists `bool` as a primitive that is `Copy`. Confirmed safe.

**Risk 2: `push` on `[CalibrationSample]` where element is a `@copy` struct (LOW)**

`push(tracker.samples, sample)` appends a `CalibrationSample`. The untyped-array-of-aggregates issue (CLAUDE.md gotcha 22) is marked RESOLVED as of v4.47+. The type checker infers `[CalibrationSample]` from the first push. The `@copy` annotation means the element is scalar-sized, so no boxing/unboxing issues.

**Risk 3: Generic function `add_probable_sample<T>` (LOW for full vision, not in MVP)**

The existing `std.probable` already has generic free functions (`probable<T>`, `certain<T>`, `with_source<T>`, etc.), all of which work per confirmed source. `add_probable_sample<T>` follows the identical pattern. The risk is low but the function is excluded from the MVP to keep scope minimal.

**Risk 4: Bin boundary edge case for confidence == 1.0 (LOW)**

The `ece()` implementation handles this: the last bin uses `s.predicted >= lo and s.predicted <= hi` (inclusive upper bound) while all other bins use `< hi`. Confidence values of exactly 0.0 fall in the first bin (`>= 0.0 and < 0.1`), and values of exactly 1.0 fall in the last bin. The smoke test uses 0.7, which falls in either the `[0.6, 0.7)` or `[0.7, 0.8)` bin depending on float comparison. With 10 bins and width 0.1, confidence 0.7 falls in the `[0.7, 0.8)` bin (since `0.7 >= 0.7` is true and `0.7 < 0.8` is true). Avg_conf = 0.7, acc = 0.7, ECE contribution = 0.0. Correct.

**Risk 5: Duplicate binning loop in `calibration_summary` (KNOWN, accepted)**

The summary function duplicates the loop from `ece()`. This is the stated design choice: Kryos stdlib style (per CLAUDE.md doctrine) prefers repetition over a premature abstraction, especially when the closure-based alternative has edge cases (closures capturing mutable `bin_count` / `bin_correct` locals). If the duplication becomes painful after the MVP, refactor into a private `_bin_stats(tracker, b)` helper function returning a tuple `(count: i64, correct: i64, conf_sum: f64)`.

**Risk 6: Float representation in output (LOW)**

`to_string(0.0)` on Kryos prints `0` not `0.0` -- the exact format depends on the runtime's float-to-string implementation. The smoke test checks the numeric value of `error > 0.05`, not the string representation. The `println` output may show `0` instead of `0.0`; this is cosmetic and not a correctness issue.

**Risk 7: ECE is meaningful only for classification tasks (HONEST)**

ECE is defined for binary or multi-class classification where "correct" has a clear meaning. For open-ended generation tasks (summarization, code generation), defining `correct: bool` requires a judge function or a rubric. The MVP explicitly targets use cases where correctness is deterministic (classification labels, factual Q&A with known answers). Document this limitation in the module comment. Using `CalibrationTracker` on open-ended generation without a well-defined correctness criterion will produce ECE values that measure the calibration of the judge, not of the generator.

**Risk 8: Integration with kryos-bench-governed requires matching `BenchResult` types (MEDIUM, post-MVP)**

Project 07 defines `BenchResult` with `answer: Probable<str>` and an `expected: str` field on `BenchCase`. Bridging them to `CalibrationTracker` requires calling `add_sample(tracker, r.answer.confidence, infer_confidence(r.answer.value, c.expected) >= 0.85)` in a loop. This is straightforward but requires importing both modules in the same file. No language-level blocker; just implementation work scoped to the post-MVP integration step.
