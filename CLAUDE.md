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

loop {                // infinite, break to exit (desugars to `while true`)
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
| `char_code(s) -> i64`               | First byte of `s` as integer.                  |
| `contains(haystack, needle)`        | Substring search.                              |
| `sqrt`, `pow`, `sin`, `cos`         | Math (also in `std::math`).                    |

## Standard library

Imported with `use std::<module>::{symbol1, symbol2}`. Available modules:

`agent`, `chan`, `collections`, `cost`, `crypto`, `datetime`, `db`, `ffi`, `fmt`, `fs`, `http`, `io`, `iter`, `json`, `math`, `net`, `option`, `os`, `path`, `probable`, `process`, `re`, `result`, `stream`, `string`, `sync`, `tensor`, `term`, `test`, `tracked`, `wasm`.

Line splitting is `use std::string::{split_lines}` (`split_lines(s) -> [str]`, handles `\n` and `\r\n`) — it is a `std::string` function, not a global builtin, so it needs the import.

```kryos
use std::json::{json_stringify, json_object, json_string}

fn main() {
    // abs / min / max are polymorphic builtins — call them WITHOUT importing.
    println(to_string(abs(-42)))     // works on i64 and f64
    println(to_string(min(3, 8)))
}
```

> Note: `abs`, `min`, and `max` are polymorphic builtins (i64 or f64) available without any import. `use std::math::{abs, min, max}` imports **f64-only** versions that shadow the builtins, so `abs(-42)` would then fail to type-check — don't import them unless you specifically want the f64 form. `sqrt`, `pow`, `sin`, `cos` are likewise builtins.

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
- Capability violations use `E0501`-`E0507` (import/missing/attenuation/escalation/builtin/FFI/propagation); unsafe-block misuse is `E0500`. Ownership: moved value `E0300`, uninitialized `E0301`, immutable assign `E0302`, partial move `E0303`, conditional-move warning `W0300`. (`E0382` is a Rust code, not Kryos.) All are explainable via `kryos explain <code>`.

`kryos explain <code>` gives the full version with examples.

## Gotchas Claude needs to know

1. **String interpolation works:** `"hello {name}"` (braces, not `${}`). `+` concatenation also works; numbers need `to_string()` when concatenated.
2. **`if let`, `while let`, and `let ... else` all work.** e.g. `if let Foo.Bar(x) = v { ... } else { ... }`, `while let Foo.Bar(v) = next() { ... }`, and `let Foo.Bar(x) = v else { return }`. They desugar to `match`. For `let ... else`, the binding pattern must be a refutable enum pattern (`Enum.Variant(..)`); the `else` block runs on a non-match and its bindings are in scope for the rest of the enclosing block.
3. **Both `elif` and `else if` work** (`else if` is accepted as an alias). The self-host source uses `elif` by convention.
4. **No `null`.** Use `Option<T>` from `std::option`.
5. **Tuple destructuring `let (a, b) = ...` AND tuple field access `t.0` / `t.1` work on both backends** (`kryos run` Cranelift JIT and `kryos build --release` LLVM), for mixed element types too (e.g. `(i64, str, bool)`). The earlier Cranelift destructure miscompile (returned 0) was fixed by inferring tuple element types in MIR lowering.
6. **Array indexing `arr[i]` accepts any integer index** (i8..i64, u8..u64) on the native backends — an `i32` index works without an explicit `as i64` cast (the type checker accepts it and both Cranelift and LLVM sign-extend). The experimental `--backend wasm` (v0.1) still assumes i64; use i64 indices there.
7. **`file_write` doesn't create parent directories.** Call `create_dir(parent)` first.
8. **Top-level `let mut x = some_call()`** is allowed for pure builtins (`env_get`, `args`) but not for arbitrary user functions — move that into `main`.
9. **Glob imports `use std::os::*` work** — they import all public symbols of the module (equivalent to a bare `use std::os`).
10. **The default `kryos run` uses Cranelift, which does not support every codegen path the LLVM backend does** — if `run` fails but `build --release` works, that's the gap. Prefer `build --release` when you hit one.
11. **Closures use bar syntax, not arrows.** `|x| x + 1`, `|x: i64| x + 1`, or `fn(x: i64) -> i64 { ... }`. There is **no** `(x) => expr` form (`=>` is match-arm-only). Closures capture surrounding variables by value (`let n = 10; let f = |x| x + n`), can be passed as `fn(i64) -> i64` params, returned from named functions (`fn mk(n: i64) -> fn(i64) -> i64 { return |x| x + n }`), and stored in variables — all on both backends. Direct currying works too: `let make = |n| |x| x + n; let add10 = make(10); add10(5)` returns a closure from a closure on both backends.
12. **`?` operator works on both backends** (Cranelift JIT and LLVM AOT). `let v = parse(s)?` returns early with the `Err` on failure. Use `Result<T, E>` (e.g. `Result<i64, str>`) return types.
13. **`Result<T,E>`/`Option<T>` payloads keep their real type when matched** — `match r { Result.Err(e) => println(e) }` binds `e` as `str` (for `Result<i64,str>`) and prints it correctly; `Ok(v)`/`Some(v)` bind their real type too. The match-payload binding now uses the monomorphized enum type (e.g. `Result___i64_str`) rather than the i64-erased generic stub, so direct use (bare `println(e)`, `==`, arithmetic) dispatches on the right type on both backends. **Always annotate `Result<T,E>`/`Option<T>` on signatures** — a *bare* `Result`/`Option` (no `<...>`) still erases its payload to i64.
14. **Or-patterns work in match arms:** `match n { 1 | 2 | 3 => "low", _ => "high" }` and `match c { Red | Green => "warm", Blue => "cool" }`. Alternatives must be non-binding (literals or bare enum variants).
15. **Matching on a tuple value works:** `match p { (0, 0) => ..., (x, 7) => x, (5, _) => ..., _ => ... }` — literal elements are compared, ident elements bind the field, and `_` elements match anything (on both backends). String-literal elements (`("ok", n) => n`) work too.
16. **Struct-style enum variants are not supported.** `enum E { A { x: i64 } }` is rejected with a clear error — use a tuple variant `A(i64)` and match `A(x)`.
17. **`impl` methods on generic structs work.** Both `impl Wrap { fn get(self: Wrap<i64>) -> i64 {..} }` and the concrete `impl Wrap<i64> { .. }` syntax parse and run on both backends. As with field access on generic structs, payload slots are i64-sized, so a method on a non-`i64` instantiation that returns the payload directly sees the raw slot — annotate the method for the concrete instantiation you use (the common case).
18. **Integer division/modulo and bounds are checked; out-of-range float→int casts are not.** Integer `a / 0` / `a % 0` panic ("integer division by zero"), array/string out-of-bounds panic, and signed division truncates toward zero (`-7 / 2 == -3`) consistently on both backends. But casting an out-of-range `f64` to an integer (e.g. `1.0e300 as i64`) is undefined on the AOT backend (the JIT saturates) — guard the range yourself before casting if the value can exceed the target type.
19. **Closures passed to higher-order functions infer their parameter types** from the function's signature — `fold(xs, 0, |acc, x| acc + x)`, `reduce`, `scan`, `map_indexed`, `filter`, `map` all work without annotating the closure params, including when the accumulator and element types differ (`fold(strs, 0, |acc, x| acc + len(x))`). This relies on a generic `fn(T, U) -> U`-style signature; a custom HOF you write should declare one (`fn myfold<T, U>(arr: [T], init: U, f: fn(U, T) -> U) -> U`) rather than a bare `f: fn`. Substring search is `use std::string::{find}` (`find(haystack, needle) -> i64`), not `index_of`.
20. **Closures keep their real `str`/struct/array/**float** type across a HOF boundary, in both params AND return.** An un-annotated closure passed to a higher-order function carries its inferred param type into the body and its inferred return type out, so `fold(words, "", |acc, x| acc + x)` concatenates strings (not integer-adds handles), `fold(nums, 0.0, |acc, x| acc + x)` over `[f64]` sums as floats, and `map([1.0, 2.0, 3.0], |x| x * 2.0)` returns a real `[f64]` whose elements read as floats with **no binding annotation** needed (the uniform i64 closure ABI bit-casts floats in/out via the env-thunk; only the static type flows so a generic `[U]` result binds `U=f64`). Works on both backends.
21. **AOT struct fields are 8-byte slots; a tuple/enum field followed by other fields is unsafe on `build --release`.** The LLVM backend addresses struct fields with an i64-stride GEP (every field assumed 8 bytes), so a multi-element tuple/enum field (>8 bytes) throws off the byte offset of any field declared AFTER it: mutating a trailing field (`r.after = x`) **silently writes to the wrong offset on AOT** (a real miscompile; `kryos run`/JIT is correct). Enum-typed fields are additionally flattened to a bare `i64` slot, so extracting one and using it as the enum fails to compile on AOT. **Workarounds:** declare tuple/enum fields LAST in the struct, or use `kryos run`. (UPDATE: the trailing-field-mutation miscompile is FIXED — StoreField now uses a struct-indexed GEP; the enum-field-as-aggregate read also works now. This gotcha is mostly historical.)
22. **Known remaining limitations (use `kryos run`/JIT or the workaround on AOT) — the shrinking bug tail:**
    - **Untyped array of aggregates: RESOLVED.** `let mut a = []` followed by `push(a, Struct{..})` (or `a = push(a, ...)`) now infers `a: [Struct]` — `a[i].field` and `let last = pop(a); last.field` work on both backends with no annotation. The type checker gives `push`/`pop` real generic signatures and resolves the element type from later pushes; the resolved type is threaded into MIR so both backends agree, and `pop` is element-typed (AOT unboxes aggregate elements, reinterprets float/ptr slots).
    - **Narrow-int literal assignment: RESOLVED.** `let x: u8 = 200`, u8 fn params/returns, `[u8]` array literals, and u8 struct-field inits all accept plain int literals on both backends, and `to_string`/`println` of unsigned values zero-extends correctly (u8 200 prints `200`, not `-56`). Note: like the signed types, range is not checked at compile time — an out-of-range literal truncates.
    - **Nested struct field mutation: RESOLVED.** `o.a.v = 99` (any depth, e.g. `a.b.c.z = 77`) works on both backends — MIR lowers it as read-modify-writeback so the mutation propagates through intermediate copies. Storing a str literal into a struct field on AOT also works now (the slot store coerces ptr/double/i1 SSA values). Remaining edge: mutating a field of a struct-typed *function parameter* (`fn f(o: Outer) { o.a.v = 5 }`) still fails on AOT — copy the param into a `let mut` local first.
    - **`Option`/`Result` with a multi-field struct payload: RESOLVED.** `Option<User>` / `Result<User, str>` where `User` has any number of fields (mixed i64/str/f64) now type-check and run correctly on both backends — `match`, `if let`, `Some`/`Ok`/`Err`/`None` paths, and direct field access on the bound payload all work. Tuple payloads (`Option<(i64, str)>`, `Result<(i64, str), str>`) work too.

## When in doubt

- Read `docs/19-language-reference.md` — that's the spec.
- Look at `examples/` for working code: `hello`, `fibonacci`, `word_count`, `shapes`, `calculator`, plus `examples/showcase/` for fuller apps (HTTP server, kvdb, ssg, agent, markdown renderer, bytecode VM).
- Run `kryos check <file>` for fast type-check feedback without compiling.
- Run `kryos explain <code>` for any error code in the diagnostics.

## When generating Kryos code

- **Don't use semicolons.** This is the #1 mistake.
- **Don't use `else if`.** Use `elif`.
- **Closures are `|x|` / `|x: i64|`, never `(x) => ...`.** Don't directly nest lambda literals (`|n| |x| ...`); use a named outer function.
- **Annotate `Result<T, E>` and `Option<T>` on function signatures** so non-`i64` payloads keep their type when used directly.
- **Don't use struct-style enum variants** (`A { x: i64 }`); use tuple variants (`A(i64)`).
- **Don't fabricate stdlib functions.** If you're not sure a function exists, write a thin wrapper around the builtins in the table above.
- **Don't claim async/await works without checking.** It exists in the grammar but has caveats — read `docs/09-concurrency.md` first.
- **Test what you wrote.** `kryos run` is fast; use it. If the user has the toolchain installed, run the file before claiming it works.
