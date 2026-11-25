# Kryos benchmark results

> **Canonical source:** [`../BENCHMARKS.md`](../BENCHMARKS.md), which records
> the commit, machine and toolchains of the run, and whose tables come from
> `benchmarks/results.json`. The table below is a copy of its numbers; if the
> two ever disagree, `../BENCHMARKS.md` wins.

Medians of 5 runs, seconds (lower is better). Last refresh: **1.3.0 plus the 1.3.1
memory wave, 2026-11-24**, at commit `9a0dc8cc`, Windows 11 x64 (rustc 1.95.0, clang/clang++
21.1.8 -O2, go 1.25.7, CPython 3.14.2).

| Benchmark | Kryos LLVM | Rust -O | ratio vs Rust |
| --- | --- | --- | --- |
| hashmap 1M+1M | 0.086 | 0.124 | 0.70x (beats Rust) |
| matmul 512^2 | 0.626 | 0.658 | 0.95x |
| mandelbrot 1000^2 x1000 | 0.375 | 0.373 | 1.00x |
| fib(40) | 0.358 | 0.349 | 1.03x |
| fannkuch-redux(10) | 0.203 | 0.197 | 1.03x |
| nbody 2M steps | 0.146 | 0.110 | 1.32x |
| binary_trees d16 | 1.597 | 0.799 | 2.00x |

Kryos is level with Rust (within 5%) on four of the 7 benchmarks and beats it
outright on hashmap. It trails on nbody (1.32x) and binary_trees (2.00x).
binary_trees was 1.42x at 1.0.0-rc.2. Its `shared` nodes are freed again
(LEDGER item 137: peak 24 MB instead of 1.1 GB), but that did not change the
time; the allocator and the reference counting are what is left.

See [`../BENCHMARKS.md`](../BENCHMARKS.md) for full methodology, per-benchmark
analysis, spreads, the Cranelift column, and startup floors.
