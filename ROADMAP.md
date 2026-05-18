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

### v2.9 — LLVM backend parity

The Cranelift backend (used by `kryos run` and `kryos build` without
`--release`) is the path most programs take. LLVM is the release path
and currently lags it on a handful of features. v2.9 closes the gap:

- Audit every MIR opcode that has a Cranelift implementation. For each
  one missing in the LLVM backend, either implement it or document why
  the LLVM path can't lower it.
- Add a CI matrix that runs every smoke test under both backends
  (`kryos run` for Cranelift, `kryos build --release && ./out` for LLVM).
- Performance pass on the self-hosted bootstrap lexer + Stage 1 parser.
  Stage 0 closure shipped in v2.7; Stage 1 mini-parser started; v2.9
  aims for measurable speed-up on the LLVM path so the self-host story
  has a credible performance story.

### v3.0 — Foreign function interface

`kryos bindgen` already turns a C header into `extern "C" { ... }`
declarations. v3.0 turns this from "tech demo" into "ship to clients":

- End-to-end test: link a real C library (zlib, sqlite, libcurl), call
  it from a `.kry` program, ship a binary. Document every step.
- Audit `extern "C"` parsing in the language frontend. Confirm calling
  conventions, struct layouts, and pointer ownership rules match the
  C ABI on x86_64-linux, aarch64-darwin, and x86_64-windows-msvc.
- Expand `bindgen` coverage to `typedef`s, anonymous structs / unions,
  variadic functions, and `static inline` (when the user opts in to
  reproducing the inline body in Kryos).
- Add a manual chapter on FFI with worked examples: opening a SQLite
  database, parsing JSON via a C library, calling into a CUDA stub.

### v3.1 — Language server depth

`kryos lsp` exists and speaks the protocol. v3.1 makes it useful enough
to be the default editor experience:

- Audit completion, hover, go-to-definition, find-references, rename,
  document-symbols, workspace-symbols, signature-help, code actions,
  inlay hints.
- For each capability that is missing or shallow, either implement it
  to mainstream-language quality or document the limitation in the
  language server doc.
- Ship VS Code / Zed / Neovim / Helix configs that demonstrate working
  setup, alongside CI tests that exercise the LSP via the protocol.

### v3.2 — Package manager and registry

`kryos pkg` has every subcommand (`init`, `add`, `remove`, `update`,
`install`, `lock`, `publish`, `search`, `info`, `sync`, `outdated`)
implemented as a CLI shape, but the registry is empty. v3.2 changes
that:

- Stand up a real registry endpoint (Postgres-backed, behind a thin
  authenticator). The current `tools/registry/` crate is the starting
  point.
- Publish 5-10 first-party packages: `kryos-net-extras`, `kryos-cli`
  helpers, `kryos-time`, `kryos-yaml`, `kryos-toml`. These cover the
  gaps users hit immediately when they leave the stdlib.
- Document publishing: keys, signing, version yanks, namespacing.
  Establish a yanking policy so security issues can be handled.
- Add `kryos pkg verify` to re-resolve a lock file against the registry
  and confirm checksums match what the lock file claims.

### v3.3 — Concurrency

Today Kryos has channels (`chan`, `send`, `recv`, `close_chan`) and
mutexes. The actor model in `std::agent` is the highest-level primitive.
v3.3 expands the concurrency story:

- OS-thread spawn (`thread_spawn`) with explicit `join` / `detach`. Run
  the existing channel and mutex builtins across threads with a real
  multi-producer / multi-consumer story.
- A work-stealing scheduler primitive for CPU-bound work that doesn't
  want to think about threads directly.
- Async / await (already parses; the codegen path for state-machine
  splitting exists in `kryos-mir`'s async lowering pass). Promote from
  "experimental" to "supported": tutorial chapter, regression tests,
  documented runtime model.

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
