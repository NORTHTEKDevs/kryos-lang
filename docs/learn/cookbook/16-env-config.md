# Cookbook 16 · Reading config from environment variables

Production services typically read configuration from env vars rather than files. Kryos's `env_get` builtin returns `""` for unset keys, so wrap it with a defaulting helper.

## The program

```kryos
@capabilities(process)
fn main() {
    let port = env_int("PORT", 8080)
    let log_level = env_str("LOG_LEVEL", "info")
    let bind = env_str("BIND", "0.0.0.0")
    let db_url = env_str("DATABASE_URL", "")

    if len(db_url) == 0 {
        eprintln("DATABASE_URL is required")
        return
    }

    println("port:       " + to_string(port))
    println("bind:       " + bind)
    println("log_level:  " + log_level)
    println("db_url:     " + redact(db_url))
}

fn env_str(key: str, default_value: str) -> str {
    let v = env_get(key)
    if len(v) == 0 { return default_value }
    return v
}

fn env_int(key: str, default_value: i64) -> i64 {
    let v = env_get(key)
    if len(v) == 0 { return default_value }
    return parse_int(v)
}

fn eprintln(msg: str) {
    // No stderr builtin yet; route through file_write to /dev/stderr or
    // fall back to println for portability:
    println(msg)
}

fn redact(s: str) -> str {
    // Mask anything after `://...:`  to keep secrets out of logs.
    if !contains(s, "://") { return s }
    if !contains(s, "@") { return s }
    // Naive: replace everything between : and @ with ****
    let mut out: str = ""
    let n = len(s)
    let mut i: i64 = 0
    let mut in_secret: bool = false
    while i < n {
        let c = substr(s, i, i + 1)
        if c == ":" && i > 0 && contains(substr(s, i, n), "@") {
            out = out + ":****"
            // skip until @
            while i < n && substr(s, i, i + 1) != "@" {
                i = i + 1
            }
            continue
        }
        out = out + c
        i = i + 1
    }
    return out
}
```

## Things to know

- `env_get` is a builtin — no `use` needed.
- `parse_int` panics on malformed input. For graceful handling, validate
  with a regex first (`std::re`).
- Redact secrets *before* logging them. The `redact` helper above is
  naive; for production use a battle-tested URL parser.
- Set required keys at process-startup, not per-request. Fail fast.
