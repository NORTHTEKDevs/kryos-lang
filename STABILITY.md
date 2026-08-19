# Kryos Stability & Support Policy

This document describes what the Kryos language, compiler, and runtime
guarantee at each release, what is explicitly **not** guaranteed, and the
process by which a release is cut. It is the source of truth referenced
from `CHANGELOG.md` and tooling.

Last updated: 2026-08-16 (v0.9.0). Check counts sync to README/CI, which
`tests/docs_status_gate.sh` gate-checks; this file is refreshed after them.

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

## 4. Current pass rates (v0.9.0)

- Cranelift JIT (`kryos run`): **100%** on the native runner suite.
- LLVM release (`kryos build --release`): **100%** on the
  `native_build_release_tests` suite (155 programs, both backends).
- Compiler unit tests: **100%** (495 tests across the front/middle/back end).
- Capability soundness: `inferred_soundness.sh` green; `strict_caps_examples.sh`
  **91 / 91**; `ecosystem_check.sh` **259 / 259** (deny-by-default across all
  packages, incl. the extern-call gate).
- Examples gate: root **45 / 45**, fixtures **16 / 16**, showcase **24 / 24**,
  capability-rejection **1 / 1**, multi-file project OK.
- Self-host: `kryos check` **18 / 18** self-host sources + stage-1 mini-parser.
- Docs snippets: **55 / 55** type-check.
- Completeness: **157 executed feature probes across three audit tiers, 0
  defects on both backends.**
- Compiler build warnings: **zero**.

---

## 5. Known limitations (v0.9.0)

Both concurrency release blockers tracked at 0.9.0 (`conf_spinlock_mutex`
use-after-free and the `conf_errors_concurrency` actor deadlock) were traced to
a single spawn-wrapper ABI defect and fixed on 2026-07-28. Conformance is 65/65
on both backends (`bash tests/conformance/run_conformance.sh`; grows as tests
are added -- `tests/docs_status_gate.sh` fails CI if this number drifts). See
[docs/BUGS.md](docs/BUGS.md).

- **Capability enforcement is sound for direct calls AND for closure/fn-value
  indirection through a parameter, local, return value, passthrough chain,
  actor message, `spawn`, generic, `dyn Trait` dispatch, OR a CONTAINER (a
  struct field, array element, or map value that holds a closure, including
  nested combinations) -- in EVERY mode including `--strict-capabilities`.**
  A struct/array/map value that CONTAINS a closure is traced back to its
  construction site (a struct field precisely, an array/map value
  conservatively across all elements), so a zero-capability function that
  drills into a container to invoke a privileged closure is caught at the
  call site that supplied it. See [docs/10-capabilities.md § Closure
  indirection, including
  containers](docs/10-capabilities.md#closure-indirection-including-containers-is-sound-read-this-if-you-are-trusting-it-with-secrets)
  for the full sound surface and the one documented residual (a container
  populated from a non-literal source requires `all`, the same conservative
  fallback used for every other unresolvable closure provenance).

Honest, non-blocking residuals:

- **Turbofish struct construction** (`Box<i64>{..}`) is unsupported; use bare
  `Box{..}` with inference (Rust rejects the turbofish-literal form too).
- **`panic` is not catchable by `try`/`catch`** — `throw` is the recoverable
  path; `panic` (div-by-zero, integer division overflow `MIN / -1`, OOB,
  `file_read` on a missing file, non-exhaustive nested match) aborts. This
  is intentional, documented semantics.
- **Program nesting is capped (E0010):** grammar recursion at 256 levels
  (clang-class limit) and total expression depth — including flat
  `a+b+c+...` chains — at 2048. No legitimate program approaches either
  bound; the caps exist so no input, however adversarial, can crash the
  compiler with a stack overflow instead of a diagnostic.
- **AOT (`kryos build --release`) needs a host C toolchain** (MSVC/clang) for
  linking; the JIT (`kryos run`) needs nothing.
- **`kryos fmt` does not preserve non-doc comments.** The formatter is
  AST-based and the lexer discards `//` line and `/* */` block comments
  (only `///` doc comments are retained and re-emitted). Formatting a file
  drops its ordinary comments. `fmt` IS semantics-preserving otherwise —
  string interpolation and literal braces round-trip correctly. Comment
  preservation needs a trivia-carrying parse and is tracked as follow-up;
  until then, avoid `kryos fmt` on comment-heavy files you care about.

`unsafe { }` is now ENFORCED: a raw-pointer dereference outside any `unsafe`
block is rejected with E0500.
- **AOT memory corruption in enum-payload array handling -- FIXED
  (2026-08-18, LEDGER item 44 WAVE 3):** an array whose elements are enum
  values could be double-freed on `kryos build --release` if it flowed
  through more than one call to a shared closure/builtin dispatcher (e.g. an
  interpreter's own `apply()`) during the program's lifetime -- root cause
  was a second raw bit-copy site (the `push` builtin's aggregate-boxing
  codegen) missing the same per-field heap-reference dup that
  `RValue::EnumVariant` construction already had. Both backends are now
  clean: `tests/minilisp_gate.sh` (tier 1) reports JIT 10/10 and AOT 11/11
  (the original 10-program corpus plus a new closure-counter case), all with
  zero use-after-free/double-free diagnostic lines under
  `KRYOS_BOX_DIAG=1 KRYOS_FREE_DIAG=1`. A fix-candidate leak this session
  found via its own adversarial memory measurement (a freshly-constructed
  enum immediately pushed with no other use, re-dup'd on top of
  construction's own independent reference) was mitigated, not left in.
  See LEDGER item 44 for the full mechanism and residual notes.
- **JIT closure-counter regression -- FIXED (LEDGER item 44, WAVE 1,
  2026-08-19; item 44 is now fully CLOSED, both backends):**
  `make-counter`'s returned closure (`set!`-mutating a captured local
  across two calls, `tests/minilisp/t10.lisp`) used to fail
  NONDETERMINISTICALLY on `kryos run` -- illegal-instruction or a wrong
  "unbound symbol" answer, zero diagnostic lines either way. Root-caused to
  two Cranelift-only bugs: `RValue::EnumVariant`/`RValue::Struct`
  construction computed `kryos_array_dup`'s `elem_kind` arg with a
  numbering that does not match the dup function's real convention
  (Map-typed fields silently skipped every retain -- `Value.Closure`'s
  captured-env chain is exactly an Array-of-Map payload), and
  `kryos-mir::lower.rs`'s `retain_for_ty` never covered Struct/Enum for the
  `m[k]=v`/`arr[i]=v` IndexAssign value-retain site (a UAF surfacing as a
  `br_table`-on-garbage-tag SIGILL). Fixed entirely in
  `kryos-codegen-cranelift/src/codegen.rs`; `kryos-mir`,
  `kryos-codegen-llvm`, and `kryos-rt` untouched. `tests/minilisp_gate.sh`
  is now 11/11 on BOTH backends (22/22 total), zero diagnostic lines, t10
  10x per backend under diag-on and diag-off all correct; the demo prints
  "closure counter: 1 2 3" correctly on both backends, byte-identical to
  each other, for the first time since item 44 opened. See LEDGER item 44
  for the full six-session evidence chain.
- **AOT-only enum-array-push leak, OPEN (LEDGER item 45, characterized
  2026-08-19, NOT fixed):** a separate, pre-existing, proportional leak in
  the general enum-array-push pattern -- ~454MB peak at 5M fresh-enum
  pushes on AOT, JIT flat/clean (~11MB) at the same scale. Found while
  closing item 44's JIT residual; neither of that fix touches this path
  (scoped to construction's own field-dup and to the map/array-insert
  retain, not `kryos_array_push`'s AOT-only aggregate-boxing step).
  Committed regression/characterization probe:
  `tests/mem/enum_array_push_leak.kry` (`LEAK_ITERS`-gated). Ranked as a
  LEAK, below any silent-wrong-answer/crash class: requires millions of
  fresh-enum-then-immediately-pushed iterations to become visible, does not
  corrupt output or crash. Candidate fix site:
  `local_is_always_fresh_enum_construction` in
  `kryos-codegen-llvm/src/codegen.rs`.


### 5.0 WASM backend coverage (v0.7)

The `wasm32` backend is a compute-focused subset, not full parity. **Works**
(verified via the node host): i64/f64, all integer widths + narrow-int casts
(`x as u8`), strings, structs, enums, arrays (host-backed + mutable,
`push`/`pop` in place — matching native), maps (`map<str,i64>`), if/else,
loops (`while`, `for i in a..b`, NESTED loops, `break`/`continue` — via a
dispatch-relooper fallback for CFGs the structured translator can't express),
recursion, generics, traits, `Result`/`Option`, **closures** (no-capture AND
capturing, via a heap env array + per-lambda thunk + `call_indirect`), and
**higher-order functions** (`fold`/`map`/`filter` with lambdas, incl. captured
variables). **Not on wasm:** capturing an f64/f32 value (i64/str/handle
captures work), and all concurrency (spawn/channels/actors — wasm is
single-threaded by design). The native Cranelift JIT and LLVM AOT backends
remain the full-language, at-parity path.

The v2.1 "known limitations" listed below were all closed in v2.2 and are kept
here as historical context.

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
