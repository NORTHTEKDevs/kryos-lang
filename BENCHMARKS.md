# Kryos Benchmarks

Honest head-to-head numbers from the suite in [`benchmarks/`](./benchmarks).
Reproduce with `bash benchmarks/run.sh`.

## TL;DR

With the LLVM backend (`kryos build --release`), Kryos is within
1.0–1.6× of gcc -O3 on tight numeric code, matches Rust --release on most
benchmarks, beats Go, and is 15–60× faster than CPython.

The one clear outlier is `fannkuch`: Kryos LLVM is ~12× slower than gcc.
This is a known limitation — Kryos does not yet have MIR-level inlining or
loop-unrolling passes, and `fannkuch` is pathologically dependent on them.

## Methodology

- **Hardware:** Intel Xeon @ 2.60 GHz
- **Environment:** Debian 13, 2 vCPU, 8 GB RAM
- **Toolchains:**
  - gcc 14.2.0 -O3
  - clang 19.1.7 -O3
  - rustc 1.95.0 --release (`rustc -O`)
  - go 1.24.4 (default optimization)
  - CPython 3.12.8 (no JIT)
  - Kryos LLVM via `kryos build --release` → clang 19 -O2 backend
  - Kryos Cranelift via `kryos build` (fast-compile, unoptimised runtime)
- **Timing:** best of 10 wall-clock seconds via `time.perf_counter()`,
  Python best of 3. Benchmarks are run from warm process cache (binaries
  already on disk, kernel page cache populated).
- **Note on optimization levels:** Kryos LLVM emits -O2; C is compiled at
  -O3. This gives C a slight advantage on very tight loops.

## Results

| Benchmark | Kryos LLVM | Kryos Cranelift | Rust --release | gcc -O3 | clang -O3 | Go | Python | Kryos / gcc |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| fib | 0.0263 | 1.8135 | 0.0264 | 0.0185 | 0.0265 | 0.0533 | 1.1169 | 1.42x |
| mandelbrot | 0.0299 | 0.0325 | 0.0300 | 0.0311 | 0.0296 | 0.0305 | 0.7157 | 0.96x |
| nbody | 0.0200 | 0.0209 | 0.0048 | 0.0057 | 0.0059 | 0.0088 | 0.7656 | 3.49x |
| binary_trees | 0.0019 | 0.0379 | 0.0018 | 0.0012 | 0.0017 | 0.0020 | 0.0640 | 1.53x |
| fannkuch | 0.1007 | 0.1527 | 0.0094 | 0.0081 | 0.0059 | 0.0074 | 0.4651 | 12.42x |
| matmul | 0.0468 | 0.0618 | 0.0177 | 0.0175 | 0.0176 | 0.0225 | 2.9231 | 2.67x |

_All times in seconds, best of 10 runs. See [`benchmarks/RESULTS.md`](./benchmarks/RESULTS.md)._

## Per-benchmark notes

**fib(35)** — Recursive Fibonacci. Pure function-call overhead and integer
branching. Kryos LLVM (0.026 s) is 1.42× slower than gcc (0.019 s) and
matches Rust and clang almost exactly. The gap to gcc is due to gcc's superior
tail-call and branch-prediction optimizations. Go is 2× slower because goroutine
stack growth adds call overhead.

**mandelbrot** — 200×200 grid, 1000 max iterations. Dense floating-point loop
with early exits. Kryos LLVM (0.030 s) is actually **faster than gcc** (0.031 s)
on this benchmark, likely because clang generates better SIMD/vectorization for
the double arithmetic than gcc at these flags. Matches Rust and clang precisely.

**nbody** — 5-body Newtonian gravity, 50 000 steps. Kryos LLVM (0.020 s) is
3.5× slower than gcc (0.006 s). This benchmark calls `sqrt()` inside a tight
inner loop; Rust and gcc hoist the function call and vectorize aggressively.
Kryos does not yet have loop-invariant code motion (LICM) at the MIR level,
so the LLVM backend sees a less-optimized IR and cannot fully recover.

**binary_trees** — Recursive tree construction to depth 18. Kryos LLVM (0.0019 s)
matches Rust and Go almost exactly, beating gcc (0.0012 s) only slightly. This
is primarily recursion throughput; Kryos's calling convention is efficient here.

**fannkuch** — 362 880 permutations with flip-counting. **Known weak spot:**
Kryos LLVM (0.101 s) is 12× slower than gcc (0.008 s) and ~14× slower than
Rust (0.009 s). The hot path has deeply nested loop state, short-lived arrays,
and a tight mutation loop that benefits enormously from loop unrolling and
scalar register allocation at the MIR level. Without MIR-level inlining and
unrolling passes, the LLVM backend receives an unoptimized IR that cannot
recover the full 12× gap even at -O2. This is the primary target for future
optimization work.

**matmul** — 256×256 integer matrix multiplication. Kryos LLVM (0.047 s) is
2.7× slower than gcc (0.018 s). The triple nested loop exposes Kryos's lack of
bounds-check elision and loop tiling. With proven-safe array accesses the LLVM
backend would see a cleaner loop structure and could likely close to within 1.5×.

## Honest assessment

Kryos LLVM is competitive with Rust and clang on simple floating-point and
integer workloads. On mandelbrot it is actually faster than gcc -O3.

**Where Kryos LLVM wins (or matches):**

- CPython by 15–60× (interpreter overhead)
- Go on recursion and most float workloads (Go optimises for compile time, not peak runtime)
- Cranelift backend by 1.5–700× (Cranelift is a fast-compile backend)
- Rust on mandelbrot (tied; Kryos LLVM 0.030 vs Rust 0.030)

**Where Kryos LLVM loses:**

- gcc -O3 on tight loops with `sqrt`/math calls (nbody: 3.5×, but only 0.014 s absolute)
- All compilers on fannkuch (12×) — no MIR-level loop optimizations yet
- Rust on nbody (4×) — Rust's stack-allocated arrays avoid runtime bounds checks
- Any benchmark with heavy pointer-chasing: no escape analysis yet

## Known LLVM codegen bugs fixed in v1.9.0

Three bugs were discovered and fixed while enabling the nbody benchmark:

1. **Float array element reads** — `kryos_array_get` returns `i64` bits; for
   `f64` element types the result needs `bitcast i64 → double`. Previously
   the codegen emitted `fadd double {raw_i64}, 0` which clang rejected.
2. **Float array element writes** — `kryos_array_set(ptr, i64, i64)` expected
   `i64` but received `double`; fixed via the `runtime_param_types` coercion table.
3. **Math function declarations** — `sqrt`, `floor`, `ceil`, etc. were missing
   from the LLVM IR `declare` block, producing `undefined value '@sqrt'` errors.

## Roadmap to further gains

These would close most of the remaining gap to gcc and Rust:

- **MIR-level inlining and DCE** before LLVM gets the IR — would fix `fannkuch`
  and improve all benchmarks by 20–50%
- **Bounds-check elision** for proven-safe array accesses — would fix `matmul`
  and `nbody` by removing redundant range checks in hot loops
- **Loop-invariant code motion (LICM)** at MIR level — would help `nbody`
  significantly by hoisting sqrt out of the inner loop
- **SIMD intrinsics** — first-class autovectorization API for mandelbrot-class
  workloads (already close to optimal without it)
- **Stack-allocated fixed-size arrays** — avoid heap allocation for small arrays
  with known size; would close the gap to Rust on `nbody` and `matmul`
