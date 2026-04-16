# Kryos Compiler Architecture

## Build Constraints

Debug builds consume ~48 GB RAM due to monomorphization. Always use:

```bash
cargo build --release -j 4
cargo test --release -j 4
```

## Crate Map

| Crate | Purpose | Lines |
|-------|---------|-------|
| `kryos-cli` | CLI entry point, 11 subcommands | ~700 |
| `kryos-lexer` | Tokenizer | ~1400 |
| `kryos-parser` | Recursive descent parser | ~2200 |
| `kryos-ast` | AST types | ~900 |
| `kryos-types` | Type checker with inference, generics, Self type | ~3200 |
| `kryos-mir` | MIR lowering + 6 optimization passes | ~7500 |
| `kryos-codegen-cranelift` | Cranelift AOT + JIT | ~5800 |
| `kryos-codegen-llvm` | LLVM IR text emitter | ~3500 |
| `kryos-linker` | Native linker invocation | ~400 |
| `kryos-driver` | Pipeline orchestration | ~600 |
| `kryos-rt` | Runtime (builtins, panic, trace, string interning) | ~1200 |
| `kryos-stdlib-native` | Native stdlib (math, string, io, env, json) | ~400 |
| `kryos-errors` | Error types + colored diagnostics | ~800 |
| `kryos-ownership` | Ownership/borrow checker + Arc insertion | ~1200 |
| `kryos-capabilities` | Capability system | ~300 |
| `kryos-test-runner` | @test annotation runner + native build test harness | ~800 |
| `kryos-fmt` | Code formatter | ~1500 |
| `kryos-doc` | Documentation generator | ~800 |
| `kryos-lsp` | Language server protocol | ~1100 |
| `kryos-package` | Package manager (manifest, semver, resolve, lock, registry) | ~1740 |
| `kryos-bindgen` | C header to Kryos bindings | ~1580 |

## MIR Optimization Passes

Applied in order during MIR lowering:

1. Inline - inline `@inline` annotated functions
2. Constant fold - evaluate constant expressions at compile time
3. Pure/CSE - common subexpression elimination + dead call elimination for `@pure` functions
4. DCE - dead code elimination
5. TCO - tail call optimization
6. Strength reduction - replace expensive ops with cheaper equivalents

## Compilation Pipeline

```
Source (.kry)
  → Lexer (kryos-lexer)
  → Parser (kryos-parser) → AST (kryos-ast)
  → Type Checker (kryos-types)
  → Ownership Checker (kryos-ownership)
  → Capability Checker (kryos-capabilities)
  → MIR Lowering + Optimization (kryos-mir)
  → Codegen [Cranelift | LLVM] (kryos-codegen-*)
  → Linker (kryos-linker)
  → Native Binary
```

## Language Syntax Reference

| Construct | Syntax |
|-----------|--------|
| Boolean operators | `and`, `or`, `not` (not `&&`, `\|\|`, `!`) |
| Integer type | `i64` (not `int`, `Int`) |
| Float type | `f64` |
| String type | `str` (not `string`, `String`) |
| Immutable variable | `let x = 5` |
| Mutable variable | `let mut x = 5` |
| Function | `fn name(param: Type) -> ReturnType { body }` |
| Enum construction | `Shape.Circle(5.0)` (dot syntax) |
| Match destructuring | `Shape::Circle(r) => r * r` (double-colon in patterns) |
| String conversion | `to_string(value)` builtin |
| Print | `println(str_value)` -- takes a string |
| Assert | `assert(condition)` or `assert(condition, "message")` |
| Conditionals | `if`, `elif`, `else` |
| Loops | `for x in collection`, `while condition`, `break`, `continue` |
| Annotations | `@pure`, `@test`, `@inline`, `@deprecated`, `@copy` |
| Imports | `use module_name` |

## Testing

```bash
# Unit tests (925+ across all crates)
cargo test --release -j 4

# Native build tests (kry programs compiled and checked)
cargo test --release -j 4 -p kryos-test-runner

# File-level integration tests
cargo run --release -j 4 -- test
```

## Key Design Decisions

- **No borrow checker**: ARC-based ownership with move semantics enforced at compile time. The type checker inserts Arc wrapping automatically.
- **Dual backend**: Cranelift for fast iteration (~500ms compile), LLVM for optimized release. Selected via `--llvm` flag.
- **Windows CRT**: The linker links `vcruntime.lib` + `legacy_stdio_definitions.lib` to resolve `__imp_*` mismatches when mixing DLL CRT with direct printf calls from codegen.
- **REPL JIT**: The REPL uses `OutputType::Mir` to skip the binary `main` gate; each REPL iteration JIT-compiles into the running process.
- **Do not** change `extern "C"` on runtime functions to `extern "C-unwind"` -- Cranelift JIT on Windows cannot catch unwinding panics through JIT frames.
