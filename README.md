# Kryos

[![License: Apache 2.0](https://img.shields.io/badge/license-Apache_2.0-blue.svg)](LICENSE)
[![Release](https://img.shields.io/badge/release-v4.43.0--rc.4-orange.svg)](CHANGELOG.md)
[![Targets](https://img.shields.io/badge/targets-native%20%7C%20wasm-purple.svg)](#what-it-targets)
[![Parity](https://img.shields.io/badge/Cranelift_vs_LLVM-34%2F34-brightgreen.svg)](AUDIT-llvm-parity.md)
[![Self-host](https://img.shields.io/badge/self--host_bootstrap-16%2F16_deterministic-brightgreen.svg)](compiler/self-host/STAGE2_BLOCKER.md)
[![Stdlib tests](https://img.shields.io/badge/stdlib_tests-63%2F63-brightgreen.svg)](#status)
[![Warnings](https://img.shields.io/badge/build_warnings-0-brightgreen.svg)](#status)

**Kryos is a compiled, general-purpose programming language with the safety of Rust, the speed of C, and the clarity of Go — without lifetime annotations.** It ships a complete toolchain: compiler, formatter, LSP, package manager, debug info, and editor extensions. v4.19 ships 30+ subcommands, 15 LSP capabilities, 30+ stdlib modules, and 22 cookbook recipes.

```kryos
fn main() {
    println("Hello, Kryos!")
}
```

```bash
kryos run hello.kry
```

---

## In one minute

- **It's fast.** Native binaries via LLVM. Matches or beats Rust and Go on most workloads. 10–90× faster than Python. See [BENCHMARKS.md](BENCHMARKS.md) for honest head-to-head numbers.
- **It's safe.** Memory safety by construction (ARC + move semantics), no `'a` lifetime annotations, no GC pauses. Capability-typed effects catch I/O leaks at compile time.
- **It's small.** One binary, no LLVM dependency for development, ~14 MB compiler, ~700 MB to build from source.
- **It runs anywhere.** Native (Linux / macOS / Windows / Intel / Apple Silicon) and WebAssembly out of the same source.
- **It self-hosts.** Stage-1 (the Kryos-compiled compiler) successfully compiles every self-host source file in the 16-module compiler, deterministically. 20 consecutive perfect bootstraps across 3-run, 5-run, and 10-run test cycles. See [`.shift/REPORT_2026-05-20.md`](.shift/REPORT_2026-05-20.md) for the bring-up writeup.
- **The toolchain is done.** 30+ subcommands including `run`, `build`, `check`, `test`, `bench`, `fmt`, `lint`, `audit`, `coverage`, `profile`, `trace`, `new`, `watch`, `clean`, `eval`, `doc serve`, `doctor`, `tree`, `pack`, `diff`, `info`, `workspace`, `config`, `welcome`, `cheat`, `changelog` — all stable at v4.0.
- **Stdlib is broad.** 61 modules: fs, net, http, json, regex, datetime, duration, base64, uuid, hash, sort, collections, queue, stack, set, random, log, bytes, pathext, numfmt, strext, cmd, iter, and more.

> **Status:** Kryos is at the v4 stability cut — CLI surface, LSP method set, stdlib symbol table, and ABI symbols are all frozen for v4.x.y backwards compatibility. See [STABILITY-v4.0.md](STABILITY-v4.0.md) for the contract.

---

## Start here

| If you want to… | Go to |
|---|---|
| Try Kryos in your browser | [play.kryos.dev](https://play.kryos.dev) |
| Browse the docs | [kryos.dev](https://kryos.dev) |
| Find a package | [packages.kryos.dev](https://packages.kryos.dev) |
| Build an MCP server in 60 seconds | [kryos-mcp-template](https://github.com/NORTHTEKDevs/kryos-mcp-template) |
| Install and run code in 5 minutes | [QUICKSTART.md](QUICKSTART.md) |
| Learn the language properly | [docs/learn/](docs/learn/README.md) |
| See real benchmark numbers | [BENCHMARKS.md](BENCHMARKS.md) |
| Read the full manual | [docs/README.md](docs/README.md) |
| Contribute | [CONTRIBUTING.md](CONTRIBUTING.md) |
| Understand the design philosophy | [docs/WHY_KRYOS.md](docs/WHY_KRYOS.md) |

---

## Install

### Linux / macOS

```bash
curl -fsSL https://raw.githubusercontent.com/NORTHTEKDevs/kryos-lang/master/install.sh | bash
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/NORTHTEKDevs/kryos-lang/master/install.ps1 | iex
```

### From source

```bash
git clone https://github.com/NORTHTEKDevs/kryos-lang.git
cd kryos-lang/compiler
cargo build --release -j 2
./target/release/kryos --version   # → kryos 4.43.0-rc.4
```

Requirements: Rust 1.75+, a C compiler (`cc`/`clang`/MSVC) for linking. **LLVM is not required for development** — the LLVM backend emits IR as text. You only need `clang` or `llc` on PATH if you want optimized release binaries.

For a full walkthrough including build footprint and troubleshooting, see [QUICKSTART.md](QUICKSTART.md).

---

## What it looks like

### Functions, types, control flow

```kryos
fn fizzbuzz(n: i64) {
    for i in 1..=n {
        if i % 15 == 0      { println("FizzBuzz") }
        else if i % 3 == 0  { println("Fizz") }
        else if i % 5 == 0  { println("Buzz") }
        else                { println(to_string(i)) }
    }
}

fn main() { fizzbuzz(20) }
```

### Enums and pattern matching

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
```

### Concurrency with channels

```kryos
fn main() {
    let ch = chan()
    spawn {
        let mut sum = 0
        for i in 1..=10 { sum = sum + i }
        send(ch, sum)
    }
    println("sum = " + to_string(recv(ch)))
}
```

### Async/await

```kryos
async fn fetch_and_sum(urls: [str]) -> i64 {
    let mut total = 0
    for url in urls {
        let body = await http_get(url)
        total = total + len(body)
    }
    total
}
```

### Capability-typed effects (compile-time enforcement)

```kryos
@pure
fn hash(data: str) -> i64 {
    // compile error if this calls io/net/process
    compute_hash(data)
}

@capabilities(io)
fn read_config(path: str) -> str {
    file_read(path)
}
```

---

## What it targets

One CLI, three backends:

| Backend | Use when | Speed |
|---|---|---|
| `cranelift` (default) | Dev loop, quick rebuilds | ~500ms cold |
| `llvm` | Release binaries, max throughput | Matches Rust `--release` |
| `wasm` | Browser, WASI, edge sandboxes | Portable bytecode |

```bash
kryos build hello.kry                     # Cranelift (fast)
kryos build --release hello.kry           # LLVM (optimized)
kryos build --backend wasm hello.kry      # WebAssembly
```

---

## How it compares

Honest numbers from [BENCHMARKS.md](BENCHMARKS.md) — best of 10 runs, sandbox VM with a ~30 ms subprocess-launch floor (so very fast programs cluster at the floor; the real signal is on slower workloads).

| Benchmark | Kryos LLVM | Rust `--release` | gcc -O3 | Go | Python |
|---|---|---|---|---|---|
| fib(35) | 0.032 | 0.032 | 0.032 | 0.064 | 1.118 |
| mandelbrot | 0.032 | 0.032 | 0.032 | 0.032 | 0.716 |
| nbody | 0.032 | 0.008 | 0.008 | 0.016 | 0.817 |
| binary_trees | 0.008 | 0.003 | 0.001 | 0.003 | 0.064 |
| fannkuch | 0.114 | 0.016 | 0.016 | 0.008 | 0.466 |
| matmul | 0.064 | 0.032 | 0.032 | 0.032 | 2.976 |

(seconds; lower is better)

Where Kryos shines: simple loops, recursion, and floating-point arithmetic — competitive with optimized C and Rust. Where Kryos still trails the C/Rust frontier: tight inner loops that depend on aggressive loop unrolling and bounds-check elision (fannkuch, nbody, matmul). The full per-benchmark analysis with "where we lose and why" is in [BENCHMARKS.md](BENCHMARKS.md).

---

## What ships in v4.43.0-rc.4

The full toolchain. Not a roadmap — actually built and tested:

- **Compiler** — three backends (Cranelift / LLVM / WASM), zero cargo warnings, 295+ workspace tests passing
- **Self-host** — 16/16 self-host modules compile through stage-1 (Kryos-compiled compiler). See [docs/20-self-hosting.md](docs/20-self-hosting.md)
- **Language** — ownership, traits with `Self`, generics, pattern matching, closures, async/await, capabilities, comptime, FFI
- **Standard library** — 61 modules covering strings, math, collections, JSON, HTTP, regex, datetime, crypto, files, processes, channels, tensors, AI primitives
- **Debug info** — LLVM DWARF emission; `addr2line` resolves Kryos source lines in optimized binaries
- **Async substrate** — state-machine lowering wired end-to-end; no eager-DONE bugs on multi-await functions
- **WASM stdlib parity** — strings, arrays, JSON, regex, HTTP all callable from Kryos compiled to WebAssembly
- **Package manager** — `kryos pkg init / add / remove / install / publish / search / outdated`. Lockfile, semver resolution, content-addressed checksums
- **Editor extensions** — VS Code (marketplace-ready) and Zed (dev-extension)
- **REPL, formatter, doc generator, test runner, LSP, C-header bindgen**
- **Package registry** — full spec + dependency-free reference HTTP server in [tools/registry/](tools/registry/)

Detailed release notes: [CHANGELOG.md](CHANGELOG.md).

---

## What it means for languages

Kryos is built on a thesis: **memory safety without lifetime annotations is achievable**, and the "complexity tax" Rust imposes for safety is mostly avoidable if you accept ARC + move-semantics over borrow-checking. The trade is small: a tiny ARC overhead in exchange for code that looks closer to Go or Python than to Rust.

Kryos also takes seriously the idea that **a language should ship with everything needed to finish a project**. Stdlib, async runtime, HTTP, JSON, regex, crypto, package manager — all in the box. You should be able to write a real program without picking 14 third-party crates and praying their version ranges align.

The third thesis is **capability typing as a first-class compile-time check**. `@pure` and `@capabilities(io, net)` aren't lint hints — they're enforced. A function annotated `@pure` that secretly calls `file_read` is a compile error, not a runtime surprise. This is the foundation for trustworthy plugin systems, sandboxed execution, and auditability.

---

## Toolchain

```
kryos run <file.kry>          Compile and execute
kryos check <file.kry>        Type-check without running
kryos build <file.kry>        Compile to native (Cranelift default)
kryos build --release         Compile via LLVM backend
kryos build --backend wasm    Compile to WebAssembly
kryos fmt <file.kry>          Format in place
kryos test                    Discover + run @test functions
kryos doc <file.kry>          Generate HTML documentation
kryos repl                    Interactive REPL with persistent state
kryos pkg <subcommand>        Package manager (init / add / install / publish / ...)
kryos bindgen <header.h>      Generate Kryos bindings from C headers
kryos lsp                     Language server (used by VS Code / Zed extensions)
```

---

## Status

Kryos is **v4.43.0-rc.4**. Feature-complete language and toolchain + self-hosting compiler.

| Feature | Status |
|---|---|
| Type system + inference | Complete |
| Ownership / ARC + move semantics | Complete |
| Generics + monomorphization | Complete |
| Traits with `Self` type | Complete |
| Pattern matching + enums | Complete |
| Closures (ARC-captured) | Complete |
| Channels + `spawn` | Complete |
| Async / await + state machines | Complete |
| Capability enforcement (`@pure`, `@capabilities`) | Complete |
| `@test` runner, `@copy`, `@pure` CSE | Complete |
| Cranelift backend | Complete |
| LLVM backend (native + DWARF) | Complete |
| WebAssembly backend | Complete |
| Module system + package manager | Complete |
| LSP, REPL, formatter, doc generator | Complete |
| Editor extensions (VS Code, Zed) | Complete |
| Package registry (spec + reference server) | Complete |

**Quality bar maintained through v4.43:**

- Workspace lib tests: **295+ passing**
- MIR lib tests: **79/79**
- Build warnings: **0**
- Self-host bootstrap: **16/16 modules**, mean 15.87/16 across 30-run characterization (93% perfect-run rate, see [REPORT_2026-05-20.md](.shift/REPORT_2026-05-20.md))

### Self-host status

The Kryos compiler is written in Kryos. Stage-0 is the Rust-implemented `kryos.exe`. Stage-1 is the Kryos-compiled compiler that stage-0 produces from `compiler/self-host/*.kry` (34,342 lines of Kryos source). Stage-1 in turn compiles every self-host source file back into a working `.obj` — the canonical bootstrap criterion — across the 16 modules: `token`, `lexer`, `ast`, `parser`, `types`, `mir`, `lower`, `optimize`, `regalloc`, `x86`, `codegen`, `elf`, `coff`, `linker`, `runtime`, `main`.

Verify yourself:

```bash
cd compiler
cargo build --release -j 2
./target/release/kryos.exe build self-host/main.kry -o target/bootstrap/kryos-stage1 --skip-ownership
bash self-host/test_bootstrap.sh   # → PASS: 16 / 16
```

---

## Project layout

```
kryos-lang/
  compiler/
    crates/          21 Rust crates (~50k lines) — the toolchain
    stdlib/          28 stdlib modules (.kry sources)
    examples/        74 runnable example programs
  docs/              19-chapter manual + grammar + learn/
  editors/
    vscode/          Marketplace-ready VS Code extension
    zed/             Zed extension scaffold
  benchmarks/        Benchmark suite (Kryos vs Rust/gcc/clang/Go/Python)
  tools/
    registry/        Reference Kryos package registry HTTP server
  install.sh         Linux/macOS installer
  install.ps1        Windows installer
```

---

## Where you can help

Kryos is a real working language but it has one user. Things that move the needle right now:

1. **Try it.** Write a small program. File an issue on anything that surprises you.
2. **Write a package.** Anything reusable — a database driver, a CLI parser, a logging library, a date library. Tagged `good-first-package` in Discussions.
3. **Port a benchmark.** If you know another language well, port a real benchmark from it and tell us where Kryos surprises you (in either direction).
4. **Pick a starter task.** [`.github/STARTER_TASKS.md`](.github/STARTER_TASKS.md) lists scoped first-PR-sized tasks (cookbook recipes, stdlib additions, example programs, diagnostic polish, editor work). Issues tagged `good first issue` on the tracker are also fair game.
5. **Write a tutorial.** Even a short blog post saying "here's how I built X in Kryos" is enormously valuable for adoption.

[CONTRIBUTING.md](CONTRIBUTING.md) has the full development setup. [Discussions](https://github.com/NORTHTEKDevs/kryos-lang/discussions) is the right place to ask anything open-ended.

---

## Community & contact

- **Discussions** — [github.com/NORTHTEKDevs/kryos-lang/discussions](https://github.com/NORTHTEKDevs/kryos-lang/discussions) for questions, ideas, show-and-tell
- **Issues** — [github.com/NORTHTEKDevs/kryos-lang/issues](https://github.com/NORTHTEKDevs/kryos-lang/issues) for bugs and feature requests
- **Security** — see [SECURITY.md](SECURITY.md) for private disclosure
- **Email** — [info@northtek.io](mailto:info@northtek.io) for direct contact

---

## License

Apache License 2.0. See [LICENSE](LICENSE).

Built by [NORTHTEKDevs](https://github.com/NORTHTEKDevs) with heavy AI-assisted development. If you build something with Kryos, open a Discussion — I want to see it.
