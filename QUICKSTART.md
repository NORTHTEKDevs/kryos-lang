# Kryos Quick Start

Get from zero to running Kryos code in under five minutes.

## Prerequisites

You need:

- **Rust 1.75+** (stable). Install via [rustup](https://rustup.rs): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **A C compiler for linking.** On Linux/macOS this is `cc`/`clang` (already on most systems). On Windows you need MSVC Build Tools 2022 with the C++ workload.
- **~6 GB free disk** for `target/`.

You do **not** need LLVM installed — the LLVM backend emits IR as text. Only if you opt into `kryos build --backend llvm` and want to produce a binary do you need `llc` or `clang` on PATH.

## 1. Build the compiler

```bash
git clone https://github.com/NORTHTEKDevs/kryos-lang.git
cd kryos-lang/compiler
cargo build --release -j 2
```

Expected time on a typical laptop: **1–2 minutes** cold. Bump `-j` up to your core count if you have more than 8 GB of RAM. Each parallel rustc job needs roughly 1–2 GB while compiling `cranelift-codegen`, which is the heaviest crate in the dependency tree.

When `cargo build` finishes you should see three artifacts:

- `target/release/kryos` (or `kryos.exe` on Windows) — the compiler binary
- `target/release/libkryos_rt.a` (Linux/macOS) / `kryos_rt.lib` (Windows MSVC) — the runtime static library
- `target/release/libkryos_stdlib_native.a` (Linux/macOS) / `kryos_stdlib_native.lib` (Windows MSVC) — the native stdlib static library

> **Windows MSVC note:** The Windows libraries use the MSVC `.lib` extension with no `lib` prefix: `kryos_rt.lib` and `kryos_stdlib_native.lib`, not `libkryos_rt.a`. The env vars `KRYOS_RT_LIB` and `KRYOS_STDLIB_NATIVE_LIB` accept either form.

The compiler binary locates the two static libraries automatically when it links final user binaries. If you move `kryos` around, either set `KRYOS_RT_LIB` / `KRYOS_STDLIB_NATIVE_LIB` to the absolute paths, or use the conventional layout `<prefix>/bin/kryos` + `<prefix>/lib/libkryos_rt.a` (Unix) / `<prefix>/lib/kryos_rt.lib` (Windows).

## 2. Run your first program

```bash
./target/release/kryos run ../examples/hello.kry
```

Expected output:

```
Hello, Kryos!
```

## 3. Try the rest of the tour

```bash
./target/release/kryos run ../examples/fibonacci.kry      # recursion
./target/release/kryos run ../examples/shapes.kry         # enums + match
./target/release/kryos run ../examples/calculator.kry     # if/else + functions
./target/release/kryos run ../examples/word_count.kry     # structs + arrays
./target/release/kryos run ../examples/channels.kry       # concurrency
```

Every example file starts with one or more `// expect-stdout: <text>` lines that document the expected output, so you can sanity-check them by eye.

## 4. Editor support

**VS Code** — install the **[Kryos extension](https://marketplace.visualstudio.com/items?itemName=northtekdevs.kryos)** from the marketplace (search "Kryos"), or build the dev extension locally:

```bash
cd editors/vscode && npm install && npm run package
# Produces kryos-0.4.0.vsix — install via VS Code: "Extensions: Install from VSIX…"
```

**Zed** — in Zed, open Extensions (`cmd+shift+x` / `ctrl+shift+x`) → "Install Dev Extension" → select `editors/zed/`.

**Anything else** — point your editor's LSP client at `kryos lsp` over stdio. Standard LSP, no custom protocol.

## 5. Where to next

The **[Learn Kryos](docs/learn/README.md)** track is the recommended path from here: a 30-minute tour of the language followed by 27 runnable cookbook recipes (CLI tools, HTTP servers, JSON pipelines, worker pools, async fetch, libraries, dates, regex, encoding, structured logs, sorting, hashes, dedup, CSV, env config, retry, input validation, subprocesses, number formatting, path manipulation, random numbers, fuzzy search, resilience patterns, priority tasks, caching, and LLM chat).

For deeper reference:

- [docs/01-getting-started.md](docs/01-getting-started.md) — full beginner tour
- [docs/19-language-reference.md](docs/19-language-reference.md) — complete spec
- [docs/grammar.md](docs/grammar.md) — formal grammar
- [docs/06-ownership.md](docs/06-ownership.md) — ARC + move semantics without lifetimes
- [docs/10-capabilities.md](docs/10-capabilities.md) — `@capabilities` / `@pure` annotations
- [CHANGELOG.md](CHANGELOG.md) — what's new in v1.0.0-beta.4

## Troubleshooting

**Q: `cargo build --release` is using too much RAM and getting OOM-killed.**
A: Drop parallelism: `cargo build --release -j 1`. The single heaviest rustc invocation (compiling `cranelift-codegen`) needs ~1.5 GB and ~30 seconds; everything else is lighter.

**Q: `kryos run foo.kry` fails with `undefined reference to kryos_rt_init`.**
A: The runtime static library wasn't found. This happens if you copied just the `kryos` binary somewhere else. Either move the runtime libraries to the same directory as `kryos`, or set `KRYOS_RT_LIB` and `KRYOS_STDLIB_NATIVE_LIB` to their absolute paths. On Linux/macOS: `libkryos_rt.a` and `libkryos_stdlib_native.a`. On Windows MSVC: `kryos_rt.lib` and `kryos_stdlib_native.lib`.

**Q: Linker error mentioning `libssl` / `libsqlite3` on Linux.**
A: Install the system dev headers: `sudo apt-get install -y libsqlite3-dev libssl-dev pkg-config`.

**Q: Windows Defender (or another AV) flags `kryos.exe`.**
A: A false positive. New, unsigned compiler binaries occasionally trip
machine-learning AV heuristics (compilers JIT-emit and execute code, which
looks suspicious to a model). Verify your download against the SHA-256
checksums published on the release page before trusting it, then restore the
file from quarantine or add an exclusion for your Kryos install directory.
If you hit this on an official release binary, please open an issue — we
submit false positives to the AV vendor for reclassification.

**Q: Where is the package manager? REPL? doc generator?**
A: All built into `kryos`: try `kryos pkg --help`, `kryos repl`, `kryos doc <file.kry>`. Full list with `kryos --help`.
