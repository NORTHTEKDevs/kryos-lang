# Kryos Stdlib & Builtin Reference

This file is the single-page reference for everything you can call from a
Kryos program without importing third-party code. It lists every always-available
builtin, summarises every stdlib module, calls out the naming gotchas that
bite new users, and documents the `@test` annotation and the `kryos` CLI
surface.

For deep dives on individual modules, see [`docs/stdlib/`](stdlib/README.md).

---

## 1. Always-available builtins

These are registered by the compiler and require no `use` statement. They
fall into a handful of groups; each group lists the function signature and
a one-line summary. Return types follow Kryos naming: `i64`, `f64`, `str`,
`bool`, `void`, `[T]` for arrays.

### 1.1 I/O & process

| Builtin                         | Returns | Notes                                                  |
| ------------------------------- | ------- | ------------------------------------------------------ |
| `println(msg, ...)`             | void    | Write a line to stdout with interpolation support.     |
| `print(msg, ...)`               | void    | Same but without trailing newline.                     |
| `eprintln(msg, ...)`            | void    | Write a line to stderr.                                |
| `input(prompt)`                 | str     | Read a line of input.                                  |
| `readline()`                    | str     | Read a line from stdin (no prompt).                    |
| `read_line()`                   | str     | Alias of `readline()`.                                 |
| `exit(code)`                    | void    | Terminate the process with `code`.                     |
| `args()`                        | [str]   | Program arguments. `args()[0]` is the first user arg.  |
| `env_get(name)`                 | str     | Read an environment variable (empty string if unset).  |
| `sleep(secs)`                   | void    | Block for `secs` seconds.                              |
| `sleep_ms(ms)`                  | void    | Block for `ms` milliseconds.                           |

### 1.2 Assertions, panics, tests

| Builtin              | Returns | Notes                                                                 |
| -------------------- | ------- | --------------------------------------------------------------------- |
| `assert(cond)`       | void    | Aborts if `cond` is false.                                            |
| `assert_eq(a, b)`    | void    | Aborts if `a != b`; prints both stringified values on failure (v2.7+). |
| `panic(msg)`         | void    | Abort with a message. Exits with code 101.                            |
| `type_of(value)`     | str     | Returns a string naming the runtime type.                             |

### 1.3 Strings

| Builtin                         | Returns | Notes                                            |
| ------------------------------- | ------- | ------------------------------------------------ |
| `len(s)`                        | i64     | Length in bytes (for ASCII strings, character count). |
| `to_string(x)`                  | str     | Convert int/float/bool/char to string.           |
| `to_upper(s)` / `to_lower(s)`   | str     | Case conversion.                                 |
| `trim(s)` / `trim_start` / `trim_end` | str | Whitespace trimming.                             |
| `substr(s, start, end)`         | str     | Slice by byte offset.                            |
| `contains(s, needle)`           | bool    | Substring test (also works for arrays).          |
| `starts_with(s, prefix)`        | bool    |                                                  |
| `ends_with(s, suffix)`          | bool    |                                                  |
| `replace(s, from, to)`          | str     | First-occurrence replace.                        |
| `split(s, sep)`                 | [str]   | Returns an array of strings.                     |
| `join(arr, sep)`                | str     | Join `[str]` with separator.                     |
| `index_of(s, needle)`           | i64     | Returns -1 when not found.                       |
| `char_code(s)` / `char_from(n)` | i64/str | ASCII helpers.                                   |
| `byte_at(s, i)` / `chr(n)`      | i64/str | Byte access / inverse of `char_code`.            |
| `format(fmt, args...)`          | str     | printf-style formatter (also covered by interpolation). |
| `parse_int(s)` / `parse_float(s)` | i64/f64 | String to number.                              |

String interpolation lives in the lexer: `"hello {name}, you have {count} items"`. Literal
braces are written as `{{` and `}}` (v2.8+), or `\{` and `\}` (still supported).

### 1.4 Numbers & math

| Builtin                              | Returns | Notes                                            |
| ------------------------------------ | ------- | ------------------------------------------------ |
| `abs(i)` / `abs_f(f)`                | i64/f64 | Absolute value (typed variants).                 |
| `min(a, b)` / `max(a, b)`            | i64     |                                                  |
| `min_f(a, b)` / `max_f(a, b)`        | f64     |                                                  |
| `sqrt(f)` / `pow(b, e)`              | f64     |                                                  |
| `floor(f)` / `ceil(f)` / `round(f)`  | f64     |                                                  |
| `sin(f)` / `cos(f)` / `tan(f)`       | f64     | Radians.                                         |
| `log(f)` / `log2(f)` / `log10(f)`    | f64     | Natural, base-2, base-10.                        |
| `int(x)` / `float(x)`                | i64/f64 | Cross-cast (truncating for int).                 |
| `wrapping_add/sub/mul`               | i64     | Two's-complement wrap on overflow.               |
| `checked_add/sub/mul`                | i64     | Throws on overflow (Result-style sites pending). |
| `saturating_add/sub/mul`             | i64     | Saturates at i64::MIN / i64::MAX.                |
| `time_now()` / `time_now_secs()` / `time_now_millis()` | i64 | Wall-clock time.                |

### 1.5 Arrays

| Builtin                  | Returns | Notes                                                     |
| ------------------------ | ------- | --------------------------------------------------------- |
| `len(arr)`               | i64     | Same name as the string length builtin.                   |
| `push(arr, v)`           | [T]     | Returns the (possibly resized) array handle.              |
| `pop(arr)`               | T       | Removes and returns the last element.                     |
| `sort(arr)`              | void    | In-place ascending sort.                                  |
| `reverse(arr)`           | void    | In-place reverse.                                         |
| `contains(arr, v)`       | bool    | Membership check.                                         |
| `keys(map)`              | [T]     | Returns array of keys (use `map_keys` / `map_keys_str` for typed variants). |

### 1.6 Filesystem

| Builtin                                  | Returns | Notes                                |
| ---------------------------------------- | ------- | ------------------------------------ |
| `file_read(path)` / `read_file(path)`    | str     | Both names work; prefer `file_read`. |
| `file_write(path, contents)`             | i64     | Returns bytes written.               |
| `append_file(path, contents)`            | i64     |                                      |
| `file_exists(path)` / `file_size(path)`  | bool/i64 |                                     |
| `create_dir(path)`                       | void    |                                      |

### 1.7 Networking & HTTP

`tcp_*`, `tls_*`, `uds_*`, `ws_*`, `http_*`, `https_*`, `http2_*`, and
`pg_*` (PostgreSQL) builtins are registered in the compiler and backed by
runtime FFI. See [`docs/stdlib/net.md`](stdlib/net.md) and
[`docs/stdlib/http.md`](stdlib/http.md) for full signatures; the count and
type-registry live in `compiler/crates/kryos-mir/src/lower.rs`.

### 1.8 Binary buffers

`buf_new`, `buf_write_byte`, `buf_write_i16_le`, `buf_write_i32_le`,
`buf_write_i64_le`, `buf_write_bytes`, `buf_write_str`, `buf_write_zeros`,
`buf_len`, `buf_get_byte`, `buf_set_byte`, `buf_patch_i32_le`,
`buf_patch_i64_le`, `buf_write_to_file`, `buf_free`. These are the raw
byte-buffer primitives used by codegen back-ends and serialization libs.

### 1.9 JSON, crypto, regex

All `json_*`, `sha256` / `sha512` / `sha1_hex` / `sha1_base64`,
`base64_encode` / `base64_decode`, `random_bytes`, and `regex_*` are
always available. See the corresponding stdlib doc page for each module.

### 1.10 Concurrency & channels

`chan(capacity)`, `send(ch, v)`, `recv(ch)`, `close_chan(ch)`,
`mutex_new()`, `mutex_lock(m)`, `mutex_unlock(m)`, `mutex_drop(m)`.
Actor model lives in `std::agent`.

### 1.11 Browser host (WASM target)

When compiling with `--backend wasm`, these host imports are available:
`dom_set_text`, `dom_get_value`, `alert`, `canvas_fill_rect`,
`canvas_clear`, `fetch_text`.

---

## 2. Stdlib modules (loaded with `use`)

Source lives in [`compiler/stdlib/`](../compiler/stdlib/), documented per
module under [`docs/stdlib/`](stdlib/). Brief index:

| `use` path             | Purpose                                             |
| ---------------------- | --------------------------------------------------- |
| `std::collections`     | `map`, `filter`, `reduce`, `sort_by`, `enumerate`.  |
| `std::iter`            | Lazy iterator combinators.                          |
| `std::string`          | Higher-level string utilities beyond core builtins. |
| `std::math`            | Extended math (`pi`, `e`, `random`).                |
| `std::json`            | Ergonomic wrappers around `json_*` builtins.        |
| `std::fs`              | File-system helpers built on `file_*` builtins.     |
| `std::os`              | Platform detection (`name`, `arch`, `is_linux`).    |
| `std::process`         | Process spawn / pipe wrappers.                      |
| `std::path`            | Path manipulation.                                  |
| `std::net`             | HTTP/HTTPS client, TCP/UDS wrappers.                |
| `std::http`            | Higher-level HTTP request / response.               |
| `std::re`              | Regular expressions.                                |
| `std::crypto`          | Hashing and random bytes.                           |
| `std::datetime`        | Calendar dates, formatting, durations.              |
| `std::term`            | Terminal control (raw mode, color, dimensions).     |
| `std::db`              | Database client (currently SQLite-flavoured).       |
| `std::ffi`             | Pointer ops for `extern "C"` interop.               |
| `std::sync`            | Mutex helpers, latches, barriers.                   |
| `std::chan`            | Channel wrappers on top of `chan`/`send`/`recv`.    |
| `std::stream`          | Reactive streams (pure Kryos).                      |
| `std::tensor`          | N-d tensors (FFI-backed).                           |
| `std::probable`        | Confidence-tagged values.                           |
| `std::agent`           | Agent runtime with memory + tools + alignment.      |
| `std::tracked`         | Lineage tracking.                                   |
| `std::cost`            | AI compute budget enforcement.                      |
| `std::result`          | `Result<T, E>` helpers.                             |
| `std::option`          | `Option<T>` helpers.                                |
| `std::test`            | Test harness primitives.                            |
| `std::fmt`             | Format / pretty-print helpers.                      |
| `std::wasm`            | WASM browser host utility wrappers.                 |

---

## 3. Naming gotchas

These are the cases where a user-friendly name and the actual builtin
name diverge. Almost every "function not found" mistake new users hit
comes from one of these.

| You might write     | Use this instead                                |
| ------------------- | ----------------------------------------------- |
| `length(x)`         | `len(x)` (works for both strings and arrays)    |
| `print_line`        | `println`                                       |
| `string(x)`         | `to_string(x)`                                  |
| `upper(s)`          | `to_upper(s)`                                   |
| `lower(s)`          | `to_lower(s)`                                   |
| `slice(s, a, b)`    | `substr(s, a, b)`                               |
| `find(s, n)`        | `index_of(s, n)` (returns -1 when missing)      |
| `range_contains(a)` | `contains(arr, a)`                              |
| `read_input`        | `input(prompt)` / `readline()` / `read_line()`  |
| `os.getenv`         | `env_get(name)` (top-level builtin)             |
| `sys.exit`          | `exit(code)` (top-level builtin)                |
| `Array::new()`      | Literal: `let a: [i64] = []`                    |
| `Map::new()`        | Builtin: `let m = map_new()`                    |
| `now()`             | `time_now()` (seconds-resolution wall clock)    |

Other things that catch people:

- `len` returns **bytes**, not Unicode codepoints. For ASCII strings the
  two coincide; for UTF-8, write your own char iterator if you care.
- `push` returns the array; assign it back: `arr = push(arr, v)`.
- `read_file(path)` and `file_read(path)` are aliases; both work, pick
  one and stay consistent.
- `time_now()` returns seconds; `time_now_millis()` returns milliseconds.
  There is no `time_now_nanos()` yet.
- Integer arithmetic panics on overflow in debug mode. Use the
  `wrapping_*`, `checked_*`, or `saturating_*` family explicitly when
  you need defined behaviour.
- `json_parse` returns an opaque handle (`i64`). Use `json_get` /
  `json_to_str` / `json_to_int` / `json_to_float` to extract values.
- String interpolation uses `{expr}` braces. Use `{{` / `}}` (v2.8+) for
  literal braces, or `\{` / `\}` (older form, still supported).

---

## 4. The `@test` annotation

Functions annotated with `@test` are discovered by `kryos test` and
executed in a JIT thunk (no separate test binary). They take no
arguments and return `void`; failures are signalled by `assert`,
`assert_eq`, or `panic`.

```kryos
@test
fn test_addition() {
    assert_eq(2 + 2, 4)
}

@test
fn test_string_split() {
    let parts = split("a,b,c", ",")
    assert_eq(len(parts), 3)
    assert_eq(parts[0], "a")
}
```

`@test` functions live alongside regular code in any `.kry` file.
`kryos test` will:

1. Search the current project (or `--path PATH`) for `.kry` files.
2. Compile each, discovering `@test fn ...` items.
3. Run each test in isolation via the Cranelift JIT (very fast startup).
4. Print a summary and exit non-zero if any test failed.

Filter syntax (positional argument):

```
kryos test                       # run everything
kryos test math                  # run tests whose name contains "math"
kryos test test_addition --exact # run only the exact name
kryos test --list                # list discovered tests, don't run
kryos test --format json         # machine-readable output for CI
kryos test --nocapture           # show stdout from passing tests
```

The smoke tests under [`tests/smoke/`](../tests/smoke/) demonstrate the
mix of top-level `main()` (AOT path) and `@test` thunks (JIT path) that
the test harness exercises.

---

## 5. The `kryos` CLI surface

Run `kryos help <command>` for full options. Summary of every
subcommand:

| Subcommand   | What it does                                                                |
| ------------ | --------------------------------------------------------------------------- |
| `kryos build [PATH]`  | Compile a project or single file. Flags: `--release`, `--backend cranelift\|llvm\|wasm`, `--target`, `-o OUTPUT`, `--emit-mir`, `--emit-llvm`, `--cache`, `--lto`, `-g`. |
| `kryos run FILE [ARGS...]` | Compile + run via the Cranelift JIT.                                  |
| `kryos check [PATH]`  | Parse + type-check only.                                                |
| `kryos repl`          | Interactive REPL.                                                       |
| `kryos test [FILTER]` | Discover and run `@test fn ...` tests. Flags: `--exact`, `--list`, `--format pretty\|json`, `--nocapture`, `--path`. |
| `kryos fmt [FILES...]` | Format `.kry` files. `--check` for dry-run.                            |
| `kryos doc [FILES...]` | Generate Markdown (default) or HTML docs from sources. `--html`, `-o`. |
| `kryos bindgen HEADER` | Generate Kryos `extern "C"` bindings from a C header.                  |
| `kryos pkg <init\|add\|remove\|update\|install\|lock\|publish\|search\|info\|sync\|outdated>` | Package manager (registry currently empty). |
| `kryos lsp`           | Start the language server (stdio).                                      |
| `kryos explain CODE`  | Long-form explanation of an error code (`kryos explain E0302`). `--list` to see every code. |
| `kryos version`       | Detailed build info (compiler version, commit, target).                 |

Environment variables the compiler reads:

| Variable           | Effect                                                  |
| ------------------ | ------------------------------------------------------- |
| `KRYOS_DUMP_IR=1`  | Dump Cranelift IR to stderr during `kryos run`.         |
| `KRYOS_CACHE_DIR`  | Override the build cache directory.                     |
| `XDG_CACHE_HOME`   | Fallback cache directory parent.                        |
| `HOME`             | Fallback for `~/.cache/kryos`.                          |
| `RUST_BACKTRACE`   | Standard Rust panic backtrace control (compiler bugs).  |

Useful one-liners:

```
# Dump MIR to a file (for debugging codegen issues):
kryos build src/main.kry --emit-mir -o /tmp/dump.mir

# Format every .kry file in a project:
kryos fmt

# Quick CI: type-check only, no codegen:
kryos check .

# Run a single test by exact name in JSON mode:
kryos test test_my_thing --exact --format json
```

---

## 6. Where to file gaps

If a builtin or stdlib function does not behave as documented here, please
open an issue at <https://github.com/NORTHTEKDevs/kryos-lang/issues> and
include:

1. A minimal `.kry` repro.
2. The output of `kryos version`.
3. Whether it reproduces under `kryos run` (Cranelift JIT) and / or
   `kryos build --release` (LLVM AOT).
