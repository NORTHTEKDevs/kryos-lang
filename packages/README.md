# First-party Kryos packages

Five small libraries shipped alongside the compiler. Each one is independently versioned and installable via `kryos pkg add <name>` once a registry is wired up.

| Package | What it provides |
| --- | --- |
| [`kryos-test-ext`](./kryos-test-ext/) | Extra assertion helpers: `assert_eq_i64`, `assert_eq_str`, `assert_lt`, `assert_contains_str`, `assert_msg` |
| [`kryos-http-router`](./kryos-http-router/) | Minimal HTTP/1.1 method+path parser + response builder for use inside a TCP accept loop |
| [`kryos-uuid-pkg`](./kryos-uuid-pkg/) | Ergonomic v4 UUID helpers around `std::uuid` |
| [`kryos-base64-pkg`](./kryos-base64-pkg/) | Ergonomic base64 helpers around `std::base64`, plus a `data_url(...)` builder |
| [`kryos-time-pkg`](./kryos-time-pkg/) | Datetime helpers: `UtcDate` struct, `now_utc()`, `now_iso()`, `ymd_utc(...)`, `days_between(...)`, `weekday_short(...)` |

## Layout

Each package follows the standard Kryos project layout:

```
packages/<name>/
  kryos.toml      # package manifest
  src/lib.kry     # entry point
  README.md       # docs
```

## Local install (offline registry)

The packages can be served locally via `kryos pkg serve --root packages/` once the offline-registry mode lands. Until then, point your project at the local path:

```toml
# kryos.toml
[dependencies]
kryos-test-ext = { path = "../packages/kryos-test-ext" }
```

## Stability

All packages are 0.1.0 — pre-1.0 minor bumps may break API. The 1.0 cut will lock the public surface alongside the next compiler `1.x` release.
