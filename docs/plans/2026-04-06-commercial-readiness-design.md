# Kryos Commercial Readiness — Design Document

**Date:** 2026-04-06
**Goal:** Make Kryos a professional, complete, optimized language that developers can use, investors can believe in, and nobody laughs at.
**Approach:** Layered sweep (6 concentric rings), each leaving the compiler in a strictly better state.

## Current State

- 21-crate Rust compiler, 40,238 lines of Rust
- 28 stdlib modules, 11,151 lines of .kry code
- Dual backends: Cranelift (fast debug) + LLVM IR text (optimized release)
- Full toolchain: CLI, LSP, formatter, doc generator, package manager, test runner
- 3 example programs, 5 benchmarks, 42-file documentation manual
- Performance: debug builds 4-8x faster than Rust debug, release matches Rust on fib(42)

### What's Broken Right Now

1. **Build fails** — `Decl::Const` added to AST but not handled in `kryos-doc` and `kryos-fmt`
2. **LLVM release builds** — mutable variable handling may produce invalid IR in edge cases (alloca/store/load approach exists but needs verification)
3. **Cross-module resolution** — `modules/main.kry` test fails, `use` doesn't always resolve symbols
4. **Struct codegen** — some edge cases trigger Cranelift verifier errors (layout computation exists and looks correct, but no validation layer)

### What's Actually Implemented (corrected from earlier assumptions)

- **Parallel for**: Fully implemented — chunked spawning across 4 threads
- **Select statement**: Fully implemented — polling with try_recv, closed-channel detection
- **Constant folding**: Exists in `consteval.rs` with test suite, NOT wired into compilation pipeline
- **Comptime**: Partial — consteval can fold expressions, but comptime blocks don't invoke it

### What's Genuinely Missing

- **Borrowing (`&`, `&mut`)**: No reference types, no borrow checker — move-only semantics
- **Dynamic dispatch (`dyn Trait`)**: No vtables — static monomorphization only
- **Optimization passes**: Constant folding exists but not integrated; no DCE, inlining, TCO, LICM
- **Comptime evaluation**: The evaluator exists but isn't called from comptime blocks

---

## Ring 0 — Fix the Build (~30 minutes)

### 0.1 Handle `Decl::Const` in `kryos-doc`
- File: `compiler/crates/kryos-doc/src/lib.rs` (~line 385)
- Add match arm for `Decl::Const` that generates a documentation item (name, type annotation, value expression, visibility)

### 0.2 Handle `Decl::Const` in `kryos-fmt`
- File: `compiler/crates/kryos-fmt/src/formatter.rs` (~line 90)
- Add match arm that formats: `const NAME: Type = value` (or `pub const NAME: Type = value`)

### 0.3 Handle `Decl::Const` in `kryos-mir`
- File: `compiler/crates/kryos-mir/src/lower.rs`
- Verify const declarations are lowered (currently may be caught by wildcard)
- Should lower to: evaluate value expression, store as module-level constant accessible by name

### 0.4 Verify build and tests
- `cargo build` must succeed
- `cargo test` must be green (all existing tests pass)

**Exit criteria:** `cargo build` + `cargo test` = clean.

---

## Ring 1 — Correctness (critical bugs)

### 1.1 LLVM Mutable Variable Verification
- File: `compiler/crates/kryos-codegen-llvm/src/codegen.rs`
- The alloca/store/load approach exists (lines 507-551)
- Verify it works correctly for: loops with mutation, nested scopes, function parameters
- Write targeted tests: mutable counter in while loop, for loop with accumulator, nested if with mutation
- Fix any edge cases where LLVM IR is invalid
- Ensure `kryos build --release` works for all benchmark programs

### 1.2 Struct Codegen Hardening
- File: `compiler/crates/kryos-codegen-cranelift/src/codegen.rs` (lines 130-152 layout, 1568-1628 init/access)
- File: `compiler/crates/kryos-codegen-llvm/src/codegen.rs`
- Write comprehensive struct tests: nested structs, structs with multiple field types, struct methods, struct in collections
- Fix any Cranelift verifier errors that surface
- Ensure LLVM backend handles struct layout identically

### 1.3 Cross-Module Name Resolution
- File: `compiler/crates/kryos-driver/` (module resolution logic)
- File: `compiler/tests/modules/main.kry` + `modules/math.kry`
- Debug the failing test — trace what `use math` resolves to
- Fix: ensure imported symbols are registered in the caller's scope before type checking
- Add tests: multi-file imports, selective imports (`use math::{add, sub}`), re-exports

### 1.4 Const Declaration End-to-End
- After Ring 0 adds the match arms, verify that `const PI: f64 = 3.14159` actually works end-to-end
- Must work in: module scope, referenced in expressions, used as function arguments
- Add tests for const declarations

**Exit criteria:** All benchmarks pass on both backends. Struct tests comprehensive and green. Module test passes. 100% of `cargo test` green.

---

## Ring 2 — Completeness (language features)

### 2.1 Borrowing (`&` and `&mut`)
This is the single biggest credibility gap. A systems language without borrowing will be dismissed.

**Scope (deliberately simpler than Rust):**
- Immutable references (`&T`): multiple simultaneous readers allowed
- Mutable references (`&mut T`): exclusive access, no other refs active
- No named lifetimes — all borrows scoped to the enclosing block
- No self-referential structs
- References cannot be stored in structs (initially)

**Implementation plan:**
1. **AST**: Already parsed (`&` and `&mut` in type expressions)
2. **Type system** (`kryos-types`): Add `Type::Ref(Box<Type>, Mutability)` if not already present
3. **Ownership analyzer** (`kryos-ownership`): Extend `OwnershipState` with `Borrowed { mutable: bool, count: u32 }`. Track active borrows per variable. Enforce: no move while borrowed, no `&mut` while any borrow active, no `&` while `&mut` active.
4. **MIR** (`kryos-mir`): Add `MirType::Ref` handling in lowering. References lower to pointer operations. Dereference is implicit for field access, explicit for reassignment.
5. **Codegen** (both backends): References are pointers. `&x` = address-of, `*x` = load, `&mut x` = address-of with exclusive flag in analysis (codegen is identical to `&x`).

**Tests:** Borrow and use, borrow and try to move (error), multiple immutable borrows, mutable borrow exclusivity, borrow across function calls.

### 2.2 Comptime Evaluation Engine
- File: `compiler/crates/kryos-mir/src/consteval.rs` (exists, 15KB, has fold logic)
- Wire comptime blocks to invoke `consteval::fold()` during MIR lowering
- When a `comptime { expr }` block is encountered, evaluate `expr` at compile time
- If evaluation succeeds, substitute the result as a constant in the MIR
- If evaluation fails (references runtime values), emit a compile error
- Support: arithmetic, boolean logic, string concatenation, array literals, if/else, function calls to comptime-known functions

### 2.3 Dynamic Dispatch (`dyn Trait`)
**Implementation plan:**
1. **Type system**: Add `Type::DynTrait(TraitName)` — a fat pointer (data ptr + vtable ptr)
2. **MIR**: When a value is cast to `dyn Trait`, generate vtable construction — a struct of function pointers for each trait method
3. **Codegen**: `dyn Trait` values are 2 x i64 (data pointer + vtable pointer). Method calls on `dyn Trait` load the vtable, index to the method, and do an indirect call.
4. **Vtable layout**: Methods ordered alphabetically by name for determinism. Each entry is a function pointer.

### 2.4 Wire Constant Folding into Pipeline
- File: `compiler/crates/kryos-driver/src/pipeline.rs`
- After MIR lowering, before codegen, run a constant folding pass over all functions
- Use existing `consteval.rs` logic
- This is low-hanging fruit — the pass exists, just needs to be called

### 2.5 Attribute Enforcement
- Currently only `@capabilities` is enforced
- Implement enforcement for: `@pure` (no side effects), `@inline` (hint to inliner), `@deprecated` (emit warning), `@test` (mark as test function)
- These are cheap wins that make the language feel complete

**Exit criteria:** Programs using references, comptime, dyn Trait, and attributes compile and run correctly. Each feature has 10+ targeted tests.

---

## Ring 3 — Performance (optimization passes)

All passes operate on MIR, benefiting both backends.

### 3.1 Dead Code Elimination
- Remove unreachable basic blocks (no predecessors after entry)
- Remove assignments whose results are never read
- Remove functions that are never called (after monomorphization)
- Run after constant folding (folding may create dead branches)

### 3.2 Function Inlining
- At MIR level, inline functions below a size threshold (e.g., < 20 instructions)
- Respect `@inline` attribute as a force-inline hint
- Do not inline recursive functions
- Run before constant folding (inlining may expose foldable expressions)

### 3.3 Tail Call Optimization
- Detect functions where the last operation is a recursive call to self
- Replace with: assign new argument values, jump to function entry block
- This turns O(n) stack recursive functions into O(1) loops
- Critical for functional-style Kryos code

### 3.4 Loop-Invariant Code Motion
- For each loop, identify computations whose operands don't change within the loop
- Hoist those computations to the loop preheader block
- Requires dominance analysis (straightforward on MIR's basic block structure)

### 3.5 Strength Reduction
- Replace `x * 2` with `x << 1`, `x * power_of_2` with `x << log2(n)`
- Replace `x / power_of_2` with `x >> log2(n)` (for unsigned)
- Replace `x % power_of_2` with `x & (n-1)` (for unsigned)

### 3.6 Pipeline Integration
- File: `compiler/crates/kryos-driver/src/pipeline.rs`
- Add optimization pass ordering: inline -> fold -> DCE -> LICM -> strength reduction
- Gate behind `--release` flag (debug builds skip optimization for speed)
- Add `--emit-mir-opt` flag to dump optimized MIR for debugging

**Exit criteria:** Benchmarks show measurable improvement. Sum loop benchmark competitive with Rust release. Fib with TCO shows order-of-magnitude improvement for tail-recursive variant.

---

## Ring 4 — Ecosystem (toolchain completeness)

### 4.1 Formatter (`kryos fmt`)
- Handle all 10 Decl types (including Const from Ring 0)
- Consistent style: 4-space indent, trailing newline, blank line between top-level decls
- `kryos fmt --check` returns non-zero if formatting needed (CI integration)
- Format all stdlib modules as a smoke test

### 4.2 Doc Generator (`kryos doc`)
- Generate markdown documentation for all public items
- Include: function signatures, struct fields, enum variants, trait methods, constants
- Support `///` doc comments (verify parser extracts these)
- Generate an index page with all modules

### 4.3 LSP Verification
- Test in VS Code with a `.kry` file
- Verify: diagnostics appear on save, completion suggests local variables and builtins, hover shows type signatures, go-to-def jumps to function source
- Create a minimal VS Code extension manifest (`package.json` + `language-configuration.json`)

### 4.4 Package Manager Flow
- Verify end-to-end: `kryos pkg init myproject` -> creates `kryos.toml` + `src/main.kry`
- `kryos pkg add github:user/repo@^1.0.0` -> updates `kryos.toml`
- `kryos pkg lock` -> generates deterministic `kryos.lock`
- Multi-file compilation with dependencies resolves correctly

### 4.5 Test Runner
- `kryos test` discovers functions annotated with `@test` or in `tests/` directory
- Reports pass/fail with clear output
- `kryos test --filter name` runs subset
- Verify it works on a real test suite

### 4.6 REPL
- `kryos repl` starts interactive session
- Supports: expressions, let bindings, function definitions, multi-line input
- Shows types and values of evaluated expressions

**Exit criteria:** A new developer can: init a project, write code with LSP support, format it, test it, generate docs. The entire toolchain works end-to-end.

---

## Ring 5 — Showcase (sell the language)

### 5.1 Demo Programs (5 real applications)

**demo_http_server.kry** — A simple HTTP server
- Uses stdlib `http` and `net` modules
- Handles GET/POST routes, JSON responses
- Demonstrates: structs, match, error handling, concurrency

**demo_cli_tool.kry** — A file search tool (mini-grep)
- Command-line argument parsing
- Recursive directory traversal
- Pattern matching with regex
- Demonstrates: I/O, error handling, string processing, modules

**demo_pipeline.kry** — Concurrent data processing pipeline
- Producer-consumer pattern with channels
- Parallel for over data chunks
- Aggregation with actors
- Demonstrates: spawn, channels, parallel for, actors, structs

**demo_neural_net.kry** — Neural network forward pass (already exists, enhance)
- Two-layer network with tensor operations
- Matrix multiply, ReLU, softmax
- Demonstrates: tensor runtime, math, arrays

**demo_web_scraper.kry** — Concurrent web scraper
- Actor-based URL queue
- Concurrent HTTP requests with spawn
- Channel-based result collection
- Demonstrates: actors, HTTP, channels, error handling

### 5.2 Benchmark Suite
- Re-run all benchmarks with optimization passes enabled
- Add new benchmarks: binary trees, nbody, spectral norm (from benchmarks game)
- Publish comparison table: Kryos vs Rust vs Go vs Zig
- Include compilation speed AND runtime performance

### 5.3 "Why Kryos?" Positioning Document
- Clear, honest positioning vs Rust, Go, Zig, Carbon
- Kryos advantages: faster compilation than Rust, safer than Go, AI-native runtime, capability-based security
- Not a Rust replacement — a pragmatic alternative for teams that want safety without the learning curve
- Target audiences: AI/ML engineers, systems programmers tired of Rust's compile times, teams that need capability-based security

### 5.4 Getting Started Guide
- 5-minute path: install -> hello world -> first struct -> first test -> first build
- No assumed knowledge beyond "you've programmed before"
- Immediately shows what makes Kryos different

### 5.5 Investor-Grade README
- Architecture diagram (text-based, in README)
- Key metrics: lines of code, test count, benchmark results
- Roadmap with clear milestones
- Team section
- "How to try it" section with copy-paste commands

**Exit criteria:** Someone clones the repo, reads the README, runs the demos, reads "Why Kryos?", and comes away thinking "this is real and I want to know more."

---

## Additions for Professional Polish

### Error Messages
- Audit all compiler error messages for clarity
- Every error should: name what went wrong, show the source location, suggest a fix
- Follow Rust's error message quality bar (they're the gold standard)

### VS Code Extension
- Syntax highlighting (TextMate grammar for `.kry` files)
- LSP client configuration
- Snippet support (fn, struct, enum, impl, match templates)
- Publish to VS Code marketplace (or provide VSIX for manual install)

### CI/CD
- GitHub Actions workflow: build, test, clippy, fmt check on every push
- Release workflow: build binaries for Windows, macOS, Linux
- Automated benchmark tracking (catch regressions)

### Website (stretch goal)
- Simple landing page: what, why, how, try it
- Playground (compile Kryos in browser via WASM — long-term)
- Documentation hosted online

---

## Priority Order

| Ring | Effort | Impact | Dependency |
|------|--------|--------|------------|
| 0 — Build | 30 min | Unblocks everything | None |
| 1 — Correctness | Heavy | Programs actually work | Ring 0 |
| 2 — Completeness | Heavy | Language is credible | Ring 1 |
| 3 — Performance | Medium | Benchmarks impress | Ring 2 |
| 4 — Ecosystem | Medium | Toolchain is real | Ring 0 |
| 5 — Showcase | Medium | Language sells itself | Rings 1-4 |

Rings 3 and 4 can partially overlap. Ring 5 must come last.

---

## Success Criteria

When this is done, Kryos should pass the "show a senior engineer" test:
1. They can read the README and understand what Kryos is and why it exists
2. They can clone, build, and run a demo program in under 5 minutes
3. They can write a non-trivial program and it compiles and runs correctly
4. They can look at the benchmarks and see competitive performance
5. They can examine the toolchain (LSP, formatter, test runner, package manager) and see it's real
6. They cannot poke a hole by asking "but can it do X?" for any core language feature
7. The error messages help them when they make mistakes
8. They come away thinking "this person built something serious"
