# Kryos Self-Hosting Compiler

## Status: 15/16 FILES CLEAN, STAGE 1 COMPILES

As of 2026-05-15, the self-hosting compiler **compiles to a working Stage-1 binary**
and the ownership checker is happy with the source: `main.kry` reports **0 errors**
(7 warnings only). The remaining 89 errors are confined to `runtime.kry`, and are
all **undefined-builtin** errors for low-level intrinsics (`syscall6`, `mem_read_i64`,
and friends) that the Rust compiler does not yet expose as builtins. They are
unrelated to ownership / partial moves.

The code is structurally complete (16 files, 19,202 lines).

### What works

- **Stage-1 binary compiles** using the Rust compiler
- **`main.kry` passes `check` with 0 errors** (7 warnings)
- **15 of 16 files pass `check` cleanly** -- only `runtime.kry` reports errors
  (89 undefined-builtin errors for `syscall6`, `mem_read_i64`, etc.)
- **`@copy` annotations** are applied across all self-host structs (65 structs
  across 10 files)
- **Partial-move on @copy structs is fixed** -- the ownership checker correctly
  skips partial-move tracking when the parent struct is `@copy`. Regression tests:
  see `compiler/crates/kryos-ownership/tests/ownership.rs`
  (`copy_struct_field_access_no_partial_move`,
  `copy_struct_field_then_other_field`,
  `non_copy_struct_partial_move_still_detected`).
- **Array sentinel reuse across struct fields is OK** -- empty array literals
  (`[]`) used to initialize multiple struct fields each construct a fresh value
  and do not interfere with one another. Regression test:
  `empty_array_sentinel_reused_in_struct_fields`.

### What is still rough

- **`runtime.kry` -- 89 undefined-builtin errors**: requires intrinsics support
  (e.g. `syscall6`, `mem_read_i64`, `mem_write_i64`). Tracked as a separate item;
  not blocking on ownership analysis.
- **Stage-2 self-host bootstrap is very slow** (>600s on the current machine).
  Stage 1 produces a usable binary, but Stage 2 (Stage-1 compiling itself end-to-end)
  needs further codegen / runtime work before it is practical to time-bound.

### Per-file `check` status (kryos 2.2.0)

| File | Status |
|------|--------|
| ast.kry | clean |
| codegen.kry | clean (1 warning) |
| coff.kry | clean |
| elf.kry | clean |
| lexer.kry | clean |
| linker.kry | clean |
| lower.kry | clean (1 warning) |
| **main.kry** | **0 errors, 7 warnings** |
| mir.kry | clean |
| optimize.kry | clean |
| parser.kry | clean (3 warnings) |
| regalloc.kry | clean (1 warning) |
| runtime.kry | 89 errors (undefined builtins) |
| token.kry | clean |
| types.kry | clean |
| x86.kry | clean |

### @copy annotations applied (2026-04-11)

All 65 structs across 10 files now have `@copy`. Key structs annotated:
- **token.kry**: `Token`
- **ast.kry**: `Expr`, `Stmt`, `Decl`, `Pattern`, `TypeExpr`, `Param`, `MatchArm`,
  `StringPart`, `SelectBranch`, `Annotation`, `GenericParam`, `StructField`,
  `EnumVariant`, `MessageHandler`, `ImportPath`, `Module`
- **parser.kry**: `Parser`, `ParseExprResult`, `ParseStmtResult`, `ParseDeclResult`,
  `ParseTypeResult`, `ParsePatternResult`, `ParseNameResult`, `ParseAnnotationResult`,
  `ParseGenericsResult`, `ParseParamsResult`, `ParseBlockResult`, `ParseArgsResult`
- **types.kry**: `TypeInfo`, `Symbol`, `Scope`, `StructDef`, `EnumDef`, `FnSig`,
  `TraitDef`, `TypeAlias`, `TypeChecker`, `LookupResult`, `TCExprResult`
- **mir.kry**: `MirType`, `Operand`, `RValue`, `Instruction`, `Terminator`,
  `BasicBlock`, `MirLocal`, `MirParam`, `MirFunction`, `MirStructDef`, `MirEnumDef`,
  `MirModule`
- **Other**: `Lexer`, `LowerCtx`, `CodegenCtx`, `BranchPatch`, `LiveInterval`,
  `RegAllocResult`, `LivenessMap`, `RegPool`, `PrologueInfo`, `LinkerSymbol`,
  `LinkerReloc`, `LinkerInput`, `LinkerMerged`, `ResolvedSymbol`, `LinkerResult`,
  `ElfObject`, `ShstrtabResult`, `CoffObject`

Historical note: an earlier revision of this README documented two ownership-
checker bugs that blocked the self-host (partial-move on `@copy` structs, and
array sentinel reuse). Both are now fixed in `kryos-ownership/src/analysis.rs`
and covered by regression tests in `kryos-ownership/tests/ownership.rs`. The
remaining 89 errors are all in `runtime.kry` and are undefined-builtin errors
for low-level intrinsics, not ownership.

## Architecture

The self-hosting compiler reimplements the Rust-based Kryos compiler entirely in
Kryos. It follows the same pipeline:

```
Source (.kry)
  |
  v
[token.kry]     Token kinds, keyword lookup table
  |
  v
[lexer.kry]     Tokenizer: source string -> Token array
  |
  v
[ast.kry]       AST node structs and constructors
  |
  v
[parser.kry]    Recursive-descent + Pratt expression parser
  |
  v
[types.kry]     Type checker: resolves types, checks correctness
  |
  v
[mir.kry]       MIR data structures (basic blocks, instructions, locals)
  |
  v
[lower.kry]     AST -> MIR lowering
  |
  v
[optimize.kry]  MIR optimization passes
  |
  v
[regalloc.kry]  Linear-scan register allocator (x86_64)
  |
  v
[x86.kry]       x86_64 machine code encoder
  |
  v
[codegen.kry]   MIR -> x86_64 native code generation
  |
  v
[elf.kry]       ELF64 object file emitter (Linux)
[coff.kry]      COFF/PE object file emitter (Windows)
  |
  v
[linker.kry]    Static linker: objects -> native executable
  |
  v
[runtime.kry]   Runtime library (I/O, memory, strings, arrays)
  |
  v
[main.kry]      Entry point: wires all phases together
```

## Files

| File | Lines | Size | Description |
|------|-------|------|-------------|
| `token.kry` | 352 | 13 KB | Token kind constants and keyword lookup table |
| `lexer.kry` | 608 | 19 KB | Full lexer, mirrors Rust kryos-lexer crate |
| `ast.kry` | 671 | 19 KB | AST node structs, mirrors Rust kryos-ast crate |
| `parser.kry` | 2,855 | 104 KB | Recursive-descent parser with Pratt expressions |
| `types.kry` | 2,239 | 77 KB | Strict type checker |
| `mir.kry` | 1,195 | 34 KB | MIR data structures (CFG, instructions, locals) |
| `lower.kry` | 2,658 | 89 KB | AST-to-MIR lowering pass |
| `optimize.kry` | 1,599 | 47 KB | MIR optimization passes |
| `regalloc.kry` | 1,196 | 35 KB | Linear-scan register allocator for x86_64 |
| `x86.kry` | 759 | 25 KB | x86_64 machine code instruction encoder |
| `codegen.kry` | 1,202 | 41 KB | MIR-to-x86_64 code generation |
| `elf.kry` | 692 | 22 KB | ELF64 relocatable object file emitter (Linux) |
| `coff.kry` | 537 | 19 KB | COFF object file emitter (Windows) |
| `linker.kry` | 1,488 | 45 KB | Static linker producing ELF/PE executables |
| `runtime.kry` | 613 | 18 KB | Runtime library (syscalls, I/O, memory, strings) |
| `main.kry` | 538 | 16 KB | Entry point, AST printer, CLI driver |
| **Total** | **19,202** | **623 KB** | |

## Building

### Prerequisites

- Rust toolchain (for building the Stage-0 compiler)
- The Rust-based Kryos compiler: `cargo build --release -j 4`

### Type-checking individual files

```bash
# Clean pass (5 files):
cargo run --release -- check self-host/token.kry
cargo run --release -- check self-host/ast.kry
cargo run --release -- check self-host/x86.kry
cargo run --release -- check self-host/elf.kry
cargo run --release -- check self-host/coff.kry

# Pass with --skip-ownership (15 of 16 files):
cargo run --release -- check self-host/main.kry --skip-ownership

# Full check (0 errors in main.kry, only 89 errors remain in runtime.kry):
cargo run --release -- check self-host/main.kry
```

### Bootstrap attempt

```bash
./self-host/bootstrap.sh --verbose
```

The script strips internal `use` statements from the concatenated source and
feeds it to the Stage-0 compiler. Stage 1 compiles successfully; Stage 2
(Stage-1 compiling itself) currently runs but is too slow to complete inside
the project's default timeouts (>600s) and needs more codegen/runtime work
before it is practical to time-bound.

### Expected output (current state)

```
=== Pre-flight: Type-checking self-host files ===
  PASS  token.kry
  PASS  lexer.kry
  ...
  Files passing check:                 15 / 16
  Files failing (runtime intrinsics):   1 / 16  (runtime.kry, 89 errors)

=== Stage 1: Compiling self-host with Rust compiler -> stage-1 ===
  Stage 1 binary: .../kryos-stage1

=== Stage 2: Compiling self-host with stage-1 -> stage-2 ===
  (currently very slow; see Roadmap)
```

## Roadmap

To achieve full self-hosting (Stage-2 == Stage-3), the following issues must be
resolved in roughly this priority order:

### 1. Speed up Stage-2 self-host bootstrap
The Stage-1 binary now successfully runs the full pipeline on its own source,
but Stage 2 is too slow to complete within reasonable bounds (>600s). This is
likely a combination of unoptimized codegen for hot inner loops in the
self-host (lexer / parser / type-checker) and runtime allocation pressure.
Next steps:
- Profile Stage-1 under perf/cargo-flamegraph to find hot spots
- Audit lexer / parser tight loops for unnecessary cloning
- Ensure the optimizer is actually running on the Stage-1 build

### 2. ~~Fix remaining ownership errors~~ (DONE)
Resolved 2026-05. `main.kry` reports 0 errors; the previously listed
partial-move-on-`@copy` and array-sentinel-reuse bugs are fixed in the
ownership analyzer and covered by regression tests in
`compiler/crates/kryos-ownership/tests/ownership.rs`.

### 3. Runtime builtins as intrinsics (blocks runtime.kry -- 89 errors)
`runtime.kry` calls low-level primitives (`syscall6`, `mem_read_i64`,
`mem_write_i64`, `__builtin_map_keys_str`, etc.) that are not defined in
user-space Kryos. These need to be:
- Compiler intrinsics recognized by the Stage-0 compiler
- Or inline assembly support
- Or an FFI mechanism to call platform-native functions

Note: For Stage-0 builds (Rust compiler -> Stage-1), the runtime is excluded and
the Rust compiler provides these builtins natively. This only blocks the Stage-1+
builds where the self-host must be fully self-contained.

### Priority path to self-hosting

1. ~~**Fix ownership checker for @copy + array-sentinel reuse**~~ (DONE 2026-05)
2. **Speed up Stage-2 bootstrap** -- profile/optimize hot paths so Stage-2
   completes within a reasonable wall-clock budget
3. **Compiler intrinsics for runtime** -- makes runtime.kry compilable for
   Stage-1+ self-contained builds
4. **Full bootstrap verification** -- Stage-2 == Stage-3 binary identity check
