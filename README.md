# Kryos

A compiled systems language with ownership-based memory safety, compile-time capability annotations, and a native AI runtime.

## Key Features

- **Ownership without lifetimes** -- move semantics and borrowing enforced at compile time, no lifetime annotations
- **Cranelift compilation** -- fast native code generation via Cranelift, with an LLVM backend in development
- **AI-native primitives** -- tensors, agents, probability types, and reactive streams as language-level constructs
- **Capability annotations** -- functions declare required system resources via `@capabilities`
- **Compile-time evaluation** -- `comptime` blocks evaluate arbitrary expressions during compilation
- **Full toolchain** -- LSP, formatter, doc generator, package manager, test runner, REPL, C bindgen

## Quick Start

```bash
git clone https://github.com/FrostbyteDevTeam/kryos-lang.git
cd kryos-lang/compiler && cargo build --release -j 4
cargo run --release -- run ../examples/demo.kry
```

Hello world:

```
fn main() {
    println("Hello, Kryos!")
}
```

## Architecture

```
Source (.kry)
  |
  v
Lexer .......... 80+ token types, string interpolation
  |
  v
Parser ......... Recursive descent, Pratt expression parsing
  |
  v
Type Checker ... Inference, validation, capability tracking
  |
  v
Ownership ...... Move tracking, use-after-move detection
  |
  v
Capabilities ... Resource annotation tracking
  |
  v
MIR ............ SSA basic blocks, monomorphization
  |
  +-- Optimization passes: constant folding, DCE, inlining, TCO, strength reduction
  |
  +---> Cranelift codegen (native binaries + JIT)
  +---> LLVM codegen (in development)
  +---> LSP server (diagnostics, hover, completion)
```

### 21 Compiler Crates

| Crate | Purpose |
|-------|---------|
| `kryos-cli` | CLI entry point |
| `kryos-lexer` | Tokenizer |
| `kryos-parser` | Recursive descent parser |
| `kryos-ast` | AST node types |
| `kryos-types` | Type checker with inference |
| `kryos-ownership` | Move tracking, use-after-move detection |
| `kryos-capabilities` | Capability annotation tracking |
| `kryos-mir` | Mid-level IR (SSA, basic blocks, monomorphization) |
| `kryos-codegen-cranelift` | Cranelift native backend |
| `kryos-codegen-llvm` | LLVM IR backend (in development) |
| `kryos-linker` | Native binary linking |
| `kryos-driver` | Compilation pipeline orchestration |
| `kryos-rt` | Runtime (strings, arrays, maps, tensors, channels, spawn) |
| `kryos-stdlib-native` | Native stdlib (process, file I/O, terminal) |
| `kryos-lsp` | Language Server Protocol |
| `kryos-fmt` | Code formatter |
| `kryos-doc` | Documentation generator |
| `kryos-bindgen` | C header to Kryos binding generator |
| `kryos-package` | Package manager |
| `kryos-test-runner` | Test framework |
| `kryos-errors` | Diagnostic reporting |

## Performance

| Benchmark | Kryos (Cranelift) | Rust (release) | Go |
|-----------|-------------------|----------------|----|
| fib(42) | 1190ms | 594ms | 1094ms |
| Sum 100M | 46ms | 5ms | 27ms |
| Nested 1Kx1K | 5ms | 5ms | 6ms |
| Compilation | ~500ms | ~600ms | ~720ms |

Five MIR-level optimization passes (constant folding, dead code elimination, function inlining, tail-call optimization, strength reduction) bring Cranelift debug builds within striking distance. An LLVM backend is in development for optimized release builds.

## Toolchain

```
kryos build <file>            Compile to native binary (Cranelift)
kryos run <file>              Compile and execute
kryos check <file>            Type-check without codegen
kryos test                    Run project tests
kryos repl                    Interactive REPL
kryos fmt <file>              Format source code
kryos lsp                     Start language server
kryos doc <file>              Generate documentation
kryos bindgen <header>        Generate Kryos bindings from C headers
kryos pkg init                Create new project
kryos pkg add <dep>           Add dependency
kryos version                 Print version info
```

## Language Overview

```
let x: i64 = 42                            // Immutable by default
let mut counter = 0                         // Explicit mutability

fn fib(n: i64) -> i64 {                    // First-class functions
    if n <= 1 { return n }
    return fib(n - 1) + fib(n - 2)
}

struct Point { x: f64, y: f64 }            // Structs with impl blocks
impl Point {
    fn magnitude(self: Point) -> f64 {
        return (self.x * self.x + self.y * self.y) ** 0.5
    }
}

trait Printable { fn display(self: Self) -> str }   // Traits

match direction {                           // Pattern matching
    "north" => go_up()
    _ => stay()
}

try { throw "error" } catch e { println(e) }       // Error handling

let ch = chan()                             // Channels + spawn
spawn { send(ch, 42) }
let value = recv(ch)

let size = comptime { 2 * 2 * 2 * 2 }     // Compile-time evaluation

@capabilities(net, io)                      // Capability annotations
fn download(url: str, path: str) { }
```

## Standard Library

28 modules covering strings, math, collections, I/O, networking, cryptography, JSON, regex, datetime, terminal, process management, tensors, agents, probability types, reactive streams, data lineage, and cost tracking. All 847 functions are implemented -- zero stubs.

See the [language manual](docs/README.md) for complete documentation.

## Examples

| File | Demonstrates |
|------|-------------|
| [`demo.kry`](examples/demo.kry) | Recursion, higher-order functions, float math, error handling, maps, structs, arrays, strings |
| [`http_server.kry`](examples/http_server.kry) | Structs, string match, error handling, request routing |
| [`pipeline.kry`](examples/pipeline.kry) | Channels, spawn, concurrency, data processing pipeline |
| [`fibonacci_showcase.kry`](examples/fibonacci_showcase.kry) | Recursion, tail-call optimization, comptime, higher-order functions |
| [`calculator.kry`](examples/calculator.kry) | String match, tail expressions, function composition |
| [`word_count.kry`](examples/word_count.kry) | String operations, for loops, builtins |
| [`json_counter.kry`](examples/json_counter.kry) | Structs, channels, spawn, try/catch, integer match |
| [`mini_grep.kry`](examples/mini_grep.kry) | File I/O, error handling, string search |
| [`all_features.kry`](examples/all_features.kry) | Comprehensive showcase of all language features |

## Self-Hosting (In Progress)

An 18,700-line self-hosted compiler written entirely in Kryos lives in `compiler/self-host/`. It implements the complete pipeline: lexer, parser, type checker, MIR lowering, 5-pass optimizer, register allocator, x86_64 machine code emission, and ELF/COFF linking -- with zero external dependencies beyond the OS kernel.

The self-host requires a module/import system to compile as a multi-file project. This is currently in development. The bootstrap verification follows the same 3-stage technique used by GCC, Rust, and Go:

```
stage-0 (Rust/Cranelift) -> stage-1 binary
stage-1 (Kryos)          -> stage-2 binary
stage-2 (Kryos)          -> stage-3 binary
stage-2 == stage-3        -> compiler faithfully reproduces itself
```

## Status

**v0.2.0** -- 21-crate Rust compiler with 689 passing tests and zero clippy warnings. Core language features are fully implemented: type inference, ownership analysis, pattern matching, generics, `dyn Trait`, `comptime`, concurrency primitives, and FFI. The Cranelift backend produces working native binaries on x86_64.

### What Works

- All core language features (structs, enums, traits, generics, closures, channels, try/catch)
- Native binary compilation via Cranelift
- Full toolchain: formatter, type-checker, doc generator, test runner, REPL, LSP, C bindgen
- 28 stdlib modules with 847 real implementations
- Ownership-based move tracking for struct types
- Rust-quality error diagnostics

### Known Limitations

- LLVM backend is in development (Cranelift-only for now)
- Module/import system is in development (single-file compilation only)
- Capability annotations are tracked but not yet enforced at compile time
- Error handling (`throw`) does not propagate across function boundaries
- Self-hosted compiler requires the module system to compile

### Roadmap

- Module system and multi-file compilation
- Cross-function error propagation
- LLVM release backend
- Capability enforcement
- Complete self-hosting bootstrap verification
- Async runtime with structured concurrency
- Package registry
- Incremental compilation

## Requirements

- Rust 1.75+ (to build the compiler)
- LLVM 15+ (optional, for future release builds)

## License

Proprietary. Copyright FrostByte Digital. All rights reserved.

---

Built by [FrostByte Digital](https://frostbytedigital.io)
