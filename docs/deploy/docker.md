# Deploying Kryos in Docker

A Kryos binary built with `kryos build --release` is a statically-linked
native executable. The minimal production container is `gcr.io/distroless/cc`
(or `alpine` if you need a shell).

## Multi-stage Dockerfile

```dockerfile
# ---- build stage ----------------------------------------------------------
FROM ubuntu:24.04 AS build

ARG KRYOS_VERSION=v1.0.0
RUN apt-get update && apt-get install -y --no-install-recommends \
        clang lld libssl-dev pkg-config ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

# Install the Kryos toolchain (pre-built tarball).
# The release asset name is NOT version-stamped -- it is exactly
# kryos-linux-x86_64.tar.gz for every tag. The archive already contains the
# conventional prefix layout (bin/kryos, lib/libkryos_rt.a,
# lib/libkryos_stdlib_native.a, stdlib/), so extract it WITHOUT
# --strip-components: unpacking into /usr/local yields /usr/local/bin/kryos
# with the runtime libs where the compiler looks for them.
RUN curl -fsSL "https://github.com/NORTHTEKDevs/kryos-lang/releases/download/${KRYOS_VERSION}/kryos-linux-x86_64.tar.gz" \
        | tar -xz -C /usr/local

WORKDIR /src
COPY kryos.toml ./
COPY src ./src
RUN kryos build --release src/main.kry -o /out/myapp

# ---- runtime stage --------------------------------------------------------
FROM gcr.io/distroless/cc:nonroot
COPY --from=build /out/myapp /usr/local/bin/myapp
USER nonroot
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/myapp"]
```

## Image size

A typical hello-world Kryos binary is ~2.5 MB stripped. With the distroless
runtime base (~22 MB) the total image lands at ~25 MB. Compare to a
node:20-alpine container at ~180 MB.

## Build + run locally

```bash
docker build -t myapp .
docker run --rm -p 8080:8080 myapp
```

## Health check + readiness

Have your `main()` write a `/tmp/ready` file once the listener is up:

```kryos
file_write("/tmp/ready", "ok\n")
```

Then in the Dockerfile:

```dockerfile
HEALTHCHECK --interval=10s --timeout=3s --retries=3 \
    CMD test -f /tmp/ready || exit 1
```

## Multi-arch

```bash
docker buildx build --platform linux/amd64,linux/arm64 -t myapp:latest .
```

Kryos LLVM AOT targets both x86_64 and aarch64; the `kryos build --release
--target aarch64-unknown-linux-gnu` cross-compile path is exercised in
`cross.yml`.
