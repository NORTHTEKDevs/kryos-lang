# Kryos

A compiled systems language with ownership-based memory safety, compile-time capability enforcement, and a native AI runtime.

## Key Features

- **Ownership without lifetimes** -- move semantics and borrowing enforced at compile time, no lifetime annotations
- **Dual-backend compilation** -- Cranelift for fast development builds, LLVM for optimized release binaries
- **AI-native primitives** -- tensors, agents, probability types, and reactive streams as language-level constructs
- **Capability-based security** -- functions declare required system resources; the compiler enforces deny-by-default access within annotated scopes
- **Compile-time evaluation** -- `comptime` blocks evaluate arbitrary expressions during compilation
- **Module system** -- `use` imports with stdlib resolution, selective imports, transitive dependencies, and diamond deduplication
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
Type Checker ... Inference, generics, monomorphization
  |
  v
Ownership ...... Move tracking, use-after-move detection
  |
  v
Capabilities ... Deny-by-default resource enforcement
  |
  v
MIR ............ SSA basic blocks, optimization passes
  |
  +-- Constant folding, DCE, inlining, TCO, strength reduction
  |
  +---> Cranelift codegen (fast native binaries)
  +---> LLVM codegen (optimized release builds)
  +---> LSP server (diagnostics, hover, completion)
```

### 21 Compiler Crates

| Crate | Purpose |
|-------|---------|
| `kryos-cli` | CLI entry point |
| `kryos-lexer` | Tokenizer |
| `kryos-parser` | Recursive descent parser |
| `kryos-ast` | AST node types |
| `kryos-types` | Type checker with inference and generics |
| `kryos-ownership` | Move tracking, use-after-move detection |
| `kryos-capabilities` | Compile-time capability enforcement |
| `kryos-mir` | Mid-level IR (SSA, basic blocks, monomorphization) |
| `kryos-codegen-cranelift` | Cranelift native backend |
| `kryos-codegen-llvm` | LLVM IR backend |
| `kryos-linker` | Native binary linking |
| `kryos-driver` | Compilation pipeline and module resolution |
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

| Benchmark | Kryos (Cranelift) | Kryos (LLVM) | Rust (release) | Go |
|-----------|-------------------|--------------|----------------|----|
| fib(42) | 1190ms | 594ms | 594ms | 1094ms |
| Sum 100M | 46ms | ~5ms | 5ms | 27ms |
| Nested 1Kx1K | 5ms | 5ms | 5ms | 6ms |
| Compilation | ~500ms | -- | ~600ms | ~720ms |

LLVM release builds match Rust. Five MIR-level optimization passes (constant folding, dead code elimination, function inlining, tail-call optimization, strength reduction) improve debug builds and compound with LLVM's own optimizations in release mode.

## Toolchain

```
kryos build <file>            Compile to native binary (Cranelift)
kryos build <file> --release  Compile with LLVM optimizations
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

use std::math::{abs, min, max}             // Module imports

@capabilities(net, io)                      // Capability-based security
fn download(url: str, path: str) { }
```

## Module System

Kryos supports file-based modules with `use` imports:

```
use mylib                           // Import sibling file mylib.kry
use utils::helpers                  // Import utils/helpers.kry
use std::math::{abs, min, max}     // Selective stdlib import
use std::json                       // Full stdlib module import
```

The module resolver supports transitive imports, diamond deduplication, and cycle detection. 28 stdlib modules are available via the `std::` prefix.

## Capability-Based Security

Functions annotated with `@capabilities` are subject to compile-time resource enforcement:

```
@capabilities(io)
fn save(data: str) {
    file_write("output.txt", data)      // OK -- has io capability
}

@capabilities(net)
fn fetch() {
    file_write("output.txt", "sneaky")  // COMPILE ERROR -- net != io
}
```

35 builtin functions are mapped to 7 capability categories (io, net, process, term, crypto, time, ffi). Functions without `@capabilities` annotations have ambient authority and are unconstrained.

## Standard Library

28 modules covering strings, math, collections, I/O, networking, cryptography, JSON, regex, datetime, terminal, process management, tensors, agents, probability types, reactive streams, data lineage, and cost tracking. All 847 functions are implemented -- zero stubs.

See the [language manual](docs/README.md) for complete documentation.

## Examples

| File | Demonstrates |
|------|-------------|
| [`demo.kry`](examples/demo.kry) | Recursion, higher-order functions, float math, error handling, maps, structs, arrays, strings |
| [`imports_demo.kry`](examples/imports_demo.kry) | Multi-file imports with `use mylib` |
| [`math_imports_demo.kry`](examples/math_imports_demo.kry) | Selective stdlib imports with `use std::math::{abs, min, max}` |
| [`http_server.kry`](examples/http_server.kry) | Structs, string match, error handling, request routing |
| [`pipeline.kry`](examples/pipeline.kry) | Channels, spawn, concurrency, data processing pipeline |
| [`fibonacci_showcase.kry`](examples/fibonacci_showcase.kry) | Recursion, tail-call optimization, comptime, higher-order functions |
| [`calculator.kry`](examples/calculator.kry) | String match, tail expressions, function composition |
| [`word_count.kry`](examples/word_count.kry) | String operations, for loops, builtins |
| [`json_counter.kry`](examples/json_counter.kry) | Structs, channels, spawn, try/catch, integer match |
| [`mini_grep.kry`](examples/mini_grep.kry) | File I/O, error handling, string search |
| [`all_features.kry`](examples/all_features.kry) | Comprehensive showcase of all language features |

## Self-Hosting

An 18,700-line self-hosted compiler written entirely in Kryos lives in `compiler/self-host/` (15 files + runtime). It implements the complete pipeline: lexer, parser, type checker, MIR lowering, 5-pass optimizer, register allocator, x86_64 machine code emission, and ELF/COFF linking -- with zero external dependencies beyond the OS kernel.

The concatenated self-host type-checks cleanly through the Rust compiler (0 errors). The stage-1 binary (1MB PE32+ on Windows) compiles and runs, successfully parsing and tokenizing Kryos source files.

Bootstrap verification follows the same 3-stage technique used by GCC, Rust, and Go:

```
stage-0 (Rust/Cranelift) -> stage-1 binary
stage-1 (Kryos)          -> stage-2 binary
stage-2 (Kryos)          -> stage-3 binary
stage-2 == stage-3        -> compiler faithfully reproduces itself
```

## Status

**v0.2.0** -- 21-crate Rust compiler with 748 passing tests and zero clippy warnings. Core language features are fully implemented: type inference, ownership analysis, generics with monomorphization, `dyn Trait`, `comptime`, pattern matching, concurrency primitives, cross-function error propagation, module imports, and capability enforcement. Dual backends: Cranelift for fast builds, LLVM for optimized release binaries. The self-hosted compiler type-checks cleanly and produces a working stage-1 native binary.

### Roadmap

- Complete self-hosting bootstrap verification (stage-2 == stage-3)
- Namespace-scoped module imports (`use math as m; m.abs(-5)`)
- Async runtime with structured concurrency
- GPU compute backend via capability annotations
- Package registry
- Incremental compilation

## Requirements

- Rust 1.75+ (to build the compiler)
- LLVM 15+ (optional, for release builds)

## License

Proprietary. Copyright FrostByte Digital. All rights reserved.

---

Built by [FrostByte Digital](https://frostbytedigital.io)
