# Kryos Professional Release — 10/10 Audit Scores

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Bring all 8 audit categories to 10/10 — documentation, examples, tests, error messages, stdlib, LSP, package manager, self-hosting — making Kryos credible for public release.

**Architecture:** Work outward from the most visible gaps (docs, examples) to the deepest (LSP, pkg manager, self-hosting). Each task is self-contained and commits independently. Tests run after every change.

**Tech Stack:** Rust compiler (21 crates), Cranelift + LLVM backends, Kryos .kry source files

**Build command:** `cargo build --release -j 4` (debug builds use 48GB RAM)
**Test command:** `cargo test --release -j 4`
**Run .kry:** `cargo run --release -j 4 -- run <file.kry>`

---

## Phase 1: Documentation (2/10 → 10/10)

### Task 1: Write README.md

**Files:**
- Create: `README.md` (project root, NOT compiler/)

**What:** Professional README with: what Kryos is, key features, code examples (hello world + struct/enum/match), installation, project structure, current status (v0.2.0 alpha), license.

**Requirements:**
- No emojis
- Feature list: ownership semantics, capability-safe, two backends (Cranelift JIT + LLVM AOT), pattern matching, enums with payloads, structs, closures, higher-order functions, channels/actors, self-hosting compiler
- Show 2 code examples inline: (1) hello world, (2) the Shape enum + area function from proof.kry
- Installation: `git clone` + `cd compiler` + `cargo build --release -j 4`
- Running: `cargo run --release -- run examples/proof.kry`
- Link to `compiler/docs/LANGUAGE_REFERENCE.md` and `compiler/docs/GETTING_STARTED.md`
- Status section: honest about v0.2.0 alpha, what works, what's in progress
- License: check if there's a LICENSE file, reference it; if not, note TBD

**Test:** `cat README.md | head -5` should show the title. Verify links reference real files.

**Commit:** `docs: add project README`

---

### Task 2: Write Language Reference

**Files:**
- Create: `docs/LANGUAGE_REFERENCE.md`

**What:** Complete reference for every language construct. NOT a tutorial — a reference. Organized by category.

**Sections (each with syntax + example):**
1. **Types** — i8, i16, i32, i64, i128, u8-u128, f32, f64, bool, char, str, arrays `[T]`, tuples `(T, U)`, maps, Option, Result
2. **Variables** — `let`, `let mut`, `const`
3. **Functions** — `fn`, parameters, return types, `-> T`
4. **Control Flow** — `if`/`elif`/`else`, `while`, `for..in`, `match`, `break`, `continue`, `return`
5. **Structs** — declaration, construction `Name { field: value }`, field access `x.field`, methods via `impl`
6. **Enums** — declaration with payloads, construction `Enum.Variant(val)`, matching `Enum::Variant(bind) =>`
7. **Pattern Matching** — `match` expression, literal/ident/enum/wildcard patterns
8. **Closures & Higher-Order Functions** — `fn(T) -> U` types, passing functions as arguments
9. **Ownership** — move semantics, `&` references, `&mut` mutable references, Drop
10. **Error Handling** — `try`/`catch`, `throw`, Result/Option enums
11. **Concurrency** — `spawn`, channels (`chan`), actors
12. **Capabilities** — `@capability`, `@pure`, capability-safe functions
13. **Built-in Functions** — `println`, `to_string`, `len`, `push`, `pop`, `parse_int`, `parse_float`, `assert`, `time_now`, `file_read`, `file_write`, `env_get`
14. **Attributes** — `@deprecated`, `@inline`, `@test`, `@capability`, `@pure`
15. **Modules** — `use` statements, module resolution

**Source of truth:** Read the parser (`crates/kryos-parser/src/parser.rs`) and type checker (`crates/kryos-types/src/check.rs`) to verify every construct. Read `crates/kryos-ast/src/expr.rs` and `stmt.rs` for the full list of expressions and statements.

**Test:** Verify every code example in the reference actually compiles: pick 3 examples, save as temp .kry files, run them.

**Commit:** `docs: add language reference`

---

### Task 3: Write Getting Started Guide

**Files:**
- Create: `docs/GETTING_STARTED.md`

**What:** Walk a new developer through their first Kryos program. Step-by-step.

**Sections:**
1. Prerequisites (Rust toolchain, git)
2. Building the compiler (`cargo build --release -j 4`)
3. Hello World — create a file, run it
4. Variables and types — let, let mut, arithmetic
5. Functions — define and call
6. Structs — create a Person struct, access fields
7. Enums and match — define a Shape, write area()
8. Control flow — if/elif/else, while, for
9. Working with strings — concatenation, to_string
10. Next steps — link to language reference, examples

**Each section:** Show the .kry code, show the command to run it, show the expected output.

**Test:** Follow the guide yourself — create hello.kry from step 3, run it, verify output matches.

**Commit:** `docs: add getting started guide`

---

## Phase 2: Examples (3/10 → 10/10)

### Task 4: Create Example Programs

**Files:**
- Create: `examples/hello.kry`
- Create: `examples/fibonacci.kry`
- Create: `examples/calculator.kry`
- Create: `examples/word_count.kry`
- Create: `examples/grep.kry`
- Create: `examples/shapes.kry`
- Create: `examples/channels.kry`
- Create: `examples/README.md`

**What:** 7 example programs + an index README. Each must compile and run successfully.

**Programs:**
1. `hello.kry` — Simple hello world with string concatenation
2. `fibonacci.kry` — Recursive + iterative fibonacci, prints both
3. `calculator.kry` — Evaluates arithmetic using match on operation enum
4. `word_count.kry` — Reads a string, counts words/characters using a while loop
5. `grep.kry` — Simple string search in a hardcoded text (demonstrates string operations, for loops, if/else)
6. `shapes.kry` — Enum with payloads, area/perimeter calculations, demonstrates pattern matching
7. `channels.kry` — Producer/consumer with channels, demonstrates concurrency

**`examples/README.md`:** List each example with description and run command.

**Test:** Run EVERY example with `cargo run --release -j 4 -- run examples/<name>.kry` and verify clean output.

**Commit:** `docs: add 7 example programs with index`

---

### Task 5: Create Benchmark Suite

**Files:**
- Create: `benchmarks/fibonacci.kry`
- Create: `benchmarks/binary_trees.kry`
- Create: `benchmarks/sum_loop.kry`
- Create: `benchmarks/string_concat.kry`
- Create: `benchmarks/struct_alloc.kry`
- Create: `benchmarks/README.md`

**What:** 5 benchmark programs that can be timed. Each prints a result to prevent dead code elimination. These are the standard benchmarks used to compare language performance.

**Programs:**
1. `fibonacci.kry` — `fibonacci(35)`, prints result (should output 9227465)
2. `binary_trees.kry` — Allocate/deallocate binary tree nodes using structs, depth 15
3. `sum_loop.kry` — Sum integers 1 to 100_000_000 in a while loop
4. `string_concat.kry` — Concatenate strings in a loop (10000 iterations)
5. `struct_alloc.kry` — Create and drop 100000 structs with string fields

**`benchmarks/README.md`:** Explains how to run each with timing: `time cargo run --release -- run benchmarks/fibonacci.kry`

**Test:** Run each benchmark, verify it produces correct output. Time them to make sure none hang.

**Commit:** `perf: add benchmark suite`

---

## Phase 3: Error Messages (7.5/10 → 10/10)

### Task 6: Human-Readable Token Display in Parser Errors

**Files:**
- Modify: `crates/kryos-parser/src/parser.rs`
- Modify: `crates/kryos-lexer/src/lib.rs` (or token.rs — wherever TokenKind is defined)

**What:** Parser error messages currently show raw `{:?}` debug format for tokens (e.g., `expected identifier, found LeftBrace`). Implement a `Display` trait for `TokenKind` that shows human-readable names.

**Mapping (key ones):**
- `LeftParen` → `(`
- `RightParen` → `)`
- `LeftBrace` → `{`
- `RightBrace` → `}`
- `LeftBracket` → `[`
- `RightBracket` → `]`
- `Comma` → `,`
- `Colon` → `:`
- `Semicolon` → `;`
- `Arrow` → `->`
- `FatArrow` → `=>`
- `Dot` → `.`
- `Eq` → `=`
- `EqEq` → `==`
- `Bang` → `!`
- `BangEq` → `!=`
- `Plus`, `Minus`, `Star`, `Slash` → `+`, `-`, `*`, `/`
- `Eof` → `end of file`
- `IntLiteral` → `integer literal`
- `FloatLiteral` → `float literal`
- `StringLiteral` → `string literal`
- `Ident` → `identifier`
- Keywords → the keyword itself (e.g., `fn`, `let`, `struct`)

Then change all `format!("expected ..., found {:?}", tok.kind)` to use `{}` (Display) instead of `{:?}` (Debug).

**Test:** Write a .kry file with a deliberate syntax error, compile it, verify the error message is human-readable.

**Commit:** `dx: human-readable token names in parser errors`

---

### Task 7: Add Parser Error Recovery Hints

**Files:**
- Modify: `crates/kryos-parser/src/parser.rs`

**What:** For common mistakes, add helpful notes to parser errors.

**Patterns to handle:**
1. Missing closing brace: `note: unclosed block started at line N`
2. `elif` vs `else if`: if someone writes `else if`, suggest `elif` (or vice versa — check which Kryos uses). Actually, Kryos uses `elif`. If parser sees `else` followed by `if`, it should work — verify this.
3. Semicolons: if someone adds a `;` at end of statement, note: `Kryos does not use semicolons`
4. `let x: i64 = 5` — verify type annotations work; if not, add a note about syntax

**Test:** Create test .kry files with each mistake, verify the improved error messages.

**Commit:** `dx: add parser error recovery hints`

---

## Phase 4: Test Coverage (7/10 → 10/10)

### Task 8: Add Native E2E Tests for Under-Tested Features

**Files:**
- Create: `crates/kryos-test-runner/tests/native/enum_f64_payload.kry`
- Create: `crates/kryos-test-runner/tests/native/enum_multi_variant.kry`
- Create: `crates/kryos-test-runner/tests/native/nested_struct.kry`
- Create: `crates/kryos-test-runner/tests/native/string_ops.kry`
- Create: `crates/kryos-test-runner/tests/native/higher_order.kry`
- Create: `crates/kryos-test-runner/tests/native/match_exhaustive.kry`
- Create: `crates/kryos-test-runner/tests/native/recursive_fib.kry`
- Create: `crates/kryos-test-runner/tests/native/for_array.kry`
- Create: `crates/kryos-test-runner/tests/native/map_basic.kry`
- Create: `crates/kryos-test-runner/tests/native/try_catch.kry`

**What:** 10 new native run-and-verify tests covering features that were just fixed or are under-tested. Each uses `// expect-stdout:` or `// expect-exit:` annotations.

**Tests:**
1. `enum_f64_payload.kry` — Enum with f64 payload, match, arithmetic on extracted value. Tests the fix we just made.
2. `enum_multi_variant.kry` — Enum with 3+ variants, each with different payload types, match all.
3. `nested_struct.kry` — Struct containing struct, access nested fields.
4. `string_ops.kry` — String concatenation, to_string on various types, string in struct.
5. `higher_order.kry` — Pass function as argument, return function, apply_twice pattern.
6. `match_exhaustive.kry` — Match on integers with default arm, match on enum with all variants.
7. `recursive_fib.kry` — fibonacci(10) = 55, verify recursion works.
8. `for_array.kry` — For loop over array, accumulate sum.
9. `map_basic.kry` — Create map, insert, get, has, len.
10. `try_catch.kry` — throw and catch, verify caught value.

**Format:** Each file starts with `// expect-stdout: <expected output>` or `// expect-exit: <code>`.

**Test:** `cargo test --release -j 4 -p kryos-test-runner` — all new tests pass.

**Commit:** `test: add 10 native E2E tests for under-tested features`

---

## Phase 5: Stdlib Expansion (7/10 → 10/10)

### Task 9: Add String Utility Functions

**Files:**
- Modify: `crates/kryos-rt/src/string.rs`
- Modify: `crates/kryos-codegen-cranelift/src/codegen.rs` (register builtins)
- Modify: `crates/kryos-types/src/check.rs` (add type signatures for builtins)
- Create: `crates/kryos-test-runner/tests/native/string_utils.kry`

**What:** Add string utility functions that any real program needs:
- `contains(haystack: str, needle: str) -> bool`
- `starts_with(s: str, prefix: str) -> bool`
- `ends_with(s: str, suffix: str) -> bool`
- `trim(s: str) -> str`
- `to_upper(s: str) -> str`
- `to_lower(s: str) -> str`
- `split(s: str, delimiter: str) -> [str]` (returns array of strings)
- `replace(s: str, from: str, to: str) -> str`

**Implementation:** Each is a `#[no_mangle] pub extern "C" fn kryos_builtin_<name>` in `string.rs`. Takes KryosString handles, returns KryosString handles.

**Registration:** Add each to the codegen builtin dispatch (search for existing `kryos_builtin_` registrations to see the pattern). Add type signatures in the type checker so programs can call them.

**Test:** Create `string_utils.kry` that exercises each function.

**Commit:** `feat(stdlib): add string utility functions`

---

### Task 10: Add Math Utility Functions

**Files:**
- Modify: `crates/kryos-rt/src/lib.rs` or create `crates/kryos-rt/src/math.rs`
- Modify: `crates/kryos-codegen-cranelift/src/codegen.rs`
- Modify: `crates/kryos-types/src/check.rs`
- Create: `crates/kryos-test-runner/tests/native/math_utils.kry`

**What:** Basic math functions:
- `abs(x: i64) -> i64` and `abs_f(x: f64) -> f64`
- `min(a: i64, b: i64) -> i64` and `min_f(a: f64, b: f64) -> f64`
- `max(a: i64, b: i64) -> i64` and `max_f(a: f64, b: f64) -> f64`
- `sqrt(x: f64) -> f64`
- `floor(x: f64) -> f64`
- `ceil(x: f64) -> f64`

**Test:** Create `math_utils.kry` that exercises each function and prints results.

**Commit:** `feat(stdlib): add math utility functions`

---

## Phase 6: LSP Enhancement (7/10 → 10/10)

### Task 11: Improve LSP Completion and Hover

**Files:**
- Modify: `crates/kryos-lsp/src/completion.rs`
- Modify: `crates/kryos-lsp/src/hover.rs`

**What:**
1. **Completion:** Add all builtin function names (println, to_string, len, push, pop, etc.) to completion results. Currently only keywords and declarations are suggested. Add all builtins with their signatures as detail text.
2. **Hover:** For builtin functions, show signature and one-line description. Currently only keywords have hover info.

**Test:** Manual — start LSP server, verify completion and hover work. Or write a simple test that calls the completion/hover handlers directly.

**Commit:** `feat(lsp): improved completion and hover for builtins`

---

## Phase 7: Package Manager (6/10 → 10/10)

### Task 12: Add `kryos pkg install` Command

**Files:**
- Modify: `crates/kryos-package/src/lib.rs` (or wherever CLI commands are dispatched)
- Modify: `crates/kryos-cli/src/main.rs` (add install subcommand if missing)

**What:** Wire the existing resolution + fetch logic into an `install` command:
1. Read `kryos.toml` manifest
2. Resolve dependencies (already implemented in `resolve.rs`)
3. Fetch each dependency (already implemented in `fetch.rs`)
4. Write `kryos.lock` (already implemented in `lock.rs`)
5. Print summary of installed packages

**Test:** Create a test `kryos.toml` with a `{ path = "../some-lib" }` dependency, run `kryos pkg install`, verify it resolves without error.

**Commit:** `feat(pkg): wire install command to resolution + fetch`

---

## Phase 8: Self-Hosting Verification (6/10 → 10/10)

### Task 13: Create Bootstrap Verification Script

**Files:**
- Create: `self-host/bootstrap.sh`
- Create: `self-host/README.md`

**What:**
1. `bootstrap.sh`: Compiles `self-host/main.kry` with the Rust-built compiler, runs the result, captures output. Reports success/failure.
2. `README.md`: Documents the self-hosting status honestly — what compiles, what doesn't yet, what language features are needed to close the gap.

**Test:** Run the bootstrap script, document what happens (success or specific failure).

**Commit:** `docs: add self-hosting bootstrap script and status`

---

## Phase 9: Final Verification

### Task 14: Full Test Suite + Proof Program

**Steps:**
1. Run `cargo test --release -j 4` — all tests pass
2. Run `cargo run --release -j 4 -- run examples/proof.kry` — all 17 tests pass
3. Run every new example program — all produce correct output
4. Run every benchmark — all produce correct output
5. Count total tests across all crates
6. Verify README links all point to real files
7. Commit any final fixes

**Commit:** `chore: verify all tests and examples pass`
