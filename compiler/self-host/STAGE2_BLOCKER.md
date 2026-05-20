# Stage-2 Bootstrap Blocker — Root Cause

## Symptom

When stage-1 compiles the self-host source files, larger files
(ast.kry, lexer.kry, parser.kry, etc.) segfault non-deterministically
during the lower/codegen pass. Smaller files (token.kry) sometimes
succeed, sometimes fail.

## Root cause: push of struct values

Self-host source pushes whole struct values into typed arrays:

```kryos
fn lex_emit(lex: Lexer, kind: i32, ...) -> Lexer {
    let tok = Token { kind: kind, text: text, ... }
    let mut tokens = lex.tokens
    push(tokens, tok)            // <-- struct value into [Token] array
    return Lexer { src: lex.src, pos: lex.pos, tokens: tokens }
}
```

The Kryos runtime (`kryos-rt/array.rs`) stores array elements as
`i64`-sized slots (8 bytes each). Pushing a 40-byte struct
necessarily loses data.

## How both backends fail

### LLVM (--release)

`clang` compilation of stage-1 fails with:

```
%t454 = extractvalue %Token %_5, 0
call void @kryos_array_push(ptr %t453, i64 %t454)
                                       ^^^
defined with type 'i32' but expected 'i64'
```

The LLVM emit extracts field 0 of the struct (an i32) and passes it
as the i64 value to `kryos_array_push`. clang's type checker
rejects the mismatched signatures. So stage-1 cannot even be built
in --release mode.

### Cranelift (default)

Cranelift's IR is loosely typed. It accepts the call with an i32
value where i64 is expected. The i32 occupies the low 32 bits of
the register; the high 32 bits are uninitialized.

When the Rust runtime's `kryos_array_push(handle: i64, val: i64)`
reads its arg, the i64 it sees includes garbage in the high bits.
That garbage IS the source of the non-determinism: each call has
fresh stack/register junk that may or may not pass internal sanity
checks (e.g. when the runtime treats the i64 as a pointer or index
into a smaller table).

## Why simple programs work

Programs that only push primitive types (i64, str-as-i64-handle)
work fine because the value already IS i64. The bug only triggers
when struct values are pushed.

## Fix paths (in order of effort)

1. **Source-level fix.** Rewrite self-host source to push the
   struct's handle (heap-allocated pointer) instead of the value:
   ```kryos
   let tok_ptr = box_token(kind, text, start, end)
   push(tokens, tok_ptr)
   ```
   Requires changing every `push(arr, struct_value)` site (~dozens
   across lexer.kry, parser.kry, ast.kry, etc.).

2. **Runtime fix.** Add a multi-word array runtime that stores
   elements as variable-sized records. Bigger architectural change.

3. **Cranelift codegen fix.** Treat struct fields as their declared
   LLVM type and explicitly widen at call boundaries. Need to track
   field types through the MIR-to-Cranelift translation. Roughly a
   day of work in `compiler/crates/kryos-codegen-cranelift/src/codegen.rs`
   around `translate_operand` and `RValue::Field`.

4. **MIR fix.** Insert a coercion node in MIR that lower.kry emits
   when a struct field is used as an arg to a function expecting i64.
   The codegen then sees an explicit widen and emits the right insn.

Path 1 is the cleanest near-term workaround. Path 3 is the proper
fix for the bigger Cranelift-LLVM parity issue and should be done
regardless.

## Update 2026-05-19 — narrower repro found

Non-determinism reproduces with just **three struct constructions in
sequence**, no push needed:

```kryos
struct Tok { kind: i32, pos: i32 }
fn make_tok(k: i32, p: i32) -> Tok { return Tok { kind: k, pos: p } }
fn main() {
    let t1 = make_tok(1, 10)
    let t2 = make_tok(2, 20)
    let t3 = make_tok(3, 30)
    println("done")
}
```

Five-run sample: `0 0 139 139 0` (40% segfault rate).

The bug therefore is not specifically about `push(arr, struct_value)`.
It's triggered by **repeated struct allocations in the same function**.
Path 1 (rewrite push sites) would not fix this; path 3 (Cranelift
codegen / drop or struct-store correctness) is required.

Other experiments tried (none changed determinism):
- `--no-lto` build of stage-1
- `-g` debug-info build
- LLVM `--release` mode rejects compile entirely (see top of file).

The bug survives all toolchain knobs available from `kryos build`,
so it lives in the Rust-side codegen / runtime — most likely in
`emit_drop_for_value` or the `RValue::Struct` calloc + field-store
sequence in `compiler/crates/kryos-codegen-cranelift/src/codegen.rs`.

Next session needs to:
1. Run stage-1 under `cdb`/`windbg` or `gdb` to get a faulting RIP +
   stack trace.
2. Cross-reference against the named `__kryos_drop_*` helpers stage-0
   emits per @copy struct to find the bad load/store.
3. Likely need a `--validate-cl-ir` flag on stage-0 that asks
   Cranelift to verify the function after each `RValue::Struct`
   emission.

## RESOLVED 2026-05-19

The root cause was **#3 in disguise** — `RValue::Struct` was the bad path,
but the bug was not in `emit_drop_for_value`. It was in the field-store
emission itself.

`compiler/crates/kryos-codegen-cranelift/src/codegen.rs:3895` (the
`RValue::Struct` branch) computed each field's Cranelift type via
`compute_struct_layout` but then **discarded it** (captured as `_cl_ty`
with an underscore) and stored `stored_val` raw without any coercion.

`translate_operand` for `Operand::Constant(Constant::Int(n))` always
emits `iconst(types::I64, n)` regardless of the destination field
width. So:
- field declared `i32` (4 bytes) + value emitted as I64 (8 bytes)
  → 8-byte store into a 4-byte slot
- excess 4 bytes overflowed into the next field, or past the calloc'd
  struct end, corrupting adjacent heap blocks.

The 40% non-determinism came from heap layout: depending on what was
allocated next to the struct, the overflow either landed in unused
padding (no observable effect) or in another live allocation's header
(eventual `HeapReAlloc` crash).

**Fix** (commit `baff370`): apply the same coercion logic already
present in `Instruction::StoreField` and `Instruction::Assign` — for
every field store, `ireduce` / `sextend` / `bitcast` the value to the
field's actual Cranelift type before storing.

Verification (commits `baff370`, `887e2a2`):
- `repros/repro_3struct.kry`: 100/100 (was 60/100)
- `repros/repro_const_init.kry`: 100/100
- `repros/repro_mixed_fields.kry`: 100/100
- Above all also 100/100 under `KRYOS_USE_REALLOC=1` (historic
  HeapReAlloc path is now safe again).
- `kryos build --release` AOT: 100/100.

The `kryos_array_push` alloc+copy+leak workaround in
`compiler/crates/kryos-rt/src/array.rs` was reverted to realloc as the
default; the leak path is preserved behind `KRYOS_USE_ALLOC_LEAK=1`
as a diagnostic for any future regression.

## Stage-2 status after the fix

Stage-1 obj-mode on every self-host module (with the cranelift fix
plus stage-1 polish in commits `a77fe3f`):

```
OK    token.kry      (105 decls, 104 MIR fns)
OK    lexer.kry      (18 decls)    [parser drops 4 of 22 decls; needs work]
OK    ast.kry        (129 decls, 113 MIR fns)
FAIL  parser.kry     [stage-1 codegen crash]
OK    types.kry      (90 decls,  79 MIR fns)
FAIL  mir.kry        [stage-1 codegen crash at cg[~107-109]]
FAIL  lower.kry      [stage-1 codegen crash]
FAIL  optimize.kry   [stage-1 codegen crash]
FAIL  regalloc.kry   [stage-1 codegen crash]
OK    x86.kry        (101 decls, 101 MIR fns)
FAIL  codegen.kry    [stage-1 codegen crash]
OK    elf.kry        (65 decls,  63 MIR fns)
OK    coff.kry       (56 decls,  55 MIR fns)
OK    linker.kry     (60 decls,  54 MIR fns)
OK    runtime.kry    (75 decls,  75 MIR fns)
OK    main.kry       (18 decls,  10 MIR fns)
```

**11-15/16 modules pass stage-1** (5-run avg ~12.4, peak 15/16). Up from
4/16 prior to any fixes. The flakiness band remains because the underlying
heap pressure isn't fully eliminated.

Breakthrough fix in commit `0cb9089`: `Instruction::Drop`'s
`MirType::Struct` branch was a no-op for @copy structs (comment said
"the original owner will free" — but no owner ever ran field drops),
so every @copy-struct local with retained heap fields leaked its
field allocations. Stage-1 has hundreds of such locals across each
module. The leaked allocations piled up across compilation, tripping
the allocator and producing the heap-state-sensitive crashes that
had been blocking modules deterministically. Removing the no-op and
running drops through emit_drop_for_value gives multi-owner ref-count
balance and frees correctly on the last drop.

Two related sites (emit_drop_for_value's Struct-field iteration at
line 5776, and the exception-cleanup-locals filter near 6042) were
patched together with helper-call dispatch but caused a different
regression and were reverted. The proper fix for those is the
named-helper approach (`__kryos_drop_<Name>` already exists; needs
to be the default call mode at every inline site).

| Module     | Before fix | After cranelift + ty_compat | After lower.kry copy-to-temp |
|------------|------------|------------------------------|------------------------------|
| token      | OK         | OK                           | OK                           |
| lexer      | FAIL       | OK (skip-types)              | OK (skip-types)              |
| ast        | OK         | OK                           | OK                           |
| parser     | FAIL       | FAIL                         | OK                           |
| types      | FAIL       | OK (skip-types)              | OK (skip-types)              |
| mir        | FAIL       | FAIL                         | FAIL                         |
| lower      | FAIL       | FAIL                         | FAIL                         |
| optimize   | FAIL       | FAIL                         | FAIL                         |
| regalloc   | FAIL       | FAIL                         | FAIL                         |
| x86        | OK         | OK                           | OK                           |
| codegen    | FAIL       | FAIL                         | FAIL (flaky, sometimes OK)   |
| elf        | FAIL       | OK (skip-types)              | OK (skip-types)              |
| coff       | FAIL       | OK (skip-types)              | OK (skip-types)              |
| linker     | FAIL       | OK (skip-types)              | OK (skip-types)              |
| runtime    | OK         | OK                           | OK                           |
| main       | FAIL       | OK (skip-types)              | OK (skip-types)              |
| **TOTAL**  | **4/16**   | **10/16**                    | **11/16 (codegen flaky)**    |

The "flaky" mark on codegen.kry: across recent test runs it has
both passed and failed on identical source, indicating the same
heap-fragmentation-sensitive bug class that blocks mir/lower/
optimize/regalloc. So 11/16 is the firm count; codegen.kry hops
the line depending on heap state.

The remaining 6 modules crash with a DIFFERENT bug class, also in
stage-1's codegen. Investigation narrowed it to functions that contain
an inline-array-literal StoreField pattern:

```kryos
// Crashes in stage-1's codegen:
fn make_r(x: i32) -> R {
    let mut r = R { a: empty_arr }
    r.a = [x]      // <-- inline array literal in StoreField
    return r
}

// Works fine:
fn make_r(x: i32) -> R {
    let mut r = R { a: empty_arr }
    let tmp: [i32] = [x]   // <-- via temporary
    r.a = tmp
    return r
}
```

Empirical: a script-rewrite of all such sites in mir.kry shifted
the crash position but didn't eliminate it, suggesting there's
ALSO a heap-fragmentation-sensitive bug (after enough allocations,
something else trips). The trace flag `KRYOS_CG_TRACE=1` (added in
codegen.kry's `cg_emit_module`) prints which function is being
codegened so the crash can be located precisely on any new repro.

## Open items for the next session

### Highest-value lead: kryos_array_clone shallow-copy double-free

**Hypothesis (likely the next root cause).** Stage-1's @copy structs
that contain `[str]` / `[[T]]` / `[Struct]` fields trigger a
**double-free** under the current `kryos_array_clone` semantics:

- `compiler/crates/kryos-rt/src/array.rs:kryos_array_clone` allocates
  a new array and does `ptr::copy_nonoverlapping((*arr).data,
  (*result).data, len * ELEM_SIZE)`. That bulk-copy duplicates the
  element pointers verbatim — no per-element clone / no ref_count
  bump on heap-pointer elements.
- The new clone array has `ref_count = 1`. The original still has
  `ref_count = 1`. Both arrays now own the same string/sub-array
  pointers.
- Drop (`compiler/crates/kryos-codegen-cranelift/src/codegen.rs`
  line 5681, `MirType::Array` branch) iterates elements when
  `ref_count == 1` and calls `kryos_string_free` /
  `kryos_array_free` / `__kryos_drop_<Struct>` per element.
- When BOTH the original and the clone get dropped — each one is
  sole owner of its array header, so each enters the iterate-and-free
  branch — they both free the SAME element pointers. Double-free.

Empirically matches the symptoms: heap-fragmentation-sensitive
crashes that hit specifically when modules have many @copy structs
with array-of-pointer fields (`mir.kry`, `lower.kry`, etc.) and
that don't trigger on the smaller bootstrap-OK modules.

**Fix paths:**
1. Per-element deep-clone in `kryos_array_clone`. Needs element type
   info at runtime — KryosArray has `elem_size` but no type tag.
   Could store a per-array drop-fn pointer in the header.
2. In codegen.rs `RValue::Struct` (line 3895) and `emit_deep_copy_struct`,
   after calling `kryos_array_clone`, iterate elements and call
   appropriate per-element clone (`kryos_string_clone` /
   recursive `kryos_array_clone` / `__kryos_clone_<Struct>`).
   This is the canonical fix — keeps the runtime polymorphism-free.
3. Or: change @copy struct array fields to use retain semantics
   (`kryos_array_retain`) instead of clone, sharing the underlying
   array. Breaks @copy invariant on mutation but most stage-1 @copy
   structs don't mutate after copy. Diagnostic value.

### Other open items

1. **Stage-1 parser drops decls** — `lexer.kry` has 22 top-level decls
   but the parser only reports 18. Bisect: parser bails on something in
   `lex_scan_string` (line 192-272). Lex_scan_string has nested while
   with `continue` inside it.
2. **Stage-1 type checker is incomplete** — produces many false-positive
   errors on source that stage-0 type-checks cleanly. KRYOS_SKIP_TYPES=1
   added as escape hatch but proper fix needed for production.
3. **Optimizer disabled on obj path** — even constant_fold causes
   lower to subsequently crash. Symptom of the same regalloc/codegen
   fragility above.
4. **Stage-3 fixed point** — gated on every module above passing.
