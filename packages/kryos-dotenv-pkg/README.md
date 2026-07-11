# kryos-dotenv-pkg

`.env` file parsing that shows off Kryos's capability model: parsing is
pure (no grant needed); only `dotenv_load` touches the filesystem, and it
declares exactly `fs:read` — not coarse `io`.

```kryos
use lib::{dotenv_load, dotenv_get}

@capabilities(fs:read)
fn main() {
    let env = dotenv_load(".env")
    let key = dotenv_get(env, "API_KEY", "")
    if len(key) == 0 {
        println("API_KEY not set")
    }
}
```

## Behavior

- `KEY=VALUE` per line; the first `=` splits key from value
- `#` comment lines and blank lines are skipped
- Optional `export ` prefix is stripped
- Values may be single- or double-quoted (quotes stripped, no escape
  processing inside)
- Keys and unquoted values are whitespace-trimmed
- Duplicate keys: last one wins
- `dotenv_load` on a missing file returns an empty map (no panic)

## API

| Function | Capability | Purpose |
| --- | --- | --- |
| `dotenv_parse(content: str) -> map<str, str>` | none | Parse .env text |
| `dotenv_get(env, key, default) -> str` | none | Lookup with default |
| `dotenv_load(path: str) -> map<str, str>` | `fs:read` | Read + parse a file |

## Test

```bash
kryos run packages/kryos-dotenv-pkg/src/selftest.kry
# PASS kryos-dotenv-pkg selftest (10 checks)
```
