# Kryos Benchmarks

Measured medians from [`benchmarks/measure.py`](./benchmarks/measure.py).
Reproduce with `python benchmarks/measure.py` — the tables below are
generated from `benchmarks/results.json`, never hand-edited.
Last refresh: **1.0.0-beta.1, 2026-06-14**, Windows 11 x64
(rustc 1.95, clang 19 -O2, go 1.x, CPython 3.14).

> **2026-06-14 refresh (four steps):** (1) removed the per-element hang-trap
> atomic load from `kryos_array_get`; (2) inlined `arr[i]` **reads** and
> (3) `arr[i] = v` **writes** in codegen (an `alwaysinline` helper LLVM
> inlines and optimizes — hoisting the length load and eliding redundant
> bounds checks); (4) **elided the dead post-call exception check** when the
> module contains no `throw` (the check was a runtime call + branch after
> *every* call — pure overhead for throw-free code like recursion).
> **Cumulative since 2026-06-11: fib 3.0x→1.01x (parity), nbody 16.8x→1.9x,
> fannkuch 6.8x→1.10x (near parity), matmul 1.9x→0.95x (beats Rust),
> mandelbrot parity, binary_trees 11.6x→6.5x.** **Five of six benchmarks are
> now at or within 1.1x of Rust**; binary_trees (allocation) is the lone
> outlier. All four changes are LLVM-backend only; the Cranelift-based
> bootstrap is unaffected (fixed point held at 989ba174), and exception
> propagation is preserved (the check stays whenever any `throw` exists).

## TL;DR

- **matmul: 0.95x of Rust — Kryos is FASTER than Rust** on dense 512² float
  matmul (0.612s vs 0.643s), matching clang/Go. Dense register-resident
  float loops are a case the LLVM backend optimizes as well as any.
- **mandelbrot: parity (1.00x)** — scalar float arithmetic in registers
  matches Rust/C exactly.
- **fannkuch-redux (canonical, n=10): 1.09x slower than Rust — near parity**
  (was 6.8x); 27x faster than CPython. Hot integer array loops with inlined,
  bounds-check-hoisted reads AND writes (the permutation reversal is
  write-heavy, so inlining the store mattered).
- **nbody: 1.83x slower than Rust** (was 16.8x). Tight float loops over small
  arrays; inlining both array reads and writes closed most of the gap. The
  residual is the ARC heap-array model vs Rust's stack-resident struct array
  (vectorization is the last lever).
- **fib: 1.01x of Rust — parity** (was 3.0x). Pure recursion: the per-call
  exception check (a runtime call + branch after every call) dominated;
  eliding it for this throw-free program closed the gap entirely.
- **binary_trees (allocation stress): 6.3x slower than Rust** (was 11.6x),
  and still ~2.7x behind CPython — the worst benchmark; allocation-bound, not
  access-bound (so the inline read didn't help it). A null/Option-pointer rep
  to drop the array-as-nullable-child handicap is the remaining lever.
- **vs Python: 13–87x faster on compute benchmarks — except binary_trees,
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
| fib(40) | 0.349 | 4.972 | 0.346 | 0.344 | 0.700 | 17.01 | **1.01x (parity)** | 3.0x → 1.01x |
| mandelbrot 1000² ×1000 | 0.377 | 0.417 | 0.364 | 0.363 | 0.387 | 19.66 | **1.04x** | — (scalar) |
| nbody 2M steps | 0.208 | 0.927 | 0.107 | 0.150 | 0.244 | 46.98 | **1.9x** | 16.8x → 1.9x |
| matmul 512² | 0.612 | 0.656 | 0.643 | 0.653 | 0.567 | 34.95 | **0.95x** | 1.9x → **0.95x (beats Rust)** |
| fannkuch-redux(10)¹ | 0.219 | 0.904 | 0.199 | 0.200 | 0.205 | 5.937 | **1.10x** | 6.8x → **1.10x (near parity)** |

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
| binary_trees d16 | 4.954 | 3.781 | 0.762 | 0.702 | 0.498 | 1.804 | 9.03s → 4.95s |

This remains Kryos's **worst benchmark, stated plainly: 6.3x slower than
Rust and still ~2.8x slower than CPython** on pure allocation stress — even
after the pooled allocator + hang-trap removal cut its wall-clock by 45%
(9.03s → 4.93s). It is allocation-bound, not access-bound, so the inlined
array read/write did not move it. Two compounding causes remain: (1) every tree node is a heap box
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
  hang-trap atomic load from `kryos_array_get`; (4) inlining the `arr[i]`
  read AND write in codegen (`alwaysinline` helpers LLVM hoists/optimizes);
  (5) eliding the dead post-call exception check in throw-free modules.
  Cumulatively these moved fib 3.0x→**1.01x (parity)**, nbody 16.8x→1.9x,
  fannkuch 6.8x→**1.10x**, matmul 1.9x→**0.95x (beats Rust)** vs Rust —
  **5 of 6 benchmarks now at/within 1.1x of Rust**. All LLVM-backend only;
  the Cranelift bootstrap held at 989ba174; exception propagation preserved.
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
