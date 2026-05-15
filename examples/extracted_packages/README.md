# Kryos Extracted Packages

Source-of-truth for the five starter packages published to the
[kryos-registry](https://github.com/NORTHTEKDevs/kryos-registry).

Each subdirectory contains a `kryos.toml` manifest, a `src/lib.kry` main module,
and a `README.md` with usage docs.

## Packages

| Package | Description | Install |
|---------|-------------|---------|
| [markdown](./markdown/) | CommonMark-subset Markdown to HTML renderer | `kryos pkg add markdown` |
| [http-router](./http-router/) | Simple HTTP request/response structs and routing helpers | `kryos pkg add http-router` |
| [json](./json/) | Friendly wrappers around Kryos built-in JSON functions | `kryos pkg add json` |
| [sqlite](./sqlite/) | FFI wrapper for SQLite via libsqlite3 | `kryos pkg add sqlite` |
| [regex](./regex/) | Regex matching via POSIX regcomp/regexec with pure-Kryos fallback | `kryos pkg add regex` |

## Registry index

All packages are indexed in [NORTHTEKDevs/kryos-registry](https://github.com/NORTHTEKDevs/kryos-registry)
under `<first-two-chars-of-name>/<name>.json`, one JSON line per version.

## Building tarballs

```sh
mkdir -p /tmp/registry-build
for pkg in markdown http-router json sqlite regex; do
    tar -czf /tmp/registry-build/$pkg-0.1.0.tar.gz $pkg/
done
```

Run from within `examples/extracted_packages/`.
