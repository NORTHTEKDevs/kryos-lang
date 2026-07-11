# Kryos benchmark results

> **Canonical source:** [`../BENCHMARKS.md`](../BENCHMARKS.md). That file is
> generated from `benchmarks/results.json` and is never hand-edited. The table
> below is a copy of its headline numbers; if the two ever disagree,
> `../BENCHMARKS.md` wins.

Medians of 5 runs, seconds (lower is better). Last refresh: **1.0.0-rc.2,
2026-07-10**, Windows 11 x64 (rustc 1.95.0, clang/clang++ 21 -O2, go 1.25.7,
CPython 3.14.2).

| Benchmark | Kryos LLVM | Rust -O | ratio vs Rust |
| --- | --- | --- | --- |
| hashmap 1M+1M | 0.082 | 0.127 | 0.65x (beats Rust) |
| matmul 512² | 0.620 | 0.648 | 0.96x (beats Rust) |
| mandelbrot 1000²×1000 | 0.369 | 0.368 | 1.00x |
| fannkuch-redux(10) | 0.202 | 0.198 | 1.02x |
| fib(40) | 0.351 | 0.340 | 1.03x |
| nbody 2M steps | 0.141 | 0.107 | 1.31x (beats clang/clang++ -O2) |
| binary_trees d16 | 1.097 | 0.773 | 1.42x |

All 7 benchmarks land within 1.42x of Rust, and Kryos beats Rust outright on
matmul (0.96x) and hashmap (0.65x). The honest worst case is binary_trees at
1.42x; nbody (1.31x) still beats clang/clang++ -O2. These numbers were
re-measured on rc.2 after the memory-model overhaul — the correctness work
did not cost performance.

See [`../BENCHMARKS.md`](../BENCHMARKS.md) for full methodology, per-benchmark
analysis, spreads, the Cranelift column, and startup floors.
