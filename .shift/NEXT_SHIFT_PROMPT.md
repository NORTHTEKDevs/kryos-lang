# Next-shift handoff prompt (kryos-lang) — updated 2026-05-21 step 55

Copy the **PROMPT** section below into a fresh Claude Code session opened
in `~/projects/active/kryos-lang`. Everything above PROMPT is for the
human.

---

## State summary (commit `e2b5852`)

What works:
- Bootstrap 16/16, examples 8/8, 295 unit tests pass.
- Reproducible stage-1 builds.
- Multi-.obj stage-2 link succeeds (3.8 MB kryos-stage2.exe).
- Stage-1 emits proper KryosString handles for string literals.
- `kryos-build.bat` uses the unified Rust runtime.
- Call-clobber-aware register allocation:
  intervals crossing CALLs go to callee-saved (RBX, R12-R15) or spill.
- `cg_get_local_reg` falls back to R12 for short-lived temps with no
  recorded interval.

Programs working through `kryos-build.bat` (stage-1 + Rust runtime):
- hello world: `hello from stage-2!`
- `let r = id(42); println(to_string(r))`: `42`
- `args_test` with no args: `less than 2 args` (sometimes -- see below)
- `intermediate` (helper, multi-println): `start / from helper / end`
- `usage_test` (6 printlns): 3 of 6 lines + duplicates

What does not work yet:
- **Stage-2.exe segfaults at startup.** Caused by RV_USE rename aliasing
  (see Root Cause below).
- Some programs are FLAKY -- running the same binary on the same input
  sometimes prints, sometimes segfaults, sometimes silent-exits.
  Suggests heap-allocation-order dependency in the regalloc (a
  HashMap iteration somewhere, or struct sharing semantics).

## Root cause of remaining stage-2 segfault

For Kryos code like
```kry
let cli_args = args()
let n: i64 = len(cli_args)
if n < 2 { ... }
```

The MIR lower emits a `_2 = use _3` rename (cli_args from args() result,
n from len() result, etc.). At codegen time the regalloc assigns the
SAME register (R12) to multiple locals in this chain because:

1. The intervals of the renamed pair `_2/_3` abut, not overlap, so
   the linear scan "sees" the source as dead by the time the rename's
   destination is allocated.
2. A subsequent `_4 = const_int 2` then takes the same R12 (the source
   is now expired).
3. But the rename's destination is STILL LIVE -- the next instruction
   reads it for the comparison.

Net effect: `cmp r12, r11` ends up comparing `2` with `2` (both copies
of the const), not `len` with `2`. The wrong branch is taken, which
in stage-2 leads to dereferencing into nowhere.

Disasm of stage-2's main() at offsets 0x650D-0x6540 shows the
overlap directly.

## Fix candidates (any one will do)

### Option A: kill the rename in lower.kry (CLEANEST)

In `lower_let_stmt` (line 1627 of `self-host/lower.kry`), when the
let is IMMUTABLE and the value is `op_local(other)`, skip allocating
a fresh MIR local and emitting the redundant `rv_use`. Just bind the
let's NAME to the existing source local:

```kry
if not s.mutable {
    if val.is_local {
        ctx_define_local(ctx, s.name, val.local_id)
        return
    }
}
```

I tried this in step 55 -- it builds clean, bootstrap stays 16/16,
but args_test regresses (prints "got args" instead of "less than 2
args"). That regression suggests the rename is load-bearing somewhere
other than what I expected -- possibly the type tracker or some
codegen path that relies on the local existing. Worth digging into.

### Option B: extend the rename-source's interval through the rename

In `ra_compute_liveness`, when processing an `INST_ASSIGN(dest, RV_USE(src))`,
treat it as `def(dest)` AND extend `src`'s interval to at least
`dest's last use position`. That keeps `src`'s register reserved for
the lifetime of the rename's value, so the regalloc won't reuse it
for another local that overlaps `dest`.

### Option C: force a mov even when src == dest

In `cg_emit_rvalue` for `RV_USE`, instead of `if r != dest_reg { mov }`,
always emit a `mov dest_reg, scratch` via an intermediate scratch
when src is in the same physical register as dest. This forces the
regalloc's intervals to be respected by the actual emitted code.

(Hackier; doesn't address the underlying regalloc bug.)

## Hard rules

- Bootstrap stays 16 / 16. Revert and bisect if you drop it.
- Examples stay 8 / 8.
- Don't modify `kryos_string_new`'s ABI.
- Don't delete the `kryos_field_set` stub.
- Don't remove the R12 fallback in `cg_get_local_reg` until the
  regalloc properly handles every local.
- Don't remove `reg_pool_alloc_callee_saved` or the `ra_compute_call_positions`
  / `ra_interval_crosses_call` helpers.
- Preserve build reproducibility.
- Use `elif`, not `else if` -- the latter triggers a parser-drop
  bug in stage-1 (dropped 22 functions from regalloc.obj on my first
  attempt).

## Read-first

- `.shift/progress.txt` (last ~150 lines, steps 49-55)
- `compiler/self-host/regalloc.kry` lines 671-770 (new call-clobber helpers)
  and 884-1010 (`ra_linear_scan_with_calls`)
- `compiler/self-host/lower.kry` lines 1627-1642 (`lower_let_stmt`,
  Option A target)
- `compiler/self-host/codegen.kry` lines 452-462 (`RV_USE` codegen,
  Option C target)
- Disasm of stage-2's main():
  `dumpbin /DISASM C:/Users/Krist/AppData/Local/Temp/stage2_link/main.obj`
  (offset 0x64EF onward; the offending sequence is at 0x652D-0x6540)

---

## PROMPT

```
Continue Kryos self-hosting work. State at commit e2b5852:

- Bootstrap 16/16, examples 8/8, 295 unit tests pass.
- 5 user programs print correctly through stage-1 + Rust runtime
  (hello, id(42), args_test, intermediate, usage_test partial).
- Multi-.obj stage-2 link succeeds (3.8 MB binary) but segfaults
  on startup.

The remaining stage-2 blocker is RV_USE rename aliasing in the
register allocator. For Kryos code like

    let cli_args = args()
    let n: i64 = len(cli_args)
    if n < 2 { ... }

the regalloc assigns the same register (R12) to multiple locals in
the rename chain, then a later `const_int` takes the same register
because it sees the rename's source as expired -- but the rename's
destination is still live. The compare ends up comparing
const-with-const instead of len-with-const, taking the wrong branch.
Disasm of stage-2/main.obj at offsets 0x650D-0x6540 shows the
overlap.

Fix candidates (in order of cleanness, see .shift/NEXT_SHIFT_PROMPT.md):

A. lower.kry's lower_let_stmt: for immutable lets whose value is a
   local, skip allocating a fresh MIR local and bind the name to the
   source local directly (`ctx_define_local(ctx, s.name, val.local_id)`).
   I tried this; bootstrap stayed 16/16 but args_test regressed. The
   regression suggests the rename is load-bearing somewhere -- track
   that down. This is the cleanest fix.

B. regalloc.kry's ra_compute_liveness: on `INST_ASSIGN(dest, RV_USE(src))`,
   extend `src`'s interval through `dest`'s last use, so `src`'s
   register stays reserved.

C. codegen.kry's RV_USE branch: always emit a mov via an intermediate
   scratch when src and dest map to the same physical register.

End-of-shift goal: rebuilt stage-2.exe prints the "Kryos Self-Hosted
Compiler" usage banner when run with no args.

Verify after each change:
  cd compiler
  rm -f target/bootstrap/kryos-stage1*
  target/release/kryos.exe build self-host/main.kry \
      -o target/bootstrap/kryos-stage1 --skip-ownership
  cp target/bootstrap/kryos-stage1 target/bootstrap/kryos-stage1.exe
  bash self-host/test_bootstrap.sh    # must stay 16/16
  bash self-host/test_examples.sh     # must stay 8/8

Test programs (must keep working):
  /c/Users/Krist/AppData/Local/Temp/stage2_hello.exe
  /c/Users/Krist/AppData/Local/Temp/inreg_call.exe
  /c/Users/Krist/AppData/Local/Temp/args_test.exe   <- must print "less than 2 args"

Stage-2 link recipe:
  rm -f /tmp/stage2_link/*.obj /tmp/stage2_link/*.exe
  for f in token lexer ast parser types mir lower optimize regalloc \
           x86 codegen elf coff linker runtime main; do
    KRYOS_NO_ASLR=1 KRYOS_SKIP_TYPES=1 \
      target/bootstrap/kryos-stage1.exe obj self-host/$f.kry \
      -o /tmp/stage2_link/${f}.obj
  done
  cmd //c "C:\\Users\\Krist\\AppData\\Local\\Temp\\link_stage2_v2.cmd"
  /c/Users/Krist/AppData/Local/Temp/stage2_link/kryos-stage2.exe

Read first:
  .shift/NEXT_SHIFT_PROMPT.md (full context)
  .shift/progress.txt (steps 49-55)
  compiler/self-host/regalloc.kry:671-1010
  compiler/self-host/lower.kry:1627-1642
  compiler/self-host/codegen.kry:452-462

Hard rules:
  - Don't break 16/16 bootstrap or 8/8 examples.
  - Use `elif`, not `else if` (triggers parser-drop bug).
  - Don't modify kryos_string_new's ABI.
  - Don't delete the kryos_field_set stub.
  - Don't remove the R12 fallback in cg_get_local_reg until regalloc
    properly handles every local.
  - Don't remove the call-clobber helpers in regalloc.kry.
  - Preserve build reproducibility (sorted-iter + /Brepro).
```
