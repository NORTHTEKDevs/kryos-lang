# Next-shift handoff prompt (kryos-lang) — updated 2026-05-21 step 57

Copy the **PROMPT** section below into a fresh Claude Code session
opened in `~/projects/active/kryos-lang`. Everything above PROMPT is
for the human.

---

## State summary (commit `046024a`)

What works (consistent across runs):
- Bootstrap 16/16, examples 8/8, 295 unit tests pass.
- Stage-1 BINARY is bit-identical across builds (reproducible).
- Multi-.obj stage-2 link succeeds.
- Call-clobber-aware register allocation.
- cg_get_local_reg R12 fallback for short-lived temps.
- Deep-clone Operand on every push into call_args.
- Immutable `let name = local_expr` aliases the existing local.
- cg_emit_binop loads operands into scratch R10/R11 (not dest_reg).

User Kryos programs that consistently print correctly via stage-1:
- hello world: `hello from stage-2!`
- cmp_test (`1 < 2`): `yes less`
- args_test no args: `less than 2 args`
- args_test with arg: `got args`
- complex_test no args: full Kryos usage banner (4 lines)
- multi_println: `first / second / third`
- intermediate (helper + multi-println): correct multi-line output
- file_read_test: reads file, prints name + length

## What does not work yet

Stage-2 binary (16-module link) is unreliable due to STAGE-1 OUTPUT
NON-DETERMINISM. Despite stage-1's binary being bit-identical:

```
md5sum (3 runs of same stage-1.exe): SAME hash
md5sum (3 runs of obj output on same input):
  run 1: ba5c142163ecb902252b8f29ad534c61
  run 2: 94ae6d5b2d8ac869d6647a50f1054e94
  run 3: 94ae6d5b2d8ac869d6647a50f1054e94
```

The .obj bytes differ by 4 bytes (out of 429) -- the
register-allocator picks RBX in run 1 and R12 in run 2 for the same
local. Both are valid callee-saved registers; both produce correct
small programs. But the variance amplifies in stage-2 (the 16-module
link of the self-host), where different runs sometimes produce a
working stage-2.exe (prints usage banner) and sometimes produce a
segfaulting one.

This indicates that something inside stage-1's runtime reads
non-deterministically -- most likely `pool.used[REG_RBX]` reads as
either false (pick RBX) or true (skip to R12) across runs of the
same binary on the same input.

## Next-shift goal

Find and fix the stage-1 runtime non-determinism. Once that's gone,
stage-2 should reproduce its first successful run (which DID print
the full usage banner). Then stage-3 fixed-point validation becomes
possible.

## Approach (high-information path)

1. Verify stage-1 output non-determinism is reproducible:
   ```bash
   for i in 1 2 3 4 5; do
     KRYOS_NO_ASLR=1 KRYOS_SKIP_TYPES=1 \
       target/bootstrap/kryos-stage1.exe obj /tmp/t.kry -o /tmp/x$i.obj
     md5sum /tmp/x$i.obj
   done
   ```
   Should see at least 2 distinct hashes.

2. Add diagnostic prints in `self-host/regalloc.kry` `reg_pool_new()`:
   immediately after pushing the 16 falses, print each used[i]'s
   actual numerical value (`println(to_string(used[i]))`). All
   should print 0. If any prints non-zero, that's the bug.

3. Likely culprit: the @copy struct `RegPool { used: [bool] }`
   may have aliasing issues with the bool array. Bool arrays in
   Kryos are stored as i64 slots (per kryos-rt/array.rs ELEM_SIZE).
   When `push(used, false)` is called, the runtime stores 0 in the
   slot. The read `pool.used[r]` should read 0.

4. Maybe relevant: stage-1's @copy struct semantics share-on-clone.
   When `reg_pool_alloc(pool)` is called with `pool` by value, the
   field mutation `pool.used[r] = true` should propagate back. If
   the @copy clone has any bug here, the mutations might persist
   across pool resets between functions, leading to varying initial
   state.

5. Alternative: maybe the non-determinism is in
   `ra_compute_call_positions` returning slightly different lists
   between runs. The `push(positions, pos)` would normally be
   deterministic but could be affected by share-on-clone bugs.

## Hard rules

- Bootstrap stays 16 / 16. Revert and bisect if you drop it.
- Examples stay 8 / 8.
- Don't modify `kryos_string_new`'s ABI in kryos-rt.
- Don't delete the `kryos_field_set` stub.
- Don't remove the R12 fallback in `cg_get_local_reg`.
- Don't remove the call-clobber helpers in regalloc.kry.
- Don't remove the deep-clone-on-push in lower.kry.
- Don't remove the binop scratch-load fix in codegen.kry.
- Use `elif`, not `else if`.
- Preserve build reproducibility.

## Read-first

- `.shift/progress.txt` (last ~200 lines, steps 49-57)
- `compiler/self-host/regalloc.kry` lines 662-770 (reg_pool, callee-saved,
  call_positions helpers)
- `compiler/self-host/regalloc.kry` lines 920-1010 (ra_linear_scan_with_calls)
- `compiler/crates/kryos-rt/src/array.rs` lines 36-160 (array runtime)

---

## PROMPT

```
Continue Kryos self-hosting. State at commit 046024a:

- Bootstrap 16/16, examples 8/8.
- 8 user Kryos programs print correctly via stage-1.
- Stage-1 binary is reproducible (same hash across builds).
- Stage-2 (16-module link) is FLAKY due to stage-1 output
  non-determinism: same binary on same input produces different
  .obj files between runs. Bisected to register-allocator picking
  RBX in one run and R12 in another -- both valid but different.

Reproducer:
  echo 'fn main() { println("hi") }' > /tmp/t.kry
  for i in 1 2 3 4 5; do
    KRYOS_NO_ASLR=1 KRYOS_SKIP_TYPES=1 \
      target/bootstrap/kryos-stage1.exe obj /tmp/t.kry -o /tmp/x$i.obj
    md5sum /tmp/x$i.obj
  done
  # Will show 2+ distinct hashes

Hypothesis: `pool.used[REG_RBX]` in reg_pool_alloc_callee_saved
reads non-deterministically. Either initialized to garbage instead
of false, or mutated unexpectedly somewhere. Looking for stage-1
runtime bug -- possibly in bool array push/read, or @copy struct
share-on-clone semantics for RegPool's inner used array.

Next-shift goal: fix the non-determinism so stage-2 reliably
prints its usage banner. Then attempt stage-3 (stage-2 compiles
the self-host source), and verify stage-2 == stage-3 byte-identical.

Approach:
1. Add diagnostic prints in `reg_pool_new()` after the push loop
   to verify used[0..15] all read as 0. If any read non-zero,
   that's the bug.
2. Investigate the @copy RegPool struct's share-on-clone behavior
   around `pool.used[r] = true` mutations.
3. Once fixed, rebuild stage-1, rebuild stage-2, verify it
   reliably prints usage banner across multiple runs.

Verify after each change:
  cd compiler
  rm -f target/bootstrap/kryos-stage1*
  target/release/kryos.exe build self-host/main.kry \
      -o target/bootstrap/kryos-stage1 --skip-ownership
  cp target/bootstrap/kryos-stage1 target/bootstrap/kryos-stage1.exe
  bash self-host/test_bootstrap.sh    # must stay 16/16
  bash self-host/test_examples.sh     # must stay 8/8

Stage-2 build + test recipe in
.shift/NEXT_SHIFT_PROMPT.md.

Read first:
  .shift/NEXT_SHIFT_PROMPT.md (full context)
  .shift/progress.txt (steps 49-57)
  compiler/self-host/regalloc.kry:662-1010
  compiler/crates/kryos-rt/src/array.rs:36-160

Hard rules:
  - Don't break 16/16 bootstrap or 8/8 examples.
  - Use `elif`, not `else if`.
  - Don't modify kryos_string_new's ABI.
  - Don't delete the kryos_field_set stub or call-clobber helpers.
  - Don't remove the R12 fallback in cg_get_local_reg.
  - Don't remove deep-clone-on-push in lower.kry.
  - Don't remove binop scratch-load in codegen.kry.
  - Preserve build reproducibility.
```
