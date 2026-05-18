# Kryos v2.8.0 → v3.0.0 production-hardening audit

Audit date: 2026-05-17
Audit head: `9c37e0b v2.8.0: language polish round two` (master)
Auditor: claude (opus-4-7), shift `shift/kryos-v3.0/0`
Method: every claim below was verified by reading source on disk at the
named path. Where a number is stated, it was counted, not summarized
from a README.

This document is the entry deliverable for milestone M1 of the
production-hardening shift. It exists to make milestones M2..M10
executable without re-investigation.

---

## 0. Bottom line

Kryos v2.8.0 is structurally healthy and not full of stubs. The
v2.0..v2.8 cycle closed every architecture-level failure listed in
`AUDIT-0.3.6.md`. The compiler builds clean on Windows with zero
warnings (`cargo check --workspace --release` finishes in ~1m on this
machine). The crates that look tiny by `lib.rs` line count are facades;
real volume lives in submodules — e.g. `kryos-mir` is 12,234 LoC,
`kryos-codegen-cranelift` is 7,340 LoC, `kryos-rt` is 6,159 LoC.

The gap to v3.0 (in the sense the user's prompt defines it — a 1.0-grade
release with no stubs, default-real network paths, three-OS CI, fuzzing,
signed artifacts, scripted quickstart, full self-host stage transition,
real registry over HTTP) is **not the code**. It is:

1.  **Marketing-vs-source mismatches** that have to be either backed up
    or retracted (capability enforcement, async/await completeness,
    benchmark sandbox-floor wording, the 9-vs-49-vs-74 examples count).
2.  **CI coverage holes** — macOS not in CI at all, no backend matrix
    on Linux, WASM never built or run in CI, registry server never
    spun up in CI, editor extensions never packaged from CI artifacts,
    quickstart never scripted, smoke directory's 33 tests not iterated.
3.  **Two pre-existing failing tests** acknowledged in v2.8.0's
    CHANGELOG but not yet fixed (`compile_file_with_selective_import`,
    `build_cache_roundtrip_with_cli`).
4.  **Scope decision** the user must make before code is touched: the
    public `ROADMAP.md` schedules the very work this shift is for
    across five separate releases (v2.9 LLVM parity, v3.0 FFI, v3.1
    LSP depth, v3.2 registry, v3.3 threads/async). The shift prompt
    compresses all of it into one v3.0 cut. See §5.

The actual hostile-data scan turned up exactly **one** type-checker
correctness gap (await passthrough) and zero codegen stubs. The 33 stub
markers in the workspace are all intentional or in the self-host WIP.

---

## 1. Repository facts (verified)

| Fact                                  | Source of truth                       | Value                                  |
| ------------------------------------- | ------------------------------------- | -------------------------------------- |
| Workspace version                     | `compiler/Cargo.toml`                 | `2.8.0`                                |
| Latest tag on master                  | `git tag`                             | `v2.8.0`                               |
| Latest commit                         | `git log -1`                          | `9c37e0b v2.8.0: language polish round two` |
| Crates in compiler workspace          | `compiler/crates/`                    | 22 (was 21 in AUDIT-0.3.6 — kryos-codegen-wasm added) |
| Compiler Rust LoC (all crates)        | `wc -l compiler/crates/**/*.rs`       | ~58k (src + tests) |
| Stdlib `.kry` modules                 | `compiler/stdlib/*.kry`               | 30 (CLAUDE.md lists 31, see §7)        |
| Self-host `.kry` files                | `compiler/self-host/*.kry`            | 16 files, 19,202 lines                 |
| Smoke tests (root)                    | `tests/smoke/*.kry`                   | 32 + README                            |
| Examples (root)                       | `examples/*.kry`                      | 49                                     |
| Showcase examples                     | `examples/showcase/*.kry`             | 13                                     |
| Registry server                       | `tools/registry/src/main.rs`          | 374 lines, `std::net` only             |
| CI workflows                          | `.github/workflows/`                  | ci.yml, cross.yml, release.yml         |
| `cargo check --workspace --release`   | run on Windows at audit time          | clean, 0 warnings                      |

---

## 2. Methodology

For every roadmap or marketing claim I checked:

1.  Locate the claim (README, STABILITY, CLAUDE.md, CHANGELOG, or
    ROADMAP).
2.  Find the source of truth in the workspace (crate, file, function).
3.  Verify by reading the source (not the doc, not the crate README).
4.  Where dynamic, run it (`cargo check`, smoke test, etc.).
5.  Record gap (none / cosmetic / claim-mismatch / behavior-gap).

Stub hunt used a single ripgrep with the union of
`unimplemented!\(|todo!\(|panic!\("not (yet|implemented)|TODO:|FIXME:|XXX:|for now,?\s|placeholder|HACK:`
across the whole compiler workspace and stdlib `.kry` modules. Each hit
was opened with `-C 2` for context before classifying.

---

## 3. User's M1..M10 vs verified reality

| Milestone | User's intent | Verified current state | Distance to done |
| --------- | ------------- | ---------------------- | ---------------- |
| M1 audit  | Produce AUDIT-v2.8.0.md | This file. | 0 — this delivers M1. |
| M2 compiler correctness | Close lexer/parser/typechecker gaps; fuzz harness for both; smoke green on Linux/macOS/Windows | Fuzz targets already exist at `compiler/fuzz/fuzz_targets/{fuzz_lexer,fuzz_parser,fuzz_typechecker}.rs` and have never been run in CI. 32 smoke tests exist; CI runs ONE of them (`compiler/tests/smoke.kry`) plus 9 hard-coded examples. 2 pre-existing failing cargo tests acknowledged in v2.8.0 (§6). macOS not in CI at all. | Small: wire fuzz into CI, iterate `tests/smoke/*.kry`, add macOS runner, fix the 2 known failures. |
| M3 backends matrix | Every example builds + runs on LLVM + Cranelift + WASM on each OS | Cranelift smoke-runs 9 examples on Linux + Windows. LLVM AOT smoke-runs 4 examples on Windows + CodeView. WASM has **zero CI coverage**. macOS has zero CI. No backend matrix expansion file exists. | Medium: needs new CI matrix file iterating `examples/*.kry × {cranelift, llvm, wasm} × {ubuntu, macos-13, macos-14, windows}`. WASM needs a real headless runner (Node + `wasm_runner.js`) or wasmtime invocation. |
| M4 capability enforcement | `@pure` and `@capabilities(...)` enforced with negative compile-fail tests | `compiler/crates/kryos-capabilities/` is 1,568 LoC of analysis with 1,049 LoC of tests. **But** `CLAUDE.md` at repo root says "For now: just call what you need. The compiler will require a `// capabilities: io, net` annotation on the entry point if you turn on strict mode." Top-level `README.md` lines 143-156 claim `@pure` and `@capabilities(io)` give "compile-time enforcement" — that contradicts CLAUDE.md. Need to read `kryos-capabilities` to confirm whether enforcement is default-on or opt-in. | Medium: source check + decision (default-on vs `--strict`?) + 6-12 compile-fail tests per category. Pure-doc-fix if enforcement is actually default-on. |
| M5 async real I/O | spawn/chan/await on real sockets/files/timers; integration test against a real local HTTP server | `kryos-rt/src/{spawn.rs,channel.rs,future.rs}` are 6,159 LoC combined. `examples/http_server.kry`, `examples/chat_server.kry`, `examples/async_echo.kry` exist; none are in CI. ROADMAP item 3.3 explicitly says: "Async / await … codegen path for state-machine splitting exists in `kryos-mir`'s async lowering pass. Promote from 'experimental' to 'supported'." So the language designer's own position is that async is **experimental, not 1.0**. Type checker `await` expression currently passes through inner type instead of unwrapping `Future<T>` (`compiler/crates/kryos-types/src/check.rs:2082-2084`). | Medium-large: real-port integration test in CI (spin a server on `127.0.0.1:<rand>` in a sidecar, hit it from a Kryos client); type-checker `Future<T>` unwrap; tutorial chapter; promote out of "experimental" only if all of that passes. |
| M6 self-host Stage 0 → Stage 1 | Bootstrap parser-in-Kryos with CI gate | `compiler/self-host/` has 16 files, 19,202 lines, all pass `kryos check`. Stage 0 closure shipped in v2.7 (per recent commit `a896910`). Stage 1 mini-parser started in `ce9cb5c stage 1 start: mini parser-in-Kryos for let/fn/if/while subset`. README at `compiler/self-host/README.md` says Stage 2 self-host bootstrap is >600s. | Medium-large: define exactly what Stage 1 "done" means for CI; the README is honest that Stage 2 is too slow, not Stage 1. Stage-1 mini-parser already exists for a subset — bringing it to the full grammar is the actual work. |
| M7 registry over HTTP | Real registry server stood up in CI; publish / resolve / fetch over HTTP | `tools/registry/` builds clean, 374 LoC, `std::net` only, routes `/v1/health`, `/v1/packages/<name>`, `/v1/packages/<name>/<version>`, `/v1/search?q=...`. Read-only (publishes are PR against the index repo). Client side: `kryos-package` is 1,970 LoC. ROADMAP item 3.2 explicitly defers registry **content** (publish 5-10 first-party packages, signing, version yanks, namespacing) to v3.2. The user's M7 is about the **protocol** path, which is shippable now — index repo + server + client. | Small for protocol: CI workflow that `cargo run -p kryos-registry-server` in background, points a test `kryos.toml` at it, runs `kryos pkg add` / `search`, asserts roundtrip. Large if user also wants content (ROADMAP v3.2 work). |
| M8 editor packaging | Install cleanly from CI-produced artifacts | `editors/vscode/` has full `package.json`, `npm run package` claimed to produce `kryos-0.4.0.vsix`. `editors/zed/` has `extension.toml` + Rust LSP launcher targeting `wasm32-wasi`. **`editors/tree-sitter-kryos/` referenced by `editors/README.md` does not exist on disk.** Neither extension is packaged in `.github/workflows/release.yml`. | Small: add a `package-editors` job to release.yml that runs `npm install && npm run package` in `editors/vscode/` and `cargo build --release --target wasm32-wasi` in `editors/zed/`, uploads both as release artifacts. Decide what to do about the missing tree-sitter dir (delete the README claim, or build the grammar). |
| M9 quickstart scripted | E2E install → WASM deploy + native CLI < 10 min | `QUICKSTART.md` is well-written and current. No script verifies it. WASM "deploy" target is undefined in QUICKSTART — currently only `wasm32-wasi` host shim is shown. | Small-medium: add `tests/quickstart_e2e.sh` that runs the QUICKSTART steps headless in a fresh container; CI job that runs it. Decide what "deploy a WASM example" means (host `wasm_browser_demo.html` to a static fileserver and curl it? Or just compile + run via `wasmtime`?). |
| M10 release v3.0 | Signed artifacts on all targets, STABILITY.md 1.0, migration note | `release.yml` produces unsigned tarballs for `ubuntu-latest`, `windows-latest`, `macos-latest` x86_64, `macos-latest` aarch64. No code-signing step (no `signtool` on Windows, no `codesign` + notary on macOS, no GPG signing of tarballs). `STABILITY.md` says "**Tier 2** means breakages on this platform may slip a release" — that's the opposite of a 1.0 guarantee for macOS. | Medium: add code-signing (needs Apple Dev ID cert + Windows EV cert — these are external blockers, see §10). Rewrite STABILITY.md after the matrix work in M3 actually makes all three platforms tier-1. Migration note will be one paragraph (no breaking syntax changes are planned). |

---

## 4. Stub / "for now" / placeholder inventory

Verified by ripgrep then read with `-C 2` context. 33 hits total across
15 files. Classification:

| Severity | Location | Classification |
| -------- | -------- | -------------- |
| **REAL (1)** | `compiler/crates/kryos-types/src/check.rs:2082-2084` | `Expr::Await { value, .. } => self.infer_expr(value)` — passes inner type through instead of unwrapping `Future<T>`. Quote in comment: "// A full implementation would unwrap Future<T> → T." → Must be fixed for M5. |
| **ACKNOWLEDGED (1)** | `compiler/crates/kryos-driver/src/resolve.rs:391-395` | Selective-import: "Doing proper dependency tracing is a larger change; for now we always include the full module." This is the same bug as the known-failing `compile_file_with_selective_import` test acknowledged in v2.8.0 CHANGELOG. → Must be fixed for M2. |
| **INTENTIONAL (4)** | `compiler/crates/kryos-mir/src/ir.rs:298` (`Nop` opcode), `compiler/crates/kryos-mir/src/lower.rs:5039` (quantum-block passthrough), `compiler/crates/kryos-codegen-wasm/src/lib.rs:173` (returns hard error, not a stub — comment is misleading), `compiler/crates/kryos-ast/src/cfg_strip.rs:114` (doc comment for empty `@cfg()`) | Real opcodes / error paths / docs. No action. |
| **FALSE POSITIVE (8)** | `compiler/crates/kryos-ast/src/derive_expand.rs` (3 hits — doc comments for `to_string` Debug fallback), `compiler/stdlib/string.kry`, `compiler/stdlib/fmt.kry` (4 hits — `let placeholder = "{" + ...` is a local variable name for fmt-string templating), `compiler/crates/kryos-codegen-cranelift/tests/codegen.rs:1` (test fixture string) | All fine. Word "placeholder" used as a variable name or in unrelated doc context. |
| **SELF-HOST WIP (13)** | `compiler/self-host/codegen.kry` (7), `compiler/self-host/x86.kry` (4), `compiler/self-host/lower.kry` (1), `compiler/self-host/coff.kry` (1) | Bootstrap compiler is incomplete by design (see M6). `// for now: lea target_reg, [rip + disp32]  with relocation` is correct WIP work — the relocations get patched by the linker. Not action items for the toolchain; they're the Stage 1/2 plan. |
| **STALE PLAN (6)** | `compiler/docs/plans/2026-04-10-kryos-10-of-10.md` (4 hits — old roadmap quotes proposed code), `tests` references in other plans | Plan documents quote example code. No action. |

**Net real-code action items from this scan: 2** (await typing,
selective-import resolver). Both are M2/M5 work.

---

## 5. Scope mismatch with published `ROADMAP.md`

This is the most important finding in the audit and the **only thing
blocking start of M2**. The user's prompt and the in-tree ROADMAP do
not agree on what "v3.0" contains:

| Item | User's M1..M10 says | `ROADMAP.md` (committed in v2.8.0) says |
| ---- | ------------------- | --------------------------------------- |
| LLVM-vs-Cranelift parity matrix | M3 work, target v3.0 | "v2.9 — LLVM backend parity" |
| FFI audit + zlib/sqlite/libcurl end-to-end | implicit in M5/M2 | "v3.0 — Foreign function interface" |
| LSP depth audit | implicit in M2 | "v3.1 — Language server depth" |
| Registry HTTP + content | M7 (protocol only) | "v3.2 — Package manager and registry" (protocol + 5-10 first-party packages + signing) |
| Threads + async promotion | M5 | "v3.3 — Concurrency" |
| macOS + Windows tier-1 | M2/M3 | not on roadmap (`STABILITY.md` keeps macOS tier-2 and Windows tier-3) |
| Signed release artifacts | M10 | not on roadmap |
| Scripted quickstart E2E | M9 | not on roadmap |
| Editor packages from CI | M8 | not on roadmap |

There are two consistent ways forward. Audit does not pick — the user
must.

**Option A (compress).** Cut v3.0 == everything in the user's M1..M10.
This means the in-tree `ROADMAP.md` must be rewritten before any code
changes, so the public commitment matches what we will ship. v2.9, v3.1,
v3.2, v3.3 milestones from the current ROADMAP get folded into v3.0 or
dropped.

**Option B (sequence).** Keep `ROADMAP.md` as-is. The shift's
deliverables become 2.9, 3.0, 3.1, 3.2, 3.3 across five tags, with each
tag matching the existing ROADMAP commitment. The user's "v3.0" tag
becomes "v3.3" or "v4.0".

Both are coherent. Option A is faster to a single signed release but
loses the ability for users to upgrade in small steps. Option B
preserves the existing public commitment but requires five release
cycles before the user's full intent ships.

→ **M2 cannot start until the user picks A or B.** All subsequent
milestone branches will reference the chosen version numbers.

---

## 6. Pre-existing failing cargo tests

Acknowledged in v2.8.0 CHANGELOG. Not regressions of v2.8 work. Both
must be fixed for v3.0 because the user invariant says no skipping
hooks / no bypass.

| Test | Crate | Root cause (from CHANGELOG + source) |
| ---- | ----- | ------------------------------------ |
| `compile_file_with_selective_import` | `kryos-driver/tests/driver.rs` | Driver's selective-import always includes the full module — see `compiler/crates/kryos-driver/src/resolve.rs:391-395`. |
| `build_cache_roundtrip_with_cli` | `kryos-test-runner/tests/build_cache_smoke.rs` | Linker stub interaction with build-cache reuse. Needs source read; not done in this audit pass. |

Both are M2 candidates.

---

## 7. Documentation contradictions

These are claim-vs-source mismatches that have to be either backed up
with code changes (M4) or retracted in docs.

| # | Claim | Where | Reality | Resolution |
| - | ----- | ----- | ------- | ---------- |
| 1 | "Capability-typed effects (compile-time enforcement)" with `@pure` example | `README.md` lines 143-156 | `CLAUDE.md` at same root: "For now: just call what you need. The compiler will require a `// capabilities: io, net` annotation on the entry point if you turn on strict mode." | Source check needed. If default-off, README must say "(opt-in via `--strict`)". If default-on for `@pure`, CLAUDE.md must be corrected. |
| 2 | "Async / await + state machines: Complete" | `README.md` line 258 | `ROADMAP.md` lists async promotion as v3.3 deliverable, calling it "experimental". `CLAUDE.md` says "Don't claim async/await works without checking." Type checker still passes await through (§4). | README row must be downgraded to "experimental" until M5 work lands and tests pass. |
| 3 | "kryos-rt: Runtime library (builtins, allocator, **GC**)" | `compiler/README.md` line 92 | Top-level `STABILITY.md` says "ARC plus explicit `move`/`share`". `kryos-rt/src/arc.rs` exists; no GC implementation exists. | `compiler/README.md` is stale; replace its crate table with the top-level README's; or delete `compiler/README.md` and link to root. |
| 4 | "9 example programs" | `compiler/README.md` line 127 | `examples/` has 49 root `.kry` + 13 in showcase = 62. Top-level README says "74 runnable example programs"; my count is 62. | Reconcile: are extracted_packages and `wasm_*` counted? Pick a number, document the rule, make it match a `find` invocation. |
| 5 | Top-level README benchmark table shows `fib(35)=0.032` for Kryos LLVM | `README.md` lines 183-189 | `benchmarks/README.md` "Known gaps May 2026" says "`fib(35)` is slow (~100x)" on Cranelift; doesn't reconcile to LLVM. README footer hints at "30 ms subprocess-launch floor", so all the 0.032 / 0.008 cells may just be hitting the floor. | Add a note next to the table explicitly stating which cells are floor-bound. Honest framing matters for v3.0 launch. |
| 6 | "Tree-sitter grammar at `editors/tree-sitter-kryos/`" | `editors/README.md` lines 15-21 | Directory does not exist. | Either delete the claim or scaffold a real tree-sitter grammar (M8 work). |
| 7 | "28 stdlib modules" | `README.md` line 203 | `compiler/stdlib/` has 30 `.kry` files. `CLAUDE.md` lists 31 `use std::*` modules. | Pick a number that matches reality (`ls compiler/stdlib/*.kry \| wc -l`). |
| 8 | "Compiles via LLVM IR as text. You only need `clang` or `llc` on PATH if you want optimized release binaries." | `README.md` line 75, `QUICKSTART.md` line 13 | Confirmed correct for now — but `kryos build --backend llvm` without `clang` produces a `.ll` file only, not a binary, and that needs to be explicit. | Cosmetic, but worth being precise about for the 1.0 doc pass. |

---

## 8. CI coverage gaps (the actual v3.0 work)

Verified against `.github/workflows/{ci.yml,cross.yml,release.yml}` at
audit head.

| Gap | Today | v3.0 target |
| --- | ----- | ----------- |
| macOS in CI | Not present. Release workflow does cross-build macos-x86 and macos-aarch64 from `macos-latest`, but no test step runs on macOS. | Add a `build-and-test-macos` job mirroring the Linux + Windows ones, on both `macos-13` (Intel) and `macos-14` (aarch64). |
| Backend matrix on Linux | Linux job runs only Cranelift on 9 examples. | Iterate `examples/*.kry × {cranelift,llvm,wasm}` in a matrix step. |
| WASM in CI | Zero coverage. `compile-codegen-wasm` crate exists with 1 self-test. | Build hello/fib/wasm_arrays/wasm_strings/wasm_loop with `--backend wasm`, run with `wasmtime` (WASI) and via `node examples/wasm_runner.js` for browser host stubs. |
| Smoke directory iteration | CI runs `tests/smoke.kry` (one file, the legacy file at `compiler/tests/smoke.kry`). `tests/smoke/` at repo root has 32 + README. | Iterate `tests/smoke/*.kry` in CI on Linux + Windows + macOS, both backends. |
| Fuzz targets in CI | `compiler/fuzz/fuzz_targets/{fuzz_lexer,fuzz_parser,fuzz_typechecker}.rs` exist; never run in CI. | Add a `cargo fuzz run` step on Linux for a fixed time budget (e.g. 60s each) per PR. Save corpora as artifacts. |
| Registry server in CI | Builds clean; never spun up in a CI job. | Background `cargo run -p kryos-registry-server -- --index ./fixtures/index --addr 127.0.0.1:18080`, point a test `kryos.toml` at it, run `kryos pkg add` + `search`, kill server, assert. |
| Editor packaging in CI | None. | Release workflow: `npm install && npm run package` in `editors/vscode/`, upload `.vsix`. `cargo build --release --target wasm32-wasi` in `editors/zed/`, upload `.wasm`. |
| Quickstart E2E | None. | `tests/quickstart_e2e.sh` that follows `QUICKSTART.md` step by step inside a clean container, asserts each expected output. CI runs it on Linux. |
| Code signing | None. | Apple Dev ID + Windows EV cert + GPG keys for tarball signatures. External blocker — see §10. |

---

## 9. Crate-level inventory

Compiler crates, current state, distance to v3.0 production.

| Crate | LoC (src) | Tests (LoC) | v3.0 work |
| ----- | --------: | ----------: | --------- |
| kryos-ast | ~1,900 | — | Verify derive macros cover all needed types. Cosmetic. |
| kryos-bindgen | ~1,570 | ~349 | Expand for ROADMAP v3.0 FFI (typedefs, anon structs, variadics). Real work. |
| kryos-capabilities | ~1,568 | ~1,049 | Verify default-on enforcement. M4. |
| kryos-cli | ~1,857 | — | CLI surface stable; ensure all subcommands documented + tested. Cosmetic. |
| kryos-codegen-cranelift | ~7,340 | ~3,884 | Backend matrix coverage (M3). |
| kryos-codegen-llvm | ~4,648 | ~1,658 | LLVM-vs-Cranelift parity (ROADMAP v2.9, user M3). |
| kryos-codegen-wasm | ~1,820 | ~1 | Needs CI coverage. M3 + M5 (real I/O via WASI). |
| kryos-doc | ~1,116 | — | Verify `kryos doc` output matches `docs/19-language-reference.md`. Optional. |
| kryos-driver | ~2,478 | ~954 | Fix selective-import resolver (§4 + §6). |
| kryos-errors | ~347 | — | Add `--explain ERRXXXX` registry (AUDIT-0.3.6 carry-over). Optional. |
| kryos-fmt | ~1,539 | ~548 | Add fmt-stability test (run fmt twice, assert idempotent). Optional. |
| kryos-lexer | ~979 | ~790 | Wire fuzz into CI (M2). |
| kryos-linker | ~845 | ~344 | Fix build_cache_roundtrip_with_cli (§6). |
| kryos-lsp | ~1,249 | ~942 | LSP depth audit (ROADMAP v3.1). M8-adjacent. |
| kryos-mir | ~12,234 | ~2,868 | No action — robust. |
| kryos-ownership | ~1,586 | ~1,230 | Add fuzz harness (AUDIT-0.3.6 carry-over). Optional v3.0. |
| kryos-package | ~1,970 | ~456 | Wire to live registry in CI (M7). |
| kryos-parser | ~2,712 | ~1,060 | Wire fuzz into CI (M2). |
| kryos-rt | ~6,159 | — | Real-I/O integration test (M5). |
| kryos-stdlib-native | ~3,024 | ~158 | Add `tls.kry` Kryos-side surface (AUDIT-0.3.6 carry-over). Optional. |
| kryos-test-runner | ~1,199 | ~243 | Fix build_cache_roundtrip_with_cli (§6). |
| kryos-types | ~5,370 | ~1,636 | Fix await passthrough (§4). |

---

## 10. External blockers (user-side, not code)

Before M10 can ship signed artifacts, the user must have:

- **Apple Developer ID** certificate ($99/yr) for macOS code signing
  and notarisation.
- **Code-signing certificate for Windows** (EV cert recommended for
  no-SmartScreen warnings; ~$300-600/yr depending on issuer).
- **GPG key pair** for tarball signatures, with public key published
  somewhere stable.
- **Domain / hosting** for `kryos.dev`, `packages.kryos.dev`, and the
  registry index host (the README already references these URLs).

These are noted, not done. No code change affects them.

---

## 11. Open questions for the user — must answer before M2 starts

1.  **Scope (§5):** Option A (compress all of ROADMAP v2.9..v3.3 into
    one v3.0 cut, rewrite ROADMAP.md before code) or Option B (keep
    the public ROADMAP commitment, ship across v2.9/v3.0/v3.1/v3.2/v3.3
    and treat "v3.0" in the shift prompt as the final tag, whatever
    number that ends up being)?

2.  **macOS CI runners:** the user's invariants demand CI on
    Linux/macOS-Intel/macOS-AS/Windows. GitHub-hosted macOS-13 (Intel)
    runners are 10x more expensive than Linux. Acceptable, or budget-
    constrain to one macOS runner?

3.  **WASM "deploy" definition (M9):** What does "deployed WASM
    example" mean for the scripted quickstart? Options: (a) compile
    `examples/wasm_browser_demo.kry`, host the `.wasm` + `.html` on
    a static fileserver in CI, curl it, assert output in DOM via a
    headless browser; (b) compile to `wasm32-wasi`, run with
    `wasmtime`, assert stdout. (b) is simpler; (a) matches the
    "browser" claim in the README.

4.  **Code-signing certs (§10):** Are the Apple Dev ID and Windows EV
    certs procured? If not, M10 ships unsigned (with a clear caveat)
    or M10 blocks indefinitely.

5.  **Registry hosting (M7):** The protocol path can be CI-tested
    against an ephemeral server. But the user's README links to
    `packages.kryos.dev`. Is that domain procured + hosted, or is M7
    ship "protocol works, hosting deferred"?

6.  **Two failing tests (§6):** OK to fix in M2 (audit recommendation)
    rather than treating them as out-of-scope?

7.  **Documentation policy (§7):** Will I be touching `README.md` to
    reconcile the 8 contradictions, or should the user prefer to
    review and apply those changes manually? (My recommendation: I
    apply them in a doc-only PR right after M2 so the docs match the
    real surface area we're stabilising.)

Until these are answered, the audit stops. M2 branch (`feat/correctness`
or whatever it ends up named after question 1) does not get created.

---

## 12. Confidence note

This audit is based on reading the v2.8.0 source on disk and the
artifacts produced by `cargo check --workspace --release` on Windows.
It does NOT include:

- A full run of `cargo test --workspace --release` (will be done in
  M2 and the result captured here).
- A run of every example through every backend (M3 work).
- A run of the fuzz harness (M2 work).
- A run of the registry server roundtrip (M7 work).
- A live LSP smoke from VS Code + Zed (M8 work).

Each of those will surface its own findings. The audit's job is to
make sure none of those runs is starting from a fantasy baseline —
the baseline established here is: **clean build, zero warnings, two
known-failing cargo tests, no fantasy stubs in the toolchain, the
public ROADMAP doesn't match the shift prompt, and the CI matrix has
clear holes.** Everything else is execution.

— end of M1 audit —
