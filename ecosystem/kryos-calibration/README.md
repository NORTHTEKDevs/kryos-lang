# kryos-calibration

Probability **calibration** for the `Probable<T>` confidence axis.

A confidence score is only useful if it *means* something. If a model says it is
`0.9` confident and is right 90% of the time, it is well-calibrated; if it says
`0.9` and is right half the time, it is **overconfident** and every downstream
"trust this if confidence >= 0.8" gate is wrong. This library measures that gap
(ECE, MCE, Brier, reliability curve) and corrects it (histogram-binning
recalibration), then maps the corrected confidence back onto a `Probable<T>`.

## Why Kryos

The whole metric and recalibration engine (`src/calibration.kry`) is annotated
`@capabilities(compute)` and nothing else. That is not a comment, it is a
**compile-time proof**:

```
kryos check --strict-capabilities .        # passes: engine touches no I/O
```

A calibration report is, provably, a pure function of recorded outcomes -- it
cannot read a file, the environment, or the network to "phone home" with your
prediction data. The only capability anywhere in the package is `process`, used
solely by the test driver's `exit(1)`-on-failure (see `kryos.toml`).

It also reuses the confidence axis directly: samples are the same
`std::probable::CalibrationSample` that `std::probable.add_sample` records, and
`recalibrate_probable` returns a real `Probable<T>` -- so calibration plugs into
existing Probable-based code with no adapter layer.

## Install

```toml
# kryos.toml
[dependencies]
kryos_calibration = { path = "../kryos-calibration" }

[capabilities]
allowed = ["compute"]   # the engine needs nothing more
```

## Usage

```kryos
use kryos_calibration::calibration::{
    new_sampleset, add, ece, mce, brier, reliability_curve,
    fit_histogram, recalibrate_probable, metrics_report,
}
use std::probable::{Probable}

// 1. Collect (predicted_confidence, was_correct) outcomes.
let mut s = new_sampleset()
s = add(s, 0.9, true)
s = add(s, 0.9, false)   // stated 0.9, was wrong
// ... more outcomes ...

// 2. Measure. (10 = number of confidence bins / deciles.)
let e   = ece(s.samples, 10)     // expected calibration error,  0 = perfect
let m   = mce(s.samples, 10)     // worst-case bin error
let b   = brier(s.samples)       // mean squared error (calibration + sharpness)
let cur = reliability_curve(s.samples, 10)   // per-bin mean_conf / accuracy / count
print(metrics_report(s.samples, 10))         // all of the above, formatted

// 3. Correct: fit a histogram map, then recalibrate live predictions.
let rc = fit_histogram(s.samples, 10)
let raw: Probable<str> = Probable { value: "ship it", confidence: 0.9, source: "model" }
let cal = recalibrate_probable(rc, raw)      // cal.confidence is now the empirical accuracy
```

### What the metrics mean

| Function | Returns | Reading |
| --- | --- | --- |
| `ece(samples, bins)` | f64 in [0,1] | sample-weighted mean of \|confidence - accuracy\| per bin; **0 = perfectly calibrated** |
| `mce(samples, bins)` | f64 in [0,1] | the single worst bin's \|confidence - accuracy\| |
| `brier(samples)` | f64 in [0,1] | mean (confidence - outcome)^2; lower is better; penalizes hedging too |
| `reliability_curve(samples, bins)` | `[ReliabilityBin]` | one row per bin: `lo, hi, mean_conf, accuracy, count` (counts sum to N) |
| `fit_histogram(samples, bins)` | `Recalibrator` | per-bin map: raw confidence -> that bin's empirical accuracy |
| `recalibrate(rc, raw)` | f64 | corrected confidence for one raw value |
| `recalibrate_probable(rc, p)` | `Probable<T>` | `p` with its confidence corrected (value/source kept) |

## Run it

```bash
# Verification driver (asserts every metric on two fixtures; exit 1 on failure):
kryos run ecosystem/kryos-calibration/tests/run_calibration.kry

# 30-line demo of the overconfident-model -> recalibrated-Probable flow:
kryos run ecosystem/kryos-calibration/demo.kry

# Prove the engine is side-effect-free:
kryos check --strict-capabilities ecosystem/kryos-calibration
```

The driver checks, on a **perfectly-calibrated** fixture (ECE/MCE ~ 0) and an
**overconfident** fixture (ECE = 0.35, MCE = 0.40, Brier = 0.37), that the
metrics are correct, that reliability buckets partition the samples, and that
histogram recalibration collapses the overconfident set's ECE from 0.35 to ~0.

## Honest limitations

- **Sample type is reused, not redefined.** Samples are
  `std::probable::CalibrationSample`, whose fields are `predicted` and `correct`.
  The collector API keeps the spec's `predicted_confidence` / `was_correct`
  *parameter* names, but a second struct literally named `CalibrationSample`
  cannot exist in the same build -- struct names share one flat namespace once a
  module is loaded. Reuse is the better outcome anyway: the same samples
  `std::probable` records flow straight in.
- **Histogram binning is non-parametric.** It maps each confidence to the
  empirical accuracy of its bin and nothing more. Bins with **no** training
  samples pass raw confidence through unchanged. Sparse bins give noisy
  estimates -- you need enough samples per bin (rule of thumb: tens). It does
  **not** enforce monotonicity and is not isotonic regression or Platt scaling;
  those would be follow-on recalibrators.
- **`add` is O(n^2) to build n samples.** The collector is functional/immutable
  (each `add` returns a fresh set), matching `std::probable`'s tracker and
  avoiding struct-aliasing pitfalls. Fine for the hundreds-to-thousands of
  samples a calibration set holds; batch differently for very large n.
- **ECE assumes classification with a known label.** "Correct" must be
  well-defined ground truth. On open-ended generation, ECE measures the *judge*
  that produced the correct/incorrect flag, not the generator (same caveat as
  `std::probable`).
- **Verified via a `kryos run` driver, not `kryos test`.** Every metric returns
  an f64 and the driver prints them; the `kryos test` JIT cannot compile
  `to_string(f64)`. The driver asserts with tolerance and `exit(1)`s on the
  first failure, which is the equivalent RED signal.

## License

Apache-2.0. See [LICENSE](./LICENSE).
