# Contributing to Kryos

Thank you for your interest in contributing. This document covers how to set up a development environment, navigate the compiler, write tests, and submit changes.

---

## Prerequisites

- Rust 1.75 or later (`rustup update stable`)
- LLVM 15+ (optional -- required only for `--llvm` release builds)
- Git

> **Memory warning:** Debug builds consume ~48 GB of RAM due to monomorphization. Always build with `--release -j 4`.

---

## Setup

```bash
git clone https://github.com/FrostbyteDevTeam/kryos-lang.git
cd kryos-lang/compiler
cargo build --release -j 4
```

Verify:

```bash
./target/release/kryos run examples/proof.kry
```

Expected output: `=== All 17 tests passed ===`

---

## Repository Layout

```
kryos-lang/
  compiler/
    crates/          21 Rust crates (the compiler)
    stdlib/          28 Kryos stdlib modules (.kry)
    self-host/       Self-hosting compiler in Kryos (19k lines)
    examples/        14 runnable example programs
    tests/           Integration test suite
  docs/              Language manual (15 chapters)
  editors/           VS Code extension
  benchmarks/        Criterion benchmarks
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

**Example programs** in `compiler/examples/` are run as part of CI. Add a new example if you're demonstrating a major feature.

---

## Code Style

- Rust: standard `rustfmt` formatting (`cargo fmt`)
- Kryos: `kryos fmt` (the formatter enforces the canonical style)
- No clippy warnings (`cargo clippy -- -D warnings` must be clean)
- No unused imports, no dead code

---

## Submitting Changes

1. Fork the repo and create a branch from `master`
2. Make your changes -- ensure all tests pass and clippy is clean
3. Write or update tests for the change
4. Open a pull request with a clear description of what and why

---

## Key Design Decisions

- **No lifetime annotations** -- ownership is tracked via ARC + move semantics, not borrow checker lifetimes.
- **Dual backends** -- Cranelift for developer experience (fast iteration), LLVM for production (optimization parity with Rust/C).
- **Capability enforcement is opt-in** -- unannotated functions have ambient authority. `@capabilities` scopes are explicit sandboxes.
- **Self type is resolved at the impl site** -- `Self` in trait signatures binds to the concrete type when the trait is implemented, not when it is declared.
- **ARC env for closures** -- closure environments are heap-allocated via `kryos_arc_alloc`. Captures of heap values (Str, Array, Map, Function, Shared) are cloned at capture time.

---

## Contact

Open an issue on GitHub for bugs, questions, or feature proposals.
