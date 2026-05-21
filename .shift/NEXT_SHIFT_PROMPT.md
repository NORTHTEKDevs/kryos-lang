# Next-shift handoff prompt (kryos-lang) — updated 2026-05-21 step 54

Copy the **PROMPT** section below into a fresh Claude Code session opened in
`~/projects/active/kryos-lang`. Everything above PROMPT is for the human.

---

## State summary (commit `ce2ea14`)

The Kryos self-host bootstrap chain is `stage-0 (Rust) → stage-1 (Kryos)
→ stage-2 (multi-.obj link) → stage-3 (fixed-point)`.

What works today:
- Bootstrap test passes **16 / 16**. Examples pass **8 / 8**.
- 295 workspace unit tests pass.
- Stage-1 builds are bit-identical (sorted iteration + /Brepro).
- Multi-.obj stage-2 link succeeds, producing a 3.6 MB kryos-stage2.exe.
- Stage-1 emits proper KryosString handles for string literals via
  `kryos_string_new(ptr, len)`.
- `kryos-build.bat` links against the full Rust runtime (kryos_rt.lib).
- `cg_get_local_reg` has an R12 fallback for short-lifetime temps.
- **A hello-world `.kry` compiled by stage-1 and linked against the
  full runtime prints "hello from stage-2!"** — the full pipeline
  works for simple programs.
- `let r = id(42); println(to_string(r))` correctly prints `42`.

What does not work:
- `stage2.exe` segfaults at startup. Reproducer:
  ```kry
  fn main() {
      let cli_args = args()
      let n: i64 = len(cli_args)
      if n < 2 {
          println("less than 2 args")
          return
      }
      println("got args")
  }
  ```
  When run with no args this segfaults. With one arg it exits cleanly
  but prints nothing.

## The remaining blocker (commit `ce2ea14`, step 54)

Disasm of the failure shows the regalloc is putting values in
*caller-saved* registers (RBX, RCX, RDX, etc.) that get clobbered by
intervening calls. Example sequence from a `println("...")` inside
the args-handling branch:

```
mov rbx, r12          ; spill args() result into rbx
sub rsp, 0x20
mov rcx, rbx          ; load arg = cli_args for len()
call len
...
sub rsp, 0x20
call kryos_string_new ; -> handle in rax. Clobbers caller-saved regs.
mov rdx, rax          ; capture handle in rdx ... but rdx is caller-saved!
sub rsp, 0x20         ; shadow for kryos_println_str
mov rcx, rbx          ; <-- arg for println comes from rbx (wrong value!)
call kryos_println_str
```

The interval holding the string handle was assigned RDX (caller-saved),
and the next instruction is another call. The handle survives that
call by accident only when no Rust runtime helper writes to RDX. In
the disasm above `println` ends up getting rbx (which is the cli_args
array, not the string handle).

The bug is **acknowledged in the source** at `compiler/self-host/regalloc.kry:187-193`:

```kry
// Callee-saved first so values survive function calls. The
// overlap-detection in this linear-scan implementation does not
// model call-clobber correctly: when a local interval crosses a
// CALL, it should be barred from any caller-saved register.
// Until that is fixed, preferring callee-saved registers makes
// every interval safe at the cost of a few extra prologue
// push/pop pairs.
```

The "prefer callee-saved" workaround helps but runs out — there are
only 5 callee-saved regs (RBX, R12-R15), and any real function uses
more live intervals than that.

## What the next shift must do

Implement call-clobber-aware register allocation in
`compiler/self-host/regalloc.kry`.

The simplest correct fix:

1. Build a sorted list of CALL positions while walking instructions
   in `ra_compute_liveness`. Store on the LivenessMap.

2. In `ra_linear_scan`, when about to assign a register to interval I:
   - Check whether any CALL position falls in `(I.start, I.end)`.
   - If yes, restrict the candidate set to callee-saved registers only.
   - If no callee-saved is free, spill I instead of stealing a
     caller-saved.

3. A `ra_active_insert` and `ra_spill_at` may need parallel updates
   so the call-crossing flag follows the interval.

Steps to verify after each commit:

```
cd compiler
rm -f target/bootstrap/kryos-stage1*
target/release/kryos.exe build self-host/main.kry \
    -o target/bootstrap/kryos-stage1 --skip-ownership
cp target/bootstrap/kryos-stage1 target/bootstrap/kryos-stage1.exe
bash self-host/test_bootstrap.sh           # must stay 16/16
bash self-host/test_examples.sh            # must stay 8/8

# args reproducer
powershell.exe -Command "Set-Location 'C:\Users\Krist\projects\active\kryos-lang\compiler'; .\self-host\kryos-build.bat 'C:\Users\Krist\AppData\Local\Temp\args_test.kry' 'C:\Users\Krist\AppData\Local\Temp\args_test.exe'"
/c/Users/Krist/AppData/Local/Temp/args_test.exe          # SHOULD print "less than 2 args" -- currently segfaults
/c/Users/Krist/AppData/Local/Temp/args_test.exe foo      # SHOULD print "got args"
```

End-of-shift goal:
- `args_test` prints expected output (both branches)
- Rebuilt stage-2.exe prints the usage banner when run with no args

## Hard rules

- Bootstrap stays 16 / 16. Revert and bisect if you drop it.
- Examples stay 8 / 8.
- Don't modify `kryos_string_new`'s ABI in kryos-rt.
- Don't delete the `kryos_field_set` stub.
- Don't remove the R12 fallback in `cg_get_local_reg` until the
  regalloc properly handles every local.
- Preserve build reproducibility.

## Read-first

- `.shift/progress.txt` (last ~100 lines — steps 49-54, fresh log)
- `compiler/self-host/regalloc.kry` lines 187-200 (acknowledged bug)
  and 820-915 (ra_linear_scan)
- `compiler/self-host/codegen.kry` lines 259-280 (cg_get_local_reg R12
  fallback) and lines 896-985 (cg_emit_call)

---

## PROMPT

```
Continue Kryos self-host bootstrap. State at commit ce2ea14:

- Bootstrap 16/16, examples 8/8, 295 unit tests pass.
- Hello world compiled by stage-1 and linked against the Rust runtime
  prints "hello from stage-2!". `let r = id(42); println(to_string(r))`
  prints "42".
- Stage-2 binary LINKS (3.6 MB) but segfaults at startup on the
  no-args path of main.kry.

The remaining blocker is a known regalloc bug acknowledged in the
source at compiler/self-host/regalloc.kry:187-193. Intervals that
cross a CALL get assigned caller-saved registers (RBX, RCX, RDX,
RSI, RDI, R8-R11) and the call clobbers them. The "prefer callee-
saved" workaround in ra_allocatable_regs helps but runs out.

Reproducer (currently segfaults on the no-arg invocation):

  fn main() {
      let cli_args = args()
      let n: i64 = len(cli_args)
      if n < 2 {
          println("less than 2 args")
          return
      }
      println("got args")
  }

The fix is call-clobber-aware allocation in ra_linear_scan:
  1. While walking instructions in ra_compute_liveness, build a
     sorted list of CALL positions on the LivenessMap.
  2. In ra_linear_scan, when allocating a register for interval I,
     check whether any CALL position falls in (I.start, I.end).
     If yes, restrict candidates to callee-saved regs only. If no
     callee-saved is free, spill.

Verify after each change:
  cd compiler
  rm -f target/bootstrap/kryos-stage1*
  target/release/kryos.exe build self-host/main.kry \
    -o target/bootstrap/kryos-stage1 --skip-ownership
  cp target/bootstrap/kryos-stage1 target/bootstrap/kryos-stage1.exe
  bash self-host/test_bootstrap.sh    # must stay 16/16
  bash self-host/test_examples.sh     # must stay 8/8

  # The two args_test invocations should print, not segfault:
  powershell.exe -Command "Set-Location 'C:\Users\Krist\projects\active\kryos-lang\compiler'; .\self-host\kryos-build.bat 'C:\Users\Krist\AppData\Local\Temp\args_test.kry' 'C:\Users\Krist\AppData\Local\Temp\args_test.exe'"
  /c/Users/Krist/AppData/Local/Temp/args_test.exe
  /c/Users/Krist/AppData/Local/Temp/args_test.exe foo

End-of-shift goal: rebuilt stage-2.exe prints "Kryos Self-Hosted
Compiler" usage banner when run with no args.

Read first:
  .shift/NEXT_SHIFT_PROMPT.md (full context)
  .shift/progress.txt (steps 49-54)
  compiler/self-host/regalloc.kry:187-200 + 820-915
  compiler/self-host/codegen.kry:259-280 + 896-985

Hard rules:
  - Don't break 16/16 bootstrap or 8/8 examples.
  - Don't modify kryos_string_new's ABI.
  - Don't delete the kryos_field_set stub.
  - Don't remove the R12 fallback in cg_get_local_reg until regalloc
    properly handles every local.
  - Preserve build reproducibility.
```
