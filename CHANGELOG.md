# Changelog

All notable changes to Kryos will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.3.5] - 2026-04-16

### Fixed
- `MirType::Map` sentinel migration: replaced `Ptr(Str)` map-handle hack with typed `Map { key, value }` variant throughout MIR, lowering, and both backends
- REPL `:type` map inference: map literals now report the actual key/value element types instead of always `Map<i64, i64>`
- REPL `:type` index inference: indexing into a `Map<K, V>` now returns `V` instead of `i64`
- Package registry: `parse_index_entry` now parses the `deps` JSON object into the dependency map; transitive dependency resolution from registry responses now works
- `NodeTable::get_mut` and `::remove` dead_code warnings suppressed in `kryos-stdlib-native/src/json.rs` -- intentional forward-facing API surface

### Changed
- `README.md`: corrected version badge from v0.3.4 to v0.3.5
- `CONTINUE.md` (internal dev artifact) replaced with `ARCHITECTURE.md` for public distribution
- `examples/README.md`: documented all 20 examples (was 12); added blocking notes for `http_api.kry` and `mcp_server.kry`

---

## [0.3.4] - 2026-04-14

### Added
- `float(str)` builtin -- parse a string as f64 via `kryos_builtin_parse_float` (Cranelift backend)
- Example: `ai_agent.kry` -- research agent using the Kryos agent framework with Anthropic API integration
- Example: `http_api.kry` -- in-memory task-list REST API with routing and JSON responses
- Example: `mcp_server.kry` -- Model Context Protocol server over stdio (JSON-RPC 2.0)

### Fixed
- MIR match arm type inference: enum variant field types now correctly propagate to the result local (fixes f64 fields inferred as i64 in `JsonValue::Number(n) => n` patterns)
- JSON stdlib: `if/else if/else` chains in `_parse_string`, `_parse_number`, and `_escape_string` converted to sequential `if` + flag pattern (avoids compiler branch target bug in deep else-if chains)
- JSON parser: `@copy` on `Parser` struct prevents ownership errors across recursive descent calls

---

## [0.3.3] - 2026-04-14

### Added
- `Self` type in trait method signatures resolves to the implementing type at each call site
- `Type::method(args)` associated function syntax (`StaticMethodCall` AST node, parsed, type-checked, MIR-lowered, both backends)
- `install.ps1` Windows PowerShell installer
- `CONTRIBUTING.md` developer guide with compiler pipeline walkthrough

### Fixed
- Clippy: `&param_ty` double-reference in `kryos-types/src/check.rs:1421` (immediate deref lint)
- Version bump: `compiler/Cargo.toml` 0.2.1 -> 0.3.3

---

## [0.3.2] - 2026-04-13

### Added
- Developer adoption sprint: stdlib completions, string safety improvements, DX ergonomics
- Module system for stage-0 self-host build (`use` imports in bootstrap)
- Calling closures stored as struct fields
- Correct MirType for fn-typed captures in lambda thunks

---

## [0.3.1] - 2026-04-12

### Added
- `@pure` attribute optimization -- CSE (common subexpression elimination) and dead call elimination at MIR level
- `@test` annotation runner -- discover and JIT-execute `@test` functions via `kryos test`

### Fixed
- REPL state persistence -- `use`/`type`/`extern`/`actor`/`pub` classified as declarations, persist across lines
- Array element drop recursion -- named type drop helpers for struct/enum fields (prevents infinite recursion)
- Closure capture memory leak -- per-closure dropper thunks generated for ARC env cleanup

---

## [0.2.2] - 2026-04-09

### Fixed
- Deep memory safety pass: ownership cloning, Shared drop, @copy ARC retain
- String interpolation intermediate leak
- try/catch result enum leak
- LLVM backend drop parity (enum, struct, array, map, function)
- Const eval overflow: checked arithmetic, unfoldable at compile time
- Formatter: doc comments preserved on Actor, TypeAlias, Import, Extern declarations

---

## [0.2.1] - 2026-04-08

### Fixed
- Critical memory safety, control flow, and type system fixes
- Exception cleanup includes MirType::Enum in droppable filter
- CI/CD GitHub Actions matrix (Ubuntu + Windows + macOS)
- Clippy clean (0 warnings)
- @copy struct deep-copy: Function/Shared fields call kryos_arc_retain
- ActorSend: heap-typed args cloned before send

---

## [0.2.0] - 2026-04-08

### Self-Hosting Milestone
- 18,700-line self-hosted compiler written in Kryos (15 files)
- Full compilation pipeline: lexer, parser, type checker, MIR lowering, optimizer, register allocator, x86_64 codegen, ELF/COFF linker
- Zero-dependency runtime (runtime.kry): raw Linux x86_64 syscalls, bump allocator, byte buffers, string/array/map operations
- 3-stage bootstrap verification script (stage-0 Rust -> stage-1 -> stage-2 -> stage-3, SHA-256 identity proof)
- Stage-1 binary: 1MB PE32+ executable, compiles and runs Kryos programs
- Self-host type-checks cleanly (0 errors) through the Rust compiler via concatenation

### Module System
- File-based module resolution with `use` imports
- Stdlib resolution via `use std::math`, `use std::json`, etc.
- Selective imports: `use std::math::{abs, min, max}`
- Transitive imports with diamond deduplication and cycle detection
- Sibling file and directory module (`foo/mod.kry`) resolution
- Const declarations now importable by name

### Capability Enforcement
- 35 builtin functions mapped to 7 capability categories (io, net, process, term, crypto, time, ffi)
- Deny-by-default enforcement within `@capabilities`-annotated scopes
- Cross-function capability propagation (caller must have callee's required capabilities)
- Opt-in design: unannotated functions have ambient authority (backward compatible)

### LLVM Backend
- Fixed systematic ptr/i64 type mismatch in LLVM IR emitter
- Added `coerce_value` helper for type-safe conversions at 15+ boundary points
- Fixed identity copy pattern (`add ptr` -> `getelementptr i8`) for pointer types
- Fixed `to_string` return type coercion and float argument dispatch
- LLVM tools available on Windows (clang 21.1.8, lld-link)

### Compiler Fixes
- Generic functions now monomorphize per call site (fresh type variables)
- Multiple trait impls no longer clobber each other's `self` type
- `throw` propagates across function boundaries via thread-local exception state
- `to_string()` on strings returns the string (not the raw pointer address)
- `sqrt`, `floor`, `ceil`, `abs` use native Cranelift instructions (fixes ICE)
- MIR type inference for untyped constants (no longer defaults to I64)
- Cranelift float/int type coercion uses proper bitcast instructions
- `kryos pkg init` now creates files on disk (kryos.toml, src/main.kry, .gitignore, README.md)
- `kryos check` now supports `--skip-ownership` flag

### Added
- Array concatenation operator: `a + b` and `a += b` for arrays (type-checked, MIR-lowered, both backends)
- `kryos_array_concat` runtime function for array concatenation
- Closure environments heap-allocated via `malloc` (fixes segfault when closures escape their creating function)
- `push(arr, val)` and `pop(arr)` now borrow the array instead of moving it in ownership analysis
- Native test runner prefers release binary over debug (matches `--release` build workflow)
- `StoreField` MIR instruction for proper struct field mutation (replaces `__kryos_field_store` hack)
- Full `StoreField` implementation in both Cranelift and LLVM backends
- `--skip-ownership` CLI flag for self-host bootstrap (ownership checker fires on refcounted patterns)
- `kryos_string_char_at` runtime function for string indexing
- `no_struct_lit` parser flag to prevent struct literal ambiguity in if/while/for/match conditions
- `parse_expr_no_struct_lit()` parser function used in all conditional contexts
- Array/tuple codegen now uses runtime `kryos_array_new`/`push`/`get` for consistency
- Array size coercion: fixed-size arrays assignable to dynamic arrays (`[T; N]` -> `[T]`)
- Division-by-zero check widened to i64 for narrow integer types
- Float-to-int and int-to-float bitcasting in function call argument coercion
- IndexAccess type inference for arrays, tuples, and strings in MIR lowering
- MIR elif duplicate block fix (prevents self-loop when last elif has no else)
- New example: `word_count.kry`
- Package registry now computes deterministic content hash (replaces TODO placeholder)

### Fixed
- Demo example: removed unimplemented tensor extern calls that caused segfault
- Calculator example: added `**` (power) operator to string-matched calculator
- Clippy: removed dead code, unused imports, function-cast-as-integer warnings
- Clippy: fixed prefix-stripping pattern in semver parser

### Changed
- Self-host MIR: array concatenation (`arr + [elem]`) replaced with `push(arr, elem)` for efficiency
- Self-host main: `std.io.read_file` -> `file_read`, `std.process.args()` -> `args()` (runtime functions)
- Self-host codegen: `&&`/`||` -> `and`/`or` (correct Kryos syntax), `char_at` -> `char_code(substr(...))`
- Bootstrap script upgraded from 2-stage to proper 3-stage verification (stage-2 == stage-3)
- FFI crates (`kryos-rt`, `kryos-stdlib-native`) now properly document safety and suppress raw-pointer clippy lints

## [0.1.1] - 2026-04-07

### Fixed
- Parser struct-literal ambiguity: `match TK_EOF { ... }` no longer parsed as struct literal
- Struct field access segfaults: structs now heap-allocated (malloc) instead of stack slots
- String match patterns: `match s { "hello" => ... }` now emits equality-comparison chain instead of integer switch
- Tail expression return: functions ending with bare `match`/`if` now implicitly return the result
- String concatenation with non-string operands: automatic coercion via `coerce_to_string()` helper
- Double-free prevention: `dropped_locals` tracking prevents nested scope re-drops
- ComptimeBlock type inference: `comptime { expr }` now infers correct result type
- Copy semantics for computed expressions: BinaryOp, FnCall, MatchExpr, IfExpr, UnaryOp, MethodCall, Cast, IndexAccess, Block, PipeExpr, and Borrow/Deref now correctly report copy when result is a primitive type
- `type_of()` builtin: compile-time type dispatch for all MIR types (f64, bool, str, etc.) instead of always returning "i64"
- `assert()` builtin: accepts 1 or 2 args, bool conditions extended to i64, default "assertion failed" message

### Added
- 4 new example programs: calculator, word_count, json_counter, all_features showcase
- String pattern matching in match expressions (via BinOp::Eq chain with Branch terminators)
- Implicit return for tail expressions in non-void functions
- `fn main()` wrapper for kryos_bootstrap.kry self-hosting lexer example
- Criterion benchmark suite: 9 groups (lex, parse, typecheck, ownership, capabilities, MIR, codegen, pipeline, JIT fibonacci)
- 9 new ownership analysis tests for copy semantics validation
- `is_type_expr_copy()` helper for cast expression type analysis

### Changed
- Ownership analyzer `expr_is_copy()` now recursively handles 15+ expression types
- Type checker: `assert()` signature updated to accept `bool` condition, 1-arg special case
- Type checker: `type_of()` parameter type set to `Error` (accepts any type)
- Documentation: fixed incorrect builtin names (`int`/`float`/`str` → `parse_int`/`parse_float`/`to_string`)
- Documentation: updated tail expression return note in Functions chapter
- Documentation: added implementation status callouts for borrowing and self-healing runtime
- Documentation: fixed `dyn Trait` implementation status (vtable-based dispatch is implemented)
- Standard library stubs: fixed broken references in math, string, collections, crypto, fmt, http, json, net, and test modules
- README: updated version to v0.1.1, added all_features example

## [0.1.0] - 2026-04-07

### Added
- 21-crate Rust compiler (49,000+ lines)
- Dual backends: Cranelift (fast debug builds) and LLVM (optimized release builds)
- Ownership-based memory safety without lifetime annotations
- Compile-time capability enforcement (deny-by-default resource access)
- Compile-time evaluation with `comptime` blocks
- Type inference with explicit annotations where needed
- Pattern matching with integer, string, enum, and wildcard patterns
- Dynamic dispatch via `dyn Trait` (vtable-based)
- Generics with monomorphization
- Concurrency: `spawn`, typed channels, actors, `select`
- 5 MIR optimization passes: constant folding, dead code elimination, function inlining, tail-call optimization, strength reduction
- 28 standard library modules (strings, math, collections, I/O, networking, crypto, JSON, regex, datetime, tensors, agents, probability, reactive streams)
- Ergonomic builtins: `file_read`, `file_write`, `env_get`, `time_now`, `assert`, `parse_int`, `parse_float`, `type_of`
- Error handling with `try`/`catch`/`throw`
- VS Code extension with syntax highlighting, snippets, and language configuration
- Language Server Protocol (LSP) server
- Code formatter (`kryos fmt`)
- Documentation generator (`kryos doc`)
- Package manager (`kryos pkg`)
- Test runner (`kryos test`)
- Interactive REPL (`kryos repl`)
- C header binding generator (`kryos bindgen`)
- Native tensor runtime with 38 FFI operations
- GitHub Actions CI (build, test, clippy, fmt on Linux and Windows)
- 13 example programs
- 15-chapter language manual
- 680+ tests, all passing
