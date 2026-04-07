# Kryos

A compiled systems language with ownership-based memory safety, compile-time capability enforcement, and a native AI runtime.

## Key Features

- **Ownership without lifetimes** -- move semantics and borrowing enforced at compile time, no lifetime annotations
- **Dual-backend compilation** -- Cranelift for fast development builds, LLVM for optimized release binaries
- **AI-native primitives** -- tensors, agents, probability types, and reactive streams as language-level constructs
- **Capability-based security** -- functions declare required system resources; the compiler enforces deny-by-default access
- **Compile-time evaluation** -- `comptime` blocks evaluate arbitrary expressions during compilation
- **Full toolchain** -- LSP, formatter, doc generator, package manager, test runner, REPL

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
Capabilities ... Deny-by-default resource enforcement
  |
  v
MIR ............ SSA basic blocks, monomorphization
  |
  +-- Optimization passes: constant folding, DCE, inlining, TCO, strength reduction
  |
  +---> Cranelift JIT (fast debug builds)
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
| `kryos-types` | Type checker with inference |
| `kryos-ownership` | Move tracking, use-after-move detection |
| `kryos-capabilities` | Compile-time capability enforcement |
| `kryos-mir` | Mid-level IR (SSA, basic blocks, monomorphization) |
| `kryos-codegen-cranelift` | Cranelift JIT backend |
| `kryos-codegen-llvm` | LLVM IR backend |
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

| Benchmark | Kryos (Cranelift) | Kryos (LLVM) | Rust (release) | Go |
|-----------|-------------------|--------------|----------------|----|
| fib(42) | 1190ms | 594ms | 594ms | 1094ms |
| Sum 100M | 46ms | ~5ms | 5ms | 27ms |
| Nested 1Kx1K | 5ms | 5ms | 5ms | 6ms |
| Compilation | ~500ms | -- | ~600ms | ~720ms |

LLVM release builds match Rust. Five MIR-level optimization passes (constant folding, dead code elimination, function inlining, tail-call optimization, strength reduction) improve debug builds and compound with LLVM's own optimizations in release mode.

## Toolchain

```
kryos build <file>            Compile to native binary
kryos build <file> --release  Compile with LLVM optimizations
kryos run <file>              Compile and execute
kryos check <file>            Type-check without codegen
kryos test                    Run project tests
kryos repl                    Interactive REPL
kryos fmt <file>              Format source code
kryos lsp                     Start language server
kryos doc <file>              Generate documentation
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
    fn magnitude(self: Point) -> f64 { return sqrt(self.x * self.x + self.y * self.y) }
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

@capabilities(net, io)                      // Capability-based security
fn download(url: str, path: str) { }
```

## Standard Library

28 modules covering strings, math, collections, I/O, networking, cryptography, JSON, regex, datetime, terminal, process management, tensors, agents, probability types, reactive streams, data lineage, and cost tracking.

See the [language manual](docs/README.md) for complete documentation.

## Examples

| File | Demonstrates |
|------|-------------|
| [`demo.kry`](examples/demo.kry) | Recursion, higher-order functions, float math, error handling, maps, tensors |
| [`neural_net.kry`](examples/neural_net.kry) | Two-layer neural network forward pass with native tensor runtime |
| [`http_server.kry`](examples/http_server.kry) | Structs, match expressions, error handling, request routing |
| [`pipeline.kry`](examples/pipeline.kry) | Channels, spawn, concurrency, data processing pipeline |
| [`fibonacci_showcase.kry`](examples/fibonacci_showcase.kry) | Recursion, tail-call optimization, comptime, higher-order functions |

## Status

**v0.1.0** -- The compiler is functional with 680+ passing tests. All core language features are implemented: type inference, ownership analysis, capability checking, pattern matching, generics with monomorphization, `dyn Trait`, `comptime`, concurrency primitives, and FFI.

### Roadmap

- Self-hosting: rewrite the compiler frontend in Kryos
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
