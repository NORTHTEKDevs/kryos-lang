# Next-shift handoff prompt (kryos-lang) — updated 2026-05-21

Copy the **PROMPT** section below into a fresh Claude Code session opened in
`~/projects/active/kryos-lang`. Everything above PROMPT is for the human.

---

## State summary (as of commit `48e00ad`)

The Kryos self-host bootstrap chain is `stage-0 (Rust) → stage-1 (Kryos)
→ stage-2 (multi-.obj link) → stage-3 (fixed-point)`.

- Bootstrap test passes 16 / 16. Examples pass 8 / 8.  295 unit tests pass.
- Stage-1 builds are bit-identical (reproducible). Step 49.
- `<top-level>` symbols are emitted as STATIC, not EXTERNAL. Step 50.
- Parser-drop bug fixed (no_struct_lit save/restore). Step 51.
- String literals now emit `kryos_string_new(rodata_ptr, len)` calls,
  matching stage-0's ABI. Step 52a (commit `48e00ad`).
- `kryos-build.bat` switched from the legacy minimal C runtime
  (`kryos_runtime.lib`) to the full Rust runtime (`kryos_rt.lib` +
  `kryos_stdlib_native.lib`). Step 52a.
- The multi-.obj link **succeeds**. Produces a 3.6 MB `kryos-stage2.exe`
  from 16 stage-1-emitted .obj files plus the Rust runtime libraries.

## The remaining blocker (NEW root cause as of step 52a)

The previously-suspected "string ABI mismatch" is now fixed. Stage-1
emits proper `kryos_string_new` calls. Disasm of any compiled
`println("hello")` shows the correct sequence:

```
lea rcx, [__rodata_base]   ; arg-0 = rodata ptr
mov edx, 13h               ; arg-1 = 19 (length)
sub rsp, 20h               ; Win64 shadow
call kryos_string_new      ; -> handle in RAX
add rsp, 20h
```

But the **next** call, `kryos_println_str(handle)`, looks like this:

```
sub rsp, 20h
xor rcx, rcx               ; <-- WRONG. Should be `mov rcx, rax`.
call kryos_println_str
```

`rcx` is zeroed instead of receiving the handle that just came back
from `kryos_string_new` in `rax`. So the println receives a null
handle and prints nothing.

This is **not** specific to strings. The same bug reproduces with
`let r = id(42); println(to_string(r))`:

```
mov eax, 2Ah               ; _0 = 42 (regalloc put _0 in RAX)
sub rsp, 20h
call id                    ; <-- BUG: no `mov rcx, rax` before this
```

The MIR is correct (`_0 = const_int 42; _1 = call id(_0)`). The
codegen.kry source for `cg_emit_call` (line 959-971) is also correct:

```kry
let mut i = reg_count - 1
while i >= 0 {
    let target = cg_arg_reg_for(ctx.target_os, i)
    let r = cg_load_operand(ctx, ra, rv.args[i], target)
    if r != target {
        x86_mov_reg_reg(buf, target, r)
    }
    i = i - 1
}
```

But the emitted code shows the loop produced no output. Two
hypotheses, both should be tested:

### Hypothesis A: `cg_get_local_reg` falls through to "not found"

If a temp local has no `LiveInterval` in the regalloc result,
`cg_get_local_reg` hits the trailing `return scratch` and returns the
*scratch* register passed in (which is `target` here). So
`r == target`, the move is skipped, but the actual value is in some
other register entirely.

This would mean the regalloc (`ra_compute_liveness` in
`self-host/regalloc.kry:450`) is dropping short-lifetime temps.

### Hypothesis B: The conditional `if r != target` is mis-compiled

Stage-1 itself was built by stage-0. If stage-0 has a compilation bug
around integer comparison or function-arg passing, the `if r != target`
might evaluate as the wrong branch consistently. This is unlikely
(everything else works), but worth ruling out.

## What the next shift must do

1. Confirm which hypothesis is right by adding diagnostic prints in
   `cg_get_local_reg` (`self-host/codegen.kry:259`). For each lookup,
   print `local_id`, `intervals.len`, whether found, and the returned
   register. Build a fresh stage-1, compile a 5-line hello world,
   and inspect.

2. If hypothesis A: examine `ra_compute_liveness` for the case where
   a temp is defined by `rv_const_int` and used once in the very next
   instruction. The fix is probably either:
     - Extend the def's `lm.uses[lid]` to at least `lm.defs[lid] + 1`
       in `ra_compute_liveness` (so the interval has nonzero length)
     - Or in `ra_scan_instruction` make sure the USE side of a call
       arg properly bumps `lm.uses[lid]`
     - Or in `cg_get_local_reg` make the "not found" fallback panic
       with a useful diagnostic so the bug is surface-visible rather
       than silent

3. If hypothesis B: look at how the Rust `kryos-codegen-cranelift`
   lowers `if a != b { ... }` for `i32` types. Likely fine but worth
   confirming.

## Critical hard rules

- Bootstrap must stay 16 / 16. Revert and bisect any change that drops it.
- Examples must stay 8 / 8.
- Do not touch `kryos_string_new`'s ABI in `kryos-rt/src/string.rs`.
- Do not delete the `kryos_field_set` stub.
- Preserve build reproducibility.

## Read-first list

- `.shift/progress.txt` (last ~100 lines — steps 49-52a)
- `compiler/self-host/codegen.kry` lines 259-276 (cg_get_local_reg) and
  lines 896-985 (cg_emit_call)
- `compiler/self-host/regalloc.kry` lines 60-90 (LiveInterval) and
  450-555 (ra_compute_liveness)
- `compiler/self-host/STAGE2_BLOCKER.md` (historical context)

---

## PROMPT

```
Continue the Kryos self-host bootstrap. State as of commit 48e00ad:

- Bootstrap 16/16, examples 8/8, 295 unit tests pass.
- Multi-.obj stage-2 link produces a 3.6 MB kryos-stage2.exe.
- String-literal ABI in stage-1 is fixed (commits 5b74607 + 48e00ad):
  every "..." emits kryos_string_new(ptr, len) like stage-0 does.

Remaining blocker: stage-1's cg_emit_call (self-host/codegen.kry
line 896) does NOT emit the per-arg move-to-arg-reg. Disasm of
`let r = id(42); println(to_string(r))` shows:

    mov eax, 2Ah        ; _0 = 42, regalloc put _0 in RAX
    sub rsp, 20h        ; cg_emit_call shadow space
    call id             ; <-- no `mov rcx, rax` was emitted

The MIR is correct (_0 = const_int 42; _1 = call id(_0)) and the
codegen source (lines 959-971) reads correctly:

    let r = cg_load_operand(...)
    if r != target {
        x86_mov_reg_reg(buf, target, r)
    }

But the loop produces no output, meaning either:
  (A) cg_get_local_reg (line 259) hits the trailing "Not found —
      use scratch" branch and returns target itself, so r==target.
      That would mean regalloc dropped short-lifetime temp intervals.
  (B) The conditional `if r != target` is being mis-compiled.

Step 1 — confirm root cause. Add diagnostic prints in
cg_get_local_reg to print local_id, intervals.len, found?, and the
returned register. Rebuild stage-1, compile a 5-line hello world
that uses an intermediate let, inspect.

Step 2 — fix:
  - If hypothesis A is right, the fix is in ra_compute_liveness
    in self-host/regalloc.kry. Likely missing a `use` recording
    for call-argument operands, or interval_end being computed
    too tight (interval_end = def_pos with no extension means the
    interval covers ZERO instructions including the use).
  - Make the cg_get_local_reg "Not found" fallback emit a UD2
    (illegal instruction) or panic so future occurrences surface
    immediately instead of silently emitting wrong code.

Verify after each change:
  cd compiler
  rm -f target/bootstrap/kryos-stage1*
  target/release/kryos.exe build self-host/main.kry \
    -o target/bootstrap/kryos-stage1 --skip-ownership
  cp target/bootstrap/kryos-stage1 target/bootstrap/kryos-stage1.exe
  bash self-host/test_bootstrap.sh    # must stay 16/16
  bash self-host/test_examples.sh     # must stay 8/8

End-of-shift goal: rebuilt stage-2.exe prints "Kryos Self-Hosted
Compiler" usage banner when run with no args. That proves stage-1's
codegen now correctly passes args to runtime functions and the
bootstrap chain works end-to-end.

Use shift-engineer discipline: checkpoint every ~60 min, never
regress bootstrap, restore from a known-good tag if a change breaks
things.

Read first:
  .shift/NEXT_SHIFT_PROMPT.md (full context)
  .shift/progress.txt (steps 49-52a)
  compiler/self-host/codegen.kry:259-276 (cg_get_local_reg)
  compiler/self-host/codegen.kry:896-985 (cg_emit_call)
  compiler/self-host/regalloc.kry:450-555 (ra_compute_liveness)

Hard rules:
  - Don't break 16/16 bootstrap or 8/8 examples.
  - Don't modify kryos_string_new's ABI.
  - Don't delete the kryos_field_set stub.
  - Preserve build reproducibility (sorted-iter + /Brepro).
```
