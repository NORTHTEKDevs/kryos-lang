# CLAUDE.md

This file gives Claude (Code, Computer, or any other Anthropic-driven tool) the context it needs to write correct, idiomatic Kryos. Drop it into any project that uses Kryos and Claude will read it automatically.

If you're reading this as Claude: **everything below is authoritative for the toolchain at this commit.** The full reference is in `docs/19-language-reference.md`; this file is the working subset you need to be productive without trial-and-error.

## What Kryos is

A capability-safe, ownership-aware systems language with two backends:

- **Cranelift** (`kryos run`) — debug JIT, fast compile, no external linker
- **LLVM** (`kryos build --release`) — release AOT, full optimization, links via `cc`/`clang`/`link.exe`

Targets Linux x86_64, Windows x86_64 MSVC, macOS x86_64/aarch64, and `wasm32-unknown-unknown` (JS host contract — browser or `node tools/wasm-host/run.mjs`; WASI is not supported). The same `.kry` source runs on every target — the only platform-conditional code you should write is filesystem-path handling.

## Hard rules (these cause compile errors)

1. **No semicolons.** Line breaks terminate statements. To wrap a long expression, end the line with `(`, `[`, `{`, `,`, or a binary operator.
2. **Block comments `/* ... */` work and nest.** Line comments are `//`, doc comments `///`. (The self-host compiler source under `compiler/self-host/` avoids block comments by convention, but the language supports them.)
3. **No `null` / `nil` keyword.** Use `Option<T>` from `std::option` for nullable values.
4. **String interpolation works in EVERY string literal:** `"hello {name}"` (delimiter is `{ }`, NOT `${ }`). Unlike Python/Rust where only an `f"..."`/`format!` string interpolates, **ALL Kryos strings interpolate**, so a bare `{` in any string opens an interpolation. To put a **literal brace** in a string you MUST double it (`{{`, `}}`) or backslash-escape it (`\{`, `\}`). This bites JSON, code-gen, and set notation: `"{\"a\":1}"` FAILS to parse (the `{` opens an interpolation) — write `"{{\"a\":1}}"`, or build the string with `+` (`"{" + "\"a\":1" + "}"`). `+` concatenation also works generally; cast numerics with `to_string(x)` when concatenating.
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
| `i64`       | Default integer. Overflow wraps modulo 2^64 on both backends.      |
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

<!-- docs-example: skip -->
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

## Value semantics (ARC-backed — reuse-after-pass is safe)

`str`, `[T]`, `map<K, V>`, and structs/enums are reference-counted heap handles; primitives (`i64`, `f64`, `bool`) are `Copy`. Passing a value to a function **shares** the underlying data (a cheap refcount bump), so **reusing the original variable afterward is safe and allowed** — there is no destructive move, and you do **not** need `.clone()` to keep using a value after passing it:

```kryos
fn main() {
    let s: str = "hello"
    consume(s)
    println(s)   // OK — `s` is still valid; the data is shared, not consumed
}

fn consume(x: str) {
    println(x)
}
```

The compiler runs an advisory ownership/borrow analysis and may surface move/borrow *diagnostics*, but it does **not** block reuse of ARC-backed values — programs that pass then reuse a value compile and run correctly on both backends. (This is deliberate: the self-host compiler threads shared handles through its passes under this model.)

For an **independent** copy (so a later mutation of one does not affect the other), copy into a `let mut` local and mutate that — see gotcha #23 for the exact copy/mutation semantics. `.clone()` is not currently a method; assignment (`let b = a`) already deep-copies heap fields per gotcha #23.

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

`throw "message"` raises an exception that unwinds to the nearest enclosing `try`/`catch`. The thrown value is stringified at the throw site, so the catch variable is always a `str`. If nothing catches it, the program prints `kryos: uncaught exception: <msg>` to stderr and exits with code 101 (both backends).

## Capabilities (Kryos's defining feature)

Every function has a capability set inferred from the builtins it calls. If your function calls `file_write`, it needs `fs:write`. If you `spawn`/`exec` a process **or read the environment** (`env_get`/`env_set` — reading env can exfiltrate secrets), you need `process`. Network needs `net:http` / `net:tcp`. `crypto` for `sha256`/`hmac`. `exit`/`abort`, clock reads, and `sleep` are ambient (not gated). The compiler tracks these through the call graph.

Three enforcement modes, via `--capabilities-mode=<mode>` or `[capabilities] mode` in `kryos.toml`:
- **`inferred`** (THE DEFAULT for `kryos run`, `check`, and `build`): deny-by-default at the boundary. Declare `@capabilities(...)` on `main` (and any `pub` fn); interior helpers are inferred. An unannotated `main` that transitively uses a gated builtin is rejected — the error names the exact set to add.
- **`strict`** (`--strict-capabilities`): every function must declare its own caps.
- **`permissive`**: only annotated functions are checked. Opt in per-invocation with `--capabilities-mode=permissive` (works on `run` too) for scratch files.

When writing Kryos for a `kryos new` project, put `@capabilities(...)` on `main` listing what the program needs; leave helpers unannotated. See `docs/10-capabilities.md` for the full model.

**Extern calls are capability-gated too (E0506):** calling a function declared in an `extern { }` block requires the capability of the builtin it backs when the name is `kryos_*` (e.g. `kryos_env_get` needs `process`), and `ffi` for any other extern name (C libraries). Declaring the extern is free; calling it demands authority — there is no FFI bypass around deny-by-default.

### Sub-capabilities (least-privilege)

Capabilities come in coarse families and finer **sub-capabilities** written `family:scope`:

- `net:http` (HTTP[S] clients/servers) and `net:tcp` (raw TCP/TLS/unix sockets) under coarse `net`.
- `fs:read` (read/stat) and `fs:write` (write/create/mutate) under coarse `io`/`fs` (both spellings mean the same coarse filesystem cap; `io` is the legacy name).

**Coarse grants all its sub-caps** (back-compat): a function declaring `@capabilities(net)` may call `http_get` (needs `net:http`) and `tcp_connect` (needs `net:tcp`); `@capabilities(io)` may call both `file_read` and `file_write`; `all` grants everything. The reverse does NOT hold: a function declaring only `@capabilities(fs:read)` that calls `file_write` is **rejected** under `--strict-capabilities` (`error[E0505]: builtin \`file_write\` requires \`fs:write\` capability`) — declare `fs:write` (or coarse `io`) instead. Likewise `net:http` does not grant `net:tcp`. Declaring the precise sub-cap is the least-privilege default; declaring the coarse family is the broad escape hatch. Builtin → required cap: `file_read`/`list_dir`/`path_exists` → `fs:read`; `file_write`/`create_dir`/`remove_file` → `fs:write`; `http_get`/`http_post`/`http2_*` → `net:http`; `tcp_*`/`tls_*`/`uds_*` → `net:tcp`. NOTE: `file_read`/`file_write`/`env_get` and the raw `tcp_*`/`tls_*`/`uds_*` functions are true global builtins (no import; `use std::net::{tcp_listen}` is an error -- std::net does not export them, it wraps them in `TcpListener`/`TcpStream`). The higher-level HTTP helpers (`http_get`, `http_post`, ...) live in `std::net` and are imported: `use std::net::{http_get}`. The cap a function requires is the same whether or not you import it.

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
| `file_read(path: str) -> str`       | Read entire file as UTF-8. PANICS if missing/unreadable — probe with `file_exists` or use `std::fs::read_file` (throws). |
| `file_write(path, content)`         | Write file. Does NOT create parent dirs.       |
| `file_exists(path) -> i64`          | `1` if exists, `0` otherwise.                  |
| `create_dir(path)`                  | Create directory (idempotent).                 |
| `push(arr, item) -> [T]`            | Return new array with item appended.           |
| `substr(s, start, end) -> str`      | Byte-indexed substring.                        |
| `char_code(s) -> i64`               | First byte of `s` as integer.                  |
| `contains(haystack, needle)`        | Substring search.                              |
| `sqrt`, `pow`, `sin`, `cos`         | Math (also in `std::math`).                    |

## Standard library

Imported with `use std::<module>::{symbol1, symbol2}`. The stdlib ships 66 `.kry` modules. Commonly used ones:

`agent`, `agent_bridge`, `backoff`, `bloom`, `bytes`, `chan`, `circuit`, `collections`, `cost`, `crypto`, `csv`, `datetime`, `db`, `deque`, `diff_ops`, `duration`, `ffi`, `fmt`, `fs`, `fuzzy`, `hash`, `heap`, `histogram`, `http`, `interval`, `io`, `iter`, `json`, `jwt`, `llm`, `log`, `lru`, `math`, `mathx`, `matrix`, `net`, `numfmt`, `option`, `os`, `path`, `pathext`, `probable`, `process`, `queue`, `random`, `ratelimit`, `re`, `result`, `semaphore`, `semver`, `set`, `slice_ops`, `smtp`, `stack`, `stat`, `stream`, `strext`, `string`, `sync`, `tensor`, `term`, `test`, `tracked`, `trie`, `utf8`, `wasm`.

Line splitting is `use std::string::{split_lines}` (`split_lines(s) -> [str]`, handles `\n` and `\r\n`) — it is a `std::string` function, not a global builtin, so it needs the import.

```kryos
use std::json::{stringify, json_object, json_string}

fn main() {
    // abs / min / max are polymorphic builtins — call them WITHOUT importing.
    println(to_string(abs(-42)))     // works on i64 and f64
    println(to_string(min(3, 8)))
    println(stringify(json_string("hi")))   // prints "hi" (JSON-quoted)
}
```

> **Import namespace gotcha:** imports share ONE flat namespace and there is **no import aliasing** (`use m::{parse as p}` is a parse error). Two modules exporting the same name (`std::csv::parse` vs `std::json::parse`) cannot both be imported; the compiler errors at the import, and a module-qualified call (`json::parse(..)`) is only sugar for the flat name -- the compiler validates it against the import's ORIGIN and errors if it came from a different module. Resolve collisions by importing disjoint names selectively. **Actors are constructed with `Name()`** (state is private, zero-initialized) -- the struct-literal form `Name { field: v }` is rejected. **`json_object(keys: [str], values: [JsonValue])` takes two arrays**, not zero args.

> `std::json` gotcha: the JsonValue serializer is **`stringify`** (and `parse`, `pretty_print`, `get`, `set`, `to_str`...), NOT `json_stringify`. The `json_*` names in the module are only the **constructors** (`json_string`, `json_number`, `json_object`, `json_array`, `json_bool`, `json_null`). A separate native handle-based builtin named `json_stringify` exists in the runtime with a DIFFERENT (i64-handle) signature — importing/mixing it with JsonValue values fails with a type mismatch. Use the `std::json` module API end-to-end.

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
| `kryos explain <code>`        | rustc-style long-form error explanation. e.g. `kryos explain E0300`. 32 codes documented. |
| `kryos dap`                   | Start the source-level debugger (Debug Adapter Protocol, stdio). |
| `kryos audit [path]`          | Audit capability usage, extern surface, and secret patterns.   |
| `kryos bindgen <header.h>`    | Generate Kryos `extern` declarations from a C header.          |

## Common error codes

- `E0101` — unknown type. Did you misspell `i64` / `str` / etc.?
- `E0102` — undefined variable. Likely typo or missing `use` import.
- `E0300` — advisory move diagnostic. ARC-backed values (`str`, `[T]`, `map`, structs) may be reused after being passed; this is a lint, not a hard error, and does not block compilation of reuse-after-pass.
- `E0100` — type mismatch. Expected one type, found another.
- Capability violations use `E0501`-`E0507` (import/missing/attenuation/escalation/builtin/FFI/propagation); unsafe-block misuse is `E0500`. Ownership: moved value `E0300`, uninitialized `E0301`, immutable assign `E0302`, partial move `E0303`, conditional-move warning `W0300`. (`E0382` is a Rust code, not Kryos.) All are explainable via `kryos explain <code>`.

`kryos explain <code>` gives the full version with examples.

## Gotchas Claude needs to know

1. **String interpolation works in EVERY string:** `"hello {name}"` (braces, not `${}`). ALL strings interpolate, so a literal brace needs doubling/escaping: `{{` `}}` or `\{` `\}`. JSON/code-gen: `"{\"a\":1}"` FAILS; use `"{{\"a\":1}}"` or build with `+`. `+` concatenation also works; numbers need `to_string()` when concatenated. (Map key membership is `contains(m, k)` for str- AND int-keyed maps.)
2. **`if let`, `while let`, and `let ... else` all work.** e.g. `if let Foo.Bar(x) = v { ... } else { ... }`, `while let Foo.Bar(v) = next() { ... }`, and `let Foo.Bar(x) = v else { return }`. They desugar to `match`. For `let ... else`, the binding pattern must be a refutable enum pattern (`Enum.Variant(..)`); the `else` block runs on a non-match and its bindings are in scope for the rest of the enclosing block.
3. **Both `elif` and `else if` work** (`else if` is accepted as an alias). The self-host source uses `elif` by convention. **A trailing `if ... else ...` is a block's VALUE**: `let x = { if c { a } else { b } }` and a match arm `x => { if ... }` yield the `if`'s value (elif chains included), same as a bare `let x = if c { a } else { b }`. Block-locals bound to runtime strings and used in a compound tail — `{ let inner = to_string(x) + "y"; inner + "!" }` — now work on both backends (the str-temp double-free was fixed).
4. **No `null`.** Use `Option<T>` from `std::option`.
5. **Tuple destructuring `let (a, b) = ...` AND tuple field access `t.0` / `t.1` work on both backends** (`kryos run` Cranelift JIT and `kryos build --release` LLVM), for mixed element types too (e.g. `(i64, str, bool)`). The earlier Cranelift destructure miscompile (returned 0) was fixed by inferring tuple element types in MIR lowering.
6. **Array indexing `arr[i]` accepts any integer index** (i8..i64, u8..u64) on the native backends — an `i32` index works without an explicit `as i64` cast (the type checker accepts it and both Cranelift and LLVM sign-extend). The experimental `--backend wasm` (v0.1) still assumes i64; use i64 indices there.
7. **`file_write` doesn't create parent directories.** Call `create_dir(parent)` first.
8. **Top-level `let mut x = some_call()`** is allowed for pure builtins (`env_get`, `args`) but not for arbitrary user functions — move that into `main`.
9. **Glob imports `use std::os::*` work** — they import all public symbols of the module (equivalent to a bare `use std::os`).
10. **The default `kryos run` uses Cranelift, which does not support every codegen path the LLVM backend does** — if `run` fails but `build --release` works, that's the gap. Prefer `build --release` when you hit one.
11. **Closures use bar syntax, not arrows.** `|x| x + 1`, `|x: i64| x + 1`, or `fn(x: i64) -> i64 { ... }`. There is **no** `(x) => expr` form (`=>` is match-arm-only). Closures capture surrounding variables by **reference** when the captured binding is not mutated inside the closure body (mutations to the variable after the closure is created ARE visible to it); when the closure mutates the captured variable, it captures by move. Closures can be passed as `fn(i64) -> i64` params, returned from named functions (`fn mk(n: i64) -> fn(i64) -> i64 { return |x| x + n }`), and stored in variables — all on both backends. Direct currying (nested lambda literals) works: `let make = |n| |x| x + n; let add10 = make(10); add10(5)` returns 15 on both backends. **Caveat (escaping closures + heap captures):** the "sees later mutations" rule holds for a closure called directly in its defining scope, and for scalar (`i64`/`f64`/`bool`) captures in ALL cases. But a closure STORED in a struct field or array element (an escaping closure) SNAPSHOTS a *heap* capture (`[T]`/`map`/`str`) at storage time -- if you `push` a closure that reads `arr`, then reassign `arr = push(arr, ..)`, the stored closure still sees the old (pre-store) array. Both backends agree (consistent value-capture, not a JIT/AOT divergence). Scalar captures in stored closures stay live. Workaround: call the closure directly, or pass the captured heap data in as an argument instead of capturing it. The SAME root affects a **self-referential closure built via reassignment**: `let mut fact = |x| 1; fact = |n| if n <= 1 { 1 } else { n * fact(n - 1) }` captures the OLD `fact` (the placeholder), so `fact(5)` returns `5`, not `120` (both backends agree -- the fn-typed capture is snapshotted, not a live slot reference). Workaround: use a NAMED recursive function (`fn fact(n: i64) -> i64 { if n <= 1 { return 1 } return n * fact(n - 1) }`) -- named-fn recursion is fully supported on both backends.
12. **`?` operator works on both backends** (Cranelift JIT and LLVM AOT). `let v = parse(s)?` returns early with the `Err` on failure. Use `Result<T, E>` (e.g. `Result<i64, str>`) return types.
13. **`Result<T,E>`/`Option<T>` payloads keep their real type when matched** — `match r { Result.Err(e) => println(e) }` binds `e` as `str` (for `Result<i64,str>`) and prints it correctly; `Ok(v)`/`Some(v)` bind their real type too. The match-payload binding now uses the monomorphized enum type (e.g. `Result___i64_str`) rather than the i64-erased generic stub, so direct use (bare `println(e)`, `==`, arithmetic) dispatches on the right type on both backends. **Always annotate `Result<T,E>`/`Option<T>` on signatures** — a *bare* `Result`/`Option` (no `<...>`) still erases its payload to i64.
14. **Or-patterns work in match arms:** `match n { 1 | 2 | 3 => "low", _ => "high" }` and `match c { Red | Green => "warm", Blue => "cool" }`. Alternatives must be non-binding (literals or bare enum variants).
15. **Matching on a tuple value works:** `match p { (0, 0) => ..., (x, 7) => x, (5, _) => ..., _ => ... }` — literal elements are compared, ident elements bind the field, and `_` elements match anything (on both backends). String-literal elements (`("ok", n) => n`) work too.
16. **Struct-style enum variants are not supported.** `enum E { A { x: i64 } }` is rejected with a clear error — use a tuple variant `A(i64)` and match `A(x)`. **Three variant-reference spellings all work on both backends and lower identically:** bare `Some(7)`, dotted `Opt.Some(7)`, and Rust-style `Opt::Some(7)` — in construction, as a function argument, in array literals, in `match` arms, and nested. A *nullary* variant used in value position must be qualified (`Opt::None` or `Opt.None`); bare unqualified `None`/`Red` in an expression is rejected (`E0102`) to avoid cross-enum ambiguity (bare is still fine in `match` patterns, where the matched type disambiguates).
17. **`impl` methods on generic structs work, including parametric `impl<T>`.** All three forms parse and run on both backends: `impl Wrap { fn get(self: Wrap<i64>) -> i64 {..} }`, the concrete `impl Wrap<i64> { .. }`, AND the parametric `impl<T> Wrap<T> { fn get(self: Wrap<T>) -> T {..} }`. **Using one generic method at multiple concrete types in the same program is fully supported** — each call instantiates fresh type vars and binds them to the receiver, so `let a: i64 = p.get(); let c: str = q.get()` (two instantiations of the same method) no longer contaminate each other. **A bare `-> T` method return resolves to the receiver's concrete type**, including `f64` — `to_string(box_of_f64.get())` prints the float, not raw i64-slot bits (fixed; annotation no longer required). Multi-parameter impls (`impl<A, B> Pair<A, B>`) work too. **Chaining a method call ON the result of a generic method now resolves without annotation** — `ww.get().get()` (and an un-annotated `let x = ww.get()` intermediate) correctly infers and codegens on both backends when the inner method's return is a bare `-> T`; the explicit-annotation workaround (`let l1: Box<i64> = ww.get()`) still works too, it's just no longer required. One residual edge remains: a method whose return TYPE is a COMPOUND shape merely mentioning the parameter (`-> (T, i64)`, `-> [T]`) keeps the erased i64-slot element for non-pointer `T` (a float tuple element prints as its bit pattern) — it builds and is backend-consistent, but annotate or return the bare `T` for exact floats.
18. **Integer division/modulo, bounds, AND out-of-range float→int casts are all checked/defined.** Integer `a / 0` / `a % 0` panic ("integer division by zero"), `i64::MIN / -1` and `% -1` panic ("integer division overflow" — the quotient is unrepresentable), array/string out-of-bounds panic, and signed division truncates toward zero (`-7 / 2 == -3`) consistently on both backends. Casting an out-of-range `f64` to an integer now **saturates** on BOTH backends (`1.0e300 as i64` → `i64::MAX`, `-1.0e300 as i64` → `i64::MIN`, `NaN as i64` → `0`) — no undefined behavior, no manual range guard needed. (This was previously UB on AOT; fixed.)
19. **Closures passed to higher-order functions infer their parameter types** from the function's signature — `fold(xs, 0, |acc, x| acc + x)`, `reduce`, `scan`, `map_indexed`, `filter`, `map` all work without annotating the closure params, including when the accumulator and element types differ (`fold(strs, 0, |acc, x| acc + len(x))`). These HOFs live in `std::iter` and must be imported (`use std::iter::{fold, filter, map, reduce}`) — they are NOT global builtins (unlike `abs`/`min`/`max`/`sqrt`). This relies on a generic `fn(T, U) -> U`-style signature; a custom HOF you write should declare one (`fn myfold<T, U>(arr: [T], init: U, f: fn(U, T) -> U) -> U`) rather than a bare `f: fn`. Substring search is `use std::string::{find}` (`find(haystack, needle) -> i64`), not `index_of`.
20. **Closures keep their real `str`/struct/array/**float** type across a HOF boundary, in both params AND return.** An un-annotated closure passed to a higher-order function carries its inferred param type into the body and its inferred return type out, so `fold(words, "", |acc, x| acc + x)` concatenates strings (not integer-adds handles), `fold(nums, 0.0, |acc, x| acc + x)` over `[f64]` sums as floats, and `map([1.0, 2.0, 3.0], |x| x * 2.0)` returns a real `[f64]` whose elements read as floats with **no binding annotation** needed (the uniform i64 closure ABI bit-casts floats in/out via the env-thunk; only the static type flows so a generic `[U]` result binds `U=f64`). Works on both backends.
21. **AOT struct fields are 8-byte slots; a tuple/enum field followed by other fields is unsafe on `build --release`.** The LLVM backend addresses struct fields with an i64-stride GEP (every field assumed 8 bytes), so a multi-element tuple/enum field (>8 bytes) throws off the byte offset of any field declared AFTER it: mutating a trailing field (`r.after = x`) **silently writes to the wrong offset on AOT** (a real miscompile; `kryos run`/JIT is correct). Enum-typed fields are additionally flattened to a bare `i64` slot, so extracting one and using it as the enum fails to compile on AOT. **Workarounds:** declare tuple/enum fields LAST in the struct, or use `kryos run`. (UPDATE: the trailing-field-mutation miscompile is FIXED — StoreField now uses a struct-indexed GEP; the enum-field-as-aggregate read also works now. This gotcha is mostly historical.)
22. **Known remaining limitations (use `kryos run`/JIT or the workaround on AOT) — the shrinking bug tail:**
    - **`dyn Trait` in an ARRAY is NOT supported (avoid — it CRASHES).** A single `dyn Trait` value or parameter works and dispatches correctly on both backends (`fn describe(s: dyn Shape) -> f64 { return s.area() }`). But an ARRAY of trait objects is unimplemented: `push` onto a `[dyn Shape]` SEGFAULTS on both backends (the element store assumes a scalar slot, not a vtable fat pointer), and a heterogeneous `[dyn Shape]` array literal (`[Circle{..}, Square{..}]`) is rejected at type-check (element inference is bottom-up against element 1's concrete type, with no propagation of the declared `[dyn Shape]`). Workaround: use an enum with a variant per concrete type and `match`, or keep separate typed arrays. (Full trait-object array storage is backlogged.)
    - **Untyped array of aggregates: RESOLVED.** `let mut a = []` followed by `push(a, Struct{..})` (or `a = push(a, ...)`) now infers `a: [Struct]` — `a[i].field` and `let last = pop(a); last.field` work on both backends with no annotation. The type checker gives `push`/`pop` real generic signatures and resolves the element type from later pushes; the resolved type is threaded into MIR so both backends agree, and `pop` is element-typed (AOT unboxes aggregate elements, reinterprets float/ptr slots).
    - **Narrow-int literal assignment: RESOLVED.** `let x: u8 = 200`, u8 fn params/returns, `[u8]` array literals, and u8 struct-field inits all accept plain int literals on both backends, and `to_string`/`println` of unsigned values zero-extends correctly (u8 200 prints `200`, not `-56`). Note: like the signed types, range is not checked at compile time — an out-of-range literal truncates.
    - **Nested struct field mutation: RESOLVED.** `o.a.v = 99` (any depth, e.g. `a.b.c.z = 77`) works on both backends — MIR lowers it as read-modify-writeback so the mutation propagates through intermediate copies. Storing a str literal into a struct field on AOT also works now (the slot store coerces ptr/double/i1 SSA values). Mutating fields of a struct-typed *function parameter* compiles on both backends now too — but see gotcha #23 for a semantic caveat.
    - **`Option`/`Result` with a multi-field struct payload: RESOLVED.** `Option<User>` / `Result<User, str>` where `User` has any number of fields (mixed i64/str/f64) now type-check and run correctly on both backends — `match`, `if let`, `Some`/`Ok`/`Err`/`None` paths, and direct field access on the bound payload all work. Tuple payloads (`Option<(i64, str)>`, `Result<(i64, str), str>`) work too.
23. **Struct param/copy semantics across backends (PARTIALLY RESOLVED).** **All-scalar `@copy` struct params** (no str/array/map/struct/fn fields) are now copied at function entry on the JIT, so a field mutation inside the callee no longer aliases the caller's value — both backends agree on pass-by-value for plain-data structs (regression test: `tests/smoke/test_copy_param_value_semantics.kry`). Residual divergences:
    - **Heap-bearing `@copy` structs as params:** the JIT still passes the caller's pointer (aliasing) while AOT passes a shallow byval copy. The entry copy is deliberately NOT applied here: the self-host parser threads `Parser { tokens: [Token], .. }` through every `p_*` call under the share-on-clone model, and copying it at entry clones the token array per call (deep copy OOMs stage-1; even a shallow copy is an allocation on the hottest path).
    - **Non-`@copy` structs:** JIT aliases, AOT copies. The ownership move rules make this unobservable in legal programs, and the self-host compiler relies on the JIT aliasing internally (lower.kry ctx fns), so this stays until those call sites are rewritten.
    - **Heap-field content on copies: UNIFIED (step 224).** `@copy` ASSIGNMENT (`let c = b`) deep-clones array/str/map fields on BOTH backends now — "each copy owns its data" (regression test: `tests/smoke/test_copy_assign_deep.kry`). The LLVM backend gained the same per-field clone the Cranelift backend always had; clones leak under the no-op `@copy` drop model on both backends (leak-on-copy, consistent).

    **Portable pattern (unchanged): copy the param into a `let mut` local before mutating** (`let mut local = o`), or return the modified struct; never rely on cross-call visibility of heap-field mutations.

## When in doubt

- Read `docs/19-language-reference.md` — that's the spec.
- Look at `examples/` for working code: `hello`, `fibonacci`, `word_count`, `shapes`, `calculator`, plus `examples/showcase/` for fuller apps (HTTP server, kvdb, ssg, agent, markdown renderer, bytecode VM).
- Run `kryos check <file>` for fast type-check feedback without compiling.
- Run `kryos explain <code>` for any error code in the diagnostics.

## When generating Kryos code

- **Don't use semicolons.** This is the #1 mistake.
- **Don't use `else if`.** Use `elif`.
- **Closures are `|x|` / `|x: i64|`, never `(x) => ...`.** Nested lambda literals (currying via `|n| |x| x + n`) work on both backends.
- **Annotate `Result<T, E>` and `Option<T>` on function signatures** so non-`i64` payloads keep their type when used directly.
- **Don't use struct-style enum variants** (`A { x: i64 }`); use tuple variants (`A(i64)`).
- **Don't fabricate stdlib functions.** If you're not sure a function exists, write a thin wrapper around the builtins in the table above.
- **Don't claim async/await works without checking.** It exists in the grammar but has caveats — read `docs/09-concurrency.md` first.
- **Test what you wrote.** `kryos run` is fast; use it. If the user has the toolchain installed, run the file before claiming it works.
