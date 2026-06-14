# Cross-compilation

Kryos can produce binaries for targets other than the host. Cross-compilation
goes through the **LLVM release backend** (`--release`), which forwards the
target triple to LLVM. The Cranelift debug backend is host-only.

## Quick reference

```bash
# Print all known target triples
kryos build --target=help

# Static Linux x86_64 binary (musl)
kryos build src/main.kry --release --target=x86_64-unknown-linux-musl -o app

# Windows x86_64 (MinGW)
kryos build src/main.kry --release --target=x86_64-pc-windows-gnu -o app.exe

# ARM64 Linux
kryos build src/main.kry --release --target=aarch64-unknown-linux-gnu -o app
```

## Supported targets

These triples are recognized by `kryos build --target=`. Each requires
the corresponding **linker + sysroot** to be installed on your host.

| Triple | Notes | Linker / sysroot you need |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | The default on most Linux | `cc` (gcc/clang) |
| `x86_64-unknown-linux-musl` | Fully static; great for containers | `musl-gcc` / `x86_64-linux-musl-gcc` |
| `aarch64-unknown-linux-gnu` | ARM64 Linux (Pi 4, Graviton, ...) | `aarch64-linux-gnu-gcc` |
| `aarch64-unknown-linux-musl` | Static ARM64 Linux | `aarch64-linux-musl-gcc` |
| `x86_64-pc-windows-gnu` | Windows via MinGW | `x86_64-w64-mingw32-gcc` |
| `x86_64-pc-windows-msvc` | Windows native (requires `link.exe`) | MSVC toolchain |
| `aarch64-pc-windows-msvc` | Windows on ARM | MSVC ARM toolchain |
| `x86_64-apple-darwin` | macOS Intel | `cctools` + macOS SDK |
| `aarch64-apple-darwin` | macOS Apple Silicon | `cctools` + macOS SDK |
| `wasm32-unknown-unknown` | Standalone WebAssembly (**experimental v0.1**, limited feature subset) | `wasm-ld` (bundled with `lld`) |

> **`wasm32-unknown-unknown` is experimental (v0.1), not on par with the
> native targets above.** The WASM backend supports integer/float
> arithmetic, booleans, basic loops, recursion, string literals, and arrays,
> but not structs, enums, tuples, maps, closures, `match`, `to_string()`,
> string interpolation, or any `std` module. WASI is not supported (the
> JS-host contract only). Use it for previews, not production.

You can also pass any arbitrary LLVM triple — it'll be forwarded as-is.
This is useful for embedded targets like `riscv64gc-unknown-linux-gnu`.

## How it works

The pipeline is the same as native build, with two differences in the
backend:

1.  **LLVM backend** receives the target triple via `EmitOptions::target_triple`
    and asks LLVM to emit machine code for that target instead of the host.
2.  **Linker invocation** uses a target-specific linker if available
    (e.g. `x86_64-linux-musl-gcc` rather than the host `cc`).

The Kryos runtime (`kryos-rt`) and native stdlib (`kryos-stdlib-native`)
must also be cross-compiled. This happens automatically because they
build alongside the user program in the same `cargo` workspace — Cargo
picks up the target triple from the `kryos build` driver and rebuilds
the runtime for it.

## Common failure modes

**`error: linker x86_64-linux-musl-gcc not found`**

Install the musl cross-toolchain (`musl-tools` on Debian/Ubuntu,
`musl-cross` from Homebrew on macOS). The linker has to be on `PATH`.

**`error: clang not found`**

`--release` requires `clang` for the final compile from LLVM IR. Install
LLVM (`apt install clang`, `brew install llvm`, or download from
[llvm.org](https://llvm.org)) and ensure `clang` is on `PATH`.

**`warning: --target=... requires --release (LLVM backend).`**

You passed `--target` without `--release`. Cranelift (the default debug
backend) only emits code for the host. Add `--release` to use LLVM, which
supports all the targets above.

**`warning: target X is not in the known-good list`**

You passed a triple Kryos doesn't recognize. LLVM might still accept it —
this is just a hint to double-check spelling.

## Linking for unfamiliar targets

For some triples (especially embedded), you'll need to pass extra linker
flags. Kryos doesn't currently expose `--linker` or `-Clink-arg=` flags
directly; for now, emit LLVM IR with `--emit-llvm` and link manually:

```bash
kryos build src/main.kry --release --target=riscv64gc-unknown-linux-gnu \
    --emit-llvm > main.ll
llc -filetype=obj -mtriple=riscv64-linux-gnu main.ll -o main.o
riscv64-linux-gnu-gcc main.o -o main -static
```

Native flag forwarding will land in a later release.

## What's not yet supported

- **iOS** (`aarch64-apple-ios`) — needs Apple-signed linker tooling.
- **Android** (`aarch64-linux-android`) — needs the Android NDK.
- **`no_std` targets** — Kryos always links the runtime, which requires
  `malloc`/`free` from the host libc. Pure-bare-metal targets aren't
  supported.

These are all on the roadmap.
