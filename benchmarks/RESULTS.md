# Kryos benchmark results

> **Canonical source:** [`../BENCHMARKS.md`](../BENCHMARKS.md), which records
> the commit, machine and toolchains of the run, and whose tables come from
> `benchmarks/results.json`. The table below is a copy of its numbers; if the
> two ever disagree, `../BENCHMARKS.md` wins.

Medians of 5 runs, seconds (lower is better). Last refresh: **1.3.0,
2026-11-24**, at commit `797463af`, Windows 11 x64 (rustc 1.95.0, clang/clang++
21.1.8 -O2, go 1.25.7, CPython 3.14.2).

| Benchmark | Kryos LLVM | Rust -O | ratio vs Rust |
| --- | --- | --- | --- |
| hashmap 1M+1M | 0.081 | 0.126 | 0.64x (beats Rust) |
| matmul 512^2 | 0.626 | 0.664 | 0.94x |
| mandelbrot 1000^2 x1000 | 0.374 | 0.374 | 1.00x |
| fib(40) | 0.357 | 0.356 | 1.00x |
| fannkuch-redux(10) | 0.203 | 0.197 | 1.03x |
| nbody 2M steps | 0.147 | 0.109 | 1.34x |
| binary_trees d16 | 1.406 | 0.802 | 1.75x |

Kryos is level with Rust (within 6%) on four of the 7 benchmarks and beats it
outright on hashmap. It trails on nbody (1.34x) and binary_trees (1.75x).
binary_trees was 1.42x at 1.0.0-rc.2: most of the slowdown is a memory leak,
because its `shared` nodes are never freed (LEDGER item 137).

See [`../BENCHMARKS.md`](../BENCHMARKS.md) for full methodology, per-benchmark
analysis, spreads, the Cranelift column, and startup floors.
