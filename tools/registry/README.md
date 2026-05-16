# kryos-registry-server

Reference HTTP server for the [Kryos package registry](../../docs/package-registry.md).

**Design notes**

- Dependency-free: uses only Rust's `std` (no `tokio`, no `hyper`, no `serde`).
- Read-only: there are no write endpoints; publishing is PR-based against
  the index Git repo.
- Stateless beyond the local Git checkout: restart and rollout are trivial.

## Routes

| Method | Path | Description |
|---|---|---|
| `GET` | `/v1/health` | Liveness probe; returns `ok` |
| `GET` | `/v1/packages/<name>` | All versions of `<name>`, NDJSON |
| `GET` | `/v1/packages/<name>/<version>` | Single version, JSON |
| `GET` | `/v1/search?q=<query>` | Package names matching `<query>`, JSON array |

## Build

```bash
cd tools/registry
cargo build --release
# produces target/release/kryos-registry-server
```

## Run

The server needs a local checkout of a Kryos registry index:

```bash
git clone https://github.com/NORTHTEKDevs/kryos-registry /var/lib/kryos-registry

./target/release/kryos-registry-server \
  --index /var/lib/kryos-registry \
  --addr 0.0.0.0:8080
```

By default it `git pull`s the index every 5 minutes in a background
thread. Disable with `--no-pull`.

## Self-hosting your own registry

1. Create a new Git repo with the layout described in
   [`docs/package-registry.md`](../../docs/package-registry.md).
2. Run this server pointed at it.
3. In each consuming project's `kryos.toml`, override the registry URL:

   ```toml
   [registry]
   url = "https://your-host.example.com"
   ```

## Why no framework?

This server is intentionally minimal so that operators auditing the
registry can read the entire HTTP surface in one file. It is not
intended to be a high-throughput service; the Git mirror is the
canonical source and clients can `git pull` directly. If you need
higher RPS in front of a popular registry, put a CDN (Cloudflare,
Fastly) in front — it caches `GET` responses by URL trivially.
