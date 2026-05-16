# Kryos Stability & Support Policy

This document describes what the Kryos language, compiler, and runtime
guarantee at each release, what is explicitly **not** guaranteed, and the
process by which a release is cut. It is the source of truth referenced
from `CHANGELOG.md` and tooling.

Last updated: 2026-05-15 (v2.1.0).

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
| macOS x86_64 / aarch64    | 2    | Best-effort                 | Best-effort                            | No  |
| Windows x86_64 (MSVC)     | 3    | Compiles                    | Not tested end-to-end                  | No  |
| iOS / Android / embedded  | —    | Out of scope                | Out of scope                           | —   |

"Tier 1" means every release is verified on this platform. "Tier 2" means
breakages on this platform may slip a release but will be fixed promptly
when reported. "Tier 3" means the platform is wired up but not part of the
release gating set.

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

## 4. Current pass rates (v2.1.0)

- Cranelift JIT (`kryos run`): **100%** on the native runner suite.
- LLVM release (`kryos build --release`): **112 / 115 (97.4%)** on the
  `native_build_release_tests` suite.

The three remaining LLVM-release failures are tracked as known
architectural limitations (§5) and are scheduled for v2.2.

---

## 5. Known limitations (v2.1.0)

These are real, reproducible gaps. They are documented here so that users
do not hit them by surprise. None of them affect `kryos run` (Cranelift
JIT) — they are all on the `kryos build --release` (LLVM) path.

### 5.1 Closures that escape their defining scope

**Affected tests:** `closure_escape`, `closure_capture_fn`.

The current LLVM lambda ABI takes captures as **direct parameters** to
the generated lambda function. This works correctly when the
`closure_locals` optimization fires and the closure is called in the
same lexical scope where it was defined, because the call site has the
captures in hand and passes them directly.

It does **not** work when the closure escapes (returned from a function,
stored in a struct field that outlives the defining scope, or passed as
a callback through an indirect call site) because the indirect call site
only has an env pointer; it does not know the capture count or types,
so it cannot reconstruct the direct-parameter call.

**Fix path (v2.2):** Either (a) change the lambda ABI to take the env
pointer as the first parameter and emit a prologue that loads captures
from env slots, or (b) record capture-count metadata in the env header
so call sites can dynamically marshal captures. Option (a) is preferred
because it composes with `closure_locals`.

### 5.2 `dyn Trait` method dispatch

**Affected test:** `dyn_trait`.

The LLVM backend's `VtableCall` MIR opcode is currently a documented
Ring-3 placeholder that returns `0`. Real vtable construction (per-impl
fn-pointer table, fat pointer layout `{ data_ptr, vtable_ptr }`, and
indirect dispatch at call sites) is scheduled for v2.2.

Workaround in user code: replace `dyn Trait` with a tagged enum and
explicit match.

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
- **Windows-tested** release pipeline (the code compiles, but is not
  part of the release gate).

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
