# Kryos Series A Readiness — Design Document

> **Goal:** Make Kryos investor-ready, production-ready, and credible for public launch. Nobody clones this repo and laughs.

**Date:** 2026-04-07
**Status:** Approved

---

## Current State

- 21-crate Rust compiler, 49k+ lines, dual Cranelift/LLVM backends
- 680 tests passing, 0 failures
- 8 example programs (only 2 run without crashing)
- Professional README with benchmarks, WHY_KRYOS.md positioning doc
- VS Code extension (syntax highlighting, snippets)
- CI pipeline (GitHub Actions, multi-OS)
- REPL, formatter, check mode, doc generator all functional
- Stdlib: 28 .kry modules (never tested through compiler, imports broken)
- License contradiction: Cargo.toml says MIT, README says Proprietary

## Problem

5 of 8 examples crash with segfaults, heap corruption, or illegal instructions. The stdlib import system doesn't work. There's no install path besides "build from source with Rust." The benchmark claims aren't verifiable. An investor or Hacker News reader would clone, run an example, see a segfault, and close the tab.

---

## Phase 1: Stop the Bleeding — Fix Every Crash

**Priority:** CRITICAL — blocks all other phases

### Scope
- Debug and fix 5 crashing examples: `fibonacci_showcase` (segfault in comptime section), `http_server` (illegal instruction in routing), `pipeline` (heap corruption after spawn/channel), `mini_grep` (segfault in string search), `kryos_bootstrap` (parse error on reassignment)
- Fix `all_features.kry` (stdlib import errors + parse errors)
- Each fix gets a regression test

### Root Cause Clusters
- **Comptime evaluation:** fibonacci_showcase section 4 hits null pointer during compile-time eval
- **String operations:** mini_grep and http_server crash during string method dispatch
- **Heap after concurrency:** pipeline corrupts heap after spawn + channel + filtering
- **Reassignment syntax:** `done = true` not parsing — parser may require `let` for all assignments
- **Stdlib imports:** all_features.kry fails on `use std::math` (Phase 2 dependency)

### Exit Criteria
All 8 examples run to completion with correct output. Zero segfaults, zero crashes. Regression tests for each fix.

---

## Phase 2: The Foundation Works — Stdlib & Module System

### Scope
- Fix module resolution: `use std::X` finds `compiler/stdlib/X.kry`
- Audit all 28 stdlib .kry files through `check_file()` — fix every error
- Fix license: Proprietary everywhere (Cargo.toml, README, LICENSE file)

### Strategy
- `find_stdlib_dir()` helper: checks `KRYOS_STDLIB_DIR` env var, `<exe>/../stdlib/`, `CARGO_MANIFEST_DIR` ancestor
- Compile-check test that runs every stdlib module through type checker
- Fix common issues: `ptr` -> `i64`, unrecognized `__builtin_*`, missing cross-module imports
- `license = "LicenseRef-Proprietary"` in all Cargo.toml files, add LICENSE file

### Exit Criteria
`use std::math`, `use std::string`, `use std::collections` etc. all resolve and type-check. CI test enforces this. License consistent everywhere.

---

## Phase 3: A Real Language — Ergonomic Builtins & Runtime Safety

### Scope
- Ergonomic builtins: `file_read()`, `file_write()`, `env_get()`, `time_now()`, `assert()`, `args()`, `parse_int()`, `parse_float()`, `type_of()` work without extern blocks
- Panic handler: div-by-zero and out-of-bounds produce `kryos panic: division by zero at main.kry:14:5`
- Stack traces: panics print full call chain

### Strategy
- Each builtin: `kryos_builtin_*` in kryos-rt, registered in MIR lowerer + Cranelift codegen + JIT builder
- `kryos_trace_enter`/`kryos_trace_exit` emitted at function entry/return
- `kryos_check_div_zero`/`kryos_check_bounds` before every div and array index
- Always-on tracing for now (correctness > speed at launch, ~5ns overhead per call)

### Exit Criteria
- `println(file_read("test.txt"))` compiles without extern blocks
- `let x = 1 / 0` prints panic with file/line, not segfault
- Nested function panic shows full call stack

---

## Phase 4: Proof It Works — Real Programs & Benchmarks

### Scope
- 5 real programs:
  1. CLI Calculator (~100 lines) — args, parsing, recursion, errors
  2. File Line Counter / wc clone (~150 lines) — file I/O, string ops, formatted output
  3. JSON Key Counter (~200 lines) — stdlib imports, maps, sorting
  4. TCP Echo Server (~150 lines) — spawn, channels, networking
  5. CSV Analyzer (~250 lines) — structs, methods, arrays, float math, tables
- Each gets a run-expect test
- Benchmark suite: criterion harness for the 5 existing benchmarks, `cargo bench` reproduces README table

### Dependencies
- JSON Key Counter depends on Phase 2 (stdlib)
- All programs depend on Phase 3 (builtins like file_read, args)

### Exit Criteria
All 5 compile, run, produce correct output. `cargo bench` reproduces README performance claims.

---

## Phase 5: Developer Experience — Packages, Scaffolding & Tutorial

### Scope
- `kryos pkg init` generates working project that compiles immediately
- Package registry: `kryos pkg publish` (tarball), `kryos pkg update` (resolve + download)
- 3 starter packages: std-test, std-cli, std-csv
- Tutorial: "Build a CSV Analyzer in Kryos" — 5 chapters, verified snippets

### Strategy
- Git-based index registry (no server at launch)
- Tutorial written last (depends on everything else working)
- CI checks tutorial snippets compile

### Exit Criteria
`kryos pkg init myproject && cd myproject && kryos run src/main.kry` works. Tutorial followable by a newcomer.

---

## Phase 6: Ship It — Releases, Install Script & Polish

### Scope
- GitHub Actions release workflow: tag push -> binaries for Linux x86_64, macOS arm64/x86_64, Windows x86_64
- Install script: `curl -fsSL <url>/install.sh | sh`
- CHANGELOG.md (v0.1.0 baseline)
- Final integration sweep: full test suite, all examples, all programs, tutorial, pkg init flow
- README polish: install instructions first, CI/license badges

### Strategy
- `cross` for static Linux builds, native cargo for macOS/Windows
- Install script ~50 lines shell, downloads right binary, adds to PATH
- Integration sweep is final gate

### Exit Criteria
Developer on any major OS installs in one command, runs `kryos run hello.kry`, it works. README leads with that experience.

---

## Risk Notes

- Debug builds consume ~48GB RAM. All CI and local builds use `--release -j 4`.
- Phase 1 crash fixes may reveal deeper codegen issues that expand scope.
- Package registry is MVP (git-based) — a real registry server is post-launch.
- LLVM backend test coverage is weaker than Cranelift — some fixes may be Cranelift-only initially.
