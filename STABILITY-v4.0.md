# Kryos v4.0 stability statement

This document specifies what is stable across the v4.x line. **Within v4.x,
none of the items in §1–§6 will break in a backwards-incompatible way.**
Forward-compatible additions are permitted at any minor version (`4.x.0`).

Last updated: 2026-05-18 (v4.0.0-rc.1 cut).

---

## 1. Source compatibility

Every program that compiled cleanly against `kryos 4.0.0` will continue to
compile cleanly against every `4.x.y` release. The frozen surface is:

- Lexer: the keyword set, operator set, literal forms, and comment syntax.
  No new reserved words will be added at minor versions; only at the next
  major bump.
- Parser: the syntactic forms accepted by `kryos check` at v4.0.0. New
  syntactic *additions* are allowed in minor releases (e.g. a future
  `kryos 4.5.0` may add `?Sized` bounds), but no deletions or reinterpretations.
- Standard library: the exported function signatures of every `std::*`
  module documented in `docs/STDLIB.md` at v4.0.0. New stdlib symbols may
  be added; existing ones are not renamed, retyped, or removed.
- Annotation set: `@pure`, `@capabilities(...)`, `@test`, `@bench`,
  `@inline`, `@deprecated`. Behavior is frozen; new annotations may be
  added.

If a program compiles on `4.0.0` but breaks on a later `4.x.y`, that's a
P0 bug — file at https://github.com/NORTHTEKDevs/kryos-lang/issues.

## 2. ABI compatibility

The v4.x line freezes:

- `kryos-rt` exported C-ABI symbol names (`kryos_*`) and their signatures
  for every function documented in `docs/STDLIB.md`.
- `kryos-stdlib-native` exported symbols listed in the same.
- `kryos.toml` and `kryos.lock` file formats. New optional fields may be
  added; existing fields are not removed or renamed.
- The package-registry HTTP API: `/v1/health`, `/v1/packages/<name>`,
  `/v1/packages/<name>/<version>`, `/v1/search?q=...`.

Not frozen:

- The MIR layout (internal to the compiler crates).
- The compiler's internal IR symbol naming for synthesised closures and
  dyn-thunks (`<name>_env`, `<name>_dyn`).
- Optimisation-pipeline ordering and inlining heuristics.

## 3. CLI surface

The following `kryos` subcommands are stable across v4.x — present, with
the same positional + flag shape:

| Subcommand | Stability |
| --- | --- |
| `kryos run [file] [--time]` | Stable |
| `kryos build [file] [--release] [--backend wasm|llvm|cranelift] [-g] [-o PATH]` | Stable |
| `kryos check [path] [--watch] [--skip-ownership]` | Stable |
| `kryos eval "<expr>" [-v]` | Stable |
| `kryos test [filter] [--exact] [--nocapture] [--format=pretty|json] [--list] [--path PATH]` | Stable |
| `kryos bench [filter] [--exact] [--warmup N] [--measure N] [--path PATH]` | Stable |
| `kryos coverage [path] [--format=pretty|json]` | Stable |
| `kryos profile <file>` | Stable |
| `kryos trace <file>` | Stable |
| `kryos lint [path] [--format=pretty|json] [--enable] [--disable] [--strict]` | Stable |
| `kryos audit [path] [--format=pretty|json]` | Stable |
| `kryos fmt [files] [--check]` | Stable |
| `kryos doc [files] [-o DIR] [--html]` | Stable |
| `kryos doc serve [files] [--address ADDR]` | Stable |
| `kryos new <name> [--template cli|http|lib|agent] [--path PATH]` | Stable |
| `kryos doctor [--verbose]` | Stable |
| `kryos tree [path] [--transitive]` | Stable |
| `kryos watch <file> [--interval MS] [--run run|check]` | Stable |
| `kryos clean [path] [--dry-run]` | Stable |
| `kryos explain <Exxx>` | Stable |
| `kryos repl` | Stable |
| `kryos lsp` | Stable |
| `kryos bindgen <header>` | Stable |
| `kryos pkg {init,add,remove,update,install,lock,publish,search,info,sync,outdated,list-local}` | Stable |
| `kryos version` | Stable |

Forward-compatible: new subcommands may be added; existing subcommands may
gain new optional flags. Positional argument shape is frozen.

## 4. LSP surface

The LSP server (`kryos lsp`) implements the following methods at v4.0,
and they will continue to be implemented (with at least the same returned
shape) through the v4.x line:

`initialize`, `initialized`, `shutdown`, `exit`,
`textDocument/didOpen`, `textDocument/didChange`, `textDocument/didClose`,
`textDocument/publishDiagnostics`,
`textDocument/completion`, `textDocument/hover`, `textDocument/definition`,
`textDocument/references`, `textDocument/rename`,
`textDocument/documentSymbol`, `workspace/symbol`,
`textDocument/documentHighlight`, `textDocument/foldingRange`,
`textDocument/formatting`, `textDocument/signatureHelp`,
`textDocument/inlayHint`, `textDocument/codeAction`,
`textDocument/semanticTokens/full`.

## 5. Platform support

| Platform | Tier | Cranelift JIT | LLVM AOT | CI |
| --- | --- | --- | --- | --- |
| Linux x86_64 (glibc) | 1 | Supported | Supported | Yes |
| macOS aarch64 (Apple Si) | 1 | Supported | Supported | Yes (macos-14) |
| Windows x86_64 (MSVC) | 1 | Supported | Supported | Yes |
| macOS x86_64 (Intel) | 2 | Best-effort | Best-effort | Tag matrix only |
| Linux aarch64 (glibc) | 2 | Best-effort | Best-effort | cross.yml |
| Linux x86_64 (musl) | 2 | Best-effort | Best-effort | cross.yml |
| WebAssembly (wasm32-wasip1) | 2 | n/a | Supported | wasm-smoke job |

Tier 1: every v4.x.y release verified end-to-end (build, smoke, parity
matrix) on this platform. Regressions block the release.

Tier 2: builds + basic smoke; full parity isn't gated.

## 6. Release process

Every v4.x.y release produces:

1. Source tarball + SHA-256 checksum
2. Per-tier-1-platform binary tarball + SHA-256
3. GPG-attested release artifacts via OIDC (Sigstore-style)
4. VS Code extension `.vsix`
5. Zed wasm extension
6. CHANGELOG.md entry covering every commit since the previous tag
7. Updated parity matrix snapshot in `tests/parity/results/`

Pre-1.0 caveats from the v3.0 stability statement no longer apply at v4.0.

## 7. Migration from v3.x to v4.0

v4.0 is fully source-compatible with v3.17. Every program that compiled
under `kryos 3.17` compiles unchanged under `kryos 4.0.0`.

Tooling changes you may notice:

- `kryos --version` reports `4.0.0` (was `3.17.0-rc.1`).
- The release-candidate suffix has been dropped — v4.0.0 is the first
  stable cut of the v4.x line.
- Behavior of `kryos run`, `kryos build`, `kryos test`, `kryos check`,
  `kryos fmt`, `kryos lsp` is byte-identical to the v3.17 build for all
  programs that don't depend on previously-undefined behavior.

## 8. Reporting incompatibilities

Open an issue at https://github.com/NORTHTEKDevs/kryos-lang/issues with:

1. `kryos --version`
2. The minimal `.kry` source that worked under v4.x and breaks under
   another v4.y
3. The exact command line
4. Expected vs actual output

Source incompatibility within the v4.x line is treated as a P0 bug.
