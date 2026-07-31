# Capabilities

> **Implementation Status:** The `@capabilities(...)` annotation is parsed and the compile-time capability checker (`kryos-capabilities` crate) is fully implemented. It enforces: functions must declare capabilities matching the stdlib modules and builtins they use, child scopes cannot exceed parent capabilities (attenuation), and extern blocks require the `ffi` capability. The actual capability variants are: `net` (coarse), `net:http`, `net:tcp`, `io`/`fs` (coarse, aliases), `fs:read`, `fs:write`, `ffi`, `compute`, `crypto`, `process`, `env`, `term`, `db`, `time`, `all`. Three enforcement modes are implemented — `permissive`, `inferred` (deny-by-default with interior inference), and `strict` — selectable via `--capabilities-mode` or `[capabilities] mode` in `kryos.toml`; `kryos new` defaults new projects to `inferred`. Enforcement is sound for DIRECT calls: free-function calls, method/static dispatch, and a gated BUILTIN passed and invoked as a first-class value. **It is NOT currently sound against closure/fn-value indirection — see "Known limitation" below before relying on this for a security boundary.** Runtime enforcement, audit logging, and sandboxing APIs described in some earlier drafts are **not yet implemented** (enforcement is entirely compile-time). Note: `env_get`/`env_set` require the `process` capability (reading the environment can exfiltrate secrets), not ambient.

Capabilities are Kryos's security model. Every function declares exactly what system resources it needs -- filesystem access, network connections, process spawning, FFI calls. If a function tries to use something it did not declare, the program fails at compile time. Not at runtime, not with a warning -- it does not compile.

This is the opposite of how most languages work. In JavaScript or Go, any function can open a file, make a network request, or call `eval`. You only find out about unauthorized access when something goes wrong in production. Kryos inverts that: you see every capability a program uses before you run it.

## The model: three enforcement modes

Enforcement has three modes, selected with `--capabilities-mode=<mode>` on
`kryos check` / `kryos build`, or per project via `[capabilities] mode` in
`kryos.toml`. `kryos new` scaffolds `mode = "inferred"`, so **new projects are
deny-by-default from day one.**

| Mode | What it does | When |
|---|---|---|
| `permissive` | Only functions carrying a `@capabilities(...)` annotation are checked. An unannotated function is unconstrained. Capabilities are advisory. | Opt-in for scratch/legacy code via `--capabilities-mode=permissive`. |
| `inferred` | **Deny-by-default at the boundary, with interior inference.** `main` — and any annotated function — must actually hold every capability its code *transitively* uses. Interior helpers need no annotation: the compiler infers each one's capability set as the union of what it and its callees require. So an unannotated `main` that (directly or through helpers) writes a file is rejected: declare `@capabilities(fs:write)` on `main`. **Recommended, and the `kryos new` default.** | New projects; production code. |
| `strict` | Every function is checked as if annotated with exactly its declaration (empty unless declared). Every DIRECT gated builtin call from an unannotated function is an error. This is `--strict-capabilities`. **Not sound against closure/fn-value indirection — see "Known limitation" below.** | Maximum scrutiny; security-critical libraries. |

The key property of `inferred` mode, **with one important gap (read "Known
limitation" below first)**: reading `main`'s annotation is *meant* to tell you
the program's entire authority. For direct paths — direct builtin calls,
method/static dispatch (`obj.write()`), and a gated builtin passed BY NAME as
a first-class value (`apply(file_write, ...)`) — this holds today: all are
accounted for. It does **not** hold when authority is laundered through an
ordinary closure (a function returning `|| file_read(path)` and calling it
through an unnamed `fn`-typed parameter elsewhere) — the checker tracks
authority by callee NAME, and a call through a local/parameter of function
type is invisible to it regardless of mode. See "Known limitation" below.

```bash
kryos check --capabilities-mode=inferred src/main.kry   # deny-by-default
kryos check --strict-capabilities src/main.kry          # every fn must declare
kryos build --release --capabilities-mode=inferred src/main.kry
```

In a project, set it once in `kryos.toml` (a fresh `kryos new` already does):

```toml
[capabilities]
mode = "inferred"
```

## Known limitation: closure/fn-value indirection is NOT enforced (read this before trusting it with secrets)

**If you are evaluating Kryos for a use case where capability enforcement is
the thing standing between untrusted code and a secret (an embedded agent
that must not exfiltrate a password, an API key, a token), read this section
first. It applies in EVERY mode, including `--strict-capabilities`.**

The checker (`kryos-capabilities/src/checker.rs`) tracks authority **by
callee name** — a `HashMap<String, CapabilitySet>` keyed by function/actor
names, checked at every direct call site. A call through a **value** of
function type — a closure captured in a local, a parameter typed `fn(...) ->
...`, a struct field, an array element, anything that isn't a literal named
function or builtin — resolves to nothing in that map, so it is invisible to
enforcement **regardless of what the closure actually does at runtime**:

```
@capabilities(fs:read)
fn make_secret_reader(path: str) -> fn() -> str {
    return || file_read(path)
}

// Zero capabilities declared. Its body only ever calls a PARAMETER of
// function type -- never a NAMED builtin or NAMED function -- so it is
// invisible to the by-name checker in every mode.
fn zero_cap_tool(reader: fn() -> str) -> str {
    return reader()
}

@capabilities(fs:read)
fn main() {
    let reader = make_secret_reader("secret.txt")
    println(zero_cap_tool(reader))   // reads the file. zero_cap_tool declares NO capabilities.
}
```

This compiles clean and runs under `--capabilities-mode=inferred` (the
default) AND `--strict-capabilities` -- `zero_cap_tool` is not flagged in
either mode, and `kryos audit` also misses it (it reads the same by-name
map). The gap needs no raw memory, no FFI, no special builtin -- just
ordinary closures, one of the language's most idiomatic features (every
`std::iter` higher-order function and callback-shaped API uses them). A
direct call to a gated builtin (`file_read(path)` written literally inside
`zero_cap_tool`) IS correctly rejected — this is specifically an
indirection gap, not a broadly broken checker.

**There is no workaround that preserves least-privilege.** The conservative
fix (treat any call through a non-directly-named fn-typed value as requiring
`Capability::All`) was evaluated and rejected: closures passed to
higher-order functions are pervasive and idiomatic in Kryos, so it would
force nearly every callback-shaped call site in the language and its own
self-host compiler to declare `all`, defeating least-privilege as
completely as the escape itself.

**Status: open, unfixed, tracked as the top item in
[`tools/loop/LEDGER.md`](../tools/loop/LEDGER.md).** The real fix is a
call-site-sensitive interprocedural capability analysis for fn-typed
arguments (closer in scope to adding a new kind of generic bound than to an
incremental patch) — sketched in the ledger for whoever picks it up. Until
it lands: **do not treat `@capabilities`/`--strict-capabilities` as a
guarantee against a hostile or buggy piece of code that can construct or
receive a closure.** It is sound today only against code that calls gated
authority directly by name.

Under strict mode, a pure function like this is fine -- it calls no capability-gated builtins:

```
fn square(x: i64) -> i64 {
    return x * x
}
```

But calling `file_read` in an unannotated function is a compile error:

```
fn bad_function() -> str {
    return file_read("secret.txt")
}
```

Error output:

```
error[E0505]: builtin `file_read` requires `fs:read` capability
 --> src/main.kry:2:12
  2 |     return file_read("secret.txt")
   |            ^^^^^^^^^^^^^^^^^^^^^^^^ requires `fs:read`
  = note: add `@capabilities(fs:read)` to the enclosing function or actor
```

## Declaring Capabilities

Use the `@capabilities(...)` attribute on a function:

```
@capabilities(fs:read)
fn load_config(path: str) -> str {
    return file_read(path)
}
```

For pure computation -- math, string manipulation, data structures -- no annotation is needed. `println` and `print` are also ambient (no capability required).

For real I/O, declare what you need:

```
@capabilities(net:http)
fn fetch_data(url: str) -> str {
    return https_get(url)   // https_get; not http_get (does not exist as a builtin)
}

@capabilities(fs:read)
fn read_config(path: str) -> str {
    return file_read(path)
}
```

### Combining Capabilities

A function can declare multiple capabilities:

```
@capabilities(net:http, fs:write)
fn download_to_file(url: str, path: str) {
    let data = https_get(url)
    file_write(path, data)
}
```

## All Capability Types

### net (coarse)

Grants all network sub-capabilities: `net:http` and `net:tcp`. Use the narrowest sub-cap that fits.

### net:http

HTTP(S) client and server operations.

```
@capabilities(net:http)
fn fetch(url: str) -> str {
    return https_get(url)
}
```

### net:tcp

Raw TCP connections, TLS, and unix-domain sockets.

```
@capabilities(net:tcp)
fn connect(host: str, port: i64) {
    tcp_connect(host, port)
}
```

### io / fs (coarse)

File I/O -- grants both `fs:read` and `fs:write`. `io` and `fs` are aliases for the same coarse capability (back-compat: `io` is the legacy spelling). Use the narrower sub-cap when possible.

### fs:read

Read-only file access.

```
@capabilities(fs:read)
fn load(path: str) -> str {
    return file_read(path)
}
```

### fs:write

Write, create, and mutate files and directories.

```
@capabilities(fs:write)
fn save(path: str, data: str) {
    file_write(path, data)
}
```

### process

Process spawning and environment variable access. Environment variables are gated here because they can contain secrets. The top-level `env_get` builtin requires this capability. Process spawning (`exec`, `spawn_process`) and `env_set` are modeled under this capability and surface through the `std::process` stdlib module.

```
@capabilities(process)
fn get_home() -> str {
    return env_get("HOME")
}
```

Note: `exit` and `abort` terminate the current process only and are **ambient** -- no capability required (same philosophy as Rust's `process::exit`).

### env

Reserved for future use as a narrower environment-variable-only split from `process`. Currently `env_get` / `env_set` map to `process`.

### ffi

Foreign function interface -- calling code written in other languages. Required on `extern` blocks.

```
@capabilities(ffi)
extern {
    fn my_c_function(x: i64) -> i64
}
```

### compute

Heavy computation including GPU dispatch and SIMD intrinsics. Pure arithmetic, string manipulation, and data structure operations are ambient and do not require `compute`.

### crypto

Cryptographic operations: hashing, signing, encrypting.

```
@capabilities(crypto)
fn hash_data(data: str) -> str {
    return sha256(data)
}
```

### term

Terminal control -- raw mode, cursor positioning, terminal size queries. The `term` capability type is recognized in `@capabilities(...)` and enforced on `use std::term::*` imports. Top-level `term_*` builtins are defined in the model but surface through the `std::term` stdlib module rather than as standalone builtins.

### db

Database access -- queries and transactions via the db stdlib module.

### time

System clock access. Currently the `time` variant is reserved for a future deterministic-execution mode. `time_now` and `sleep` are **ambient** today -- no capability required. (`time_millis` is defined in the model for future use but is not currently a top-level builtin; use `time_now` instead.)

### all

Grants every capability. Use only in top-level entry points or trusted shells. Auditable: declaring `all` is visible in every code review and capability audit.

## The Capability Hierarchy

Capabilities narrow downward -- they never elevate. If a function has `@capabilities(fs:read)`, any function it calls can have `fs:read` or less. It cannot call a function that requires `fs:write`.

```
@capabilities(fs:read)
fn safe_load(path: str) -> str {
    return file_read(path)
}

@capabilities(fs:write)
fn save(path: str, data: str) {
    file_write(path, data)
}

@capabilities(fs:read)
fn process_config(path: str) {
    let data = safe_load(path)        // OK -- same capability
    // save(path, data)               // COMPILE ERROR -- would elevate
}
```

Coarse caps satisfy their sub-caps (back-compat). A function declaring `@capabilities(net)` may call both `https_get` (needs `net:http`) and `tcp_connect` (needs `net:tcp`). The reverse does not hold: `net:http` does not grant `net:tcp`.

## Compile-Time Enforcement

The capability checker runs as a static analysis pass before codegen. Violations are errors, not warnings.

Example: calling `file_read` inside a function that declared only `net`:

```
@capabilities(net)
fn bad_function() -> str {
    return file_read("secret.txt")
}
```

Actual error output:

```
error[E0505]: builtin `file_read` requires `fs:read` capability
 --> src/main.kry:3:12
  3 |     return file_read("secret.txt")
   |            ^^^^^^^^^^^^^^^^^^^^^^^^ requires `fs:read`
  = note: add `@capabilities(fs:read)` to the enclosing function or actor
```

The error code `E0505` means "builtin requires a capability not in the declared set." Propagation errors (calling a function that has more capabilities than the caller) use `E0507`. All capability error codes (`E0501`-`E0507`) are explainable via `kryos explain <code>`.

## Builtin Function Capability Requirements

**Ambient (no capability required):**
`println`, `print`, `eprintln`, `len`, `range`, `to_string`, `abs`, `type_of`, `parse_int`, `parse_float`, `str`, `sqrt`, `sin`, `cos`, `tan`, `log`, `pow`, `floor`, `ceil`, `min`, `max`, `assert`, `push`, `pop`, `substr`, `contains`, `char_code`, `exit`, `abort`, `time_now`, `sleep`, `file_exists`

**fs:read** (or coarse `io`/`fs`):
`file_read`, `read_file`

**fs:write** (or coarse `io`/`fs`):
`file_write`, `write_file`, `create_dir`, `remove_file`, `remove_dir`, `copy_file`, `rename_file`

**net:http** (or coarse `net`):
`https_get`, `http2_get`, `http2_post`, `http2_request`

**net:tcp** (or coarse `net`):
`tcp_connect`, `tcp_listen`, `tcp_accept`, `tcp_send`, `tcp_recv`, `tls_server_config`, `tls_accept`, `tls_send`, `tls_recv`, `tls_close`, `uds_connect`, `uds_bind`, `uds_accept`, `uds_send`, `uds_recv`, `uds_close`

**net** (coarse only -- straddles connect + protocol):
`pg_connect`, `pg_exec`, `pg_query`, `pg_close`, `ws_accept_key`, `ws_encode_text`, `ws_encode_binary`, `ws_encode_close`, `ws_encode_ping`, `ws_encode_pong`, `ws_unmask`, `ws_read_frame`

**process**:
`env_get` (top-level builtin); `env_set`, `exec`, `spawn_process` are recognized by the capability checker but surface through the `std::process` stdlib module rather than as standalone builtins

**term** (via `std::term` module -- not standalone builtins):
`term_clear`, `term_raw_mode`, `term_size`

**crypto**:
`sha256`, `sha512`, `random_bytes`, `hmac_sha256`

## Real-World Patterns

### Web server with minimal privileges

```
@capabilities(net:tcp, fs:read)
fn serve(port: i64) {
    let config = file_read("config.toml")
    tcp_listen("0.0.0.0", port)
    // Can read files for config, can accept connections.
    // Cannot write files, cannot spawn processes.
}
```

### Data pipeline with read-only input

```
@capabilities(fs:read)
fn load_data(path: str) -> [str] {
    let raw = file_read(path)
    return raw.split("\n")
}

@capabilities(fs:write)
fn save_results(path: str, results: [str]) {
    file_write(path, join("\n", results))
}

@capabilities(io)
fn pipeline(input: str, output: str) {
    let data = load_data(input)
    // ... process data ...
    save_results(output, data)
}
```

The `load_data` function cannot accidentally write to disk. The `save_results` function cannot read arbitrary files. Each function has exactly the access it needs.

### Pure computation (no annotation needed)

```
fn transform(xs: [i64]) -> [i64] {
    // No annotation needed -- only uses ambient builtins.
    let mut out: [i64] = []
    for x in xs {
        out = push(out, x * 2)
    }
    return out
}
```

## Why This Matters

Most security vulnerabilities come from code doing something it was never intended to do. A logging library that makes network calls. A template engine that reads arbitrary files. A math utility that spawns processes.

Capabilities make these violations impossible. When you look at a function's `@capabilities` declaration, you know exactly what it can do. When you audit a program, you get a complete map of every capability used by every function.

This is not just defense -- it is documentation. Reading `@capabilities(fs:read)` tells you more about a function's behavior than reading its entire implementation.
