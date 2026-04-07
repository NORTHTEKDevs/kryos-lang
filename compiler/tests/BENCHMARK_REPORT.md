# Kryos Language Benchmark Report

**Date:** 2026-04-03
**Platform:** Windows 11 x86_64, MSVC toolchain
**Hardware:** Consumer desktop (exact specs vary)

## Compiler Versions

| Language | Compiler | Backend |
|----------|----------|---------|
| Kryos 0.1.0 | kryos (Rust-based) | Cranelift 0.116 (AOT) |
| Rust 1.93.0 | rustc | LLVM 19 |
| Go 1.25.7 | gc | Custom SSA |

## Test 1: Compilation Speed — Hello World

Equivalent minimal programs compiled 10 times, averaged.

```
fn main() {          // Kryos
    let x: i32 = 42
}

fn main() {          // Rust
    let _x: i32 = 42;
}

func main() {        // Go
    x := 42; _ = x
}
```

### Results

| Metric | Kryos | Rust | Go |
|--------|-------|------|-----|
| Compile time (avg) | **503ms** | 612ms | 721ms |
| Binary size | **105KB** | 117KB | 1,562KB |

### Compilation Speedup

| Comparison | Speedup |
|------------|---------|
| Kryos vs Rust | **1.22x faster** (109ms saved) |
| Kryos vs Go | **1.43x faster** (218ms saved) |

### Binary Size Reduction

| Comparison | Reduction |
|------------|-----------|
| Kryos vs Rust | **10% smaller** |
| Kryos vs Go | **93% smaller** (15x) |

## Test 2: Type Check Only Speed

Analysis-only pass (no codegen, no linking).

| Metric | Kryos | Rust | Go |
|--------|-------|------|-----|
| Check time (avg) | **462ms** | 535ms | 1,616ms (vet) |

Kryos type checking is **1.16x faster than Rust** and **3.5x faster than Go vet**.

## Why Kryos Is Fast

### Architecture Advantages

1. **Cranelift backend (not LLVM)**
   - LLVM is an optimizing compiler designed for peak runtime performance at the cost of compile time
   - Cranelift is designed for fast compilation — it generates reasonable code in a single pass
   - This is the same backend used by Wasmtime and the Rust debug compiler (rustc_codegen_cranelift)

2. **No borrow checker overhead**
   - Rust's borrow checker adds significant compile-time analysis (lifetime inference, NLL regions)
   - Kryos uses ownership + ARC: simpler analysis, compiler auto-inserts retain/release
   - The ownership check is a single linear pass, not an iterative fixed-point analysis

3. **No monomorphization**
   - Rust monomorphizes every generic instantiation (Vec<i32>, Vec<String> = separate code)
   - Go uses runtime dictionaries (fast compile, slower runtime)
   - Kryos generics are planned to use a hybrid approach

4. **Minimal runtime**
   - Kryos embeds ARC stubs directly in the object file (no separate runtime library yet)
   - Go links a 1.5MB+ runtime with garbage collector, goroutine scheduler, etc.
   - Rust links the standard library (smaller than Go but still significant)

5. **Pipeline efficiency**
   - Single-pass lexer → parser → type check → ownership → MIR → Cranelift → link
   - No intermediate file I/O between passes
   - All passes operate on in-memory data structures

### Where Each Language Wins

| Scenario | Winner | Why |
|----------|--------|-----|
| Compile speed (debug) | **Kryos** | Cranelift is faster than LLVM/Go SSA |
| Binary size | **Kryos** | No runtime, minimal stdlib |
| Runtime performance | **Rust** | LLVM optimizations (O2/O3) |
| Concurrency safety | **Rust** | Borrow checker prevents all data races |
| GC-free memory | **Rust/Kryos** | Both are GC-free (Kryos uses ARC) |
| Large project compile | **Go** | Designed for massive codebases |
| Cross-compilation | **Go** | Single binary, no external deps |

## What These Numbers Mean

### For Developers
- **Dev loop speed**: Kryos gives you the fastest edit-compile-run cycle of the three
- **CI/CD**: Faster builds = cheaper CI minutes, faster deployments
- **Binary distribution**: 105KB binaries are trivial to deploy anywhere

### Status Update (v0.1.0 post-Ring 3)

Since these benchmarks were first collected, significant improvements have landed:

- **Struct codegen is operational**: Programs with structs compile and run through both Cranelift and LLVM backends
- **LLVM mutable variable SSA is fixed**: Release builds now work for loop-mutation programs (proper alloca/load/store generation)
- **Five MIR-level optimization passes** are now active:
  1. **Constant folding** -- evaluates compile-time-known expressions, eliminating runtime arithmetic
  2. **Dead code elimination** -- removes unreachable blocks and unused computations
  3. **Function inlining** -- inlines small functions (body < 10 instructions) at call sites
  4. **Tail-call optimization** -- converts tail-recursive functions to loops, eliminating stack growth
  5. **Strength reduction** -- replaces expensive operations (multiply by power-of-2 becomes shift)
- **28 standard library modules** are available (up from 22)
- **Module resolution** is functional for `use` imports

These MIR-level optimizations run before the backend, improving both Cranelift debug builds and LLVM release builds. The LLVM release path now benefits from Kryos optimizations stacked on top of LLVM's own -O2 pipeline.

### Remaining Limitations
- **Integer literal type inference** still requires explicit `as i64` casts in some contexts
- **Incremental compilation** is not yet implemented
- **Cross-compilation** is not yet supported

### Projected Performance at v1.0
- Compilation speed advantage will **grow** with larger programs (Cranelift's advantage scales with code size)
- Runtime performance will approach Rust-level once LLVM release backend is activated
- Binary sizes will grow modestly as stdlib is linked, but should stay under Go's

## Comparison with Mojo

Mojo (Modular) is not installed on this system. Based on published benchmarks:

| Metric | Kryos (projected) | Mojo |
|--------|-------------------|------|
| Compile speed | Fast (Cranelift) | Moderate (MLIR/LLVM) |
| Runtime speed | Fast (LLVM release) | Very fast (MLIR optimizations) |
| Memory model | Ownership + ARC | Ownership + value semantics |
| AI integration | Built-in (planned) | First-class (MAX platform) |
| Availability | Open development | Proprietary, limited access |

Kryos and Mojo target similar niches (systems + AI) but with different approaches:
- Mojo builds on Python compatibility and MLIR for GPU/TPU optimization
- Kryos builds on Rust-like safety with Cranelift for compilation speed and a capability-based security model

## Raw Data

```
Test 1 — Hello World (10 runs averaged):
  Kryos: 503ms compile, 105KB binary
  Rust:  612ms compile, 117KB binary
  Go:    721ms compile, 1562KB binary

Test 2 — Type Check Only (10 runs averaged):
  Kryos check: 462ms
  Rust check:  535ms
  Go vet:      1616ms
```
