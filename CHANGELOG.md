# Changelog

All notable changes to Kryos will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [1.0.0] - 2026-05-14 — "production"

First stable release. Same code as 0.5.0 with a 1.0 version stamp,
committing Kryos to the stability guarantees in `docs/STABILITY.md`.

From this release forward:

* The lexical grammar, the `pub` standard library, the documented
  builtins, the `kryos.toml` schema, and the `kryos` CLI subcommand
  set are stable. Breaking changes require a 2.0.0 bump.
* The `2026` language edition is the default for projects that omit
  `edition` from their manifest.
* Patch releases (`1.0.z`) fix bugs without changing behaviour. Minor
  releases (`1.y.0`) may add features and APIs but never change the
  meaning of existing code.
* Deprecations carry a warning for at least one minor cycle before
  removal in a future major.

No functional changes from 0.5.0 — see the entry below for the full
list of what shipped in this push.

## [0.5.0] - 2026-05-14 — "universal language"

The production-ready push. Kryos can now write the things it was designed
to write: HTTP servers, MCP servers, LLM agents, static site generators,
persistent databases, parallel job pools, and small compiler tools — all
in pure Kryos. The plumbing required to ship and run those programs is
also in place: a package manager with local path dependencies, prebuilt
binary distribution, a stable VS Code LSP client, and a written stability
policy.

### Added

#### Showcase apps (all runnable end-to-end)
- `examples/showcase/rest_api.kry` — full CRUD HTTP server using real
  mutable module-level globals; verified against curl.
- `examples/showcase/markdown.kry` — pure-Kryos markdown→HTML converter.
- `examples/showcase/kvdb.kry` — append-only persistent key/value store
  with tab/newline-safe percent encoding, in-memory replay, and compaction.
- `examples/showcase/mcp_server.kry` — real Model Context Protocol
  server speaking JSON-RPC 2.0 over stdio. Implements `initialize`,
  `tools/list`, `tools/call`, `shutdown`. Built-in tools: `echo`, `now`,
  `add`, `read_file`, `write_file`, `http_get`.
- `examples/showcase/agent.kry` — OpenAI-compatible Chat Completions
  agent with tool-use loop. Drives multi-turn conversations through
  function calling; falls back to an offline demo that prints the
  exact OpenAI wire-format request.
- `examples/showcase/ssg.kry` — static site generator: inlined
  markdown→HTML, layout template, manifest-driven build. Emits a real
  multi-page HTML site plus a shared `style.css`.
- `examples/showcase/worker_pool.kry` — fan-out/fan-in concurrency
  showcase using `spawn` plus channels and sentinel-based shutdown.
- `examples/showcase/kdoc.kry` — a small documentation extractor
  written in Kryos itself. Scans `.kry` files for `pub` declarations
  and emits a Markdown API reference. Satisfies the self-host milestone.

#### Language and compiler
- **Real mutable module-level globals.** `let mut <name>: <type> = <expr>`
  at file scope, no workarounds, with proper MIR type inference.
- **String comparison codegen.** `<`, `>`, `<=`, `>=` on strings now
  lower through `kryos_string_compare(a, b) -> i64` and `icmp`.
  Available in both the AOT and JIT backends.
- **f64↔i64 round-trips in codegen** for `json_number` and friends.
- **Mutable globals participate in type inference** for indexing and
  assignment.

#### Package manager
- `parse_dep_string` accepts bare relative/absolute paths (`./foo`,
  `../foo`, `/abs`) and an explicit `path:<dir>` form in addition to
  the existing `<source>@<version>` form.
- Driver import resolver: walks up from each source file looking for
  `.kryos/deps/<pkg>.redirect` written by `kryos pkg install`, parses
  the `path = "..."` entry, and resolves `use pkg` to `<dep>/src/lib.kry`
  (or `<dep>/src/<pkg>.kry`) and `use pkg::a::b` to `<dep>/src/a/b.kry`.
  Verified end-to-end with two side-by-side projects.

#### Distribution
- `install.sh` / `install.ps1` already shipped — now coupled with the
  release workflow that builds prebuilt binaries for
  `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`,
  `x86_64-apple-darwin`, and `aarch64-apple-darwin` when a `v*` tag is
  pushed.
- New `.github/workflows/cross.yml`: cheap cross-build matrix
  (`linux-gnu`, `linux-musl`, `windows-gnu`, `aarch64-linux-gnu`) on
  every push.

#### Editor support
- VS Code extension v0.3.0 wires up the LSP client. Launches
  `kryos lsp` over stdio with `vscode-languageclient`. Configurable
  via `kryos.serverPath`, `kryos.serverArgs`, and `kryos.trace.server`.

#### Documentation
- `docs/STABILITY.md` — written stability policy: SemVer, what's stable
  vs. internal, deprecation lifecycle, and the language-edition
  mechanism (`edition = "2026"` is the current default).
- `docs/12-modules-and-packages.md` — appended a verified local-path-dep
  walkthrough.

### Fixed
- `infer_expr_type` now consults `ctx.mutable_globals` so indexing a
  global `[str]` array returns a `str`, not a pointer-sized `i64`. This
  unblocked the kvdb showcase and similar code that holds collections
  in a global.

## [0.4.0] - 2026-05-11 — "credible beta"

This is the release that takes Kryos from a hand-rolled toy compiler to a
language that can credibly be tried by someone other than its author.
Every item below ships with documentation, tests, or a runnable demo;
nothing in this release is marked experimental.

### Added

#### Reliability
- **Runtime panics carry source spans.** Every runtime panic (overflow,
  division by zero, array-OOB, stack overflow, etc.) now points at the
  `file:line:col` where it originated rather than at the runtime crate
  internals.
- **Stack-overflow detection** via a `SIGSEGV` alt-stack handler that
  distinguishes recursion blow-outs from generic segfaults and reports
  them with a friendlier message + the offending span.
- **Integer-overflow policy** is now defined and documented in
  `docs/16-integer-overflow.md`: `wrapping_*` / `checked_*` /
  `saturating_*` builtins are available, signed overflow with the plain
  `+ - *` operators is well-defined as wrap-on-release, panic-on-debug.
- **Unsafe-block audit** in `docs/17-unsafe-audit.md`: every `unsafe`
  region in the runtime and native stdlib (8 patterns across 8 files)
  has a documented invariant.

#### Tooling
- **`kryos explain ERRXXXX`** with 20 long-form error articles (modelled
  on `rustc --explain`). Each includes a broken example, a fixed example,
  and the rationale behind the diagnostic. Run `kryos explain --list` for
  the catalog.
- **`kryos test` cargo-parity**: positional `FILTER` argument,
  `--exact`, `--nocapture`, `--list`, and `--format=json` for
  newline-delimited JSON output that mirrors
  `cargo test --format=json` events.
- **`kryos build --target=<triple>`** is now wired through to LLVM
  rather than silently using the host triple. Eleven known-good targets
  ship with descriptions; `--target=help` prints the table. See
  `docs/18-cross-compilation.md` for required toolchains and known
  failure modes.
- **Benchmark suite** under `benchmarks/` covering mandelbrot, n-body,
  binary-trees, fannkuch, matmul, and fib against Rust and C baselines.
  `benchmarks/run.sh` produces a reproducible `RESULTS.md`; on the
  reference hardware Kryos hits parity with C on mandelbrot (1.03×) and
  stays within 3.5×4.5× on the numeric benchmarks.

#### Documentation
- **`docs/19-language-reference.md`** — the authoritative v0.4 language
  spec: lexical structure, type system, expression grammar (with the
  full precedence table), control flow, declarations, pattern matching,
  ownership / drop order, integer overflow, concurrency, unsafe code,
  modules, panics, and a conformance checklist.
- **`docs/BUGS.md`** records the one known-leaky pattern in the v0.4
  ownership checker (string-field move across struct-returning function
  boundaries) along with its workaround.

#### Showcase suite
Five end-to-end programs under `examples/showcase/` proving the
language can be used to build the kinds of things it claims to support:

- `cli_tool.kry`       — grep-style CLI with POSIX exit codes.
- `parser.kry`         — recursive-descent calculator with error
  reporting (source columns, three failure modes).
- `bytecode_vm.kry`    — stack VM with a 13-opcode ISA, disassembler,
  and three demo programs (sum 1..10, factorial(7), fib(10)).
- `agent_runtime.kry`  — LLM-style tool-use loop: history, planner,
  tool registry, bounded step budget.
- `web_server.kry`     — minimal HTTP/1.0 server using `tcp_listen` /
  `tcp_accept` / `tcp_send`, serving HTML / JSON / 404 routes.

See `examples/showcase/README.md` for run instructions.

### Changed
- Workspace version bumped to `0.4.0` across all crates.
- The test runner library now exposes `RunOptions`, `run_test_with`,
  `run_all_with`, `run_annotated_tests_with`, and `format_report_json`
  in addition to the existing entry points. Existing callers keep
  working unchanged.
- 843 workspace tests now pass (up from 831 at the start of the v0.4
  cycle); +12 from new unit tests across the `kryos test`, `explain`,
  and `build --target` work.

### Status

Kryos v0.4.0 is the **credible-beta** release: the toolchain is
complete enough that someone other than the author can clone it, build
it, follow the docs, and write real programs. Real users and a stable
1.0 API still ahead.

---

## [0.3.6] - 2026-05-11

### Fixed
- CI green again: resolved clippy errors introduced in the LLVM aggregate-ABI and Cranelift drop-path commits (`collapsible_match`, `too_many_arguments`, `if_same_then_else`) via targeted `#[allow]` attributes; no behavior change.
- `rustfmt` drift across `kryos-codegen-cranelift`, `kryos-codegen-llvm`, `kryos-mir`, `kryos-stdlib-native`, `kryos-types` -- all formatted with `rustfmt 1.95`.

### Changed
- Repository home: all `FrostbyteDevTeam/kryos-lang` URLs in README, docs, install scripts, `Cargo.toml`, VS Code extension, and contributing guide updated to `NORTHTEKDevs/kryos-lang`.
- `README.md`: replaced the misleading "~48 GB RAM" debug-build warning with a calibrated build-footprint note (~6 GB disk, ~3 GB peak RAM with `-j 2`, ~2 min cold). Documented that LLVM is **not** a build dependency -- the LLVM backend emits IR as text.
- `README.md` quick-start example path now points at `../examples/hello.kry` (the previously referenced `examples/proof.kry` did not exist).
- Bumped to `v0.3.6` across `Cargo.toml`, `install.ps1`, `docs/01-getting-started.md`, `docs/WHY_KRYOS.md`.

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
