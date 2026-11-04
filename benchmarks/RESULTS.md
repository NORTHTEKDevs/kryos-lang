# Kryos benchmark results

> **Canonical source:** [`../BENCHMARKS.md`](../BENCHMARKS.md), which records
> the commit, machine and toolchains of the run, and whose tables come from
> `benchmarks/results.json`. The table below is a copy of its numbers; if the
> two ever disagree, `../BENCHMARKS.md` wins.

Medians of 5 runs, seconds (lower is better). Last refresh: **1.3.0,
2026-11-03**, at commit `cbd88996`, Windows 11 x64 (rustc 1.95.0, clang/clang++
21.1.8 -O2, go 1.25.7, CPython 3.14.2).

| Benchmark | Kryos LLVM | Rust -O | ratio vs Rust |
| --- | --- | --- | --- |
| hashmap 1M+1M | 0.086 | 0.147 | 0.59x (beats Rust) |
| matmul 512^2 | 0.620 | 0.645 | 0.96x |
| mandelbrot 1000^2 x1000 | 0.383 | 0.381 | 1.00x |
| fib(40) | 0.362 | 0.358 | 1.01x |
| fannkuch-redux(10) | 0.204 | 0.196 | 1.04x |
| nbody 2M steps | 0.150 | 0.112 | 1.34x |
| binary_trees d16 | 1.455 | 0.772 | 1.88x |

Kryos is level with Rust (within 4%) on four of the 7 benchmarks and beats it
outright on hashmap. It trails on nbody (1.34x) and binary_trees (1.88x).
binary_trees was 1.42x at 1.0.0-rc.2: most of the slowdown is a memory leak,
because its `shared` nodes are never freed (LEDGER item 137).

See [`../BENCHMARKS.md`](../BENCHMARKS.md) for full methodology, per-benchmark
analysis, spreads, the Cranelift column, and startup floors.
