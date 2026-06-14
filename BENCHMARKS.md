# Kryos Benchmarks

Measured medians from [`benchmarks/measure.py`](./benchmarks/measure.py).
Reproduce with `python benchmarks/measure.py` — the tables below are
generated from `benchmarks/results.json`, never hand-edited.
Last refresh: **1.0.0-beta.1, 2026-06-14**, Windows 11 x64
(rustc 1.95, clang 19 -O2, go 1.x, CPython 3.14).

> **2026-06-14 refresh:** the per-element-access hang-trap atomic load was
> removed from `kryos_array_get` (gated behind a default-off feature). Every
> array read previously paid a relaxed atomic load + branch; dropping it cut
> array-heavy benchmarks another 13–31%. **matmul now beats Rust** (0.57s vs
> 0.66s). Cumulative since 2026-06-11: nbody 16.8x→8.3x, binary_trees
> 11.6x→6.4x, fannkuch 6.8x→4.4x, matmul 1.9x→0.86x vs Rust. The two
> non-array benchmarks (fib, mandelbrot) are unchanged — the expected
> signature.

## TL;DR

- **matmul: 0.86x of Rust — Kryos is now FASTER than Rust** on dense 512²
  float matmul (0.568s vs 0.664s), and matches clang/Go. Dense register-
  resident float loops are a case the LLVM backend optimizes as well as any.
- **mandelbrot: parity (1.00x)** — scalar float arithmetic in registers
  matches Rust/C exactly.
- **fib: 3.0x slower than Rust** — pure recursion; no arrays, so untouched by
  the array-access work. Call overhead is the remaining gap.
- **fannkuch-redux (canonical, n=10): 4.4x slower than Rust** (was 6.8x);
  7.1x faster than CPython. Hot integer array loops.
- **binary_trees (allocation stress): 6.4x slower than Rust** (was 11.6x),
  and still ~2.7x behind CPython — the worst benchmark; allocation-bound (see
  its section below). The pooled allocator + a null/Option-pointer rep are
  the remaining levers.
- **nbody: 8.3x slower than Rust** (was 16.8x). Tight float loops over small
  arrays remain the worst *float* case: element access still goes through
  ARC-managed heap arrays with bounds checks while Rust/clang vectorize a
  stack-resident struct array. Bounds-check elision + vectorization are the
  remaining levers.
- **vs Python: 7–62x faster on compute benchmarks — except binary_trees,
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

Medians of 5 runs. The `Δ since 06-11` column is the cumulative change from
the 2026-06-11 numbers (before the pooled allocator, alloca-hoist, and
hang-trap removal), so each improvement is auditable.

| Benchmark | kryos LLVM | kryos Cranelift | rust -O | clang -O2 | go | python | kryos/rust | Δ since 06-11 |
|---|---|---|---|---|---|---|---|---|
| fib(40) | 1.058 | 4.976 | 0.348 | 0.352 | 0.717 | 16.96 | **3.0x** | — (no alloc) |
| mandelbrot 1000² ×1000 | 0.375 | 0.424 | 0.375 | 0.389 | 0.413 | 19.52 | **1.00x** | — (scalar) |
| nbody 2M steps | 0.921 | 0.951 | 0.112 | 0.150 | 0.252 | 45.53 | **8.3x** | 16.8x → 8.3x |
| matmul 512² | 0.568 | 0.682 | 0.664 | 0.661 | 0.568 | 35.18 | **0.86x** | 1.9x → **0.86x (beats Rust)** |
| fannkuch-redux(10)¹ | 0.872 | 0.909 | 0.199 | 0.199 | 0.202 | 6.207 | **4.4x** | 6.8x → 4.4x |

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

| | kryos LLVM | kryos Cranelift | rust -O | clang -O2 | go | python | Δ since 06-11 |
|---|---|---|---|---|---|---|---|
| binary_trees d16 | 4.974 | 3.714 | 0.783 | 0.736 | 0.514 | 1.844 | 9.03s → 4.97s |

This remains Kryos's **worst benchmark, stated plainly: 6.4x slower than
Rust and still ~2.7x slower than CPython** on pure allocation stress — even
after the pooled allocator + hang-trap removal cut its wall-clock by 45%
(9.03s → 4.97s). Two compounding causes remain: (1) every tree node is a heap box
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
- **Landed (2026-06-11 → 06-14):** (1) a pooled box/buffer allocator
  (per-thread size-class freelists); (2) a codegen pass hoisting loop-body
  `alloca`s to the entry block; (3) removal of the per-element-access
  hang-trap atomic load from `kryos_array_get`. Cumulatively these moved
  nbody 16.8x→8.3x, binary_trees 11.6x→6.4x, matmul 1.9x→**0.86x (now beats
  Rust)**, and fannkuch 6.8x→4.4x vs Rust — all without changing emitted
  codegen for the self-host compiler (bootstrap fixed point held at every
  step).
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
