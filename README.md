# Kryos

[![License: Apache 2.0](https://img.shields.io/badge/license-Apache_2.0-blue.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-843%20passing-brightgreen.svg)](#status)
[![Release](https://img.shields.io/badge/release-v1.9.0-orange.svg)](CHANGELOG.md)
[![Targets](https://img.shields.io/badge/targets-native%20%7C%20wasm-purple.svg)](#targets)

A compiled, general-purpose systems language with ownership-based memory safety, capability enforcement, and three codegen targets: Cranelift (fast dev), LLVM (optimized native), and WebAssembly (browser/WASI). Solo-built with AI assistance.

**v1.9.0** — LLVM backend now production-ready. `kryos build --release` produces native code that **matches Rust `--release`** and beats Go on CPU-bound workloads. See [BENCHMARKS.md](BENCHMARKS.md) for honest head-to-head numbers across 6 benchmarks and 7 language/backend combinations.

**v1.1.0** -- 843 tests passing + new WebAssembly backend. Kryos programs now compile to native binaries *and* to `.wasm` modules that run in browsers and WASI hosts. The TCP stack no longer serializes connections through a global mutex, so spawned worker threads can handle requests concurrently.

---

## Why Kryos?

Kryos gives you the control of C, the safety of Rust, and the clarity of Go -- without lifetime annotations. The ownership model is ARC-based with move semantics enforced at compile time. No borrow checker. No `'a` annotations. You get memory safety by construction, not by wrestling with the compiler.

It's also designed to be a language you can *actually finish things in*. The standard library covers strings, math, collections, JSON, HTTP, regex, datetime, crypto, file I/O, processes, channels, tensors, and an AI runtime out of the box. Twenty-eight modules, 847 functions, no third-party packages required.

---

## Targets

v1.1.0 ships three codegen backends behind a single CLI flag:

| Backend | Use when | Speed |
|---|---|---|
| `cranelift` (default) | Dev loop, JIT, quick rebuilds | ~500ms cold |
| `llvm` | Release binaries, max throughput | optimized |
| `wasm` (new) | Browser, WASI, edge, sandboxed exec | portable |

```
kryos build --backend wasm program.kry
# -> program.wasm, runs in any browser or wasmtime
```

See `examples/wasm_browser_demo.html` for a complete browser demo.

WASM v0.1 scope: integers, floats, booleans, if/else/elif chains, while loops,
functions, recursion, direct calls, and `println` via host imports. Full feature
parity (strings, arrays, structs, channels, HTTP, regex, JSON) tracks the LLVM
backend and is planned for v1.2+.

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

If you just want a working compiler in 5 minutes, follow [QUICKSTART.md](QUICKSTART.md).


### Linux / macOS

```bash
curl -fsSL https://raw.githubusercontent.com/NORTHTEKDevs/kryos-lang/master/install.sh | bash
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/NORTHTEKDevs/kryos-lang/master/install.ps1 | iex
```

### Build from Source

Requirements: Rust 1.75+, a C compiler (`cc`/`clang`/MSVC) for linking final binaries. **LLVM is not required** -- the LLVM backend emits IR as text, so only the optional `kryos build --backend llvm` path needs `llc` or `clang` on PATH.

```bash
git clone https://github.com/NORTHTEKDevs/kryos-lang.git
cd kryos-lang/compiler
cargo build --release -j 2
./target/release/kryos run ../examples/hello.kry
```

### Via `cargo install` (from a local checkout)

If you already have the Rust toolchain and want the `kryos` binary on your `PATH` without the install script:

```bash
git clone https://github.com/NORTHTEKDevs/kryos-lang.git
cargo install --path kryos-lang/compiler/crates/kryos-cli
# kryos is now on $PATH (typically ~/.cargo/bin/kryos)
kryos --version
```

Note: `cargo install --git` is not currently supported because the workspace ships runtime staticlibs (`libkryos_rt.a`, `libkryos_stdlib_native.a`) that the driver looks up at link time. Use the install script or a release archive if you want those bundled — or use `cargo install --path` from a local checkout and the compiler will rebuild and locate them inside the workspace `target/` automatically.

Build footprint on a typical machine, cold from a clean checkout with the workspace's tuned `[profile.release]`:

| Metric            | -j 2 (low-RAM laptop) | -j N (full core count) |
|-------------------|-----------------------|------------------------|
| Wall time         | ~4-5 min              | ~1-2 min               |
| Peak RAM          | ~2-3 GB               | ~1-2 GB per parallel job |
| `target/` on disk | **~700 MB**           | ~700 MB                |
| `kryos` binary    | ~14 MB                | ~14 MB                 |

`cranelift-codegen` is the heaviest dep -- bump `-j` up to your core count if you have ~1-2 GB of RAM per job to spare. If you're tight on RAM, stick with `-j 2`.

**Smaller binary for distribution.** A `dist` profile is provided for release artifacts:

```bash
cargo build --profile dist -j 2     # slower, fat LTO + 1 codegen unit, smallest binary
```

**Why release-only?** Debug builds (`cargo build` without `--release`) compile `cranelift-codegen` with full debuginfo and can spike to >30 GB. The `[profile.dev]` settings in `compiler/Cargo.toml` opt heavy deps up to `opt-level = 2` to bound this, but release is what you want for everyday use.

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
    let ch = chan()
    spawn {
        let mut sum = 0
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

Kryos is **v1.0.1**. The language, toolchain, and standard library are feature-complete. See [CHANGELOG.md](CHANGELOG.md) for the release history.

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

## Community & Contact

- **Discussions:** [GitHub Discussions](https://github.com/NORTHTEKDevs/kryos-lang/discussions) — questions, ideas, show-and-tell.
- **Issues:** [GitHub Issues](https://github.com/NORTHTEKDevs/kryos-lang/issues) — bugs and feature requests.
- **Email:** [info@northtek.io](mailto:info@northtek.io) — direct contact for sponsorship, partnerships, or anything that doesn't fit a public thread.

---

## License

Apache License 2.0. See [LICENSE](LICENSE).

Kryos was built solo by [NORTHTEKDevs](https://github.com/NORTHTEKDevs) with heavy AI-assisted development. If you build something with it, I'd love to see it -- open an issue or discussion.
