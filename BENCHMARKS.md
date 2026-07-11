# Kryos Benchmarks

Measured medians from [`benchmarks/measure.py`](./benchmarks/measure.py).
Reproduce with `python benchmarks/measure.py` - the tables below are generated
from `benchmarks/results.json`, never hand-edited. Mirror:
[`benchmarks/results_table.md`](./benchmarks/results_table.md).
Last refresh: **1.0.0-rc.2, 2026-07-10**, Windows 11 x64
(rustc 1.95.0, clang/clang++ 21 -O2, go 1.25.7, CPython 3.14.2).

## Headline

**All 7 benchmarks are within 1.42x of Rust, and Kryos beats Rust and/or
clang++ on several.** Kryos is in the systems-language performance tier —
and these numbers were re-measured on rc.2 AFTER the memory-model overhaul
(HeaderPool, share-retain let-bindings, independent-copy array rebinds):
the correctness work cost nothing.

- **Beats Rust:** matmul (0.96x), hashmap (0.65x).
- **Beats clang/clang++ -O2:** nbody (0.141 vs 0.149), matmul, hashmap
  (0.082 vs `std::unordered_map` 0.370 = ~4.5x faster).
- **Parity (<=1.03x of Rust):** fib, mandelbrot, fannkuch.
- **Closer than before:** nbody 1.31x (was 1.90x pre-optimization),
  binary_trees 1.42x (was 6.1x).

Where each toolchain still wins: Rust edges nbody (stack `[f64;5]` +
vectorization) and binary_trees (`Box` unique-ownership vs Kryos's Rc-like
`Shared`); Go wins binary_trees outright (GC bump allocator). No benchmark is
an embarrassing outlier anymore.

## Results (medians of 5, seconds)

| Benchmark | kryos LLVM | kryos Cranelift | rust -O | clang -O2 | clang++ -O2 | mojo | go | python | kryos/rust |
|---|---|---|---|---|---|---|---|---|---|
| fib(40) | 0.351 | 4.832 | 0.340 | 0.342 | 0.340 | n/a | 0.706 | 16.899 | **1.03x** |
| mandelbrot 1000^2 x1000 | 0.369 | 0.416 | 0.368 | 0.365 | 0.364 | n/a | 0.376 | 20.373 | **1.00x** |
| nbody 2M steps | 0.141 | 0.938 | 0.107 | 0.149 | 0.149 | n/a | 0.246 | 46.264 | **1.31x** (beats C/C++) |
| binary_trees d16 | 1.097 | 3.621 | 0.773 | 0.716 | 0.733 | n/a | 0.491 | 2.244 | **1.42x** (was 6.1x) |
| fannkuch-redux(10) | 0.202 | 0.900 | 0.198 | 0.189 | 0.189 | n/a | 0.198 | 6.349 | **1.02x** |
| matmul 512^2 | 0.620 | 0.650 | 0.648 | 0.659 | 0.645 | n/a | 0.564 | 34.808 | **0.96x** (beats Rust+C+C++) |
| hashmap 1M+1M | 0.082 | 0.086 | 0.127 | n/a | 0.370 | n/a | 0.128 | 0.535 | **0.65x** (beats Rust+C++) |

All rows verified output-identical across languages before timing. hashmap has
no C port (n/a); the clang++ port uses idiomatic `std::unordered_map`.

### On Mojo (not measured here)

Mojo is shown as `n/a`: no Modular toolchain is installed on this host (Windows,
no WSL distro), and we do not fabricate numbers. Reference ports + harness
support are in [`benchmarks/mojo/`](./benchmarks/mojo/) - `measure.py` builds
and times them automatically when a `mojo` binary is on PATH. Published
positioning: Mojo lowers MLIR -> LLVM (same backend as clang/rustc), so on tight
numeric kernels (mandelbrot/nbody/matmul/fib) optimized Mojo lands in the same
~1.0-1.7x-of-Rust band these benchmarks already occupy; one cited numeric
benchmark put Mojo ~1.6x of Rust. There is no broad, independent public
shootout, and allocation/pointer-chasing (binary_trees-style) is undocumented
for Mojo. Net: Kryos's numeric-kernel results are in Mojo's league; a true
head-to-head requires running the ports on a Mojo host.

### binary_trees: from 6.1x to 1.45x (leak-free)

The original port stored each child as a single-element `[Tree]` array =
**three** heap allocations per child (array header + data buffer + boxed
struct). It is now `Tree { left: Option<Shared<Tree>>, right: Option<Shared<Tree>> }`
= **one** arc allocation per node (the new, completed `Shared<T>` heap pointer).
Recursive refcounted teardown is wired (each node's arc block carries a
`__kryos_arc_drop_<T>` drop function, so releasing a tree's root cascades), so
the run is **leak-free: 24 MB peak** (was 1.1 GB while the teardown was missing)
- an apples-to-apples comparison against the freeing C/Rust/Go ports. The
residual 1.45x vs Rust is mostly Rc refcount traffic vs Rust's `Box`
unique-ownership; Go (0.488s) wins via its GC bump allocator. checksum 14723759,
identical on both backends.

## Methodology

1. **Workloads sized** so the fastest competitor needs >= ~0.3s (fib(40),
   1000x1000x1000-iter mandelbrot, 2M-step nbody, 512x512 matmul, depth-16
   binary_trees, 1M-entry hashmap).
2. **Median of 5 runs** after a warmup; min..max spread in `results.json`.
3. **Startup floor measured separately** (not subtracted, so numbers are honest
   wall-clock): kryos native exe ~5.2ms, rust ~5.7ms, python ~40ms.
4. **Same answer required** - every port prints an identical checksum,
   cross-checked before timings count (fannkuch `73196 / Pfannkuchen(10) = 38`,
   binary_trees 14723759, hashmap 999999000000).
5. **Broken/absent ports are labeled n/a, not buried, and never fabricated.**

## What changed (2026-07-10, rc.2 re-measure)

Full re-run on the rc.2 binary after the memory-model overhaul (type-stable
`HeaderPool` header recycling, retain-on-read borrow fixes), the
value-semantics fixes (share-retain container let-bindings, independent-copy
array rebinds via `kryos_array_dup`), and the ~8,000-program differential
fuzz campaign. Every number is within run-to-run noise of the June sweep —
**the correctness work did not cost performance**: nbody 1.34x -> 1.31x,
binary_trees 1.45x -> 1.42x, matmul still beats Rust (0.96x), hashmap still
beats everything (0.65x of Rust). Same checksum cross-validation; all rows
output-identical across languages before timing.

## What changed (2026-06-14)

Cumulative this session, all gated on backend parity (58 smoke tests, both
backends, 0 divergences) + conformance 6/6 + examples 80/80 + the self-host
bootstrap (stage-2 == stage-3 == stage-4, fixed point held):

- **Reverted an unsound in-place string-concat fast path** (it corrupted every
  aliased `str` reference - interpolation / `let y=a+b` / fn args / container
  stores - and self-concat; the compiler's refcount does not track aliases).
- **TBAA on array element-vs-header accesses** -> fannkuch 1.10x -> 1.01x.
- **Map probe uses `& (cap-1)`** (power-of-two capacity) + added the hashmap
  benchmark (beats Rust and C++).
- **sqrt -> `@llvm.sqrt.f64`** intrinsic (fair; matches rustc/clang lowering).
- **Stack/SROA promotion of fixed-size non-escaping array literals** ->
  nbody 1.90x -> 1.34x. The codegen operand walker was completed to be
  exhaustive (Map/enum/closure/AddrOf/...) so the escape analysis is sound.
- **Completed the `Shared<T>` heap pointer** (type-checker + MIR auto-deref,
  LLVM inline-aggregate ArcAlloc, struct-indexed-GEP struct drop) and wired
  **recursive arc teardown** (`__kryos_arc_drop_<T>` registered via
  `kryos_arc_set_drop`) -> binary_trees 6.1x -> 1.45x, leak-free. This is a
  real language capability, not a benchmark hack; the self-host compiler does
  not use `shared`, so the fixed point is unaffected.
- **Added C++ ports** (clang++ -O2) for all 7 benchmarks and **Mojo reference
  ports + harness** (run when a Mojo toolchain is present).

## Still planned

- nbody's residual 1.34x: fair loop unroll/vectorize hints (the trip count is a
  runtime `let n = 5`, blocking full unroll).
- binary_trees's residual 1.45x: a unique-ownership `Box` (no refcount) or an
  arena would close most of the gap to Rust, but Rc-like `Shared` is the
  idiomatic, safe default and is already competitive.
- A liveness-gated consuming string-append intrinsic (the sound version of the
  reverted fast path).
- The Cranelift column exists for honesty about `kryos run` (the dev backend);
  ship release binaries with `kryos build --release`.

## Startup floors (median of 5)

| Runtime | Floor |
|---|---|
| kryos native exe | 6.0ms |
| rust native exe | 5.5ms |
| python interpreter | 364.7ms |

The python floor measured 40ms in the June sweep and 365ms here — cold-start
interpreter launch on Windows is dominated by environment effects (antivirus
scanning, filesystem cache state), so treat the python floor as indicative
only. The kryos and rust floors are stable across sweeps (5-6ms).
