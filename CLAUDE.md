# CLAUDE.md

This is the lean core. The complete, verified gotcha encyclopedia -- full histories, root causes, commit hashes, resolved-bug narratives, and every edge case -- lives at `docs/claude/FULL-REFERENCE.md`. Grep it by topic (closures, strings/UTF-8, FFI/extern, AOT vs JIT divergence, memory/ARC, generics, `dyn Trait`, capabilities) **before** fighting any weird behavior. Record new findings THERE, not here.

## What Kryos is

A capability-safe, ownership-aware systems language with two backends:

- **Cranelift** (`kryos run`) — debug JIT, fast compile, no external linker
- **LLVM** (`kryos build --release`) — release AOT, full optimization, links via `cc`/`clang`/`link.exe`

Targets Linux x86_64, Windows x86_64 MSVC, macOS x86_64/aarch64, and `wasm32-unknown-unknown` (JS host contract — browser or `node tools/wasm-host/run.mjs`; WASI is not supported). The same `.kry` source runs on every target — the only platform-conditional code you should write is filesystem-path handling.

## Hard rules (these cause compile errors)

1. **No semicolons.** Line breaks terminate statements. To wrap a long expression, end the line with `(`, `[`, `{`, `,`, or a binary operator. **The converse trap: a NEW line that STARTS with `-`, `(`, or `[` CONTINUES the previous expression** (JS-ASI-class grammar): `let a = 5` followed by a line `-1` parses as `let a = 5 - 1` (a is 4, silently); `println("hi")` followed by `(x, y)` parses as a call continuation; and `let x = arr` followed by `[0]` parses as `let x = arr[0]` (indexing). Never begin a statement line with unary `-`, a parenthesized expression, or a `[`-literal — bind it (`let n = -1`) or restructure. As of LEDGER item 9's W0001 diagnostic, the `||` case is no longer fully silent: `kryos check`/`run`/`build` now warn (not error -- the merge still happens, unchanged, for backward compat) when a fresh `||` right after a newline is the FIRST `||` encountered while building an expression (the dangerous shape); an established multi-line `||` chain (the operator already appeared earlier in the same statement, e.g. an `is_digit`-style predicate) does not warn. Single `|` is deliberately NOT covered (a real corpus sweep found single-`|` multi-line bitwise-or bit-packing is a common, legitimate pattern that would false-positive), and `-`/`(`/`[` still have no diagnostic at all. **This list is not exhaustive: the parser has NO newline-awareness at all** (verified by reading `kryos-parser`/`kryos-lexer` — tokens carry only byte-offset spans, no line numbers, and the Pratt expression loop keeps consuming any token with infix binding power regardless of a line break). ANY line that starts with a token usable as a binary operator continues the previous statement, silently, with zero diagnostic when the types happen to align. This bites the **closure literal opener `||` and `|`** in particular, since they double as the empty-/single-param closure prefix AND as boolean-or / bitwise-or infix operators: `let ready: bool = check_a()` followed on the next line by `|| check_b()` parses as `check_a() || check_b()` (one boolean-or expression, not two statements) — if both sides are `bool`, this compiles clean and runs, silently short-circuiting/dropping the second statement instead of defining a closure (see gotcha #11's mutated-capture note for the closure-specific fallout; a bare leading `+` continues the same way, since Kryos has no unary `+`). Workaround: never let a fresh statement's first token be one of `-`, `(`, `[`, `|`, `||`, or (by the same mechanism) any other binary-operator token; assign a closure literal via its OWN `let name = || ...` (a fresh expression position, nothing to merge with) rather than leaving it as a bare tail/standalone statement.
2. **Block comments `/* ... */` work and nest.** Line comments are `//`, doc comments `///`. (The self-host compiler source under `compiler/self-host/` avoids block comments by convention, but the language supports them.)
3. **No `null` / `nil` keyword.** Use `Option<T>` from `std::option` for nullable values.
4. **String interpolation works in EVERY string literal:** `"hello {name}"` (delimiter is `{ }`, NOT `${ }`). Unlike Python/Rust where only an `f"..."`/`format!` string interpolates, **ALL Kryos strings interpolate**, so a bare `{` in any string opens an interpolation. To put a **literal brace** in a string you MUST double it (`{{`, `}}`) or backslash-escape it (`\{`, `\}`). This bites JSON, code-gen, and set notation: `"{\"a\":1}"` FAILS to parse (the `{` opens an interpolation) — write `"{{\"a\":1}}"`, or build the string with `+` (`"{" + "\"a\":1" + "}"`). `+` concatenation also works generally; cast numerics with `to_string(x)` when concatenating. **An interpolation cannot START with a string literal directly** (`"{"lit" + x}"` fails): the lexer reads `{"` as a literal brace before a closing quote — a deliberate ambiguity guard (it protects `"{"` and `["{", "}"]` from silent cross-element corruption). Put a space after the brace (`"{ "lit" + x}"`) or bind the string to a variable first.
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
| `[T]`       | Owned dynamic array. `len`, `push`, `pop`, `arr[i]`.                |
| `map<K, V>` | Hash map. `m[k]`, `m[k] = v`, `contains(m, k)`.                    |
| `(A, B, C)` | Tuple.                                                              |
| `Option<T>` | From `std::option`. `Some(x)` / `None()`.                          |
| `Result<T, E>` | From `std::result`. `Ok(x)` / `Err(e)`.                          |
| `*T`        | Raw pointer. Only inside `unsafe` or `extern` blocks.               |

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
| ------------------------------------ | ----------------------------------------------- |
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

> **Import namespace gotcha:** imports share ONE flat namespace and there is **no import aliasing** (`use m::{parse as p}` is a parse error). Two modules exporting the same name (`std::csv::parse` vs `std::json::parse`) cannot both be imported; the compiler errors at the import, and a module-qualified call (`json::parse(..)`) is only sugar for the flat name -- the compiler validates it against the import's ORIGIN and errors if it came from a different module. Resolve collisions by importing disjoint names selectively. (The resolver ALSO pulls every STRUCT of an imported module regardless of the selective list, so two modules DEFINING a same-named struct collide even when your imports are disjoint -- `std::chan`'s concurrency types are prefixed `ChanWaitGroup`/`ChanOnce` for exactly this reason, so chan + sync co-import cleanly; type-reachability import is backlogged.) **Actors are constructed with `Name()`** (state is private, zero-initialized) -- the struct-literal form `Name { field: v }` is rejected. **`json_object(keys: [str], values: [JsonValue])` takes two arrays**, not zero args.

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

Package registry: `NORTHTEKDevs/kryos-registry` on GitHub. **`kryos pkg install`/`add` now verifies a real content checksum before trusting a package (fixed; LEDGER item 1b CLOSED).** `kryos pkg install` still fetches a dependency by `git clone --depth 1` of the registry's current default-branch HEAD and copies out `packages/<name>/<version>/` -- it does not download a `.tar.gz` tarball -- but it now computes a canonical `sha256:<hex>` hash over that fetched directory's actual `kryos.toml` + `src/**.kry` (+ `stdlib/**.kry`) content, in deterministic sorted-path order (`kryos-package/src/registry.rs::content_checksum`), and compares it against the checksum recorded for that exact version in the registry index. A mismatch OR a missing checksum is rejected and the offending cache entry is deleted -- this verification runs on every install, including a cache hit, so a package mutated on disk after a previous install can never be silently reused. `kryos.lock` now records the verified checksum (previously always `None`). `pkg add` still writes a wildcard version constraint (`name = "*"`) into `kryos.toml` by default, so pinning a specific version still depends on committing `kryos.lock` -- but the CONTENT behind whatever version is locked is now cryptographically checked on every install, closing the "compromised/force-pushed registry silently changes an already-published version" hole. `kryos pkg info`/`show` still displays the checksum for humans. `copy_dir_all` also now refuses any symlink entry while copying a fetched package (defense against a malicious commit using a symlink to pull unrelated files into the local cache). See `tools/loop/LEDGER.md` item 1b (CLOSED table) for the live repro, fix, and evidence.

**`kryos pkg install` now PINS to a committed `kryos.lock` instead of silently re-resolving it (fixed; LEDGER item 12 CLOSED).** If `kryos.lock` exists and already covers every dependency in `kryos.toml` (name present, and for a `Remote` dep its locked version still satisfies the manifest's requirement), `install` fetches EXACTLY what the lock says, checksum-verifies it, and does not touch the registry index or rewrite the lock at all -- matching `npm ci` / `cargo install --locked` semantics. A newly added manifest dependency the lock doesn't cover yet still triggers a fresh resolve for the whole graph (so `kryos pkg add` + `install` stays usable without a separate `update` step first), but any package that WAS already locked and drifts as a side effect of that resolve is reported with an explicit `warning: ... drifted from the committed kryos.lock` line rather than silently overwritten. `kryos pkg update` remains the explicit, deliberate re-resolve command.

**An explicit `git = "..."` source (or the `kryos pkg add github:org/repo@ver` CLI form) is now actually honored instead of being silently discarded in favor of a by-name registry lookup (fixed; LEDGER item 17 CLOSED).** `install`/`update` now check whether a `Remote` dependency's `source` field is non-empty; if so, they fetch that EXACT source (`kryos_package::fetch::fetch_explicit_source`) instead of ever calling the registry client for that dependency's name -- so a project pinning a private fork or a security-patched mirror under a name that also exists in the public registry gets its OWN declared source, not the official package. There is no registry index behind an explicit source, so there is no pre-published checksum to check the first fetch against -- trust is established on first fetch (the same model a `cargo` git dependency with no `rev` pin uses), and the computed checksum is written into `kryos.lock` so item 12's pinned-install path re-verifies it on every later install instead of re-trusting the source blindly. Fetching an explicit source that is unreachable, private, or simply wrong now fails FAST with a real git error naming the source, instead of a misleading "not found in registry". A live bug found WHILE fixing this and fixed alongside it: a git clone of an inaccessible repo could hang indefinitely waiting on an interactive credential/GUI-askpass prompt this headless tool can never answer -- every `git clone`/`git pull` this crate runs now passes `GIT_TERMINAL_PROMPT=0` plus `-c credential.helper=` plus `-c core.askpass=` together (all three were required; any one alone still hung on this machine) to fail fast instead.

## Tooling

| Command                       | Purpose                                                        |
| ------------------------------ | ---------------------------------------------------------------- |
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

## Gotchas Claude needs to know (compressed index)

Full histories, root causes, commit hashes, and resolved-bug narratives for every item below live in `docs/claude/FULL-REFERENCE.md` — grep it by number or topic. This list states only the live rule/workaround, present tense, nothing else. Entries that were purely historical (RESOLVED with no remaining behavioral rule) are omitted here entirely; they still exist in the reference.

1. String interpolation: EVERY string literal interpolates (`"hi {x}"`, braces not `${}`); a literal brace needs `{{`/`}}` or `\{`/`\}` escaping; build JSON/set-notation strings with `+` instead. Map key lookup is `contains(m, k)` for both str- and int-keyed maps.
2. `if let` / `while let` / `let ... else` all work (desugar to `match`); `let ... else`'s pattern must be a refutable enum pattern, and the `else` block's bindings stay in scope after it.
3. Both `elif` and `else if` work (self-host source uses `elif`); a trailing `if`/`elif`/`else` is a block's VALUE, usable as a `let` initializer or a match-arm tail.
4. No `null`/`nil` — use `Option<T>` from `std::option`.
5. Tuple destructuring (`let (a, b) = ..`) and field access (`t.0`) work on both backends, mixed element types included.
6. `arr[i]` accepts any integer index type (i8..u64) on native backends; the experimental `--backend wasm` still needs an explicit `as i64`.
7. `file_write` does not create parent directories — call `create_dir(parent)` first.
8. Top-level `let mut` may only call pure builtins (`env_get`, `args`); move calls to user functions into `main()`.
9. Glob imports (`use std::os::*`) work and pull in every public symbol.
10. `kryos run` (Cranelift) supports fewer codegen paths than LLVM — if `run` fails but `build --release` works, prefer `build --release`.
11. Closures: `|x| ...` / `|x: i64| ...` only, never `(x) => ..`. A read-only capture is by reference (later outer mutations are visible); a captured var the closure ITSELF mutates is captured by move. A closure STORED in a struct field/array element (escaping) snapshots a heap (`[T]`/`map`/`str`) capture at store time — reassigning the source later does not update it. A self-referential closure built via reassignment (`fact = |n| .. fact(n-1) ..`) captures the OLD value; use a named recursive `fn` instead. Mutating a captured MAP by key, or a nested field reached through a captured array-of-structs, writes through to the outer value — only a whole-binding mutation (`x = ..`, `s.field = ..`, `arr[i] = ..`) isolates.
12. `?` works on both backends for `Result<T, E>`-returning functions.
13. Always annotate `Result<T, E>`/`Option<T>` on signatures — a bare `Result`/`Option` erases the payload to i64.
14. Or-patterns in match arms (`1 | 2 | 3 => ..`) work; alternatives must be non-binding (literals or bare enum variants).
15. Matching a tuple value (`match p { (0, 0) => .., (x, 7) => x, _ => .. }`) works, string-literal elements included.
16. No struct-style enum variants (`A { x: i64 }`) — use a tuple variant `A(i64)`. A bare unqualified nullary variant in an expression position (not a match pattern) silently resolves to whichever enum declared/imported it FIRST when two enums share a variant name — prefer `Opt::None`/`Color.Red` qualified form when more than one imported enum shares a variant name.
17. Generic `impl`/`impl<T>` methods, multi-instantiation of the same generic method, chained `.get().get()`, and a bare `-> T`/compound (`(T, i64)`, `[T]`, `map<K, T>`) self-field-passthrough return all carry the real per-instantiation type now — no manual annotation needed.
18. Integer `/0` and `%0` panic; `i64::MIN / -1` / `% -1` panics; out-of-range float→int casts SATURATE (no UB) on both backends. `as` binds tighter than unary `-` — parenthesize a negative cast operand: `(-1.0e300) as i64`. `parse_float("-0.0")` loses its sign on AOT only; a NaN's sign bit differs by backend (only visible via bit-reinterpret, or `sort()` on an `[f64]` containing NaN, which orders differently by backend) — never depend on NaN sign or sorted NaN position. A user `fn` now always shadows a same-named builtin (`sin`, `abs`, ...) on both backends. Residual: on AOT only, a CONSTANT-argument call to a libm-named `(f64) -> f64` fn gets LLVM-constant-folded to libm's value, ignoring your body — pass a runtime argument to avoid it.
19. `std::iter` HOFs (`fold`, `reduce`, `scan`, `map_indexed`, `filter`, `map`, ...) must be imported (not global builtins) and infer closure param types from the HOF signature; a custom HOF should declare a real `f: fn(T, U) -> U` param, not bare `f: fn`. Substring search is `std::string::find`, not `index_of`.
20. Closures keep their real `str`/struct/array/`f64` type across an HOF boundary (params and return), with no annotation needed, on both backends.
22. Known remaining limitations (original gotcha #21 was fully resolved with no residual rule — dropped; numbering below matches FULL-REFERENCE.md):
    - `push(arr, v)` grows the shared buffer IN PLACE — always reassign `arr = push(arr, v)`; never read a pre-push alias (`let b = push(a, v)` then reading `a` is undefined). Same applies to `std::heap`/`queue`/`stack`/`deque` ops built on `push`.
    - Importing a name that collides with a global builtin that ANOTHER imported stdlib module uses internally breaks that module (one flat namespace) — don't selectively import a name (e.g. `contains` from `std::trie`/`set`/`interval`) that shadows a builtin another imported module needs.
    - `to_string` of an array/tuple/map with no custom method returns a placeholder (`<array>`/`<tuple>`/`<map>`), not its contents — iterate and build the string yourself with `+` and per-element `to_string`.
    - `extern` to a non-`kryos_*` symbol (real C-library FFI) is rejected at check time (`E0508`) — arbitrary C FFI is not implemented.
    - Raw-memory builtins (`alloc`, `free_bytes`, `ptr_read_i64`, `ptr_write_i64`, `ptr_byte_at`, `ptr_set_byte`, `str_to_ptr`) need NO capability and are not gated by `unsafe {}` — treat them as an ungated unsafe surface in a security-sensitive program.
    - `std::json` keeps in-range integers as `JsonValue.Int(i64)` (exact) vs `Number(f64)` for anything with a fraction/exponent or an i64-overflowing integer — `is_number`/`to_int`/`to_float` handle both, but code that `match`es `JsonValue` directly needs an `Int` arm.
    - Self-referential structs (`struct Tree { kids: [Tree] }`) work on both backends; a cycle of plain (non-array) struct fields is still a compile error (infinite size).
    - `cargo build -p kryos-cli` does NOT rebuild `kryos_rt.lib`/`kryos_stdlib_native.lib` (the AOT-linked staticlibs) — run a full `cargo build --release` before measuring any kryos-rt/kryos-stdlib-native runtime change, or you're testing the old runtime.
    - One known leak remains: passing a struct with HEAP fields across a CALL boundary leaks ~85 bytes/call (not method-specific — a free function leaks the same as a method, and a chain multiplies it). Workaround in a hot loop: read fields directly instead of passing/returning the struct, keep heap data out of structs crossing calls, or reuse one instance.
    - Parameters are BORROWS — the caller owns and frees a `str`/`[T]`/`map` argument; the callee just reads it. STRUCT arguments are the exception and keep ownership-transfer semantics (shared by `spawn`/container reads).
    - Building a string with `s = s + chunk` in a loop is O(n²) — use `std::string::string_builder()` (`sb.append(..)`, then `sb.build()` once) for O(n).
    - A blocking `recv(ch)` on a CLOSED, drained channel returns `0`, indistinguishable from a real `send(ch, 0)` — use `chan_try_recv`/`chan_is_closed` to tell them apart.
    - A closure captured by `spawn` is SHARED (not snapshotted) — two `spawn` blocks referencing the same mutating closure act on one logical piece of shared state, the same way a `Mutex`/`atomic_int`/actor does. This is now SAFE, not merely possible: every call to a mutating closure's underlying function is serialized under a per-closure lock (LEDGER item 7b, CLOSED), so concurrent calls converge on the mathematically correct result with no lost updates — see `docs/09-concurrency.md`. It serializes the whole call (correctness over throughput), so for a hot shared counter `std::sync::atomic_int()` is still faster.
    - Runtime panics (div-by-zero, index OOB, `file_read` on a missing file) are NOT catchable by `try`/`catch` — only `throw` is. Guard the precondition or use a `Result`-returning wrapper.
    - `comptime {}` blocks run at RUNTIME like an ordinary block (not a compile-time evaluator yet) — don't rely on them for isolation, compile-time constants, or side-effect suppression, despite what `docs/11-comptime.md` claims.
    - Shift amount >= the operand's bit width is hardware-dependent (masks mod width) — keep shift amounts strictly less than the operand's width.
    - Hand-declaring a `kryos_*` runtime extern with a `str`/array/map/struct-typed param or return is rejected at check time (`E0508`) — use the real builtin (e.g. `env_get(...)`) instead of hand-declaring the runtime symbol.
    - `std::collections::List<T>`/`Set<T>`: annotate the `let` (`let ls: List<str> = List.new()`) so `T` binds correctly from construction.
    - `i128`/`u128` are not functional — use `i64`/`u64`; using them is now a clean `E0110` compile error, not a crash.
    - `pop()` on an empty array returns `0` (not a panic) — guard with `len(a) > 0` first if a real zero must be distinguishable.
    - `base64_encode`/`chr`/`byte_at` use a latin-1 BYTE-BUFFER model (codepoints 0-255 == bytes), not UTF-8 text — encoding a string with any codepoint > 0xFF silently truncates it to its low byte. `len()` overcounts once a byte-buffer string holds a logical byte >= 0x80 — use `std::utf8::codepoint_count` or `std::bytes` (codepoint-indexed) instead of `len`/raw indexing on such buffers.
    - `substr()` can split a multibyte codepoint and produce invalid UTF-8 from ordinary byte-index arithmetic; downstream string ops now PANIC with a clear message on invalid UTF-8 instead of silently returning `""`/`false`. Check `std::utf8::is_valid(s)` first if a slice might land mid-codepoint.
    - A block-tail closure literal CAN capture any earlier `let` binding in the same scope with no special restriction — an apparent capture-scope error here is almost always the `||`-continuation parse trap (Hard Rule 1), not a real limitation.
    - `any` is type-erased to i64 — `to_string`/`std::fmt::format` on a `str`/`f64` value routed through `[any]`/`args: [any]` prints the raw bit pattern, not the value. Build such strings manually with `+` and per-type `to_string`/`float_fixed` instead. The element-typed `std::iter` HOFs (`map`, `filter`, `find`, `sum`, `enumerate`, `zip`, `unzip`, `chunks`, `windows`, `flat_map`, ...) ARE generic and preserve `str`/`f64` correctly.
    - A `std::collections` List/Stack/Queue/Deque `push` shares its backing buffer — don't branch two derived collections off one parent (`let a = xs.push(1); let b = xs.push(2)` aliases). Use the single-threaded form `xs = xs.push(v)`.
    - `gcd`/`lcm` of `i64::MIN` cannot return the correct positive magnitude (inherent i64 range limit, `|i64::MIN|` overflows) — every other input is correct.
    - `dyn Trait` cannot be stored INSIDE a container (`[dyn Shape]`, `Option<dyn Shape>`, tuple/map values) — clean `E0110` at check time; use an enum + `match` instead. A single `dyn Trait` value/param/field/return/let-binding works and dispatches correctly. Residual: a generic-struct `impl<T> Trait for Box<T>` called through `dyn` with non-i64 `T` reads a `str`/`f64` field as a raw bit pattern (use a non-generic impl per concrete type, or call directly); a `dyn` method returning a by-value aggregate (struct/tuple/enum/`Option`/`Result`) is a clean `E0110` — return a scalar or heap handle instead.
    - Narrow-int (`u8`/`i8`/`u16`/etc.) literals are not range-checked at compile time — an out-of-range literal truncates silently.
23. Struct param/copy semantics (still backend-divergent in places):
    - All-scalar `@copy` struct params are copied at function entry on BOTH backends now (plain-data structs agree).
    - Heap-bearing `@copy` struct PARAMS still diverge: JIT aliases the caller's pointer, AOT passes a shallow byval copy (deliberate — a JIT-side entry copy would deep-clone-per-call on the self-host parser's hot path).
    - Non-`@copy` structs still diverge on params: JIT aliases, AOT copies (mostly unobservable; the historical crash case is fixed).
    - `@copy` variable-to-variable ASSIGNMENT (`let c = b`) deep-clones str/array/map fields on both backends now — each copy owns its data, but the clone leaks under the no-op `@copy` drop model (a known leak-on-copy, consistent on both backends).
    - The deep-copy contract is assignment-only: reading a struct OUT of a collection (`let a = m["k"]`, `let a = arr[0]`) returns a SHARED handle on both backends — a later mutation through the container (`m["k"].name = ..`) IS visible through `a`. For a real snapshot, copy field-by-field (`let a = S { name: m["k"].name }`).
    - Genuine backend DIVERGENCE (not fixed): mutating THROUGH an alias itself (`a.tag = a.tag + "!"` after `let a = arr[0]`) then reading a SECOND alias of the same source shows the mutation on every alias under Cranelift/JIT but only through `a` itself under LLVM/AOT. Never rely on a second alias observing a mutation made through a first alias — read the field immediately after taking the alias, or mutate through a freshly re-evaluated `container[key].field = ..` chained target (which does agree on both backends).
    - Portable pattern: copy the param into a `let mut` local before mutating, or return the modified struct — never rely on cross-call visibility of a struct's heap-field mutation.

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
