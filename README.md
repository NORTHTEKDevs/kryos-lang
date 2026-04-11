# Kryos

A compiled systems language with ownership-based memory safety, capability enforcement, and dual-backend native compilation.

---

## Features

- **Ownership without lifetimes** -- ARC-based move semantics enforced at compile time, no borrow checker or lifetime annotations.
- **Capability-safe functions** -- `@capabilities` and `@pure` annotations enable deny-by-default resource access, checked at compile time.
- **Dual-backend compilation** -- Cranelift for fast JIT/debug builds, LLVM for optimized AOT release binaries.
- **Pattern matching** -- Enums with typed payloads, destructuring, and exhaustive match expressions.
- **Structs with methods** -- `impl` blocks, nested structs, and field access with ownership-tracked Drop.
- **Closures and higher-order functions** -- First-class function values, lambdas, and function parameters.
- **Channels and actors** -- `chan()`, `spawn`, `send`, and `recv` for structured concurrency.
- **Full toolchain** -- REPL, LSP, formatter, documentation generator, test runner, package manager, and C bindgen.
- **Self-hosting compiler** -- 19K lines of Kryos reimplementing the full compilation pipeline (work in progress).

## Quick Start

### Install

```bash
git clone https://github.com/FrostbyteDevTeam/kryos-lang.git
cd kryos-lang/compiler
cargo build --release -j 4
```

### Hello World

Create a file called `hello.kry`:

```kryos
fn main() {
    println("Hello, Kryos!")
}
```

Run it:

```bash
cargo run --release -- run hello.kry
```

### Run the proof program

```bash
cargo run --release -- run examples/proof.kry
```

## Code Example

Enums with typed payloads, pattern matching, and functions calling functions:

```kryos
enum Shape {
    Circle(f64),
    Rectangle(f64, f64),
    Point,
}

fn area(s: Shape) -> f64 {
    match s {
        Shape::Circle(r) => 3.14159 * r * r,
        Shape::Rectangle(w, h) => w * h,
        Shape::Point => 0.0,
    }
}

fn classify_area(a: f64) -> str {
    if a > 100.0 {
        return "large"
    } elif a > 10.0 {
        return "medium"
    } else {
        return "small"
    }
}

fn main() {
    let c = Shape.Circle(12.0)
    let circle_area = area(c)
    let size = classify_area(circle_area)
    println("circle(12) is " + size)
}
```

## Project Status

Kryos is **v0.2.0 alpha**. The core language is functional and the compiler passes 690+ Rust tests and 132 `.kry` end-to-end test files. Ownership analysis, type inference, generics with monomorphization, pattern matching, concurrency primitives, and capability enforcement all work. Both Cranelift and LLVM backends produce native binaries.

This is pre-1.0 software. The language design, standard library surface, and toolchain interfaces may change without notice. It is not yet suitable for production use.

The self-hosting compiler (19K lines of Kryos in `compiler/self-host/`) produces a working stage-1 native binary. Three-stage bootstrap verification is in progress.

## Project Structure

The compiler is organized as 21 Rust crates under `compiler/crates/`:

| Crate | Purpose |
|-------|---------|
| `kryos-cli` | Command-line entry point |
| `kryos-lexer` | Tokenizer |
| `kryos-parser` | Recursive-descent parser with Pratt expression parsing |
| `kryos-ast` | AST node definitions |
| `kryos-types` | Type checker with inference and generics |
| `kryos-ownership` | Move tracking, use-after-move detection |
| `kryos-capabilities` | Compile-time capability enforcement |
| `kryos-mir` | Mid-level IR -- SSA, basic blocks, monomorphization |
| `kryos-codegen-cranelift` | Cranelift native backend (JIT/debug) |
| `kryos-codegen-llvm` | LLVM IR backend (AOT/release) |
| `kryos-linker` | Native binary linking |
| `kryos-driver` | Compilation pipeline and module resolution |
| `kryos-rt` | Runtime -- strings, arrays, maps, channels, spawn |
| `kryos-stdlib-native` | Native stdlib -- process, file I/O, terminal |
| `kryos-lsp` | Language Server Protocol implementation |
| `kryos-fmt` | Code formatter |
| `kryos-doc` | Documentation generator |
| `kryos-bindgen` | C header to Kryos binding generator |
| `kryos-package` | Package manager with dependency resolution |
| `kryos-test-runner` | Test framework |
| `kryos-errors` | Diagnostic reporting with structured error codes |

## Documentation

- [Language Manual](docs/README.md) -- complete reference for the Kryos language
- [Getting Started](docs/01-getting-started.md) -- installation and first program
- [Structs and Enums](docs/05-structs-and-enums.md) -- data types with ownership
- [Ownership](docs/06-ownership.md) -- move semantics and memory safety
- [Capabilities](docs/10-capabilities.md) -- compile-time resource enforcement
- [Concurrency](docs/09-concurrency.md) -- channels, spawn, and actors
- [Modules and Packages](docs/12-modules-and-packages.md) -- the module system and package manager
- [Grammar](docs/grammar.md) -- formal grammar specification

## Requirements

- Rust 1.75+ (to build the compiler)
- LLVM 15+ (optional, for release builds)

## License

Proprietary. Copyright (c) 2026 FrostByte Digital. All rights reserved.
