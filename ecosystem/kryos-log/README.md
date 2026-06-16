# kryos-log

Structured, capability-tagged JSON-line logging for Kryos.

`kryos-log` emits one JSON object per log line (the JSON Lines / ndjson
convention). Each record carries a level, a message, and an ordered list of
`(key, value)` string fields. Field values pass through a redaction hook before
serialization, so secret-shaped values never reach the sink. A record can also
carry a `cap_tag` describing the capability surface it was emitted under, read
from a sidecar `caps.json`.

```
{"level":"info","msg":"user authenticated","user":"alice","password":"[REDACTED]"}
```

## Why string-only fields

A logger's fields are string key/value pairs, so every value here is a `str`.
That is a deliberate design choice: it keeps records portable (the JSON is
assembled by a hand-written string builder that leans only on the
always-available string builtins, with no JSON runtime linked) and it avoids
threading any `f64` through a function argument, which sidesteps a known JIT
verifier issue on float-typed parameters. Numeric fields are passed as their
string form (`("port", "8080")`).

## MVP scope

This is the MVP. It implements exactly:

- **Leveled, structured records.** `debug` / `info` / `warn` / `error`, each
  taking a message and `[(str, str)]` fields.
- **JSON-line output.** `emit_line` (pure, in-memory, `@capabilities()`)
  returns the serialized line; `log_to_file` (`@capabilities(io)`) appends a
  record plus a newline to an ndjson file.
- **Redaction hook.** Field values are redacted when the KEY looks sensitive
  (`password`, `token`, `secret`, `api_key`, `authorization`, ...) or the VALUE
  looks like a secret (a `Bearer`/`Basic` header, an `sk-`/`sk_live_`/`sk_test_`
  key prefix, or a JWT-shaped blob). The placeholder is the constant
  `[REDACTED]`, so redacted lines stay valid JSON.
- **Capability tag.** `cap_tag_from_sidecar` reads a `caps.json` sidecar of the
  shape `{"version":1,"declared":["compute","io"]}` and produces a compact tag
  (`"compute,io"`) that rides into the record as a `cap_tag` member. A missing
  or malformed sidecar yields an empty tag, which is omitted from the output.

## Deferred (out of scope for the MVP)

Async sinks, log rotation, sampling, and OTLP export are intentionally not here.

## Layout

```
src/json_line.kry   JSON string assembler (escape, quote, member, array)
src/redact.kry      redaction hook (key + value heuristics, redact_value)
src/cap_tag.kry     caps.json sidecar -> compact cap_tag string
src/log.kry         levels, record serialization, in-memory + file sinks
tests/test_log.kry  serialization, redaction, and cap_tag tests
demo_log.kry        end-to-end demonstration
```

## Public API (src/log.kry)

| Function | Capability | Returns | Purpose |
| --- | --- | --- | --- |
| `info(msg, fields)` (also `debug`/`warn`/`error`) | `compute` | `str` | serialize a leveled record |
| `emit_line(level, msg, fields)` | `compute` | `str` | serialize at an arbitrary level |
| `emit_line_tagged(level, msg, fields, cap_tag)` | `compute` | `str` | as above, with a capability tag |
| `log_to_file(path, level, msg, fields, cap_tag)` | `io` | `i64` | append one ndjson record; returns new file size |

`redact_value(key, value) -> str` (src/redact.kry) is the standalone redaction
hook; `cap_tag_from_sidecar(path) -> str` (src/cap_tag.kry) reads a tag off
disk.

## Run

```bash
# from the repository root
kryos test --path ecosystem/kryos-log
kryos run ecosystem/kryos-log/demo_log.kry
```

## License

Apache-2.0. See `LICENSE`.
