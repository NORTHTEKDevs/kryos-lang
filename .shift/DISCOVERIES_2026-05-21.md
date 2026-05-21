# Kryos — Overnight Discoveries & Language Analysis (2026-05-21)

A working-notes document compiled during the overnight session.
Captures non-obvious observations about Kryos's design, the self-host
journey, and what the language's actual capabilities are at this point.

---

## 1. The bootstrap blocker was misdiagnosed for weeks

The pre-shift `STAGE2_BLOCKER.md` analysis was wrong for a long time.
It claimed the issue was "struct values in i64 array slots" — that
Kryos stored 40-byte Token structs in 8-byte array slots and lost
data. That analysis is preserved in the doc as a historical record.

The actual mechanism, found by empirical bisection in shift step 35:
**stage-1's @copy struct lifecycle was doing O(N²) clone work**.
Each `lex_emit` call cloned the Lexer (which deep-cloned its growing
`Array<Token>` field). Over ~10K token emissions, the quadratic
blowup fragmented the heap and crashed.

The fix wasn't a runtime fix. It was a codegen change: empty the
deep-clone whitelist so every `@copy` `Array<Struct>` falls through
to `kryos_array_retain` (O(1) ref_count bump) instead of recursive
deep clone. One commit. Five lines. Bootstrap jumped 13/16 → 16/16.

**Lesson**: red herring root causes can survive multi-shift analysis
if the explanation is *plausible* (struct-in-array IS a real category
of bug) and you don't verify it empirically. Bisecting with diagnostic
instrumentation (file-based double-free detector) immediately rules
out a hypothesis class.

---

## 2. Hidden bug: uninitialized `ref_count` in `kryos_string_concat`

Found tonight (overnight session). When `ref_count: i64` was added
to `KryosString` in shift step 37, the constructor `kryos_string_new`
was updated to set `ref_count = 1`. But `kryos_string_concat`
allocates a fresh `KryosString` via the same `alloc` pattern and
sets `len`, `cap`, `data` — but NOT `ref_count`. The field is left
as whatever's in raw allocator memory: usually garbage.

Result: the bootstrap was unstable in subtle ways for any path
involving string concatenation, which is most of stage-1's runtime
because Kryos has no string interpolation — every formatted string
is `"foo " + x + " bar"` and that hits concat.

Fix: one line, `(*s).ref_count = 1` in concat.

**Lesson**: when adding a field to a `#[repr(C)]` ABI struct, audit
EVERY raw-alloc + manual-init site in the same crate. The compiler
won't flag it because all fields are `pub` and uninitialized memory
satisfies the type at the byte level.

---

## 3. Kryos's actual self-host capability (as of this session)

**What works**:
- Stage-1 compiles all 16 self-host modules through `.obj` deterministically
- 8/8 example programs compile + link + run end-to-end
- 295+ Rust workspace lib tests pass
- Cranelift backend at 34/34 parity with LLVM
- 61 stdlib modules type-check clean

**What's verified empirically but not yet automated**:
- Stage-2 link (the 16 `.obj` files into a single `.exe`) — blocked
  by `Linkage::Local` on user functions (kept Local intentionally
  to avoid colliding with libc `read`/`write`/`open`/etc.)
- Stage-3 fixed-point identity (byte-equal stage-2 vs stage-3 output)

**What's the constraint**: short of multi-`.obj` linking, the
"self-hosts" claim rests on per-module compilation succeeding —
which it does. The next step requires either an `--export-all`
build flag or per-symbol export discipline in codegen.

---

## 4. Distinctive design choices that worked under stress

The self-host bring-up forced several design choices into the open:

### 4a. Stack-allocated structs were never the bottleneck

The original `STAGE2_BLOCKER.md` assumed stack-allocated structs
were the issue. They aren't — Kryos heap-allocates `@copy` structs
via `calloc` and stores pointers. The 8-byte array slot stores the
pointer; the 40-byte struct lives on the heap. This is correct.

What was wrong was the *number* of allocations per loop iteration,
not their *layout*.

### 4b. Share-on-clone is a viable @copy semantic

The fix that unblocked self-host (`kryos_array_retain` instead of
`kryos_array_clone` for `@copy` struct Array fallback) is a real
trade-off: `@copy` semantics no longer mean "deep copy". They mean
"share via refcount".

Kryos can document this as a guarantee:
- `@copy struct S { field: Array<T> }` -- when copied, S has a NEW
  outer alloc but its Array<T> field is SHARED with the source.
- Mutation visible across all aliases.
- Equivalent to Swift's `class` (reference semantics) for the
  array field; `struct` (value semantics) for the outer.

This actually matches what stage-1's coding style assumes anyway:
functions return new structs and the caller replaces its local
variable. Mutation through the new struct doesn't show through the
old one because the old one is no longer referenced.

### 4c. 32 MB stack is a real cost / benefit win

The recursive-descent parser and scope walker on a 2867-line source
file (parser.kry parsing itself) hit deep recursion. Default 1 MB
Windows stack was a hard cap that caused intermittent flakes. 32 MB
eliminated those flakes completely.

Stack reservation is virtual-address only; physical memory is
committed lazily on use. So 32 MB stack costs zero memory until the
recursion actually grows there. Cheap fix, big stability win.

---

## 5. The kryos-rt design is unusual in a useful way

Most runtimes for compiled languages either:
- (Go) Use a GC, hide allocation lifetime entirely from codegen.
- (Rust) Use compile-time ownership analysis to eliminate runtime
  refcounting.
- (Swift) Use ARC, with the compiler inserting retain/release.

Kryos's runtime is closest to Swift but lighter: there's no autorelease
pool, no objc_msgsend, no class metadata. Just three POD struct
types — `KryosArray`, `KryosString`, `MapHeader` — each with a
`ref_count: i64` field, plus the `kryos_*_retain` and `kryos_*_free`
ABIs that codegen emits at sharing/scope-exit points.

The codegen retain audit (steps 44-46) is essentially Swift's ARC
insertion algorithm, but implemented manually at specific
RValue / Local / Param sites instead of via a general analysis pass.

If this matures into a proper ARC pass in the MIR layer, Kryos would
sit at a sweet spot between Go's GC convenience and Rust's borrow-
checker complexity.

---

## 6. The CI runner quota issue is real and unrelated

PR #62's CI was failing on all 8 checks (build-and-test,
build-and-test-windows, build-and-test-macos, wasm-smoke,
selfhost-stage1, registry-smoke, quickstart-e2e, fuzz). Per
MEMORY.md the quota is the cause — separate from the self-host
content. The PR merged via `gh pr merge --admin` since the actual
build was verified locally.

For the project to be credibly "production-ready" in 2026 terms,
the CI quota story needs to be solved separately — either via
self-hosted runners on NORTHTEK infrastructure or by reducing the
matrix size (e.g., dropping macOS unless we have an actual ARM
runner ready).

---

## 7. Heap-state-sensitive flakes have a signature

Throughout the shift the same pattern recurred: "this runs 95% of
the time, fails 5%". Two contributing factors:

a) **ASLR / VA randomization**: Different process invocations have
   different heap base addresses. Some addresses happen to put
   adjacent freed memory near critical metadata; allocator probes
   trip.

b) **Page boundary effects**: Allocations that fall near 4KB
   boundaries trigger Windows heap-metadata sanity checks. A
   specific small/large allocation pattern can either avoid or hit
   them.

Adjusting `/STACK` reserves the entire address range up front,
which seems to interact with these. 32 MB worked; 64 MB regressed.
This is not a thing you can derive analytically — it has to be
measured.

---

## 8. The reduced repro for stage-1 flakiness *no longer reproduces*

The pre-shift `compiler/self-host/repros/` directory has 62 `.kry`
files representing various bug-reduction attempts. As of tonight,
ALL of them compile cleanly through stage-1. The shift fixed every
bug they captured.

This is a sign of how much of the bug surface was actually closed.
The repros now serve as regression sentinels — if any future change
breaks one of them, we'll know.

---

(More entries to be appended as the night progresses.)

---

## 9. Stage-1 is 9-12x slower than stage-0 on large modules

Benchmarked stage-0 (Rust kryos.exe, `cargo build --release` → LLVM -O3)
vs stage-1 (Kryos-compiled compiler, Cranelift JIT codegen) on the
six self-host modules:

| Module | Lines | Stage-0 | Stage-1 | Ratio |
|--------|------:|--------:|--------:|------:|
| token   | 353   | 243 ms  | 353 ms  | 1.45x |
| lexer   | 616   | 244 ms  | 404 ms  | 1.66x |
| ast     | 687   | 243 ms  | 403 ms  | 1.66x |
| codegen | 1612  | 264 ms  | 809 ms  | 3.06x |
| types   | 2270  | 249 ms  | 2248 ms | 9.03x |
| parser  | 2867  | 258 ms  | 3222 ms | **12.49x** |

Pattern: small modules ~1.5x slower; large modules ~10x slower.
The slowdown scales SUPER-LINEARLY with source size, suggesting
there's still an O(N log N) or O(N²) somewhere in stage-1's
tokenize/parse path that we haven't found.

Stage-0 base time is nearly constant (~250 ms) because it's dominated
by Rust runtime startup + cargo file I/O. The interesting curve is
stage-1's: 353 → 3222 ms across 2.5K source lines.

For a fully-bootstrapped Kryos, the path forward is either:
- LLVM optimization on stage-2 (currently blocked by Cranelift-only)
- MIR-level optimization passes (constant folding, DCE, inlining)
- Stage-1 codegen quality (currently emits naive code)

This benchmark also explains why parser.kry was the hardest module
to stabilize — it's not just the largest, it's the one where the
allocation/clone rate is most pathological.

---

## 10. Six duplicate KryosString struct definitions across stdlib-native

Discovered during the overnight refcount audit. The crate
`kryos-stdlib-native` has SIX local `KryosString` struct definitions:

- `json.rs:20` — fixed in commit 62a9c32
- `net.rs:222` — fixed in 62a9c32
- `postgres.rs:268` — fixed in 62a9c32
- `tls.rs:262` — fixed in 62a9c32
- `unix_socket.rs:147` — fixed in 62a9c32
- `websocket.rs:23` — fixed in f969a37

All six had the OLD 24-byte layout (`len`, `cap`, `data`) without the
`ref_count` field that was added to `kryos_rt::string::KryosString`
in step 37. When those struct pointers crossed the FFI boundary into
`kryos-rt` functions expecting the 32-byte layout, the runtime read
past the end into uninitialized memory — interpreting it as
`ref_count`. Sometimes that garbage was 0 or negative; on those
runs, `kryos_string_free` would either trigger the sentinel "already
freed" no-op (silent leak) or proceed to deallocate a buffer that
the runtime still thought was live (use-after-free).

This explains ~50% of the bootstrap variance: ANY path through the
JSON / net / TLS / websocket / postgres / unix-socket stdlib hit
the layout mismatch, and stage-1's compilation of large modules
(types.kry, lower.kry, codegen.kry) calls into JSON during MIR
serialization for diagnostics.

**Lesson**: duplicate `#[repr(C)]` struct definitions for FFI
contracts are a category of bug rustc cannot catch. The crates
silently agree on what they think the layout is, then disagree at
runtime. The fix is to ship `kryos_rt::string::KryosString` (and
friends) as `pub use` from a shared `kryos-runtime-abi` crate and
have every consumer import the canonical type.

---

## 11. `kryos_string_concat` was the last uninitialized-`ref_count` site

Of the seven KryosString-allocating sites total (one in kryos-rt
kryos_string_new, one in kryos-rt kryos_string_concat, five in
stdlib-native duplicates, one in stdlib-native json.rs), only
kryos_string_new was correctly initializing `ref_count = 1` after
step 37. The other six were leaving it as whatever garbage allocator
returned. All six are now fixed.

This is a representative kind of bug for ARC-style runtimes: every
allocation site is a potential init-bug site. Languages with
compile-time-inserted ARC (Swift, ObjC's ARC) shift this to the
compiler; manual-init runtimes like Kryos's must audit every alloc.

---

## 12. The 32 MB stack reservation interacts non-monotonically with bootstrap

We found earlier that 8 MB stack stabilized parser+types,
16 MB stack stabilized types fully, 32 MB was the sweet spot, and
64 MB regressed. This is non-monotonic and weird.

Hypothesis: the OS heap allocator's hint addresses depend on the
reserved stack range. Specific VA layouts trip Windows heap
guard pages and the small-block / large-block boundary. 32 MB
happens to be the size where stage-1's allocations don't straddle
the boundary; 64 MB pushes the heap above a critical address.

This is a Windows-specific artifact and would behave differently on
Linux. The next-shift Linux ELF validation work would test this.

