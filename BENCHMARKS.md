# Kryos Benchmarks

Measured medians from [`benchmarks/measure.py`](./benchmarks/measure.py); every
port's output is cross-checked by
[`benchmarks/check_outputs.py`](./benchmarks/check_outputs.py). Reproduce with
`python benchmarks/measure.py`, then `python benchmarks/check_outputs.py`. The
results table is [`benchmarks/results_table.md`](./benchmarks/results_table.md),
copied verbatim; it and the startup floors come from `benchmarks/results.json`,
written by the same run, and are not hand-edited.

Measured at commit: `4d5c93011a638b01492c3f4c55942114df7ab313` (Kryos 1.3.0 plus the 1.3.1 memory wave, unreleased)

- date: 2026-11-24, about 10:54 to 11:12 local time, UTC-9 (about 18 min). Two earlier runs, of the two previous compiler commits, are superseded.
- machine: AMD RYZEN AI MAX+ 395 w/ Radeon 8060S (16 cores, 32 threads), 31.6 GB RAM, Windows 11 Home 10.0.26200
- toolchains: kryos 1.3.0 (the release build of this commit's compiler), rustc 1.95.0, clang and clang++ 21.1.8, go 1.25.7, CPython 3.14.2; no `mojo` on PATH (tool versions as in the 1.3.0 run, not re-queried)
- load: a working desktop, not an idle benchmark host, shared with other build and measurement sessions. CPU use was not sampled this time; no run holding the measurement lock was active during it, but nothing outside that lock was checked. Read the spreads in `results.json`.
- outputs: `check_outputs.py` PASS, every port of every benchmark printed the same output

## Headline

On this commit the LLVM backend (`kryos build --release`) is level with Rust
on four of the seven benchmarks, faster than Rust on one, and clearly behind
on two.

- **Behind Rust:** binary_trees **1.94x** and nbody **1.31x**. On binary_trees,
  C (0.707 s) and Go (0.487 s) are also faster than Kryos (1.513 s).
- **binary_trees: the leak is fixed, the time is not.** The tree's `shared`
  nodes used to be never freed (since 1.1.0) and the run peaked at 1.1 GB;
  they are freed now and it peaks at 24 MB (checksum 14723759 on both). The
  wall time did not improve, and freeing costs a little: master and this
  commit, built from the same program and run interleaved 7 times each in one
  session, took 1.41 to 1.78 s (median 1.47) and 1.55 to 1.91 s (median 1.59),
  which is about 8% slower. The table run below gives 1.513 s (1.476 to 1.549)
  against the 1.455 s of the 1.3.0 run, 4% slower. The ratio went from 1.88x to
  1.94x. The earlier note that most of the slowdown "is a memory leak" was wrong
  about the time: the leak was most of the memory, and the time is the allocator
  and the reference counting (LEDGER item 137). The history of the slowdown is
  in the "What changed (2026-11-03)" section below.
- **Level with Rust (0.95x to 1.01x, inside run-to-run spread):** fib 1.01x,
  mandelbrot 1.00x, fannkuch 1.01x, matmul 0.95x. On matmul Go is faster than
  all of them (0.581 s against Kryos's 0.621 s).
- **Faster than Rust:** hashmap 0.63x (and 4.5x faster than clang++ with
  `std::unordered_map`).
- **Against Python:** 1.42x faster on binary_trees, 6.2x on hashmap, and 31x to
  330x on the other five.

## Results (medians of 5, seconds)

| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | clang++ -O2 | mojo | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|---|---|
| fib | 0.359s | 5.196s | 0.355s | 0.356s | 0.350s | n/a | 0.719s | 17.034s | 1.01x |
| mandelbrot | 0.376s | 0.429s | 0.377s | 0.378s | 0.374s | n/a | 0.383s | 20.224s | 1.00x |
| nbody | 0.145s | 0.922s | 0.111s | 0.151s | 0.151s | n/a | 0.249s | 47.871s | 1.31x |
| binary_trees | 1.513s | 3.057s | 0.780s | 0.707s | 0.763s | n/a | 0.487s | 2.147s | 1.94x |
| fannkuch | 0.202s | 0.931s | 0.200s | 0.190s | 0.187s | n/a | 0.200s | 6.340s | 1.01x |
| matmul | 0.621s | 0.648s | 0.653s | 0.649s | 0.648s | n/a | 0.581s | 33.428s | 0.95x |
| hashmap | 0.080s | 0.085s | 0.128s | n/a | 0.363s | n/a | 0.137s | 0.494s | 0.63x |

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
every cell is in `results.json`. 39 of the 48 cells vary by 10% or less
between their fastest and slowest run; most of the other 9 have one slow run
(clang++ nbody 0.149..0.487, Go fib 0.715..1.057, Go matmul 0.568..0.912). Differences of a few
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
14723759, identical on both backends. (Measured 2026-06. It is no longer true:
since 1.1.0 the `shared` nodes are never freed, the run peaks at 1.1 GB, and on
1.3.0 binary_trees is 1.88x. LEDGER item 137 frees them again: peak 24 MB,
and the wall time is about 4% above 1.3.0, see the headline.)

## Methodology

1. **Workloads sized** (table above) so that the LLVM, Rust, C, C++ and Go
   ports run for 0.09 s to 1.5 s, many times the ~10 ms process-start floor.
2. **Median of 5 runs** after a warmup; min..max spread in `results.json`.
3. **Startup floor measured separately** and not subtracted, so the numbers
   are honest wall-clock; see the table at the end.
4. **Same answer required**: `check_outputs.py` runs every measured port once
   and fails unless all ports of a benchmark print identical output.
5. **Broken/absent ports are labeled n/a, not buried, and never fabricated.**

## What changed (2026-11-24, 1.3.1 memory wave re-measure)

Re-measured because the compiler changed after the 1.3.0 numbers
(`tools/loop/check-bench-current.sh` fails on a stale table). The change that
matters here is that `shared` nodes are freed (LEDGER item 137).

- Kryos/Rust ratios, 1.3.0 -> this commit: fib 1.01x -> 1.01x, mandelbrot
  1.00x -> 1.00x, nbody 1.34x -> 1.31x, binary_trees 1.88x -> 1.94x, fannkuch
  1.04x -> 1.01x, matmul 0.96x -> 0.95x, hashmap 0.59x -> 0.63x. Rust's hashmap
  time moved (0.147 s -> 0.128 s), which is most of the hashmap ratio change;
  Kryos's went 0.086 s -> 0.080 s.
- Apart from binary_trees, every Kryos LLVM time is within 7% of the 1.3.0
  table (hashmap 0.086 -> 0.080 s is the largest move).
- binary_trees: see the headline. Peak memory 1.1 GB -> 24 MB; time 1.455 s ->
  1.513 s (4%) in the table, about 8% above master in an interleaved run.
- The tests/mem probes that exist to measure leaks (a tree or a list built and
  dropped in a loop) run 25 to 30% longer than on master at 1000 iterations,
  because master never freed anything and these programs do nothing else.

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

- binary_trees: the `shared` leak is fixed (LEDGER item 137) but the time is
  not better (4% to 8% slower, the cost of freeing). The extra reference counting and the allocator are what is left;
  a unique-ownership `Box` (no refcount) or an arena would close most of the
  remaining gap to Rust.
- nbody's residual 1.34x: fair loop unroll/vectorize hints (the trip count is a
  runtime `let n = 5`, blocking full unroll).
- A liveness-gated consuming string-append intrinsic (the sound version of the
  reverted fast path).
- The Cranelift column exists for honesty about `kryos run` (the dev backend);
  ship release binaries with `kryos build --release`.

## Startup floors (median of 5)

| Runtime | Floor |
|---|---|
| kryos native exe | 9.4ms |
| rust native exe | 9.5ms |
| python interpreter | 322.2ms |

Process start on Windows depends on antivirus scanning and file-cache state:
the rc.2 sweep (July) measured these floors at 6.0, 5.5 and 364.7 ms. Treat
them as indicative. The kryos and rust floors stay level with each other
across sweeps.
