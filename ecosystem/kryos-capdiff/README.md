# kryos-capdiff

CI gate that diffs two `kryos-manifest-v1` files and fails with exit code 1 when any function widens into a configurable "dangerous" capability set (default: `ffi`, `process`).

## Usage

```bash
kryos run ecosystem/kryos-capdiff -- <base.json> <head.json> [--dangerous cap1,cap2]
```

Where `base.json` and `head.json` are files produced by `kryos manifest --caps`.

## Output

Prints a Markdown table to stdout classifying every per-function capability change:

```
| Function | Change | Base Caps | Head Caps |
|----------|--------|-----------|----------|
| foo      | WIDENED | []        | [process] |
| bar      | REMOVED | [io]      | []        |
| baz      | NARROWED | [io, net] | [io]     |
| qux      | ADDED   | []        | []        |
```

Exit codes:
- `0` — no dangerous capability widening detected
- `1` — one or more functions gained a dangerous capability
- `2` — bad arguments

## Change kinds

| Kind     | Meaning                                                  |
|----------|----------------------------------------------------------|
| WIDENED  | Function gained capabilities in head vs. base            |
| NARROWED | Function lost capabilities in head vs. base              |
| ADDED    | Function appears in head but not in base                 |
| REMOVED  | Function appears in base but not in head                 |

Only WIDENED into the dangerous set triggers a non-zero exit.

## GitHub Actions snippet

```yaml
- name: Generate base manifest
  run: kryos manifest --caps --format json -o base.json

- name: Generate head manifest
  run: kryos manifest --caps --format json -o head.json

- name: Check for capability escalation
  run: kryos run ecosystem/kryos-capdiff -- base.json head.json
```

To use a custom dangerous set:

```yaml
- name: Check for capability escalation
  run: kryos run ecosystem/kryos-capdiff -- base.json head.json --dangerous ffi,process,net
```

## Running tests

```bash
kryos test --path ecosystem/kryos-capdiff
```

## Design notes

- Pure Kryos — reads two JSON files via a hand-rolled manifest scanner, diffs in memory, prints Markdown.
- The scanner avoids `use std::json` because `std::json`'s `_format_number` function has a Cranelift JIT verifier bug that fails all `@test` functions at compile time. The same workaround is used in `kryos-plugin-sandbox`.
- `src/manifest.kry` — scanner for `kryos-manifest-v1` JSON
- `src/diff.kry` — diff two `[FnEntry]` lists → `[FnChange]`
- `src/report.kry` — format `[FnChange]` as Markdown

## Schema version pinning

`parse_manifest` throws on any schema other than `kryos-manifest-v1`:

```
capdiff: unsupported manifest schema 'X' (expected kryos-manifest-v1)
```
