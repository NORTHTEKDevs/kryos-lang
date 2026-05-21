# Next-shift handoff prompt (kryos-lang) — updated 2026-05-21 step 56

Copy the **PROMPT** section below into a fresh Claude Code session opened
in `~/projects/active/kryos-lang`. Everything above PROMPT is for the
human.

---

## State summary (commit `b6ac406`) — STAGE-2 USAGE BANNER PRINTS

What works:
- Bootstrap 16/16, examples 8/8, 295 unit tests pass.
- Reproducible stage-1 builds (bit-identical hashes).
- Stage-1 emits proper KryosString handles for string literals.
- Multi-.obj stage-2 link succeeds (3.8 MB kryos-stage2.exe).
- Call-clobber-aware register allocation (callee-saved for crossings).
- cg_get_local_reg R12 fallback for short-lived temps.
- Deep-clone on every push(call_args, ...) in lower.kry.
- Immutable `let name = local_expr` aliases rather than emitting
  redundant rv_use rename.
- cg_emit_binop loads operands into scratch R10/R11 (not dest_reg),
  preventing operand-clobber when left/right and dest overlap.

**STAGE-2 (16-module link) now correctly prints the Kryos
Self-Hosted Compiler usage banner when run with no args:**

```
Kryos Self-Hosted Compiler

Usage:
  kryos-sh compile <input.kry> [-o output]
  kryos-sh check <input.kry>
  kryos-sh ast <input.kry>
```

Many user Kryos programs also print correctly through stage-1 +
Rust runtime (hello world, cmp_test, args_test both branches,
intermediate, complex_test full banner, multi_println).

## What does not work yet

Stage-2 with subcommand args:
- `kryos-stage2.exe check <file>` -- segfaults during type-check
- `kryos-stage2.exe mir <file>` -- exits 0, no output
- `kryos-stage2.exe ast <file>` -- exits 0, no output
- `kryos-stage2.exe obj <file> -o ...` -- exits 0, no .obj produced

The dispatcher in main.kry's `main()` is working (recognizes no-args
and branches correctly). The subcommand handlers (compile_file,
check_file, emit_object, dump_ast) likely hit their own bugs:
- The tokenize/parse code probably has more share-on-clone issues
  (the AST nodes are full of @copy structs with similar push patterns).
- Or further register-overlap bugs in larger functions.

## Next-shift goal

Make stage-2.exe successfully process a tiny .kry file end-to-end:

```bash
echo 'fn main() { println("hi from stage 2!") }' > /tmp/tiny.kry
/c/Users/Krist/AppData/Local/Temp/stage2_link/kryos-stage2.exe obj /tmp/tiny.kry -o /tmp/tiny.obj
# Expected: tiny.obj exists, no segfault
```

If that works, stage-3 (stage-2 compiles the self-host itself) is
the next milestone. Stage-3 == stage-2 byte-identical would close
the bootstrap loop.

## Approach

1. Reproduce the `check` segfault with the smallest input.
   ```
   echo 'fn main() { println("hi") }' > /tmp/t.kry
   kryos-stage2.exe check /tmp/t.kry
   ```

2. Find what input position segfaults via a debugger or by adding
   `println` debug output in the self-host source.

3. The check command's body (in main.kry) calls tokenize + parser +
   tc. Whichever first segfaults is the target. Look for the same
   patterns we already fixed:
   - push(arr, struct_value) where struct_value should be deep-cloned
   - Binary operations where both operands need scratch loading
   - Immutable lets that could be aliased

4. Apply the same kinds of fixes (deep-clone struct on push, scratch
   regs in binops, alias-on-immutable-let). The patterns are now
   well-understood.

## Critical hard rules

- Bootstrap stays 16 / 16. Revert and bisect if you drop it.
- Examples stay 8 / 8.
- Don't modify `kryos_string_new`'s ABI in kryos-rt.
- Don't delete the `kryos_field_set` stub.
- Don't remove the R12 fallback in `cg_get_local_reg` until the
  regalloc properly handles every local.
- Don't remove the call-clobber helpers in regalloc.kry.
- Use `elif`, not `else if` -- the latter triggers a parser-drop
  bug in stage-1.
- Preserve build reproducibility.

## Read-first

- `.shift/progress.txt` (last ~200 lines, steps 49-56)
- `compiler/self-host/lower.kry` lines 854-925 (lower_fn_call with
  arg-clone pattern) and 1627-1660 (lower_let_stmt with alias)
- `compiler/self-host/codegen.kry` lines 603-715 (cg_emit_binop with
  scratch-register pattern)
- `compiler/self-host/regalloc.kry` lines 671-770 (call-clobber
  helpers) and 884-1010 (ra_linear_scan_with_calls)

---

## PROMPT

```
Continue Kryos self-hosting. State at commit b6ac406:

- Bootstrap 16/16, examples 8/8.
- STAGE-2 (16-module link of self-host) prints "Kryos Self-Hosted
  Compiler" usage banner correctly when run with no args.
- 6+ user Kryos programs print expected output through stage-1.

Remaining gap to full self-hosting:
- `stage2.exe check <file>` segfaults during type-check.
- `stage2.exe mir <file>`, `ast <file>`, `obj <file>` exit 0
  silently (no output, no .obj produced).

The dispatcher works (no-args -> usage banner). The subcommand
handlers (compile_file, check_file, emit_object) hit their own
bugs -- probably the same patterns we already fixed (share-on-clone
struct push, regalloc operand-clobber, immutable-let rename) but
in tokenize/parse/typecheck/lower.

Next-shift goal: make stage-2 successfully emit a .obj file for a
trivial input:
  echo 'fn main() { println("hi") }' > /tmp/t.kry
  /c/Users/Krist/AppData/Local/Temp/stage2_link/kryos-stage2.exe obj /tmp/t.kry -o /tmp/t.obj
  # Expected: t.obj exists, no segfault, no missing output.

Once that works, stage-3 fixed-point validation closes the loop.

Approach:
1. Reproduce the check segfault with smallest input.
2. Add `println` debug output in the self-host source to localize
   the segfault to a specific function (tokenize / parser / tc).
3. Apply the same fix patterns we already used:
   - Deep-clone struct values before push into arrays
   - Load binop operands into R10/R11, not dest_reg
   - Alias immutable lets to existing locals
   - Detect call-crossing intervals in regalloc

Verify after each change:
  cd compiler
  rm -f target/bootstrap/kryos-stage1*
  target/release/kryos.exe build self-host/main.kry \
      -o target/bootstrap/kryos-stage1 --skip-ownership
  cp target/bootstrap/kryos-stage1 target/bootstrap/kryos-stage1.exe
  bash self-host/test_bootstrap.sh    # must stay 16/16
  bash self-host/test_examples.sh     # must stay 8/8

Then rebuild stage-2:
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
  .shift/progress.txt (steps 49-56)
  compiler/self-host/lower.kry:854-925 + 1627-1660
  compiler/self-host/codegen.kry:603-715

Hard rules:
  - Don't break 16/16 bootstrap or 8/8 examples.
  - Use `elif`, not `else if`.
  - Don't modify kryos_string_new's ABI.
  - Don't delete the kryos_field_set stub or call-clobber helpers.
  - Don't remove the R12 fallback in cg_get_local_reg.
  - Preserve build reproducibility.
```
