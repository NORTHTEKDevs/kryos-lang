# Kryos Stability Policy

Kryos follows [Semantic Versioning 2.0.0](https://semver.org/) for compiler and
standard library releases. This document is the authoritative reference for
what changes are considered breaking, what is stable today, and how
deprecations are handled.

## Versioning

Versions take the form `MAJOR.MINOR.PATCH`:

* **PATCH** (`x.y.Z`): bug fixes and performance improvements that do not change
  observable behaviour. Always safe to upgrade within a minor line.
* **MINOR** (`x.Y.0`): new language features, new stdlib APIs, new compiler
  flags, and new diagnostics. Existing code that compiled before continues to
  compile and run with the same behaviour.
* **MAJOR** (`X.0.0`): breaking changes. May remove deprecated APIs, change
  syntax, or alter runtime behaviour. Always paired with a migration guide.

Pre-1.0 (`0.y.z`) releases use the same scheme but minor bumps may include
breaking changes. After 1.0, the guarantees above are firm.

## What is stable

Everything in `compiler/stdlib/` reachable via `use std::*` is considered part
of the public API as of the version it was added in. Stable surfaces include:

* The lexical grammar described in `docs/19-language-reference.md`.
* All declarations marked `pub` in the standard library tree.
* The set of builtins documented in `docs/02-variables-and-types.md` through
  `docs/14-ai-runtime.md`.
* The `kryos.toml` schema documented in `docs/12-modules-and-packages.md`.
* CLI subcommands documented in `kryos --help` (`run`, `build`, `check`, `fmt`,
  `pkg`, `lsp`, `bindgen`).
* Exit codes: `0` on success, `1` for diagnostics emitted, `2` for usage
  errors, `101` for an internal compiler error.

## What is not stable

* The internal IR (MIR) and codegen surfaces (`kryos-codegen-*`) are
  implementation details.
* Symbols in `kryos-rt` and `kryos-stdlib-native` that begin with `kryos_*` are
  ABI for the compiler-emitted code; user code MUST go through the documented
  builtins.
* Experimental compiler flags prefixed with `-Z` may change at any time.
* The on-disk format of `.kryos/deps/*.redirect` is an implementation detail.

## Deprecation policy

Public APIs and language constructs may be deprecated but never silently
removed during a minor cycle.

1. **Announce.** A deprecated item must compile with a diagnostic of kind
   `warning` (not error) for at least one full minor release.
2. **Document.** Mark the item with `#[deprecated("...")]` (or the documented
   equivalent for syntactic features) and add a CHANGELOG entry under
   "Deprecations" with the replacement.
3. **Remove.** Deprecated items may be removed in the next MAJOR bump.

## Language editions

Kryos uses a Rust-style **edition** mechanism. Each project declares its
edition in `kryos.toml`:

```toml
[package]
edition = "2026"
```

Editions are language profiles that may differ in syntax or surface-level
semantics. The compiler always supports every edition simultaneously so that
crates of different editions can interoperate. New editions are introduced
roughly every 3 years and never silently change the behaviour of existing
code.

### Current editions

| Edition | Status      | Notes                                                        |
|---------|-------------|--------------------------------------------------------------|
| `2026`  | stable      | Current default. Real module-level globals, stable stdlib.   |
| future  | placeholder | The next edition (post-1.0) will be announced with a guide.  |

If `edition` is omitted from `kryos.toml`, the compiler treats the project as
`2026`. Edition migration tools are part of `kryos pkg` and will be invoked as
`kryos pkg migrate <new-edition>` once a second edition ships.

## Platform support

### Tier 1 (build + full test suite gate every release)

* `x86_64-unknown-linux-gnu`
* `x86_64-apple-darwin`
* `aarch64-apple-darwin`

### Tier 1.5 (build + targeted tests gate every release)

* `x86_64-pc-windows-msvc` — full release build, Cranelift `kryos run`
  path, and `kryos build --release` AOT path (LLVM -> clang -> link.exe)
  are gated by CI on `windows-latest`. The targeted Cargo test set
  covers `kryos-codegen-llvm`, `kryos-driver`, and `kryos-mir`. The full
  example/showcase smoke battery from the Linux job is not yet mirrored
  on Windows; once it is, this target moves to tier 1.

  Known limitations on Windows MSVC:

  * `kryos build --release` does not currently emit source-line debug
    info. DWARF metadata is gated off for COFF targets because emitting
    it produces a malformed object that `link.exe` rejects with LNK1107.
    A CodeView (`.pdb`) emitter is planned; until then, debugger
    backtraces resolve to LLVM IR names rather than `.kry` source lines.
  * `kryos run` (Cranelift JIT) is unaffected.

### Tier 2 (build but not exercised on every CI run)

See `docs/18-cross-compilation.md` for the current list and the
`--target` flag.

### Tier 3 (experimental — not production-ready)

- **`wasm32-unknown-unknown`** — the WASM backend is **experimental (v0.1)**.
  It is exercised by a `wasm-smoke` CI job over a deliberately limited set of
  example programs. It supports integer/float arithmetic, booleans, basic
  loops, recursion, string literals, and arrays, but **not** structs, enums,
  tuples, maps, closures, `match`, `to_string()`, string interpolation, or
  any `std` module. WASI is not supported. Do not treat it as a stable
  deployment target; see `docs/18-cross-compilation.md` for the full feature
  matrix.

## Reporting breakage

If a non-major release breaks something that compiled before, that is a bug.
Please file an issue at the project repository with the smallest reproducer
you can produce. We treat such regressions as release-blockers.
