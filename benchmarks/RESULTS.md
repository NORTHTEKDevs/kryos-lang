# Kryos benchmark results

> **Canonical source:** [`../BENCHMARKS.md`](../BENCHMARKS.md), which records
> the commit, machine and toolchains of the run, and whose tables come from
> `benchmarks/results.json`. The table below is a copy of its numbers; if the
> two ever disagree, `../BENCHMARKS.md` wins.

Medians of 5 runs, seconds (lower is better). Last refresh: **1.3.0,
2026-11-22**, at commit `60071762`, Windows 11 x64 (rustc 1.95.0, clang/clang++
21.1.8 -O2, go 1.25.7, CPython 3.14.2).

| Benchmark | Kryos LLVM | Rust -O | ratio vs Rust |
| --- | --- | --- | --- |
| hashmap 1M+1M | 0.084 | 0.144 | 0.58x (beats Rust) |
| matmul 512^2 | 0.622 | 0.697 | 0.89x |
| mandelbrot 1000^2 x1000 | 0.385 | 0.373 | 1.03x |
| fib(40) | 0.357 | 0.348 | 1.02x |
| fannkuch-redux(10) | 0.194 | 0.200 | 0.97x |
| nbody 2M steps | 0.152 | 0.109 | 1.39x |
| binary_trees d16 | 1.463 | 0.795 | 1.84x |

Kryos is level with Rust (0.89x to 1.03x, inside the spreads of this noisy run)
on four of the 7 benchmarks and beats it outright on hashmap. It trails on
nbody (1.39x) and binary_trees (1.84x).
binary_trees was 1.42x at 1.0.0-rc.2: most of the slowdown is a memory leak,
because its `shared` nodes are never freed (LEDGER item 137).

See [`../BENCHMARKS.md`](../BENCHMARKS.md) for full methodology, per-benchmark
analysis, spreads, the Cranelift column, and startup floors.
