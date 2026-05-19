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
