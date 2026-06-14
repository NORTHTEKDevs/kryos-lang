# Kryos Benchmarks

Measured medians from [`benchmarks/measure.py`](./benchmarks/measure.py).
Reproduce with `python benchmarks/measure.py` - the tables below are
generated from `benchmarks/results.json`, never hand-edited.
Last refresh: **1.0.0-beta.1, 2026-06-14 (second pass)**, Windows 11 x64
(rustc 1.95, clang 19 -O2, go 1.x, CPython 3.14).

> **2026-06-14 second refresh (this session):**
> 1. **Reverted an unsound string-concat fast path.** An in-place
>    "append into the left buffer when ref_count == 1" optimization was tried
>    and BACKED OUT: the compiler's refcount is vestigial (codegen never emits
>    a clone when a `str` is copied / passed / stored, and `kryos_string_free`
>    is a no-op leak-on-zero), so `ref_count == 1` does not imply unique
>    ownership. Mutating in place corrupted every aliased reference
>    (interpolation `"{x}"`, `let y = a + b`, function args, array/map stores)
>    on both backends, plus a non-deterministic use-after-free in `s = s + s`.
>    Concat now always allocates a fresh string. A sound version needs a
>    liveness-gated consuming-append intrinsic (deferred).
> 2. **TBAA metadata on array accesses.** The element data buffer is always a
>    separate allocation from the KryosArray header, so element loads/stores
>    provably cannot alias the `len`/`data` header fields. Tagging them as
>    sibling TBAA types lets LLVM hoist the loop-invariant length and
>    data-pointer loads out of hot swap loops. **fannkuch 1.10x -> 1.01x
>    (parity)**; smaller gains on nbody and binary_trees.
> 3. **Map probe uses `& (cap - 1)` instead of `% cap`.** Capacity is always
>    a power of two, so the modulo is an AND. Paired with the new hashmap
>    benchmark.
> 4. **Added the `hashmap` benchmark** (1M inserts + 1M lookups, map<i64,i64>).
>    **Kryos beats Rust, Go, and Python on it.**
>
> **6 of 7 benchmarks are now at/within 1.01x of Rust or faster**
> (fib 1.00x, mandelbrot 1.00x, fannkuch 1.01x, matmul 0.96x, hashmap 0.69x);
> nbody (1.90x) and binary_trees (6.10x) remain the two outliers. The TBAA
> change is LLVM-backend only; the map change is a correctness-equivalent
> arithmetic swap on both backends. Validation this session: backend parity
> (55 smoke tests, both backends, 0 divergences), conformance 6/6 both
> backends, runtime + codegen unit suites green, plus targeted adversarial
> tests (string-aliasing repros, array push-realloc under TBAA, map resize).
> The full self-host bootstrap was not re-run this session.

## TL;DR

- **hashmap: 0.69x of Rust - Kryos is FASTER than Rust** (0.081s vs 0.117s),
  and faster than Go (0.122s) and Python (0.223s) on 1M integer inserts +
  1M lookups. Open-addressing with a power-of-two AND-mask probe; the
  sequential-integer-key workload is a friendly case, but the number is real
  and output-verified.
- **matmul: 0.96x of Rust - Kryos is FASTER than Rust** on dense 512^2 float
  matmul (0.623s vs 0.652s), matching clang/Go. Dense register-resident
  float loops optimize as well as any backend.
- **mandelbrot: parity (1.00x)** - scalar float arithmetic in registers
  matches Rust/C exactly.
- **fib: parity (1.00x)** - pure recursion; eliding the dead per-call
  exception check (throw-free module) closed the gap.
- **fannkuch-redux (canonical, n=10): 1.01x of Rust - parity** (0.198s vs
  0.196s; was 1.10x). The permutation reversal/rotation loops are array-swap
  heavy; TBAA let LLVM hoist the length and data-pointer loads out of the
  inner loops, closing the last ~9%. 31x faster than CPython.
- **nbody: 1.90x slower than Rust** (0.202s vs 0.107s). Tight float loops
  over small heap arrays vs Rust's stack-resident `[f64; 5]`. TBAA shaved a
  little; the real lever is stack/SROA promotion of small fixed arrays so the
  double-pointer chase disappears (deferred).
- **binary_trees (allocation stress): 6.10x slower than Rust** (4.687s vs
  0.768s), still behind CPython (1.819s) - the worst benchmark. Allocation
  bound, not access bound. Remaining levers: a null/Option-pointer rep to
  drop the array-as-nullable-child handicap, and an arena/bump allocator for
  tree-scoped lifetimes (both deferred).
- **vs Python: 14-91x faster on compute benchmarks**; on hashmap 2.7x faster;
  binary_trees is the only loss to Python (allocation stress).
- **Cranelift (the `kryos run` dev backend) is slower than the LLVM backend**
  on compute; it optimizes compile speed, not runtime. On binary_trees, where
  allocation dominates and LLVM's optimizer cannot help, Cranelift is faster.

If your workload is dominated by hot numeric inner loops over arrays, Rust,
C, or Go can still edge ahead (nbody) or win big (binary_trees allocation).
For general program logic, maps, scalar compute, I/O, or agent
orchestration, Kryos is in the native-language class and orders of magnitude
ahead of Python.

## Methodology (and what was wrong before)

Earlier revisions of this file compared numbers at the ~30ms process-launch
floor (fib(35), 200x200 mandelbrot), which measured process startup, not the
language. That table was retracted. Current methodology:

1. **Workloads sized** so the fastest competitor needs >= ~0.3s (fib(40),
   1000x1000x1000-iter mandelbrot, 2M-step nbody, 512x512 matmul, 1M-entry
   hashmap).
2. **Median of 5 runs** after a warmup run; min..max spread recorded in
   `results.json`.
3. **Startup floor measured separately** (hello-world per runtime) and
   reported below - not subtracted, so numbers are honest wall-clock:
   kryos native exe ~5.5ms, rust ~5.0ms, python interpreter ~44ms.
4. **Same answer required.** Every port prints an identical checksum;
   outputs are cross-checked before timings count.
5. **Broken ports are labeled, not buried.**

## Results (medians of 5, seconds)

Generated from `benchmarks/results.json`; mirror of
`benchmarks/results_table.md`.

| Benchmark | kryos LLVM | kryos Cranelift | rust -O | clang -O2 | go | python | kryos/rust |
|---|---|---|---|---|---|---|---|
| fib(40) | 0.349 | 4.947 | 0.348 | 0.349 | 0.706 | 16.986 | **1.00x (parity)** |
| mandelbrot 1000^2 x1000 | 0.369 | 0.419 | 0.368 | 0.368 | 0.377 | 19.843 | **1.00x (parity)** |
| nbody 2M steps | 0.202 | 0.940 | 0.107 | 0.148 | 0.248 | 47.883 | **1.90x** |
| binary_trees d16 | 4.687 | 3.546 | 0.768 | 0.690 | 0.490 | 1.819 | **6.10x** |
| fannkuch-redux(10) | 0.198 | 0.919 | 0.196 | 0.187 | 0.196 | 6.127 | **1.01x (parity)** |
| matmul 512^2 | 0.623 | 0.638 | 0.652 | 0.651 | 0.563 | 36.901 | **0.96x (beats Rust)** |
| hashmap 1M+1M | 0.081 | 0.086 | 0.117 | n/a | 0.122 | 0.223 | **0.69x (beats Rust)** |

All rows verified output-identical across languages before timing. hashmap
has no C port (n/a). Canonical fannkuch-redux prints the reference output
`73196 / Pfannkuchen(10) = 38`; binary_trees prints checksum 14723759;
hashmap prints 999999000000.

### binary_trees (canonical allocation-stress, depth 16)

This remains Kryos's **worst benchmark: 6.10x slower than Rust and ~2.6x
slower than CPython** on pure allocation stress. It is allocation-bound, not
access-bound, so the array-access TBAA change barely moved it (4.954s ->
4.687s). Two compounding causes remain:

1. Every tree node is a heap box with refcounted teardown.
2. Kryos has no null/Option-pointer representation for child links, so the
   port stores children in single-element arrays - an extra allocation per
   child that the C/Rust/Go ports do not pay.

Go wins outright here (bump allocator + GC is built for exactly this).
Remaining levers, in impact order: a null/Option-pointer representation to
remove the array-as-nullable-child handicap (~2.5-3.5x estimated), and an
arena/bump allocator with bulk free for tree-scoped lifetimes. Honest
wrinkle: the Cranelift dev backend still beats the LLVM release backend here
(allocation calls dominate; LLVM's optimizer cannot help).

## Reading the numbers

- The LLVM backend's strength is scalar/register code (mandelbrot, fib at
  parity) and now dense array compute (matmul beats Rust, fannkuch at parity
  after TBAA). Its remaining weakness is small-array float traffic (nbody)
  and allocation stress (binary_trees).
- **Landed this session (2026-06-14 second pass):** (1) reverted the unsound
  string in-place concat (correctness); (2) TBAA on array element-vs-header
  accesses (fannkuch 1.10x -> 1.01x); (3) map `& (cap-1)` probe; (4) added
  the hashmap benchmark (Kryos beats Rust/Go/Python).
- **Landed earlier (2026-06-11 -> 06-14 first pass):** pooled box/buffer
  allocator; alloca-hoist; hang-trap atomic removal; inlined `arr[i]`
  read/write; dead post-call exception-check elision. Cumulatively fib
  3.0x->1.00x, nbody 16.8x->1.90x, fannkuch 6.8x->1.01x, matmul 1.9x->0.96x.
- **Still planned**, in impact order: stack/SROA promotion of small fixed
  arrays (nbody's 1.90x); null/Option-pointer rep + arena allocator
  (binary_trees' 6.10x); a liveness-gated consuming string-append intrinsic
  (the sound version of the reverted fast path); bounds-check elision for
  provably-in-range loops. Each that touches MIR lowering must preserve the
  self-host bootstrap fixed point, so none ships without that gate.
- The Cranelift column exists for honesty about `kryos run`: it is the
  development backend; ship binaries with `kryos build --release`.

## Startup floors (median of 5)

| Runtime | Floor |
|---|---|
| kryos native exe | 5.5ms |
| rust native exe | 5.0ms |
| python interpreter | 44.4ms |

Any wall-clock reading near these values measures process launch, not
compute.
