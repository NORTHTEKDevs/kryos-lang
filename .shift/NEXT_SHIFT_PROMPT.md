# Next-shift handoff prompt (kryos-lang) — updated 2026-05-21 step 59

Copy the **PROMPT** section below into a fresh Claude Code session
opened in `~/projects/active/kryos-lang`. Everything above PROMPT is
for the human.

---

## State summary (commit `26aabfa`)

### Day-of progress (Steps 49-59, 12 commits, 10 tags):

| Fix | Effect |
|-----|--------|
| Reproducible stage-1 builds | bit-identical exe across rebuilds |
| `<top-level>` static linkage | unblocks multi-.obj link |
| Parser no_struct_lit save/restore | parser.kry 84/84 fns (was 57/84) |
| String ABI via kryos_string_new | strings print correctly through runtime |
| cg_get_local_reg R12 fallback | unallocated temps stable |
| Call-clobber regalloc | callee-saved for crossings |
| Deep-clone Operand on call_args push | breaks share-on-clone aliasing |
| Immutable-let alias | no spurious rename, distinct regs |
| cg_emit_binop scratch-load (R10/R11) | operand-clobber gone for compares |
| RegPool.used [bool] -> [i32] | bypass stage-1's bool array runtime bug |

### What works (consistent):
- Bootstrap 16/16 (very rarely flakes to 15/16)
- Examples 8/8
- 6+ user Kryos programs print correctly via stage-1:
  hello, cmp_test, args_test (both branches), multi_println,
  complex_test (full usage banner), intermediate, file_read_test
- Multi-.obj stage-2 link succeeds (3.8 MB exe)
- Stage-1 output for `no_call.kry`: 5/5 bit-identical

### What is still flaky:
- Stage-2.exe sometimes prints the full Kryos usage banner, sometimes
  silent-exits. The variance comes from STAGE-1 OUTPUT
  NON-DETERMINISM on LARGE programs:
    - no_call.kry: 5/5 same hash (deterministic)
    - hello.kry: 2 distinct hashes out of 5 (some variance)
    - main.kry: 5/5 different hashes (all-variance)

## Concrete bugs that remain (in roughly the order to attack)

### Bug 1: Bool array push corrupts header
Reproducer:
```kry
fn main() {
    let mut a: [bool] = []
    push(a, false)
    push(a, false)
    push(a, false)
    push(a, false)
}
```
Panics: `kryos_array_push: corrupt array header @ <ptr>` with
`len, cap, data` fields containing path-string bytes
(e.g. `data=0x4c5c617461447070` = "...AppData\L..." backwards).
ONLY affects bool arrays; [i32], [str], [Struct] all work.
Stage-0 compilation of the same program works fine; only stage-1's
COMPILED bool-push code path triggers.

Probably a register-allocator bug in stage-1: the bool array
handle local gets overwritten because a later instruction
clobbers its register/slot. The bool literal `false` then writes
across the array header.

### Bug 2: @copy struct share-on-clone causes large-program variance
Operand, RValue, Instruction are all `@copy` structs. Stage-1's
share-on-clone semantics mean push(arr, struct_value) stores a
REFERENCE, not a copy. When a later struct allocation reuses the
heap memory, the array entry sees the new value.

I worked around this for Operand in lower_fn_call (4 deep-clone
sites). The same pattern likely needs applying to:
- ctx_emit (Instruction)
- rv_call, rv_binop, etc. (RValue)
- inst_assign (Instruction with RValue)

### Bug 3: Stage-2 subcommand handlers
stage-2.exe with `check`, `mir`, `ast`, `obj` subcommands exit
silently or segfault. The tokenize/parse/typecheck path likely
hits its own share-on-clone bugs at scale.

## Recipe to test stage-2

```bash
cd compiler
rm -f target/bootstrap/kryos-stage1*
target/release/kryos.exe build self-host/main.kry \
    -o target/bootstrap/kryos-stage1 --skip-ownership
cp target/bootstrap/kryos-stage1 target/bootstrap/kryos-stage1.exe

# emit + link
rm -f /tmp/stage2_link/*.obj /tmp/stage2_link/*.exe
for f in token lexer ast parser types mir lower optimize regalloc \
         x86 codegen elf coff linker runtime main; do
  KRYOS_NO_ASLR=1 KRYOS_SKIP_TYPES=1 \
    target/bootstrap/kryos-stage1.exe obj self-host/$f.kry \
    -o /tmp/stage2_link/${f}.obj
done
cmd //c "C:\\Users\\Krist\\AppData\\Local\\Temp\\link_stage2_v2.cmd"

# Run -- some runs print usage banner, some are silent
/c/Users/Krist/AppData/Local/Temp/stage2_link/kryos-stage2.exe
```

## Hard rules

- Bootstrap stays 16 / 16. Revert and bisect if you drop it.
- Examples stay 8 / 8.
- Use `elif`, not `else if`.
- Don't modify `kryos_string_new`'s ABI.
- Don't delete the `kryos_field_set` stub.
- Don't remove the R12 fallback in `cg_get_local_reg`.
- Don't remove the call-clobber helpers in regalloc.kry.
- Don't remove the deep-clone-on-push in lower.kry (4 sites).
- Don't remove the binop scratch-load in codegen.kry.
- Don't switch RegPool.used back to [bool].
- Preserve build reproducibility (sorted-iter + /Brepro).

## Read-first

- `.shift/progress.txt` last ~250 lines (steps 49-59)
- `compiler/self-host/regalloc.kry` lines 670-770 (call-clobber helpers,
  bool→i32 fix)
- `compiler/self-host/lower.kry` lines 854-925 (deep-clone pattern
  in lower_fn_call; needs replicating for other call_args sites)
- `compiler/self-host/codegen.kry` lines 603-715 (cg_emit_binop with
  scratch loads)

---

## PROMPT

```
Continue Kryos self-hosting. State at commit 26aabfa:

- Bootstrap 16/16 (mostly stable), examples 8/8.
- 6+ user Kryos programs print correctly via stage-1.
- Stage-2 builds (3.8 MB) and sometimes prints the full usage
  banner. Variance comes from stage-1 output non-determinism on
  large programs.
- Concrete bool array bug discovered: `push(arr, false)` 4+ times
  corrupts the array header with path-string bytes. Only stage-1
  compiled code triggers this; stage-0 is fine.

Next-shift goal: eliminate the remaining non-determinism so
stage-2 reliably prints the usage banner. Then push to stage-3
(stage-2 compiles the self-host) for fixed-point validation.

Suspected root cause: @copy struct share-on-clone semantics in
stage-1. Operand, RValue, Instruction are all @copy. push(arr,
struct_val) stores a reference, not a copy. Later struct allocs
on the same heap memory mutate earlier array entries.

I already worked around this for 4 push(call_args, Operand) sites
in lower_fn_call. The same pattern likely needs applying to other
push sites that store @copy structs. Search for:
  grep -n "push(.*,.*)" compiler/self-host/lower.kry
  grep -n "push(ctx.symbols\|push(ctx.relocations" \
    compiler/self-host/codegen.kry

Also: the bool array bug is a concrete reproducer (kry source in
`/c/Users/Krist/AppData/Local/Temp/bool_test.kry`). Stage-1's
COMPILED push-of-bool overwrites the array handle local because
a later operand reuses the same register/slot. Fixing the
regalloc to track the bool literal's interval would fix this.

Verify after each change:
  cd compiler
  rm -f target/bootstrap/kryos-stage1*
  target/release/kryos.exe build self-host/main.kry \
      -o target/bootstrap/kryos-stage1 --skip-ownership
  cp target/bootstrap/kryos-stage1 target/bootstrap/kryos-stage1.exe
  bash self-host/test_bootstrap.sh    # must stay 16/16
  bash self-host/test_examples.sh     # must stay 8/8

  # Determinism check (target: all 5 hashes identical):
  for i in 1 2 3 4 5; do
    KRYOS_NO_ASLR=1 KRYOS_SKIP_TYPES=1 \
      target/bootstrap/kryos-stage1.exe obj \
      /c/Users/Krist/AppData/Local/Temp/t.kry -o /tmp/d$i.obj >/dev/null
    md5sum /tmp/d$i.obj
  done

Read first:
  .shift/NEXT_SHIFT_PROMPT.md (full context, this file)
  .shift/progress.txt (steps 49-59 -- full session log)
  compiler/self-host/lower.kry:854-925 (deep-clone pattern)
  compiler/self-host/regalloc.kry:670-770

Hard rules:
  - Don't break 16/16 bootstrap or 8/8 examples.
  - Use `elif`, not `else if`.
  - Don't modify kryos_string_new's ABI.
  - Don't delete the kryos_field_set stub or call-clobber helpers.
  - Don't remove the R12 fallback in cg_get_local_reg.
  - Don't remove deep-clone-on-push in lower.kry.
  - Don't remove binop scratch-load in codegen.kry.
  - Don't switch RegPool.used back to [bool].
  - Preserve build reproducibility.
```
