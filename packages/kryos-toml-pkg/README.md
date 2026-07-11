# kryos-toml-pkg

TOML subset parser in pure Kryos — the format Kryos projects already use
for `kryos.toml`. No capabilities required.

```kryos
use lib::{toml_parse, toml_get_str, toml_get_int}

fn main() {
    let t = toml_parse(file_content)
    let name = toml_get_str(t, "package.name", "unnamed")
    let port = toml_get_int(t, "server.port", 8080)
}
```

Keys flatten to `"section.key"` (top-level keys are just `"key"`). Values
keep their raw text in the map; the typed getters convert on access and
return the caller's default on a missing key or a type mismatch — no
panics.

## Supported

- Tables `[section]` and top-level scalars
- Basic strings `"..."`, literal strings `'...'`
- Integers (including negative), floats, booleans
- `#` comments — full-line and trailing; a `#` inside a quoted value is
  preserved (quote-aware stripping)

## Not supported (by design, v0.1)

Dotted/nested keys, arrays, arrays-of-tables `[[x]]`, inline tables,
dates, multi-line strings, escape sequences inside strings.

## API

| Function | Purpose |
| --- | --- |
| `toml_parse(content: str) -> map<str, str>` | Parse to a flat raw-value map |
| `toml_has(t, key) -> bool` | Key existence |
| `toml_get_str(t, key, default) -> str` | String (quotes stripped) |
| `toml_get_int(t, key, default) -> i64` | Integer, default on mismatch |
| `toml_get_float(t, key, default) -> f64` | Float, default on mismatch |
| `toml_get_bool(t, key, default) -> bool` | `true`/`false`, default otherwise |

## Test

```bash
kryos run packages/kryos-toml-pkg/src/selftest.kry
# PASS kryos-toml-pkg selftest (14 checks)
```
