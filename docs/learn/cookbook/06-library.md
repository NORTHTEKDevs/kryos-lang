# Cookbook 06 · Build a small library

Package your code as a reusable module that another Kryos project can depend on.

## What we're building

A tiny **stats** library that exports `mean`, `median`, and `stddev`. Two files:

```
mystats/
├── kryos.toml
└── src/
    └── lib.kry
```

## Step 1 · Scaffold

```bash
kryos pkg init mystats
cd mystats
```

This produces a starter `kryos.toml`:

```toml
[package]
name = "mystats"
version = "0.1.0"
description = "Simple statistics helpers"
authors = ["You <you@example.com>"]
license = "Apache-2.0"
kryos_version = ">=2.3.0"

[dependencies]
```

## Step 2 · The library

Replace `src/lib.kry`:

<!-- docs-example: skip -->
```kryos
pub fn mean(xs: [f64]) -> f64 {
    if len(xs) == 0 { return 0.0 }
    let mut sum = 0.0
    for x in xs { sum = sum + x }
    return sum / (len(xs) as f64)
}

pub fn median(xs: [f64]) -> f64 {
    if len(xs) == 0 { return 0.0 }
    let mut sorted = xs
    sort(sorted)
    let n = len(sorted)
    if n % 2 == 1 {
        return sorted[n / 2]
    } else {
        return (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}

@pure
pub fn stddev(xs: [f64]) -> f64 {
    let m = mean(xs)
    let mut acc = 0.0
    for x in xs {
        let d = x - m
        acc = acc + d * d
    }
    return sqrt(acc / (len(xs) as f64))
}
```

Things to notice:

- **`pub`** marks each function as importable.
- **`@pure`** on `stddev` tells the compiler this function has no side effects. Calling `file_read` from it would be a compile error.
- **No `main`.** Libraries don't have an entry point.

## Step 3 · Tests

Add tests at the bottom of `src/lib.kry`:

<!-- docs-example: skip -->
```kryos
@test
fn test_mean_basic() {
    assert(mean([2.0, 4.0, 6.0]) == 4.0)
}

@test
fn test_median_odd() {
    assert(median([3.0, 1.0, 2.0]) == 2.0)
}
```

Run:

```bash
kryos test
# → 2 tests passed
```

## Step 4 · Use it from another project

In a separate project, add a dependency in `kryos.toml`:

```toml
[dependencies]
mystats = { path = "../mystats" }     # local dep
# or, once published:
# mystats = "^0.1"
```

And import:

<!-- docs-example: skip -->
```kryos
use mystats::{mean, stddev}

fn main() {
    let xs = [1.0, 2.0, 3.0, 4.0, 5.0]
    println("mean   = " + to_string(mean(xs)))
    println("stddev = " + to_string(stddev(xs)))
}
```

## Step 5 · Publish

```bash
kryos pkg publish
```

This produces a tarball under `target/package/` and prints a registry-entry JSON line. To actually publish:

1. Upload the tarball to your release host (GitHub Releases, S3, etc.).
2. Open a PR against the registry index repo appending the JSON line.

See [docs/package-registry.md](../../package-registry.md) for the full publishing workflow.

## What this teaches

- **`pub`** + module path → import surface.
- **`@test`** functions are discovered automatically by `kryos test`.
- **`@pure`** is a real, compile-time-checked guarantee.
- **Local path dependencies** (`{ path = "..." }`) let you develop against unpublished libraries.

## Variations to try

- Add a `mode` function that handles ties properly.
- Make the library generic over `f32`/`f64` by adding a `T: Numeric` trait constraint.
- Write a benchmark using `@bench` to track perf across changes.

---

**You've finished the cookbook.** From here, browse [examples/](../../examples) for 74 more programs covering every corner of the language, or jump back to [Learn Kryos](../README.md) for deeper dives.
