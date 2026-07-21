# Cookbook 12 · Structured logging

`std::log` emits one line per record to stdout (via `println` — Kryos has no dedicated stderr builtin) in `LEVEL msg="..."[ k=v k=v]` format. There is no `ts=` field; the trailing `k=v` payload is only appended when you pass a non-empty `kv` string. Easy to pipe through `awk` / `grep` / `jq`.

## The program

```kryos
use std::log::{new_level, set_level, emit, DEBUG, INFO}

fn process_request(log: [i64], id: i64) {
    emit(log, INFO(), "request started", "id=" + to_string(id))
    // ... do work ...
    emit(log, INFO(), "request done", "id=" + to_string(id) + " status=200")
}

fn main() {
    // Create a level holder; default min-level is INFO (2). Set to DEBUG (1).
    let log = new_level()
    set_level(log, DEBUG())

    emit(log, INFO(), "boot", "build=v4.3.0")
    emit(log, DEBUG(), "config loaded", "path=/etc/myapp.toml")
    emit(log, INFO(), "starting workers", "count=4")

    process_request(log, 42)
    process_request(log, 99)

    emit(log, INFO(), "shutting down", "uptime_secs=3600")
}
```

## Levels

| Constant | Severity | Use for |
| --- | --- | --- |
| `0` | TRACE | very-fine-grained internal flow |
| `1` | DEBUG | dev-only insights |
| `2` | INFO | normal operational events |
| `3` | WARN | recoverable issues |
| `4` | ERROR | something failed but the program continues |
| `5` | FATAL | the program is about to exit |

## Tips

- `std::log` has no environment-variable support — the min level is set purely in code via `set_level(log, WARN())`. Read your own env var (`env_get("LOG_LEVEL")`, needs the `process` capability) and map it to a level constant if you want that behavior.
- Pipe to `jq -R 'split(" ") | ...'` if you want JSON for ingestion.
- The `k=v` payload is verbatim — escape `"` and spaces yourself.
