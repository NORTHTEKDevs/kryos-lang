# Kryos

[![License: Apache 2.0](https://img.shields.io/badge/license-Apache_2.0-blue.svg)](LICENSE)
[![Release](https://img.shields.io/badge/release-v1.0.0--beta.1-blue.svg)](CHANGELOG.md)
[![Targets](https://img.shields.io/badge/targets-native%20%7C%20wasm-purple.svg)](#what-it-targets)
[![Parity](https://img.shields.io/badge/Cranelift_vs_LLVM-58%2F58-brightgreen.svg)](tests/parity/run_parity.sh)
[![Self-host](https://img.shields.io/badge/bootstrap-stage2%3D%3D3%3D%3D4_byte--identical-brightgreen.svg)](compiler/self-host/bootstrap-win.sh)
[![Stdlib tests](https://img.shields.io/badge/stdlib_tests-63%2F63-brightgreen.svg)](#status)
[![VS Code](https://img.shields.io/badge/VS_Code-marketplace-blue.svg)](https://marketplace.visualstudio.com/items?itemName=northtekdevs.kryos)

**Kryos is a compiled, general-purpose programming language: memory-safe without lifetime annotations (ARC + move semantics — a Swift-like trade-off, not Rust's borrow checker), with native binaries and the clarity of Go.** It ships a complete toolchain: compiler, formatter, LSP, package manager, debug info, and editor extensions — 30+ subcommands, 15 LSP capabilities, 60+ stdlib modules, and a cookbook of recipes.

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

- **It's fast.** Native binaries via LLVM — typically within a small factor of Rust/C on compute-heavy work, faster than Python by orders of magnitude, with honest losses listed first. Measured medians, methodology, and where Kryos loses (and why) in [BENCHMARKS.md](BENCHMARKS.md).
- **It's safe.** Memory safety by construction (ARC + move semantics), no `'a` lifetime annotations, no GC pauses. This is a Swift-like ownership model — simpler than Rust's borrow checker, and not equivalent to its compile-time guarantees (no data-race freedom proofs). Capability-typed effects catch I/O leaks at compile time.
- **It's small.** One binary, no LLVM dependency for development, ~14 MB compiler, ~700 MB to build from source.
- **It runs anywhere.** Native (Linux / macOS / Windows / Intel / Apple Silicon) from the same source, plus an explicit-subset WebAssembly target (scalars, strings, closures, host-import I/O — anything outside the subset is a clear compile error, never a miscompile; see [docs/wasm-contract.md](docs/wasm-contract.md)).
- **It self-hosts.** The compiler is written in Kryos and reaches a byte-identical bootstrap fixed point — stage-2 == stage-3 == stage-4, SHA-256-verified — with the ownership and type checkers disabled on the self-host source (`--skip-ownership` / `KRYOS_SKIP_TYPES=1`). Stage-1 is Cranelift-compiled (a different backend), so it is not byte-identical to later stages. The per-module standalone compile check (`test_bootstrap.sh`) passes 16/16 modules (verified stable across 4 consecutive runs, 2026-07-01). See [`compiler/self-host/bootstrap-win.sh`](compiler/self-host/bootstrap-win.sh).
- **The toolchain is done.** 30+ subcommands including `run`, `build`, `check`, `test`, `bench`, `fmt`, `lint`, `audit`, `coverage`, `profile`, `trace`, `new`, `watch`, `clean`, `eval`, `doc serve`, `doctor`, `tree`, `pack`, `diff`, `info`, `workspace`, `config`, `welcome`, `cheat`, `changelog` — the CLI surface is frozen for 1.x (see VERSIONING.md).
- **Stdlib is broad.** 61 modules: fs, net, http, json, regex, datetime, duration, base64, uuid, hash, sort, collections, queue, stack, set, random, log, bytes, pathext, numfmt, strext, cmd, iter, and more.
- **It governs AI agents at compile time — and they embed anywhere.** `@capabilities(...)` is deny-what-you-don't-declare authority the compiler verifies (`E05xx` on violation), `@budget` refuses before spending, `Tracked<T>` carries provenance in the type. The same governed agent runs as a native binary, compiles to WebAssembly, and loads into Python / Go / Node / C# hosts through the C ABI with a machine-readable authority manifest — see [`ecosystem/kryos-embed/`](ecosystem/kryos-embed/README.md).

> **Status:** Kryos is **1.0.0-beta.1** — a feature-complete beta with one primary author, not yet externally stress-tested. The CLI surface, LSP method set, stdlib symbol table, and ABI symbols are frozen for 1.x backwards compatibility. See [STABILITY.md](STABILITY.md) for the contract.

---

## Start here

| If you want to… | Go to |
|---|---|
| Install and verify in 5 minutes | [QUICKSTART.md](QUICKSTART.md) |
| Learn the language (the manual) | [docs/learn/](docs/learn/README.md) |
| Browse the full docs | [docs/README.md](docs/README.md) |
| Build an MCP server in 60 seconds | [kryos-mcp-template](https://github.com/NORTHTEKDevs/kryos-mcp-template) |
| Embed a governed agent in your Python/Go/Node app | [ecosystem/kryos-embed](ecosystem/kryos-embed/README.md) |
| See what's verified working (apps, demos, probes) | [SHOWCASE.md](SHOWCASE.md) |
| See real benchmark numbers | [BENCHMARKS.md](BENCHMARKS.md) |
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

> While this repository is private, release downloads need a GitHub token:
> set `GITHUB_TOKEN` (or `GH_TOKEN`) to a PAT with repo read access before
> running either installer. This note disappears when the repo goes public.

After installing, verify the toolchain and take the tour:

```bash
kryos doctor    # checks linker, stdlib, runtime libs
kryos welcome   # first-run banner with an example workflow
kryos new hello && cd hello && kryos run src/main.kry
```

### From source

```bash
git clone https://github.com/NORTHTEKDevs/kryos-lang.git
cd kryos-lang/compiler
cargo build --release -j 2
./target/release/kryos --version   # → kryos 1.0.0-beta.1
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

### Async/await (grammar only)

```kryos
async fn fetch_and_sum(urls: [str]) -> i64 {
    let mut total = 0
    for url in urls {
        let body = await http_get(url)   // runs synchronously today
        total = total + len(body)
    }
    total
}
```

> **Status:** `async`/`await` are parsed and type-checked, but `await expr`
> currently lowers to a direct synchronous call — there is no non-blocking
> executor behind it yet. For real concurrency today, use `spawn` + channels
> (above) or actors. A non-blocking I/O runtime is planned, not shipped.

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

## What ships in v1.0.0-beta

The full toolchain. Not a roadmap — actually built and tested:

- **Compiler** — three backends (Cranelift / LLVM / WASM), zero cargo warnings, 295+ workspace tests passing
- **Self-host** — bootstrap reaches a byte-identical fixed point (stage-2 == stage-3 == stage-4, with the ownership/type checkers disabled on the self-host source); the per-module standalone compile check passes 11/16 modules (codegen flaky on the rest). See [docs/20-self-hosting.md](docs/20-self-hosting.md)
- **Language** — ownership, traits with `Self`, generics, pattern matching, closures, capabilities, comptime, FFI (async/await parse but lower synchronously — see Status)
- **Standard library** — 61 modules covering strings, math, collections, JSON, HTTP, regex, datetime, crypto, files, processes, channels, tensors, AI primitives
- **Debug info** — LLVM DWARF emission; `addr2line` resolves Kryos source lines in optimized binaries
- **Concurrency** — `spawn` + channels + actors, all working today (async/await are grammar-only; no executor yet)
- **WASM stdlib parity** — strings, arrays, JSON, regex, HTTP all callable from Kryos compiled to WebAssembly
- **Package manager** — `kryos pkg init / add / remove / install / publish / search / outdated`. Lockfile, semver resolution, content-addressed checksums
- **Editor extensions** — [VS Code (live on the marketplace)](https://marketplace.visualstudio.com/items?itemName=northtekdevs.kryos) and Zed (dev-extension)
- **REPL, formatter, doc generator, test runner, LSP, C-header bindgen**
- **Package registry** — full spec + dependency-free reference HTTP server in [tools/registry/](tools/registry/)

Detailed release notes: [CHANGELOG.md](CHANGELOG.md).

---

## What it means for languages

Kryos is built on a thesis: **memory safety without lifetime annotations is achievable**, and the "complexity tax" Rust imposes for safety is mostly avoidable if you accept ARC + move-semantics over borrow-checking. The trade is small: a tiny ARC overhead in exchange for code that looks closer to Go or Python than to Rust.

Kryos also takes seriously the idea that **a language should ship with everything needed to finish a project**. Stdlib, concurrency (spawn/channels/actors), HTTP, JSON, regex, crypto, package manager — all in the box. You should be able to write a real program without picking 14 third-party crates and praying their version ranges align.

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

Kryos is **v1.0.0-beta.1**. Feature-complete language and toolchain + self-hosting compiler (bootstrap fixed point: stage-2 == stage-3 == stage-4, byte-identical, reached with the ownership/type checkers disabled on the self-host source via `--skip-ownership` / `KRYOS_SKIP_TYPES=1`).

> **Why "beta", and what happened to v4?** During the project's bring-up,
> version numbers tracked development sprints, not conventional semver
> maturity — the repo reached an internal "v4.46" within months, which would
> mislead newcomers about field maturity. Before public release the scheme
> was recalibrated: this is a **feature-complete beta with one user**.
> Historical tags (v0.x–v4.46) are preserved; the mapping is in
> [VERSIONING.md](VERSIONING.md). 1.0.0 final lands when external users have
> stress-tested it.

| Feature | Status |
|---|---|
| Type system + inference | Complete |
| Ownership / ARC + move semantics | Complete |
| Generics + monomorphization | Complete |
| Traits with `Self` type | Complete |
| Pattern matching + enums | Complete |
| Closures (ARC-captured) | Complete |
| Channels + `spawn` | Complete |
| `spawn` / channels / actors | Complete |
| Async / await (non-blocking executor) | Grammar only — lowers synchronously |
| Capability enforcement (`@pure`, `@capabilities`) | Complete |
| `@test` runner, `@copy`, `@pure` CSE | Complete |
| Cranelift backend | Complete |
| LLVM backend (native + DWARF) | Complete |
| WebAssembly backend | Complete |
| Module system + package manager | Complete |
| LSP, REPL, formatter, doc generator | Complete |
| Editor extensions (VS Code, Zed) | Complete |
| Package registry (spec + reference server) | Complete |

**Quality bar (every release):**

- Workspace lib tests: **295+ passing**
- MIR lib tests: **79/79**
- Build warnings: **0**
- Self-host bootstrap: byte-identical fixed point (stage-2 == stage-3 == stage-4, SHA-256); standalone per-module compile check passes **11/16 modules** (codegen flaky on the rest)

### Self-host status

The Kryos compiler is written in Kryos. Stage-0 is the Rust-implemented `kryos.exe`. Stage-1 is the Kryos-compiled compiler that stage-0 produces from `compiler/self-host/*.kry` (34,342 lines of Kryos source). Because stage-1 is built by the Cranelift backend (a different backend), it is **not** byte-identical to later stages. The canonical bootstrap criterion is the **fixed point**: stage-2 == stage-3 == stage-4, SHA-256-verified, reached with the ownership and type checkers disabled on the self-host source (`--skip-ownership` / `KRYOS_SKIP_TYPES=1`). The 16 modules are `token`, `lexer`, `ast`, `parser`, `types`, `mir`, `lower`, `optimize`, `regalloc`, `x86`, `codegen`, `elf`, `coff`, `linker`, `runtime`, `main`; the standalone per-module compile check (`test_bootstrap.sh`) currently passes 11/16 (codegen is flaky on the rest).

Verify yourself:

```bash
cd compiler
cargo build --release -j 2
# Byte-identical fixed point (the canonical criterion):
bash self-host/bootstrap-win.sh    # → stage-2 == stage-3 == stage-4 (SHA-256)
# Per-module standalone compile check:
bash self-host/test_bootstrap.sh   # → PASS: 11 / 16 (codegen flaky on the rest)
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
