<!--
Auto-generated audit snapshot — 2026-06-15.
Produced by a parallel read-only audit (8 mappers + synthesis) and verified against ground-truth git.
This is a point-in-time map for decision-making, not normative documentation.
-->

# Kryos + Ecosystem — State of the Union (2026-06-15 snapshot)

> **Sync status (resolved):** This snapshot was taken while the local checkout was 24 commits behind
> `origin/master`. Local has since been fast-forwarded to `origin/master` (HEAD `7c32214`); everything
> language + ecosystem is present and matches GitHub. The "Repo & Git Reality" section below describes
> the *pre-sync* local state and is retained as historical context.

## Capability badging (Project 05) — hardening follow-up

Project 05 shipped via PR #73 (`a77859d`). An independent adversarial review (8 invariants + a 44-input
generalization probe) of an equivalent implementation surfaced three hardening gaps that the merged
version does **not** yet have. These are precise, low-risk follow-ups (suggest a small PR):

1. **`pkg audit` / `pkg show` of a name not in the registry returns exit 0.** A typo'd package name in CI
   is then indistinguishable from a clean audit. Fix: the registry-lookup `None` arm should return a
   non-zero error, not fall through to `Ok(())`.
2. **`CapsBadge` deserialized from an untrusted registry entry is not normalized.** A crafted entry can
   carry `dangerous` that disagrees with `capabilities`, an out-of-range `annotation_coverage_pct` (which
   suppresses the "coverage < 100%" warning), or unsorted/duplicated lists. Fix: a `normalize()` applied
   on `from_json` / index-parse that re-derives `dangerous` from `capabilities`, clamps coverage to ≤100,
   and sorts/dedups. (`audit` should also re-derive `dangerous` from the capability list rather than
   trusting the stored field.)
3. **`generate_index_entry` builds JSON with unescaped `format!()`** for name/version/dependency strings.
   A name or dep containing `"` or a newline breaks JSON/NDJSON validity. Fix: route string values through
   a JSON escaper (serde_json).

A reference implementation of all three exists locally (the equivalent build session) and is preserved on
this machine; it is not pushed because it is a structurally divergent re-implementation of an
already-merged feature.

---

# Kryos + Ecosystem - State of the Union

## Executive Summary

Kryos is a production-capable, capability-safe systems programming language positioned as "the language for trustworthy AI agent software." Version v1.0.0-beta.1 (22-crate Rust compiler, Cranelift JIT + LLVM AOT backends, 66 stdlib modules, self-hosting Stage 1 working) was released on 2026-06-11 and remains in a private GitHub repo. The strategic wedge is five language-level AI governance axes - @budget (spend enforcement that throws on exhaustion), @capabilities (compile-time authority), Tracked<T> (provenance), Probable<T> (confidence), and ComputeCost (cost) - none of which exist as language primitives in any competing shipping language. Nine of 12 planned ecosystem projects are built and committed. The single most important current reality: the repo is PRIVATE, CI is billing-blocked (GitHub Actions spending limit exhausted), and the playground deployment gate fails open because no release binary with `kryos manifest` exists yet. Public launch is the defining open item, and three specific Kristian actions gate it: raise GitHub billing limit, flip repo public, cut a tagged release from master.

---

## State at a Glance

| Component | Status | One-Line |
|---|---|---|
| Compiler (Rust, Stage-0) | Production | 22-crate workspace, Cranelift + LLVM + wasm backends, parity 34/34 on both |
| Language (v1.0.0-beta.1) | Production | Generics, closures, actors, channels, ownership, capabilities - core feature set declared complete |
| Standard Library | Production | 66 .kry modules (~15,487 lines), majority pure Kryos after v4.41 rewrite |
| Self-Host (Stage 1) | Working | Stage-1 binary compiles; Stage-2 blocked by scale-dependent regalloc miscompile |
| Self-Host (Stage 2/3) | Blocked | Field-offset resolution degrades at full 20k-line scale; fixed point NOT verified |
| CLI + Tooling | Working | 37 subcommands including 14 pkg sub-subcommands, LSP, formatter, doc gen, coverage |
| Package Manager | Partial | kryos.toml + NDJSON index code is complete; 5 starter packages in local examples/, none indexed in live registry-index repo |
| Capability Badging (Project 05) | Working | CapsBadge schema shipped (PR #73 merged); no packages in live registry have badges yet |
| Benchmarks | Working | All 7 benchmarks within 1.45x of Rust; beats Rust on matmul (0.95x) and hashmap (0.68x) |
| Ecosystem Apps (Projects 01-10) | Working | 9 of 12 projects built/committed; 3 remaining (06 unclear, 11 not started, 12 not started) |
| 12-Project Roadmap | Partial | 9 built, 2 not started (11, 12), status of 06 contradicted across maps |
| @capabilities enforcement | Partial | Opt-in only; unannotated functions are unconstrained - the honesty gap |
| @budget enforcement | Working | Throws on token/call exhaustion via stdlib hooks in std::llm; USD/energy axes at 0.0 |
| CI / GitHub Actions | Blocked | Spending limit exhausted; all jobs fail in 2 seconds |
| Repo visibility | Blocked | PRIVATE; free minutes and public attestation require flip-to-public |
| Playground (play.kryos.dev) | Blocked | Dockerfile pins KRYOS_VERSION=v2.3.0 (predates `kryos manifest`); gate fails open |

---

## Compiler

**Version:** 1.0.0-beta.1 (`compiler/Cargo.toml` workspace.package.version)

**Crates (22):** kryos-errors, kryos-ast, kryos-lexer, kryos-parser, kryos-types, kryos-ownership, kryos-capabilities, kryos-mir, kryos-codegen-cranelift, kryos-codegen-llvm, kryos-codegen-wasm, kryos-linker, kryos-bindgen, kryos-stdlib-native, kryos-driver, kryos-cli, kryos-lsp, kryos-package, kryos-rt, kryos-doc, kryos-test-runner, kryos-fmt.

**Backends:**
- Cranelift (cranelift-codegen 0.116): default for `kryos run` (JIT, no external linker dependency)
- LLVM: default for `kryos build --release` (emits .ll, compiles via clang); http2_get/https_get AOT link gap fixed in PR #69
- Wasm (v0.1 MVP subset): explicit `--backend wasm`, handles scalars/arrays/basic control flow only; no structs, closures, or stdlib; JS host only, no WASI

**Pipeline:** source -> lex -> parse -> type check -> ownership -> capabilities -> MIR -> codegen -> link. Backends implement a Backend trait and are injected at driver call time.

**Memory constraints:** `compiler/.cargo/config.toml` sets `jobs=2` globally; Cranelift crate debug builds consume 10-15 GB per rustc invocation. `[profile.dev]` sets opt-level=1 globally and opt-level=2 for cranelift crates.

**Test gates:**
- Parity: `tests/parity/run_parity.sh` - latest captured result (SHA 0977714): 34/34 both_pass, 0 failures. 55 smoke .kry files exist but the modules/ subdirectory is excluded from the top-level glob, so true coverage is 34, not 55.
- Conformance: 6 files in `tests/conformance/`, all passing.
- Examples gate: `tests/run_examples_gate.sh` - 3-layer sweep (check root examples, run 17 fixtures, AOT-compile 24 showcase apps).
- kryos-codegen-cranelift is explicitly excluded from `cargo test` in run_tests.sh (processes are killed first).

**Self-host (`compiler/self-host/`):** 16 Kryos files, 19,202 lines total.

| Stage | Status |
|---|---|
| Stage-0 (Rust compiler builds Kryos) | Working |
| Stage-1 (stage-0 output compiles self-host source) | Working |
| Stage-2 (stage-1 binary compiles self-host) | Blocked - scale-dependent regalloc miscompile |
| Stage-2 == Stage-3 fixed point | Not run |

Stage-2 blockers (documented in `compiler/self-host/STAGE2_BLOCKER.md` and `STAGE2_MEMORY_BLOCKER.md`):
1. Field-offset resolution in linear-scan regalloc degrades from correct offset N to offset 0 at full 20k-line compilation scale; works on small inputs.
2. `parse_primary` mis-reads a token field (call-init locals typed as ANY, field access reads index 0). ~100+ call-init sites need annotation or a general fix in lower.kry.
3. The general typed-call-result fix in lower_fn_call triggers `_kryos_clone_Annotation` infinite recursion (a Stage-0 @copy-clone-gen interaction bug).
4. Retain/release in Stage-0 codegen is unbalanced; `*_free` functions are no-ops (H41 leak-on-zero model). Deallocation cannot be re-enabled until RValue/Operand/arg-passing/match-destructuring paths are audited.
5. `determinism.sh` exists but regalloc/temp-order non-determinism means consecutive Stage-2 object files differ in hash, which would block a byte-identity check even if Stage-2 completed.

The O(N^2) @copy Lexer clone that previously caused OOM was resolved 2026-05-24; the remaining Stage-2 blockers are codegen correctness bugs, not memory.

---

## Language

**Version:** v1.0.0-beta.1. Internal sprint numbering peaked at v4.46 before June 2026 semver recalibration (documented in `VERSIONING.md`).

**Core features (shipped):**
- Move semantics, single ownership, explicit clone; @copy for value types
- Hindley-Milner type inference + monomorphized generics (multi-param impl blocks shipped 2026-06-13, step 246)
- Result<T,E>, Option<T>, `?` operator - both backends
- Closures: bar syntax `|x| x+1`, captured by value, HOF type inference works
- Pattern matching: enums, tuples, literals, or-patterns; struct enum variants NOT supported (use tuple variants)
- Actors: serialized method calls
- Channels: unbounded MPMC (spawn/chan/send/recv builtins); bounded channels not implemented
- @capabilities: 11 variants (Net, Io, Ffi, Compute, Crypto, Process, Env, Term, Db, Time, All) - compile-time enforced, propagated across call graph
- @budget(tokens=N, calls=M): runtime-enforced via std::llm hooks; throws on exhaustion; USD/energy axes at 0.0 (not wired)
- async/await: GRAMMAR ONLY - lowers to synchronous call; no non-blocking executor exists
- LSP (diagnostics, completion, hover, goto-def), formatter, doc generator all present

**Known language limitations:**
- async/await is sync-only (no async executor)
- Bounded channels not implemented
- Sub-capabilities (e.g. filesystem:read vs filesystem:write) documented but not implemented
- @capabilities is opt-in; unannotated functions are unconstrained (the honesty gap; fixed by Project 11)
- @budget is not a general compiler-verified attribute - enforcement is stdlib hooks in std::llm only
- Explicit lifetimes, closure explicit capture lists, const generics, procedural macros: all "not yet"
- Module resolver does not follow transitive FFI references from use-imported functions

**Stdlib:** 66 .kry modules, ~15,487 lines. After the v4.41 rewrite, kryos-stdlib-native (Rust) contains only true syscall shims; the rest is pure Kryos. Planned but not yet authored: std.server, std.auth, std.config, std.email, std.claude, std.stripe (6 modules, docs only).

**Active toolchain gotchas:**
- `kryos test` panics for any std::cost importer (ComputeCost.to_string f64->i64 JIT codegen bug) - use `kryos run test_driver.kry`
- `assert_eq` broken for str in JIT; use `assert(a==b)`
- `env_get` and `exit` require @capabilities(process), not env (discrepancy vs CLAUDE.md which documents env_get as no-capability - see Contradictions)
- `http_get` is a phantom (undefined); use `http2_get`
- @budget and std::llm require the HEAD toolchain binary; the installed v4.43.0-rc.4 predates @budget

---

## CLI + Package Manager + Registry

**CLI:** 37 subcommands (`compiler/crates/kryos-cli/src/main.rs`). Key:
- `kryos build` (Cranelift/LLVM/wasm, --release, --backend, --emit-mir, --emit-llvm, --lto, -g, --cache)
- `kryos run`, `kryos check [--watch]`, `kryos eval`, `kryos test`, `kryos repl`
- `kryos manifest --caps`: writes `target/caps.json` via kryos-capabilities::extract_package_caps; prints human summary. The `--deny <caps>` flag gates CI exit code.
- `kryos pkg` (14 sub-subcommands): init, add, remove, update, install, lock, publish, search, info, sync, outdated, list-local, show, audit
- `kryos pkg show` / `kryos pkg audit`: print capability badge and diff dangerous caps between versions
- `kryos new --template (cli|http|lib|agent)`

**CapsBadge schema (`kryos-caps/1`, `compiler/crates/kryos-package/src/caps.rs`):**
- Fields: schema, capabilities (Vec<String>), per_function (BTreeMap, elided if >100 fns), dangerous, annotation_coverage_pct, inferred_uncovered
- DANGEROUS_CAPS = ['ffi', 'process'] - 'net' is NOT dangerous; a package silently adding network access only warns, does not fail `pkg audit`

**Registry architecture - three distinct components:**

| Component | Repo | Local path | Purpose |
|---|---|---|---|
| NDJSON index | NORTHTEKDevs/kryos-registry-index | `~/.kryos/registry/index/` (bare clone) | Authoritative package entries, one NDJSON line per version under `<two-char-prefix>/<name>.json` |
| Registry server | NORTHTEKDevs/kryos-registry-server | `/c/Users/Krist/projects/active/kryos-registry/` | Next.js API shim over index repo, deployed at packages.kryos.dev |
| Source of truth for Kryos pkg tooling | kryos-registry-index | synced via `git clone/pull` to `~/.kryos/registry/index/` | RegistryClient.lookup() reads from here |

**Critical distinction:** `kryos-registry` (local at `/c/Users/Krist/projects/active/kryos-registry/`) is the Next.js WEBSITE, not the NDJSON index. The NDJSON index is a separate repo (kryos-registry-index) with a local cache at `~/.kryos/registry/index/`. The index cache contains 5 packages (http-router, json, markdown, regex, sqlite). The index repo itself (NORTHTEKDevs/kryos-registry-index) has directory skeleton (aa/, bb/, cc/, kr/) but the 5 packages are indexed under `~/.kryos/registry/index/packages/` by two-char prefix (ht/, js/, md/, re/, sq/) - these may be committed to the index repo or may only exist in the local cache.

**Registry gaps:**
- Only http-router has a `target/caps.json` capability badge; json/markdown/regex/sqlite need `kryos manifest --caps` run
- `pack()` in registry.rs produces a text file listing, not a real .tar.gz (no tar/gzip dep) - described as "sufficient for v0.1"
- `--tarball-only` flag mentioned in kryos-registry-index README does not exist in current CLI
- `env_get` mapped to Capability::Process in model.rs but documented as no-capability in CLAUDE.md (discrepancy)

---

## Benchmarks

**Suite:** 7 programs, `benchmarks/` directory. Harness: `benchmarks/measure.py` (medians of 5 runs). Results in `benchmarks/results.json` (machine-generated, dated 2026-06-14) and mirrored to `BENCHMARKS.md`.

**Results (Windows 11 x64, rustc 1.95, clang/clang++ 21 -O2, Kryos LLVM backend):**

| Benchmark | Kryos | Rust | Ratio |
|---|---|---|---|
| fib(40) | 0.349s | 0.347s | 1.01x |
| mandelbrot 1000^2 | 0.368s | 0.368s | 1.00x |
| nbody 2M steps | 0.141s | 0.105s | 1.34x |
| binary_trees depth-16 | 1.098s | 0.759s | 1.45x |
| fannkuch-redux(10) | 0.197s | 0.195s | 1.01x |
| matmul 512^2 | 0.618s | 0.653s | **0.95x - beats Rust** |
| hashmap 1M+1M | 0.080s | 0.118s | **0.68x - beats Rust** |

binary_trees improvement from 6.1x to 1.45x was via Shared<T> heap pointers + recursive arc teardown (commit f9aa178); peak memory dropped from 1.1 GB to 24 MB leak-free.

Strategic framing: perf is table-stakes (must be in the systems-language tier), NOT the wedge. BENCHMARKS.md frames this as "Kryos is in the systems-language performance tier." The wedge is the five AI governance axes.

**Warning - stale results file:** `benchmarks/RESULTS.md` (generated by the older `run.sh`) still shows outdated numbers from an older methodology (best of 10, clang 19, gcc -O3 as C baseline) with nbody at 4.20x and binary_trees at 5.92x. This file is superseded by `BENCHMARKS.md` (measure.py) but has not been removed or clearly marked stale. It will confuse readers.

Mojo head-to-head: reference ports exist at `benchmarks/mojo/` (fib/mandelbrot/matmul only) but no Mojo toolchain is installed on this Windows host. Kryos numeric results are claimed to be "in Mojo's league" but this is unverified locally.

---

## Ecosystem Apps

All ecosystem code is written in Kryos (.kry), not Rust or TypeScript. The ecosystem/ directory is vendored into kryos-lang (commit e080077). Key paths:

**Built projects:**

| # | Project | Location | Status | Notes |
|---|---|---|---|---|
| 01 | kryos-manifest CLI | `compiler/crates/kryos-cli/src/commands/manifest_cmd.rs` | Shipped (c3aeac1, 18 integration tests) | Implemented in Rust inside compiler, not an ecosystem/ Kryos project |
| 02 | kryos-governed-agent stdlib extension | `compiler/stdlib/agent_bridge.kry` | Shipped (ec5f187, 6 unit tests) | 5 bridges: tracked_cost, tracked_merge, tracked_to_citation, budget_remaining, filter_confident |
| 03 | kryos-agent-loop | `ecosystem/kryos-agent-loop/` | Shipped (9361441 on master) | chat_tools_governed() with @budget(tokens=100000,calls=50); Claude + NVIDIA NIM demos |
| 04 | kryos-rag | `ecosystem/kryos-rag/` | Shipped (bf8842f merged) | RAG pipeline with Tracked<str> citation lineage; ENTIRELY UNTRACKED in local working tree |
| 05 | Registry capability badging | `compiler/crates/kryos-package/src/caps.rs` + `extract.rs` | Shipped (PR #73 merged, 4e94dd7) | CapsBadge schema, `kryos pkg badge/show/audit` |
| 06 | Playground capability-gated sandbox | See Contradictions | CONTRADICTED | Maps disagree: one says spec-only, another says PRs merged |
| 07 | kryos-bench-governed | `ecosystem/builds/kryos-bench-governed/bench.kry` | Shipped (PR #68, f811d3f) | Single-file ~300 lines; requires HEAD compiler binary |
| 08 | kryos-audit-trail | `ecosystem/builds/kryos-audit-trail/` | Shipped (commits 745c9f0 + 24fb18d) | EU AI Act Annex IV compliance reporter; f64 codegen bug means tests run via `kryos run` not `kryos test` |
| 09 | kryos-mcp-governed | `ecosystem/` (kryos-mcp-template) | Shipped (PR #65 merged 9603139) | Capability-verified MCP server template; real fetch un-stubbed in 9796107 |
| 10 | kryos-calibration | `compiler/stdlib/probable.kry` | Shipped (commits bc8386e / 85c5eb9 / 36e6601) | CalibrationTracker + ece() in std.probable |
| 11 | Strict-capabilities deny-by-default | compiler change only | NOT BUILT | Remove `has_annotated_scope()` guard at checker.rs:537; ~2-3 days; highest ROI item |
| 12 | kryos-plugin-sandbox | - | NOT BUILT | Blocked on 11 + wasm struct/closure/stdlib coverage |

**Untracked items requiring git commit:**
- `ecosystem/kryos-rag/` - entire directory untracked locally (merged to origin/master in bf8842f but local copy never staged)
- `ecosystem/kryos-agent-loop/demo_nvidia.kry` - untracked

---

## Roadmap

**ECOSYSTEM.md recommended order:** 01 -> 02 -> (03, 10 parallel) -> 04/05 -> 06/07/08/09 -> 11 -> 12

**Current state vs plan:**

Projects 01-05 and 07-10 are built. Projects 06, 11, and 12 are the remaining work.

**Project 11 (strict-capabilities, ~2-3 days) is the single highest-ROI item.** It is the only change that makes the @capabilities "authority" wedge claim honest. Today, unannotated functions are unconstrained - the compile-time enforcement only fires if you opt in with @capabilities. Removing the `has_annotated_scope()` guard at `compiler/crates/kryos-capabilities/src/checker.rs:537` flips this to deny-by-default. Without it, the claim "Kryos enforces capability contracts at compile time" is technically opt-in documentation, not enforcement.

**Project 06 (playground sandbox):** Status is contradicted across maps (see Contradictions section). Cannot be stated as definitively built or not-built from the available evidence.

**Project 12 (wasm plugin-sandbox):** Blocked on both Project 11 and wasm backend gaining struct/enum/closure/stdlib coverage. The wasm backend is currently scalars/arrays/basic control flow only. This is a multi-week effort minimum.

**Remaining language work backlog (post-12):**
- @budget(usd=N) USD charge path (usd/energy axes currently 0.0)
- Sub-capabilities (filesystem:read vs filesystem:write)
- async/await with real non-blocking executor
- Bounded channels
- Explicit lifetimes, const generics, procedural macros

---

## Repo & Git Reality

**kryos-lang (NORTHTEKDevs/kryos-lang, PRIVATE):**
- Local master is 23 commits BEHIND origin/master - needs `git pull` (fast-forward)
- 1419 tracked files, 14 GB working tree, 296 MB .git
- 10 unstaged modified files (all in kryos-capabilities and kryos-package crates - session-11 capability/registry work)
- 5 untracked items: `compiler/crates/kryos-capabilities/src/extract.rs`, `compiler/crates/kryos-package/src/caps.rs`, `ecosystem/kryos-agent-loop/demo_nvidia.kry`, `ecosystem/kryos-rag/` (full directory), `scratch-http-router-badge.json`
- 1 stash on `feat/llvm-parity`: "wip-feat-llvm-parity-cleanup" - needs to be applied or dropped
- 70+ local branches (feat/v3.1 through v4.40, ecosystem/, fix/, shift/, worktree- branches)
- No spending-limit blocker text exists in `.github/workflows/` files - the CI failure is an account-level GitHub billing setting, not a workflow change

**Registry index (NORTHTEKDevs/kryos-registry-index - separate repo):**
- Local cache: `~/.kryos/registry/index/` (bare clone, latest commit fc1db7c)
- Contains 5 packages: http-router, json, markdown, regex, sqlite (under `packages/` by two-char prefix)
- The GitHub-hosted index repo has directory skeleton (aa/, bb/, cc/, kr/) committed, but whether the 5 package NDJSON files are committed to origin or only exist in the local cache is unclear from the maps

**Registry server (NORTHTEKDevs/kryos-registry-server):**
- Local path: `/c/Users/Krist/projects/active/kryos-registry/` (Next.js 16.2.7 + React 19 + TypeScript 5.7.2)
- Up to date with origin/master; 2 unstaged changes: package.json and package-lock.json (likely from a local npm install)
- Deployed at packages.kryos.dev on Vercel
- Reads from kryos-registry-index via GitHub API; requires GITHUB_TOKEN to read the private index repo

**CI workflows:** `ci.yml`, `cross.yml`, `release.yml`. Uses actions/checkout@v6, dtolnay/rust-toolchain@stable. Smoke tests 32 .kry files. The spending limit exhaustion is an account-level GitHub billing issue, not visible in workflow files.

---

## Open Decisions & Blockers

Ranked by importance:

**1. GitHub Actions CI billing - HARD BLOCKER**
All CI fails in 2 seconds. Private repo minutes are exhausted (macOS runner 10x multiplier is the likely accelerant). Local gates (parity 34/34, conformance 6/6, bootstrap Stage-1) are the verification floor. Resolution: Kristian raises spending limit at github.com/settings/billing, OR flips repo public (which grants free minutes). No in-repo change required.

**2. Repo is PRIVATE - launch gate**
kryos-lang is PRIVATE on NORTHTEKDevs. Public launch, free CI minutes, and user trust all require flipping to public. Kristian's decision; no technical blocker.

**3. Playground gate fails open - launch gate**
kryos-runner Dockerfile pins `KRYOS_VERSION=v2.3.0`, which predates `kryos manifest` (shipped in Project 01). The gate that checks manifest capability output cannot pass until a release binary with manifest is cut from HEAD and `KRYOS_VERSION` is bumped. No tagged release with manifest exists yet. Action: cut a v1.0.0-beta.1 (or v1.0.0) release tag from master, update Dockerfile.

**4. Project 11 (@capabilities deny-by-default) - honesty gap**
Currently @capabilities is opt-in. Unannotated functions are unconstrained. The "compile-time capability enforcement" claim in all marketing and ecosystem documentation is technically accurate only if you annotate everything. The fix is removing one guard at `compiler/crates/kryos-capabilities/src/checker.rs:537`. Estimated ~2-3 days. Without this, the authority wedge is documentation, not enforcement.

**5. Untracked local files - data loss risk**
The following must be committed before any branch switch or reset:
- `ecosystem/kryos-rag/` (full directory - project 04, merged to origin but not locally staged)
- `compiler/crates/kryos-capabilities/src/extract.rs` (new file)
- `compiler/crates/kryos-package/src/caps.rs` (new file)
- `ecosystem/kryos-agent-loop/demo_nvidia.kry`
- 10 unstaged modifications to kryos-capabilities and kryos-package

Additionally: pull 23 commits from origin before starting new session work.

**6. Registry index packages not confirmed committed to GitHub**
The 5 starter packages exist in the local `~/.kryos/registry/index/` cache and in `examples/extracted_packages/` locally. It is not confirmed that NDJSON entries for all 5 are committed to NORTHTEKDevs/kryos-registry-index on GitHub. `kryos pkg install` and `kryos pkg search` against the live registry will fail until entries are committed there. Only http-router has a capability badge; json/markdown/regex/sqlite need `kryos manifest --caps` run.

**7. Self-host Stage-2 blocked - not a launch blocker, but a credibility claim issue**
Stage-2 (self-hosted compiler compiling itself) is blocked by scale-dependent regalloc miscompile. The Stage-2 == Stage-3 fixed-point identity check has never been run. MEMORY.md claims "stage2==stage3==stage4, byte-identical bootstrap" - this appears to be aspirational or from a different version of the self-host than what is in the current tree, and is contradicted by the compiler area map which says Stage-2 is blocked.

**8. kryos-leads Neon DB - potential live cost**
MEMORY.md archives Kryos Leads as shut down 2026-05-31, but does NOT confirm the Neon project `kryos-leads` (us-west-2, project delicate-cloud-66306696) was deleted, unlike Northtek OS's Neon which was explicitly deleted 2026-06-13. If compute is running, this is an ongoing cost. Verify and delete if confirmed inactive.

**9. ECOSYSTEM.md status markers stale**
ECOSYSTEM.md marks only Project 01 as DONE. Projects 02-10 are also built and committed but ECOSYSTEM.md has not been updated to reflect this. Low priority but will mislead anyone reading the roadmap doc.

---

## Contradictions and Stale Claims

The following are direct conflicts between the area maps, or between the maps and other memory. Do not paper over these.

**Contradiction 1 - Project 06 (playground) status**
The ecosystem area map (kryos-lang/ecosystem/ directory) says Project 06 is "spec only at ecosystem/projects/06-kryos-playground-capability-gated-sandbox.md. NOT YET BUILT." The strategic memory map says "06 playground capability gate (DONE, kryos-runner PR#1 MERGED 33668ef, kryos-playground PR#1 MERGED 94f494b)." These are directly contradictory. The strategic memory is more recent (2026-06-15) and references specific PR numbers and SHAs; the directory map is based on what was observed in the filesystem. It is possible PRs were merged to the kryos-playground and kryos-runner repos (separate from kryos-lang) but ecosystem/projects/06-*.md was never updated. Resolution requires checking the kryos-playground and kryos-runner repos directly. Do not claim Project 06 is either done or not-done without verifying.

**Contradiction 2 - Self-host fixed point claim**
MEMORY.md and the strategic area map state "stage2==stage3==stage4, byte-identical bootstrap, verified across 9+ chain runs, fixed point HELD at sha 989ba174." The compiler area map (drawn from actual files in `compiler/self-host/`) states explicitly: "Stage-2 is blocked by scale-dependent codegen miscompiles... Stage-2 == Stage-3 identity check has NOT been achieved." This is a material contradiction. The fixed-point claim appears to be either aspirational, from a different branch, or from a version predating the current self-host source. Do not publish the "bootstrap fixed point" claim until Stage-2 completes successfully and is verified.

**Contradiction 3 - env_get capability requirement**
`model.rs` maps `env_get` and `exit` to `Capability::Process`. `CLAUDE.md` documents `env_get` as a no-capability builtin. This means code that calls `env_get` without `@capabilities(process)` may pass `kryos check` but fail `kryos manifest --caps` audits or trigger unexpected warnings. One of these must be corrected.

**Contradiction 4 - Parity gate coverage**
The compiler area map states "55 smoke tests total" in `tests/smoke/` but "latest captured result: total=34." The readme and strategic memory use "48/48" in one reference and "34/34" in the parity result file. The difference is the `modules/` subdirectory excluded from the top-level glob. The "48/48 local" claim in MEMORY.md (`project_kryos_selfhost_2026_06_09.md`) does not match the captured result file (34/34 at SHA 0977714). The true number of parity-tested files is 34; 55 .kry smoke files exist.

**Contradiction 5 - kryos-code IDE description**
`project_kryos_code.md` (April 2026) describes kryos-lang as "a Python-based interpreter (pip install, Python VM)." This is fully obsolete - kryos-lang is a 22-crate Rust compiler. The IDE memory file is from a different era and should not be treated as current state.

**Contradiction 6 - RESULTS.md vs BENCHMARKS.md**
`benchmarks/RESULTS.md` (generated by older `run.sh`) shows nbody at 4.20x and binary_trees at 5.92x using gcc -O3 as the C baseline. `BENCHMARKS.md` (generated by current `measure.py`) shows nbody at 1.34x and binary_trees at 1.45x using clang++ 21 -O2. Both files are committed and public. Any reader will see wildly different numbers depending on which file they read. The older RESULTS.md must be deleted or marked superseded.

**Contradiction 7 - stale kryos-twin-native version pin**
`project_kryos_twin_native_2026_05_15.md` pins the project to "kryos-lang v1.2.0" but the language was recalibrated to v1.0.0-beta.1 in June 2026. Compatibility of the twin-native port with the current language is unverified.