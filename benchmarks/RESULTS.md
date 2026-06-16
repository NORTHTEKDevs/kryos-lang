# Kryos benchmark results

> **Canonical source:** [`../BENCHMARKS.md`](../BENCHMARKS.md). That file is
> generated from `benchmarks/results.json` and is never hand-edited. The table
> below is a copy of its headline numbers; if the two ever disagree,
> `../BENCHMARKS.md` wins.

Medians of 5 runs, seconds (lower is better). Last refresh: **1.0.0-beta.1,
2026-06-14**, Windows 11 x64 (rustc 1.95, clang/clang++ 21 -O2, go 1.x,
CPython 3.14).

| Benchmark | Kryos LLVM | Rust -O | ratio vs Rust |
| --- | --- | --- | --- |
| hashmap 1M+1M | 0.080 | 0.118 | 0.68x (beats Rust) |
| matmul 512² | 0.618 | 0.653 | 0.95x (beats Rust) |
| mandelbrot 1000²×1000 | 0.368 | 0.368 | 1.00x |
| fib(40) | 0.349 | 0.347 | 1.01x |
| fannkuch-redux(10) | 0.197 | 0.195 | 1.01x |
| nbody 2M steps | 0.141 | 0.105 | 1.34x (beats clang/clang++ -O2) |
| binary_trees d16 | 1.098 | 0.759 | 1.45x |

All 7 benchmarks land within 1.45x of Rust, and Kryos beats Rust outright on
matmul (0.95x) and hashmap (0.68x). The honest worst case is binary_trees at
1.45x; nbody (1.34x) still beats clang/clang++ -O2.

See [`../BENCHMARKS.md`](../BENCHMARKS.md) for full methodology, per-benchmark
analysis, spreads, the Cranelift column, and startup floors.
