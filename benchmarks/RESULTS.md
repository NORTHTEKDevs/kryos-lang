# Kryos benchmark results

> **Canonical source:** [`../BENCHMARKS.md`](../BENCHMARKS.md), which records
> the commit, machine and toolchains of the run, and whose tables come from
> `benchmarks/results.json`. The table below is a copy of its numbers; if the
> two ever disagree, `../BENCHMARKS.md` wins.

Medians of 5 runs, seconds (lower is better). Last refresh: **1.3.0 plus the 1.3.1
memory wave, 2026-11-24**, at commit `4d5c9301`, Windows 11 x64 (rustc 1.95.0, clang/clang++
21.1.8 -O2, go 1.25.7, CPython 3.14.2).

| Benchmark | Kryos LLVM | Rust -O | ratio vs Rust |
| --- | --- | --- | --- |
| hashmap 1M+1M | 0.080 | 0.128 | 0.63x (beats Rust) |
| matmul 512^2 | 0.621 | 0.653 | 0.95x |
| mandelbrot 1000^2 x1000 | 0.376 | 0.377 | 1.00x |
| fib(40) | 0.359 | 0.355 | 1.01x |
| fannkuch-redux(10) | 0.202 | 0.200 | 1.01x |
| nbody 2M steps | 0.145 | 0.111 | 1.31x |
| binary_trees d16 | 1.513 | 0.780 | 1.94x |

Kryos is level with Rust (within 5%) on four of the 7 benchmarks and beats it
outright on hashmap. It trails on nbody (1.31x) and binary_trees (1.94x).
binary_trees was 1.42x at 1.0.0-rc.2. Its `shared` nodes are freed again
(LEDGER item 137: peak 24 MB instead of 1.1 GB), but that did not make it
faster (4% to 8% slower than the build that never freed); the allocator and
the reference counting are what is left.

See [`../BENCHMARKS.md`](../BENCHMARKS.md) for full methodology, per-benchmark
analysis, spreads, the Cranelift column, and startup floors.
