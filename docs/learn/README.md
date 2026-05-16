# Learn Kryos

A guided path from "never seen Kryos" to "building real things." Designed to take a working programmer from any background and get them productive in an evening.

If you already know what you're doing and just want the spec, jump to the [language reference](../19-language-reference.md). If you're new, follow the path below in order.

---

## 1 · Install (10 minutes)

Pick the route that matches your system. The full guide is [QUICKSTART.md](../../QUICKSTART.md).

- **Linux / macOS:** `curl -fsSL https://raw.githubusercontent.com/NORTHTEKDevs/kryos-lang/master/install.sh | bash`
- **Windows:** `irm https://raw.githubusercontent.com/NORTHTEKDevs/kryos-lang/master/install.ps1 | iex`
- **From source:** `git clone … && cd compiler && cargo build --release -j 2`

Sanity check:

```bash
kryos --version   # → kryos 2.3.0
```

---

## 2 · Your first program (5 minutes)

Create `hello.kry`:

```kryos
fn main() {
    println("Hello, Kryos!")
}
```

Run it:

```bash
kryos run hello.kry
```

That's it. You're up and running. The same source file will compile to a native binary with `kryos build hello.kry` (Cranelift), an optimized binary with `kryos build --release hello.kry` (LLVM), or a WebAssembly module with `kryos build --backend wasm hello.kry`.

---

## 3 · The 30-minute tour

A short, complete tour of every major language feature:

[**docs/learn/tour.md**](./tour.md) — variables, functions, types, control flow, structs, enums, traits, closures, channels, async/await, capabilities, error handling, modules.

By the end you'll recognize idiomatic Kryos and be able to read any program in the repo.

---

## 4 · The cookbook

Practical, runnable recipes for things people actually build:

| Recipe | What you'll build |
|---|---|
| [01 · CLI tool](./cookbook/01-cli-tool.md) | A command-line word counter that reads files and prints stats |
| [02 · HTTP server](./cookbook/02-http-server.md) | A tiny JSON API that handles concurrent requests |
| [03 · JSON pipeline](./cookbook/03-json-pipeline.md) | Read JSON, transform it, write it back — the workhorse pattern |
| [04 · Concurrent worker pool](./cookbook/04-worker-pool.md) | `spawn` + channels for parallel work |
| [05 · Async fetch many](./cookbook/05-async-fetch.md) | Async/await across many HTTP calls without thread explosion |
| [06 · Build a small library](./cookbook/06-library.md) | Package your code as a reusable module |

Each recipe is a complete, working program — copy it, run it, modify it.

---

## 5 · Deeper dives (when you need them)

The 19-chapter manual covers each topic in depth. Read these as questions come up, not all at once:

- [01 · Getting started](../01-getting-started.md)
- [02 · Variables and types](../02-variables-and-types.md)
- [03 · Functions](../03-functions.md)
- [04 · Control flow](../04-control-flow.md)
- [05 · Structs and enums](../05-structs-and-enums.md)
- [06 · Ownership](../06-ownership.md) — **read this when ARC starts to feel weird**
- [07 · Error handling](../07-error-handling.md)
- [08 · Traits and generics](../08-traits-and-generics.md)
- [09 · Concurrency](../09-concurrency.md)
- [10 · Capabilities](../10-capabilities.md) — `@pure` and `@capabilities` explained
- [11 · Comptime](../11-comptime.md)
- [12 · Modules and packages](../12-modules-and-packages.md)
- [13 · FFI](../13-ffi.md) — calling C libraries
- [14 · AI runtime](../14-ai-runtime.md)
- [15 · Codegen](../15-codegen.md) — Cranelift / LLVM / WASM internals
- [16 · Integer overflow](../16-integer-overflow.md)
- [17 · Unsafe audit](../17-unsafe-audit.md)
- [18 · Cross-compilation](../18-cross-compilation.md)
- [19 · Language reference](../19-language-reference.md) — the full grammar and spec

---

## 6 · Editor setup

Productivity in Kryos requires an editor with LSP support.

- **VS Code:** install the **Kryos** extension from the marketplace (or load the dev extension from `editors/vscode/`).
- **Zed:** install the dev extension from `editors/zed/` (Extensions → Install Dev Extension).
- **Anything else:** point your editor at `kryos lsp` over stdio. The protocol is standard LSP.

The extension gives you syntax highlighting, autocomplete, hover-for-types, goto-definition, and diagnostics that update as you type.

---

## 7 · When you get stuck

- Look at [`examples/`](../../examples) — 74 runnable programs covering nearly every feature
- Search [Discussions](https://github.com/NORTHTEKDevs/kryos-lang/discussions) — ask if you don't find it
- File a bug on [Issues](https://github.com/NORTHTEKDevs/kryos-lang/issues) with a minimal repro

If something in the docs is confusing, that's a docs bug. File it. The point of this path is to get strangers productive, and feedback is the only way to know it works.
