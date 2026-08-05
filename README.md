# Kryos

[![License: Apache 2.0](https://img.shields.io/badge/license-Apache_2.0-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.9.0-blue.svg)](STABILITY.md)
[![Targets](https://img.shields.io/badge/targets-native%20%7C%20wasm-purple.svg)](#what-it-targets)
[![Parity](https://img.shields.io/badge/Cranelift_vs_LLVM-77%2F77-brightgreen.svg)](tests/parity/run_parity.sh)
[![Diff-fuzz](https://img.shields.io/badge/differential_fuzz-14k%2B_programs%2C_0_divergences-brightgreen.svg)](tools/diff-fuzz/)
[![Self-host](https://img.shields.io/badge/bootstrap-stage3%3D%3D4_byte--identical-brightgreen.svg)](compiler/self-host/bootstrap-win.sh)
[![Ecosystem check](https://img.shields.io/badge/ecosystem_deny--by--default-259%2F259-brightgreen.svg)](tests/ecosystem_check.sh)
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
- **It's small.** One binary, no LLVM dependency for development, ~20 MB compiler, ~700 MB to build from source.
- **It runs anywhere.** Native (Linux / macOS / Windows / Intel / Apple Silicon) from the same source, plus an explicit-subset WebAssembly target (scalars, strings, closures, host-import I/O — anything outside the subset is a clear compile error, never a miscompile; see [docs/wasm-contract.md](docs/wasm-contract.md)).
- **Both backends are held to the same answer.** A differential fuzzer generates random type-correct programs (structs, enums, closures, loops, containers, exception paths) and diffs the JIT's output against the AOT binary's — 14,000+ programs, zero divergences, and a smoke run repeats on every CI push. An ICE-hunt harness additionally mutates valid programs into garbage and requires diagnostics, never a compiler panic — 1,168 mutants, zero ICEs ([tools/diff-fuzz/](tools/diff-fuzz/)).
- **It self-hosts.** The compiler is written in Kryos and reaches a byte-identical self-hosting fixed point — a self-host-compiled compiler recompiling its own source produces a bit-identical compiler (stage-3 == stage-4, SHA-256-verified). The self-host source **type-checks clean** (all 16 modules) and **ownership-checks with zero errors**; the reproduction bootstrap passes `--skip-ownership` for byte-determinism (a codegen-determinism concern, not a checker failure). Stage-1 is Cranelift-compiled (a different backend), so it is not byte-identical to later stages. The per-module standalone compile check (`test_bootstrap.sh`) passes 16/16 modules. See [`compiler/self-host/bootstrap-win.sh`](compiler/self-host/bootstrap-win.sh).
- **The toolchain is done.** 30+ subcommands including `run`, `build`, `check`, `test`, `bench`, `fmt`, `lint`, `audit`, `coverage`, `profile`, `trace`, `new`, `watch`, `clean`, `eval`, `doc serve`, `doctor`, `tree`, `pack`, `diff`, `info`, `workspace`, `config`, `welcome`, `cheat`, `changelog` — the CLI surface is frozen for 1.x (see VERSIONING.md).
- **Stdlib is broad.** 66 modules: fs, net, http, json, regex, datetime, duration, base64, uuid, hash, sort, collections, queue, stack, set, random, log, bytes, pathext, numfmt, strext, cmd, iter, and more.
- **It governs AI agents at compile time for direct authority AND closure/fn-value indirection, including through containers — and they embed anywhere.** `@capabilities(...)` is authority the compiler tracks through the call graph and verifies (`E05xx` on violation). Three enforcement modes — `permissive`, `inferred` (deny-by-default with interior inference), and `strict` (`--strict-capabilities`; all 91 examples pass it in CI). **Deny-by-default is the default.** A loose `kryos run foo.kry` — no flag, no project file — rejects any undeclared authority at compile time; you declare it once on `main` and the compiler infers every helper, catching it through direct calls, method dispatch, a builtin passed as a value BY NAME, or a closure/fn-value flowing through a parameter, local, return value, passthrough chain, actor message, `spawn`, generic, `dyn Trait` dispatch, or a CONTAINER (a struct field, array element, or map value that holds a closure, including nested combinations). `--capabilities-mode=permissive` opts out. `kryos new` scaffolds this posture. The compiler, examples, self-host compiler, and every ecosystem package are all deny-by-default (details in [docs/capability-roadmap.md](docs/capability-roadmap.md)). See [docs/10-capabilities.md](docs/10-capabilities.md) for the full sound surface and the one documented residual (a container populated from a non-literal source falls back to requiring `all`, the same sound conservative stance used for every other unresolvable shape). Alongside it, `@budget` refuses before spending and `Tracked<T>` carries provenance in the type. The same governed agent runs as a native binary, compiles to WebAssembly, and loads into Python / Go / Node / C# hosts through the C ABI with a machine-readable authority manifest — see [`ecosystem/kryos-embed/`](ecosystem/kryos-embed/README.md).

> **Status:** Kryos is **0.9.0** — feature-complete, self-hosting, and usable. The two concurrency deadlocks that blocked 1.0 are fixed and conformance is 62/62 on both backends (`bash tests/conformance/run_conformance.sh`; this count grows as tests are added -- `tests/docs_status_gate.sh` fails CI if this number drifts from the live file count); remaining pre-1.0 work is tracked in [HANDOFF.md](HANDOFF.md). The CLI surface, LSP method set, stdlib symbol table, and ABI symbols are shaped for 1.x compatibility but are **not frozen until 1.0**. See [STABILITY.md](STABILITY.md) for the contract.

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
| Understand how it was built (and verified) | [docs/HOW_THIS_WAS_BUILT.md](docs/HOW_THIS_WAS_BUILT.md) |
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

After installing, verify the toolchain and take the tour:

```bash
kryos doctor    # checks linker, stdlib, runtime libs
kryos welcome   # first-run banner with an example workflow
kryos new hello && cd hello && kryos run
```

### From source

```bash
git clone https://github.com/NORTHTEKDevs/kryos-lang.git
cd kryos-lang/compiler
cargo build --release -j 2
./target/release/kryos --version   # → kryos 0.9.0
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

### Async/await (cooperative executor)

```kryos
async fn worker(name: str, n: i64) {
    let mut i = 0
    while i < n {
        coop_record(name + to_string(i))
        await 0                 // yields to the scheduler
        i = i + 1
    }
}

fn main() {
    coop_spawn(worker("A", 3))
    coop_spawn(worker("B", 3))
    coop_run()
    println(coop_order())       // → A0 B0 A1 B1 A2 B2 (genuinely interleaved)
}
```

> **Status:** `await` is a real cooperative suspension point — it hands control
> to the scheduler so another ready task runs, and tasks genuinely interleave
> under `coop_run()` (verified on both backends). What is **not** yet shipped
> is a non-blocking *I/O* runtime: `await http_get(url)` yields cooperatively
> but the underlying socket read is still blocking. So `async`/`await` gives
> you cooperative concurrency today, not async I/O. See
> [docs/09-concurrency.md](docs/09-concurrency.md).

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

- **Compiler** — three backends (Cranelift / LLVM / WASM), zero cargo warnings, 670+ workspace tests passing
- **Self-host** — bootstrap reaches a byte-identical fixed point (stage-3 == stage-4); the self-host source type-checks and ownership-checks clean, and the per-module standalone compile check passes 16/16 modules. See [docs/20-self-hosting.md](docs/20-self-hosting.md)
- **Language** — ownership, traits with `Self`, generics, pattern matching, closures, capabilities, FFI (async/await parse but lower synchronously — see Status). `comptime` also parses and runs, but not at compile time yet — see [docs/11-comptime.md](docs/11-comptime.md).
- **Standard library** — 66 modules covering strings, math, collections, JSON, HTTP, regex, datetime, crypto, files, processes, channels, tensors, AI primitives
- **Debug info** — LLVM DWARF emission; `addr2line` resolves Kryos source lines in optimized binaries
- **Concurrency** — real OS-thread parallelism with `spawn` + channels (`chan<T>` MPMC, `select`), `actor` message-passing (private state, in-order handling, threads joined at exit), and **non-blocking `async`/`await`**: a blocking I/O op (`sleep`, `http_get`, `tcp_*`) inside an `async` task yields the scheduler, so `coop_spawn` N tasks and their I/O overlaps — 4 × 300 ms of I/O finishes in ~300 ms, not 1.2 s (verified concurrent on both backends). `await` also interleaves CPU-bound tasks cooperatively.
- **WASM stdlib parity** — strings, arrays, JSON, regex, HTTP all callable from Kryos compiled to WebAssembly
- **Package manager** — `kryos pkg init / add / remove / install / publish / search / outdated`. Lockfile, semver resolution, content-addressed checksums
- **Editor extensions** — [VS Code (live on the marketplace)](https://marketplace.visualstudio.com/items?itemName=northtekdevs.kryos) and Zed (dev-extension)
- **REPL, formatter, doc generator, test runner, LSP, C-header bindgen**
- **Package registry** — full spec + dependency-free reference HTTP server in [tools/registry/](tools/registry/)

Detailed release notes: [CHANGELOG.md](CHANGELOG.md).

---

## What it means for languages

Kryos is built on a thesis: **memory safety without lifetime annotations is achievable**, and the "complexity tax" Rust imposes for safety is mostly avoidable if you accept ARC + move-semantics over borrow-checking. The trade is small: a tiny ARC overhead in exchange for code that looks closer to Go or Python than to Rust.

Kryos also takes seriously the idea that **a language should ship with everything needed to finish a project**. Stdlib, concurrency (spawn/channels/async), HTTP, JSON, regex, crypto, package manager — all in the box. You should be able to write a real program without picking 14 third-party crates and praying their version ranges align.

The third thesis is **capability typing as a first-class compile-time check**. `@pure` and `@capabilities(io, net)` aren't lint hints — they're enforced. A function annotated `@pure` that secretly calls `file_read` is a compile error, not a runtime surprise. This is the foundation for trustworthy plugin systems, sandboxed execution, and auditability.

---

## Toolchain

```
kryos run [file|dir]          Compile and execute (defaults to the current project)
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

Kryos is **v0.9.0** — feature-complete but pre-1.0. `docs/BUGS.md` tracks fixed regressions as a changelog; the live, ranked list of everything still open is [tools/loop/LEDGER.md](tools/loop/LEDGER.md). The package-registry supply-chain gap (`kryos pkg install`/`add` verifies a real `sha256:<hex>` content hash of the fetched package against the registry index before trusting it, rejects a mismatch or a missing checksum, and re-verifies even a cache hit) and the container-shaped closure/fn-value capability-laundering gap (a struct field, array element, or map value holding a closure is traced by capability checking in every mode, including `--strict-capabilities`) are both CLOSED. **CORRECTION (2026-08-05): this paragraph previously claimed no capability/trust-boundary gap remained — that is no longer true.** A later red-team round found a live, unfixed capability escape: a closure returned by an ordinary zero-capability wrapper function (`fn wrap_once(inner: fn() -> str) -> fn() -> str { return || inner() }`, an everyday decorator/logging/retry shape) defeats `deny!()` under both the default-inferred and `--strict-capabilities` enforcement modes — see `tools/loop/LEDGER.md` item 10, ranked HIGHEST PRIORITY, and [`docs/LAUNCH-READINESS.md`](docs/LAUNCH-READINESS.md) for the full launch-readiness verdict. Do not rely on `deny!()` as a hard security boundary against this shape until item 10 is closed. Everything else in the toolchain (type system, ownership/ARC, generics, concurrency, both native backends, self-hosting) is feature-complete and gated green; `tools/loop/LEDGER.md`'s OPEN section is the authoritative current queue. Self-hosting compiler: bootstrap fixed point stage-3 == stage-4, byte-identical; the self-host source type-checks and ownership-checks clean, with `--skip-ownership` used in the reproduction bootstrap for byte-determinism.

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
| Channels + `spawn` + `select` | Complete |
| Actors (message-passing) | Complete (JIT + AOT) |
| Async / await | Complete — non-blocking I/O (blocking ops yield the scheduler; async tasks overlap I/O) + cooperative CPU interleaving; both backends |
| Capability enforcement (`@pure`, `@capabilities`) | Complete |
| `@test` runner, `@copy`, `@pure` CSE | Complete |
| Cranelift backend | Complete |
| LLVM backend (native + DWARF) | Complete |
| WebAssembly backend | Complete (v0.1 — assumes i64 array indices) |
| Module system + package manager | Complete |
| LSP, REPL, formatter, doc generator | Complete |
| Editor extensions (VS Code, Zed) | Complete |
| Package registry (spec + reference server) | Functional; install-time content-hash verification implemented |

**Quality bar (every release):**

- Workspace lib tests: **670+ passing** (70 suites, zero failures)
- MIR lib tests: **79/79**
- Build warnings: **0**
- Self-host bootstrap: byte-identical fixed point (stage-3 == stage-4, SHA-256); standalone per-module compile check passes **16/16 modules**

### Self-host status

The Kryos compiler is written in Kryos. Stage-0 is the Rust-implemented `kryos.exe`. Stage-1 is the Kryos-compiled compiler that stage-0 produces from `compiler/self-host/*.kry` (~22,300 lines of Kryos source). Because stage-1 is built by the Cranelift backend (a different backend), it is **not** byte-identical to later stages. The canonical bootstrap criterion is the **fixed point**: stage-3 == stage-4, SHA-256-verified (stage-2 is produced by the Cranelift-built stage-1, so it converges one stage later). The self-host source type-checks clean and ownership-checks with zero errors; the reproduction bootstrap passes `--skip-ownership` for byte-determinism (not because ownership checking fails). The 16 modules are `token`, `lexer`, `ast`, `parser`, `types`, `mir`, `lower`, `optimize`, `regalloc`, `x86`, `codegen`, `elf`, `coff`, `linker`, `runtime`, `main`; the standalone per-module compile check (`test_bootstrap.sh`) passes 16/16.

Verify yourself:

```bash
cd compiler
cargo build --release -j 2
# Byte-identical fixed point (the canonical criterion):
bash self-host/bootstrap-win.sh    # → stage-3 == stage-4 (SHA-256)
# Per-module standalone compile check:
bash self-host/test_bootstrap.sh   # → PASS: 16 / 16
```

---

## Project layout

```
kryos-lang/
  compiler/
    crates/          22 Rust crates (~115k lines) — the toolchain
    stdlib/          66 stdlib modules (.kry sources)
  examples/          59 runnable example programs (repo root)
  docs/              20-chapter manual + grammar + learn/
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
