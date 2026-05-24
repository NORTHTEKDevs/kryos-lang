# Stage-2 Bootstrap Blocker — Memory O(n^2) [RESOLVED 2026-05-24, step 84]

## STATUS: MEMORY BLOCKER RESOLVED. Gate is now blocker #2 (codegen miscompile).

### RESOLUTION (step 84)

Root cause of the O(n^2): `kryos_array_clone` (array.rs) is a **deep O(len) buffer
copy**, NOT the O(1) refcount-bump its own doc claims. Codegen calls it on `@copy`
struct construction AND field-read of array fields. The self-host stores the growing
`tokens` array in the `@copy` `Lexer` struct and rebuilds `Lexer` per token
(functional update), so each token deep-copied the whole `tokens` array twice
(field-read + construction) -> sizes 1,2,...,30560 -> O(n^2). (The `cap=1025,1026,...`
pairs signature; token array alone ~3.7 GB.) The runtime CANNOT use mutable globals
(no support), so the runtime's "big collections live in module-globals, not struct
fields" design assumption was violated.

**Fix: drop `@copy` from `Lexer`** (lexer.kry) so its array field is shared (refcount)
and grown in place via `push` -> O(n). Verified:
- examples 9/9; field_offsets exact (11/22/33/44).
- sub6 (5151 lines): **13169 MB -> 756 MB live (17x)**.
- full source (20787 lines): **>18 GB OOM -> ~9.7 GB, COMPLETES**, valid 1.2 MB obj.
- stage-2.exe links via `link_stage2.bat` (rt_shim_win.c + kryos_rt.lib) -> 1.22 MB.

Measurement tooling: `crates/kryos-rt/src/memstats.rs`, `KRYOS_MEMSTATS=1` (gated, zero
cost off). Shows arr/str/struct new/free counts, per-category bytes, big-array caps.

### REMAINING residual memory (not blocking; next optimization)

`Parser` is still `@copy` and holds growing arrays -> ~9 GB residual `arr_new`
(max_cap 104295). Same fix applies (drop `@copy` from Parser and other growing-array
structs) IF the parser doesn't rely on `@copy` value snapshots for backtracking
(verify first). Would bring full-source compile to ~1-2 GB.

### THE NEW GATE: blocker #2 — stage-1 miscompiles the large self-host source

`stage-1 ast tiny.kry` works perfectly (Tokens: 19, AST dumped, exit 0). But the
stage-2 it produces crashes (exit 139): `stage-2 ast/obj/check tiny.kry` prints the
command header then segfaults at the first real operation (file_read / tokenize);
`stage-2` with no args prints usage cleanly (exit 0). So stage-2's `main` dispatch
works, but the command handlers crash -> **stage-1's codegen miscompiles the large
self-host source** (scale-dependent: examples 9/9 compile fine via stage-1, but the
20k-line self-host does not). This is the long-standing "blocker #2" (large-`main`
spill/call-sequence-at-scale codegen bug), now cleanly reproducible and no longer
masked by the OOM. Repro: build stage-1, build stage-2 (extlink obj + link_stage2.bat),
then `kryos-stage2.exe ast tiny.kry` (crashes) vs `kryos-stage1.exe ast tiny.kry` (works).

---

## (historical) original O(n^2) analysis below

Two things happened on 2026-05-24 (shift steps 82-83):

1. **FIXED: field-access offset bug** (committed, step 82). `cg_emit_field_access`
   always emitted offset 0, so every multi-field struct field read returned the
   first field (`lx.pos` -> garbage pointer). Now the field index is resolved at
   lowering from `MirModule.struct_defs` via the receiver operand type and carried
   in `rv.field_idx`; codegen emits `field_idx * 8`. Verified: examples 9/9,
   `examples/field_offsets.kry` prints distinct `lit.a..d` and `param.a..d`
   (11/22/33/44) for both struct-literal-local and parameter receivers.

2. **DISCOVERED: the real stage-2 gate is a pre-existing memory blowup**, not the
   field bug. Prior sessions' "stage-2" binaries were built from a **stale/truncated
   `.obj`** — the watchdog had been killing stage-1 mid-emit at the old 3 GB cap, and
   a leftover obj from an earlier run was being linked. A clean rebuild reveals
   stage-1 cannot produce the full-source obj at all.

## Evidence

`stage-1 obj <source>` peak working set (self-measured, polled 1 Hz):

| input                          | lines | `ast` peak | `obj` peak | completes? |
|--------------------------------|-------|-----------|-----------|------------|
| examples/*.kry                 | ~30   | low       | low       | yes (9/9)  |
| runtime+token+lexer (sub3)     | 1581  | fast      | ~OK       | yes        |
| 5000 trivial `let v=N`         | 5003  | 5113 MB   | —         | yes        |
| 5000 trivial, `KRYOS_USE_REALLOC=1` | 5003 | 2839 MB | —     | yes        |
| +ast+parser (sub6)             | 5151  | >13 GB    | >13 GB    | **OOM**    |
| +ast+parser (sub6), realloc    | 5151  | —         | **14.8 GB** | **yes, no crash** |
| half (runtime..lower)          | 11625 | >16 GB    | >16 GB    | **OOM**    |
| full (16 files)                | 20782 | —         | >18 GB    | **OOM**    |
| full (16 files), realloc       | 20782 | —         | ~8.6 GB then **CRASH** | exit 127 |

- ~1 MB/statement on TRIVIAL code; but **REAL code is ~3 MB/line** (sub6 = 5151
  lines completed at 14.8 GB *with realloc*). At O(n^~2) the full 20 782-line source
  would need **>100 GB regardless of the crash** — memory efficiency, not the crash,
  is the primary wall. No env var or runtime tweak fixes this; the retain/release
  leak (arrays never reaching rc=0) must be fixed so memory is reclaimed during the
  compile.
- realloc is **safe through sub6** (5 k lines, frontend files: runtime/token/lexer/
  ast/parser) — completes cleanly, "Type check: OK, MIR functions 398, Object file
  written". It only **crashes on the full source**, so the remaining heap corruption
  is introduced by a later file (types/mir/lower/optimize/regalloc/x86/codegen/elf/
  coff/linker/main). That narrows the OOB-write hunt to those files.
- **Confirmed pre-existing**: a stage-1 built from the step-81 source (before the
  field fix) OOMs identically (16385 MB on half). The field fix is NOT the cause.

## Root cause

The runtime (`crates/kryos-rt`) **deliberately leaks** to stay alive under heap
corruption, and the full self-host source still corrupts the heap:

- `array.rs` `kryos_array_push` grow path defaults to **alloc-copy-leak** (the old
  data buffer is never freed; line ~122 "H26: default to alloc-copy-leak grow path").
  `KRYOS_USE_REALLOC=1` switches to a realloc (leak-free) grow path.
- `kryos_array_free` DOES free by default (only ~40 B header leaked);
  `KRYOS_LEAK_ON_ZERO` is opt-in/off. So frees are not the primary leak.
- The deeper issue (`array.rs:360-363`): codegen retain/release is **not balanced**
  ("the full fix requires auditing codegen to emit retain at every array pointer
  copy ... tracked as separate next-shift work"). Over-retained arrays never reach
  rc=0, so even the working free path can't reclaim them.

Why we can't just flip the leak off:
- `KRYOS_USE_REALLOC=1` cuts trivial-5k from 5113->2839 MB and does NOT crash on
  trivial code, **but CRASHES on the full self-host source** (exit 127, only the
  header prints, ~8.6 GB). HeapReAlloc probes adjacent heap-block headers; a
  remaining out-of-bounds write somewhere in the self-host's generated code corrupts
  heap metadata, and realloc/free are the canaries that surface it. The leak path
  masks it by never touching freed/old blocks.

So the true blocker is **remaining heap corruption** in stage-1's compilation of the
full source (an OOB store in some codegen path), plus the unbalanced retain/release
that forces the leak. The field-access fix removed one source of garbage pointers
but not all corruption.

## Candidate fixes (next shift, in order of safety/value)

1. **Find the remaining OOB write.** Build `kryos_rt` with debug assertions or a
   guard allocator; run stage-1 on sub6 with `KRYOS_USE_REALLOC=1` and catch the
   first corrupt-header panic (`kryos_array_push` already panics on corrupt headers —
   widen that check to fire earlier, log the allocation that overflows). Likely an
   uncoerced i64->i32 store or an array/struct write past its length, similar to the
   step-79/80 fixes.
2. **Balance codegen retain/release** (stage-0, `crates/kryos-codegen-llvm` +
   `kryos-mir` drop insertion) so arrays reach rc=0 and free. This is the
   "separate next-shift work" the runtime comments defer. Delicate; gate every step
   on examples 9/9 + bootstrap 16/16 to catch use-after-free.
3. **`free-on-grow when ref_count<=1`** in `kryos_array_push` (alloc+copy+dealloc-old,
   not realloc) — only safe once (1)/(2) land; risks use-after-free if rc is
   under-counted.

## SAFETY — this project has OOM'd the machine before

- Always run `scratch/kryos-watchdog.ps1 -CapMB <N>` before any full-source stage-1
  run. With ~33 GB free, `-CapMB 16384` is safe headroom. The leak path needs
  >18 GB for the full obj and was never observed to complete — do NOT raise the cap
  chasing a completion; fix the leak instead.
- `KryosLeakGuard` service (auto-kills kryos-stage*.exe >2 GB) should stay Running.
  `KryosTwin` must stay Stopped (it auto-runs the 35 GB self-host build).

## How to reproduce

```bash
cd compiler
# build stage-1 (safe, stage-0 is the Rust compiler)
KRYOS_NO_ASLR=1 ./target/release/kryos.exe build self-host/main.kry \
    -o target/bootstrap/kryos-stage1 --skip-ownership
cp -f target/bootstrap/kryos-stage1 target/bootstrap/kryos-stage1.exe
# reproduce the OOM (under watchdog!) — sub6 is the smallest reliable repro
cat self-host/{runtime,token,lexer,ast,parser}.kry \
  | grep -vE '^use (token|lexer|ast|parser|types|mir|lower|optimize|regalloc|x86|codegen|elf|coff|linker|runtime|main)$' > /tmp/sub6.kry
KRYOS_SKIP_TYPES=1 KRYOS_NO_ASLR=1 ./target/bootstrap/kryos-stage1 obj /tmp/sub6.kry -o /tmp/sub6.obj
```
