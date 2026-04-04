# Kryos Language Manual — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Write a comprehensive developer manual for the Kryos programming language — 41 files covering syntax, semantics, stdlib API reference, and appendices.

**Architecture:** Markdown files in `kryos-lang/docs/`, organized by chapter number. Each chapter is self-contained and follows a consistent format. Content is derived from the actual source code in `kryos/compiler/`, `kryos/stdlib/`, `kryos/runtime/`, `tests/programs/`, and `examples/`.

**Tech Stack:** Markdown, Kryos code examples

**Source material locations:**
- Language implementation: `kryos/compiler/` (lexer.py, parser.py, interpreter.py, types.py, ownership.py, codegen.py, capabilities.py, comptime.py, ai_assist.py, packages.py)
- Stdlib modules: `kryos/stdlib/` (22 Python files, each implementing a module)
- Runtime: `kryos/runtime/` (tensor.py, agents.py, probable.py, streams.py, lineage.py, cost.py, ffi.py)
- LSP builtins: `kryos/lsp/server.py` (103+ function signatures with docstrings)
- Test programs: `tests/programs/*.kry` (19 files demonstrating features)
- Examples: `examples/demo.kry`, `examples/neural_net.kry`, `examples/kryos_bootstrap.kry`
- Existing docs: `README.md` (28KB language overview)
- Token definitions: `kryos/compiler/tokens.py` (all keywords, operators)
- AST nodes: `kryos/compiler/ast_nodes.py` (all language constructs)

---

## Task 1: Manual Index

**Files:**
- Create: `docs/README.md`

**Content:** Table of contents linking to all chapters, stdlib modules, and appendices. Brief one-sentence description per entry. Include a "Quick Start" snippet (5 lines of Kryos showing hello world + a function).

**Step 1:** Read `README.md` for the language overview to understand what to highlight.
**Step 2:** Write `docs/README.md` with the full TOC and quick start.
**Step 3:** Commit: `docs: add manual index`

---

## Task 2: Getting Started

**Files:**
- Create: `docs/01-getting-started.md`
- Reference: `README.md`, `kryos/cli.py`, `setup.py`

**Content:**
- How to install Kryos (`pip install kryos` or from source)
- Running your first program (`kryos run hello.kry`)
- CLI commands overview (run, build, repl, lsp, test, fmt, check)
- File extension: `.kry`
- Hello World program
- A slightly more complex example (fibonacci + struct)
- Project structure conventions

**Step 1:** Read `kryos/cli.py` for all CLI commands and flags.
**Step 2:** Write the chapter.
**Step 3:** Commit: `docs: add getting started chapter`

---

## Task 3: Variables and Types

**Files:**
- Create: `docs/02-variables-and-types.md`
- Reference: `kryos/compiler/types.py`, `kryos/compiler/lexer.py`, `tests/programs/02_variables.kry`

**Content:**
- `let` and `let mut` — immutable by default, explicit mutability
- Type annotations vs inference
- All numeric types: i8-i128, u8-u128, f32, f64
- bool, str, char, none
- Array types: `[i32]`, `[str]`
- Map type
- Domain types: Tensor, Secret, Qubit, Probable
- Integer literal formats: decimal, hex (0x), binary (0b), octal (0o), underscore separators
- Float literals: decimal, scientific notation
- String literals: double quotes, interpolation `"Hello {name}"`, escape sequences
- Char literals: single quotes
- Type conversion: `to_string()`, `int()`, `float()`, `str()`
- `type_of()` for runtime type inspection
- Coming from Python: no `type()`, use `type_of()`
- Coming from Rust: no lifetime annotations, simpler type syntax
- Common mistake: forgetting `mut` when you need to reassign

**Step 1:** Read `kryos/compiler/types.py` for full type definitions.
**Step 2:** Read `kryos/compiler/lexer.py` for literal formats.
**Step 3:** Write the chapter.
**Step 4:** Commit: `docs: add variables and types chapter`

---

## Task 4: Functions

**Files:**
- Create: `docs/03-functions.md`
- Reference: `kryos/compiler/parser.py` (fn_decl parsing), `kryos/compiler/interpreter.py` (function calls), `tests/programs/03_functions.kry`, `tests/programs/10_closures.kry`, `tests/programs/17_anonymous_fn.kry`

**Content:**
- Function declarations: `fn name(param: Type) -> ReturnType { }`
- Parameters with type annotations
- Return values and `return` keyword
- Functions as first-class values: passing functions as arguments
- Closures: capturing surrounding scope
- Anonymous functions: `let f = fn(x) { return x * 2 }`
- Higher-order functions: functions returning functions (make_adder pattern)
- Pipe operator: `value |> transform |> process`
- Default parameter values (if supported)
- Recursive functions
- Coming from Python: `fn` not `def`, explicit types on params, braces not indentation
- Coming from Rust: no `->` on closures, simpler closure syntax
- Common mistake: forgetting return type annotation

**Step 1:** Read the parser for fn_decl and lambda parsing.
**Step 2:** Read test programs 03, 10, 17 for working examples.
**Step 3:** Write the chapter.
**Step 4:** Commit: `docs: add functions chapter`

---

## Task 5: Control Flow

**Files:**
- Create: `docs/04-control-flow.md`
- Reference: `kryos/compiler/parser.py`, `tests/programs/04_control_flow.kry`, `tests/programs/16_match.kry`

**Content:**
- `if` / `elif` / `else` — NOT `else if`
- `while` loops with conditions
- `for` loops: `for i in range(0, n)` and `for item in collection`
- `break` and `continue`
- `match` expressions: pattern matching on values, enums, wildcards `_`
- `match` as expression (returns a value)
- Nested control flow
- Coming from Python: `elif` same concept, braces required, no colon
- Coming from Rust: `elif` not `else if`, `match` uses `=>` not `=>`... actually same
- Common mistake: using `else if` instead of `elif`

**Step 1:** Read parser for if/while/for/match parsing.
**Step 2:** Read test programs 04, 16.
**Step 3:** Write the chapter.
**Step 4:** Commit: `docs: add control flow chapter`

---

## Task 6: Structs and Enums

**Files:**
- Create: `docs/05-structs-and-enums.md`
- Reference: `kryos/compiler/parser.py`, `kryos/compiler/interpreter.py`, `tests/programs/05_structs.kry`, `tests/programs/14_enums.kry`

**Content:**
- Struct declarations: `struct Point { x: f64, y: f64 }`
- Struct literals: `Point { x: 1.0, y: 2.0 }`
- Field access: `p.x`
- Impl blocks: `impl Point { fn distance(self: Point) -> f64 { } }`
- Self parameter: `self: TypeName` (not `&self`)
- Enum declarations: simple variants and variants with data
- Enum access: `Color.Red`, `Shape.Circle(5.0)`
- Pattern matching on enums with `match`
- Nested structs
- Struct + enum composition patterns
- Coming from Python: structs replace classes, `impl` replaces class methods
- Coming from Rust: `self: Type` not `&self`, enum access with `.` not `::`
- Common mistake: using `::` for enum variants instead of `.`

**Step 1:** Read test programs 05, 14 for working examples.
**Step 2:** Read interpreter for struct/enum implementation.
**Step 3:** Write the chapter.
**Step 4:** Commit: `docs: add structs and enums chapter`

---

## Task 7: Ownership

**Files:**
- Create: `docs/06-ownership.md`
- Reference: `kryos/compiler/ownership.py` (923 lines — the borrow checker)

**Content:** This is the most important chapter. It teaches the novel concept.
- Why ownership exists: memory safety without garbage collection
- Move semantics: what happens when you assign or pass a value
- The ownership rules: one owner at a time, owner cleanup
- Borrowing: accessing without taking ownership
- Mutability rules: `let` vs `let mut` and why immutable by default
- Use-after-move errors: what they are, how to fix them
- Double borrow prevention
- Ownership in function calls
- Ownership with arrays and structs
- Patterns: cloning to avoid moves, restructuring to avoid borrows
- How Kryos differs from Rust: no lifetime annotations, no `&` references, simpler model
- Coming from Python: everything is a reference in Python; Kryos values move
- Coming from Rust: same concept, simpler syntax, no lifetimes
- Common mistakes: trying to use a variable after passing it to a function

**Step 1:** Read `kryos/compiler/ownership.py` thoroughly — this IS the spec.
**Step 2:** Read `tests/test_ownership.py` (539 lines) for edge cases.
**Step 3:** Write the chapter with clear progressive examples.
**Step 4:** Commit: `docs: add ownership chapter`

---

## Task 8: Error Handling

**Files:**
- Create: `docs/07-error-handling.md`
- Reference: `kryos/compiler/self_heal.py`, `kryos/compiler/interpreter.py`, `tests/programs/15_try_catch.kry`

**Content:**
- try/catch/throw syntax
- Throwing custom error values
- Nested try/catch
- The self-healing runtime: what it auto-recovers (div by zero, index OOB, type coercion)
- When self-healing kicks in vs when errors propagate
- Self-healing vs try/catch: when to use each
- Error types: LexerError, ParseError, TypeError, OwnershipError, KryosRuntimeError
- AI-assisted error explanation (the error explainer)
- Coming from Python: similar try/except but `catch` not `except`, throw not raise
- Coming from Rust: try/catch replaces Result/? pattern
- Common mistake: not catching errors from I/O operations

**Step 1:** Read `kryos/compiler/self_heal.py` for recovery strategies.
**Step 2:** Read test program 15.
**Step 3:** Write the chapter.
**Step 4:** Commit: `docs: add error handling chapter`

---

## Task 9: Traits and Generics

**Files:**
- Create: `docs/08-traits-and-generics.md`
- Reference: `kryos/compiler/parser.py` (trait_decl), `kryos/compiler/interpreter.py` (trait dispatch)

**Content:**
- Trait declarations: `trait Printable { fn display(self: Self) -> str }`
- Implementing traits: `impl Printable for Point { ... }`
- Trait methods and default implementations
- Generic functions with trait bounds
- When to use traits vs enums
- Dynamic dispatch
- Common patterns: Printable, Comparable, Serializable
- Coming from Python: traits replace abstract base classes
- Coming from Rust: similar concept, slightly different syntax

**Step 1:** Read parser for trait declarations.
**Step 2:** Read interpreter for trait dispatch.
**Step 3:** Write the chapter.
**Step 4:** Commit: `docs: add traits and generics chapter`

---

## Task 10: Concurrency

**Files:**
- Create: `docs/09-concurrency.md`
- Reference: `kryos/compiler/interpreter.py` (spawn), `kryos/runtime/agents.py`, `tests/programs/19_spawn.kry`

**Content:**
- `spawn { ... }` for parallel execution
- Actor declarations: `actor Name { state, on handlers }`
- Message passing between actors
- `sleep(seconds)` for timing
- Coordination patterns
- State management in concurrent code
- Error handling in spawned tasks
- Common patterns: producer-consumer, worker pool, pipeline
- Coming from Python: spawn replaces threading/asyncio
- Coming from JS: spawn replaces Promises/async-await
- Common mistake: shared mutable state without actors

**Step 1:** Read test program 19 and interpreter spawn handling.
**Step 2:** Read `kryos/runtime/agents.py` for actor model.
**Step 3:** Write the chapter.
**Step 4:** Commit: `docs: add concurrency chapter`

---

## Task 11: Capabilities

**Files:**
- Create: `docs/10-capabilities.md`
- Reference: `kryos/compiler/capabilities.py` (784 lines)

**Content:**
- What capability-based security means
- `@capabilities(compute)` — pure computation only
- `@capabilities(network)` — network access
- `@capabilities(filesystem)` — file system access
- `@capabilities(gpu)` — GPU compute
- `@capabilities(ffi)` — foreign function interface
- Combining capabilities: `@capabilities(compute, network)`
- Functions with no @capabilities can only do pure computation
- Capability auditing: verifying what functions access
- Sandboxing: running untrusted code with restricted capabilities
- Why this matters for security
- Real-world patterns: secure API handler, data processor

**Step 1:** Read `kryos/compiler/capabilities.py` for the full model.
**Step 2:** Read `tests/test_capabilities.py` and test program 09, 13.
**Step 3:** Write the chapter.
**Step 4:** Commit: `docs: add capabilities chapter`

---

## Task 12: Comptime

**Files:**
- Create: `docs/11-comptime.md`
- Reference: `kryos/compiler/comptime.py` (215 lines)

**Content:**
- What comptime blocks do: execute code at compile time, embed results as constants
- Syntax: `comptime { expression }`
- Use cases: lookup tables, constant computation, configuration
- What can run at comptime vs what can't
- Performance benefits
- Comparison with Rust's `const fn` and Zig's `comptime`
- Coming from C: comptime replaces `#define` and preprocessor macros
- Common mistake: trying to do I/O inside comptime blocks

**Step 1:** Read `kryos/compiler/comptime.py` and `tests/test_comptime.py`.
**Step 2:** Write the chapter.
**Step 3:** Commit: `docs: add comptime chapter`

---

## Task 13: Modules and Packages

**Files:**
- Create: `docs/12-modules-and-packages.md`
- Reference: `kryos/compiler/packages.py` (658 lines)

**Content:**
- `use` statements for importing
- Module system and resolution
- Package manager: init, add, remove, update, publish
- Package manifest format
- Semver resolution
- Private vs public modules
- Project structure conventions

**Step 1:** Read `kryos/compiler/packages.py` for the full package system.
**Step 2:** Write the chapter.
**Step 3:** Commit: `docs: add modules and packages chapter`

---

## Task 14: FFI

**Files:**
- Create: `docs/13-ffi.md`
- Reference: `kryos/runtime/ffi.py` (296 lines)

**Content:**
- Python FFI: calling Python from Kryos
- C FFI: calling C from Kryos
- Data type marshalling between languages
- @capabilities(ffi) requirement
- Safety considerations
- Practical examples: using numpy, calling system libraries

**Step 1:** Read `kryos/runtime/ffi.py`.
**Step 2:** Write the chapter.
**Step 3:** Commit: `docs: add FFI chapter`

---

## Task 15: AI Runtime

**Files:**
- Create: `docs/14-ai-runtime.md`
- Reference: `kryos/runtime/tensor.py` (1,234 lines), `kryos/runtime/agents.py`, `kryos/runtime/probable.py`, `kryos/runtime/streams.py`, `kryos/runtime/lineage.py`, `kryos/runtime/cost.py`

**Content:**
- Tensors: creation (zeros, ones, rand, randn), operations (matmul, softmax, relu, sigmoid)
- Autodiff: automatic differentiation, @differentiable
- Agents: autonomous entities with memory (working, episodic, semantic)
- Probability types: `Probable<T>` with confidence scores
- Reactive streams: lazy composable data streams
- Data lineage: provenance tracking
- Cost tracking: computation budgets
- Example: building a neural network in Kryos

**Step 1:** Read all runtime module files.
**Step 2:** Read `examples/neural_net.kry` for practical patterns.
**Step 3:** Write the chapter.
**Step 4:** Commit: `docs: add AI runtime chapter`

---

## Task 16: Codegen

**Files:**
- Create: `docs/15-codegen.md`
- Reference: `kryos/compiler/codegen.py` (1,849 lines)

**Content:**
- LLVM compilation pipeline
- `kryos build file.kry` to produce native binary
- What gets compiled vs interpreted
- Optimization levels
- Target architectures
- Cross-compilation (if supported)
- Debugging compiled output

**Step 1:** Read `kryos/compiler/codegen.py` and `tests/test_codegen_extended.py`.
**Step 2:** Write the chapter.
**Step 3:** Commit: `docs: add codegen chapter`

---

## Task 17: Stdlib — Core Builtins

**Files:**
- Create: `docs/stdlib/README.md`
- Create: `docs/stdlib/core-builtins.md`
- Reference: `kryos/compiler/interpreter.py` (builtin implementations), `kryos/lsp/server.py` (signatures + docs)

**Content:** Full API reference for all always-available builtins:
- I/O: println, print, stdin_read
- Math: abs, sqrt, sin, cos, tan, log, pow, floor, ceil, min, max, round, log10, random, pi, e
- Strings: len, char_at, char_code, char_from, substr, contains, starts_with, ends_with, upper, lower, trim, split, join, replace
- Arrays: len, push, pop, range
- Conversion: to_string, int, float, str, type_of
- Assert: assert

Each function: signature, description, example, edge cases, see also.

**Step 1:** Read `kryos/lsp/server.py` for all builtin signatures.
**Step 2:** Read interpreter for implementation details.
**Step 3:** Write stdlib README (module index) and core-builtins.md.
**Step 4:** Commit: `docs: add stdlib core builtins reference`

---

## Task 18: Stdlib — Modules Batch 1 (io, net, crypto, term, server)

**Files:**
- Create: `docs/stdlib/io.md`
- Create: `docs/stdlib/net.md`
- Create: `docs/stdlib/crypto.md`
- Create: `docs/stdlib/term.md`
- Create: `docs/stdlib/server.md`
- Reference: `kryos/stdlib/io_module.py`, `kryos/stdlib/net_module.py`, `kryos/stdlib/crypto_module.py`, `kryos/stdlib/term_module.py`, `kryos/stdlib/server_module.py`

**Content:** Full API reference for each module. Read each stdlib .py file for function signatures, parameters, and behavior. Each function gets: signature, description, example, edge cases, see also.

**Step 1:** Read each module source file.
**Step 2:** Write all 5 files.
**Step 3:** Commit: `docs: add stdlib io, net, crypto, term, server reference`

---

## Task 19: Stdlib — Modules Batch 2 (collections, json, map, string, process, regex)

**Files:**
- Create: `docs/stdlib/collections.md`
- Create: `docs/stdlib/json.md`
- Create: `docs/stdlib/map.md`
- Create: `docs/stdlib/string.md`
- Create: `docs/stdlib/process.md`
- Create: `docs/stdlib/regex.md`
- Reference: `kryos/stdlib/collections.py`, `kryos/stdlib/json_module.py`, `kryos/stdlib/map_module.py`, `kryos/stdlib/string_ext_module.py`, `kryos/stdlib/process_module.py`, `kryos/stdlib/regex_module.py`

**Step 1:** Read each module source file.
**Step 2:** Write all 6 files.
**Step 3:** Commit: `docs: add stdlib collections, json, map, string, process, regex reference`

---

## Task 20: Stdlib — Modules Batch 3 (email, auth, stripe, db, claude, config, datetime, set, math)

**Files:**
- Create: `docs/stdlib/email.md`
- Create: `docs/stdlib/auth.md`
- Create: `docs/stdlib/stripe.md`
- Create: `docs/stdlib/db.md`
- Create: `docs/stdlib/claude.md`
- Create: `docs/stdlib/config.md`
- Create: `docs/stdlib/datetime.md`
- Create: `docs/stdlib/set.md`
- Create: `docs/stdlib/math.md`
- Reference: corresponding `kryos/stdlib/*_module.py` files

**Step 1:** Read each module source file.
**Step 2:** Write all 9 files.
**Step 3:** Commit: `docs: add stdlib email, auth, stripe, db, claude, config, datetime, set, math reference`

---

## Task 21: Appendix

**Files:**
- Create: `docs/appendix/keywords.md`
- Create: `docs/appendix/operators.md`
- Create: `docs/appendix/attributes.md`
- Create: `docs/appendix/coming-from.md`
- Reference: `kryos/compiler/tokens.py` (keywords, operators), `kryos/compiler/parser.py` (attributes)

**Content:**
- `keywords.md`: All 41 keywords in a table with brief description
- `operators.md`: Full operator precedence table (arithmetic, comparison, logical, bitwise, pipe, assignment)
- `attributes.md`: All `@` attributes (@capabilities, @compute, @export, @differentiable, @zero_copy, @real_time, @target, @layout, @no_std)
- `coming-from.md`: Quick reference tables — "In Python you do X, in Kryos you do Y" for Python, Rust, JavaScript, C

**Step 1:** Read `kryos/compiler/tokens.py` for all keywords and operators.
**Step 2:** Write all 4 appendix files.
**Step 3:** Commit: `docs: add appendix (keywords, operators, attributes, coming-from)`

---

## Task 22: Final Review

**Step 1:** Verify all 41 files exist and have content.
**Step 2:** Check all internal links in README.md resolve to real files.
**Step 3:** Spot-check 5 random code examples for correct Kryos syntax (fn not def, elif not else if, no semicolons).
**Step 4:** Final commit: `docs: complete Kryos language manual v1.0`
