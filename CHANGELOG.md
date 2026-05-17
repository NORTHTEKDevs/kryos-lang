# Changelog

All notable changes to Kryos will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [2.5.1] - 2026-05-17 — "Generics: correct return-type substitution"

A correctness fix in monomorphization. No new language surface, no
breaking changes. Smoke tests still 15/15, router 21/21, config 12/12,
all 7 prior real-program examples still build.

### Fixed

- **Generic functions returning `T` extracted from a generic-typed
  parameter crashed on return.** When a generic function declared
  `fn first<T>(items: [T]) -> T` was called with a concrete array, the
  call-site return-type inference and the monomorphization step both
  only recognised `T` when it appeared as a bare `Simple` type at the
  parameter level. They missed `T` nested inside `[T]`, `(A, B)`,
  `fn(T) -> U`, `&T`, `Ptr<T>`, and `Shared<T>`. The fallback path
  resolved the un-substituted `T` to `MirType::Struct("T")`, which
  caused the caller to treat plain `i64` results as heap pointers and
  call `free()` on them at function-exit drop, segfaulting.
  Replaced the two ad-hoc match-only-`Simple` substitution sites in
  `compiler/crates/kryos-mir/src/lower.rs` with recursive helpers
  `extract_type_bindings` and `substitute_type_expr_to_mir` that walk
  compound type shapes.
  Affected programs: any use of `fn f<T>(xs: [T]) -> T`,
  `fn pair<A, B>(...) -> A`, etc. Minimal reproducer:
  `fn first<T>(items: [T]) -> T { return items[0] } fn main() { println(to_string(first([10, 20, 30]))) }`
  printed `10` and then `kryos panic: stack overflow`.

### Added

- **`KRYOS_DUMP_IR=1` environment variable** dumps Cranelift IR for
  every AOT-compiled function to stderr. Companion to the existing
  `KRYOS_JIT_DUMP_IR=1` for the JIT path. Useful for debugging
  codegen-level issues like the one fixed above.

## [2.5.0] - 2026-05-17 — "Test runner, importable libraries, JIT correctness"

This release closes the gap between AOT and JIT compilation paths, makes
`kryos test` work on single files and on libraries imported via `use`,
and adds two new real-world example libraries with full @test suites.
All 15 smoke tests, 21 router tests, and 12 config-parser tests pass.

No breaking changes. No new language surface.

### Added

- **`kryos test PATH`** now accepts a single `.kry` file or a directory
  as a positional argument. `--path` is the new explicit flag; if the
  positional argument is an existing file or directory it is treated
  as a path, otherwise as a name filter (existing behaviour preserved).
  `kryos-test-runner` gained `discover_tests_in_file`,
  `discover_annotated_tests_in_file`, and
  `run_annotated_tests_in_file` for per-file discovery.

- **Test runner uses `compile_file` instead of `compile_source`** so
  `use module` imports resolve correctly when running @test functions
  in a file that pulls in a sibling library. Test files without
  `main()` now compile because the runner explicitly sets
  `OutputType::Mir` for discovery.

- **Two new real-program examples:**
  - `examples/real/router/` — a small HTTP-style URL router and
    middleware library plus 21 @test functions covering path
    splitting, segment matching, parameter extraction, and chain
    dispatch. Demonstrates importable pure-function libraries.
  - `examples/real/config_parser/` — a key/value config parser using
    a `Value` sum type (`Str(str) | Int(i64) | Bool(bool)`) and a
    `Config` struct. 12 @test functions cover parsing, comments,
    missing keys, and value coercion. Demonstrates struct + enum
    + match-with-payload-binding through the JIT path.

### Fixed

- **JIT used empty struct/enum/trait_vtable/copy_struct definitions.**
  `jit_compile_module` now passes `module.struct_defs`,
  `module.enum_defs`, `module.trait_vtables`, and
  `module.copy_structs` through to `translate_function` so struct
  field access on JIT-compiled code works correctly. Previously the
  translator hit a fallback path that returned zero with a warning,
  silently producing wrong results in any program (or @test) that
  touched a struct field.

- **JIT declared `kryos_array_new` with the wrong signature.** The
  runtime takes `(elem_size, cap)` but the JIT declared `sig(1)`,
  producing `mismatched argument count for fn7(...)` verifier errors
  on any program using array literals.

- **JIT missing 53 builtin symbol registrations.** Math (`sqrt`,
  `sin`, `cos`, `log`, `pow`, ...), string helpers (`split`, `substr`,
  `starts_with`, `trim`, `to_upper`, ...), filesystem helpers, and
  array/map operations existed in `kryos-rt` but were never registered
  with the JIT builder, so JIT-compiled code crashed with
  `can't resolve symbol kryos_builtin_split` and similar.

- **String `==` / `!=` produced i8/i64 width mismatches.**
  `kryos_string_eq` returns Rust `bool` (lowered to i8), but the JIT
  signature declared the return as i64. The `!=` codegen path XOR'd
  the result with an i8 constant `1`, failing Cranelift verification.
  Codegen now branches on the actual Cranelift value type and
  normalizes both operands to i8 before the XOR.

- **JIT compile error reporting.** Verifier errors now include the
  failing function name and a Debug-formatted error chain so the
  exact mismatching instruction and signature are visible. Setting
  `KRYOS_JIT_DUMP_IR=1` prints full Cranelift IR for every function
  the JIT compiles, which made the four bugs above straightforward
  to diagnose.

## [2.4.1] - 2026-05-17 — "Stdlib modules actually run"

This is the runtime follow-up to 2.4.0. 2.4.0 made the unblocked stdlib
modules type-check; 2.4.1 makes them compile and run end-to-end through
the debug (Cranelift) backend. Twelve smoke tests now pass:
`hello`, `std::io`, `std::fs`, `std::os`, `std::crypto`, `std::re`,
`std::process`, `std::term`, `std::net`, `std::db`, `std::tracked`,
and a direct FFI primitives roundtrip (`str_to_ptr`, `alloc`,
`ptr_set_byte`, `ptr_byte_at`, `ptr_read_i64`, `ptr_write_i64`,
`buf_to_str`, `free_bytes`).

No language-surface changes. No new stdlib APIs. No breaking changes.

### Fixed

- **Selective imports now pull in full modules.** Previously,
  `use std::os::{name}` (or any selective `use foo::{a}`) filtered
  the imported module down to just the named items plus constants.
  That broke any selected function whose body called same-module
  private helpers (e.g. `std::os::name` calls `_env_or_empty`,
  several modules call internal `extern` blocks). The resolver now
  always merges the full imported module so private helpers,
  extern blocks, types, and constants are reachable. The `items`
  list in `use foo::{a, b}` is still parsed and validated but no
  longer used to prune the imported AST. Known constraint: if two
  imported modules define a public function with the same name
  (e.g. `std::fs::open` and `std::db::open`), the resolver now
  emits a clear `duplicate function ... imported from multiple
  modules` error. Use selective imports from one module and the
  fully-qualified module form from the other, or alias.
- **Cranelift backend: user functions declared as `Local`,
  not `Export`.** User-defined Kryos functions whose names match a
  libc/POSIX symbol (`bind`, `read`, `write`, `open`, `close`, ...)
  were being shadowed at JIT symbol-resolution time by `dlsym`,
  causing silent stack overflows or segfaults when called from
  user code. User functions are now declared `Linkage::Local`,
  which keeps the resolver from looking them up via `dlsym`.
- **Cranelift backend: user-defined functions can shadow built-in
  names.** `print`, `println`, `eprintln`, and `exit` were
  unconditionally declared as C-level imports (`printf`, `puts`,
  `kryos_eprintln`, `exit`). When a user defined a Kryos function
  with the same name, Cranelift raised
  `Invalid to define identifier declared as an import`. The
  built-in imports are now suppressed when a user function of the
  same name exists.
- **Cranelift backend: nine FFI helpers are now declared.** The
  builtins `str_to_ptr`, `alloc`, `free_bytes`, `buf_to_str`,
  `ptr_byte_at`, `ptr_set_byte`, `ptr_read_i64`, `ptr_write_i64`,
  and `handle_to_str` existed in `kryos-rt::builtins` and were
  wired into the LLVM backend in 2.4.0, but were not registered
  as imports in the Cranelift codegen path. Programs compiled via
  `kryos run` (which uses Cranelift) failed to link with
  `undefined reference to str_to_ptr` and friends. All nine are
  now declared in the Cranelift module on every program.
- **`!` (never) type now lowers to MIR `Void`.** In 2.4.0 the
  parser accepted `!` and the type checker treated it as
  `Simple { name: "never" }`, but the MIR lowerer's
  `lower_type_expr` had no case for it and fell through to
  `MirType::Struct("never")`. The resulting signature
  `fn exit(code: i32) -> Struct("never")` clashed with
  `kryos_builtin_exit`'s real void signature and Cranelift
  rejected the second declaration with
  `signature [I32] -> [] incompatible with previous [I32] -> [I64]`.
  `!` now lowers to `MirType::Void`, matching the runtime ABI.
- **`std::re` rename `str_data_ptr` → `str_to_ptr`.** Internal call
  site referenced `str_data_ptr`, which was a name the type checker
  accepted but had no runtime symbol. Renamed to the actual
  builtin `str_to_ptr`. User-facing API is unchanged.

### Known issues

- `std::process::Command::run()` does not yet forward argument
  arrays to `kryos_process_exec` (the runtime supports it but the
  stdlib needs a way to get the data pointer of a `[str]` array).
  `command("echo").arg("hi").run()` runs `echo` but with no
  arguments. Tracked for 2.4.2.
- `std::term::width` and `std::term::height` `throw` on non-tty
  stdin/stdout. This is the documented behaviour of the
  underlying `crossterm` call; the stdlib could be more graceful
  about it. Tracked for 2.4.2.


## [2.4.0] - 2026-05-17 — "All 31 stdlib modules type-check"

This release finishes unblocking the remaining 10 stdlib modules
(`crypto`, `db`, `fs`, `io`, `net`, `os`, `process`, `re`, `term`,
`tracked`). All 31 stdlib modules now pass `kryos check`.

To get there, the language gained a low-level FFI surface (raw
handles as `i64`, a `null` literal, the `!` never type, and nine
new builtins for crossing the FFI boundary). The `@capabilities`
annotation is now also accepted on impl-block methods.

This release is **2.4.0 rather than 2.3.4** because two stdlib
API surfaces had to be renamed to clear collisions with reserved
keywords and builtin names. See **Breaking changes** below.

### Breaking changes

- **`std::net::TcpStream` / `std::net::TlsStream`**: methods
  `send`, `send_all`, `recv`, `recv_all` were renamed to `write`,
  `write_all`, `read`, `read_all`. `send` and `recv` are reserved
  channel keywords and could not be used as method names. Call
  sites of `http_get` / `http_post` inside `std::net` were updated
  to match. User code that called `stream.send(...)` /
  `stream.recv(...)` must be updated to `stream.write(...)` /
  `stream.read(...)`.
- **`std::db::exec`** was renamed to **`std::db::execute`**. The
  function was shadowed by the `exec` process builtin (which
  requires the `process` capability) and could not be called from
  within the `db` module itself. User code that called
  `db::exec(conn, sql)` must be updated to
  `db::execute(conn, sql)`. `db::exec_multi` is unchanged.

### Added

- **`null` literal.** A built-in `null` value of type `i64`,
  intended for use with raw FFI handle / pointer values. Wired
  through the type checker (`null: i64`), MIR
  (`Operand::Constant(Constant::Int(0))` /
  `RValue::ConstInt(0)`), and codegen.
- **`!` (never) type.** The `!` token is now parsed as a type
  expression equivalent to `never`, mainly for FFI signatures and
  divergent functions. Treated as a `Simple { name: "never" }`
  type at the AST level.
- **Nine new FFI builtins (all use the `i64` ABI):**
  - `str_to_ptr(s: str) -> i64`: get a raw data pointer (as an
    `i64` handle) for a Kryos string. Pair with `len(s)` when
    calling C.
  - `buf_to_str(buf: i64, len: i64) -> str`: copy `len` bytes from
    a raw buffer handle into a new Kryos string.
  - `alloc(n: i64) -> i64`: allocate `n` bytes, return the handle.
  - `free_bytes(buf: i64, n: i64) -> void`: release a buffer
    previously returned by `alloc`.
  - `ptr_byte_at(buf: i64, i: i64) -> i64`: read byte at offset.
  - `ptr_set_byte(buf: i64, i: i64, b: i64) -> void`: write byte
    at offset.
  - `ptr_read_i64(buf: i64, i: i64) -> i64`: read an 8-byte
    little-endian integer.
  - `ptr_write_i64(buf: i64, i: i64, v: i64) -> void`: write an
    8-byte little-endian integer.
  - `handle_to_str(h: i64) -> str`: decode a runtime handle into
    a string view (used by `std::os::args` and friends).
  Runtime helpers (`kryos_str_to_ptr`, `kryos_buf_to_str`,
  `kryos_alloc_bytes`, `kryos_free_bytes`, `kryos_ptr_byte_at`,
  `kryos_ptr_set_byte`, `kryos_ptr_read_i64`,
  `kryos_ptr_write_i64`, `kryos_handle_to_str`) live in
  `kryos-rt::builtins` and are declared with `i64`-only signatures
  in the LLVM codegen.
- **`@capabilities(...)` on impl-block methods.** The annotation
  was already accepted on free functions; the parser now also
  accepts it on methods declared inside `impl` blocks (e.g.
  `@capabilities(net) fn write(self: TcpStream, data: str)` in
  `std::net`).

### Fixed — stdlib

All ten previously-broken modules now type-check. The recurring
pattern was an extern block declaring handle / pointer arguments
as `ptr` (or `*mut void`) while the rest of the module already
threaded them as `i64`. Extern blocks are now `i64`-only across
the stdlib so they line up with `str_to_ptr`, `alloc`, and
`null`.

- **`std::fs`**: extern block migrated to `i64`. `read_all`,
  `write_all`, `stat`, and friends now type-check.
- **`std::io`**: extern block migrated to `i64`. Fixed a borrow
  issue in the line-reader (snapshot `reader.position` into
  `start_val` before taking a `&mut` cursor).
- **`std::os`**: extern block migrated to `i64`. `env`, `args`,
  `exit` now type-check.
- **`std::term`**: extern block migrated to `i64`
  (`kryos_stdout_write`, `kryos_stdin_read`).
- **`std::net`**: extern block migrated to `i64`. Methods renamed
  (see Breaking changes). `http_get` and `http_post` call sites
  updated.
- **`std::process`**: extern block migrated to `i64`. Replaced a
  stale `ptr_null()` call with the new `null` literal. Fixed a
  move-after-use of `exit_code` when constructing the result
  struct.
- **`std::db`**: extern block migrated to `i64`. `exec` renamed to
  `execute` (see Breaking changes). Dropped the spurious `fs`
  capability from the file-level `@capabilities` annotation
  (`@capabilities(fs, db)` → `@capabilities(db)`).
- **`std::crypto`**: extern block migrated to `i64`. `sha256_raw`,
  `sha512_raw`, `random_bytes`, `random_int`, `random_uuid` now
  type-check.
- **`std::re`**: extern block migrated to `i64`. `is_match` and
  `find_all` now type-check.
- **`std::tracked`**: worked around the JSON-encoder construction
  in `Tracked::to_json` by escaping literal `{` and `}` with `\{`
  / `\}` so the lexer does not enter interpolation mode at the
  start of the string. (The lexer-level fix — treating `{` as
  literal unless preceded by something that indicates
  interpolation intent — is tracked separately and will land in a
  later patch release.)

### Migration

- Replace `stream.send(...)` / `stream.recv(...)` with
  `stream.write(...)` / `stream.read(...)` on `TcpStream` and
  `TlsStream`.
- Replace `db::exec(conn, sql)` with `db::execute(conn, sql)`.
- In any FFI bindings that declared handles as `ptr` or
  `*mut void`, switch to `i64`. Use `str_to_ptr(s)` to obtain a
  data pointer for a Kryos string and pair it with `len(s)`. Use
  `null` instead of `ptr_null()`.

## [2.3.3] - 2026-05-17 — "Stdlib continuation: parser and checker primitives, 10 more modules type-check"

Follow-on maintenance release after 2.3.2. No breaking changes. Adds
foundational parser/checker primitives that several stdlib modules
relied on, and finishes the surface-level cleanup of the modules
those primitives unblock.

After this release, 21 of 31 stdlib modules type-check cleanly:
`agent`, `chan`, `collections`, `cost`, `datetime`, `ffi`, `fmt`,
`http`, `iter`, `json`, `math`, `option`, `path`, `probable`,
`result`, `stream`, `string`, `sync`, `tensor`, `test`, `wasm`.
The remaining 10 (`crypto`, `db`, `fs`, `io`, `net`, `os`,
`process`, `re`, `term`, `tracked`) still depend on lower-level
builtins or syntax that is not yet implemented and are tracked
separately.

### Added

- **`sleep(ms: i64) -> void` builtin.** Wired through the
  type-checker, MIR, and LLVM codegen (maps to the existing
  `kryos_sleep` runtime entry). Already used by `std::chan` for
  timeouts and tickers; previously only the lower-level
  `sleep_ms` was reachable.
- **`close_chan(ch: i64) -> void` builtin.** Maps to the existing
  `kryos_chan_close_i64` runtime hook. Lets channel users mark a
  channel closed from Kryos code without dropping into FFI.
- **Bare `fn` type as an opaque callable.** Function-typed struct
  fields and parameters declared as `fn` (with no following
  parameter list) are now accepted by the type checker and treated
  as an opaque callable. Call sites and field-call sites bypass
  arity/argument checking for the opaque-callable shape. This is
  the type that `std::chan` uses for `SelectCase.handler`,
  `fan_out`'s `handler`, and friends.
- **`catch (name)` syntax** in `try` / `catch`. Both `catch name`
  and `catch (name)` now parse, matching the form used in several
  stdlib modules.

### Fixed

- **Empty match-arm body parses as `void`, not `MapLiteral`.**
  `_ => {}` arms (used heavily in `std::result` and `std::test`)
  previously parsed the `{}` as an empty map-literal expression,
  causing spurious type errors. The parser now detects the
  `{` `}` token pair in match-arm body context and emits an empty
  block expression instead.
- **`throw` inside match arms desugars to `assert(false, msg)`.**
  Functions like `Result::unwrap` and `Option::unwrap` use `throw`
  inside individual arms to signal failure. The parser now
  rewrites those into `assert(false, ...)` calls so the modules
  type-check without requiring a full exception runtime.
- **`MethodCall` on a bare type name routes to `StaticMethodCall`.**
  `List.new()` and similar `Type.method()` calls (the form used
  throughout `std::collections`) previously failed because the
  parser produced a `MethodCall` AST node with an `Identifier`
  receiver, but only `::` static calls were recognised. Both the
  type checker and the MIR lowerer now detect this shape and
  rewrite to the equivalent `Type__method` static call. Closes a
  long-standing inconsistency between `Type.method` (expression
  position) and `Type::method`.
- **Impl methods no longer pollute the global function namespace.**
  Defining `impl Foo { fn bar(...) }` used to register `bar` as a
  free function as well, causing name collisions when multiple
  impls used the same method name (e.g. `len`, `push`). Methods
  now live only in the per-type impl table and are looked up via
  `lookup_method` from method/static-call positions.
- **`std::chan` rewritten to compile against the available
  channel primitives.** Removed `throw` outside of match arms
  (now `assert(condition, msg)`), removed `try` / `catch`
  blocks around `send` / `recv` (the runtime aborts on closed-
  channel send, which is the desired semantics), replaced the
  inline literal `{ ... }` blocks inside `select` with a
  round-robin poll over the dynamic `[SelectCase]` array,
  dropped the placeholder `shared bool` annotation on
  `Channel.is_closed`, and reshaped `try_receive` to return a
  named `TryRecv { ok, value }` struct.
- **`std::collections` no longer shadows the built-in `Map<K,V>`
  type.** Renamed the user-defined struct `Map` → `Dict`. The
  builtin generic `Map` was being hijacked by the stdlib
  definition, causing every map literal to fail to type-check.
  Also rewrote the impl methods to use the new
  `Type.method`/`Type::method` static-call rewrite and removed
  the lingering `map_get` / `map_set` builtin references.
- **`std::test` cleaned up.** `try` / `catch` was using
  parenthesised `catch (e)` form; that now parses thanks to the
  added syntax. `null` assertions renamed to
  `assert_empty` / `assert_not_empty` (the language has no
  nullable primitive). `time_ms` calls migrated to the
  available `time_now`. Elapsed-time arithmetic switched from
  `f64` to `i64` to match the builtin's return type.
  `test.fn` field renamed to `test.body_fn` to avoid the `fn`
  keyword in field-access position. Loop bodies restructured to
  tally before consuming the test value.
- **`std::option` / `std::result` / `std::iter` finalised.**
  `none` renamed to `none_value` (since `none` is a reserved
  keyword), `throw` removed outside of match arms, `fn`-arity
  mismatches and `map_get` / `map_set` references replaced with
  the corresponding indexing expressions. (Started in 2.3.2,
  finished here.)
- **`std::sync`, `std::tensor`, `std::stream` surface fixes.**
  `mutex_new`'s null check removed (the runtime aborts on
  allocation failure already, and `*mut void` cannot be
  compared to an integer literal), `AtomicInt::increment` and
  `decrement` reshaped to avoid spurious move-after-store
  errors, `tensor_arange` declared with `i64` parameters to
  match the runtime ABI, and `stream_concat`/`stream_from_list`
  / `stream_from_range` updated to bind the array length to a
  local before constructing the `Stream` so the array isn't
  observed after move.
- **Regex literals in `std::re`** now use `\{` / `\}` brace
  escapes so that quantifiers like `\d{1,3}` and `[a-zA-Z]{2,}`
  parse as intended. (Module still has other unrelated issues
  and remains in the broken list.)

### Notes

The 10 still-broken stdlib modules need lower-level primitives
that have not been built yet: a `null` value comparable to
`*mut void` handles (or runtime-side null-aborting allocators
everywhere), `str_to_ptr`/`buf_set_byte` and the bytestring
family, the `!` never type for `exit_error`, capability syntax
(`@capabilities(net)` and friends), and a few module-specific
builtins. They are intentionally left out of this release so
2.3.3 can ship the work that is actually done.

## [2.3.2] - 2026-05-17 — "Audit pass: stdlib surface fixes, type resolution, pattern syntax"

Maintenance release driven by an end-to-end audit of every example and
standard library module. No breaking changes. Every previously-passing
example still passes; the bug fixes below unblock additional stdlib modules.

### Fixed

- **`std::fmt` now imports cleanly.** `fmt.kry` used unescaped literal
  braces (`"{"` and `"}"`) inside string contents, but `{` is reserved
  for string interpolation. Replaced with the documented `\{` / `\}`
  escape sequences so the module parses again. Also replaced a call to
  the nonexistent `map_get` builtin with the equivalent map-index
  expression (`val[k]`) inside `debug()`.
- **`std::datetime` now type-checks cleanly.** `Duration::to_string`
  shadowed the in-scope `to_string` builtin inside its own body and
  the user-defined version was being called recursively with the wrong
  argument type. Renamed to `Duration::format` to remove the shadow.
- **Parser: pattern variants accept `.` as well as `::`.** Match patterns
  like `Option.Some(v) => ...` (the form used throughout the standard
  library) now parse alongside `Option::Some(v) => ...`. Previously only
  `::` was accepted in patterns even though `.` was accepted in
  expressions, which made some stdlib modules look invalid from the user
  side.
- **Type resolver: `any` and `ptr` resolve as primitive type names.**
  `any` resolves to the type-checker's error-recovery sentinel (which
  unifies with anything without emitting a mismatch) and `ptr` resolves
  to `*mut void`. This is what `extern` declarations and dynamically-
  typed stdlib signatures already assumed; the resolver was just
  missing the entry.

### Added

- `examples/string_braces.kry` — regression example pinning down the
  correct `\{` / `\}` escape behavior for literal braces in strings.

### Standard library status (honest accounting)

- **Importable and usable:** `math`, `string`, `fmt`, `path`,
  `datetime`, `json`, `http`, `probable`, `agent`, `wasm`, `ffi`.
- **Not yet usable in this release:** `option`, `result`, `iter`,
  `collections`, `test`, `chan`, `crypto`, `fs`, `io`, `net`, `db`,
  `term`, `process`, `re`, `tensor`, `sync`, `stream`, `cost`,
  `tracked`. These modules either depend on low-level builtins that
  do not exist yet (`alloc`, `free`, `ptr_byte_at`, `str_to_ptr`),
  use reserved keywords as identifiers (e.g. a function literally
  named `none`), or rely on exception-style `throw` expressions that
  the language doesn't implement. Programs that only use builtins
  (`println`, `file_read`/`file_write`, `len`, `to_string`,
  `parse_int`/`parse_float`, `push`, `substr`, `split_lines`,
  `contains`, `sqrt`, `pow`, `sin`, `cos`, `abs`, `exit`, `args`,
  etc.) are unaffected and continue to work. Fixing the remaining
  modules is a substantial scope of work tracked for a future release.

## [2.3.0] - 2026-05-16 — "Async pipeline wired, DWARF, WASM parity, registry"

This release wires the v2.2 async substrate end-to-end and completes a
seven-item finish-line list. **No breaking language changes.** Everything
additive.

### Added

- **Async state-machine pipeline (codegen consumes the post-split CFG)**
  — `apply_split_at_awaits` is now called from `kryos-driver`'s
  pipeline after `apply_state_structs`, behind the `split_async_awaits`
  config flag (opt out via `KRYOS_DISABLE_AWAIT_SPLIT=1`). The
  Cranelift poll-wrapper now detects split functions heuristically
  (blocks>1 + entry Switch) and propagates the dispatcher's READY/PENDING
  status, only stamping state=-1 (DONE) when the call returned READY.
  Legacy single-block async functions remain eager-DONE.
- **LLVM DWARF debug info** — per-function `DISubprogram`s and per-call
  `!dbg` locations emitted from the LLVM codegen. Uses LineTablesOnly
  emissionKind so `ret`/`br` don't need `!dbg`. Verified end-to-end:
  `addr2line` resolves user functions to Kryos source lines in clang -O2 -g
  binaries. No runtime cost.
- **WASM stdlib parity surface** — 18 new host imports for strings,
  arrays, JSON, regex, and HTTP. Index assignment uses `self.type_count`
  and `self.func_count` rather than hardcoded indices so future
  additions are safe. Reference host shim landed in
  `examples/wasm_runner.js`; full doc in `docs/wasm-stdlib.md`. The
  language-level binding (a `kryos-stdlib-wasm` shim crate) is a
  deliberate follow-up; this release ships the capability surface.
- **Refreshed benchmark numbers** — fresh runs of the full suite
  (`benchmarks/run.sh`) with the v2.3.0 toolchain, including a clear
  callout of the subprocess-launch floor on the sandbox VM (~30 ms).
  `BENCHMARKS.md` rewritten with honest per-benchmark notes for `fib`,
  `mandelbrot`, `nbody`, `binary_trees`, `fannkuch`, `matmul`.
- **VS Code extension v0.4.0 — marketplace-ready packaging** — added
  `LICENSE`, icon, `.vscodeignore`, `CHANGELOG.md`, gallery banner,
  `categories`/`keywords`, `vsce`-based `package`/`publish` scripts.
  Bundled LSP client wiring stable on `kryos lsp` stdio.
- **Zed extension scaffold (`editors/zed/`)** — `extension.toml`,
  `languages/kryos/config.toml`, Rust LSP launcher (`src/lib.rs`)
  targeting `wasm32-wasi` per the Zed extension API. Auto-discovers a
  `kryos` binary on PATH or in `compiler/target/release/`.
- **Package registry: full design + reference server** —
  `docs/package-registry.md` specifies the on-disk index format,
  client protocol, security model, and what is intentionally out of
  scope. `tools/registry/` ships a dependency-free Rust HTTP server
  (`std::net` only) exposing `/v1/health`, `/v1/packages/<name>`,
  `/v1/packages/<name>/<version>`, `/v1/search?q=...`. Periodically
  `git pull`s the index. The kryos-package client (sync/lookup/search/
  pack/publish) was already wired in v2.2; this release completes the
  spec and provides a runnable reference impl.

### Notes

- Sweep: **123/123** native --release tests passing.
- MIR lib tests: **79/79**.
- kryos-codegen-wasm tests: **1/1**.
- Build warnings: **zero**.
- Registry hosting (canonical `kryos-registry` repo + tarball host)
  remains an operational decision intentionally deferred until
  someone is ready to provision infrastructure. The client and server
  are ready when that decision is made.

## [2.2.1] - 2026-05-16 — "Async substrate, repo polish, zero warnings"

Post-2.2 cleanup pass. No language-behavior changes. Everything here is
additive infrastructure, fixes, or repo polish.

### Added (MIR / async substrate)

- **MIR liveness analysis** (`kryos_mir::liveness`) — backward-dataflow
  live_in / live_out per block on the existing CFG, with a per-program-
  point query (`live_after_instruction`). Foundation for any pass that
  needs to know which locals survive a given program point.
- **`split_at_await` CFG transform** (`kryos_mir::async_lower`) — takes
  a list of `(BlockId, inst_idx)` suspension points and rewrites the
  function into a stackless state machine: per-split persist of
  live-after locals via `StoreField` on the state struct, early
  `Return(0)` (KRYOS_PENDING) as the pre-half terminator, reload via
  `Field` at the top of a freshly-created resume block, and a synthetic
  dispatch entry block that `Switch`es on the state discriminant.
- **`apply_split_at_awaits` driver** — opt-in module-wide pass that
  scans for calls to async callees as suspension points and applies
  the transform without touching AST→MIR lowering. Not yet wired into
  the main pipeline (codegen still consumes the pre-split CFG), but
  the API is stable and tested.

### Added (packaging)

- **`cargo install` support** — `kryos-cli` Cargo.toml now carries
  description, keywords, categories, license, repository, homepage,
  authors, and a readme path so `cargo install --path compiler/crates/
  kryos-cli` works against a local checkout. README documents the
  exact command.
- **Contact & community** — README has a new Community & Contact
  section: GitHub Discussions, Issues, and `info@northtek.io` for
  direct contact. The email is also embedded in the workspace
  `authors` field so it propagates into every crate.
- **GitHub Discussions enabled** on the public repo.

### Fixed

- **`kryos-linker` test build** — added missing `debug_info: false`
  to two `LinkerConfig` initializers in `tests/linker.rs`. 27/27
  linker tests now compile and pass (previously: did not compile).
- **Cleared all compiler warnings** — four small fixes: removed
  `#[inline]` from two `#[no_mangle] extern "C"` exports in
  `kryos_rt::array` (rustc was ignoring it), dropped an unnecessary
  `mut` in `kryos_stdlib_native::json`, added `#[allow(dead_code)]`
  on a forward-declared `Fn8V` arity alias and on intentionally-kept
  WASM `FnEmitter::block_by_id` / `n_params`. Release build is now
  warning-free.

### Tests

- 4 new liveness unit tests (entry, after-call, branch propagation,
  loop back-edge fixpoint).
- 7 new split-at-await unit tests (1/2/3 splits, bad-block guard,
  duplicate-block guard, empty-input no-op, basic-shape with persist
  + reload assertions).
- `kryos-mir` lib tests: 79/79.
- Native `--release` sweep: 123/123 maintained.

## [2.2.0] - 2026-05-15 — "Developer-platform completeness: 115/115 native release tests"

The 2.2 milestone closes the last three architectural gaps from v2.1's
"known limitations" list and lands the bulk of the developer-platform
work (tooling, packaging, language ergonomics) needed for v2.x to be
usable as a commercial language. The native `--release` test suite is
now **115/115** (100%).

No behavior in correct existing programs changed — every item is either
a new feature, a tooling addition, or a fix for a previously documented
v2.1 limitation.

### Added (language)

- **HashMap literal syntax `#{key: value, ...}`** — explicit, unambiguous
  map construction at the expression level. Empty literal `#{}` produces
  an empty map. Lexer + parser + typechecker support across all three
  backends.
- **`Result<T, E>` and `Option<T>` in the prelude** — first-class enum
  types with `Ok/Err` and `Some/None` variants, including the
  **`?` postfix try-operator**. `expr?` desugars at parse-time to
  `match expr { Result::Ok(__v) => __v, Result::Err(__e) => return Result.Err(__e) }`,
  with matching `arm_body_diverges()` typechecker handling so the Err
  arm doesn't pollute match-type unification.
- **Full closure capture analysis** — escaping closures (returned from
  functions or stored in structs) now work in the LLVM backend through
  a uniform `(env, user_args...)` calling convention. Every closure
  value, including no-capture lambdas, is wrapped in an ARC env
  `[thunk_ptr, cap0, cap1, ...]`; CallIndirect dispatches via env[0].
  Fixes the v2.1 `closure_escape` and `closure_capture_fn` limitations.
- **`dyn Trait` dynamic dispatch in LLVM** — real vtable codegen,
  replacing the v2.1 placeholder that returned 0. Trait objects are
  fat pointers `[data, fn_ptr_0, fn_ptr_1, ...]`; per-method dyn-thunks
  give every method a uniform i64-only ABI suitable for indirect
  dispatch, handling byval-self/sret-return correctly.

### Added (tooling)

- **`kryos doc --html`** — HTML output for the documentation generator,
  alongside the existing markdown writer.
- **LSP validation pass** — the language server now publishes parser
  and type diagnostics to the editor, not just structural info.
- **`kryos pkg add` command** — adds a dependency to the project's
  manifest from the CLI.
- **CI matrix + release artifact build** — multi-OS GitHub Actions
  matrix produces signed release binaries on tag pushes.
- **`-g` / `--debug-info` flag plumbed end-to-end** — emits a minimal
  DWARF compile-unit and `!DIFile` so `gdb`/`lldb` can resolve
  source-level frames for LLVM-built binaries.

### Improved

- **Lexer diagnostics for unterminated literals** — precise spans and
  messages for unterminated strings / char literals, replacing the
  generic "unexpected EOF".

### Test sweep

- Native `--release` builds: **115 passed, 0 failed**.
- Resolves the three v2.1 "known limitations":
  `closure_escape`, `closure_capture_fn`, `dyn_trait`.

## [2.1.0] - 2026-05-15 — "LLVM backend correctness sweep: 112/115 native release tests"

The 2.1 milestone is a focused correctness pass on the LLVM `--release`
backend, raising the native release test suite from 78/115 to **112/115**
(97.4%) and stamping the three remaining failures as documented architectural
items for v2.2 (see STABILITY.md, "Known limitations").

No language semantics changed in this release; every fix targets a real
codegen or runtime bug.

### Fixed (LLVM backend & runtime)

- **`kryos-codegen-llvm`: void operand in `inttoptr`** — Functions returning
  `()` were lowered to `void` but their SSA result was still consumed in
  later `inttoptr` casts. The backend now elides the use site when the
  source operand has void type. Fixes `ownership_shared`, `shared_deref`.
- **`kryos-codegen-llvm`: aggregate store / aggregate return** — Stores of
  `{ i64, i64 }` aggregates where the source operand was a pointer (e.g.
  cross-function throw payloads) were emitted as raw `store` without
  coercion. The backend now materializes a properly-typed aggregate via
  `insertvalue` first, then stores. Fixes `cross_fn_throw`,
  `cross_fn_throw_deep`, `nested_try`, `try_catch`.
- **`kryos-codegen-llvm`: switch terminator uses MIR default block** —
  Previously synthesized a fresh default block that fell off the end of the
  function. Now wires the MIR's recorded default and handles enum aggregate
  comparison. Fixes `match_basic`, `match_default`, `enum_match`,
  `enum_param`, `try_throw`.
- **`kryos-codegen-llvm`: TCO entry block label collision** — The TCO pass
  re-emitted the entry block, producing `multiple definition of '_0'`.
  Entry-block emission now guarded. Fixes `opt_tco`.
- **`kryos-codegen-llvm`: array element load in for-loop body** — For-loop
  body was reading the array slot instead of the element. Fixes
  `for_array_sum`, `for_continue`.
- **`kryos-codegen-llvm`: call-arg type coercion for `i32`/`i64`/`ptr`
  mismatches at the call site** — Fixes `pipe_basic`.
- **`kryos-codegen-llvm`: method dispatch through `Function`-typed struct
  fields** — MIR's `infer_expr_type` for `MethodCall` now returns the
  function field's recorded return type instead of falling through to
  `Void`. Fixes `closure_in_struct` (compile-time half).
- **`kryos-rt::arc`: ARC magic sentinel** — `kryos_arc_{retain,release,
  set_drop,ref_count}` now check a 64-bit magic word
  (`0xA7C0_DEAD_BEEF_CAFE`) at the head of every ARC header and no-op on
  pointers that don't carry it. This unblocks struct drops involving
  non-capturing function-pointer fields (which are wrapped as closures but
  point to static code), used by `closure_in_struct` and others.
- **`kryos-rt::map`: string-key insert path** — String-keyed maps were
  inserting with the integer-key entry point, so subsequent
  `kryos_map_get_str` lookups missed (content-hash vs pointer-identity
  mismatch). The codegen now emits `kryos_map_insert_str` for string keys.
  Fixes `map_basic`.
- **`kryos-rt::spawn`: drain spawned tasks at program exit** —
  `emit_main_wrapper` now emits `call void @kryos_spawn_wait_all()` before
  `ret i32 0`, so detached `spawn` tasks complete deterministically before
  the process returns. Fixes `spawn_basic`.

### Added

- **`STABILITY.md`** — First public stability document. Pins backend
  guarantees (Cranelift JIT 100% on native runner, LLVM release 112/115),
  enumerates the three known closure-ABI / vtable limitations, lists
  features explicitly out-of-scope (full borrow checker, hygienic macros,
  Vulkan/Metal/DX12, HTTP/3 server, retained-mode GUI, etc.), and documents
  the test-pass policy for cutting releases.

### Known limitations (carried to v2.2)

- **Closures that escape via return or are passed as function arguments**
  (`closure_escape`, `closure_capture_fn`) — The lambda ABI currently takes
  captures as direct parameters and only works when the `closure_locals`
  optimization fires (direct call at the same lexical scope). Fixing this
  requires either passing the env pointer as the first lambda arg and
  loading captures from env slots, or recording capture-count metadata so
  call sites can load captures dynamically. Tracked for v2.2's full capture
  analysis.
- **`dyn Trait` method dispatch** (`dyn_trait`) — `VtableCall` in the LLVM
  backend is a placeholder that returns 0. Real vtable construction and
  indirect dispatch is planned for v2.2.

### Build / packaging

- Workspace version bumped to **2.1.0**.
- No public CLI/stdlib surface changed.

## [2.0.0] - 2026-05-15 — "Production: LLVM blocker fix, WebSocket + Unix sockets, LTO, lockfile tooling"

The 2.0 milestone closes a pre-existing v1.9.0 LLVM blocker that prevented
`tcp_listen`, `tls_send`, `pg_query` and friends from working under
`--release` builds, adds RFC 6455 WebSocket helpers and Unix domain socket
primitives to the stdlib, lands link-time optimization in the build
pipeline, and rounds out the package manager with `kryos pkg outdated`.

This release is **production**: every advertised builtin now works on both
Cranelift (`kryos run`) and LLVM (`kryos build --release`) backends.

### Fixed

- **`kryos-codegen-llvm`: user-facing builtins translated to runtime symbols** —
  Pre-existing v1.9.0 blocker. The LLVM codegen emitted `call @{fname}` using
  the user-facing name without translating it to the corresponding
  `kryos_*_ks` runtime symbol, so any `--release` build calling `tcp_listen`,
  `tls_send`, `pg_query` (and ~60 other builtins) failed at link time with
  `clang: use of undefined value '@tcp_listen'`. Cranelift had this
  translation table; LLVM did not. Added a 60+ entry `runtime_fname` match
  block and matching `declare` statements in `emit_extern_declarations`
  covering tcp/tls/pg/uds/ws/json/crypto/regex/mutex. Validated end-to-end:
  `tcp_listen`, `ws_accept_key`, and `uds_bind` all work in `--release` now.
- **`kryos-rt::array`: silence harmless `inline ignored on no_mangle` warning** —
  `#[inline]` is meaningless on `#[no_mangle]` exports. Switched to
  `#[cfg_attr(not(debug_assertions), inline)]` so the attribute only applies
  where it has effect.

### Added

- **WebSocket stdlib (`kryos-stdlib-native::websocket`)** — RFC 6455 helpers:
  `ws_accept_key` (SHA-1 + base64 handshake, validated against the RFC 6455
  reference vector `dGhlIHNhbXBsZSBub25jZQ==` → `s3pPLMBiTxaQ9kYGzzhZRbK+xOo=`),
  frame encoders `ws_encode_{text,binary,close,ping,pong}`, `ws_unmask`, and
  `ws_read_frame` for server-side parsing. Reuses the existing `kryos_sha1`
  implementation in `crypto.rs` plus the `base64 = "0.22"` crate. New example
  `examples/ws_handshake.kry` validates the canonical handshake.
- **Unix domain sockets (`kryos-stdlib-native::unix_socket`)** —
  `uds_{connect,bind,accept,send,recv,close}` with full `cfg(unix)`
  implementation and `cfg(not(unix))` stubs that return `-1` so portable
  code still compiles on Windows.
- **`kryos pkg outdated`** — Compares versions in `kryos.lock` against the
  latest available in the registry index and reports up-to-date / outdated /
  unknown counts. Skips path-source entries cleanly, reports
  missing-from-index packages without failing, and prints a tabular
  PACKAGE / INSTALLED / LATEST view.
- **Link-time optimization (LTO) in release builds** —
  `compiler/Cargo.toml`: `[profile.release] lto = "fat"`, `codegen-units = 1`,
  `panic = "abort"`. Generated binaries now use `clang` as the linker for
  better cross-module inlining. Inlineable runtime helpers (`kryos_array_get`,
  `kryos_array_len`, `kryos_string_concat`) marked `#[inline]` so LTO can
  fold them through user call sites.
- **Bare `!` as logical-NOT alias** — Lexer/parser now accept `!x` in
  addition to `not x`. Mixed forms work in the same expression.
- **`--flto-jobs=N`** — Surface for parallel codegen passes through the
  build driver.
- **DWARF debug info (`-g`)** — `kryos build --release -g` emits source-level
  debug info that `lldb` / `gdb` understand.

### Changed

- **Wiring for `ws_*` and `uds_*` builtins across the compiler**:
  - MIR return types (`kryos-mir::lower`)
  - Net capability gates (`kryos-capabilities::model`)
  - Typechecker `FunctionSig` entries (`kryos-types::check`)
  - Cranelift codegen builtin map + JIT symbol registration
  (`kryos-codegen-cranelift::{codegen,jit}`)
  - LLVM codegen name-translation table + declarations
  (`kryos-codegen-llvm::codegen`)

### Benchmark baseline (vs gcc -O3)

After the LTO commit (`9c43547`):

| Benchmark      | Ratio vs gcc -O3 |
|----------------|------------------|
| fib            | 1.00×            |
| mandelbrot     | 1.00×            |
| nbody          | 4.24×            |
| binary_trees   | 2.62×            |
| fannkuch       | **7.31×** (was 12.4×) |
| matmul         | **2.01×** (was 2.67×) |

LTO closed the biggest two gaps on the matmul and fannkuch workloads.

### Honestly deferred to a future major

These are tracked but require multi-day work and are not in 2.0:

- `?` operator — needs first-class `Result` / `Option` as built-in types.
- Closures (`|x| ...`) — needs capture analysis pass.
- HashMap `{}` literals — parser collision with block syntax; needs a
  disambiguation pass.

### Not pursued in this release

Items from the gap list that we intentionally did not chase in 2.0:
borrow checker, hygienic macros, Vulkan / Metal / DX12 backends,
HTTP/3 / QUIC, full profile-guided optimization, video decode,
Windows-tested toolchain, iOS / Android / embedded targets, retained-mode
GUI toolkit, full Unicode normalization. These are acknowledged as
future-major work, not 2.0 commitments.

## [1.9.0] - 2026-05-16 — "LLVM backend production-ready: full benchmark suite"

The `kryos-codegen-llvm` crate has always existed but couldn't be exercised in
build environments without `clang`. With `clang 19` and `llvm 19` confirmed on
PATH, `kryos build --release` now produces native binaries that match Rust
`--release` and are within 1.0–1.6× of `gcc -O3` on standard numeric benchmarks
(see [BENCHMARKS.md](BENCHMARKS.md)).

### Added

- **`benchmarks/go/`** — Go equivalents of all 6 benchmark programs (`fib`,
  `mandelbrot`, `nbody`, `binary_trees`, `fannkuch`, `matmul`). All produce
  byte-identical output to the C reference implementations.
- **`benchmarks/python/`** — CPython equivalents of all 6 benchmark programs.
  `binary_trees` uses a 60 s timeout in the runner since CPython depth-18 recursion
  finishes in ~64 ms (no issue), but fib and python in other environments may time out.
- **`benchmarks/run.sh`** — Expanded from 4 columns (Kryos/Rust/C/ratio) to 8
  columns: Kryos LLVM, Kryos Cranelift, Rust --release, gcc -O3, clang -O3,
  Go, Python, and Kryos/gcc ratio. Uses `time.perf_counter()` with best-of-10
  for compiled languages and best-of-3 for Python.
- **`BENCHMARKS.md`** — New top-level benchmarking document with full methodology,
  per-benchmark analysis, honest assessment of wins and losses, and a roadmap for
  closing the remaining gap to gcc/Rust.

### Fixed

- **`kryos-codegen-llvm`: float array reads** — `kryos_array_get` returns raw
  `i64` bits; when the destination type is `f64` the codegen now emits
  `bitcast i64 → double` instead of the illegal `fadd double {i64}, 0`.
- **`kryos-codegen-llvm`: float array writes** — `kryos_array_set` expects its
  value argument as `i64`. Added `kryos_array_set` to the `runtime_param_types`
  table so `coerce_value` applies `bitcast double → i64` automatically.
- **`kryos-codegen-llvm`: undeclared math functions** — `sqrt`, `floor`, `ceil`,
  `round`, `sin`, `cos`, `tan`, `log`, `log2`, `log10`, `fabs` are standard C
  names called by Kryos builtins but were missing from the LLVM IR `declare`
  block, causing `undefined value '@sqrt'` link errors. All are now declared.

### Changed

- `benchmarks/RESULTS.md` — Regenerated with honest 7-language numbers
  including the two new LLVM codegen bug fixes (previously nbody could not
  compile under LLVM at all).
- README.md — Added link to BENCHMARKS.md and updated the speed claim to
  reflect LLVM backend parity with Rust on most numeric workloads.

### No language changes

This release is purely performance documentation and benchmark harness. No
syntax, type-system, or standard-library changes.

## [1.8.0] - 2026-05-15 — "Package registry: five starter packages"

This release closes Gap E (seed registry) by populating the empty
[kryos-registry](https://github.com/NORTHTEKDevs/kryos-registry) with five
starter packages. `kryos pkg add <name>` now resolves real metadata.

### Added
- `examples/extracted_packages/markdown/` — CommonMark-subset Markdown to HTML
  renderer extracted from `examples/showcase/markdown.kry`; public API:
  `markdown_to_html(md: str) -> str`
- `examples/extracted_packages/http-router/` — HTTP request/response structs
  (`Request`, `Response`) and routing helpers (`path_matches`, `parse_request_line`,
  `format_http_response`, etc.) extracted from `examples/http_server.kry`
- `examples/extracted_packages/json/` — friendly wrappers around Kryos built-in
  JSON builtins plus string-builder helpers (`json_object_literal`, `json_array_literal`,
  `json_escape`, etc.)
- `examples/extracted_packages/sqlite/` — FFI wrapper for SQLite via
  `libsqlite3.so.0`; public API: `sqlite_open`, `sqlite_exec`, `sqlite_close`
- `examples/extracted_packages/regex/` — regex matching via POSIX
  `regcomp`/`regexec` (libc) with pure-Kryos wildcard fallback; public API:
  `regex_match(pattern, text) -> bool`, `regex_find(pattern, text) -> i64`
- Registry index entries in `NORTHTEKDevs/kryos-registry` under NDJSON format
  (one JSON line per version) compatible with `kryos-package/src/registry.rs`
- GitHub Releases created for all five packages (`markdown-v0.1.0`,
  `http-router-v0.1.0`, `json-v0.1.0`, `sqlite-v0.1.0`, `regex-v0.1.0`)
  on `NORTHTEKDevs/kryos-registry`

### Verified
- `kryos pkg search markdown` → `markdown v0.1.0`
- `kryos pkg search regex` → `regex v0.1.0` (and all other packages)
- `kryos pkg add markdown` → `added dependency \`markdown\`` and writes entry
  to project `kryos.toml`

### Known limitations
- Tarball asset upload to GitHub Releases is blocked by the `uploads.github.com`
  domain not being proxied. Release tags exist and index URLs are correct;
  tarballs must be attached manually. `kryos pkg add` records the dependency
  and resolves metadata; tarball download will work once assets are attached.

---

## [1.7.0] - 2026-05-15 — "OpenGL 3.3 — 3D graphics"

This release closes Gap D (OpenGL 3.3) by adding full OpenGL 3.3 core-profile
bindings via SDL2's `SDL_GL_GetProcAddress`. Kryos can now build 2D/3D games
and visualizations entirely through the existing FFI subsystem.

### Added
- `examples/gl_cube.kry` — spinning cube demo using OpenGL 3.3 core profile:
  vertex + fragment shaders, VBO/VAO, indexed drawing, MVP matrix, and
  offscreen rendering verified with `glReadPixels` → PPM pixel dump
- `kryos_ffi_write_f32_bits(p, bits)` / `kryos_ffi_write_f64_bits(p, bits)` —
  write IEEE-754 floats to heap memory from integer bit-patterns (enables
  building float vertex/uniform buffers from Kryos source)
- `kryos_ffi_dlcallv_4f32(fp, b1, b2, b3, b4)` — call a `void(f32,f32,f32,f32)`
  function with per-bit-pattern arguments (used for `glClearColor`)
- `kryos_ffi_dlcall7` / `kryos_ffi_dlcallv5..7` — higher-arity FFI call
  helpers for 7-argument functions like `glReadPixels`
- `kryos_ffi_read_f32_bits` / `kryos_ffi_read_f64_bits` — read float memory
  as integer bit-patterns

### Verified
- Mesa software renderer (`LIBGL_ALWAYS_SOFTWARE=1`) renders orange cube;
  `glReadPixels` confirms 26 759 non-background pixels (vs 0 for clear)
- GL version 4.5 core profile obtained via `SDL_GL_CreateContext` offscreen

## [1.6.0] - 2026-05-15 — "HTTP/2, PostgreSQL, TLS server"

This release closes Gap C (HTTP/2 client) to complete the networking trifecta
alongside Gap A (TLS server) and Gap B (PostgreSQL driver) from v1.5.

### Added — HTTP/2 client (Gap C)

- `http2_get(url: str) -> str` — GET request with ALPN-negotiated HTTP/2,
  automatic HTTP/1.1 fallback, and shared connection pool. Returns body.
- `http2_post(url: str, body: str) -> str` — POST with body, returns response body.
- `http2_request(method: str, url: str, headers: str, body: str) -> str` — full
  request with method/headers/body control. headers is `"Name1: val1\nName2: val2"`
  newline-separated. Returns `"<status>\n<headers>\n\n<body>"` for complete
  response inspection.

All three builtins share a single `reqwest` blocking client instance (via
`OnceLock`) so the connection pool is reused across calls. `User-Agent: kryos/1.6`
is set by default. The h2 feature flag is enabled by default.

`examples/http2_demo.kry` demonstrates all three builtins against Cloudflare
(h2 trace) and httpbin.org (POST echo + custom headers).

---

## [1.5.0] - 2026-05-15 — "close the last five gaps"

This release closes every remaining gap toward true universality.
Kryos can now do TLS/HTTPS, build DOM/canvas/fetch WASM modules,
run cooperative async event loops, manage packages via a GitHub-backed
registry, drive a real Chromium browser over CDP+WebSocket, and paint
immediate-mode GUIs via SDL2 — all from pure Kryos.

### Added — Async I/O primitives

- `tcp_set_nonblocking(fd, bool) -> i64` — flip a socket between blocking
  and non-blocking modes.
- `tcp_try_accept(listener) -> i64` — returns 0 if no client is waiting
  instead of blocking. Pairs with `sleep_ms` for cooperative event loops.
- `tcp_try_recv(fd, max) -> str` — returns an empty string on WouldBlock.
- `poll_readable(fds, n, timeout_ms) -> i64` — bitmask of fds that
  became readable within the timeout (up to 63 fds).
- `sleep_ms(ms)` — sub-second cooperative pacing.

`examples/async_echo.kry` is a single-threaded non-blocking echo server
that accepts real TCP clients without spawning threads.

### Added — Crypto / binary primitives

- `sha1_hex(s) -> str` and `sha1_base64(s) -> str` — legacy SHA-1
  (RFC 6455 WebSocket handshakes, etc.) verified against the FIPS-180
  test vector for "abc".
- `base64_encode(s) -> str` / `base64_decode(s) -> str` — round-trip
  via latin-1 codepoints so binary data survives Kryos strings.
- `chr(n) -> str` / `byte_at(s, i) -> i64` — single-byte read/write
  primitives for hand-rolled binary protocols.

### Added — Browser bots (CDP)

- `examples/websocket_client.kry` — full RFC 6455 client framing:
  handshake (verified against the RFC test vector), masked text/ping/
  close frames, frame decoder for masked + unmasked frames.
- `examples/cdp_bot.kry` — Chrome DevTools Protocol driver that probes
  `http://localhost:9222/json` for an attached browser, opens a per-tab
  WebSocket, and dispatches `Page.navigate`, `Runtime.evaluate`, and
  `Page.captureScreenshot` over JSON-RPC.

### Added — Immediate-mode GUI

- `examples/sdl_imgui.kry` — pure-Kryos immediate-mode GUI on top of
  the existing SDL2 FFI bindings. Includes title bar, hoverable buttons,
  checkboxes, and a value slider. Runs headless under
  `SDL_VIDEODRIVER=dummy` for CI.

### Added — Package manager + registry

- Default registry rewritten to
  `https://github.com/NORTHTEKDevs/kryos-registry` (created this release,
  seeded with example entries).
- `kryos pkg add <name>` accepts bare names (resolves to `<name>@*`).
- `Op::Wildcard` added to the semver type so `*` matches any version.

### Notes

- TLS / HTTPS were already complete in 1.2.0 via rustls; verified
  end-to-end against httpbin.org and api.github.com in `examples/https_demo.kry`.
- WASM v0.3 (linear-memory arrays) and v0.4 (DOM/canvas/fetch host
  imports) shipped in this cycle on top of WASM v0.2 strings.

## [1.2.0] - 2026-05-15 — "truly universal: C FFI + graphics + WASM v0.2"

Kryos can now call any C library at runtime via dlopen/LoadLibrary,
including driving the full SDL2 window + renderer pipeline from pure
Kryos. No compiler changes were needed — the existing `extern "C"`
declaration syntax plus a small runtime in `kryos-stdlib-native::ffi`
is enough.

The WASM backend gained first-class strings: a `str` value now survives
in locals, parameters, and returns as a packed (offset, length) i64, and
string concatenation with `+` and `len(s)` both work in WASM modules.

This is the milestone where Kryos stops being a closed language and
becomes a real systems-level tool: anything libc, SDL2, libcurl,
libsqlite3, libssl, or any other shared library can do, Kryos can
now do. And anything text-shaped that runs in a browser can now be
written in Kryos.

### Added

- **Dynamic library FFI runtime** (`kryos-stdlib-native::ffi`).
  New runtime symbols:
  - `kryos_ffi_dlopen(name) -> handle` — wraps `dlopen` on Unix and
    `LoadLibraryA` on Windows.
  - `kryos_ffi_dlsym(handle, name) -> fnptr` — wraps `dlsym` /
    `GetProcAddress`.
  - `kryos_ffi_dlclose(handle)` — `dlclose` / `FreeLibrary`.
  - `kryos_ffi_dlcall0..6(fp, args...)` — call a resolved function
    pointer with 0-6 i64 args, returns i64.
  - `kryos_ffi_dlcallv0..4(fp, args...)` — void-return variants.
  - `kryos_ffi_dlcallv_4f(fp, f64, f64, f64, f64)` — four-f64-args
    helper for graphics APIs.
  - `kryos_ffi_cstr(s) -> *const char` — zero-copy convert a Kryos
    string to a NUL-terminated C string (KryosString is already
    NUL-terminated by design).
  - `kryos_ffi_string_from_ptr(ptr, len)` — read a C string back
    into a Kryos string (len = -1 uses strlen).
  - `kryos_ffi_malloc(n)` / `kryos_ffi_free(p, n)` — allocate raw
    memory blocks for C interop.
  - `kryos_ffi_read_i8/16/32/64`, `kryos_ffi_read_f32/64`, plus
    write variants — pointer-typed memory I/O.
- **C-compatible static-link FFI.** `extern "C" { fn foo(...); }`
  declarations are auto-resolved as `Linkage::Import` by Cranelift
  and linked against libc / system libs at build time. Works for
  any symbol the system linker can find.
- **SDL2 graphics demo, in pure Kryos.**
  - `examples/sdl_info.kry` — initializes SDL, queries version,
    platform, CPU count, RAM, performance counter.
  - `examples/sdl_window.kry` — opens a 320×240 window, creates a
    renderer, draws three colored rectangles (red, green, blue),
    presents the frame.
  - `examples/sdl_savepng.kry` — same scene but renders offscreen
    and dumps the framebuffer to disk via libc `fwrite`. Used to
    generate `docs/screenshots/sdl_kryos_demo.png` — the first
    rendered graphical output produced by a Kryos program.
- **libc FFI examples.**
  - `examples/ffi_libc.kry` — static-link smoke test
    (`getpid`/`getuid`/`time`).
  - `examples/ffi_module.kry` — dlopen libc and exercise
    `malloc`/`free`/`strlen`/`getpid` end-to-end.
  - `examples/ffi_test.kry`, `examples/ffi_dlopen.kry` — minimal
    starter snippets.
- **stdlib `ffi.kry` module.** Idiomatic wrappers (`dlopen`,
  `dlsym`, `call0..6`, `cstr`, `malloc`, etc.) for users who prefer
  named imports over raw `extern` blocks.

- **WASM v0.2: first-class strings.**
  - Strings now lower to a packed i64 `(offset | length << 32)` instead
    of just an i32 offset, so a `str` is a real value: it survives in
    locals, parameters, and returns without a side table.
  - String concatenation with `+` works end-to-end: the WASM backend
    detects `BinOp::Add` between two `Str` operands and emits a call to
    a new host import `kryos_string_concat(off1,len1,off2,len2) -> i64`.
  - `len(s)` is now a single `i64.shr_u` — the length is already there.
  - `println(s)` works on any string operand, not just literals.
  - New host imports plumbed (Node + browser): `kryos_string_concat`,
    `kryos_array_new`, `kryos_array_get`, `kryos_array_set`. Array
    builtins are wired in the runtime; Kryos-source-level array use in
    WASM lands in v0.3.
  - New example: `examples/wasm_strings.kry` — a Kryos program that
    builds greetings with `+`, prints them, and prints their lengths,
    running in Node and in any browser.
  - Updated `examples/wasm_runner.js` and `examples/wasm_browser_demo.html`
    with the new imports and a bump allocator above the static-data
    region (heap starts at 32 KB, memory grows on demand).

### Verified working

- `extern "C" { fn getpid() -> i64 }` returns the real PID.
- dlopen("libc.so.6") + dlsym("malloc") + write_i64 + read_i64 + free
  roundtrip.
- dlopen("libSDL2-2.0.so.0") + `SDL_Init` + `SDL_CreateWindow` +
  `SDL_CreateRenderer` + `SDL_SetRenderDrawColor` + `SDL_RenderClear`
  + `SDL_RenderFillRect` (3 colored rects) + `SDL_RenderPresent` +
  `SDL_RenderReadPixels` + clean shutdown — all from Kryos, **with
  no compiler changes**.
- `let s = "hello, " + name + "!"` compiles to WASM, runs in Node, and
  prints the concatenated string.
- All six v0.1 WASM examples (`wasm_hello`, `wasm_math`, `wasm_loop`,
  `wasm_fizz`, `wasm_control`, `wasm_browser_demo`) still pass.

## [1.1.0] - 2026-05-15 — "universal target"

Kryos now compiles to WebAssembly in addition to native code. The same
`.kry` source can be built to a native binary (Cranelift or LLVM) or to
a `.wasm` module that runs in any browser or WASI host.

Also fixes a long-standing TCP concurrency bug: spawned worker threads
no longer serialize through a global socket mutex during blocking
send/recv. Multi-client servers actually scale now.

### Added

- **WebAssembly backend (`--backend wasm`).** New crate
  `kryos-codegen-wasm` emits standalone `.wasm` modules. Supported in
  v0.1: i64/f64 arithmetic, comparisons, booleans, if/else/elif chains,
  while loops, function definitions, direct calls, recursion,
  `println(i64)`, `println(f64)`, `println(str-literal)` via host imports.
- **Browser demo.** `examples/wasm_browser_demo.{kry,wasm,html}` — a
  Kryos program (fib + factorial + sum) running in a real browser via
  `fetch` + `WebAssembly.instantiate`.
- **WASM host runner.** `examples/wasm_runner.js` — a 60-line Node.js
  host that provides the three imports the WASM backend expects.
- **New example programs.** `wasm_hello.kry`, `wasm_math.kry`,
  `wasm_fizz.kry`, `wasm_loop.kry`, `wasm_control.kry` — covers each
  category of WASM-supported control flow.

### Fixed

- **TCP send/recv no longer block other socket operations.** The
  global socket-table mutex is now released before `TcpStream::read`
  and `TcpStream::write` are called (via `try_clone`), matching the
  pattern `tcp_accept` already used. Verified by serving three
  concurrent curl requests against `examples/showcase/web_server.kry`.

### Backend coverage matrix

| Feature              | Cranelift | LLVM | WASM v0.1 |
|----------------------|:---------:|:----:|:---------:|
| Integer/float arith  | ✅ | ✅ | ✅ |
| Booleans, comparisons| ✅ | ✅ | ✅ |
| if/else/elif         | ✅ | ✅ | ✅ |
| while loops          | ✅ | ✅ | ✅ |
| Functions, recursion | ✅ | ✅ | ✅ |
| println              | ✅ | ✅ | ✅ |
| Heap strings         | ✅ | ✅ | ❌ |
| Arrays, maps         | ✅ | ✅ | ❌ |
| Structs, enums       | ✅ | ✅ | ❌ |
| Channels, spawn      | ✅ | ✅ | ❌ |
| HTTP, regex, JSON    | ✅ | ✅ | ❌ |

The `❌` rows in the WASM column track to v1.2 — they need ARC + a
linear-memory string runtime + WASI imports. v0.1 is intentionally
scoped to "the parts that need no heap".

## [1.0.1] - 2026-05-15 — "universal-language stress test"

Wrote 8 different classes of program in pure Kryos to validate the
universal-language claim. Found and fixed one real bug along the way.

### Added (showcases)
- `examples/showcase/extra/calc.kry` — arithmetic parser with recursive
  descent + precedence + mutual function recursion.
- `examples/showcase/extra/csv.kry` — CSV reader with group-by salary
  aggregation.
- `examples/showcase/extra/brainfuck.kry` — full Brainfuck interpreter
  (prints "Hello World!").
- `examples/showcase/extra/life.kry` — Conway's Game of Life on a 20×20
  grid (glider, blinker).
- `examples/showcase/extra/api_client.kry` — outbound HTTPS plus JSON
  tree walk against httpbin.org.
- `examples/showcase/extra/regression.kry` — linear regression by
  gradient descent. Learns y = 3x + 7 from noisy samples.
- `examples/showcase/extra/template.kry` — Mustache-style `{{var}}`
  templating engine.
- `examples/showcase/extra/regex.kry` — tiny regex engine: literals,
  `.`, `*`, `^`, `$`.

### Fixed
- **`sleep_ms` no longer fails to link.** The builtin was registered in
  the MIR builtins table but had no runtime symbol or codegen wiring.
  Now properly implemented as `kryos_rt::spawn::kryos_sleep_ms`, with
  Cranelift codegen dispatch and JIT symbol registration. Verified end
  to end: `sleep_ms(500)` waits exactly 500 ms.

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
