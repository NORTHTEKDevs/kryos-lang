# Kryos benchmark suite

Seven programs, each written in Kryos and ported to the languages it is
compared against. The published numbers, with the commit and machine they were
measured on, are in [`../BENCHMARKS.md`](../BENCHMARKS.md).

## What's here

```
benchmarks/
  kryos/             Kryos sources (.kry)
  rust/ c/ cpp/      Rust, C and C++ ports (hashmap has no C port)
  go/ python/        Go and Python ports
  mojo/              Mojo ports of fib, mandelbrot and matmul (timed only when
                     a `mojo` binary is on PATH)
  measure.py         Builds and times every port; writes results.json and
                     results_table.md
  check_outputs.py   Runs every measured port once; fails unless all ports of
                     a benchmark print the same output
  results.json       Medians and min..max spreads from the last measure.py run
  results_table.md   The table measure.py generates from results.json
  RESULTS.md         Short summary of ../BENCHMARKS.md
  run.sh             Deprecated runner, kept for reference
  bin/               Build artifacts (gitignored)
```

| Program        | What it tests                                                  |
| -------------- | -------------------------------------------------------------- |
| `fib`          | recursive function calls: fib(40)                              |
| `mandelbrot`   | a tight floating-point loop: 1000x1000 points, 1000 iterations |
| `nbody`        | floating-point math on arrays: 5 bodies, 2,000,000 steps       |
| `binary_trees` | allocation and recursion: trees of depth 16                    |
| `fannkuch`     | array permutation in nested loops: n = 10                      |
| `matmul`       | nested-loop integer arithmetic: 512x512 matrices               |
| `hashmap`      | map throughput: 1,000,000 inserts, then 1,000,000 lookups      |

The `strings` sources are not measured: the Kryos port builds its string with
`s = s + token`, which is O(n^2), so it would not be a like-for-like comparison
(see the comment at `BENCHES` in `measure.py`).

## Running

```bash
python benchmarks/measure.py         # warmup + 5 timed runs per port; reports the median
python benchmarks/check_outputs.py   # then check that every port printed the same output
```

`measure.py` uses the release build of the compiler at
`compiler/target/release/kryos` (`cargo build --release` in `compiler/`), the
Python interpreter that runs it, and whichever of `rustc`, `clang`, `clang++`,
`go` and `mojo` it finds. A toolchain that is not found is reported as `n/a`,
never estimated. `kryos-llvm` is `kryos build --release --backend llvm`;
`kryos-cranelift` is `kryos build` (the dev backend).

## Adding a benchmark

1. Write `kryos/foo.kry` and its ports (`rust/foo.rs`, `c/foo.c`,
   `cpp/foo.cpp`, `go/foo.go`, `python/foo.py`). Every port must print the
   same output.
2. Add `foo` to `BENCHES` in `measure.py`.
3. Run `measure.py`, then `check_outputs.py`, and update `../BENCHMARKS.md`
   with the new table and the commit you measured.
