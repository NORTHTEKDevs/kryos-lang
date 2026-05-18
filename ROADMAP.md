# Kryos Roadmap

This file is the public commitment for what ships next. It is updated each
release. Items are grouped by milestone; ordering inside a milestone is
the rough sequence of work, not a strict dependency chain.

The goal driving this roadmap is to make Kryos professional enough that
people outside the original author can ship code in it — first as a
single-file scripting tool, then for embedded / FFI integrations, then
for full applications with package distribution and editor tooling on
par with mainstream languages.

## Released

### v2.8.0 — Language polish

Bug fixes and ergonomic improvements that came out of a self-audit
against "what would block me writing a real program in Kryos today".

- **String-local clobber fix.** Use-after-free when passing a named
  string local to a function that stored it in a heap struct. Fixed at
  the MIR-lowering level by extending `consume_call_args` to the
  `Stmt::Assign -> identifier` path. Permanent regression test:
  [`tests/smoke/test_string_clobber.kry`](tests/smoke/test_string_clobber.kry).
- **Tuple-mut destructuring.** `let (mut a, mut b) = expr` and
  `let mut (a, b) = expr` both now produce mutable bindings. The
  per-element `mut` was parsed into the AST but silently dropped by the
  type checker's `bind_pattern`. Permanent regression test:
  [`tests/smoke/test_tuple_mut.kry`](tests/smoke/test_tuple_mut.kry).
- **String interpolation escape.** `{{` and `}}` produce literal braces,
  matching Rust / Python f-strings. The older `\{` / `\}` escapes still
  work. Permanent regression test:
  [`tests/smoke/test_string_brace_escape.kry`](tests/smoke/test_string_brace_escape.kry).
- **Stdlib reference doc.** [`docs/STDLIB.md`](docs/STDLIB.md): every
  always-available builtin, every `use std::*` module, naming gotchas
  table, `@test` annotation reference, and full `kryos` CLI surface.

## Planned

### v3.0 — 1.0-grade release

v3.0 is the consolidation cut: every "experimental" sticker comes off,
every prod-readiness gap from the v2.8.0 audit closes, and the v2.x
items previously scattered across v2.9 (LLVM parity), v3.0 (FFI),
v3.1 (LSP), v3.2 (registry), v3.3 (concurrency) land together.

Scope tracked in [AUDIT-v2.8.0.md](AUDIT-v2.8.0.md) and
[AUDIT-llvm-parity.md](AUDIT-llvm-parity.md). Stability promises in
[STABILITY-v3.0.md](STABILITY-v3.0.md).

What's in v3.0:

- **LLVM backend parity** (was v2.9). **34/34 smoke tests pass on
  the release AOT path — 100% Cranelift↔LLVM parity.** Per-class
  fixes for missing-builtin declarations, SSA-name collisions,
  aggregate-as-payload codegen, struct-field pointer coercion,
  user-shadow detection, interpolation stringification, push()
  result aliasing, and the KryosArray drop-loop offset bug.
  Linkage is now `internal` so libc names don't clash.
- **Foreign function interface** (was v3.0). `kryos bindgen`
  hardening, `extern "C"` ABI verification across linux-x86_64,
  windows-msvc, macos-aarch64. FFI chapter in the manual with zlib /
  sqlite / libcurl worked examples.
- **Language-server depth** (was v3.1). Completion, hover,
  go-to-definition, find-references, rename, document-symbols,
  workspace-symbols, signature-help, code actions, inlay hints —
  audited and brought to mainstream-language quality. VS Code +
  Zed + Neovim configs verified.
- **Package manager + registry** (was v3.2). Real HTTP roundtrip
  in CI against an ephemeral `kryos-registry-server`. 5-10 first-
  party packages published. `kryos pkg verify` checksum check.
  Public hosting of `packages.kryos.dev` deferred to a separate
  decision (per the prod-hardening shift); self-hosting documented
  via the `NORTHTEKDevs/kryos-registry` index repo.
- **Concurrency promotion** (was v3.3). Real HTTP integration test
  spins up a TCP server and runs a client-server roundtrip in a
  single Kryos program (`tests/smoke/test_async_http_roundtrip.kry`).
  `async fn` / `await` / `spawn { ... }` / `chan / send / recv` all
  documented and supported. Type-checker `Future<T>` unwrap
  remaining as a known caveat (STABILITY-v3.0.md §6) until the
  promoting MIR work lands.
- **CI matrix** Linux + macOS-14 + Windows tier-1. Fuzz job
  (lexer + parser + typechecker, 60s budget each). Backend parity
  matrix gated on PR. WASM via wasmtime. Editor packaging (.vsix
  + Zed .wasm) automated.
- **Release artifacts.** Signed tarballs via GitHub OIDC build-
  provenance attestations + SHA-256 checksums. No Apple Dev ID /
  Windows EV cert procured; Gatekeeper / SmartScreen first-run
  caveats documented honestly.

### v3.0.x patch line

Smaller items that land after the v3.0 cut:

- test_generics: `to_string<T=str>` cleanup-time double-free.
- test_process: MIR-elision undef-SSA in Command__arg.
- Cranelift `array_new` arity bug on `std::wasm::pack` helpers.
- Documentation pass over README.md / compiler/README.md
  (audit §7 remaining items).

### v3.4+ — Stretch

Items that are useful but not on the critical path to "people outside
me can ship Kryos code":

- WASM browser story: DOM bindings beyond the current `dom_set_text` /
  `canvas_fill_rect` minimum; `wasm-bindgen`-style codegen for richer
  browser interfaces.
- Cross-compile UX: `kryos build --target` exists; an opinionated
  `kryos cross` wrapper around it would help.
- GPU / tensor backend: `std::tensor` exists with FFI; first-class
  CUDA / Metal kernels would unlock more of the AI-runtime narrative
  the language was designed for.
- Macro / proc-macro story (currently `comptime` blocks; a Rust-style
  macro layer would let library authors hide common boilerplate).

## How this list is maintained

Each release of Kryos bumps the workspace version in `Cargo.toml`,
tags the commit, and updates `CHANGELOG.md` with one-line summaries of
every shipped item. This roadmap is updated at the same time to:

1. Move the just-released milestone into the **Released** section.
2. Renumber and re-prioritise upcoming items based on what shipped.
3. Add any new items that came out of the release cycle.

There is no marketing voice in here on purpose. If something is on the
list, it is the next thing the author actually plans to ship. If a goal
slips or gets cut, this file is updated to say so honestly rather than
quietly removed.
