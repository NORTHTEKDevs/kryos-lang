# Kryos Self-Hosting Design

## Goal

Remove the Python bootstrap compiler entirely and replace it with a production-grade Rust compiler that serves as the bootstrap for a self-hosted Kryos compiler. When complete, the Kryos compiler is written in Kryos, compiles itself, and produces native binaries — zero Python dependency.

## Architecture Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Bootstrap strategy | Rust compiler → Kryos self-host | Proven path (Go, Rust, OCaml). Debug against a proven compiler, not an incomplete one. |
| Backend | Cranelift (dev) + LLVM (release) | Fast dev cycle + optimized release. Same strategy as Zig. |
| Stdlib | Hybrid — Rust/C FFI for syscalls, Kryos for the rest | Every systems language does this. Can't write TCP sockets in pure Kryos. |
| Compilation model | AOT + REPL via Cranelift JIT | `kryos build` for binaries, `kryos run` for JIT, `kryos repl` for interactive. |
| FFI | C ABI + `kryos bindgen` (ships day one) | Universal interop. Bindgen generates Kryos extern declarations from C headers. |
| Memory model | Ownership + ARC (Swift/Mojo style) | Compile-time ownership for the common case, ARC for shared references. No borrow checker pain. |

## Competitive Position

Kryos competes with Rust, Go, Mojo, Zig, and C++. The differentiators:

- **Ownership + ARC** — memory safety without Rust's borrow checker learning curve
- **Capability system** — compile-time security guarantees (no other systems language has this)
- **AI runtime** — first-class LLM integration, agent annotations, self-healing
- **Actor model** — built-in concurrency primitives (chan/send/recv/select/ask)
- **Dual backend** — Cranelift for dev speed, LLVM for release performance
- **Self-hosted** — compiler written in Kryos, proving the language's capability

## Phase 1: Rust Compiler (Bootstrap)

The Rust compiler lives in a new top-level `compiler/` directory in the kryos-lang repo. It is a standalone Rust project (Cargo workspace) that replaces the Python `kryos/` directory entirely.

### Directory Structure

```
kryos-lang/
  compiler/                    # NEW — Rust compiler (bootstrap)
    Cargo.toml                 # workspace root
    crates/
      kryos-lexer/             # tokenizer
      kryos-parser/            # recursive descent parser → AST
      kryos-ast/               # AST node definitions, spans, visitor
      kryos-types/             # type system, inference, checking
      kryos-ownership/         # ownership analysis + ARC insertion
      kryos-capabilities/      # capability checking, attenuation
      kryos-mir/               # mid-level IR (desugared, typed, ownership-resolved)
      kryos-codegen-cranelift/ # Cranelift backend (dev builds + JIT/REPL)
      kryos-codegen-llvm/      # LLVM backend (release builds)
      kryos-linker/            # link orchestration (system linker invocation)
      kryos-bindgen/           # C header → Kryos extern declaration generator
      kryos-stdlib-native/     # Rust/C FFI layer for syscall-backed stdlib modules
      kryos-driver/            # compiler driver (orchestrates pipeline)
      kryos-cli/               # CLI frontend (build, run, repl, test, bench, bindgen)
      kryos-lsp/               # language server protocol implementation
      kryos-errors/            # diagnostics, error rendering, source spans
      kryos-package/           # package manager (resolve, fetch, lock)
  stdlib/                      # NEW — Kryos-written stdlib modules (.kry files)
    std/
      io.kry
      math.kry
      json.kry
      collections.kry
      string.kry
      regex.kry
      datetime.kry
      net.kry
      crypto.kry
      process.kry
      test.kry
      fmt.kry
      fs.kry
      sync.kry
      chan.kry
      iter.kry
      map.kry
      set.kry
      config.kry
      term.kry
      db.kry
      server.kry
  docs/                        # existing docs (updated for native compiler)
  examples/                    # existing examples
  tests/                       # compiler test suite (integration tests)
  benchmarks/                  # existing benchmarks
```

### Compiler Pipeline

```
Source (.kry)
  → Lexer (kryos-lexer)
    → Token stream
  → Parser (kryos-parser)
    → AST (kryos-ast)
  → Type Checker (kryos-types)
    → Typed AST
  → Ownership Analysis (kryos-ownership)
    → Ownership-annotated AST + ARC insertion points
  → Capability Checker (kryos-capabilities)
    → Verified AST
  → MIR Lowering (kryos-mir)
    → Mid-level IR (desugared, explicit drops, ARC ops)
  → Backend
    → Cranelift (kryos-codegen-cranelift) → native object
    → LLVM (kryos-codegen-llvm) → native object
  → Linker (kryos-linker)
    → native binary / shared library
```

### Crate Responsibilities

**kryos-lexer**: Tokenizes UTF-8 source into a token stream. Handles string interpolation, numeric literals (hex, binary, octal, underscore separators), all keyword recognition. Produces `Token { kind: TokenKind, span: Span, text: &str }`. Zero allocations for keywords/operators (interned).

**kryos-parser**: Recursive descent parser. Pratt parsing for expressions (precedence climbing). Produces a full AST with spans on every node. Handles all Kryos syntax: let/mut bindings, functions, structs, enums, traits, impls, match, for/while/loop, closures, annotations (@budget, @sandbox, @actor, @export, @capabilities), string interpolation, pipe operator, range expressions, select statements.

**kryos-ast**: AST node type definitions. Span type. Visitor trait. Pretty-printer. AST → JSON serialization (for tooling interop with kryos-code IDE). All nodes are arena-allocated for cache locality.

**kryos-types**: Full type system. Primitive types (i8, i16, i32, i64, u8, u16, u32, u64, f32, f64, bool, char, string). Generic types with monomorphization. Trait bounds. Type inference (Hindley-Milner with extensions). Numeric literal inference. Array, slice, map, set, option, result types. Function types. Struct and enum types with associated methods.

**kryos-ownership**: Ownership tracking and ARC insertion. Implements the three ownership rules (single owner, scope-based cleanup, move semantics). Detects when values need ARC wrapping (multiple references, closures capturing by reference, values shared across actors/channels). Inserts retain/release operations in the MIR. Copy types (primitives, small structs marked `@copy`) bypass ownership entirely.

**kryos-mir**: Mid-level intermediate representation. Desugared from AST — no syntactic sugar, no implicit conversions. Explicit drop points, explicit ARC retain/release, explicit function calls for operators. Control flow graph representation. This is the last common representation before backend-specific lowering.

**kryos-codegen-cranelift**: Lowers MIR to Cranelift IR. Compiles to native machine code. Used for `kryos run` (JIT), `kryos repl`, and `kryos build` (fast dev builds). Supports x86_64, aarch64. Handles ARC runtime calls, stack allocation, function calls (C ABI compatible).

**kryos-codegen-llvm**: Lowers MIR to LLVM IR via `inkwell` (safe Rust bindings to LLVM C API). Full optimization pipeline (O0-O3, LTO). Used for `kryos build --release`. Targets every platform LLVM supports. WASM output via `wasm32-unknown-unknown` target triple. Handles ARC runtime calls, vectorization hints, inlining decisions.

**kryos-linker**: Invokes the system linker (cc/ld on Unix, link.exe on Windows, wasm-ld for WASM). Links compiled objects with the Kryos runtime library and stdlib native layer. Handles static vs dynamic linking, cross-compilation target selection.

**kryos-bindgen**: Reads C header files, generates Kryos `extern` function declarations with correct type mappings. Handles: function declarations, struct layouts, enum constants, typedefs, `#define` constants (simple numeric/string). Does NOT handle: C++ (templates, overloading, namespaces), inline functions, variadic macros. Type mapping: `int` → `i32`, `long` → `i64`, `char*` → `*u8`, `void*` → `*u8`, `size_t` → `usize`, `bool` → `bool`, struct → Kryos struct with `@repr(C)`.

**kryos-stdlib-native**: Rust implementations of syscall-backed stdlib functions. Thin wrappers around libc/OS APIs exposed as `extern "C"` functions callable from compiled Kryos code. Covers: file I/O (open/read/write/close/seek), network sockets (TCP/UDP bind/connect/listen/accept/send/recv), process management (spawn/wait/kill/env), crypto (wraps ring or RustCrypto), terminal (raw mode, colors, cursor), datetime (system clock, formatting, parsing), regex (wraps regex crate).

**kryos-driver**: Orchestrates the compilation pipeline. Reads `kryos.toml` project config. Resolves imports and module paths. Manages incremental compilation (file hashing, dependency tracking). Parallelizes per-module compilation. Selects backend based on build mode.

**kryos-cli**: User-facing CLI. Commands:
- `kryos build [--release] [--target <triple>]` — compile to native binary
- `kryos run <file.kry> [args]` — JIT compile and run via Cranelift
- `kryos repl` — interactive REPL with Cranelift JIT
- `kryos test [--filter <pattern>]` — run test functions (`@test` annotation)
- `kryos bench` — run benchmark functions (`@bench` annotation)
- `kryos bindgen <header.h> [-o bindings.kry]` — generate FFI bindings
- `kryos fmt [files...]` — format Kryos source code
- `kryos check` — type-check without compiling
- `kryos doc` — generate documentation
- `kryos pkg init|add|remove|update|lock` — package management
- `kryos lsp` — start language server

**kryos-lsp**: Language Server Protocol implementation. Provides: diagnostics (errors/warnings as you type), go-to-definition, find references, hover information (types, docs), completion (context-aware), rename, format-on-save. Communicates via stdin/stdout JSON-RPC.

**kryos-errors**: Diagnostic engine. Renders errors with source context, span highlighting, fix suggestions. Inspired by rustc's error output. Supports: error, warning, info, help levels. Multi-span diagnostics (e.g., "value moved here... then used here"). Machine-readable JSON output for IDE integration.

**kryos-package**: Package manager. Resolves dependencies from `kryos.toml`. Fetches from git (github:user/pkg@^1.0.0 syntax). Semver resolution with ^, ~, = ranges. Lock file generation (`kryos.lock`). Dependency tree validation (no circular deps, capability checking across package boundaries).

### Runtime Library

A small Rust static library (`libkryos_rt.a`) linked into every compiled Kryos binary. Contains:

- **ARC runtime**: `kryos_arc_retain(ptr)`, `kryos_arc_release(ptr)`, `kryos_arc_alloc(size, drop_fn) -> ptr`. Thread-safe atomic reference counting.
- **Panic/unwinding**: `kryos_panic(msg)`, stack trace capture, clean unwinding with drop execution.
- **Channel runtime**: `kryos_chan_new() -> handle`, `kryos_chan_send(handle, val)`, `kryos_chan_recv(handle) -> val`, `kryos_chan_select(handles[], count) -> index`. Lock-free MPMC channels.
- **Actor runtime**: Actor spawn, mailbox management, supervision trees.
- **Allocator**: Default to system allocator, with `@allocator` annotation for custom allocators per scope.

### ARC Implementation Detail

The ownership + ARC model works as follows:

1. **By default, values are owned** — single owner, moved on assignment/pass, dropped at scope exit. Zero runtime cost (same as Rust's ownership).

2. **`shared` keyword creates an ARC-wrapped reference**:
   ```
   let data = vec![1, 2, 3]
   let shared_ref = shared data    // data is now ARC-wrapped
   let another = shared_ref        // cheap clone (atomic increment)
   ```

3. **Closures that capture by reference automatically use ARC**:
   ```
   let counter = 0
   let inc = fn() { counter += 1 }  // counter is ARC-wrapped
   ```

4. **Values sent across channels are moved (not shared)**:
   ```
   let msg = "hello"
   send(ch, msg)    // msg is moved into the channel
   // msg is no longer accessible
   ```

5. **`shared` values sent across channels use ARC**:
   ```
   let data = shared vec![1, 2, 3]
   send(ch, data)   // ARC ref sent, both sides can read
   ```

6. **Compiler inserts retain/release automatically** — no manual reference counting. The ownership checker determines at compile time which values need ARC and inserts the operations in MIR.

7. **Cycle detection**: ARC cannot handle reference cycles. The compiler rejects programs with provable cycles at compile time. For dynamic structures (graphs, doubly-linked lists), use `weak` references that don't increment the count.

### Type System Specifics

**Primitive types**: `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `f32`, `f64`, `bool`, `char`, `string`, `usize`, `isize`

**Compound types**: Arrays `[T; N]`, slices `[T]`, tuples `(T, U, ...)`, maps `Map<K, V>`, sets `Set<T>`, options `Option<T>`, results `Result<T, E>`

**User-defined types**: Structs, enums (algebraic data types with variants), traits (interfaces with default methods)

**Generics**: Monomorphized (like Rust, not boxed like Go). Trait bounds: `fn sort<T: Ord>(items: [T])`. Where clauses for complex bounds.

**Pattern matching**: Exhaustive match on enums. Destructuring in let bindings and match arms. Guard clauses. Or-patterns.

**Type inference**: Hindley-Milner with extensions for numeric literals, method chains, and closure return types. Explicit annotation required for function signatures.

## Phase 2: Kryos Standard Library

Written in Kryos itself, compiled by the Rust bootstrap compiler. Each module is a `.kry` file in `stdlib/std/`.

**Syscall-backed modules** (thin Kryos wrappers around kryos-stdlib-native FFI):
- `std::io` — file I/O (File, BufReader, BufWriter, stdin/stdout/stderr)
- `std::net` — TCP/UDP sockets, listeners, DNS resolution
- `std::crypto` — hashing (SHA-256/512, BLAKE3), HMAC, AES, random bytes
- `std::process` — spawn processes, env vars, command execution
- `std::term` — terminal raw mode, colors, cursor control
- `std::datetime` — system clock, timestamps, formatting, parsing
- `std::fs` — filesystem operations (path, walk, watch, temp files)
- `std::sync` — mutex, rwlock, atomic types, barriers

**Pure Kryos modules** (no FFI, written entirely in Kryos):
- `std::math` — trig, log, exp, floor, ceil, min, max, clamp, constants
- `std::json` — JSON parse/stringify with serde-style derive
- `std::collections` — Vec, HashMap, BTreeMap, LinkedList, Queue, Stack, Heap
- `std::string` — split, join, trim, replace, contains, starts_with, ends_with, format
- `std::regex` — regular expression engine (compiled NFA)
- `std::iter` — iterator adaptors (map, filter, fold, zip, chain, enumerate, take, skip)
- `std::fmt` — string formatting, display trait, debug trait
- `std::map` — ordered map (B-tree backed)
- `std::set` — ordered set (B-tree backed)
- `std::config` — TOML/env config loading
- `std::test` — test runner, assertions, benchmarking support
- `std::chan` — channel creation, typed channels, select macro
- `std::server` — HTTP server (built on std::net)
- `std::db` — database driver interface (connection, query, transaction)

## Phase 3: Toolchain Completeness

Everything a production language ships with, built from day one:

- **Formatter** (`kryos fmt`): Opinionated, zero-config code formatter. One canonical style.
- **Documentation generator** (`kryos doc`): Generates HTML docs from doc comments. Hosted at a docs site.
- **Test runner** (`kryos test`): Built into the compiler. `@test` annotation on functions. `@bench` for benchmarks.
- **Package manager** (`kryos pkg`): Git-based dependencies. `kryos.toml` manifest. Lock file. Semver resolution.
- **LSP server** (`kryos lsp`): Full IDE support from day one.
- **Bindgen** (`kryos bindgen`): C header → Kryos bindings.
- **Cross-compilation**: `kryos build --target aarch64-unknown-linux-gnu` — LLVM handles the rest.
- **WASM target**: `kryos build --target wasm32` — first-class web target.

## Phase 4: Self-Hosting

Once the Rust compiler passes the full test suite and the stdlib is complete:

1. Rewrite the Kryos compiler in Kryos (using the Rust compiler to compile it)
2. The Kryos-written compiler must pass the same test suite as the Rust compiler
3. The Kryos-written compiler must be able to compile itself
4. Freeze the Rust compiler as the bootstrap binary (like Go 1.4, Rust stage0)
5. All future compiler development happens in Kryos

The self-hosting compiler reuses the same crate structure as module boundaries:
- `kryos-lexer` → `compiler/lexer.kry`
- `kryos-parser` → `compiler/parser.kry`
- `kryos-types` → `compiler/types.kry`
- etc.

## Python Removal

Once the Rust compiler is functional and passes all existing tests:

1. Delete the `kryos/` Python directory entirely
2. Delete `kryos_cli.py`, `setup.py`
3. Remove all Python test files (rewritten as Kryos integration tests or Rust unit tests)
4. Update CI to build/test only the Rust compiler
5. Update installation docs and scripts

No Python code remains in the repository.

## Success Criteria

- [ ] `kryos build hello.kry` produces a native binary that runs correctly
- [ ] `kryos build --release hello.kry` produces an LLVM-optimized binary
- [ ] `kryos run hello.kry` JIT-compiles and runs via Cranelift
- [ ] `kryos repl` starts an interactive session
- [ ] `kryos bindgen stdio.h -o stdio.kry` generates correct bindings
- [ ] `kryos test` runs the full test suite
- [ ] `kryos fmt` formats source files
- [ ] `kryos lsp` provides IDE features
- [ ] All 22 stdlib modules functional
- [ ] Zero Python files in the repository
- [ ] The Kryos compiler compiles itself
- [ ] The self-compiled compiler passes the full test suite
- [ ] Cross-compilation to at least: x86_64-linux, x86_64-windows, aarch64-linux, aarch64-macos, wasm32
