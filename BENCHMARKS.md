# Kryos Benchmarks

Measured medians from [`benchmarks/measure.py`](./benchmarks/measure.py).
Reproduce with `python benchmarks/measure.py` — the tables below are
generated from `benchmarks/results.json`, never hand-edited.
Last refresh: **1.0.0-beta.1, 2026-06-13**, Windows 11 x64
(rustc 1.95, clang 19 -O2, go 1.x, CPython 3.14).

> **2026-06-13 refresh:** a pooled box/buffer allocator (per-thread
> size-class freelists) plus a codegen pass that hoists loop-body `alloca`s
> to the entry block improved every allocation- or loop-heavy benchmark by
> 27–39%. The two benchmarks that neither allocate nor spill in a hot loop
> (fib, mandelbrot) are unchanged — the expected signature. Old numbers are
> shown in the deltas below so the win is auditable, not asserted.

## TL;DR — where Kryos loses, first

- **nbody: 10.7x slower than Rust** (was 16.8x). Tight float loops over small
  arrays are still Kryos's worst *float* case: every element access goes
  through ARC-managed heap arrays with bounds checks while Rust/clang
  vectorize a stack-resident struct array. The pooled allocator removed the
  per-step allocation overhead; the remaining gap is the array-access model.
- **binary_trees (allocation stress): 7.0x slower than Rust** (was 11.6x),
  and **still behind CPython** (5.53s vs 1.89s) — the worst benchmark; see
  its section below. The queued arena/slab fix partially landed (the pool).
- **fannkuch-redux (canonical, n=10): 4.8x slower than Rust** (was 6.8x);
  6.2x faster than CPython. The alloca-hoist helped the 3.6M-iteration
  permutation loop directly.
- **fib: 2.9x slower than Rust** — pure recursion; no allocation, so
  unchanged by this refresh. Still behind on call overhead.
- **matmul: 1.25x slower than Rust** (was 1.9x) — dense float loops; the gap
  is now small.
- **mandelbrot: parity (1.03x)** — scalar float arithmetic in registers is
  the case where the LLVM backend matches Rust/C; nothing for the array
  model to pay for, and nothing for the allocator to improve.
- **vs Python: 6–53x faster on compute benchmarks — except binary_trees,
  where Python still wins** (allocation stress; see below).
- **Cranelift (the `kryos run` dev backend) is 1.1–4.7x slower than the LLVM
  backend** — it optimizes compile speed, not runtime. (On binary_trees,
  where allocation dominates and LLVM's optimizer can't help, Cranelift is
  actually faster than the LLVM backend.)

If your workload is dominated by hot numeric inner loops over arrays, Rust,
C, or Go will be faster today. If it is general program logic, string and
structure manipulation, I/O, or agent orchestration, Kryos's numbers are in
the native-language class — and orders of magnitude ahead of Python.

## Methodology (and what was wrong before)

Earlier revisions of this file compared numbers at the ~30ms process-launch
floor (fib(35), 200×200 mandelbrot): several rows showed Kryos == Rust ==
gcc at "0.032s", which measured **process startup, not the language**. That
table is retracted. Current methodology:

1. **Workloads sized** so the fastest competitor needs ≥ ~0.3s (fib(40),
   1000×1000×1000-iter mandelbrot, 2M-step nbody, 512×512 matmul).
2. **Median of 5 runs** after a warmup run; min..max spread recorded in
   `results.json`.
3. **Startup floor measured separately** (hello-world per runtime) and
   reported below — it is not subtracted, but you can see what it is:
   kryos native exe ≈ **5.5ms**, rust ≈ 5.8ms, python interpreter ≈ 44ms.
4. **Same answer required.** Every port must print an identical checksum;
   outputs are cross-checked before timings count.
5. **Broken ports are labeled, not buried** (see notes).

## Results (medians of 5, seconds)

Medians of 5 runs. The `Δ vs prev` column is the change from the
2026-06-11 numbers (before the pooled allocator + alloca-hoist), so each
improvement is auditable.

| Benchmark | kryos LLVM | kryos Cranelift | rust -O | clang -O2 | go | python | kryos/rust | Δ vs prev |
|---|---|---|---|---|---|---|---|---|
| fib(40) | 1.049 | 4.939 | 0.364 | 0.372 | 0.728 | 16.94 | **2.9x** | — (no alloc) |
| mandelbrot 1000² ×1000 | 0.390 | 0.422 | 0.380 | 0.375 | 0.419 | 20.22 | **1.03x** | — (scalar) |
| nbody 2M steps | 1.199 | 1.214 | 0.112 | 0.150 | 0.252 | 47.86 | **10.7x** | 16.8x → 10.7x |
| matmul 512² | 0.825 | 0.909 | 0.660 | 0.651 | 0.571 | 37.33 | **1.25x** | 1.9x → 1.25x |
| fannkuch-redux(10)¹ | 0.977 | 1.050 | 0.204 | 0.193 | 0.205 | 6.048 | **4.8x** | 6.8x → 4.8x |

All rows verified output-identical across languages before timing.

¹ Canonical fannkuch-redux (Benchmarks Game shape, added 2026-06-11): all
five ports generate all 10! permutations via the counting algorithm and
print the reference output `73196 / Pfannkuchen(10) = 38`, verified
identical before timing. This replaces the earlier *perm-flips* row, which
was a simplified kernel mislabeled "fannkuch" (and whose C port was broken
and excluded). The 2026-06-13 alloca-hoist pass moved per-iteration stack
slots out of the 3.6M-iteration permutation loop, dropping the ratio from
6.8x to 4.8x.

### binary_trees (canonical allocation-stress, depth 16)

The earlier single-tree port was constant-folded by optimizing compilers and
has been replaced with the canonical many-trees-checksummed form (all five
ports verified output-identical: checksum 14723759).

| | kryos LLVM | kryos Cranelift | rust -O | clang -O2 | go | python | Δ vs prev |
|---|---|---|---|---|---|---|---|
| binary_trees d16 | 5.532 | 3.753 | 0.785 | 0.751 | 0.513 | 1.887 | 9.03s → 5.53s |

This remains Kryos's **worst benchmark, stated plainly: 7.0x slower than
Rust and still ~2.9x slower than CPython** on pure allocation stress — even
after the 2026-06-13 pooled allocator cut its wall-clock by 39% (9.03s →
5.53s). Two compounding causes remain: (1) every tree node is a heap box
with refcounted teardown, and (2) Kryos has no null/Option-pointer
representation for child links, so the port stores children in
single-element arrays — an extra allocation per child that the C/Rust/Go
ports don't pay. Go wins outright here (bump allocator + GC is built for
exactly this). The queued arena/slab fix partially landed as the pooled
box allocator; eliminating the array-children handicap is the remaining
lever. Honest wrinkle: the Cranelift dev backend still beats the LLVM
release backend on this one (allocation calls dominate; LLVM's optimizer
cannot help).

## Reading the numbers

- The Kryos LLVM backend's strength is scalar/register code (mandelbrot at
  parity). Its weakness is **array traffic**: Kryos arrays are ARC-managed
  heap objects with bounds checks and no vectorization of access loops yet.
  nbody/matmul-class inner loops pay that on every element.
- **Landed (2026-06-13):** a pooled box/buffer allocator (per-thread
  size-class freelists) and a codegen pass hoisting loop-body `alloca`s to
  the entry block. Together these cut allocation and per-iteration stack
  overhead, moving nbody 16.8x→10.7x, binary_trees 11.6x→7.0x, matmul
  1.9x→1.25x, and fannkuch 6.8x→4.8x vs Rust.
- **Still planned**, in impact order: bounds-check elision in provably-safe
  loops, scalar replacement for small fixed arrays, loop-vectorization hints
  to LLVM, and a null/Option-pointer representation to remove the
  array-as-nullable-child handicap that dominates binary_trees. No timeline
  promised; each that touches MIR lowering must preserve the bootstrap fixed
  point.
- The Cranelift column exists for honesty about `kryos run`: it is the
  development backend; ship binaries with `kryos build --release`.

## Startup floors (median of 5)

| Runtime | Floor |
|---|---|
| kryos native exe | 5.5ms |
| rust native exe | 5.8ms |
| python interpreter | 44.4ms |

Any wall-clock reading near these values measures process launch, not
compute — which is exactly what was wrong with the previous revision of
this file.
