# Cookbook 12 · Structured logging

`std::log` emits one line per record to stderr in `LEVEL ts=<epoch_secs> msg="..." k=v k=v` format. Easy to pipe through `awk` / `grep` / `jq`.

## The program

```kryos
use std::log::{log_set_level, log_emit}

@capabilities(io)
fn main() {
    // Default min-level is 2 (INFO). Set to 1 to see DEBUG.
    log_set_level(1)

    log_emit(2, "boot", "build=v4.3.0 pid=12345")
    log_emit(1, "config loaded", "path=/etc/myapp.toml")
    log_emit(2, "starting workers", "count=4")

    // Simulate work
    process_request(42)
    process_request(99)

    log_emit(2, "shutting down", "uptime_secs=3600")
}

@capabilities(io)
fn process_request(id: i64) {
    log_emit(2, "request started", "id=" + to_string(id))
    // ... do work ...
    log_emit(2, "request done",    "id=" + to_string(id) + " status=200")
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

- Set `KRYOS_LOG_LEVEL=3` in production to drop everything below WARN.
- Pipe to `jq -R 'split(" ") | ...'` if you want JSON for ingestion.
- The `k=v` payload is verbatim — escape `"` and spaces yourself.
