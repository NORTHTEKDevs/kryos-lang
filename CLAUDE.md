# CLAUDE.md

This file gives Claude (Code, Computer, or any other Anthropic-driven tool) the context it needs to write correct, idiomatic Kryos. Drop it into any project that uses Kryos and Claude will read it automatically.

If you're reading this as Claude: **everything below is authoritative for the toolchain at this commit.** The full reference is in `docs/19-language-reference.md`; this file is the working subset you need to be productive without trial-and-error.

## What Kryos is

A capability-safe, ownership-aware systems language with two backends:

- **Cranelift** (`kryos run`) — debug JIT, fast compile, no external linker
- **LLVM** (`kryos build --release`) — release AOT, full optimization, links via `cc`/`clang`/`link.exe`

Targets Linux x86_64, Windows x86_64 MSVC, macOS x86_64/aarch64, and `wasm32-unknown-unknown` / `wasm32-wasi`. The same `.kry` source runs on every target — the only platform-conditional code you should write is filesystem-path handling.

## Hard rules (these cause compile errors)

1. **No semicolons.** Line breaks terminate statements. To wrap a long expression, end the line with `(`, `[`, `{`, `,`, or a binary operator.
2. **Block comments `/* ... */` work and nest.** Line comments are `//`, doc comments `///`. (The self-host compiler source under `compiler/self-host/` avoids block comments by convention, but the language supports them.)
3. **No `null` / `nil` keyword.** Use `Option<T>` from `std::option` for nullable values.
4. **String interpolation works:** `"hello {name}"` (the delimiter is `{ }`, NOT `${ }`). `+` concatenation also works; cast numerics with `to_string(x)` when concatenating.
5. **`let` is immutable, `let mut` is mutable.** Same as Rust.
6. **Type annotations on top-level `let` and on function params are required.** Local `let` inside a function can infer.
7. **Functions return the last expression** only if there's no trailing newline after it. Prefer explicit `return`.
8. **Top-level `let mut` cannot call functions** that touch the FFI surface directly (use `env_get`, `args`, etc. which are builtins — see "Builtins available everywhere" below). Move complex initialization into `main()`.

## Minimal program

```kryos
fn main() {
    let name: str = "World"
    println("Hello, " + name + "!")
}
```

Compile and run:

```bash
kryos run hello.kry              # Cranelift JIT, no binary produced
kryos build --release hello.kry  # LLVM AOT, produces ./hello (or hello.exe on Windows)
kryos build --release -g hello.kry -o hello  # with debug info (.pdb on Windows, DWARF elsewhere)
```

## Types

| Type        | Notes                                                              |
| ----------- | ------------------------------------------------------------------ |
| `i64`       | Default integer. Overflow is checked in debug, wraps in release.   |
| `f64`       | Default float. IEEE 754.                                           |
| `bool`      | `true` / `false`. No truthy coercion.                              |
| `str`       | UTF-8 owned string. `+` concatenates. `len(s)` returns byte count. |
| `[T]`       | Owned dynamic array. `len`, `push`, `pop`, `arr[i]`.               |
| `map<K, V>` | Hash map. `m[k]`, `m[k] = v`, `contains(m, k)`.                    |
| `(A, B, C)` | Tuple.                                                             |
| `Option<T>` | From `std::option`. `Some(x)` / `None()`.                          |
| `Result<T, E>` | From `std::result`. `Ok(x)` / `Err(e)`.                         |
| `*T`        | Raw pointer. Only inside `unsafe` or `extern` blocks.              |

Casting uses `as`: `let x: f64 = (n as f64)`. Booleans cast to integers as `0`/`1`. Strings to numbers go through `parse_int` / `parse_float`.

## Functions

```kryos
fn add(a: i64, b: i64) -> i64 {
    return a + b
}

/// Doc comment. Picked up by `kryos doc`.
fn greet(name: str) -> str {
    return "Hello, " + name
}

fn no_return() {       // unit return is implicit
    println("side effect")
}
```

## Control flow

```kryos
if x > 0 {
    println("positive")
} elif x == 0 {       // note: `elif`, not `else if`
    println("zero")
} else {
    println("negative")
}

while i < 10 {
    i = i + 1
}

for x in arr {        // iterates over [T]
    println(to_string(x))
}

loop {                // infinite, break to exit
    if done() { break }
}

match v {
    Some(x) => println("got " + to_string(x)),
    None()  => println("nothing"),
}
```

## Structs and enums

```kryos
struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn new(x: f64, y: f64) -> Point {
        return Point { x: x, y: y }
    }

    fn distance(self: Point, other: Point) -> f64 {
        let dx = self.x - other.x
        let dy = self.y - other.y
        return sqrt(dx * dx + dy * dy)
    }
}

enum Shape {
    Circle(f64),
    Rect(f64, f64),
    Empty,
}

fn area(s: Shape) -> f64 {
    match s {
        Circle(r)    => return 3.14159 * r * r,
        Rect(w, h)   => return w * h,
        Empty        => return 0.0,
    }
}
```

## Ownership (the part Rust users will find familiar)

Each value has exactly one owner. Passing a value to a function moves it; you can't use the original variable afterward. To keep using a value, either return it or take it by reference (current ergonomics: clone explicitly when in doubt — the compiler will tell you).

```kryos
fn main() {
    let s: str = "hello"
    consume(s)
    // println(s)  // ERROR: `s` moved into consume
}

fn consume(x: str) {
    println(x)
}
```

Strings and arrays are owned heap values. Primitives (`i64`, `f64`, `bool`) are `Copy` — they don't move. Structs containing only `Copy` fields are also `Copy`.

If you see `error[E0300]: use of moved value`, you need to either clone (`x.clone()` where available), pass by reference, or restructure to consume the value once.

## Error handling

Kryos uses `Result<T, E>` plus `throw` for unrecoverable failures.

```kryos
use std::result::{Result, Ok, Err}

fn divide(a: i64, b: i64) -> Result<i64, str> {
    if b == 0 {
        return Err("division by zero")
    }
    return Ok(a / b)
}

fn main() {
    match divide(10, 2) {
        Ok(v)  => println("result: " + to_string(v)),
        Err(e) => println("error: " + e),
    }
}
```

`throw "message"` aborts with a panic. Use it only for invariant violations the caller can't reasonably recover from.

## Capabilities (Kryos's defining feature)

Every function has a capability set inferred from the builtins it calls. If your function calls `file_write`, it needs the `io` capability. If you `spawn` a process, you need `process`. If you read from the network, you need `net`. The compiler tracks these through the call graph and surfaces them at the function signature.

For now: just call what you need. The compiler will require a `// capabilities: io, net` annotation on the entry point if you turn on strict mode. See `docs/10-capabilities.md` for the details.

## Builtins available everywhere

You do not need to `use` these — they're in the global namespace:

| Builtin                             | Purpose                                        |
| ----------------------------------- | ---------------------------------------------- |
| `println(s: str)` / `print(s: str)` | Stdout. `println` adds `\n`.                   |
| `len(x)`                            | Length of str / array / map.                   |
| `to_string(x)`                      | Convert i64/f64/bool to str.                   |
| `parse_int(s) -> i64`               | Parse integer.                                 |
| `parse_float(s) -> f64`             | Parse float.                                   |
| `args() -> [str]`                   | Command-line arguments.                        |
| `env_get(key: str) -> str`          | Environment variable (`""` if unset).          |
| `file_read(path: str) -> str`       | Read entire file as UTF-8.                     |
| `file_write(path, content)`         | Write file. Does NOT create parent dirs.       |
| `file_exists(path) -> i64`          | `1` if exists, `0` otherwise.                  |
| `create_dir(path)`                  | Create directory (idempotent).                 |
| `push(arr, item) -> [T]`            | Return new array with item appended.           |
| `substr(s, start, end) -> str`      | Byte-indexed substring.                        |
| `split_lines(s) -> [str]`           | Split on `\n` or `\r\n`.                       |
| `char_code(s) -> i64`               | First byte of `s` as integer.                  |
| `contains(haystack, needle)`        | Substring search.                              |
| `sqrt`, `pow`, `sin`, `cos`         | Math (also in `std::math`).                    |

## Standard library

Imported with `use std::<module>::{symbol1, symbol2}`. Available modules:

`agent`, `chan`, `collections`, `cost`, `crypto`, `datetime`, `db`, `ffi`, `fmt`, `fs`, `http`, `io`, `iter`, `json`, `math`, `net`, `option`, `os`, `path`, `probable`, `process`, `re`, `result`, `stream`, `string`, `sync`, `tensor`, `term`, `test`, `tracked`, `wasm`.

```kryos
use std::math::{abs, min, max}
use std::json::{json_stringify, json_object, json_string}

fn main() {
    println(to_string(abs(-42)))
}
```

### Known limitation in the module resolver

The resolver does not currently follow transitive references from a `use`-imported function into the FFI primitives that function calls. **Symptom:** `use std::os::{temp_dir}` fails because `temp_dir` calls `_env_or_empty` which calls the `kryos_env_get` extern. **Workaround:** call the underlying builtin directly (`env_get("TMPDIR")`) — those are always in scope.

## Cross-platform path handling

Windows accepts forward slashes in paths, so you can write `dir + "/file.txt"` on both platforms. For canonical Windows-style paths, detect platform via `env_get("WINDIR")` (non-empty on Windows only) and pick `\\` vs `/`.

For temp directories:

```kryos
fn temp_dir() -> str {
    let t: str = env_get("TEMP")
    if len(t) > 0 { return t }
    let t2: str = env_get("TMP")
    if len(t2) > 0 { return t2 }
    let t3: str = env_get("TMPDIR")
    if len(t3) > 0 { return t3 }
    return "/tmp"
}
```

`examples/showcase/kvdb.kry` and `examples/showcase/ssg.kry` use this pattern.

## Project structure

A Kryos project is a directory with `kryos.toml`:

```toml
[package]
name = "myproject"
version = "0.1.0"

[dependencies]
http-router = "1.0"
```

Source lives in `src/`. Entry point is `src/main.kry`. Run with `kryos run` (from the project root) or `kryos build --release`.

Package registry: `NORTHTEKDevs/kryos-registry` on GitHub. Index entries carry `sha256:<hex>` checksums; tarballs are pinned by hash.

## Tooling

| Command                       | Purpose                                                        |
| ----------------------------- | -------------------------------------------------------------- |
| `kryos run <file-or-project>` | Compile + JIT-execute via Cranelift.                           |
| `kryos build --release`       | AOT via LLVM. Outputs native binary.                           |
| `kryos build -g`              | Add debug info (DWARF on Unix, CodeView `.pdb` on Windows).    |
| `kryos check`                 | Type-check, do not codegen. Fast.                              |
| `kryos fmt`                   | Auto-format `.kry` files in place.                             |
| `kryos test`                  | Run `tests/` directory.                                        |
| `kryos repl`                  | Interactive shell.                                             |
| `kryos doc`                   | Generate HTML from `///` doc comments.                         |
| `kryos pkg add <name>`        | Resolve and install a registry package.                        |
| `kryos lsp`                   | Language server (stdio).                                       |
| `kryos explain <code>`        | rustc-style long-form error explanation. e.g. `kryos explain E0300`. |
| `kryos bindgen <header.h>`    | Generate Kryos `extern` declarations from a C header.          |

## Common error codes

- `E0101` — unknown type. Did you misspell `i64` / `str` / etc.?
- `E0102` — undefined variable. Likely typo or missing `use` import.
- `E0300` — use of moved value. Clone, restructure, or return ownership.
- `E0100` — type mismatch. Expected one type, found another.
- Capability violations currently use `E-CAP-*` codes (e.g. `E-CAP-IMPORT`); unsafe-block misuse is `E0500`. (There is no `E0382`/`E0501` despite older docs.)

`kryos explain <code>` gives the full version with examples.

## Gotchas Claude needs to know

1. **String interpolation works:** `"hello {name}"` (braces, not `${}`). `+` concatenation also works; numbers need `to_string()` when concatenated.
2. **`if let` and `while let` work.** e.g. `if let Foo.Bar(x) = v { ... } else { ... }` and `while let Foo.Bar(v) = next() { ... }`; they desugar to `match`. (`let ... else` is not yet supported — use `match` or `if let` + early return.)
3. **Both `elif` and `else if` work** (`else if` is accepted as an alias). The self-host source uses `elif` by convention.
4. **No `null`.** Use `Option<T>` from `std::option`.
5. **Tuple destructuring `let (a, b) = ...` works on both backends** (`kryos run` Cranelift JIT and `kryos build --release` LLVM). The earlier Cranelift miscompile (returned 0) was fixed by inferring the tuple element types in MIR lowering.
6. **Array indexing is `[i]`, but indexes are `i64` only.** Cast `usize`-ish values explicitly if you have them.
7. **`file_write` doesn't create parent directories.** Call `create_dir(parent)` first.
8. **Top-level `let mut x = some_call()`** is allowed for pure builtins (`env_get`, `args`) but not for arbitrary user functions — move that into `main`.
9. **Glob imports `use std::os::*` work** — they import all public symbols of the module (equivalent to a bare `use std::os`).
10. **The default `kryos run` uses Cranelift, which does not support every codegen path the LLVM backend does** — if `run` fails but `build --release` works, that's the gap. Prefer `build --release` when you hit one.

## When in doubt

- Read `docs/19-language-reference.md` — that's the spec.
- Look at `examples/` for working code: `hello`, `fibonacci`, `word_count`, `shapes`, `calculator`, plus `examples/showcase/` for fuller apps (HTTP server, kvdb, ssg, agent, markdown renderer, bytecode VM).
- Run `kryos check <file>` for fast type-check feedback without compiling.
- Run `kryos explain <code>` for any error code in the diagnostics.

## When generating Kryos code

- **Don't use semicolons.** This is the #1 mistake.
- **Don't use `else if`.** Use `elif`.
- **Don't fabricate stdlib functions.** If you're not sure a function exists, write a thin wrapper around the builtins in the table above.
- **Don't claim async/await works without checking.** It exists in the grammar but has caveats — read `docs/09-concurrency.md` first.
- **Test what you wrote.** `kryos run` is fast; use it. If the user has the toolchain installed, run the file before claiming it works.
