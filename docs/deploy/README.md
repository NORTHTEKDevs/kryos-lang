# Deploying Kryos applications

Kryos binaries are statically-linked native executables. There is no runtime
to install on the target machine, no interpreter, no garbage collector to
tune. Deploy them like Go or Rust binaries.

## Recipes

- [Docker](./docker.md) — distroless multi-stage image, ~25 MB total
- [systemd](./systemd.md) — service unit with full sandboxing + Type=notify

## Build flags for production

```bash
# Optimized AOT compile
kryos build --release src/main.kry

# With debug symbols (for stack traces in panic messages)
kryos build --release -g src/main.kry

# Strip the binary (smallest size; loses panic file/line info)
strip target/release/myapp

# WASM (for Cloudflare Workers, fastly Compute, etc.)
kryos build --release --backend wasm src/main.kry
```

## Cross-compilation

Tier-1 platforms can be cross-compiled from any tier-1 host:

```bash
kryos build --release --target x86_64-unknown-linux-gnu src/main.kry
kryos build --release --target aarch64-apple-darwin src/main.kry
kryos build --release --target x86_64-pc-windows-msvc src/main.kry
```

The runtime + stdlib-native static libraries for each target ship with the
toolchain tarball under `<install>/lib/<target>/`.

## Static linking + musl

For maximally portable Linux binaries (no glibc dependency):

```bash
kryos build --release --target x86_64-unknown-linux-musl src/main.kry
```

The resulting binary runs on any Linux kernel ≥ 3.2, including
distroless / scratch images, without any libc on the target.

## What's stable for production

All the items listed in [`STABILITY-v4.0.md`](../../STABILITY-v4.0.md) are
locked for the v4.x line: CLI surface, LSP method set, stdlib symbol table,
ABI symbols, `kryos.toml` format, registry HTTP API.
