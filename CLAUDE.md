# CLAUDE.md

This file gives Claude (Code, Computer, or any other Anthropic-driven tool) the context it needs to write correct, idiomatic Kryos. Drop it into any project that uses Kryos and Claude will read it automatically.

If you're reading this as Claude: **everything below is authoritative for the toolchain at this commit.** The full reference is in `docs/19-language-reference.md`; this file is the working subset you need to be productive without trial-and-error.

## What Kryos is

A capability-safe, ownership-aware systems language with two backends:

- **Cranelift** (`kryos run`) — debug JIT, fast compile, no external linker
- **LLVM** (`kryos build --release`) — release AOT, full optimization, links via `cc`/`clang`/`link.exe`

Targets Linux x86_64, Windows x86_64 MSVC, macOS x86_64/aarch64, and `wasm32-unknown-unknown` (JS host contract — browser or `node tools/wasm-host/run.mjs`; WASI is not supported). The same `.kry` source runs on every target — the only platform-conditional code you should write is filesystem-path handling.

## Hard rules (these cause compile errors)

1. **No semicolons.** Line breaks terminate statements. To wrap a long expression, end the line with `(`, `[`, `{`, `,`, or a binary operator. **The converse trap: a NEW line that STARTS with `-`, `(`, or `[` CONTINUES the previous expression** (JS-ASI-class grammar): `let a = 5` followed by a line `-1` parses as `let a = 5 - 1` (a is 4, silently); `println("hi")` followed by `(x, y)` parses as a call continuation; and `let x = arr` followed by `[0]` parses as `let x = arr[0]` (indexing). Never begin a statement line with unary `-`, a parenthesized expression, or a `[`-literal — bind it (`let n = -1`) or restructure.
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
| `push(arr, item) -> [T]`            | Append in place, return the array handle. See the aliasing note below -- always write `arr = push(arr, item)`. |
| `sort(arr)` / `reverse(arr)`        | Sort / reverse an array IN PLACE (void). Array-only; write `sort(arr)` as a statement, NOT `arr = sort(arr)`. For a string use `std::string::reverse(s)`. |
| `substr(s, start, end) -> str`      | Byte-indexed substring.                        |
| `char_code(s) -> i64`               | First Unicode CODEPOINT of `s` as an integer (`char_code("é")` = 233, not the byte 195). Inverse of `char_from`. |
| `byte_at(s, i) -> i64`              | CODEPOINT of the i-th CHARACTER (not the i-th raw UTF-8 byte): `byte_at("é", 0)` = 233. For latin-1 byte buffers (codepoints 0-255, per gotcha #22) codepoint == byte; there is no raw-byte accessor for a multibyte string. |
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
| `kryos run <file-or-project>` | Compile + execute via Cranelift (AOT + subprocess, not an in-process JIT). |
| `kryos build --release`       | AOT via LLVM. Outputs native binary.                           |
| `kryos build -g`              | Add debug info (DWARF on Unix, CodeView `.pdb` on Windows) AND panic stack traces matching `kryos run`. |
| `kryos check`                 | Type-check, do not codegen. Fast.                              |
| `kryos fmt`                   | Auto-format `.kry` files in place.                             |
| `kryos test`                  | Run `@test` fns (file or `tests/` dir) in-process. Works with stdlib imports and `@capabilities`; failing tests exit 1. LIMITATION: a runtime PANIC (not a failed assert) inside one test aborts the whole run -- panics are process-fatal by design, so remaining tests don't execute. |
| `kryos repl`                  | Interactive shell.                                             |
| `kryos doc`                   | Generate Markdown (stdout) from `///` doc comments; `--html` for HTML. |
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
11. **Closures use bar syntax, not arrows.** `|x| x + 1`, `|x: i64| x + 1`, or `fn(x: i64) -> i64 { ... }`. There is **no** `(x) => expr` form (`=>` is match-arm-only). Closures capture surrounding variables by **reference** when the captured binding is not mutated inside the closure body (mutations to the variable after the closure is created ARE visible to it); when the closure mutates the captured variable, it captures by move. Closures can be passed as `fn(i64) -> i64` params, returned from named functions (`fn mk(n: i64) -> fn(i64) -> i64 { return |x| x + n }`), and stored in variables — all on both backends. Direct currying (nested lambda literals) works: `let make = |n| |x| x + n; let add10 = make(10); add10(5)` returns 15 on both backends. **Caveat (escaping closures + heap captures):** the "sees later mutations" rule holds for a closure called directly in its defining scope, and for scalar (`i64`/`f64`/`bool`) captures in ALL cases. But a closure STORED in a struct field or array element (an escaping closure) SNAPSHOTS a *heap* capture (`[T]`/`map`/`str`) at storage time -- if you `push` a closure that reads `arr`, then reassign `arr = push(arr, ..)`, the stored closure still sees the old (pre-store) array. Both backends agree (consistent value-capture, not a JIT/AOT divergence). Scalar captures in stored closures stay live. Workaround: call the closure directly, or pass the captured heap data in as an argument instead of capturing it. The SAME root affects a **self-referential closure built via reassignment**: `let mut fact = |x| 1; fact = |n| if n <= 1 { 1 } else { n * fact(n - 1) }` captures the OLD `fact` (the placeholder), so `fact(5)` returns `5`, not `120` (both backends agree -- the fn-typed capture is snapshotted, not a live slot reference). Workaround: use a NAMED recursive function (`fn fact(n: i64) -> i64 { if n <= 1 { return 1 } return n * fact(n - 1) }`) -- named-fn recursion is fully supported on both backends. **Struct/enum closure captures on JIT: RESOLVED.** Previously a closure that captured a STRUCT/enum (heap) value and was reached by more than one closure-env teardown -- stored as a `map` VALUE (`m["a"] = || c.base + 1`), or a NESTED closure whose inner lambda also captured the outer scope's struct -- DOUBLE-FREED the captured struct at scope teardown on `kryos run`/Cranelift (exit 127 after printing the right answer); AOT was clean. Root cause: the Cranelift `RValue::Closure` capture-store stored a Struct/Enum capture as the RAW shared pointer (no clone), unlike `str`/array/map (cloned) and fn/shared (retained), while the closure-env dropper unconditionally `free()`d it -- so two envs capturing the same struct both freed it. Fixed: the Cranelift capture-store now deep-copies Struct/Enum captures into an independent block (via `emit_struct_deep_copy`/`emit_enum_deep_copy`), matching the AOT backend which already did this. Nested-closure-captures-struct, map-value struct capture, curried/single/mutating/sibling/struct-field/returned closures all run correctly on both backends now. Regression: conf_functions closure-struct-capture. **Mutating a captured struct FIELD now matches scalar semantics (RESOLVED, `c365b7c`):** a closure that mutates a captured struct field (`let bump = || { c.base = c.base + 1  c.base }`) captures BY MOVE -- one persistent independent copy across calls, outer var unaffected (`2,3,1`), same as the scalar `let n = 0; || { n = n + 1  n }` case. Was a JIT/AOT DIVERGENCE (JIT `2,3,3` aliased the outer; AOT `2,2,1` lost per-call persistence): `find_mutated_captures` only recognized a bare-identifier mutation target, not `c.field = ..` (fixed via `assign_target_root_name` walking the FieldAccess/IndexAccess chain), so the closure wrongly used the read-only fast path; on AOT the struct capture was additionally passed `byval` (copy-on-entry, write lost) and its heap fields weren't cloned (outer polluted) -- fixed via `mutated_capture_ptr_slot` (pass the one mutated struct capture by plain `ptr`) + `clone_struct_heap_fields`. Mutating str-field and array-field captures also correct on both backends. Regression: conf_functions mutating_struct_closure. **Two-or-more mutated struct captures now persist independently (RESOLVED, `5f2386e`):** `mutated_capture_ptr_slots` (was a single `Option<u32>`) records EVERY mutated struct capture's env-slot, so `|| { c1.base = ..  c2.base = .. }` persists both across calls on AOT (was JIT `111,122,133` / AOT `111,111,111` -- a second co-occurring mutated capture reverted every struct capture to the byval copy-on-entry path). Regression: conf_functions two_mutated_struct_captures. **BOUNDARY of the move-capture isolation (both backends AGREE -- capture-by-reference-into-subobjects, NOT a miscompile):** the "mutating closure captures by move, outer unaffected" rule holds for a WHOLE-BINDING mutation -- `x = ..`, `s.field = ..`, `arr[i] = ..` (including REPLACING an array-of-structs element: `arr[i] = Inner{..}` isolates). It does NOT hold when the mutation reaches INTO a shared sub-object: a captured MAP mutated by key (`|| { m["k"] = m["k"] + 1 }`) writes through to the OUTER map (outer sees the change), and an array-of-structs mutated through a NESTED field (`|| { arr[i].inner.v = .. }`) mutates the SHARED element struct in place (visible to the outer). Root: the closure deep-copies the captured collection's TOP-LEVEL container (array buffer of handles / struct container) so a top-level slot write hits the private copy, but a MAP is captured by shared handle and an array-of-structs clone is shallow (element handles shared), so a mutation reaching into the shared sub-object leaks. This matches Rust/JS capture-by-reference semantics; both backends are consistent (not a JIT/AOT bug). A recursive deep-clone of captured collection contents would close it but risks the ARC/self-host path, so it is a documented boundary. Workaround (for isolation): mutate at the TOP level (`arr[i] = Inner{ inner: Inner{ v: .. } }` instead of `arr[i].inner.v = ..`), or operate on data passed as a closure ARGUMENT rather than captured.
12. **`?` operator works on both backends** (Cranelift JIT and LLVM AOT). `let v = parse(s)?` returns early with the `Err` on failure. Use `Result<T, E>` (e.g. `Result<i64, str>`) return types.
13. **`Result<T,E>`/`Option<T>` payloads keep their real type when matched** — `match r { Result.Err(e) => println(e) }` binds `e` as `str` (for `Result<i64,str>`) and prints it correctly; `Ok(v)`/`Some(v)` bind their real type too. The match-payload binding now uses the monomorphized enum type (e.g. `Result___i64_str`) rather than the i64-erased generic stub, so direct use (bare `println(e)`, `==`, arithmetic) dispatches on the right type on both backends. **Always annotate `Result<T,E>`/`Option<T>` on signatures** — a *bare* `Result`/`Option` (no `<...>`) still erases its payload to i64.
14. **Or-patterns work in match arms:** `match n { 1 | 2 | 3 => "low", _ => "high" }` and `match c { Red | Green => "warm", Blue => "cool" }`. Alternatives must be non-binding (literals or bare enum variants).
15. **Matching on a tuple value works:** `match p { (0, 0) => ..., (x, 7) => x, (5, _) => ..., _ => ... }` — literal elements are compared, ident elements bind the field, and `_` elements match anything (on both backends). String-literal elements (`("ok", n) => n`) work too.
16. **Struct-style enum variants are not supported.** `enum E { A { x: i64 } }` is rejected with a clear error — use a tuple variant `A(i64)` and match `A(x)`. **Three variant-reference spellings all work on both backends and lower identically:** bare `Some(7)`, dotted `Opt.Some(7)`, and Rust-style `Opt::Some(7)` — in construction, as a function argument, in array literals, in `match` arms, and nested. A *nullary* variant used in value position must be qualified (`Opt::None` or `Opt.None`); bare unqualified `None`/`Red` in an expression is rejected (`E0102`) to avoid cross-enum ambiguity (bare is still fine in `match` patterns, where the matched type disambiguates).
17. **`impl` methods on generic structs work, including parametric `impl<T>`.** All three forms parse and run on both backends: `impl Wrap { fn get(self: Wrap<i64>) -> i64 {..} }`, the concrete `impl Wrap<i64> { .. }`, AND the parametric `impl<T> Wrap<T> { fn get(self: Wrap<T>) -> T {..} }`. **Using one generic method at multiple concrete types in the same program is fully supported** — each call instantiates fresh type vars and binds them to the receiver, so `let a: i64 = p.get(); let c: str = q.get()` (two instantiations of the same method) no longer contaminate each other. **A bare `-> T` method return resolves to the receiver's concrete type**, including `f64` — `to_string(box_of_f64.get())` prints the float, not raw i64-slot bits (fixed; annotation no longer required). Multi-parameter impls (`impl<A, B> Pair<A, B>`) work too. **Chaining a method call ON the result of a generic method now resolves without annotation** — `ww.get().get()` (and an un-annotated `let x = ww.get()` intermediate) correctly infers and codegens on both backends when the inner method's return is a bare `-> T`; the explicit-annotation workaround (`let l1: Box<i64> = ww.get()`) still works too, it's just no longer required. One residual edge remains: a method whose return TYPE is a COMPOUND shape merely mentioning the parameter (`-> (T, i64)`, `-> [T]`) keeps the erased i64-slot element for non-pointer `T` (a float tuple element prints as its bit pattern) — it builds and is backend-consistent, but annotate or return the bare `T` for exact floats.
18. **Integer division/modulo, bounds, AND out-of-range float→int casts are all checked/defined.** Integer `a / 0` / `a % 0` panic ("integer division by zero"), `i64::MIN / -1` and `% -1` panic ("integer division overflow" — the quotient is unrepresentable), array/string out-of-bounds panic, and signed division truncates toward zero (`-7 / 2 == -3`) consistently on both backends. Casting an out-of-range `f64` to an integer now **saturates** on BOTH backends (`1.0e300 as i64` → `i64::MAX`, `(-1.0e300) as i64` → `i64::MIN`, `NaN as i64` → `0`) — no undefined behavior, no manual range guard needed. **Precedence note:** `as` binds TIGHTER than a leading unary `-`, so write `(-1.0e300) as i64` to cast the negative value; the un-parenthesized `-1.0e300 as i64` parses as `-(1.0e300 as i64)` = `-(i64::MAX)` (one off from `i64::MIN`). Parenthesize the negative operand when you mean "cast the negative number". (This was previously UB on AOT; fixed.) **One narrow known edge:** `parse_float("-0.0")` yields `+0.0` on the AOT backend (the sign of a parsed *zero* is dropped), while `kryos run`/JIT preserves it -- so `1.0 / parse_float("-0.0")` is `+inf` on AOT vs `-inf` on JIT. Only negative ZERO is affected: non-zero negatives (`parse_float("-5.0")`) and the literal `-0.0` are correct on both backends. Almost no program depends on the sign of a parsed zero; if you do, construct it as `-1.0 * 0.0` instead. (Backlogged.) **A NaN's SIGN BIT also differs by backend** (same class): an invalid-op NaN (`0.0/0.0`, `inf - inf`, `0.0 * inf`, `sqrt(-1.0)`) is canonicalized with the sign bit SET on `kryos run`/JIT (`0xFFF8...`) but CLEAR on AOT (`0x7FF8...`). This is NOT observable through normal float use (a NaN compares false to everything, `to_string` prints "nan" on both, and arithmetic just propagates NaN) -- it only shows if you reinterpret the NaN's raw bits as an integer or inspect `copysign` on a NaN. Do not rely on the sign of a NaN; it is not semantically meaningful. **A user function shadowing a builtin now WINS on both backends** (`fn sin(x) { .. }`, `fn abs(n) { .. }`, `fn len(..)`, etc. call YOUR body, not the builtin) — previously the codegen math fast-paths silently ran the builtin instead. **Sole residual (libm-name const-fold, gotcha #18):** on the AOT backend only, LLVM's constant-folding recognizes a function whose name + `(f64)->f64` signature matches a libm routine (`atan`, `sin`, `pow`, ...) and folds a CONSTANT-argument call to libm's exact value, ignoring the Kryos body. `kryos run`/JIT always runs your body now, and AOT runs it too for any RUNTIME (non-constant) argument — only a literal-constant call to a libm-named `(f64)->f64` fn on AOT hits this. For std::math this is harmless (~1 ULP); if you shadow a libm name with different behavior, pass a runtime argument or don't name it after a libm routine.
19. **Closures passed to higher-order functions infer their parameter types** from the function's signature — `fold(xs, 0, |acc, x| acc + x)`, `reduce`, `scan`, `map_indexed`, `filter`, `map` all work without annotating the closure params, including when the accumulator and element types differ (`fold(strs, 0, |acc, x| acc + len(x))`). These HOFs live in `std::iter` and must be imported (`use std::iter::{fold, filter, map, reduce}`) — they are NOT global builtins (unlike `abs`/`min`/`max`/`sqrt`). This relies on a generic `fn(T, U) -> U`-style signature; a custom HOF you write should declare one (`fn myfold<T, U>(arr: [T], init: U, f: fn(U, T) -> U) -> U`) rather than a bare `f: fn`. Substring search is `use std::string::{find}` (`find(haystack, needle) -> i64`), not `index_of`.
20. **Closures keep their real `str`/struct/array/**float** type across a HOF boundary, in both params AND return.** An un-annotated closure passed to a higher-order function carries its inferred param type into the body and its inferred return type out, so `fold(words, "", |acc, x| acc + x)` concatenates strings (not integer-adds handles), `fold(nums, 0.0, |acc, x| acc + x)` over `[f64]` sums as floats, and `map([1.0, 2.0, 3.0], |x| x * 2.0)` returns a real `[f64]` whose elements read as floats with **no binding annotation** needed (the uniform i64 closure ABI bit-casts floats in/out via the env-thunk; only the static type flows so a generic `[U]` result binds `U=f64`). Works on both backends. **Exception -- a directly-called local closure returning `bool`:** `let is5 = || x == 5; to_string(is5())` prints `1`/`0`, not `true`/`false` -- the result is tracked as `i64` across the call. Both backends agree (a formatting gap, NOT a divergence; the VALUE is correct: `if is5() {}` branches right and `is5() == true` is right). Workarounds (both verified): annotate first (`let b: bool = is5()  to_string(b)` -> `true`) or compare (`to_string(is5() == true)` -> `true`). Minor/backlogged.
21. **AOT struct fields are 8-byte slots; a tuple/enum field followed by other fields is unsafe on `build --release`.** The LLVM backend addresses struct fields with an i64-stride GEP (every field assumed 8 bytes), so a multi-element tuple/enum field (>8 bytes) throws off the byte offset of any field declared AFTER it: mutating a trailing field (`r.after = x`) **silently writes to the wrong offset on AOT** (a real miscompile; `kryos run`/JIT is correct). Enum-typed fields are additionally flattened to a bare `i64` slot, so extracting one and using it as the enum fails to compile on AOT. **Workarounds:** declare tuple/enum fields LAST in the struct, or use `kryos run`. (UPDATE: the trailing-field-mutation miscompile is FIXED — StoreField now uses a struct-indexed GEP; the enum-field-as-aggregate read also works now. This gotcha is mostly historical.)
22. **Known remaining limitations (use `kryos run`/JIT or the workaround on AOT) — the shrinking bug tail:**
    - **`push(arr, v)` appends IN PLACE to the shared buffer, so never read a pre-push alias.** `push` grows the array's shared ARC buffer and returns the (possibly reallocated) handle; the canonical form `arr = push(arr, v)` is correct. But `let b = push(a, v)` then reading `a` is UNDEFINED: `a` may or may not see the appended element depending on whether the buffer reallocated. Both backends agree (consistent with the shared-handle model, gotcha #23), but it is a footgun. The same applies transitively to the `std::heap`/`std::queue`/`std::stack`/`std::deque` ops built on `push` (`push_min`, `enqueue`, `push_top`, `push_back`): reassign the result, never snapshot the pre-call value. For an independent snapshot, build a fresh array (copy the elements you need into a new `[]`).
    - **Namespace: importing a function whose name collides with a global builtin USED BY another imported stdlib module breaks that module.** Kryos has one flat function namespace, so `use std::trie::{contains}` (or `std::set`/`std::interval`, all of which export `contains`) makes the imported `contains` shadow the global builtin `contains` EVERYWHERE -- including inside `std::os`'s own body, which calls the builtin `contains` internally. The result is a confusing, mislocated `E0100` from inside the unrelated module. Workaround: don't selectively import a name that shadows a builtin another module needs (import the module's OTHER symbols, or call `contains(...)` as the builtin and reach the module function another way). Per-module resolution scoping is backlogged.
    - **`to_string` of an ARRAY / tuple / map (with no impl method) returns a placeholder (`<array>` / `<tuple>` / `<map>`), not the element contents.** `to_string([1,2,3])` yields `"<array>"`, not `"[1, 2, 3]"` (some first-party stdlib docs show the latter aspirationally). A STRUCT/enum with its own `to_string` method dispatches to it; scalars (i64/f64/bool/str/char) format normally. To render a collection's contents, iterate and build the string with `+` and per-element `to_string`.
    - **C FFI to non-`kryos_*` symbols is NOT emitted yet -- an `extern "C" { fn abs(x: i32) -> i32 }` to a real C library does NOT link on AOT (`undefined value @abs`) and the JIT only "works" when the name accidentally collides with a builtin.** The `kryos_*` runtime externs (used by the stdlib and the documented capability-gating example) work; arbitrary C-library FFI is a documented-but-unimplemented feature (the extern's param/symbol info isn't threaded to codegen). Do not rely on calling your own C functions until real FFI emission lands.
    - **Raw-memory builtins (`alloc`/`free_bytes`/`ptr_read_i64`/`ptr_write_i64`/`ptr_byte_at`/`ptr_set_byte`/`str_to_ptr`) require NO capability and are NOT gated by `unsafe { }`** -- they are treated as internal "runtime plumbing" (the stdlib's `buf_*`/string builders use them). This means a zero-capability program can read or write arbitrary computed addresses even under `--strict-capabilities`. A security-critical deployment should treat these as an ungated unsafe surface (audit their use directly); whether to gate them behind a capability or `unsafe { }` is an open security-model decision.
    - **Building a large string with `s = s + chunk` in a loop is O(n²) — use `std::string::string_builder()` for O(n).** Each `+` reallocates and copies the whole accumulator, so building a multi-MB string by repeated concat is quadratic (measured ~256× slower than the builder at 160k chars, and the gap grows). `let sb = string_builder()  sb.append(chunk)  … let result = sb.build()` appends into a growable buffer (amortized-O(1)) and materializes once (`build()` frees the buffer — call it exactly once). `append` chains: `sb.append("a").append("b")`. (`std::string::join`/`replace` still use `+` internally, fine for small inputs.)
    - **A blocking `recv(ch)` on a CLOSED and drained channel returns `0`, indistinguishable from a real `send(ch, 0)`.** Only `chan_try_recv` distinguishes closed (`-1`) from data. Use `chan_try_recv` / `chan_is_closed` if you need to tell "closed" from a legitimate zero (same undocumented-zero-sentinel class as `pop([])`).
    - **Runtime panics (div-by-zero, index-out-of-bounds, `file_read` on a missing file) are NOT catchable by `try`/`catch` -- only `throw` is.** `try { 10 / 0 } catch e { .. }` does NOT run the catch: the panic aborts with exit 98 (a first-party error-handling doc oversells catch as catching runtime errors; only an explicit `throw` unwinds to `catch`). Guard the precondition (`if b != 0`) or use a `Result`-returning wrapper instead of relying on catching a panic.
    - **`comptime { }` blocks are NOT a compile-time evaluator yet -- they run at RUNTIME like an ordinary block.** Despite `docs/11-comptime.md` describing isolation/determinism guarantees in present tense, both backends currently lower a `comptime { }` block's expression directly as normal runtime code: it CAN read outer-scope variables, `println` inside one prints at run time, and there is no compile-time folding or sandbox. Basic arithmetic/string examples produce correct values (because runtime evaluation gives the same answer), but do NOT rely on comptime for isolation, compile-time constants, or side-effect suppression. (The evaluator is planned; the doc's guarantees are aspirational.)
    - **Shift-by-the-type's-own-width is width-dependent** (low-stakes edge): `1i64 << 64` (shift count from a variable) masks the count mod 64 (hardware SHL semantics) so it behaves as `<< 0` = 1, while `1u8 << 8` shifts fully out to 0. Like C/Rust-release, shift-amount >= bit-width is not a defined contract -- keep shift amounts strictly less than the operand's width.
    - **Hand-declaring a `kryos_*` runtime extern with a `str`/heap signature CRASHES -- use the builtin instead.** `extern { fn kryos_env_get(key: str) -> str }` then calling it SEGFAULTS: a hand-declared `kryos_*` extern calls the raw C symbol WITHOUT the str-handle marshalling the real builtin path applies. The builtin route (`env_get("PATH")`, no extern block) works perfectly. Only hand-declared `kryos_*` externs with pointer-backed (str/array/map) params/returns are affected; i64-only signatures don't crash (they read a raw handle). Use the documented builtin; reserve hand-declared externs for genuine C-library FFI (non-`kryos_*` names).
    - **`std::collections::List<T>`/`Set<T>` with non-i64 elements: RESOLVED.** `let ls: List<str> = List.new()` now binds `T` from the let annotation (previously the no-arg ctor defaulted to `List___i64` and crashed/failed the build); the monomorphized body gets real generic bindings (its internal struct literal builds the right instance); and methods with a T-typed VALUE param (`has`/`contains`/`index_of`/`add`) monomorphize per instantiation instead of running the i64-erased copy (which stringified/compared POINTERS -- `Set<str>.has` was always false, `List<str>.index_of` matched by allocation). `List<str>`/`Set<str>`/`List<i64>` verified on both backends; regression: conf_generics. Flow-through methods (`get`/`len`/`pop`) keep the erased fast path.
    - **`i128`/`u128` are NOT functional -- do not use them (use `i64`/`u64`).** Even a trivial `let a: i128 = 100` crashes: the Cranelift/JIT hits a verifier ICE (`internal error: entered unreachable code`) and the LLVM/AOT build fails (`'%_N' defined with type 'i128' but expected 'ptr'`). Reproduces for `u128` and for arithmetic at values that fit in i64 -- the wide-int types are declared but unimplemented in codegen. Separately, an integer literal wider than i64 range is rejected at parse time (`E0009`) even when annotated `i128`, so the types are unusable for their stated purpose. (Full 128-bit codegen is backlogged; until then treat i128/u128 as absent.)
    - **`pop()` on an EMPTY array returns `0`, it does not panic.** `let x: i64 = pop([])` yields `0` (exit 0) on both backends -- intentional and consistent (`kryos_builtin_pop` returns 0 for empty/null), but inconsistent with array indexing (`a[-1]`/OOB correctly panics exit 98) and silently masks empty-pop logic bugs since `0` is a plausible real value. Guard with `len(a) > 0` before `pop` if the distinction matters.
    - **`base64`/`chr`/`byte_at` use a latin-1 BYTE-BUFFER model (codepoints 0-255 == bytes), NOT UTF-8 text.** `base64_encode` reads each codepoint as one byte and `base64_decode` emits one latin-1 codepoint per byte, so `base64_encode(base64_decode(x)) == x` for raw/binary payloads (this is what JWT signatures, WebSocket frames, and SMTP AUTH use, and there's a conformance test for the high-byte round-trip). CONSEQUENCE: `base64_encode` of a string containing a codepoint **> 0xFF** (CJK, emoji, most non-Latin scripts) **silently truncates** each such codepoint to its low byte -- `base64_encode("日")` gives `5Q==` (low byte of U+65E5), not the standard base64 of its UTF-8 bytes (`5pel`). There is no separate "encode this string's UTF-8 bytes" builtin, so to base64 arbitrary UTF-8 TEXT you must keep it in the byte-buffer form (build it from `chr()`/`byte_at()`), or the encode is lossy. This is a deliberate model choice (flipping to UTF-8-bytes would break the binary round-trip jwt/smtp/websocket rely on), not a compiler bug -- but it IS a data-loss trap for non-Latin1 text. Design follow-up (owner decision): add a distinct `base64_encode_text`/`[u8]` bytes API for the UTF-8-text case.
    - **A generic function that RETURNS a closure: pointer-backed T (str) RESOLVED; T=f64 has a residual VALUE bug.** `fn make_appender<T>(suffix: T) -> fn(T) -> T { return |x| x + suffix }` at `T=str` now builds and runs correctly on BOTH backends (the lambda's str-concat result is defined as `ptr` while the erased return slot is i64; the ret site now tracks the real SSA type and ptrtoints -- was a hard AOT build failure). `T=i64` works. **`T=f64` is WRONG on both backends (consistent, not a divergence):** the returned closure's un-annotated param stays i64-erased, so `x + suffix` int-adds the float BIT PATTERNS (prints a huge garbage float). Workaround for floats: a concrete `fn(f64) -> f64` signature (drop the type param). Regression: conf_functions mk_appender.
    - **Closures stored in an UNTYPED `[]` array lose their RETURN TYPE (erased to i64) at the call site.** Ownership and VALUES are correct -- a closure captured per-iteration in a loop and `push`ed into an array snapshots its capture correctly (`fns[k]()` returns the right value on both backends, no aliasing, no double-free). But an *untyped* `let mut fns = []` does not infer its element type from the pushed closures, so `fns[k]()` types as `i64`: when the closure returns a NON-i64 type (`str`/`f64`), the un-annotated result reads back as the raw handle/bit pattern (`println(fns[k]())` prints a giant integer). i64-returning closures are unaffected. Fix: annotate the array (`let mut fns: [fn() -> str] = []`) or the result (`let r: str = fns[k]()`) -- either makes the return type flow and the value renders correctly. (Same untyped-`[]`-element-inference gap as the resolved untyped-array-of-structs case, not yet extended to closure element types.)
    - **A closure literal RETURNED from a function does not infer its param type from the function's declared return type.** `fn mk() -> fn(f64) -> f64 { return |x| x * x }` fails to type-check (`error[E0100]: ... body evaluates to fn(?T) -> ?T`) because the un-annotated `x` is only inferred from typed expressions it's used WITH inside the body (e.g. `x + n` where `n: i64` is captured), NOT from the enclosing function's declared `-> fn(f64) -> f64`. Workaround: annotate the closure param -- `return |x: f64| x * x` -- which type-checks and runs on both backends. (Closures PASSED to a HOF infer their params from the HOF signature, per gotcha #19; only closures RETURNED by a plain function need the annotation.)
    - **A BLOCK-BODY closure (`|x| { ... return expr }`) does not infer a non-i64 RETURN TYPE across a generic higher-order-function boundary.** An EXPRESSION-body closure (`|x| if c { "a" } else { "b" }`) and a NAMED function both infer their return type correctly, but a block-body closure with `return` statements passed to a generic HOF (e.g. `std::iter::group_by(xs, |x| { if c { return "even" }  return "odd" })`) has its `str`/`f64` return erased to an i64 slot, so the value is read back as a raw handle/bit pattern (group_by then builds garbage map keys). i64 returns are unaffected. Workaround: use an expression-body closure (`|x| if c { "even" } else { "odd" }`) or a named `fn(T) -> str` helper -- both thread the return type correctly on both backends. (Same closure-return-inference family as the directly-called `bool` closure in gotcha #20.)
    - **A closure that is the TAIL VALUE of a block-expression cannot capture that block's earlier `let` bindings** -- `let g = { let base = "x"  || base + "!" }` fails to type-check with `E0102: undefined variable base` (the block-local is not threaded into the escaping closure's capture scope). Workaround: hoist the captured binding out of the block (`let base = "x"  let g = || base + "!"`) -- capturing a function-level local works normally on both backends.
    - **`any` is type-erased to i64, so `to_string`/`std::fmt::format` mis-render non-i64 `any` values.** The `any` type carries NO runtime type tag (it lowers to a bare i64 slot). A `str`/`f64` stored in an `[any]` (e.g. an `[str]` passed to a `fn(args: [any])`, or built via `push(args, "x")`) reads back through `to_string` as its raw pointer/bit representation, not its logical value -- so `std::fmt::format("{{0}}", args)` only formats i64 args correctly; str/f64 args print garbage. This is inherent to the erasure (fixing it needs tagged `any` values). Workaround: build the final string with `+` and per-type `to_string`/`float_fixed` calls on the concretely-typed values instead of routing through `[any]`/`format`. **The element-typed `std::iter` HOFs are now generic and preserve `str`/`f64`** (`map`, `filter`, `find`, `sum`, `product`, `min`, `max`, `min_by_key`, `max_by_key`, `any`, `all`, `position`, `count_if`, `partition`, `flatten`). But the PAIR/heterogeneous HOFs (`zip`, `unzip`, `enumerate`, `chunks`, `windows`, `flat_map`, `for_each`) still take/return `[any]`, so a `str`/`f64` element read out of their result prints as raw bits unless you annotate the read site (`let x: str = pair[1]`) -- they build `[index, elem]`/`[a, b]` pairs whose two slots can differ in type, which a single `<T>` can't express. Annotate on read, or use the element-typed HOFs.
    - **A `std::collections` List/Stack/Queue/Deque push shares its backing buffer, so do NOT branch two derived collections off one parent.** `let a = xs.push(1)` then `let b = xs.push(2)` from the SAME `xs` makes `a` and `b` alias the same in-place-grown buffer (both end up with both pushed elements) -- the raw `push` shared-buffer footgun (gotcha at the top of this list) leaking through the collection wrapper. The canonical single-threading form `xs = xs.push(v)` is correct; branching needs an explicit copy of the parent first. (Copy-on-push would fix branching but make every push O(n).)
    - **`gcd`/`lcm` of `i64::MIN` cannot return the correct positive magnitude** because `|i64::MIN|` = 2^63 exceeds `i64::MAX` -- an inherent i64 range limit (the result is unrepresentable), not a fixable bug. `gcd(i64::MIN, 0)` and `lcm(i64::MIN, k)` return a negative/wrong value; every other input is correct. (String formatters like `fmt::hex(i64::MIN)` are fine -- a string has no range limit.)
    - **`dyn Trait` inside ANY container is NOT supported — now a CLEAN COMPILE ERROR (E0110), no longer a crash.** A single `dyn Trait` value, parameter (including trait-default-method, dyn-receiver, and `Type::method(..)` static params), field, return, or let-binding works and dispatches correctly on both backends (the vtable slot order now follows TRAIT-declaration order, so an impl that lists methods in a different order than the trait dispatches correctly). **One residual: a GENERIC-STRUCT impl (`impl<T> Trait for Box<T>`) called through `dyn` with a NON-i64 `T` reads a `str`/`f64` field as its raw bit pattern** -- the vtable holds ONE erased method copy shared across all `T`, so `dyn` dispatch takes the i64-slot path (a direct/static call on the same value is correct on both backends). Workaround: a non-generic `impl Trait for Box_str {}` per concrete type, or call the method directly rather than through `dyn`. **Second residual: a `dyn` method that RETURNS a by-value AGGREGATE (a multi-field struct, a tuple, an enum, `Option`, or `Result`) FAILS TO BUILD on AOT** (`kryos build --release` errors with an invalid-LLVM-IR `add %Agg, 0`; the uniform i64 dyn-thunk ABI cannot pass a by-value aggregate through, so it truncates to the aggregate's first field). `kryos run`/JIT works. Workaround: have the `dyn` method return a scalar or a heap handle (a `str`/array/map, which ARE passed as i64 handles and work), or call it via static dispatch; return the fields individually. But a trait object stored INSIDE a container is unimplemented: `[dyn Shape]` arrays, `Option<dyn Shape>` / `Result<dyn Shape, E>` payloads, `(i64, dyn Shape)` tuple elements, and `map<K, dyn Shape>` values are all rejected at type-check with `error[E0110]: \`dyn X\` cannot be stored in ... yet` (previously they type-checked clean and SEGFAULTED/hung at runtime). Workaround: use an enum with a variant per concrete type and `match` (fully supported), or keep separate typed arrays. (Fat-pointer container storage is backlogged; regression gate: tests/type_soundness.sh.)
    - **Untyped array of aggregates: RESOLVED.** `let mut a = []` followed by `push(a, Struct{..})` (or `a = push(a, ...)`) now infers `a: [Struct]` — `a[i].field` and `let last = pop(a); last.field` work on both backends with no annotation. The type checker gives `push`/`pop` real generic signatures and resolves the element type from later pushes; the resolved type is threaded into MIR so both backends agree, and `pop` is element-typed (AOT unboxes aggregate elements, reinterprets float/ptr slots).
    - **Narrow-int literal assignment: RESOLVED.** `let x: u8 = 200`, u8 fn params/returns, `[u8]` array literals, and u8 struct-field inits all accept plain int literals on both backends, and `to_string`/`println` of unsigned values zero-extends correctly (u8 200 prints `200`, not `-56`). Note: like the signed types, range is not checked at compile time — an out-of-range literal truncates.
    - **Nested struct field mutation: RESOLVED.** `o.a.v = 99` (any depth, e.g. `a.b.c.z = 77`) works on both backends — MIR lowers it as read-modify-writeback so the mutation propagates through intermediate copies. Storing a str literal into a struct field on AOT also works now (the slot store coerces ptr/double/i1 SSA values). Mutating fields of a struct-typed *function parameter* compiles on both backends now too — but see gotcha #23 for a semantic caveat.
    - **`Option`/`Result` with a multi-field struct payload: RESOLVED.** `Option<User>` / `Result<User, str>` where `User` has any number of fields (mixed i64/str/f64) now type-check and run correctly on both backends — `match`, `if let`, `Some`/`Ok`/`Err`/`None` paths, and direct field access on the bound payload all work. Tuple payloads (`Option<(i64, str)>`, `Result<(i64, str), str>`) work too.
23. **Struct param/copy semantics across backends (PARTIALLY RESOLVED).** **All-scalar `@copy` struct params** (no str/array/map/struct/fn fields) are now copied at function entry on the JIT, so a field mutation inside the callee no longer aliases the caller's value — both backends agree on pass-by-value for plain-data structs (regression test: `tests/smoke/test_copy_param_value_semantics.kry`). Residual divergences:
    - **Heap-bearing `@copy` structs as params:** the JIT still passes the caller's pointer (aliasing) while AOT passes a shallow byval copy. The entry copy is deliberately NOT applied here: the self-host parser threads `Parser { tokens: [Token], .. }` through every `p_*` call under the share-on-clone model, and copying it at entry clones the token array per call (deep copy OOMs stage-1; even a shallow copy is an allocation on the hottest path).
    - **Non-`@copy` structs:** JIT aliases, AOT copies. Mostly unobservable; the historical BROAD crash hazard here (F1) is now **RESOLVED**. Was: a non-`@copy` struct with a HEAP field, reassigned inside a LOOP from a NESTED-`match`-bound enum payload (`match r { Ok(nc) => { c = nc } .. }`) in a multi-arm enum dispatch, CRASHED both backends (AOT segfault / JIT corrupt, order-dependent) -- the standard "validate-then-commit via `Result`" state-machine idiom, so ordinary interpreter/parser/RPN loops hit it. ROOT CAUSE (verified by AOT disassembly to the faulting instruction): a `let` declared inside one match arm (a VALUE-position block) was never dropped at that arm's OWN scope end -- `lower_expr_to_rvalue`'s block path had no scope-exit drop, unlike `lower_block_stmts`. The un-dropped local was swept up by the next enclosing scope (the `for`-loop body wrapping the whole match), whose shared post-match drop ran UNCONDITIONALLY every iteration, firing `__kryos_drop_S` on the arm-local's stale/null slot even on iterations that took a different arm -> null-deref (first iter) or double-free of a prior box (reverse order). FIX: factored `emit_named_scope_drops` out of `lower_block_stmts` and call it from the block-rvalue path (with the tail-identifier-move guard), so a match-arm-local drops at the end of its OWN arm; plus a null guard in the generated `__kryos_drop_<Struct>` helper (defense-in-depth, matching the enum-drop helpers) and the missing `kryos_rt_init()` call in AOT `@main`. Verified: minimal repro (both orders) + realistic RPN + `Option` variant all correct on both backends; conf 15/15, self-host 16/16; the `retain_struct_heap_fields` dup that protects the loop-body-local-subject case still works. Regression: conf_ownership `f1_run`.
    - **Heap-field content on copies: UNIFIED (step 224).** `@copy` ASSIGNMENT (`let c = b`) deep-clones array/str/map fields on BOTH backends now — "each copy owns its data" (regression test: `tests/smoke/test_copy_assign_deep.kry`). The LLVM backend gained the same per-field clone the Cranelift backend always had; clones leak under the no-op `@copy` drop model on both backends (leak-on-copy, consistent).
    - **BOUNDARY: the `@copy` deep-copy contract applies to VARIABLE-to-variable assignment only, NOT to reading a struct element OUT of a collection.** `let a = m["k"]` (map value) and `let a = arr[0]` (array element) return a SHARED handle to the stored box on both backends: a later in-place mutation (`m["k"].name = "gamma"`) IS visible through `a` (prints gamma, not the snapshot). Applies to single- and multi-field structs, `@copy` or not; plain `let a = b` between locals still deep-clones. This matches the closure sub-object capture boundary (capture-by-reference-into-subobjects) and both backends agree — a semantic boundary, not a miscompile. For a real SNAPSHOT, copy explicitly: `let a = S { name: m["k"].name }` (field-by-field), or read the fields you need into locals before mutating.

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
