# Next-shift handoff prompt (kryos-lang)

Copy the **PROMPT** section below into a new Claude Code session opened in
`~/projects/active/kryos-lang`. Everything above PROMPT is for the human.

---

## State summary (as of 2026-05-21 commit `13e528f`)

The Kryos self-host bootstrap chain is `stage-0 (Rust) → stage-1 (Kryos)
→ stage-2 (multi-.obj link) → stage-3 (fixed-point)`.

- Bootstrap test passes 16 / 16. Examples pass 8 / 8. 295 workspace
  unit tests pass.
- Stage-1 builds are now bit-identical (reproducible). Step 49.
- Stage-1 emits .obj files with correct linkage (Step 50, `<top-level>`
  is STATIC) and complete decl coverage (Step 51, parser-drop bug fixed
  by saving/restoring `no_struct_lit` in `parse_paren_or_tuple`,
  `parse_array_literal`, `parse_arg_list`).
- The multi-.obj link **does succeed**. It produces a 3.6 MB
  `kryos-stage2.exe` from 16 stage-1-emitted .obj files plus
  `kryos_rt.lib` and `kryos_stdlib_native.lib`. Step 51.
- The stage-2 binary **runs and exits cleanly with rc=0 but produces no
  output**. Step 52 diagnosed why: ABI mismatch between stage-0 and
  stage-1 in how string literals are loaded.

## The one specific blocker

In `compiler/self-host/codegen.kry` around line 331, the
`RV_CONST_STRING` branch does:

```
LEA target_reg, [rip + rodata_offset]   ; raw byte pointer
```

and stops there. It returns the raw byte pointer.

But every runtime helper that takes a string (`kryos_println_str`,
`kryos_str_concat`, …) expects a `KryosString` HANDLE — a pointer to a
32-byte struct `{ len, cap, data, ref_count }` allocated by
`kryos_string_new(ptr, len)`. Stage-0 does this correctly; see
`compiler/crates/kryos-codegen-cranelift/src/codegen.rs:5147` for the
reference pattern.

Net effect: every `println("…")` in stage-2 passes a raw byte pointer
that the runtime interprets as a `KryosString*`. `(*ptr).len` reads the
first 8 ASCII bytes, gets a garbage huge length, the runtime
short-circuits without printing, returns cleanly. Hence "links, runs,
silent."

## What the next shift must do

In `compiler/self-host/codegen.kry`, after the LEA at lines ~343-349,
emit a call to `kryos_string_new(rodata_ptr, len)` and use the return
value (in RAX) as the operand instead of the raw pointer.

That call requires:

1. Setup Windows or SysV calling convention (RCX, RDX on Windows;
   RDI, RSI on Linux). Use `target_os` already tracked by `ctx`.
2. Save caller-saved regs that hold live values across the call.
3. Move the .rodata pointer (already in target_reg) into the arg-0
   register, and the literal length into the arg-1 register.
4. Add `kryos_string_new` as an UNDEF external symbol in the .obj so
   the link can resolve it against `kryos_rt.lib`.
5. After CALL, MOV RAX into the original target_reg.

The kryos_string_new ABI is in `compiler/crates/kryos-rt/src/string.rs`.
Signature: `pub extern "C" fn kryos_string_new(data: *const u8, len: i64)
-> *mut KryosString` — returns a pointer that fits in i64.

Expect this to be 40-60 lines of new code in codegen.kry. There is no
existing helper for "emit a runtime call" in codegen.kry; you may want
to add one alongside this change.

Sanity check after each modification:

```
cd compiler
rm -f target/bootstrap/kryos-stage1*
target/release/kryos.exe build self-host/main.kry \
    -o target/bootstrap/kryos-stage1 --skip-ownership
cp target/bootstrap/kryos-stage1 target/bootstrap/kryos-stage1.exe
bash self-host/test_bootstrap.sh    # must still be 16 / 16
bash self-host/test_examples.sh     # must still be 8 / 8
```

End-to-end stage-2 check (after the codegen change is in):

```
mkdir -p /tmp/stage2_link
for f in token lexer ast parser types mir lower optimize regalloc \
         x86 codegen elf coff linker runtime main; do
  KRYOS_NO_ASLR=1 KRYOS_SKIP_TYPES=1 \
    target/bootstrap/kryos-stage1.exe obj self-host/$f.kry \
    -o /tmp/stage2_link/${f}.obj
done
cmd //c "C:\\Users\\Krist\\AppData\\Local\\Temp\\link_stage2_v2.cmd"
/c/Users/Krist/AppData/Local/Temp/stage2_link/kryos-stage2.exe
# Expected: "Kryos Self-Hosted Compiler" usage banner
```

## Read-first list

Before changing anything, read:

- `.shift/progress.txt` (last 80 lines — steps 49-52, mine)
- `compiler/self-host/STAGE2_BLOCKER.md`
- `compiler/self-host/codegen.kry` lines 320-380 (string const path)
- `compiler/crates/kryos-codegen-cranelift/src/codegen.rs` lines
  5147-5174 (reference impl from stage-0)
- `compiler/crates/kryos-rt/src/string.rs:1-80` (KryosString layout +
  `kryos_string_new` signature)
- `compiler/crates/kryos-rt/src/builtins.rs` lines 95-130 (println /
  field_set context)

## Hard rules

- Do NOT break the 16 / 16 bootstrap. If your change drops the rate,
  revert and bisect.
- Do NOT modify the kryos_string_new ABI in the runtime — match it.
- Do NOT delete the `kryos_field_set` stub in `builtins.rs`; it's
  load-bearing for multi-.obj link.
- Stage-1 binaries are now reproducible (sorted-iter + /Brepro);
  preserve that — sha256 of `target/bootstrap/kryos-stage1` should be
  identical across consecutive builds.
- Always emit the runtime call regardless of context (no peephole for
  "string already a handle" — there isn't one in stage-1 yet).

---

## PROMPT

```
Continue the Kryos self-host bootstrap work. State as of commit 13e528f
(branch master, pushed):

- Bootstrap 16/16, examples 8/8, 295 unit tests pass.
- Multi-.obj stage-2 link SUCCEEDS, produces 3.6 MB kryos-stage2.exe.
- Stage-2 runs but exits silently — diagnosed as a string-ABI mismatch.

Specific blocker: in compiler/self-host/codegen.kry around line 331,
the RV_CONST_STRING branch loads a raw .rodata byte pointer but every
runtime helper (kryos_println_str, kryos_str_concat, ...) expects a
KryosString handle. Stage-0's equivalent path in
compiler/crates/kryos-codegen-cranelift/src/codegen.rs:5147 emits
`kryos_string_new(ptr, len)` and uses its return value. Stage-1 must do
the same.

Read these before editing:
  - .shift/NEXT_SHIFT_PROMPT.md (this prompt's source — full context)
  - .shift/progress.txt (steps 49-52)
  - compiler/self-host/codegen.kry lines 320-380
  - compiler/crates/kryos-codegen-cranelift/src/codegen.rs:5147-5174
  - compiler/crates/kryos-rt/src/string.rs:1-80

Then in compiler/self-host/codegen.kry, modify the RV_CONST_STRING
branch to call kryos_string_new(rodata_ptr, len) after the LEA, capture
the returned handle in target_reg, and add kryos_string_new as an UNDEF
external. Use target_os from ctx to pick rcx/rdx (Windows) vs rdi/rsi
(SysV). Save any caller-saved live values; on this code path target_reg
is the only live value across the call so save/restore is minimal.

After each change, verify:
  bash compiler/self-host/test_bootstrap.sh    (must stay 16/16)
  bash compiler/self-host/test_examples.sh     (must stay 8/8)

Final goal of this shift: stage-2.exe prints its usage banner when run
with no args (matching what stage-1 prints). That proves the string ABI
is unified and the bootstrap is one runtime change away from being
truly self-compiling.

Use shift-engineer skill discipline: checkpoint every ~60 min,
preserve no_struct_lit semantics, never regress bootstrap.

Hard rules:
  - Don't break the 16/16 bootstrap. Revert and bisect if you do.
  - Don't modify the kryos_string_new ABI in kryos-rt; match it.
  - Don't delete the kryos_field_set stub in builtins.rs.
  - Preserve build reproducibility (sorted-iter + /Brepro).
```
