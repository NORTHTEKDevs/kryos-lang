# Kryos benchmark suite

Microbenchmarks comparing Kryos against equivalent Rust and C
implementations of six standard programs.

## What's here

```
benchmarks/
  kryos/          Kryos sources (.kry)
  rust/           Rust sources (.rs)
  c/              C sources (.c)
  run.sh          Build + time everything; writes RESULTS.md
  RESULTS.md      Latest committed numbers
  bin/            Build artifacts (gitignored)
```

Six programs:

| Program        | What it tests                                  |
| -------------- | ---------------------------------------------- |
| `fib`          | recursive function-call overhead               |
| `mandelbrot`   | tight floating-point loop                      |
| `nbody`        | floating-point + indexed array mutation        |
| `binary_trees` | deep recursion                                 |
| `fannkuch`     | array mutation + nested loops                  |
| `matmul`       | nested-loop integer arithmetic                 |

## Running

```bash
cd benchmarks
./run.sh
```

Requires: a `kryos` binary (the script looks at
`../compiler/target/release/kryos`), `rustc`, and `cc`. Times are best of
three runs, measured with Python's `time.monotonic()` for cross-platform
millisecond resolution. Results are written to `RESULTS.md`.

## How to read the numbers

The default Kryos backend is **Cranelift** (fast compile, decent perf).
The release backend is **LLVM** (slow compile, peer-to-rustc perf),
gated by having `clang` on `PATH`. The numbers in `RESULTS.md` are
Cranelift unless noted.

For each benchmark we report:

- **Kryos** wall-clock (best of 3).
- **Rust** built with `rustc -O` (i.e. `--opt-level=3`).
- **C** built with `cc -O2`.
- **Kryos / C** ratio: how much slower Kryos is than C.

A ratio of ~1.0 means Kryos is hitting peer-language perf on that
workload. Ratios above ~10 usually indicate a missing optimization
(inlining, register allocation, memcpy intrinsic, etc.) that we haven't
implemented in Cranelift codegen yet.

## Known gaps (May 2026)

- **`fib(35)` is slow** (~100x). Cranelift does not currently inline
  small recursive functions; this is the worst-case scenario.
  `mandelbrot` and `nbody` show Cranelift can hit parity when the hot
  path is a long loop instead of a deep call chain.
- **`binary_trees`** suffers the same recursion penalty.
- **`fannkuch`** spends time in array bounds checks and length reads;
  loop-invariant code motion could remove much of this.

These gaps are tracked separately. The benchmark suite's job is to make
them visible and to catch regressions, not to mask them.

## Adding a benchmark

1.  Write `kryos/foo.kry`, `rust/foo.rs`, `c/foo.c` — all three should
    compute the **same checksum or output** so we can verify correctness.
2.  Add `foo` to the `BENCHES=(...)` array in `run.sh`.
3.  Add an entry to the README and the `## Notes` block at the bottom of
    `run.sh`.
4.  Re-run `./run.sh` and commit the updated `RESULTS.md`.
