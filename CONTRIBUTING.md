# Contributing to Kryos

Thank you for your interest in contributing. This document covers how to set up a development environment, navigate the compiler, write tests, and submit changes.

---

## Where to start

If this is your first time touching the repo, here is the recommended path:

1. **Learn the language first.** Read `docs/learn/README.md` and walk through `docs/learn/tour.md`. You don't need to be an expert, but knowing what the surface looks like makes the compiler much easier to read.
2. **Build it locally.** Follow the Setup section below. Confirm `./target/release/kryos run tests/smoke.kry` works.
3. **Pick a starter task.** See [`.github/STARTER_TASKS.md`](.github/STARTER_TASKS.md) for a curated list of small, scoped tasks suitable for a first PR (cookbook recipes, stdlib additions, example programs, diagnostic improvements, editor polish). The open issue tracker also carries a `good first issue` label: <https://github.com/NORTHTEKDevs/kryos-lang/labels/good%20first%20issue>.
4. **Ask before you build big things.** For anything larger than a starter issue — new language syntax, new MIR passes, codegen changes — open a [Discussion](https://github.com/NORTHTEKDevs/kryos-lang/discussions) first so we can talk through the design before you write code.

Areas where help is always welcome, in rough order of accessibility:

- **Cookbook recipes.** New programs under `docs/learn/cookbook/` that show one concrete task end-to-end. Keep them short and runnable.
- **Example programs.** New `.kry` files in `examples/` demonstrating real features.
- **Stdlib functions.** Add a new function to an existing module under `compiler/stdlib/` with `@test` coverage.
- **Diagnostics.** Improve error messages in `kryos-errors`, `kryos-parser`, or `kryos-types` — even one clearer message helps every user.
- **Editor tooling.** The VS Code extension in `editors/vscode/` and the Zed extension scaffold in `editors/zed/` both have room for polish.
- **Compiler internals.** MIR passes, codegen, runtime, type-checker. Higher bar, more impact — talk to us first.

---

## Prerequisites

- Rust 1.75 or later (`rustup update stable`)
- LLVM 15+ (optional -- required only for `--llvm` release builds)
- Git

**Linux only:** the stdlib-native crate enables SQLite and TLS by default. Install the system packages before building:

```bash
# Debian / Ubuntu
sudo apt-get install libsqlite3-dev libssl-dev pkg-config

# Fedora / RHEL
sudo dnf install sqlite-devel openssl-devel pkg-config

# Arch
sudo pacman -S sqlite openssl pkg-config
```

macOS and Windows ship these libraries with the Rust toolchain; no extra steps needed.

> **Build footprint:** Release builds (`cargo build --release`) need ~6 GB disk and ~1-2 GB RAM per parallel rustc job -- the `cranelift-codegen` dep is the heaviest. Use `-j 2` on machines with <8 GB RAM, scale up otherwise. Debug builds (`cargo build` without `--release`) compile heavy deps with full debuginfo and can spike above 30 GB; the `[profile.dev]` overrides in `compiler/Cargo.toml` opt them up to `opt-level = 2` to keep this bounded, but release is still recommended.

---

## Setup

```bash
git clone https://github.com/NORTHTEKDevs/kryos-lang.git
cd kryos-lang/compiler
cargo build --release -j 2
```

Verify:

```bash
./target/release/kryos run ../examples/hello.kry
./target/release/kryos run tests/smoke.kry
```

Expected output of `tests/smoke.kry`:

```
30
2
Kryos
```

---

## Repository Layout

```
kryos-lang/
  compiler/
    crates/          21 Rust crates (the compiler)
    stdlib/          28 Kryos stdlib modules (.kry)
    self-host/       Self-hosting compiler in Kryos (~19k lines)
    examples/        runnable example programs
    tests/           integration test suite
  docs/
    learn/           getting-started + tour + cookbook (read this first)
    01..19           19-chapter reference manual
  editors/
    vscode/          VS Code extension (marketplace-packaged)
    zed/             Zed extension scaffold
  benchmarks/        Criterion benchmarks + run.sh harness
  tools/registry/    Reference package-registry server
  install.sh         Unix installer
  install.ps1        Windows installer
```

---

## Compiler Pipeline

Source code flows through these crates in order:

```
.kry source
    |
kryos-lexer       Tokenize into Token stream
    |
kryos-parser      Recursive-descent + Pratt -> AST
    |
kryos-ast         AST node definitions (shared)
    |
kryos-types       Type inference, generics, trait resolution, Self type
    |
kryos-ownership   Move tracking, use-after-move, @copy structs
    |
kryos-capabilities Capability enforcement (@capabilities, @pure)
    |
kryos-mir         Lower AST -> MIR (SSA, basic blocks, monomorphization)
                  @pure CSE and dead call elimination passes
    |
    +-- kryos-codegen-cranelift  -> native binary (fast dev builds)
    +-- kryos-codegen-llvm       -> LLVM IR -> native binary (optimized release)
    |
kryos-linker      Link object files -> executable
```

Supporting crates: `kryos-driver` (orchestration), `kryos-rt` (ARC runtime), `kryos-stdlib-native`, `kryos-lsp`, `kryos-fmt`, `kryos-doc`, `kryos-bindgen`, `kryos-package`, `kryos-test-runner`, `kryos-errors`.

---

## Running Tests

Unit and integration tests:

```bash
cd compiler
cargo test --release -j 4
```

Run all example programs:

```bash
for f in examples/*.kry; do echo "=== $f ==="; ./target/release/kryos run "$f"; done
```

Run the proof suite (17 assertions covering the full language):

```bash
./target/release/kryos run examples/proof.kry
```

Run `@test` annotated tests in a file:

```bash
./target/release/kryos test path/to/file.kry
```

Clippy (must pass with zero warnings):

```bash
cargo clippy --release -j 4 -- -D warnings
```

---

## Memory Model

Kryos uses ARC (Atomic Reference Counting) for all heap values.

Key runtime functions:
- `kryos_arc_alloc(size, drop_fn)` -- allocate with registered destructor
- `kryos_arc_retain(ptr)` -- increment refcount
- `kryos_arc_release(ptr)` -- decrement; calls drop_fn and frees if count reaches 0
- `kryos_string_clone`, `kryos_array_clone`, `kryos_map_clone` -- deep clone heap values

**Ownership rule:** Every heap-typed value crossing an ownership boundary (closure capture, channel send, `spawn`, actor send) must be cloned/retained. The compiler generates drop code at scope exit.

---

## Adding a Language Feature

1. **Lexer** (`kryos-lexer/src/token.rs`) -- add any new tokens.
2. **Parser** (`kryos-parser/src/parser.rs`) -- parse new syntax into AST nodes.
3. **AST** (`kryos-ast/src/`) -- add new node types to `Expr`, `Stmt`, or `Decl`.
4. **Type checker** (`kryos-types/src/check.rs`) -- type-check the new node.
5. **Ownership** (`kryos-ownership/src/analysis.rs`) -- handle move semantics.
6. **Capabilities** (`kryos-capabilities/src/checker.rs`) -- propagate capability requirements.
7. **MIR lowering** (`kryos-mir/src/lower.rs`) -- lower to MIR instructions.
8. **Cranelift codegen** (`kryos-codegen-cranelift/src/`) -- emit Cranelift IR.
9. **LLVM codegen** (`kryos-codegen-llvm/src/`) -- emit LLVM IR.
10. **Formatter** (`kryos-fmt/src/formatter.rs`) -- format the new node.

Every feature must be handled in all 10 locations. Missing any one causes a compile-time panic or incorrect output.

---

## Writing Tests

**Unit tests** live in `#[cfg(test)]` blocks inside each crate.

**Integration tests** in `compiler/tests/` are `.kry` source files paired with expected output. Add a test by:

1. Creating `tests/my_feature.kry`
2. Adding the expected output to `tests/my_feature.expected`
3. The test harness picks it up automatically

**Example programs** in `examples/` are exercised by CI; compiler regression fixtures live in `compiler/tests/fixtures/`. Add a new example if you're demonstrating a major feature.

---

## Documentation examples must compile

Every self-contained ` ```kryos ` code block in `docs/learn/**` (including
the book at `docs/learn/book/`), `docs/0*.md`, `docs/1[0-9]-*.md`,
`docs/error-codes.md`, `QUICKSTART.md`, and `CLAUDE.md` is type-checked
against the compiler as part of CI (`tools/docs-examples/check.py`, the
`docs-examples` job in `.github/workflows/ci.yml`). This applies whether
or not your PR is *about* docs -- editing a builtin's signature, a
capability rule, or a stdlib function that a doc example calls can break
this gate even if you never touched a `.md` file yourself.

If you touch anything under those globs (or change compiler behavior a
doc example depends on), build first, then run:

```bash
python3 tools/docs-examples/check.py
```

It scans the *whole* repo, not just your file, so confirm your own
file's blocks show 0 failures in the output rather than only checking the
overall exit code (other pre-existing failures, if any, aren't yours to
fix in an unrelated PR). Budget a few minutes -- it compiles every block
in the corpus, there's no per-file mode.

A block that's illustrative pseudo-code rather than a real program opts
out with `<!-- docs-example: skip -->` on the line directly above the
fence; a block demonstrating a deliberate compiler error uses a
`// ERROR` comment on the offending line, which auto-skips it. Never use
`skip` to avoid fixing a block that's supposed to compile but doesn't --
see [`docs/learn/book/STYLE.md`](docs/learn/book/STYLE.md#example-conventions)
for the full convention (binding for the book; good practice everywhere
else in `docs/`).

---

## Code Style

- Rust: standard `rustfmt` formatting (`cargo fmt`)
- Kryos: `kryos fmt` (the formatter enforces the canonical style -- see
  [`docs/learn/book/STYLE-GUIDE.md`](docs/learn/book/STYLE-GUIDE.md) for
  naming conventions, capability-annotation idioms, and stdlib patterns
  beyond what the formatter itself enforces)
- No clippy warnings (`cargo clippy -- -D warnings` must be clean)
- No unused imports, no dead code

---

## CI gates

A PR runs the full `.github/workflows/ci.yml` matrix: Linux/macOS/Windows
builds, the native test runner (Cranelift JIT *and* LLVM AOT, every
fixture), the docs-examples gate above, the self-host stage-1 check, and
a long list of narrowly-scoped regression gates under `compiler/tests/`
and the repo-root `tests/` directory (capability soundness, security/
capability-attenuation, UTF-8 handling, IR signature parity, memory-
plateau, backend parity, wasm smoke, package selftests, and more) --
each one exists because it caught a real bug once and now guards the
class of bug, not just the specific instance; their headers in
`ci.yml` say which bug. This file doesn't duplicate that list --
`.github/workflows/ci.yml` is the source of truth for exactly what runs
and why; read it before adding a new gate so you don't recreate one that
already exists.

Locally, the closest single-command approximation of the mandatory core
(build + native tests + smoke + docs-examples) is:

```bash
cd compiler && cargo build --release -j 2 && cargo test --release -j 2 -p kryos-test-runner --test native_runner -p kryos-rt
cd .. && python3 tools/docs-examples/check.py
```

Not every gate is practical to run locally before every commit (some need
`clang`/`wasmtime`/a full ecosystem checkout) -- CI is the authority; run
what you can locally to catch obvious breaks early, and expect CI to
catch the rest.

## Known limitations (`tools/loop/LEDGER.md`)

[`tools/loop/LEDGER.md`](tools/loop/LEDGER.md) is this repo's running
record of found-and-triaged defects: fixed ones (with the commit and the
evidence that closed them) and OPEN ones (real, disclosed limitations
that haven't been fixed yet, ranked by how seriously they threaten the
capability/trust model). Read it before you:

- **Report a bug** -- it may already be a known OPEN item with more
  context than a fresh issue would capture.
- **Touch ownership, capabilities, codegen, or the wasm backend** -- the
  OPEN items in those areas explain the sharp edges you're most likely to
  collide with.
- **Fix something ledger-worthy.** If your PR fixes an OPEN item, move it
  to the CLOSED table in the same commit with the evidence (gate output,
  repro) that proves it; if your PR finds a NEW real defect you aren't
  fixing this PR, add it rather than letting it evaporate at the end of
  your session. "Update this file in the SAME commit as the work" is the
  file's own standing rule -- anything not written there is lost.

---

## Submitting Changes

1. Fork the repo and create a branch from `master`
2. Make your changes -- ensure all tests pass and clippy is clean
3. Write or update tests for the change; if you touched docs (or compiler
   behavior a doc example relies on), run `tools/docs-examples/check.py`
4. If your change closes or newly discloses a `tools/loop/LEDGER.md`
   item, update the ledger in the same PR
5. Open a pull request with a clear description of what and why

A PR that changes compiler behavior, adds a language feature, or affects
capability enforcement should expect the full CI matrix above to run
before merge -- not just the Rust unit tests.

---

## Key Design Decisions

- **No lifetime annotations** -- ownership is tracked via ARC + move semantics, not borrow checker lifetimes.
- **Dual backends** -- Cranelift for developer experience (fast iteration), LLVM for production (optimization parity with Rust/C).
- **Capability enforcement is opt-in** -- unannotated functions have ambient authority. `@capabilities` scopes are explicit sandboxes.
- **Self type is resolved at the impl site** -- `Self` in trait signatures binds to the concrete type when the trait is implemented, not when it is declared.
- **ARC env for closures** -- closure environments are heap-allocated via `kryos_arc_alloc`. Captures of heap values (Str, Array, Map, Function, Shared) are cloned at capture time.

---

## Contact

- **Bugs:** file an issue with the `Bug report` template. Always include `kryos --version` output and a minimal `.kry` reproduction.
- **Feature ideas:** start in [Discussions](https://github.com/NORTHTEKDevs/kryos-lang/discussions) if the idea is open-ended. Once it's concrete, file a `Feature request` issue.
- **Security issues:** email `info@northtek.io` privately. Do not file public issues for vulnerabilities.
- **Anything else:** Discussions is the right place. We try to respond within a few days.
