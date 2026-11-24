# Kryos Benchmarks

Measured medians from [`benchmarks/measure.py`](./benchmarks/measure.py); every
port's output is cross-checked by
[`benchmarks/check_outputs.py`](./benchmarks/check_outputs.py). Reproduce with
`python benchmarks/measure.py`, then `python benchmarks/check_outputs.py`. The
results table is [`benchmarks/results_table.md`](./benchmarks/results_table.md),
copied verbatim; it and the startup floors come from `benchmarks/results.json`,
written by the same run, and are not hand-edited.

Measured at commit: `797463af20dcca9c5599b3036aa116599f232704` (Kryos 1.3.0 plus the capability-checker fixes of branch closeout/cap; only the `kryos-types` crate differs from the 1.3.0 benchmark commit)

- date: 2026-11-24, 10:02 to 10:18 UTC (the whole run took 15 min 40 s)
- machine: AMD RYZEN AI MAX+ 395 w/ Radeon 8060S (16 cores, 32 threads), 31.6 GB RAM, Windows 11 Home 10.0.26200
- toolchains: kryos 1.3.0 (a full `cargo build --release` of this commit's compiler), rustc 1.95.0, clang and clang++ 21.1.8, go 1.25.7, CPython 3.14.2; no `mojo` on PATH
- load: a working desktop on a machine shared with other sessions, not an idle benchmark host. CPU load was not sampled during this run (it read 5% just before the start; 3.2 GB of RAM was free just after), so the load of the 2026-11-03 run (median about 12%) is not claimed for this one. No other build or benchmark of mine ran at the same time.
- outputs: `check_outputs.py` PASS, every port of every benchmark printed the same output

## Headline

On this build (Kryos 1.3.0 plus the checker fixes) the LLVM backend (`kryos build --release`) is level with Rust
on four of the seven benchmarks, faster than Rust on one, and clearly behind
on two.

- **Behind Rust:** binary_trees **1.75x** and nbody **1.34x**. On binary_trees,
  C (0.721 s) and Go (0.494 s) are also faster than Kryos (1.406 s).
- **binary_trees has regressed.** It was 1.42x at 1.0.0-rc.2 (2026-07-10).
  Built by each release's own compiler and timed side by side on this machine
  (interleaved, 7 runs each after a warmup): rc.2 1.065 s, 0.9.0 1.223 s,
  1.0.0 1.250 s, 1.0.1 1.217 s, 1.1.0 1.387 s, 1.2.0 1.392 s, 1.3.0 1.436 s,
  against Rust's 0.742 s. It slowed in two steps of about 15% each: between
  rc.2 and 0.9.0, and between 1.0.1 and 1.1.0. 1.3.0 is within 4% of 1.1.0 and
  1.2.0, inside run-to-run spread. Found after this run: most of it is a memory
  leak. The tree's `shared` nodes are never freed (since 0.9.0 in this program,
  since 1.1.0 for any `shared` structure), and the run peaks at 1.1 GB, against
  24 MB at rc.2: [LEDGER item 137](./tools/loop/LEDGER.md).
- **Level with Rust (0.94x to 1.03x, inside run-to-run spread):** fib 1.00x,
  mandelbrot 1.00x, fannkuch 1.03x, matmul 0.94x. On matmul Go is faster than
  all of them (0.571 s against Kryos's 0.626 s).
- **Faster than Rust:** hashmap 0.64x (and 4.1x faster than clang++ with
  `std::unordered_map`).
- **Against Python:** 1.5x faster on binary_trees, 6.0x on hashmap, and 32x to
  327x on the other five.

## Results (medians of 5, seconds)

| Benchmark | kryos-llvm | kryos-cranelift | rust -O | clang -O2 | clang++ -O2 | mojo | go | python | kryos-llvm / rust |
|---|---|---|---|---|---|---|---|---|---|
| fib | 0.357s | 5.031s | 0.356s | 0.357s | 0.354s | n/a | 0.717s | 17.350s | 1.00x |
| mandelbrot | 0.374s | 0.426s | 0.374s | 0.374s | 0.373s | n/a | 0.384s | 20.868s | 1.00x |
| nbody | 0.147s | 0.948s | 0.109s | 0.153s | 0.153s | n/a | 0.249s | 48.099s | 1.34x |
| binary_trees | 1.406s | 3.132s | 0.802s | 0.721s | 0.773s | n/a | 0.494s | 2.155s | 1.75x |
| fannkuch | 0.203s | 0.942s | 0.197s | 0.193s | 0.188s | n/a | 0.201s | 6.505s | 1.03x |
| matmul | 0.626s | 0.647s | 0.664s | 0.662s | 0.655s | n/a | 0.571s | 36.456s | 0.94x |
| hashmap | 0.081s | 0.084s | 0.126s | n/a | 0.334s | n/a | 0.138s | 0.490s | 0.64x |

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
every cell is in `results.json`. 42 of the 48 cells vary by 10% or less
between their fastest and slowest run (spread = max/min - 1); the other 6 are
rust nbody 0.107..0.119, clang++ nbody 0.150..0.459, clang fannkuch
0.191..0.492, python matmul 32.1..38.7, rust hashmap 0.123..0.144 and go
hashmap 0.121..0.423. Differences of a few percent between toolchains are
inside that spread: treat them as ties.

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
the 2026-11-24 build binary_trees is 1.75x. See the headline and LEDGER item 137.)

## Methodology

1. **Workloads sized** (table above) so that the LLVM, Rust, C, C++ and Go
   ports run for 0.09 s to 1.5 s, many times the ~10 ms process-start floor.
2. **Median of 5 runs** after a warmup; min..max spread in `results.json`.
3. **Startup floor measured separately** and not subtracted, so the numbers
   are honest wall-clock; see the table at the end.
4. **Same answer required**: `check_outputs.py` runs every measured port once
   and fails unless all ports of a benchmark print identical output.
5. **Broken/absent ports are labeled n/a, not buried, and never fabricated.**

## What changed (2026-11-24, re-measure at the capability-checker fixes)

Re-measured because `check-bench-current.sh` requires the numbers to describe the
compiler that ships, and the checker changed after the 2026-11-03 run
(`git diff cbd88996 797463af -- compiler/crates` is four files, all in
`kryos-types`: check.rs, env.rs, infer.rs, ty.rs). The code generator and the
runtime did not change; this run is the evidence that the type-checker work
cost nothing at run time.

- Kryos/Rust ratios, 2026-11-03 -> 2026-11-24: fib 1.01x -> 1.00x, mandelbrot
  1.00x -> 1.00x, nbody 1.34x -> 1.34x, binary_trees 1.88x -> 1.75x, fannkuch
  1.04x -> 1.03x, matmul 0.96x -> 0.94x, hashmap 0.59x -> 0.64x.
- Kryos LLVM times: fib 0.362 -> 0.357, mandelbrot 0.383 -> 0.374, nbody 0.150
  -> 0.147, binary_trees 1.455 -> 1.406, fannkuch 0.204 -> 0.203, matmul 0.620
  -> 0.626, hashmap 0.086 -> 0.081 seconds: every one within 4%. The ratios
  that moved (binary_trees, hashmap) moved mostly through the Rust column
  (binary_trees 0.772 -> 0.802, hashmap 0.147 -> 0.126), which is inside Rust's
  own spread on this shared machine. The binary_trees leak (LEDGER item 137) is
  not fixed by this change.
- The load of this run was not sampled (see the provenance block).

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

- binary_trees: fix the `shared` leak and the extra reference counting behind
  most of its slowdown (LEDGER item 137), then re-measure. Beyond that, a
  unique-ownership `Box` (no refcount) or an arena would close most of the
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
| kryos native exe | 9.0ms |
| rust native exe | 8.1ms |
| python interpreter | 319.6ms |

Process start on Windows depends on antivirus scanning and file-cache state:
the rc.2 sweep (July) measured these floors at 6.0, 5.5 and 364.7 ms. Treat
them as indicative. The kryos and rust floors stay level with each other
across sweeps.
