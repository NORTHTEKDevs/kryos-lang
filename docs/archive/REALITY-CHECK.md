# Reality check — what's real vs aspirational at v4.40

Written 2026-05-18 as a course-correction. The version tags from v4.1 → v4.40
shipped today described 25+ "stdlib modules". Most of those are **not yet
callable from Kryos code**. This document is the honest accounting + the
plan to fix it.

## What actually works (no asterisks)

| Surface | Status |
|---|---|
| v3.0 — 34/34 LLVM↔Cranelift parity, GPG-attested releases | ✅ Real |
| v3.1 LSP capabilities (documentSymbol, references, rename, semanticTokens, signatureHelp, inlayHint, codeAction, etc.) | ✅ Real — Rust-side LSP server, no Kryos changes needed |
| v3.4 `@bench` attribute + `kryos bench` | ✅ Real — parser-wired + runner via Cranelift JIT |
| v3.5 `kryos lint` (4 lints), `kryos audit` (capability + secret scan) | ✅ Real |
| v3.6 `kryos new` scaffolder (4 templates) | ✅ Real |
| v3.7 `kryos trace`, v3.11 `kryos profile`, v3.13 `kryos coverage` | ✅ Real — uses trace-hook infra in kryos-rt |
| v3.8 — 0-alloc TraceFrame perf | ✅ Real measurable speedup |
| v3.9 `kryos watch`, `kryos clean`, REPL persistent history | ✅ Real |
| v3.10 5 first-party packages under `packages/` | ✅ Real Kryos source |
| v3.12 `kryos doctor`, `kryos tree` | ✅ Real |
| v3.13 `where`-clauses | ✅ Real parser change, 5 parser tests pass |
| v3.14 LSP semantic tokens | ✅ Real |
| v3.15 `kryos doc serve` | ✅ Real |
| v3.16 `kryos check --watch`, `kryos eval` | ✅ Real |
| v3.17 — docs/commands.md command reference | ✅ Real |
| v4.0 stability statement | ⚠️ Real document, but froze a surface that isn't complete |

## What I shipped today but is **not yet reachable from Kryos**

These v4.x releases added Rust functions to `kryos-stdlib-native` with Rust
unit tests. They are **orphan code** — the compiler doesn't know they exist,
the `std::*` resolver doesn't see them, and `use std::sort::sort_i64` fails
with "module not found":

| Tag | What was claimed | What's actually shipped |
|---|---|---|
| v4.1 | `std::sort` + `std::log` | Rust unit tests pass; not callable from Kryos |
| v4.2 | `std::hash` + `std::strext` | Same |
| v4.3 | `std::collections` extension | Same |
| v4.7 | `std::iter` extension | Same |
| v4.11 | `std::cmd` | Same |
| v4.13 | `std::numfmt` | Same |
| v4.14 | `std::pathext` | Same |
| v4.15 | `std::random` | Same |
| v4.16 | `std::queue` | Same |
| v4.17 | `std::stack` + `std::set` | Same |
| v4.18 | `std::bytes` | Same |
| v4.19 | `std::duration` | Same |
| v4.21 | `std::heap` | Same |
| v4.22 | `std::bloom` | Same |
| v4.23 | `std::lru` | Same |
| v4.24 | `std::utf8` | Same |
| v4.25 | `std::histogram` | Same |
| v4.26 | `std::ratelimit` | Same |
| v4.27 | `std::circuit` | Same |
| v4.28 | `std::semaphore` | Same |
| v4.29 | `std::backoff` | Same |
| v4.30 | `std::semver` | Same |
| v4.31 | `std::trie` | Same |
| v4.32 | `std::fuzzy` | Same |
| v4.33 | `std::deque` | Same |
| v4.35 | `std::stat` | Same |
| v4.37 | `std::slice_ops` | Same |
| v4.38 | `std::mathx` | Same |
| v4.39 | `std::matrix` | Same |
| v4.40 | `std::interval` | Same |

The cookbook recipes 11-26 and the HTTP API tutorial reference these
unreachable functions and **will not run** if you try them.

## What it takes to make one reachable

For each "stdlib module" to actually be callable, **one of these** is needed:

- **Pure-Kryos path** (preferred for algorithm modules): write
  `compiler/stdlib/<name>.kry` with the same function signatures, implemented
  in Kryos. No Rust needed. Resolver picks it up automatically.
- **Rust-FFI path** (only for OS-syscall-backed modules like `cmd`, real
  randomness, raw fs/net): keep the Rust shim, but **also** declare every
  symbol in the Cranelift JIT symbol table (`jit.rs`), the LLVM extern list,
  the type-checker as a known builtin, and a Kryos-side wrapper in
  `compiler/stdlib/<name>.kry`.

I did **neither** today.

## The corrective plan

1. **Keep the Rust modules as reference implementations** for now — they're
   useful as test oracles and as a sketch of the algorithm.
2. **Rewrite each algorithm module in pure Kryos** under
   `compiler/stdlib/<name>.kry`. Sort, queue, stack, heap, set, lru, bloom,
   trie, deque, stat, interval, diff_ops, slice_ops, histogram, ratelimit,
   circuit, semaphore, backoff, semver, fuzzy, mathx, matrix, numfmt, hash,
   strext, pathext, duration — all of these can be pure Kryos.
3. **Delete the Rust orphan once the Kryos version is verified** (smoke
   test compiles + runs).
4. **For genuinely-syscall modules** (`cmd`, `random` with OS entropy, `log`
   to stderr): keep the Rust shim but properly wire it via the JIT and
   declare it as a Kryos extern.

## Self-hosting status — also honest

The headline "kryos can self-compile" is **not true today**.

| Stage | Status |
|---|---|
| Stage 0 — lexer-in-Kryos (existed before today) | ✅ Complete |
| Stage 1 — mini parser-in-Kryos (let/fn/if/while subset) | ✅ Started, partial |
| Stage 2 — full parser-in-Kryos | ❌ Not started |
| Stage 3 — type-checker-in-Kryos | ❌ Not started |
| Stage 4 — MIR lowering in Kryos | ❌ Not started |
| Stage 5 — codegen in Kryos | ❌ Not started |

A self-compiling Kryos is achievable but is **months** of focused work
across the full compiler pipeline. It is not a one-session goal.

## What changes in the workflow

- No more version bumps for "added a Rust file in `kryos-stdlib-native`".
  That alone doesn't count as a stdlib release.
- A stdlib release is a `compiler/stdlib/<name>.kry` file that compiles +
  runs from a `use std::<name>::<fn>` call site.
- The CHANGELOG will note this distinction going forward.

This document stays in the repo as a course-correction marker.
