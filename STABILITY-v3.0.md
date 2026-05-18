# Kryos v3.0 — 1.0-grade stability statement

This document is the 1.0-grade stability statement for the Kryos
v3.0.0 release, supplementing `STABILITY.md` (which remains the
authoritative day-to-day policy and is updated in lockstep with every
release).

The v2.x line proved the language. v3.0 promises that the language
won't change underneath you.

---

## 1. Source compatibility

Every source program that parses and type-checks under v3.0.0 will
parse and type-check under every later v3.x.y. Specifically:

- The lexical and syntactic grammar in `docs/grammar.md` is frozen for
  the v3.x line. New syntax may be added, never removed or repurposed.
- Top-level declarations — `fn`, `struct`, `enum`, `trait`, `impl`,
  `use`, `extern`, `const`, `static` — keep their current syntax.
- Public stdlib builtin signatures cannot change in a
  source-breaking way. New optional parameters may be appended only
  after a one-minor-release deprecation warning cycle.
- `@pure` and `@capabilities(...)` enforcement is documented and
  binding (see §5).
- The async syntax (`async fn`, `await expr`, `spawn { ... }`,
  `chan() / send / recv`) is stable but the runtime model carries a
  caveat — see "Known caveats" in §6.

## 2. ABI compatibility

The v3.x line freezes:

- `kryos-rt` exported C-ABI symbol names (`kryos_*`) and signatures for
  every function listed in `docs/STDLIB.md`.
- `kryos-stdlib-native` exported symbols listed in the same.
- `kryos.toml` and `kryos.lock` file formats. New optional fields may
  be added; existing fields are not removed or renamed.
- The package-registry HTTP API at `/v1/health`, `/v1/packages/<name>`,
  `/v1/packages/<name>/<version>`, `/v1/search?q=...`.

Not frozen:

- The MIR layout (internal to the compiler crates).
- The compiler's internal IR symbol naming for synthesised closures
  and dyn-thunks (`<name>_env`, `<name>_dyn`).
- Optimisation-pipeline ordering and inlining heuristics.

## 3. Platform support

| Platform                  | Tier | Cranelift JIT | LLVM AOT | CI |
|---------------------------|------|---------------|----------|----|
| Linux x86_64 (glibc)      | 1    | Supported     | Supported | Yes |
| macOS aarch64 (Apple Si)  | 1    | Supported     | Supported | Yes (macos-14) |
| Windows x86_64 (MSVC)     | 1    | Supported     | Supported | Yes |
| macOS x86_64 (Intel)      | 2    | Best-effort   | Best-effort | Tag matrix only |
| Linux aarch64 (glibc)     | 2    | Best-effort   | Best-effort | cross.yml |
| Linux x86_64 (musl)       | 2    | Best-effort   | Best-effort | cross.yml |

Tier 1: every release is verified end-to-end (build, smoke, parity
matrix). Regressions block the release.

Tier 2: builds and basic smoke are verified; full parity isn't
gated on the release.

## 4. Release artifacts

Every v3.x.y release produces:

- A `.tar.gz` (Linux, macOS) or `.zip` (Windows) per target, containing
  `kryos` binary, runtime + stdlib staticlibs, examples, stdlib
  sources, and README/LICENSE/CHANGELOG.
- A SHA-256 checksum file per artifact.
- A GitHub OIDC build-provenance attestation per artifact, verifiable
  with `gh attestation verify <file> --repo NORTHTEKDevs/kryos-lang`.
- A VS Code `.vsix` (`kryos-vscode.vsix`) for the IDE extension.
- A Zed extension `.wasm` (`zed_kryos.wasm`).

### Code signing — what we DO and DO NOT do

We do NOT have Apple Developer ID or Windows EV code-signing certs at
v3.0. Consequences:

- **macOS users** will see Gatekeeper "unidentified developer" the first
  time they run `kryos`. Right-click → Open is the standard workaround,
  and the attestation chain confirms the binary was actually built by
  the project's CI.
- **Windows users** may see SmartScreen on first run until the binary
  builds reputation. Same attestation chain confirms provenance.

If you need a code-signed Kryos binary for a strict environment, build
from source and re-sign with your own org's cert.

## 5. Capability enforcement at 1.0

The capability system is real and active by default in v3.x:

- `@pure` is enforced at every type-check pass. A `@pure` function
  cannot call `println`, `print`, `eprintln`, `exit`, or any
  user-defined function that isn't itself `@pure`. Compile-fail tests
  in `compiler/crates/kryos-test-runner/tests/e2e/error_cases/` lock
  this behavior in.
- `@capabilities(<set>)` is enforced inside any function carrying the
  annotation. Calls to stdlib paths that require capabilities not in
  the declared set produce compile errors. Calls from an annotated
  function to another annotated function also enforce attenuation —
  a `@capabilities(net)` function cannot call into a
  `@capabilities(io)` function unless its declared set is widened.
- Unannotated functions remain "ambient" — they don't enforce per-call
  capability gating. This is a deliberate v3 design: bare scripts
  don't need annotations to compile, but any function reaching for
  the stdlib's capability-gated paths must declare itself.

## 6. Known caveats at v3.0

These are honest disclosures, not breaking changes:

- **Async/await runtime model**: `async fn`, `await`, and the
  state-machine MIR lowering all exist and work for simple shapes
  (`tcp_listen` → `accept` → `send/recv` on a single task). The
  type-checker still passes the inner type of `Future<T>` through
  `await` instead of unwrapping it. Programs depending on
  `Future<T>` ⊢ `T` for inference may need explicit type annotations.
  Tracked in `AUDIT-v2.8.0.md` §4 for v3.0.x patch.
- **Recursive enum codegen on LLVM**: `Add(Expr, Expr)` style
  payload-as-handle is supported (heap-alloc + ptr). The parity
  matrix at `tests/parity/run_parity.sh` exercises the full smoke
  suite under both backends on every PR; at v3.0 release it reports
  **34/34 pass on both Cranelift and LLVM (100%)**, including the
  previously failing `test_match_return`, `test_tuple_mut`, and
  `test_generics`.

## 7. Migration from v2.8.x to v3.0

v3.0 is fully source-compatible with v2.8.x. No code changes required.

Tooling changes you may notice:

- `kryos --version` reports `3.0.0`.
- Some LLVM-backend bug fixes change observable behavior for programs
  that depended on previously-undefined behavior (e.g. assert_eq with
  mixed string/int args now prints both formatted operands; the
  previous behavior was a clang link error). If a `@test` function
  relied on a v2.8 LLVM-build failure, the test now actually runs.

CHANGELOG.md has the full v3.0 commit-by-commit list.

## 8. Reporting incompatibilities

Open an issue at
https://github.com/NORTHTEKDevs/kryos-lang/issues with:

1. `kryos --version`
2. The minimal `.kry` source that worked under v2.8.x but breaks under v3.0
3. The exact command line
4. Expected vs actual output

Source incompatibility within the v3.x line is treated as a P0 bug.
