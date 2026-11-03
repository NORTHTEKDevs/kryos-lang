# Kryos Benchmarks

Measured medians from [`benchmarks/measure.py`](./benchmarks/measure.py); every
port's output is cross-checked by
[`benchmarks/check_outputs.py`](./benchmarks/check_outputs.py). Reproduce with
`python benchmarks/measure.py`, then `python benchmarks/check_outputs.py`. The
results table is [`benchmarks/results_table.md`](./benchmarks/results_table.md),
copied verbatim; it and the startup floors come from `benchmarks/results.json`,
written by the same run, and are not hand-edited.

Measured at commit: `cbd88996d1b11ae2172fe8e511bbce37fb1cf21b` (Kryos 1.3.0)

- date: 2026-11-03, 19:19 to 19:36 UTC (the whole run took 16 min 40 s)
- machine: AMD RYZEN AI MAX+ 395 w/ Radeon 8060S (16 cores, 32 threads), 31.6 GB RAM, Windows 11 Home 10.0.26200
- toolchains: kryos 1.3.0 (the release build of this commit's compiler), rustc 1.95.0, clang and clang++ 21.1.8, go 1.25.7, CPython 3.14.2; no `mojo` on PATH
- load: a working desktop, not an idle benchmark host. Total CPU use, sampled every 10 s during the run, had a median of about 12% (6.1% to 20.3%); 4.1 GB of RAM was free at the start.
- outputs: `check_outputs.py` PASS, every port of every benchmark printed the same output

## Headline

On Kryos 1.3.0 the LLVM backend (`kryos build --release`) is level with Rust
on four of the seven benchmarks, faster than Rust on one, and clearly behind
on two.

- **Behind Rust:** binary_trees **1.88x** and nbody **1.34x**. On binary_trees,
  C (0.691 s) and Go (0.492 s) are also faster than Kryos (1.455 s).
- **binary_trees has regressed.** It was 1.42x at 1.0.0-rc.2 (2026-07-10).
  Built by each release's own compiler and timed side by side on this machine
  (interleaved, 7 runs each after a warmup): rc.2 1.065 s, 0.9.0 1.223 s,
  1.0.0 1.250 s, 1.0.1 1.217 s, 1.1.0 1.387 s, 1.2.0 1.392 s, 1.3.0 1.436 s,
  against Rust's 0.742 s. It slowed in two steps of about 15% each: between
  rc.2 and 0.9.0, and between 1.0.1 and 1.1.0. 1.3.0 is within 4% of 1.1.0 and
  1.2.0, inside run-to-run spread. The causes are not yet known:
  [LEDGER item 137](./tools/loop/LEDGER.md).
- **Level with Rust (0.96x to 1.04x, inside run-to-run spread):** fib 1.01x,
  mandelbrot 1.00x, fannkuch 1.04x, matmul 0.96x. On matmul Go is faster than
  all of them (0.561 s against Kryos's 0.620 s).
- **Faster than Rust:** hashmap 0.59x (and 5.7x faster than clang++ with
  `std::unordered_map`).
- **Against Python:** 1.6x faster on binary_trees, 7.8x on hashmap, and 32x to
  307x on the other five.

## Results (medians of 5, seconds)

| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | clang++ -O2 | mojo | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|---|---|
| fib | 0.362s | 4.955s | 0.358s | 0.356s | 0.357s | n/a | 0.722s | 17.758s | 1.01x |
| mandelbrot | 0.383s | 0.431s | 0.381s | 0.381s | 0.373s | n/a | 0.391s | 20.466s | 1.00x |
| nbody | 0.150s | 0.969s | 0.112s | 0.154s | 0.158s | n/a | 0.258s | 46.104s | 1.34x |
| binary_trees | 1.455s | 3.291s | 0.772s | 0.691s | 0.732s | n/a | 0.492s | 2.389s | 1.88x |
| fannkuch | 0.204s | 0.918s | 0.196s | 0.192s | 0.187s | n/a | 0.197s | 6.542s | 1.04x |
| matmul | 0.620s | 0.614s | 0.645s | 0.645s | 0.643s | n/a | 0.561s | 34.818s | 0.96x |
| hashmap | 0.086s | 0.091s | 0.147s | n/a | 0.487s | n/a | 0.159s | 0.671s | 0.59x |

| Benchmark | Workload |
|---|---|
| fib | fib(40), recursive |
| mandelbrot | 1000x1000 points, up to 1000 iterations each |
| nbody | 5 bodies, 2,000,000 steps |
| binary_trees | trees of depth 16 |
| fannkuch | fannkuch-redux, n = 10 |
| matmul | 512x512 integer matrices |
| hashmap | 1,000,000 inserts, then 1,000,000 lookups |

Each cell is the median of 5 timed runs after one warmup run; the min..max of
every cell is in `results.json`. 34 of the 48 cells vary by 10% or less
between their fastest and slowest run; most of the other 14 have one slow run
(clang++ fannkuch 0.182..0.338, clang nbody 0.148..0.263). Differences of a few
percent between toolchains are inside that spread: treat them as ties.

hashmap has no C port (n/a); the clang++ port uses `std::unordered_map`. The
outputs `check_outputs.py` compared: fib(40) = 102334155, mandelbrot checksum
172812923, nbody checksum -188027, binary_trees checksum 14723759, fannkuch
73196 / Pfannkuchen(10) = 38, matmul checksum 2013258329, hashmap
999999000000.

### On Mojo (not measured here)

Mojo is shown as `n/a`: there is no `mojo` binary on PATH on this host, and we
do not fabricate numbers. Reference ports of fib, mandelbrot and matmul and
harness support are in [`benchmarks/mojo/`](./benchmarks/mojo/); `measure.py`
builds and times them automatically when a `mojo` binary is on PATH. Published
positioning: Mojo lowers MLIR -> LLVM (same backend as clang/rustc), so on
tight numeric kernels optimized Mojo is expected in the same band as the other
LLVM languages here. Whether Kryos and Mojo are level on these kernels is not
measured; a head-to-head needs the ports run on a Mojo host.

### binary_trees: from 6.1x to 1.45x (the 2026-06 rewrite)

The original port stored each child as a single-element `[Tree]` array =
**three** heap allocations per child (array header + data buffer + boxed
struct). It is now `Tree { left: Option<Shared<Tree>>, right: Option<Shared<Tree>> }`
= **one** arc allocation per node (the new, completed `Shared<T>` heap pointer).
Recursive refcounted teardown is wired (each node's arc block carries a
`__kryos_arc_drop_<T>` drop function, so releasing a tree's root cascades), so
the run is **leak-free: 24 MB peak** (was 1.1 GB while the teardown was
missing), an apples-to-apples comparison against the freeing C/Rust/Go ports.
The residual 1.45x vs Rust is mostly Rc refcount traffic vs Rust's `Box`
unique-ownership; Go (0.488s) wins via its GC bump allocator. checksum
14723759, identical on both backends. (Measured 2026-06. On 1.3.0 binary_trees
is 1.88x: see the headline and LEDGER item 137.)

## Methodology

1. **Workloads sized** (table above) so that the LLVM, Rust, C, C++ and Go
   ports run for 0.09 s to 1.5 s, many times the ~10 ms process-start floor.
2. **Median of 5 runs** after a warmup; min..max spread in `results.json`.
3. **Startup floor measured separately** and not subtracted, so the numbers
   are honest wall-clock; see the table at the end.
4. **Same answer required**: `check_outputs.py` runs every measured port once
   and fails unless all ports of a benchmark print identical output.
5. **Broken/absent ports are labeled n/a, not buried, and never fabricated.**

## What changed (2026-11-03, 1.3.0 re-measure)

The first full re-run since rc.2; 0.9.0, 1.0.0, 1.0.1, 1.1.0 and 1.2.0 shipped
with the rc.2 table.

- Kryos/Rust ratios, rc.2 table -> 1.3.0: fib 1.03x -> 1.01x, mandelbrot
  1.00x -> 1.00x, nbody 1.31x -> 1.34x, binary_trees 1.42x -> 1.88x, fannkuch
  1.02x -> 1.04x, matmul 0.96x -> 0.96x, hashmap 0.65x -> 0.59x.
- Apart from binary_trees (1.097 s -> 1.455 s), every Kryos LLVM time is
  within 7% of the rc.2 table, and so is every Rust time except hashmap
  (0.127 s -> 0.147 s), which is why hashmap's ratio moved.
- binary_trees slowed in two steps of about 15% each, between rc.2 and 0.9.0
  and between 1.0.1 and 1.1.0 (side-by-side numbers in the headline). The rc.2
  build measured 1.065 s in that comparison, close to the 1.097 s of the rc.2
  table, so the comparison and the July sweep agree.
- New: `benchmarks/check_outputs.py`. `measure.py` never compared outputs, so
  the "same answer required" step had been a manual one.
- The README's comparison table now shows this run. It had shown figures from
  an older harness (best of 10 runs in a sandbox VM with a ~30 ms launch
  floor).

## What changed (2026-07-10, rc.2 re-measure)

Full re-run on the rc.2 binary after the memory-model overhaul (type-stable
`HeaderPool` header recycling, retain-on-read borrow fixes), the
value-semantics fixes (share-retain container let-bindings, independent-copy
array rebinds via `kryos_array_dup`), and the ~8,000-program differential
fuzz campaign. Every number is within run-to-run noise of the June sweep:
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

- binary_trees: find what slowed it down between rc.2 and 0.9.0 and between
  1.0.1 and 1.1.0 (LEDGER item 137). Beyond that, a unique-ownership `Box` (no
  refcount) or an arena would close most of the remaining gap to Rust; Rc-like
  `Shared` is the idiomatic, safe default.
- nbody's residual 1.34x: fair loop unroll/vectorize hints (the trip count is a
  runtime `let n = 5`, blocking full unroll).
- A liveness-gated consuming string-append intrinsic (the sound version of the
  reverted fast path).
- The Cranelift column exists for honesty about `kryos run` (the dev backend);
  ship release binaries with `kryos build --release`.

## Startup floors (median of 5)

| Runtime | Floor |
|---|---|
| kryos native exe | 9.6ms |
| rust native exe | 9.1ms |
| python interpreter | 561.5ms |

Process start on Windows depends on antivirus scanning and file-cache state:
the rc.2 sweep (July) measured these floors at 6.0, 5.5 and 364.7 ms. Treat
them as indicative. The kryos and rust floors stay level with each other
across sweeps.
