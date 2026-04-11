# Kryos Self-Hosting Compiler

## Status: STAGE 1 COMPILES, STAGE 2 SEGFAULTS

As of 2026-04-11, the self-hosting compiler **compiles to a working Stage-1 binary**
via `--skip-ownership`, but the Stage-1 binary segfaults when attempting to compile
itself (Stage 2). The code is structurally complete (16 files, 19,202 lines) and the
Stage-1 binary successfully tokenizes the full 97,329-token source before crashing
during parsing or later phases.

### What works

- **Stage-1 binary compiles** (15.2 MB) using the Rust compiler with `--skip-ownership`
- **Stage-1 runs** and successfully tokenizes 97K+ tokens from its own source
- **6 of 16 files pass `check` cleanly**: `token.kry`, `lexer.kry` (1 warning),
  `ast.kry`, `x86.kry`, `elf.kry`, `coff.kry`
- **15 of 16 files pass with `--skip-ownership`** -- only `runtime.kry` still fails
  (89 undefined-builtin errors for `syscall6`, `mem_read_i64`, etc.)

### Where it breaks

- **Stage 2 segfault**: The Stage-1 binary crashes after tokenizing when it tries
  to compile itself. This is likely a codegen bug in the Rust compiler's handling
  of complex struct operations under `--skip-ownership` mode.
- **580 ownership errors** prevent compilation without `--skip-ownership`.

### What fails

| File | Errors | Category |
|------|--------|----------|
| `token.kry` | 0 | PASS |
| `lexer.kry` | 0 (1 warning) | PASS |
| `ast.kry` | 0 | PASS |
| `x86.kry` | 0 | PASS |
| `elf.kry` | 0 | PASS |
| `coff.kry` | 0 | PASS |
| `linker.kry` | 4 | E0300: use of moved value (`output_path`) |
| `types.kry` | 33 | E0300/E0382: ownership moves in struct construction |
| `mir.kry` | 37 | E0300: moved values in struct literals |
| `regalloc.kry` | 38 | E0300: moved values in loops |
| `codegen.kry` | 45 | E0300/E0382: moved values in conditional branches |
| `parser.kry` | 48 | E0300: moved values in struct construction (`empty_ex`) |
| `optimize.kry` | 48 | E0300: moved values in struct construction |
| `lower.kry` | 85 | E0300/E0382: moved values in MIR node construction |
| `runtime.kry` | 89 | E0102: undefined builtins (`syscall6`, `mem_read_i64`, etc.) |
| `main.kry` | 153 | E0300/E0382: accumulated from imported modules + own code |
| **Total** | **580** | |

All 491 non-runtime errors fall into two categories:
- **E0300** (128 in main.kry alone): Use of moved value -- variables like `id`,
  `name`, `empty_ex` are moved into struct literals and then used again.
- **E0382** (25 in main.kry): Use of partially moved value -- accessing a struct
  field after another field was moved.

These are not bugs in the self-host code. They reflect a gap between what the
ownership checker enforces and what the language needs: either implicit `Copy` for
small/primitive types, shared borrows for struct field access, or a `clone()`
mechanism.

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

# Full check (currently 153 errors in main.kry):
cargo run --release -- check self-host/main.kry
```

### Bootstrap attempt

```bash
./self-host/bootstrap.sh --verbose
```

The script strips internal `use` statements from the concatenated source and passes
`--skip-ownership` to the Stage-0 compiler. Stage 1 compiles successfully, but
Stage 2 segfaults.

### Expected output (current state)

```
=== Pre-flight: Type-checking self-host files ===
  PASS  token.kry
  PASS  lexer.kry
  ...
  Files passing check:                 6 / 16
  Files passing with --skip-ownership: 15 / 16
  Total ownership errors:              580

=== Stage 1: Compiling self-host with Rust compiler -> stage-1 ===
  Stage 1 binary: .../kryos-stage1
  Stage 1 size: 15187456 bytes

=== Stage 2: Compiling self-host with stage-1 -> stage-2 ===
=== Kryos Self-Hosted Compiler ===
File: .../kryos-sh-full.kry
Tokens: 97329
Segmentation fault
FAIL: Stage 2 compilation failed
```

## Roadmap

To achieve full self-hosting (Stage-2 == Stage-3), the following issues must be
resolved in roughly this priority order:

### 1. Fix Stage-2 segfault (CRITICAL -- blocks bootstrap)
The Stage-1 binary successfully tokenizes its own source (97K tokens) but segfaults
during parsing or a later phase. This is most likely a codegen bug in the Rust
compiler's `--skip-ownership` mode -- moved values are used after being invalidated,
leading to dangling pointers or corrupt data at runtime. Debugging approach:
- Run Stage-1 under a debugger (gdb/lldb) to find the crash site
- Check if the crash is in parser, type-checker, or lowering
- May require fixing the ownership errors first (see item 2)

### 2. Implicit Copy for primitives (blocks 491 ownership errors)
Values of type `i32`, `i64`, `f64`, `bool`, and `str` are moved on assignment and
struct construction. The self-host code passes the same integer ID or string name
into multiple struct fields -- legal in most languages, illegal under Kryos's current
move semantics. Options:
- Make primitive types implicitly `Copy` (like Rust)
- Add a `clone()` builtin or `.copy()` method
- Add shared/immutable borrow semantics for reads

Fixing this would eliminate 491 errors across 10 files and remove the need for
`--skip-ownership`, which would likely also fix the Stage-2 segfault.

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

1. **Implicit Copy for primitives** -- eliminates 491 ownership errors, removes
   need for `--skip-ownership`, likely fixes Stage-2 segfault
2. **Debug Stage-2 crash** -- if it persists after ownership fixes, investigate
   codegen correctness in the Rust compiler
3. **Compiler intrinsics for runtime** -- makes runtime.kry compilable for
   Stage-1+ self-contained builds
4. **Full bootstrap verification** -- Stage-2 == Stage-3 binary identity check
