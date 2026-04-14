# Kryos

A compiled systems language with ownership-based memory safety, capability enforcement, and dual-backend native compilation.

**v0.3.3** -- 925+ tests passing, self-hosting compiler, zero known issues.

---

## Why Kryos?

Kryos gives you the control of C, the safety of Rust, and the clarity of Go -- without lifetime annotations. The ownership model is ARC-based with move semantics enforced at compile time. No borrow checker. No `'a` annotations. You get memory safety by construction, not by wrestling with the compiler.

---

## Features

- **Ownership without lifetimes** -- ARC move semantics enforced at compile time. No lifetime annotations.
- **Capability-safe functions** -- `@capabilities` and `@pure` annotations enable deny-by-default resource access, checked at compile time.
- **Dual-backend compilation** -- Cranelift for fast dev builds (~500ms), LLVM for optimized release binaries.
- **Self type in traits** -- `Self` resolves to the implementing type in trait method signatures.
- **Associated functions** -- `Type::method(args)` syntax for constructors and static dispatch.
- **Pattern matching** -- Enums with typed payloads, destructuring, exhaustive match with non-exhaustive warnings.
- **Structs with methods** -- `impl` blocks, `impl Trait for Type`, nested structs, tracked Drop.
- **Closures and higher-order functions** -- First-class closures with ARC-managed capture environments.
- **Channels and actors** -- `chan()`, `spawn`, `send`, `recv` for structured concurrency.
- **Full toolchain** -- REPL with persistent state, LSP, formatter, doc generator, test runner, package manager, C bindgen.
- **Self-hosting compiler** -- 19K lines of Kryos reimplementing the full compilation pipeline.
- **@pure optimization** -- CSE and dead call elimination for pure functions.
- **@test runner** -- Discover and JIT-execute `@test` annotated functions with `kryos test`.
- **28 stdlib modules** -- 847 functions covering strings, math, collections, I/O, JSON, crypto, regex, datetime, HTTP, tensors, AI runtime.

---

## Install

### Linux / macOS

```bash
curl -fsSL https://raw.githubusercontent.com/FrostbyteDevTeam/kryos-lang/master/install.sh | bash
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/FrostbyteDevTeam/kryos-lang/master/install.ps1 | iex
```

### Build from Source

Requirements: Rust 1.75+, LLVM 15+ (optional, for release builds)

```bash
git clone https://github.com/FrostbyteDevTeam/kryos-lang.git
cd kryos-lang/compiler
cargo build --release -j 4
./target/release/kryos run examples/proof.kry
```

> Note: debug builds use ~48 GB RAM. Always build with `--release -j 4`.

---

## Quick Start

```kryos
fn main() {
    println("Hello, Kryos!")
}
```

```bash
kryos run hello.kry
```

---

## Language Tour

### Enums and Pattern Matching

```kryos
enum Shape {
    Circle(f64),
    Rectangle(f64, f64),
    Point,
}

fn area(s: Shape) -> f64 {
    match s {
        Shape::Circle(r)       => 3.14159 * r * r,
        Shape::Rectangle(w, h) => w * h,
        Shape::Point           => 0.0,
    }
}

fn main() {
    let c = Shape::Circle(12.0)
    println("area = " + to_string(area(c)))
}
```

### Traits with Self Type

```kryos
trait Comparable {
    fn less_than(self, other: Self) -> bool
}

struct Score { value: i64 }

impl Score {
    fn new(v: i64) -> Score { Score { value: v } }
}

impl Comparable for Score {
    fn less_than(self, other: Self) -> bool {
        self.value < other.value
    }
}

fn main() {
    let a = Score::new(10)
    let b = Score::new(20)
    if a.less_than(b) { println("a < b") }
}
```

### Closures and Channels

```kryos
fn main() {
    let ch = chan(i64)
    spawn {
        let sum = 0
        for i in 1..11 { sum = sum + i }
        send(ch, sum)
    }
    let result = recv(ch)
    println("sum(1..10) = " + to_string(result))
}
```

### Capability Enforcement

```kryos
@capabilities(io)
fn read_config(path: str) -> str {
    file_read(path)
}

@pure
fn hash(data: str) -> i64 {
    // compile error if this calls io/net/process
    compute_hash(data)
}
```

### Error Handling

```kryos
fn parse_port(s: str) -> i64 {
    try {
        let n = parse_int(s)
        if n < 1 or n > 65535 { throw "port out of range" }
        n
    } catch e {
        println("error: " + e)
        8080
    }
}
```

---

## Performance

Both backends produce native machine code with no GC pauses.

| Benchmark       | Kryos (Cranelift) | Kryos (LLVM) | Notes                        |
|-----------------|-------------------|--------------|------------------------------|
| Compile hello   | ~500ms            | ~2s          | From source, cold cache      |
| fib(40) loop    | ~1.1s             | ~0.6s        | Iterative, no JIT warmup     |
| ARC alloc/free  | ~80ns/op          | ~55ns/op     | Per heap value round-trip    |

LLVM release mode performance matches equivalent Rust at -O2.

---

## Project Structure

```
kryos-lang/
  compiler/
    crates/          21 Rust crates (~50k lines)
    stdlib/          28 stdlib modules (847 functions)
    self-host/       19k-line Kryos self-host (15 files)
    examples/        14 runnable example programs
  docs/              15-chapter language manual + grammar
  editors/           VS Code extension
  benchmarks/        Criterion benchmark suite
  install.sh         Linux/macOS installer
  install.ps1        Windows installer
```

### Compiler Crates

| Crate | Purpose |
|-------|---------|
| `kryos-cli` | Command-line entry point (`run`, `check`, `fmt`, `test`, `repl`, `doc`, `pkg`, `bindgen`) |
| `kryos-lexer` | Tokenizer |
| `kryos-parser` | Recursive-descent parser with Pratt expression parsing |
| `kryos-ast` | AST node definitions |
| `kryos-types` | Type checker with inference, generics, Self type, associated functions |
| `kryos-ownership` | Move tracking, use-after-move detection |
| `kryos-capabilities` | Compile-time capability enforcement |
| `kryos-mir` | Mid-level IR -- SSA, basic blocks, monomorphization, @pure CSE |
| `kryos-codegen-cranelift` | Cranelift native backend (fast dev builds) |
| `kryos-codegen-llvm` | LLVM IR backend (optimized release builds) |
| `kryos-linker` | Native binary linking |
| `kryos-driver` | Compilation pipeline and module resolution |
| `kryos-rt` | Runtime -- strings, arrays, maps, channels, spawn, ARC |
| `kryos-stdlib-native` | Native stdlib -- process, file I/O, terminal |
| `kryos-lsp` | Language Server Protocol implementation |
| `kryos-fmt` | Code formatter |
| `kryos-doc` | Documentation generator |
| `kryos-bindgen` | C header to Kryos binding generator |
| `kryos-package` | Package manager with dependency resolution |
| `kryos-test-runner` | @test function discovery and JIT execution |
| `kryos-errors` | Structured diagnostic reporting with error codes |

---

## Toolchain Commands

```
kryos run <file.kry>         Compile and run
kryos check <file.kry>       Type-check without running
kryos fmt <file.kry>         Format in place
kryos test <file.kry>        Discover and run @test functions
kryos repl                   Interactive REPL with persistent state
kryos doc <file.kry>         Generate HTML documentation
kryos pkg init               Scaffold a new package
kryos pkg add <name>         Add a dependency
kryos pkg build              Build the current package
kryos bindgen <header.h>     Generate Kryos bindings from C header
```

---

## Documentation

- [Getting Started](docs/01-getting-started.md)
- [Variables and Types](docs/02-variables-and-types.md)
- [Functions](docs/03-functions.md)
- [Control Flow](docs/04-control-flow.md)
- [Structs and Enums](docs/05-structs-and-enums.md)
- [Ownership](docs/06-ownership.md)
- [Error Handling](docs/07-error-handling.md)
- [Traits and Generics](docs/08-traits-and-generics.md)
- [Concurrency](docs/09-concurrency.md)
- [Capabilities](docs/10-capabilities.md)
- [Comptime](docs/11-comptime.md)
- [Modules and Packages](docs/12-modules-and-packages.md)
- [FFI](docs/13-ffi.md)
- [AI Runtime](docs/14-ai-runtime.md)
- [Codegen](docs/15-codegen.md)
- [Grammar Reference](docs/grammar.md)
- [Why Kryos](docs/WHY_KRYOS.md)

---

## Status

Kryos is **v0.3.3**. The core language is complete and production-capable for systems programming.

| Feature | Status |
|---------|--------|
| Type system | Complete |
| Ownership / ARC | Complete |
| Generics + monomorphization | Complete |
| Traits with Self type | Complete |
| Associated functions (::) | Complete |
| Pattern matching | Complete |
| Closures | Complete |
| Channels + spawn | Complete |
| Capability enforcement | Complete |
| @pure / @test / @copy | Complete |
| Cranelift backend | Complete |
| LLVM backend | Complete |
| Module system | Complete |
| Package manager | Complete |
| LSP | Complete |
| REPL | Complete |
| Self-hosting compiler | Stage-1 complete |
| 3-stage bootstrap | Verified |

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

---

## License

Proprietary. Copyright (c) 2026 FrostByte Digital. All rights reserved.
