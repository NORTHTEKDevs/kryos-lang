# Kryos Self-Hosted Compiler — Design Document

> **Goal:** Eliminate all Rust/LLVM/Cranelift dependencies. The Kryos compiler is written entirely in Kryos, generates native machine code, links its own executables, and compiles itself. Zero external dependencies beyond the OS kernel.

**Date:** 2026-04-07
**Status:** Approved

---

## Architecture

```
.kry source
    |
    v
 Lexer (DONE - 599 lines)
    |
    v
 Parser (DONE - 2,836 lines)
    |
    v
 Type Checker (NEW)
    |
    v
 MIR Lowering (NEW)
    |
    v
 Optimizer (NEW)
    |
    v
 Register Allocator (NEW)
    |
    v
 x86_64 Instruction Encoder (NEW)
    |
    v
 Object File Emitter (NEW - ELF/COFF/Mach-O)
    |
    v
 Linker (NEW - static linking, relocations, executable output)
    |
    v
 Native executable
```

Every component is written in Kryos. The only external dependency is the OS kernel (syscalls for file I/O, memory allocation, process management).

---

## Component Design

### 1. Type Checker (~3,000 lines estimated)

Walks the AST, resolves names, infers types, validates constraints.

**Responsibilities:**
- Symbol table: track declarations, scopes, shadowing
- Type inference: return types, generic instantiation, closure captures
- Type validation: assignment compatibility, function signatures, trait bounds
- Ownership tracking: borrow checker (immutable/mutable reference rules)
- Capability enforcement: @capabilities attribute validation
- Error collection: accumulate errors with source locations, don't abort on first error

**Data structures:**
- `SymbolTable` struct with scope stack (array of maps)
- `TypeInfo` struct representing resolved types
- `TypeError` struct with source location + message

**Strategy:** Pattern after the Rust compiler's type checker (`crates/kryos-types/src/check.rs`) but simplified. The Rust version is 3,500 lines — the Kryos version targets similar coverage.

### 2. MIR Lowering (~4,000 lines estimated)

Transforms typed AST into Mid-level IR: basic blocks, typed locals, SSA-like instructions.

**MIR data structures (in Kryos):**
- `MirModule`: array of `MirFunction`
- `MirFunction`: name, params, locals (typed), array of `BasicBlock`
- `BasicBlock`: array of `Instruction` + one `Terminator`
- `Instruction`: Assign(dest, RValue) | Drop | Nop
- `RValue`: Use | BinOp | UnOp | Call | Const | Field | Index | Array | Struct | Enum | Closure | Cast | AddrOf | Deref
- `Terminator`: Return | Goto | Branch(cond, then, else) | Switch(value, targets, default) | Unreachable
- `MirType`: I8-I128, U8-U128, F32, F64, Bool, Char, Str, Void, Ptr, Array, Struct, Enum, Function

These mirror the Rust MIR exactly (`crates/kryos-mir/src/ir.rs`). The Kryos version uses integer kind-constants (like the existing self-hosted AST does) since enum payloads bypass the type checker.

**Strategy:** Walk typed AST, allocate locals for each variable/temporary, emit instructions into basic blocks. Handle: function calls (including builtins), control flow (if/while/for/match → blocks + terminators), closures (capture analysis + environment allocation), error handling (try/catch → landing pads).

### 3. Optimizer (~2,000 lines estimated)

Operates on MIR before register allocation. Same passes as the Rust compiler already implements:

- **Constant folding:** evaluate known-constant expressions at compile time
- **Dead code elimination:** remove unreachable blocks and unused assignments
- **Function inlining:** inline small functions (< 20 instructions) at call sites
- **Tail call optimization:** convert tail-recursive calls to loops
- **Strength reduction:** replace expensive ops with cheaper equivalents (x*2 → x<<1)

### 4. Register Allocator (~2,000 lines estimated)

Maps MIR locals to physical x86_64 registers and stack slots.

**Algorithm:** Linear scan register allocation.
- Compute live ranges for each local (which blocks/instructions it's alive across)
- Sort by start position
- Greedily assign registers; spill to stack when all registers occupied
- Handle register constraints (e.g., div uses RAX/RDX, function args use RDI/RSI/RDX/RCX/R8/R9 on SysV or RCX/RDX/R8/R9 on Windows)

**Available registers (x86_64):**
- General purpose: RAX, RCX, RDX, RBX, RSI, RDI, R8-R15 (14 usable, minus RSP/RBP)
- Float: XMM0-XMM15
- Callee-saved: RBX, R12-R15, RBP (must save/restore)
- Caller-saved: RAX, RCX, RDX, RSI, RDI, R8-R11 (scratch)

**Output:** `RegAlloc` struct mapping each local to either `Reg(register_id)` or `Stack(offset_from_rbp)`.

### 5. x86_64 Instruction Encoder (~3,000 lines estimated)

Emits raw machine code bytes. Consumes MIR + register allocation output.

**Instruction set needed (~50 instructions):**

Arithmetic: `add`, `sub`, `imul`, `idiv`, `neg`, `inc`, `dec`
Bitwise: `and`, `or`, `xor`, `shl`, `shr`, `sar`, `not`
Comparison: `cmp`, `test`
Branches: `jmp`, `je`, `jne`, `jl`, `jle`, `jg`, `jge`, `ja`, `jb`
Data movement: `mov` (reg/reg, reg/imm, reg/mem, mem/reg), `movsx`, `movzx`, `lea`
Stack: `push`, `pop`, `sub rsp`, `add rsp`
Function: `call`, `ret`
Float (SSE2): `movsd`, `addsd`, `subsd`, `mulsd`, `divsd`, `ucomisd`, `cvtsi2sd`, `cvtsd2si`
System: `syscall` (for runtime), `int 0x80` (Linux 32-bit fallback)

**Encoding format:** x86_64 uses variable-length encoding (1-15 bytes per instruction). Key prefixes:
- REX prefix (0x40-0x4F): for 64-bit operands and R8-R15 registers
- ModR/M byte: specifies register/memory operand addressing
- SIB byte: for complex memory addressing (base + index * scale + displacement)

**Implementation:** A `CodeBuffer` struct that accumulates bytes. Helper functions per instruction class:
```
fn emit_mov_reg_reg(buf: CodeBuffer, dst: i32, src: i32)
fn emit_mov_reg_imm64(buf: CodeBuffer, dst: i32, imm: i64)
fn emit_add_reg_reg(buf: CodeBuffer, dst: i32, src: i32)
fn emit_call_rel32(buf: CodeBuffer, offset: i32)
fn emit_jmp_rel32(buf: CodeBuffer, offset: i32)
fn emit_cmp_reg_reg(buf: CodeBuffer, a: i32, b: i32)
fn emit_jcc(buf: CodeBuffer, condition: i32, offset: i32)
```

**Relocation handling:** Function calls and data references emit placeholder bytes, record a relocation entry `{offset, symbol, type}`. The linker resolves these.

### 6. Object File Emitter (~2,000 lines estimated)

Writes compiled machine code into standard object file formats.

**Targets:**
- **ELF** (.o) for Linux — header, section headers (.text, .data, .bss, .rodata, .symtab, .strtab, .rela.text), symbol table, relocation entries
- **COFF** (.obj) for Windows — header, section table, symbol table, relocations
- **Mach-O** (.o) for macOS — header, load commands, sections, symbol table, relocations

**Priority:** ELF first (simplest format, best documented), COFF second (Windows support), Mach-O third.

**Implementation:** Write raw bytes to a buffer using little-endian encoding. Each format has a well-defined header structure that maps directly to struct fields.

### 7. Linker (~3,000 lines estimated)

Combines object files into a final executable. Static linking only (no dynamic linking at launch).

**Responsibilities:**
- Read object files (ELF/COFF/Mach-O)
- Resolve symbols across objects (compiler output + runtime library)
- Lay out sections (.text, .data, .rodata, .bss) in the final executable
- Apply relocations (patch call/jump targets, data addresses)
- Write executable headers (ELF executable, PE executable, Mach-O executable)
- Set entry point to `main`

**Supported relocations:**
- `R_X86_64_PC32`: 32-bit PC-relative (for function calls within same binary)
- `R_X86_64_PLT32`: like PC32 but for PLT entries
- `R_X86_64_64`: absolute 64-bit address (for data references)
- `R_X86_64_32S`: sign-extended 32-bit absolute

**Strategy:** Keep it simple. No dynamic linking, no shared libraries, no lazy binding. Statically link everything into one executable. This is fine for a self-hosted compiler — the binary is self-contained.

### 8. Runtime in Kryos (~2,500 lines estimated)

Rewrite `crates/kryos-rt/` from Rust to Kryos. The runtime provides:

- **Memory:** malloc/free wrappers around OS APIs (mmap on Linux, VirtualAlloc on Windows)
- **Strings:** KryosString (length-prefixed, heap-allocated), concat, compare, slice, char access
- **Arrays:** KryosArray (dynamic, heap-allocated), push, pop, get, set, len
- **Maps:** KryosMap (hash map), insert, get, delete, keys, has
- **Channels:** bounded MPSC channels for spawn/send/receive
- **Arc:** atomic reference counting for shared data
- **Panic:** panic handler with stack traces
- **I/O:** file read/write via OS syscalls, stdin/stdout/stderr

**Platform abstraction:** A thin `sys` module with platform-specific syscall wrappers:
- Linux: `syscall(SYS_write, fd, buf, len)` etc.
- Windows: kernel32.dll calls via `extern` declarations
- macOS: `syscall(SYS_write, ...)` (similar to Linux but different numbers)

The runtime compiles into object files using the self-hosted compiler, then the linker includes them in every Kryos binary.

---

## Bootstrap Chain

```
Stage 0: Rust compiler builds kryos-stage0 (one-time, never again)
Stage 1: kryos-stage0 compiles self-hosted source -> kryos-stage1
Stage 2: kryos-stage1 compiles self-hosted source -> kryos-stage2
Verify:  diff kryos-stage1 kryos-stage2 (must be identical = bootstrap verified)
Ship:    kryos-stage2 is the release binary
```

After bootstrap verification, the Rust compiler is never needed again. Pre-built binaries are shipped for each OS/arch.

---

## Language Gaps to Fill First

Before the self-hosted compiler can be written, these gaps in the Rust-backed compiler must be fixed:

1. **Byte buffer operations:** Need `write_byte(buf, byte)`, `write_i32_le(buf, val)`, `write_i64_le(buf, val)` builtins for emitting machine code and object files
2. **Bitwise operations in codegen:** `&`, `|`, `^`, `<<`, `>>` must work end-to-end (verify)
3. **Map completeness:** `map_keys()`, `map_has()`, `map_delete()` runtime functions (needed for symbol tables)
4. **Process control:** `exit(code)` builtin, `args()` for CLI argument parsing
5. **Raw file write:** `file_write_bytes(path, buffer)` that writes raw bytes, not strings

---

## "Nobody Laughs" Checklist

Beyond self-hosting, these are required for Series A credibility:

- [ ] All 11 examples run clean (DONE)
- [ ] Self-hosted compiler compiles itself (bootstrap verified)
- [ ] Error messages with source context, colors, caret indicators, suggestions
- [ ] One-command install: `curl -fsSL https://kryos.dev/install.sh | sh`
- [ ] 10-minute tutorial: "Build a CSV Analyzer in Kryos"
- [ ] Reproducible benchmarks: `kryos bench` runs criterion suite
- [ ] Pre-built binaries: Linux x86_64, macOS arm64/x86_64, Windows x86_64
- [ ] Package manager: `kryos pkg init`, `kryos pkg add`, `kryos pkg build`
- [ ] VS Code extension published to marketplace
- [ ] Professional website with docs, playground, and blog

---

## Estimated Scope

| Component | Lines (est.) | Difficulty |
|---|---|---|
| Type checker | 3,000 | Medium |
| MIR lowering | 4,000 | Medium |
| Optimizer | 2,000 | Medium |
| Register allocator | 2,000 | Hard |
| x86_64 encoder | 3,000 | Hard |
| Object file emitter | 2,000 | Medium |
| Linker | 3,000 | Hard |
| Runtime in Kryos | 2,500 | Medium |
| Language gap fixes | 500 | Easy |
| **Total new Kryos code** | **~22,000** | |
| Existing frontend | 4,819 | Done |
| **Total self-hosted compiler** | **~27,000** | |

Plus: install script, tutorial, benchmarks, website, package registry.

---

## Risk Notes

- x86_64 encoding is fiddly but well-documented (Intel SDM Volume 2)
- Register allocation is the hardest algorithmic piece — linear scan is simpler than graph coloring and good enough
- ELF format is simpler than COFF/Mach-O — start with Linux, port later
- Bootstrap verification is the ultimate correctness test — if stage-1 and stage-2 match, the compiler is correct
- Debug builds of the Rust compiler use ~48GB RAM — all bootstrap builds use --release -j 4
