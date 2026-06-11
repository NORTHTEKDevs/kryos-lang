# Kryos Benchmarks

Measured medians from [`benchmarks/measure.py`](./benchmarks/measure.py).
Reproduce with `python benchmarks/measure.py` — the tables below are
generated from `benchmarks/results.json`, never hand-edited.
Last refresh: **1.0.0-beta.1, 2026-06-11**, Windows 11 x64
(rustc 1.95, clang 19 -O2, go 1.x, CPython 3.14).

## TL;DR — where Kryos loses, first

- **nbody: 16.8x slower than Rust.** Tight float loops over small arrays are
  Kryos's worst case today: every element access goes through ARC-managed
  heap arrays with bounds checks; Rust/clang vectorize a stack-resident
  struct array. This is the honest cost of the current array model.
- **fannkuch-redux (canonical, n=10): 6.8x slower than Rust** — array-access
  cost on hot integer loops; 4.6x faster than CPython.
- **binary_trees (allocation stress): 11.6x slower than Rust, and behind
  CPython** — the worst case; see its section below.
- **fib: 3.0x slower than Rust** — pure recursion; closer, still behind on
  call overhead.
- **matmul: 1.9x slower than Rust** — dense float loops, gap narrows when
  work per access grows.
- **mandelbrot: parity (1.00x)** — scalar float arithmetic in registers is
  the case where the LLVM backend matches Rust/C, because there is nothing
  for Kryos's array model to pay for.
- **vs Python: 4.6–53x faster on compute benchmarks — except binary_trees,
  where Python wins** (allocation stress; see below).
- **Cranelift (the `kryos run` dev backend) is 1.1–4.7x slower than the LLVM
  backend** — it optimizes compile speed, not runtime.

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

| Benchmark | kryos LLVM | kryos Cranelift | rust -O | clang -O2 | go | python | kryos/rust |
|---|---|---|---|---|---|---|---|
| fib(40) | 1.043 | 4.861 | 0.344 | 0.342 | 0.699 | 16.18 | **3.0x** |
| mandelbrot 1000² ×1000 | 0.365 | 0.409 | 0.364 | 0.359 | 0.371 | 19.44 | **1.0x** |
| nbody 2M steps | 1.852 | 1.865 | 0.110 | 0.151 | 0.250 | 45.13 | **16.8x** |
| matmul 512² | 1.205 | 1.244 | 0.644 | 0.641 | 0.563 | 34.48 | **1.9x** |
| fannkuch-redux(10)¹ | 1.341 | 1.416 | 0.198 | 0.194 | 0.205 | 6.145 | **6.8x** |

All rows verified output-identical across languages before timing.

¹ Canonical fannkuch-redux (Benchmarks Game shape, added 2026-06-11): all
five ports generate all 10! permutations via the counting algorithm and
print the reference output `73196 / Pfannkuchen(10) = 38`, verified
identical before timing. This replaces the earlier *perm-flips* row, which
was a simplified kernel mislabeled "fannkuch" (and whose C port was broken
and excluded). The canonical workload is ~13x more work and Kryos's ratio
is **better** on it (6.8x vs the old kernel's 13.2x) — the old number
overweighted per-iteration array-bounds overhead on a tiny workload.

### binary_trees (canonical allocation-stress, depth 16) — added 2026-06-11

The earlier single-tree port was constant-folded by optimizing compilers and
has been replaced with the canonical many-trees-checksummed form (all five
ports verified output-identical: checksum 14723759).

| | kryos LLVM | kryos Cranelift | rust -O | clang -O2 | go | python |
|---|---|---|---|---|---|---|
| binary_trees d16 | 9.030 | 5.201 | 0.778 | 0.699 | 0.495 | 1.876 |

This is Kryos's **worst benchmark, stated plainly: 11.6x slower than Rust
and ~4.8x slower than CPython** on pure allocation stress. Two compounding
causes: (1) every tree node is a calloc'd box with refcounted teardown, and
(2) Kryos has no null/Option-pointer representation for child links, so the
port stores children in single-element arrays — an extra allocation per
child that the C/Rust/Go ports don't pay. Go wins outright here (bump
allocator + GC is built for exactly this). Allocation-heavy workloads are
the language's weakest case today; an arena/slab story is the queued fix.
Interesting honest wrinkle: the Cranelift dev backend beats the LLVM
release backend on this one (allocation calls dominate; LLVM's optimizer
cannot help).

## Reading the numbers

- The Kryos LLVM backend's strength is scalar/register code (mandelbrot at
  parity). Its weakness is **array traffic**: Kryos arrays are ARC-managed
  heap objects with bounds checks and no vectorization of access loops yet.
  nbody/fannkuch-class inner loops pay that on every element.
- Planned work that would move these numbers, in impact order: bounds-check
  elision in provably-safe loops, scalar replacement for small fixed arrays,
  and loop vectorization hints to LLVM. No timeline promised.
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
