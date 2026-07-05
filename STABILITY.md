# Kryos Stability & Support Policy

This document describes what the Kryos language, compiler, and runtime
guarantee at each release, what is explicitly **not** guaranteed, and the
process by which a release is cut. It is the source of truth referenced
from `CHANGELOG.md` and tooling.

Last updated: 2026-07-04 (v1.0.0-beta.2).

---

## 1. Versioning

Kryos follows **SemVer** at the language and CLI surface:

- **Major** — Breaking change to source syntax, MIR ABI, runtime ABI, CLI
  command surface, or the registry/lockfile format.
- **Minor** — Backwards-compatible additions: new builtins, new stdlib
  modules, new optimization passes, new CLI subcommands, new diagnostics,
  new backends. Also: bug fixes that change observable program output for
  programs that were relying on previously-undefined behavior.
- **Patch** — Bug fixes that do not change documented behavior; build,
  packaging, or documentation fixes.

The `kryos-rt` runtime crate is versioned in lockstep with the compiler
in this repo; out-of-tree consumers should pin both.

---

## 2. Supported platforms

| Platform                  | Tier | Cranelift JIT (`kryos run`) | LLVM release (`kryos build --release`) | CI |
|---------------------------|------|-----------------------------|----------------------------------------|----|
| Linux x86_64 (glibc)      | 1    | Supported                   | Supported                              | Yes |
| macOS aarch64 (macOS-14)  | 1    | Supported                   | Supported                              | Yes |
| Windows x86_64 (MSVC)     | 1    | Supported                   | Supported                              | Yes |
| macOS x86_64              | 2    | Best-effort                 | Best-effort                            | No  |
| iOS / Android / embedded  | —    | Out of scope                | Out of scope                           | —   |

"Tier 1" means every release is verified on this platform via the
`parity-matrix` CI job (Linux / macOS-14 / Windows). "Tier 2" means
breakages on this platform may slip a release but will be fixed promptly
when reported.

Both backends are required to pass the full smoke suite on all three
tier-1 platforms before any tag is cut. See `AUDIT-llvm-parity.md` for
the parity-matrix definition of done.

---

## 3. Release gates

A release tag is cut when **all** of the following hold:

1. `cargo build --release -p kryos-cli --manifest-path compiler/Cargo.toml`
   succeeds with zero errors.
2. The Cranelift JIT native runner is at **100% pass** on the
   `kryos-test-runner::native_runner` suite (`kryos run` path).
3. The LLVM release native runner pass rate is at least the previous
   tagged release's pass rate. Regressions block the release.
4. `cargo test --release` for `kryos-test-runner` reports zero failures
   outside the documented known-limitations list (see §5 below).
5. `CHANGELOG.md` has an entry for the new version, in the format used by
   the existing entries.

---

## 4. Current pass rates (v1.0.0-beta.2)

- Cranelift JIT (`kryos run`): **100%** on the native runner suite.
- LLVM release (`kryos build --release`): **100%** on the
  `native_build_release_tests` suite.
- Parity matrix (`tests/parity/run_parity.sh`): **47 / 47
  both_pass on Cranelift + LLVM** across the smoke suite.
- Compiler build warnings: **zero**.

The v2.1 "known limitations" (escaping closures, `dyn Trait` dispatch)
were closed in v2.2 and remain green. All previously-known parity
failures (`test_generics`, `test_process`, `test_match_return`,
`test_tuple_mut`, plus the B/B' string-vs-ptr class on LLVM) are closed
— see `AUDIT-llvm-parity.md` for the per-test fix breakdown.

---

## 5. Known limitations (v1.0.0-beta.2)

There are no architectural failures in the release-gating sweep at
v1.0.0-beta.2. The v2.1 "known limitations" listed below were all closed in
v2.2 and are kept here as historical context.

### 5.1 (Closed in v2.2) Closures that escape their defining scope

**Previously affected tests:** `closure_escape`, `closure_capture_fn`.

v2.2 changed the LLVM lambda ABI to a uniform `(env, user_args...)`
calling convention. Every closure value, including no-capture lambdas,
is wrapped in an ARC env `[thunk_ptr, cap0, cap1, ...]`; `CallIndirect`
dispatches via `env[0]`. Both tests have been green since v2.2 and
remain green at v2.3.

### 5.2 (Closed in v2.2) `dyn Trait` method dispatch

**Previously affected test:** `dyn_trait`.

v2.2 replaced the `VtableCall` placeholder with real vtable codegen:
trait objects are fat pointers `[data, fn_ptr_0, fn_ptr_1, ...]` and
per-method dyn-thunks give every method a uniform i64-only ABI suitable
for indirect dispatch (handling byval-self / sret-return correctly).
Green since v2.2.

---

## 6. Out of scope (will not be implemented)

The following items are explicitly **not** on the Kryos roadmap. We list
them so users do not file bugs against them.

- **Full Rust-style borrow checker** with lifetimes. Kryos uses ARC plus
  explicit `move`/`share` and an opt-in unique-ownership lint.
- **Hygienic macros** in the Scheme/Racket sense. Kryos has compile-time
  string templating only.
- **Vulkan / Metal / DirectX 12** native graphics bindings. (Use SDL2 via
  the stdlib, which is what the demo uses.)
- **HTTP/3 / QUIC server**. The stdlib speaks HTTP/1.1 and HTTP/2 client
  via reqwest; HTTP/3 is not planned.
- **Profile-guided optimization** end to end. LTO ("fat") is on; PGO is
  not wired through the CLI.
- **Video decode** (h264/h265/AV1). Out of scope; use FFI to libav.
- **Retained-mode GUI toolkit** (Qt/GTK-style). Use SDL2 for immediate
  mode.
- **Full Unicode normalization** (NFC/NFD/NFKC/NFKD) in the stdlib. We
  expose `str_upper`/`str_lower` and `regex` (PCRE-style); full
  normalization is delegated to FFI.
- **iOS / Android / embedded** targets.

---

## 7. Backwards-compatibility commitments

Within a major version line (e.g. all `2.x.y`):

- **Source syntax** that parses and type-checks in `2.0.0` will continue
  to parse and type-check in every later `2.x.y`.
- **Public stdlib builtin signatures** will not change in incompatible
  ways. New optional parameters may be added at the end of a signature
  if and only if the compiler emits a deprecation warning for one minor
  release before requiring callers to update.
- **CLI subcommands** documented in `kryos --help` will not be removed
  or renamed.
- **`kryos.toml` / `kryos.lock` file formats** will not change in
  incompatible ways. New optional fields may be added.

Things explicitly **not** committed inside a major line:

- **MIR / IR layout**. The MIR is an internal contract between compiler
  crates and changes freely between minor releases.
- **Runtime symbol names** (`kryos_*_ks`). These are internal ABI between
  the compiler and `kryos-rt`. Out-of-tree FFI users should depend on
  the documented `extern "C"` surface in the stdlib instead.
- **Optimization pipeline order**, **inlining heuristics**, and
  **codegen quality**. Runtime numbers may move up or down by minor
  factors between minor releases.

---

## 8. Reporting bugs

Open an issue at the canonical repo with:

1. `kryos --version` output.
2. The minimal `.kry` source that reproduces.
3. The exact `kryos run` / `kryos build` command line.
4. For LLVM-release bugs: rerun with `KRYOS_KEEP_LL=1` and attach the
   preserved `/tmp/kryos_llvm_inc.ll` if the compile fails before
   linking.
5. Expected vs actual stdout / exit code.

Security-sensitive issues should be emailed to the maintainer address
in `Cargo.toml` instead of filed publicly.
