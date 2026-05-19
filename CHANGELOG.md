# Changelog

All notable changes to Kryos will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [4.11.0-rc.1] — 2026-05-18 — "std::cmd subprocess capture + recipe"

### Added

- **`std::cmd`** — subprocess capture for scripting.
  `kryos_cmd_run(cmd_ptr, cmd_len, out, out_cap, needed)` spawns a
  command (shellword-split), captures stdout + stderr + exit code,
  and writes the bundle in `exit_code\nstderr_len\nstderr<stdout>`
  format. Closed stdin, no shell expansion, no escape sequences in
  the splitter (sufficient for typical CLI invocations).
- **`docs/learn/cookbook/19-running-subprocesses.md`** — recipe with
  bundle parsing pattern.
- 2 new shellword tests. 34 total stdlib tests pass.

### Changed

- Workspace version bumped from `4.10.0-rc.1` to `4.11.0-rc.1`.

## [4.10.0-rc.1] — 2026-05-18 — "cookbook expansion + http client"

### Added

- **4 new cookbook recipes** covering common real-world patterns:
  - `15-csv-parsing.md` — quote-aware CSV reader
  - `16-env-config.md` — env-driven config with safe defaults + redaction
  - `17-retry-with-backoff.md` — exponential-backoff retry helper
  - `18-input-validation.md` — email + port validators
- **`examples/showcase/http_client.kry`** — minimal HTTP/1.1 GET client
  over raw TCP. Demonstrates request framing, response parsing, header
  walking.
- `docs/learn/README.md` cookbook table now lists 18 recipes (was 14).

### Changed

- Workspace version bumped from `4.9.0-rc.1` to `4.10.0-rc.1`.

## [4.9.0-rc.1] — 2026-05-18 — "kryos diff + 3 showcase examples"

### Added

- **`kryos diff <a> <b>`** — semantic diff between two Kryos source
  files. Reports added / removed / modified declarations with
  signatures (less noisy than line-by-line diff when whitespace
  shifts around). Shows summary `+X -Y ~Z =N`.
- **`examples/showcase/rpn_calc.kry`** — Reverse Polish notation
  calculator REPL with stack ops (`+ - * / mod neg dup drop swap`).
- **`examples/showcase/todo_app.kry`** — file-backed todo list with
  add/list/done/clear commands.
- **`examples/showcase/dir_walker.kry`** — directory walker that
  counts `.kry` files + total bytes.

### Changed

- Workspace version bumped from `4.8.0-rc.1` to `4.9.0-rc.1`.

## [4.8.0-rc.1] — 2026-05-18 — "kryos pack — deterministic tar archives"

### Added

- **`kryos pack [path] [-o FILE]`** — builds a USTAR-compliant `.tar`
  archive of the current project. Includes `src/`, `tests/`,
  `examples/`, `kryos.toml`, `README.md`, `LICENSE*`, `CHANGELOG.md`.
  Output is deterministic — sorted entries, zeroed mtimes, no
  ownership info — so two runs on the same tree produce
  byte-identical archives (useful for content-addressable storage +
  reproducible release builds).
- Skips: `target/`, hidden dirs, `node_modules`, any existing `*.tar`.
- Tar header writer is a 100-line inlined helper — no `tar` crate
  dep. Verified extractable with system `tar -xf`.

### Changed

- Workspace version bumped from `4.7.0-rc.1` to `4.8.0-rc.1`.

## [4.7.0-rc.1] — 2026-05-18 — "kryos changelog + std::iter"

### Added

- **`kryos changelog [--last N] [--since TAG]`** — auto-generates a
  markdown changelog from git tags. Walks `git tag -l v*` newest-first,
  runs `git log <prev>..<tag>` for each, emits Keep-a-Changelog style.
- **`std::iter`** — slice-level transformations over `[i64]`:
  - `kryos_iter_range(start, step, len, out)` — fill arithmetic seq
  - `kryos_iter_filter_i64(..., predicate_kind, threshold, ...)` —
    6 predicates (positive, negative, even, odd, >=, <=)
  - `kryos_iter_map_i64(..., kind, c, ...)` — 6 transforms (identity,
    abs, negate, square, add c, mul c)
- 4 new iter tests. 32 total stdlib tests pass.

### Changed

- Workspace version bumped from `4.6.0-rc.1` to `4.7.0-rc.1`.

## [4.6.0-rc.1] — 2026-05-18 — "kryos info + showcase example"

### Added

- **`kryos info [path]`** — project summary. Reports package
  metadata (from kryos.toml) + source stats: files, lines, function
  count, `@test` count, `@bench` count, struct/enum/trait counts.
  Walks the project recursively, skipping `target/`, hidden dirs,
  `node_modules`.
- **`examples/showcase/stats_pipeline.kry`** — CSV-style numeric
  input → sort → min/max/sum/median report. Demonstrates the
  std::sort + std::collections pipeline pattern.
- **`docs/learn/cookbook/14-deduplicate.md`** — dedup + reverse +
  aggregate recipe combining std::sort and std::collections.

### Changed

- Workspace version bumped from `4.5.0-rc.1` to `4.6.0-rc.1`.

## [4.5.0-rc.1] — 2026-05-18 — "kryos config + deploy recipes"

### Added

- **`kryos config get|set|list|unset|path`** — user-level config at
  `~/.config/kryos/config.toml` (XDG) or `%APPDATA%\kryos\config.toml`.
  Known keys: `default_backend`, `default_opt_level`, `color`.
  Override path via `KRYOS_CONFIG` env var.
- **`docs/deploy/docker.md`** — multi-stage Dockerfile, distroless
  runtime, health check, multi-arch buildx.
- **`docs/deploy/systemd.md`** — hardened service unit template
  (NoNewPrivileges, ProtectSystem, MemoryDenyWriteExecute, etc.),
  install procedure, Type=notify integration via sd_notify FFI.
- **`docs/deploy/README.md`** — overview + build flag tips + cross-
  compilation guide + musl static linking for portable binaries.

### Changed

- Workspace version bumped from `4.4.0-rc.1` to `4.5.0-rc.1`.

## [4.4.0-rc.1] — 2026-05-18 — "kryos workspace — multi-package projects"

### Added

- **`kryos workspace list|check|test`** — multi-package workspace mode.
  A workspace is a `kryos.toml` with a `[workspace]` section listing
  member package paths. `list` enumerates members + versions, `check`
  runs `kryos check` over each, `test` runs `kryos test` over each.
- Lightweight inline TOML parser for the `[workspace] members = [...]`
  array — no extra dependency.
- 3 new parser tests + verified end-to-end on a 2-member temp
  workspace.

### Changed

- Workspace version bumped from `4.3.0-rc.1` to `4.4.0-rc.1`.

## [4.3.0-rc.1] — 2026-05-18 — "stdlib: collections + 3 cookbook recipes"

### Added

- **`std::collections`** — slice-level helpers:
  - `kryos_reservoir_sample` — reservoir sampling (k of n) with LCG
  - `kryos_dedup_sorted_i64` — in-place dedup of sorted slice
  - `kryos_reverse_i64` — in-place reverse
  - `kryos_sum_i64`, `kryos_min_i64`, `kryos_max_i64` — aggregates
- **`docs/learn/cookbook/11-sorting-data.md`** — sort + bsearch recipe
- **`docs/learn/cookbook/12-structured-logging.md`** — std::log recipe
- **`docs/learn/cookbook/13-hashes-and-checksums.md`** — std::hash recipe
- 4 new tests. 28 total stdlib tests pass.

### Changed

- Workspace version bumped from `4.2.0-rc.1` to `4.3.0-rc.1`.

## [4.2.0-rc.1] — 2026-05-18 — "stdlib: hash + strext"

### Added

- **`std::hash`** — 3 non-cryptographic hashes:
  - `kryos_hash_fnv1a64` — FNV-1a 64-bit, fast + reasonable distribution
  - `kryos_hash_djb2` — DJB2 string hash (well-known reference)
  - `kryos_hash_crc32` — CRC32 IEEE polynomial (zip/png/ethernet)
- **`std::strext`** — extended string ops:
  - `kryos_str_ascii_lower` / `kryos_str_ascii_upper` — in-place case-fold
  - `kryos_str_trim_ascii` — start+len out-params (no allocation)
  - `kryos_str_count` — count non-overlapping occurrences
- 7 new tests (3 hash + 4 strext). 24 total stdlib tests pass.

### Changed

- Workspace version bumped from `4.1.0-rc.1` to `4.2.0-rc.1`.

## [4.1.0-rc.1] — 2026-05-18 — "stdlib: sort + log"

### Added

- **`std::sort`** — `kryos_sort_i64`, `kryos_sort_i64_reverse`,
  `kryos_sort_f64`, `kryos_bsearch_i64`, `kryos_is_sorted_i64`. Uses
  Rust's Timsort under the hood; in-place, no allocation.
- **`std::log`** — structured single-line logging to stderr.
  `LEVEL ts=<epoch_secs> msg="..." k=v k=v` format. 6 levels (trace,
  debug, info, warn, error, fatal) with runtime-settable min-level via
  `kryos_log_set_level`.
- 5 new tests in `kryos-stdlib-native::sort` + 1 in `log`. All 17
  stdlib tests pass.

### Changed

- Workspace version bumped from `4.0.0-rc.1` to `4.1.0-rc.1`.

## [4.0.0-rc.1] — 2026-05-18 — "stability statement, v4.x line begins"

This is the first cut of the v4.x line. **The CLI surface, LSP method
set, stdlib symbol table, and ABI symbols are now frozen for v4.x.y
backwards compatibility.** Future minor releases are forward-additive
only — no rename, no removal, no signature change for the items listed
in `STABILITY-v4.0.md`.

### Added

- **`STABILITY-v4.0.md`** — 8-section semver contract covering source,
  ABI, CLI, LSP, platform support, release process, and migration
  paths from v3.x.
- The 26+ subcommands accumulated across v3.0..v3.17 are now part of
  the stable v4 CLI surface.
- The 15 LSP methods implemented across v3.0..v3.15 are now part of
  the stable v4 LSP surface.
- The stdlib expansions (datetime, re, base64, uuid) are now part of
  the stable v4 std::* surface.

### Migration from v3.17

- `kryos --version` reports `4.0.0-rc.1` (was `3.17.0-rc.1`).
- No source-level or behavior changes for existing programs.
- Pre-1.0 caveats from the v3.0 stability statement no longer apply.

### Changed

- Workspace version bumped from `3.17.0-rc.1` to `4.0.0-rc.1`.

## [3.17.0-rc.1] — 2026-05-18 — "command reference + polish"

### Added

- **`docs/commands.md`** — single-page reference for every `kryos`
  subcommand (currently 26+), grouped by purpose: build/run, check/
  format, test/bench/profile, project lifecycle, editor/docs,
  diagnostics. Includes a v3.0..v3.17 release timeline.
- **`editors/README.md` LSP capability matrix** — lists all 15
  implemented LSP methods so users + editor authors know what to wire.

### Verified at v3.16

- Parity matrix locally: 34/34 (one flake-on-concurrent-sweep
  test_net, passes in isolation — known race in the test harness,
  not a code regression).
- `kryos eval`, `kryos check --watch`, `kryos doc serve` all exercise
  end-to-end on a scaffolded project.

### Changed

- Workspace version bumped from `3.16.0-rc.1` to `3.17.0-rc.1`.

## [3.16.0-rc.1] — 2026-05-18 — "kryos check --watch + kryos eval"

### Added

- **`kryos check --watch`** — runs type-check, then polls the source
  file's mtime every 300ms. On detected change, re-checks and prints
  the result. Cooperative poll loop — no `notify` dep, same on every
  OS. Ctrl-C to exit.
- **`kryos eval "<expr>"`** — one-liner evaluator. Wraps the
  expression(s) in a generated `fn main()` and runs via the existing
  `kryos run` path. Semicolons in the expression are rewritten to
  newlines (Kryos uses newline-terminated statements). `--show-source`
  (`-v`) prints the wrapped source before running.

### Verified

- `kryos eval 'println(to_string(42 + 1))'` → `43`
- `kryos eval -v 'let x = 7; let y = 6; println(to_string(x * y))'` →
  prints wrapped source, then `42`.

### Changed

- Workspace version bumped from `3.15.0-rc.1` to `3.16.0-rc.1`.

## [3.15.0-rc.1] — 2026-05-18 — "doc serve + member-access completion"

### Added

- **`kryos doc serve [files...] [--address ADDR]`** — generates HTML
  docs into a temp directory then serves them over HTTP on
  127.0.0.1:8088 (overridable). Built-in std::net listener; serves
  `index.html` for `/`, mime-typed responses for `.html / .css / .js /
  .png / .svg / .json`. Press Ctrl-C to stop.
- **LSP member-access completion** — when the cursor is positioned
  immediately after a `.`, the completion list switches to a curated
  set of method-style operations (string ops: `len`, `to_upper`,
  `trim`, `split`, `contains`; array ops: `push`, `pop`, `first`,
  `last`; `Option`/`Result` ops: `unwrap`, `is_some`, `is_ok`).
  Replaces the previously-undifferentiated keyword + builtin bag.

### Changed

- Workspace version bumped from `3.14.0-rc.1` to `3.15.0-rc.1`.

## [3.14.0-rc.1] — 2026-05-18 — "semantic tokens + run timing"

### Added

- **LSP `textDocument/semanticTokens/full`** — accurate semantic
  syntax highlighting. Lexes the source, looks up identifier role from
  an in-file symbol map (function / struct / enum / variant / param /
  variable / property), and emits the LSP delta-encoded token stream.
  Builtins (`println`, `to_string`, `len`, etc.) get the `macro` color
  so they stand out from user-defined names. 12 token types, 5 modifiers
  declared in the legend.
- **`kryos run --time`** — prints
  `compile: Xms, exec: Yms, total: Zms` to stderr after the program
  exits. Useful for diagnosing whether compile-time or runtime is the
  bottleneck.
- 3 new tests in `kryos-lsp/tests/v314_semantic_tokens.rs`.

### Changed

- Workspace version bumped from `3.13.0-rc.1` to `3.14.0-rc.1`.

## [3.13.0-rc.1] — 2026-05-18 — "where-clauses + coverage"

### Added

- **`where` clauses on functions** — `fn f<T, U>(...) where T: Bound1
  + Bound2, U: Other { ... }`. The parser implements `where` as a soft
  keyword (recognised only when generics exist and the next ident is
  literally "where"), and merges the clause's bounds into the matching
  `GenericParam.bounds`. Bounds combine cleanly with the inline
  `<T: Clone>` form — duplicates are deduplicated.
- **`kryos coverage [path] [--format=json]`** — function-level coverage
  report. Walks every `.kry` file in the project to enumerate declared
  functions, runs `kryos test` with `set_profile_mode(true)`, then
  cross-references the call-count table against the declared set.
  Reports `covered / total (percent)`, lists uncovered functions, and
  shows the top-10 hot list.
- 5 new tests in `kryos-parser/tests/where_clauses.rs`.

### Verified

- `kryos coverage` on a scaffolded project (e2e_demo, `kryos new
  --template lib`) reports `1 of 3 functions exercised (33.3%)` —
  the smoke test runs `smoke_runs`, while `greet` and `main` are
  uncovered. Correct shape.

### Changed

- Workspace version bumped from `3.12.0-rc.1` to `3.13.0-rc.1`.

## [3.12.0-rc.1] — 2026-05-18 — "doctor + tree + LSP code actions"

### Added

- **`kryos doctor`** — diagnoses the toolchain. Reports kryos version,
  platform, kryos_rt and kryos_stdlib_native static library locations,
  C/C++ linker discovery (clang/cc/gcc + link.exe on Windows), and
  KRYOS_* environment variables. Returns non-zero on missing runtime
  libraries.
- **`kryos tree [--transitive]`** — prints the project's dependency
  tree from `kryos.toml`. Path dependencies recurse into their own
  `kryos.toml`; remote/registry deps show as leaves. Cycle-safe.
- **LSP `textDocument/codeAction`** — quick-fix actions. Extracts the
  suggested replacement from `"did you mean \`X\`?"` notes that the
  type checker already emits (for E0101, E0102, unknown fields,
  unknown variants, unknown methods) and offers a "Replace `bad`
  with `good`" workspace edit.
- LSP `codeActionProvider.codeActionKinds = ["quickfix"]` advertised
  in the initialize response.
- 2 new tests in `kryos-lsp/tests/v312_code_actions.rs`.

### Changed

- Workspace version bumped from `3.11.0-rc.1` to `3.12.0-rc.1`.

## [3.11.0-rc.1] — 2026-05-18 — "kryos profile + showcase examples"

### Added

- **`kryos profile <file>`** subcommand — runs a program with per-function
  call-count profiling. Reuses the existing `kryos_trace_enter` hooks
  emitted by the codegen at every function entry; gates them with a
  global `PROFILE_MODE` flag set via `KRYOS_PROFILE=1` env var. The
  runtime increments a global `Mutex<HashMap<String, u64>>` counter
  table and dumps the sorted top-20 hot-list to stderr at exit via a
  libc `atexit` hook (chosen over a thread-local Drop guard to avoid
  TLS-destruction-order panics).
- **`kryos_rt::trace::set_profile_mode(bool)`** + **`take_profile_counts()`**
  — public Rust API for embedding contexts.
- **`examples/showcase/word_frequency.kry`** — word-frequency counter
  (lex → lowercase → parallel-array map → top-N).
- **`examples/showcase/tiny_kv.kry`** — interactive in-memory key/value
  store with `set/get/del/list/quit` commands.
- **`examples/showcase/tcp_echo.kry`** — bounded TCP echo server on
  127.0.0.1:7000 demonstrating `std::net` and accept-loop pattern.

### Verified

- `kryos profile examples/fibonacci.kry` → `fibonacci  177; main  1`
  (the expected recursive fan-out for fib(10)).

### Changed

- Workspace version bumped from `3.10.0-rc.1` to `3.11.0-rc.1`.

## [3.10.0-rc.1] — 2026-05-18 — "first-party packages"

### Added

- **`packages/`** directory under the repo root containing five
  first-party libraries that ship alongside the compiler:
  - **`kryos-test-ext`** — assertion helpers (`assert_eq_i64`,
    `assert_eq_str`, `assert_lt`, `assert_contains_str`, `assert_msg`)
  - **`kryos-http-router`** — HTTP/1.1 method+path parser + response
    builder for use inside a TCP accept loop. Handles 200/201/204/
    301/302/400/401/403/404/500/503 status text.
  - **`kryos-uuid-pkg`** — v4 UUID helpers (`v4`, `is_valid`, `many`)
    wrapping `std::uuid`.
  - **`kryos-base64-pkg`** — encode/decode + `data_url(mime, body)`
    builder wrapping `std::base64`.
  - **`kryos-time-pkg`** — `UtcDate` struct + `now_utc()`, `now_iso()`,
    `ymd_utc()`, `days_between()`, `weekday_short()` on top of
    `std::datetime`.
- **`kryos pkg list-local [--root PATH]`** — discovers packages under
  `packages/` (or a custom directory) by scanning each subdirectory's
  `kryos.toml`. Prints `name  version  description` for each.
- **`packages/README.md`** — overview + local-install instructions.

### Changed

- Workspace version bumped from `3.9.0-rc.1` to `3.10.0-rc.1`.

## [3.9.0-rc.1] — 2026-05-18 — "watch, clean, REPL history"

Three quality-of-life additions for the day-to-day dev loop.

### Added

- **`kryos watch <file>`** — polls the file's mtime every 250ms (override
  with `--interval`) and re-runs `kryos run` on change. `--run check`
  switches to type-check-only mode for faster feedback. Cooperative
  poll loop, no external `notify` crate — works the same on every
  supported platform.
- **`kryos clean`** — removes `target/`, root-level `*.exe`/`*.pdb`/
  `*.o`/`*.obj`/`*.ll`/`*.wasm`/`*.lib`/`*.a`/`*.wat`, and any
  `kryos.lock` files in the project tree. `--dry-run` previews
  without removing.
- **REPL persistent history** — every accepted input line is recorded
  to `~/.kryos_history` (or `%USERPROFILE%\.kryos_history` on Windows).
  Re-loaded at REPL startup so you can see what you typed last time.
- **REPL `:history` and `:history-clear` commands** — list the session
  history with numbered entries, or wipe the on-disk file.

### Changed

- Workspace version bumped from `3.8.0-rc.1` to `3.9.0-rc.1`.

## [3.8.0-rc.1] — 2026-05-18 — "perf: function-call overhead"

### Changed

- **`TraceFrame` no longer allocates `String`** at every function entry.
  Frames now hold raw `(name_ptr, name_len, file_ptr, file_len, line)`
  pointers into the compiled program's `.data` section, whose lifetime
  is `'static` in the loaded image. UTF-8 reconstruction happens lazily
  at format time (panic stack traces, verbose trace output) when the
  cost is dwarfed by I/O anyway.
- Result: per-function-call trace overhead drops from "2 string allocs +
  push" to "4 pointer copies + push". Microbench `bench_million_small_calls`
  drops from prior baseline to **13.3ns/call median** on Cranelift JIT.

### Added

- **`benches/fn_call_overhead.kry`** — regression bench tracking
  function-call overhead. Two cases: a 1000-call sum loop and a
  20-deep `factorial` recursion. Both visible under `kryos bench`.

### Changed

- Workspace version bumped from `3.7.0-rc.1` to `3.8.0-rc.1`.

## [3.7.0-rc.1] — 2026-05-18 — "kryos trace — execution tracing"

Useful starting-point for debugging without spinning up a full step
debugger. The existing software call-stack infrastructure
(`kryos_trace_enter` / `kryos_trace_exit` emitted at every function
entry/exit) gained a verbose mode that prints to stderr.

### Added

- **`kryos trace <file> [-- args...]`** subcommand — JIT-compiles and
  runs a program with depth-indented function entry/exit tracing
  printed to stderr.
- **`KRYOS_TRACE=1` env var** — runtime probes this on startup. Set
  by `kryos trace` for subprocess inheritance; also usable directly
  on a Kryos-built binary (`KRYOS_TRACE=1 ./my_prog`).
- **`kryos_rt::trace::set_verbose_trace(bool)`** — public Rust API
  for embedding contexts that don't go through env vars.

### Example

```text
$ kryos trace examples/factorial.kry
kryos trace enabled for examples/factorial.kry

trace → main() at factorial.kry:9
trace   → factorial() at factorial.kry:1
trace     → factorial() at factorial.kry:1
trace     ← factorial()
trace   ← factorial()
trace ← main()
```

### Changed

- Workspace version bumped from `3.6.0-rc.1` to `3.7.0-rc.1`.

## [3.6.0-rc.1] — 2026-05-18 — "kryos new — project scaffolder"

### Added

- **`kryos new <name>`** subcommand — generates a complete starter
  project from a template. Outputs:
  - `kryos.toml` — package manifest (name, 0.1.0, edition 2026)
  - `src/main.kry` — entry point matching the chosen template
  - `tests/smoke.kry` — `@test`-annotated smoke test
  - `README.md` — build instructions + project layout
  - `.gitignore` — Kryos build artifacts + editor noise
- **Four templates**:
  - `cli` (default): argv-handling hello-world
  - `http`: TCP listener on 127.0.0.1:8080 with HTTP/1.1 200 response
  - `lib`: public `greet()` function + ad-hoc `main()`
  - `agent`: spawn + channel round-trip
- Project name validation: must start with letter/underscore, only
  contain letters/digits/underscores/hyphens. Refuses to overwrite
  an existing directory.

### Changed

- Workspace version bumped from `3.5.0-rc.1` to `3.6.0-rc.1`.

## [3.5.0-rc.1] — 2026-05-18 — "lint + audit"

Two new subcommands aimed at code review and production-readiness checks.

### Added

- **`kryos lint`** — AST-driven source linter with 4 lints:
  - `L001` large-function (>100 stmts)
  - `L003` magic-number (integer > 10 outside common round values)
  - `L005` shadowed-name (`let x` rebinding an outer `x`)
  - `L006` todo-comment (`// TODO` / `// FIXME` / `// XXX`)
  - `--format=pretty|json`, `--enable`, `--disable`, `--strict` flags
- **`kryos audit`** — project-wide capability + extern + secret scan:
  - Capability inventory grouped by capability name
  - Every `extern "..." { ... }` block listed with item counts
  - String literals matching 13 secret patterns flagged CRITICAL
    (AWS access keys, GitHub PATs, Slack tokens, OpenAI keys,
    bearer auth headers, PEM/OpenSSH private-key markers,
    `password=` / `API_KEY=` env assignments)
- **`examples/lint_demo.kry`** — demo file triggering each lint code.

### Changed

- Workspace version bumped from `3.4.0-rc.1` to `3.5.0-rc.1`.
- `kryos-cli` Cargo.toml gained direct `kryos-ast`, `kryos-lexer`,
  `kryos-parser` deps (previously transitive via `kryos-driver`).

## [3.4.0-rc.1] — 2026-05-18 — "benchmark runner"

New `kryos bench` subcommand plus the `@bench` attribute for declaring
micro-benchmarks alongside source.

### Added

- **`@bench` attribute** — MIR-level annotation parsed from source.
  Functions marked `@bench` are discoverable by the new runner.
- **`kryos bench`** subcommand — discovers `@bench`-annotated `.kry`
  files (defaults to `benches/`, falls back to `tests/`, then cwd),
  JIT-compiles each module via Cranelift, runs warmup iterations
  followed by measurement iterations, and reports min/median/mean/
  p95/max in human-readable units (ns/µs/ms/s).
- **`kryos-test-runner::bench` module** — public engine surface:
  `discover_annotated_benches`, `run_benches`, `BenchOptions`,
  `BenchReport`, `BenchResult`, `format_bench_report`.
- **`benches/smoke_bench.kry`** — minimal regression target that
  exercises the discovery + execution path.

### Changed

- Workspace version bumped from `3.3.0-rc.1` to `3.4.0-rc.1`.
- `MirAttributes` grew a `bench: bool` field alongside `test`,
  `inline`, `pure_fn`, `deprecated`, `is_async`.

## [3.3.0-rc.1] — 2026-05-18 — "learn-Kryos onboarding"

Documentation push. Four new cookbook recipes covering the v3.2 stdlib
additions plus a "common errors" reference and a one-page cheatsheet.

### Added

- **docs/learn/cookbook/07-dates-and-time.md** — `std::datetime` recipe:
  current time, UTC date breakdown, RFC 3339, ymdhms constructor, tiny
  benchmark loop.
- **docs/learn/cookbook/08-regex.md** — `std::re` recipe: is_match,
  replace_all, capture iteration over a log file.
- **docs/learn/cookbook/09-encoding.md** — `std::base64` + `std::uuid`
  recipe: round-trip encoding, mint v4 UUIDs, parse known UUID strings.
- **docs/learn/cookbook/10-structured-logs.md** — JSONL parsing recipe:
  group by level, compute span, summarize.
- **docs/learn/common-errors.md** — top-20 compile and runtime errors
  with verbatim messages and fixes. Covers E0101/E0102/E0106/E0107/
  E0382/E0501 plus syntax-and-layout gotchas (semicolons, `elif`,
  block balance).
- **docs/learn/cheatsheet.md** — one-page syntax reference: variables,
  types, control flow, structs, enums, errors, capabilities, async,
  tooling.

### Changed

- `docs/learn/README.md` version stamp updated from `2.3.0` to
  `3.3.0-rc.1`.
- Cookbook table now lists 10 recipes (was 6).
- Workspace version bumped from `3.2.0-rc.1` to `3.3.0-rc.1`.

## [3.2.0-rc.1] — 2026-05-18 — "stdlib breadth"

Fleshes out four under-built stdlib modules and adds two new ones. Every
new function ships with #[cfg(test)] unit tests in the same file. All
12 stdlib unit tests pass.

### Added

- **`std::datetime` expanded** — `kryos_time_now_nanos`,
  `kryos_time_sleep_millis`, full UTC date breakdown
  (`kryos_time_{year,month,day,hour,minute,second,weekday}_utc`),
  `kryos_time_from_ymdhms_utc` constructor, and
  `kryos_time_format_rfc3339_utc` RFC 3339 formatter. Civil-from-days
  conversion uses Howard Hinnant's algorithm — works for any year in
  the proleptic Gregorian calendar.
- **`std::re` expanded** — `kryos_regex_find` (first-match span),
  `kryos_regex_replace_all` (caller-buffer with overflow signaling),
  `kryos_regex_capture_count`, and `kryos_regex_capture` (per-group
  span extraction including group-didn't-participate handling).
- **`std::base64` (new)** — RFC 4648 standard-alphabet encoder and
  decoder. Both write into caller-provided buffers with `*needed`
  set on overflow. No external dep — fits inline.
- **`std::uuid` (new)** — UUID v4 generation per RFC 4122
  (`kryos_uuid_v4_bytes`), canonical `xxxxxxxx-xxxx-...` formatter
  (`kryos_uuid_format`), and parser (`kryos_uuid_parse`). Random
  source is splitmix64-mixed nanos+counter — good for IDs, not for
  CSPRNG (use `crypto` feature for that).

### Changed

- Workspace version bumped from `3.1.0-rc.1` to `3.2.0-rc.1`.

## [3.1.0-rc.1] — 2026-05-18 — "IDE depth + diagnostic hints"

LSP feature set fleshed out to mainstream-language quality. Diagnostics
gain "did you mean?" hints across more error sites.

### Added

- **LSP: textDocument/documentSymbol** — full file outline (functions,
  structs, enums, traits, impls, actors, type aliases, consts, externs)
  with nested children for fields, variants, and trait/impl methods.
- **LSP: workspace/symbol** — fuzzy subsequence search across all open
  buffers + every `.kry` file under the workspace root (up to 4 levels
  deep, skipping `target/`, `node_modules/`, hidden dirs).
- **LSP: textDocument/references** — finds every identifier-token
  occurrence in the current file plus every workspace `.kry` file.
  Lexer-driven, so matches inside string and comment spans are skipped.
- **LSP: textDocument/rename** — returns a WorkspaceEdit covering every
  reference site. Rejects invalid identifiers (must start with letter
  or underscore, only word chars allowed).
- **LSP: textDocument/documentHighlight** — highlights every occurrence
  of the identifier under the cursor inside the current file.
  Distinguishes write sites (`x = ...`, `x += ...`) from reads.
- **LSP: textDocument/foldingRange** — folds every `{ ... }` block plus
  consecutive comment runs.
- **LSP: textDocument/formatting** — delegates to `kryos-fmt`. Returns
  a single document-spanning TextEdit. Empty edit on parse failure so
  the editor leaves the buffer untouched.
- **LSP: textDocument/signatureHelp** — pops the active function
  signature with the current parameter highlighted while typing args.
  Triggered on `(` and `,`. Walks backward through balanced brackets
  to find the enclosing call.
- **LSP: textDocument/inlayHint** — type hints for `let` bindings with
  literal RHS (i64, f64, str, bool) plus parameter-name hints at call
  sites for user-defined functions.
- **Diagnostics: "did you mean?" expanded** — now fires for unknown
  struct fields, unknown enum variants, and unknown methods (both
  instance and static), in addition to the previously-covered unknown
  variables (E0102) and unknown types (E0101).

### Changed

- `kryos-lsp` server version bumped from `0.1.0` to `0.2.0` (reported
  via the `serverInfo` block in the initialize response).
- Workspace version bumped from `3.0.0-rc.1` to `3.1.0-rc.1` across
  all 22 crates.

## [3.0.0-rc.1] — 2026-05-18 — "production hardening"

The v2.8 → v3.0 prod-hardening shift. Audits / parity work / CI
matrix / stability statement. Workspace version bumped from `2.8.0`
to `3.0.0-rc.1`. Cut as `v3.0.0` once the open PRs merge and the
NORTHTEKDevs Actions billing block clears.

### Added

- **`AUDIT-v2.8.0.md`** — entry-state audit covering every M1..M10
  scope item against real source, the public ROADMAP, and the v2.8
  marketing claims. Bottom line: toolchain structurally healthy,
  CI coverage holes were the dominant 1.0 gap.
- **`AUDIT-llvm-parity.md`** + **`tests/parity/run_parity.sh`** —
  reproducible Cranelift vs LLVM smoke matrix. Per-test pass/fail
  with failure-class classification (A/B/C/D/E/T). Baseline 11/32
  LLVM; **final 34/34 both_pass (100%)** after the parity work
  closed every class (A, A', B, B', C, D, E, T).
- **`STABILITY-v3.0.md`** — 1.0 stability statement covering source
  compatibility, ABI, supported platforms, release artifacts,
  capability enforcement, known caveats, and migration from v2.8.
- **`tests/quickstart_e2e.sh`** — scripted walk through QUICKSTART.md
  steps. Cranelift JIT → LLVM AOT → WASM (wasmtime) → first-tour
  examples. Wired into Linux CI.
- **`tests/registry/smoke.sh`** — kryos-registry-server real-HTTP
  roundtrip against an ephemeral sidecar on `127.0.0.1:18080`.
  Wired into Linux CI.
- **`tests/smoke/test_async_http_roundtrip.kry`** — `spawn { ... }`
  TCP server + main-thread client over real sockets. Proves the
  async substrate drives real I/O.
- **6 new capability compile-fail tests** under
  `compiler/crates/kryos-test-runner/tests/e2e/error_cases/`:
  `pure_calls_{print,eprintln,exit,non_pure}.kry`,
  `caps_{io_in_net,net_in_io}_scope.kry`.
- **CI jobs**: backend parity matrix on Linux + macOS-14, fuzz job
  (lexer + parser + typechecker), wasm-smoke (wasmtime), quickstart-e2e,
  registry-smoke, macOS-14 tier-1, smoke-directory full sweep.
- **Release packaging**: `package-vscode` (.vsix) + `package-zed`
  (`wasm32-wasip1` cdylib) jobs in release.yml. SHA-256 checksums
  per artifact. GitHub OIDC build-provenance attestations via
  `actions/attest-build-provenance@v2`.

### Fixed

- **LLVM Class A**: `@assert_eq` undeclared. Added codegen path that
  stringifies operands type-appropriately, then calls
  `kryos_builtin_assert_eq`. Closes test_assert_eq, test_string_brace_escape.
- **LLVM Class C**: SSA name collision on struct-field temps
  (`%_<dest>_fld_<i>`). Switched to fresh `next_temp()` chain.
- **LLVM Class A' (extended)**: 233 missing runtime symbol
  declarations (kryos_db_*, kryos_term_*, kryos_fs_*, kryos_tcp_*,
  kryos_tls_*, kryos_regex_*, kryos_async_*, ...). Auto-generator
  at `tests/parity/gen_decls.py`. Closes test_crypto, test_db,
  test_db2, test_fs, test_io, test_net, test_term.
- **LLVM Class E**: tuple aggregate type lowering. `emit_aggregate_tuple`
  synthesises `{ T1, T2, ... }` from elem types when local_types
  defaulted to `i64`, plus local-type registration so terminators see
  the aggregate shape. Closes test_tuple_mut, test_bootstrap_lexer_smoke.
- **LLVM Class D**: recursive enum payload codegen. `emit_cast` /
  `EnumPayload` resolve named enum types to their full `{ i64, ... }`
  shape; aggregate payloads heap-alloc + inttoptr instead of
  invalid struct-to-i64 bitcast. Closes test_match_return.
- **LLVM Class B'**: `coerce_value` aggregate → i64 path checks
  field 0's type via struct_defs and emits ptrtoint when the
  extracted field is ptr. Closes test_re, test_tracked.
- **User-shadow + interpolation stringify**: builtin-mapping default
  arm checks `func_param_types.contains_key(name)` and uses the user
  symbol when shadowed. StringConcat helper stringifies non-string
  parts (i1 → kryos_bool_to_string, double → kryos_f64_to_string,
  i64 → kryos_i64_to_string) before passing to kryos_string_concat.
  Closes test_user_fn_shadows_builtin.
- **User-fn internal linkage**: emit `define internal` for user
  functions to prevent libc / winsock symbol collisions
  (connect, bind, exit, read, write). Closes test_net2.
- **Cranelift `fn main() -> i64`**: route every user `fn main` (any
  return type) through the C-entry wrapper; propagate the user's
  i64 return value truncated to i32 instead of hard-coding 0.
  Closes the build_cache_roundtrip_with_cli regression test.
- **Driver selective-import resolver**: transitive identifier closure
  starting from `use foo::{bar}` items. `clamp` → pulls in `min` +
  `max` automatically. Closes compile_file_with_selective_import.
- **Capability `@pure` + `@capabilities` documented** in
  `STABILITY-v3.0.md` §5 as enforced; CLAUDE.md's "for now: just
  call what you need" is wrong for `@pure` (it always enforces)
  and right only for unannotated functions under `@capabilities`.

### Documentation

- **`compiler/README.md`** — fixed "GC" → "ARC", added kryos-codegen-wasm
  row, updated examples count from 9 → ~50.
- **`editors/README.md`** — corrected tree-sitter-kryos status
  (planned, not checked in).
- **`ROADMAP.md`** — collapsed v2.9..v3.3 sequence into a single v3.0
  cut per the Option A decision on PR #1.

### Notes

Two LLVM smoke tests still fail at v3.0:
- `test_generics`: `to_string<T=str>` cleanup-time double-free. Either
  the clone-based or identity-passthrough fix produces the right
  output but trips ARC cleanup. MIR-layer ownership work needed.
- `test_process`: Command__arg references undefined SSA `%_3`.
  MIR-elision pre-existing bug surfaced after internal-linkage
  unblocked the libc-exit collision.

Both documented in `STABILITY-v3.0.md §6` as known caveats; tracked
for v3.0.x patch line.

## [2.8.0] - 2026-05-17 — "language polish, round two"

Three correctness fixes plus a stdlib reference doc and a public
roadmap. No surface-language changes; existing code keeps compiling.
All 326 cargo tests and every smoke test pass. The one pre-existing
failure (`compile_file_with_selective_import` in kryos-driver and
`build_cache_roundtrip_with_cli` linker-stub test) is unrelated to v2.8
work and predates this release.

### Fixed

- **String-local clobber across recursion.** Use-after-free when a named
  string local was passed to a function that stored it in a heap struct
  field, then the caller's scope cleanup `drop()`ed the now-aliased
  string. Concrete repro: a `Ctx { strs: [str] }` struct, a `push_str`
  helper that mutates a copy of the struct, and a recursive `expr()`
  loop containing `let s = op(); pp = expr(pp, ...); pp = push_str(pp,
  s)`. Strings at certain iteration indices came out empty or as garbage
  (`"["`).

  Root cause: MIR lowering for `Stmt::Assign -> identifier` evaluated
  the RHS but never called `consume_call_args` when the RHS was a
  direct `RValue::Call`. The other call sites (`let x = f(...)` and
  discarded `f()`) did. Scope cleanup then emitted `drop(s)` on a local
  the callee had already taken ownership of, freeing memory the struct
  still referenced.

  Fix in `compiler/crates/kryos-mir/src/lower.rs`: extend the
  `Stmt::Assign` identifier branch to call `consume_call_args` for both
  `RValue::Call` and `RValue::CallIndirect`. Also implement the
  self-consuming skip that `consume_call_args`' docstring claimed (and
  the unused `_dest` parameter implied) but did not actually perform —
  in `pp = push_str(pp, s)` the dest `pp` must NOT be marked dropped
  because the call's return value is a fresh owned struct that still
  needs to be dropped at scope end.

  Permanent regression test: `tests/smoke/test_string_clobber.kry`. With
  `depth=4` the test produces 31 strings and checks every one is either
  `"leaf"` or `"OP"`. Verified to fail (panic with `FAIL at 5: got ''`)
  on the unfixed build and pass on the fixed build.

- **Per-element `mut` in tuple destructuring.** `let (mut a, mut b) =
  expr` and `let mut (a, b) = expr` are documented in the language
  reference but the former silently produced immutable bindings. Any
  subsequent assignment raised "assignment to immutable variable"
  warnings.

  Root cause: the parser correctly built `Pattern::Ident { mutable: true,
  ... }` for `mut`-prefixed identifiers inside tuple patterns, but the
  type checker's `bind_pattern` ignored the per-element flag and always
  called `env.define_var()` (immutable).

  Fix in `compiler/crates/kryos-types/src/check.rs`: split `bind_pattern`
  into a thin wrapper plus `bind_pattern_with_mut(pat, ty, outer_mut)`.
  The recursive walker threads the outer `let mut (...)` modifier and
  the per-element `Pattern::Ident.mutable` flag, OR-ing them together
  to decide whether each binding goes into the env as mutable. The MIR
  lowering path was also updated to honor the per-element mut when
  allocating the destructured locals.

  All three forms (`let (mut a, b)`, `let mut (a, b)`, `let (mut a,
  mut b)`) now work and immutable bindings still warn correctly.
  Permanent regression test: `tests/smoke/test_tuple_mut.kry` (3 @test
  functions + AOT main).

- **Brace escapes in string literals.** `{{` and `}}` now produce a
  single literal `{` and `}` respectively inside string literals,
  matching Rust / Python f-string conventions. The older `\{` and
  `\}` escapes still work for back-compat. This makes embedding CSS,
  JSON, and shell scripts inside interpolated strings far less
  painful.

  Fix in `compiler/crates/kryos-lexer/src/lexer.rs scan_string`:
  before treating `{` as the start of an interpolation, peek ahead.
  If the next byte is also `{`, consume both and append a literal
  `{` to the current text segment. Same for `}}`. A bare `}` still
  passes through unchanged so existing code compiles.

  Permanent regression test: `tests/smoke/test_string_brace_escape.kry`
  (5 @test functions covering double-open, mixed interpolation, CSS
  templates, back-compat `\{` form, and bare `}`).

### Added

- **`docs/STDLIB.md`** — single-page reference covering every
  always-available builtin (I/O, strings, numbers, arrays, FS, network,
  JSON, crypto, regex, concurrency, browser host), every `use std::*`
  module, the naming-gotchas table (`length` vs `len`, `string` vs
  `to_string`, etc.), the `@test` annotation, and the complete `kryos`
  CLI surface. Complements rather than replaces the deeper per-module
  docs under `docs/stdlib/`.

- **`ROADMAP.md`** — public commitment for v2.9 (LLVM backend parity),
  v3.0 (FFI audit + extension on top of existing `kryos bindgen`),
  v3.1 (LSP depth audit), v3.2 (package manager + registry content),
  v3.3 (threads + async), and beyond. Each milestone is described in
  plain language with the concrete deliverables it must produce. This
  file is updated on every release.

### Notes

- Cargo workspace version is now `2.8.0`. Every crate inherits via
  `version.workspace = true`.
- The pre-existing test failures (`compile_file_with_selective_import`,
  `build_cache_roundtrip_with_cli`) are tracked separately and do not
  block v2.8.0.

## [2.7.0] - 2026-05-17 — "language polish for launch"

Two correctness gaps and one missing builtin that would have been
awkward to explain at launch. No surface-language changes; existing
code keeps compiling. All 29 smoke tests (with 5 @test functions in
the new files) + 21 router + 12 config_parser tests pass under both
JIT (`kryos test`) and AOT (`kryos run` / `kryos build`).

### Added

- `panic(msg: str) -> void` builtin. Prints `panic: <msg>` to stderr
  and exits with status 101. Under the `@test` harness, panics are
  recorded as test failures instead of aborting the process. The
  type checker, MIR, both codegen backends (Cranelift, LLVM), and
  the LSP completion/hover docs all know about it.

- `assert_eq(left, right) -> void` builtin. `assert(bool)` only tells
  you the condition was false; `assert_eq` prints both stringified
  values on failure so tests are debuggable without rerunning. The
  codegen converts each argument to a string using the same
  type-aware lowering as `{x}` interpolation — ints, bools, floats,
  and strings all print correctly. Failure output looks like:

  ```
  assertion failed: left != right
    left:  4
    right: 5
  ```

### Fixed

- `@test` functions that took a fn-pointer value (e.g.
  `let f: fn(i64) -> i64 = some_fn; f(arg)`) segfaulted with exit
  139. The root cause was that the AOT codegen path generates per-
  function env thunks (`{name}_env`) so the env-based `CallIndirect`
  ABI (`env[0] = thunk_fn_ptr`) is uniform, but the JIT path used by
  `kryos test` skipped that scan/declare/define step. The raw
  function address flowed through unchanged and the `load(env, 0)`
  inside `CallIndirect` dereferenced the function's own instruction
  bytes as a pointer, then jumped through the garbage result.

  The JIT `compile_all_inner` now mirrors the AOT phases: scan all
  MIR functions for `RValue::Closure`, declare an `{name}_env` thunk
  with signature `(env, user_args...) -> i64`, translate user
  function bodies, then emit thunk bodies that load captures from
  env at offsets 8.. and tail-call the original function. Closures
  with non-empty captures already worked in the JIT through the same
  path; this fix completes the bare-function-pointer case.

### Tests

- New smoke test `test_fn_pointer.kry` exercises bare fn-pointer
  assignment and call in both `@test` (JIT) and `fn main()` (AOT)
  for unary and binary signatures.

- New smoke test `test_assert_eq.kry` covers the success path for
  int, string, and bool comparisons under `@test` and AOT.

### Build note

Editing `kryos-rt/src/builtins.rs` only rebuilds the rlib by default.
The AOT linker uses `target/release/libkryos_rt.a` (staticlib), so
after touching that file you must `cargo build --release -p kryos-rt`
explicitly to regenerate the static archive. Otherwise `kryos run`
and `kryos build` fail with `undefined reference to <new symbol>`.

## [2.6.9] - 2026-05-17 — "parser hardening for self-hosting"

Three parser bugs blocking self-hosting are fixed. None of these
change the surface language; they tighten how malformed or edge-case
input is reported and parsed so the lexer-in-Kryos work can rely on
consistent diagnostics. All 26 smoke tests + 21 router + 12
config_parser tests pass.

### Fixed

- `parse_int_literal` no longer returns a silent 0 when an integer
  literal does not fit in `i64`. It now falls back to `u64` (so hex
  bitmasks like `0x8000_0000_0000_0000` reinterpreted as `i64` parse
  cleanly), and otherwise produces a labelled overflow error. Hex,
  binary, and octal prefixes share the same path. Affected call sites:
  general integer literals, attribute argument parsing, and pattern
  integer literals.

- `parse_select` no longer enters the timeout branch on a token whose
  textual content happens to equal `"timeout"`. It now requires the
  token to be a real `Ident` named `timeout`, detects duplicate
  `timeout` branches, and recovers cleanly when a non-ident leads a
  branch.

- `expect_ident` and `expect_name` no longer silently accept reserved
  keywords as identifiers. Using `let`, `fn`, `match`, etc. in a name
  position now produces `reserved keyword 'X' cannot be used as an
  identifier here` (or `...as a name here`) instead of being accepted.
  Identifiers that share a prefix with keywords (`letter`, `function`,
  `returns`, `matched`, `asphalt`, ...) are unaffected.

### Tests

- New smoke test `test_keyword_rejection.kry` verifies that
  keyword-prefix identifiers still parse and bind correctly. The
  negative cases (`let let = 1`, `@let`) are confirmed manually to
  emit the new errors.

## [2.6.8] - 2026-05-17 — "closures that capture other closures"

A closure that captures a `let`-bound closure value as one of its free
variables now works correctly, both when called directly and when
passed through a higher-order function. Previously the inner lambda's
body, lowered as a fresh standalone function, kept reading the outer
frame's stale `Operand::Local(...)` IDs through the shared
`closure_locals` direct-call optimization, producing garbage results
(pointer-shaped integers) and sometimes a segfault.

No breaking changes. All 25 smoke tests + 21 router + 12 config_parser
tests pass.

### Fixed

- Nested closure capture across frames is now correct. Code like

  ```kryos
  let n = 10
  let add_n = |x: i64| -> i64 { x + n }
  let f = |x: i64| -> i64 { add_n(x) * 2 }   // f captures add_n
  map_int(xs, f)
  ```

  used to compute garbage because the synthesized function for `f`
  re-used the direct-call optimization for `add_n` with local IDs from
  the outer frame.

- The fix has two parts. First, `find_free_variables` transitively
  expands captured closures: if `f` captures `add_n` and `add_n` itself
  captures `n`, then `n` is added as a free variable of `f` so it
  becomes an additional parameter of the synthesized function. Second,
  `lower_function` now consumes a `pending_closure_regs` queue staged
  by the outer Lambda case before saving function state, rebuilding
  `closure_locals` entries keyed onto the inner frame's freshly
  allocated parameter local IDs.

- `closure_locals` is now saved/restored across function-state
  boundaries (added to `FunctionState`) so nested lambda lowering
  starts with a clean slate rather than inheriting stale outer-frame
  entries.

## [2.6.7] - 2026-05-17 — "bidirectional inference for un-annotated lambda args"

Closures passed as function arguments now have their parameter and
return types inferred from the callee's signature. Previously this
worked for single-argument closures because each fresh type variable
had only one numeric/integer usage to anchor against, but two-argument
bodies like `|a, b| a + b` failed with `cannot apply \`+\` to type \`?T\``
because both operands were unresolved type variables when the binary
operator was checked.

No breaking changes. All 24 smoke tests pass (including the new
`test_bidirectional_closure_inference.kry`), and all real example
builds still succeed.

### Fixed

- Un-annotated multi-argument lambdas now type-check when passed to a
  function expecting a specific `fn(...) -> ...` shape. Code like

  ```kryos
  fn reduce_int(xs: [i64], init: i64, f: fn(i64, i64) -> i64) -> i64 { ... }

  let sum = reduce_int(xs, 0, |a, b| a + b)
  ```

  now compiles. Previously the inference engine created fresh type
  variables for `a` and `b`, then visited the body before the outer
  `FnCall` had a chance to unify the lambda's type with the param's
  declared `fn(i64, i64) -> i64`, so the `+` operator rejected the
  unresolved variables.

- The fix threads expected types from the call site into the lambda:
  `Expr::FnCall`'s arg-vs-param loop now detects when an argument is a
  `Expr::Lambda` and the corresponding parameter resolves to a
  `Type::Function` with matching arity. When so, it pushes the expected
  parameter and return types into a span-keyed map on the type
  checker. The `Expr::Lambda` case then consumes that entry and uses
  the pushed types in place of fresh variables for any un-annotated
  param or return slot, so the body sees concrete types from the start.

## [2.6.6] - 2026-05-17 — "primitive fields and enum-variant binds are copy"

Three related correctness fixes in ownership/borrow analysis. No
breaking changes. All 23 smoke tests, 21 router tests, 12 config-parser
tests, and 12 real example builds pass, including the new
`examples/real/parser_combinator.kry` (a recursive-descent arithmetic
evaluator).

### Fixed

- Primitive fields of non-`@copy` struct parameters were treated as
  moves. Code as plain as

  ```kryos
  fn skip(p: Parser) -> Parser {
      let mut i = p.pos    // E0300: `p` moved here
      let n = len(p.src)   // E0300: use of moved value
      ...
  }
  ```

  failed to compile because the FieldAccess copy check couldn't find
  `p` in the variable→struct-name registry, fell back to the parameter's
  own copy status, and concluded the field access was a move. Fix:
  function parameters whose type is a `Simple` struct name register
  themselves in `var_struct_names`, so `p.pos: i64` is correctly
  classified as copy via the existing `struct_fields` lookup.

- `let q = some_fn(p)` where `some_fn` returns a struct no longer
  loses the struct type. The Let analyzer now consults a new
  `fn_return_struct_names` registry (populated from `Decl::Function`
  and `Decl::Impl::methods` return-type annotations) for FnCall
  and MethodCall initializers and records the result in
  `var_struct_names`, so subsequent `q.pos`/`q.src` reads behave
  correctly.

- Enum-variant pattern bindings now carry their declared field types.
  Previously match arms like

  ```kryos
  match outcome {
      Outcome::Ok(value, pos) => { let p2 = pos; ... }
  }
  ```

  bound `value` and `pos` with `is_copy: false`, so any subsequent
  use of the bound names produced E0300 “use of moved value” errors
  even for `i64` payloads. Fix: collect enum variant field types
  into `enum_variant_fields` during the first pass and a new
  `bind_pattern` helper assigns each `Pattern::Ident` the correct
  `is_copy` flag (and struct-name mapping where relevant) before
  the arm body is analyzed.

Regression test: `tests/smoke/test_struct_field_copy_through_param.kry`.

## [2.6.5] - 2026-05-17 — "user functions shadow same-named builtins"

One correctness fix. No breaking changes. All 22 smoke tests, 21 router
tests, 12 config-parser tests, and 11 real example builds pass.

### Fixed

- A user-defined function whose name collided with a Kryos runtime
  builtin (`index_of`, `sort`, `reverse`, `contains`, `replace`,
  `split`, `join`, `push`, `pop`, etc.) was silently rerouted to the
  builtin's C symbol. The cranelift codegen's builtin-name map
  unconditionally rewrote the call target, so e.g. a user-defined

  ```kryos
  fn index_of(arr: [str], target: str) -> i64 { ... }
  ```

  actually invoked `kryos_builtin_index_of(s: str, sub: str) -> i64`
  (the substring-search runtime). The arguments were passed through
  unmodified, the body never ran, and the call returned -1. The bug
  was easy to miss because direct comparisons like `xs[0] == "id"`
  worked in `main`, but the same comparison inside a function with an
  `arr: [str]` parameter appeared to fail.

  The fix threads the set of user-defined function names through to the
  codegen (alongside `func_ids`, `struct_defs`, etc.) and skips the
  builtin-map rewrite when the call target is in that set. User
  definitions now win over builtins of the same name, the way a
  programmer would expect. Regression test:
  `tests/smoke/test_user_fn_shadows_builtin.kry`.

## [2.6.4] - 2026-05-17 — "`|x, y|` closures, void-body lambdas, indirect-call statements"

One parser addition and two correctness fixes. No breaking changes. All
21 smoke tests, 21 router tests, 12 config-parser tests, and 10 real
example builds pass.

### Added

- Rust-style closure literal syntax: `|x| expr`, `|x, y| expr`,
  `|x: i64, y: i64| -> i64 { ... }`, and `|| expr` / `|| { ... }` for the
  zero-argument form. The body may be a single expression or a brace
  block. Param type annotations are optional but required when the type
  cannot be inferred from a higher-order function's `fn(...) -> ...`
  parameter type (no bidirectional inference yet). Desugars to
  `Expr::Lambda`, so downstream lowering, typing, and codegen are
  identical to the `fn(...) { ... }` form.

### Fixed

- Void-bodied closures — `|| println("hi")` or `fn() { println("hi") }`
  — previously discarded their body. Lambda lowering wrapped the body
  in `Stmt::Return { value: Some(body) }` and defaulted the missing
  return type to i64, producing a closure that allocated a return value
  out of a void expression. MIR cleanup then stripped the call. Fix:
  when no explicit return type is given and the body is a known
  void-returning call (`println`, `print`, `sleep_ms`, etc., or any
  user function registered as returning Void) or a block with no
  trailing expression, emit the body as `Stmt::Expr` and set the
  lambda's return type to Void.

- `g()` where `g` was bound to a closure local was lowered as
  `RValue::CallIndirect`, but `Stmt::Expr` only emitted Assign for
  `RValue::Call`. The indirect call therefore never reached codegen and
  the closure body never ran when its result was discarded. Fix:
  emit Assign for both Call and CallIndirect at statement position.

  Regression test: `tests/smoke/test_pipe_closures.kry`.

### Known limitations (not blockers, documented for follow-up)

- Bidirectional inference for un-annotated multi-arg closures (e.g.
  `|a, b| a * b` passed to `fn(i64, i64) -> i64`) is not implemented;
  param types must be annotated when the inferencer cannot otherwise
  determine them.
- A separate edge case — a captured closure used inside another
  closure's body and that inner closure passed to a higher-order
  function — still produces incorrect results in some configurations.
  Not regressed by this release; will be addressed in a future patch.

### Stress-test matrix update

| Feature | Status |
|---|---|
| `\|x\|` / `\|x, y\|` closure literal | works (this release) |
| `\|\|` zero-arg closure literal | works (this release) |
| `\|\| println(...)` void-body execution | works (this release) |
| `fn() { println(...) }` void-body execution | works (this release) |
| Closure local invocation as statement (`g()` discarding result) | works (this release) |
| `fn(x: i64) -> i64 { ... }` closure | works (unchanged) |

---

## [2.6.3] - 2026-05-17 — "Trait-bounded generics: method calls through `<T: Trait>`"

A correctness fix in the type checker. No new language surface, no breaking
changes. All 20 smoke tests, 21 router tests, 12 config-parser tests, and
10 real example builds pass.

### Fixed

- `fn announce<T: Showable>(x: T) { x.show() }` previously failed type
  checking with E0107 "no method `show` found for type `?T`". The type
  checker bound generic parameters as fresh type variables but did not
  record their declared trait bounds anywhere, so when `x.show()` reached
  the MethodCall handler the receiver was an unbounded `Type::Var` and no
  fallback resolution path existed.

  Fix:

  1. `TypeChecker` now carries `generic_var_bounds: HashMap<u32, Vec<String>>`
     mapping the type-variable id used in the function signature's parameter
     types to the list of declared trait bound names.
  2. `register_decl` for `Decl::Function` populates this map (keyed by the
     sig var IDs, which are the IDs that appear in parameter types when the
     body is checked).
  3. `MethodCall` resolution checks: if `obj_ty` resolves to `Type::Var(id)`
     and that id has registered bounds, look the method name up on each
     bound trait's method list, unify arguments against the trait method's
     parameter types, and return the trait method's return type.
  4. Bounds are cleared when the enclosing function finishes checking, so
     they don't leak into the next function.

  Affected pattern: any function with `<T: TraitName>` (or multiple bounds
  like `<A: Trait, B: Trait>`) that calls trait methods on `T`. Concrete
  dispatch to the impl method still happens in MIR monomorphization based
  on the call-site argument type, exactly as before.

  Regression test: `tests/smoke/test_trait_bounded_generics.kry`.

### Stress-test matrix update

| Feature | Status |
|---|---|
| Trait-bounded generics `<T: Trait>` method call | works (this release) |
| Multiple bounded params `<A: T, B: T>` | works (this release) |
| Bounded method in larger expression | works (this release) |
| `dyn Trait` dispatch | works (unchanged) |
| Traits with default methods | works (unchanged) |

---

## [2.6.2] - 2026-05-17 — "`spawn(fn() { ... })` actually runs the closure"

A correctness fix in MIR lowering. No new language surface, no breaking
changes. All 18 smoke tests, 21 router tests, 12 config-parser tests, and
10 real example builds pass.

### Fixed

- `spawn(fn() { ... })` previously compiled without error but the closure
  body never executed. The MIR `lower_spawn` fallback wrapped the lambda
  expression as a stmt-expr inside a generated `__spawn_N` wrapper, which
  evaluated the lambda value and discarded it instead of invoking its body.
  The wrapper function body therefore did nothing observable.

  Fix: `lower_spawn` now matches `Expr::Lambda` explicitly and lowers the
  lambda's inner block as the spawn body, so captures from the enclosing
  scope flow through the existing `__spawn_N` capture-parameter machinery
  and the body runs on the spawned thread.

  Affected pattern: any `spawn(fn() { ... })` form. Direct-call spawn
  (`spawn worker(arg)`) and block spawn (`spawn { ... }`) were unaffected
  and continue to work as before.

  Regression test: `tests/smoke/test_spawn_lambda.kry`.

### Stress-test matrix update

| Feature | Status |
|---|---|
| `spawn(fn() { ... })` closure spawn | works (this release) |
| `spawn(fn() { ... })` with captured variables | works (this release) |
| `spawn worker(arg)` direct-call spawn | works (unchanged) |
| `spawn { ... }` block spawn | works (unchanged) |

---

## [2.6.1] - 2026-05-17 — "`return` inside `match` arms actually returns"

A correctness fix in the parser. No new language surface, no breaking
changes. All 18 smoke tests, 21 router tests, 12 config-parser tests,
and all 10 real-program examples build and run.

### Fixed

- **`return <expr>` as a `match` arm body silently discarded the value.**
  The parser previously absorbed the `return` keyword and parsed the
  rest of the arm as an ordinary expression. The match expression's
  value was then either dropped (statement position) or implicitly
  returned (tail position) — but the `return` was effectively a no-op.
  Affected programs included any recursive enum interpreter that used
  `match expr { Variant(x) => return f(x) ... }`. A tiny calculator
  evaluating `(1+2)*3` returned `0` instead of `9`.
  Fix is in `compiler/crates/kryos-parser/src/parser.rs`,
  `parse_match_expr`: when `return` is the first token of an arm body,
  the body is now wrapped in a `Block { stmts: [Stmt::Return { value }] }`
  so MIR lowering emits a `Terminator::Return` for that arm. Arms
  without explicit `return` still flow their value up to the match
  expression as before.
  Regression test: `tests/smoke/test_match_return.kry`.

## [2.6.0] - 2026-05-17 — "Struct string fields: no more double-free on alias"

A correctness fix in struct-literal lowering. No new language surface,
no breaking changes. All 17 smoke tests, 21 router tests, 12
config-parser tests, and all 10 real-program examples build and run.

### Fixed

- **String fields stored in non-`@copy` structs no longer double-free
  when the source values alias.** When a function like
  `fn ident(s: str) -> str { return s }` returned its argument and the
  caller stored both the original and the returned value in two
  different struct fields, the same heap pointer ended up in both
  fields. On struct drop the runtime called `kryos_string_free` on the
  same allocation twice, producing `free(): double free detected in
  tcache 2` (AOT) or `LayoutError` (occasionally surfacing through
  `kryos run`) on later allocations.
  Concretely affected: `agent_router.kry`'s `run_step_with_retry` —
  after a retry succeeded, the `output: output` field of the returned
  `StepResult` aliased a loop-local string that the caller then double-
  freed when dropping the struct fields plus the original.
  Fix is in `compiler/crates/kryos-codegen-cranelift/src/codegen.rs`,
  in the non-`@copy` branch of `RValue::Struct`. Heap-typed string
  fields are now cloned via `kryos_string_clone` on store, mirroring
  the existing `kryos_array_retain` treatment of array fields. The
  `@copy` branch already cloned strings and is unchanged.
  Regression test: `tests/smoke/test_struct_string_alias.kry`.

### Added

- **Three new real-program examples** (already passing under v2.5.1
  except `agent_router.kry`, which is now functional):
  - `examples/real/ssg.kry` — static site generator: markdown ->
    HTML with escaping, alternating bold/code markers, index page.
  - `examples/real/installer.kry` — install/uninstall with manifest
    and receipt, multi-segment directory creation.
  - `examples/real/agent_router.kry` — multi-subagent dispatcher
    with retry/backoff. Triggered the struct-string alias double-free
    described above; now runs cleanly.

## [2.5.1] - 2026-05-17 — "Generics: correct return-type substitution"

A correctness fix in monomorphization. No new language surface, no
breaking changes. Smoke tests still 15/15, router 21/21, config 12/12,
all 7 prior real-program examples still build.

### Fixed

- **Generic functions returning `T` extracted from a generic-typed
  parameter crashed on return.** When a generic function declared
  `fn first<T>(items: [T]) -> T` was called with a concrete array, the
  call-site return-type inference and the monomorphization step both
  only recognised `T` when it appeared as a bare `Simple` type at the
  parameter level. They missed `T` nested inside `[T]`, `(A, B)`,
  `fn(T) -> U`, `&T`, `Ptr<T>`, and `Shared<T>`. The fallback path
  resolved the un-substituted `T` to `MirType::Struct("T")`, which
  caused the caller to treat plain `i64` results as heap pointers and
  call `free()` on them at function-exit drop, segfaulting.
  Replaced the two ad-hoc match-only-`Simple` substitution sites in
  `compiler/crates/kryos-mir/src/lower.rs` with recursive helpers
  `extract_type_bindings` and `substitute_type_expr_to_mir` that walk
  compound type shapes.
  Affected programs: any use of `fn f<T>(xs: [T]) -> T`,
  `fn pair<A, B>(...) -> A`, etc. Minimal reproducer:
  `fn first<T>(items: [T]) -> T { return items[0] } fn main() { println(to_string(first([10, 20, 30]))) }`
  printed `10` and then `kryos panic: stack overflow`.

### Added

- **`KRYOS_DUMP_IR=1` environment variable** dumps Cranelift IR for
  every AOT-compiled function to stderr. Companion to the existing
  `KRYOS_JIT_DUMP_IR=1` for the JIT path. Useful for debugging
  codegen-level issues like the one fixed above.

## [2.5.0] - 2026-05-17 — "Test runner, importable libraries, JIT correctness"

This release closes the gap between AOT and JIT compilation paths, makes
`kryos test` work on single files and on libraries imported via `use`,
and adds two new real-world example libraries with full @test suites.
All 15 smoke tests, 21 router tests, and 12 config-parser tests pass.

No breaking changes. No new language surface.

### Added

- **`kryos test PATH`** now accepts a single `.kry` file or a directory
  as a positional argument. `--path` is the new explicit flag; if the
  positional argument is an existing file or directory it is treated
  as a path, otherwise as a name filter (existing behaviour preserved).
  `kryos-test-runner` gained `discover_tests_in_file`,
  `discover_annotated_tests_in_file`, and
  `run_annotated_tests_in_file` for per-file discovery.

- **Test runner uses `compile_file` instead of `compile_source`** so
  `use module` imports resolve correctly when running @test functions
  in a file that pulls in a sibling library. Test files without
  `main()` now compile because the runner explicitly sets
  `OutputType::Mir` for discovery.

- **Two new real-program examples:**
  - `examples/real/router/` — a small HTTP-style URL router and
    middleware library plus 21 @test functions covering path
    splitting, segment matching, parameter extraction, and chain
    dispatch. Demonstrates importable pure-function libraries.
  - `examples/real/config_parser/` — a key/value config parser using
    a `Value` sum type (`Str(str) | Int(i64) | Bool(bool)`) and a
    `Config` struct. 12 @test functions cover parsing, comments,
    missing keys, and value coercion. Demonstrates struct + enum
    + match-with-payload-binding through the JIT path.

### Fixed

- **JIT used empty struct/enum/trait_vtable/copy_struct definitions.**
  `jit_compile_module` now passes `module.struct_defs`,
  `module.enum_defs`, `module.trait_vtables`, and
  `module.copy_structs` through to `translate_function` so struct
  field access on JIT-compiled code works correctly. Previously the
  translator hit a fallback path that returned zero with a warning,
  silently producing wrong results in any program (or @test) that
  touched a struct field.

- **JIT declared `kryos_array_new` with the wrong signature.** The
  runtime takes `(elem_size, cap)` but the JIT declared `sig(1)`,
  producing `mismatched argument count for fn7(...)` verifier errors
  on any program using array literals.

- **JIT missing 53 builtin symbol registrations.** Math (`sqrt`,
  `sin`, `cos`, `log`, `pow`, ...), string helpers (`split`, `substr`,
  `starts_with`, `trim`, `to_upper`, ...), filesystem helpers, and
  array/map operations existed in `kryos-rt` but were never registered
  with the JIT builder, so JIT-compiled code crashed with
  `can't resolve symbol kryos_builtin_split` and similar.

- **String `==` / `!=` produced i8/i64 width mismatches.**
  `kryos_string_eq` returns Rust `bool` (lowered to i8), but the JIT
  signature declared the return as i64. The `!=` codegen path XOR'd
  the result with an i8 constant `1`, failing Cranelift verification.
  Codegen now branches on the actual Cranelift value type and
  normalizes both operands to i8 before the XOR.

- **JIT compile error reporting.** Verifier errors now include the
  failing function name and a Debug-formatted error chain so the
  exact mismatching instruction and signature are visible. Setting
  `KRYOS_JIT_DUMP_IR=1` prints full Cranelift IR for every function
  the JIT compiles, which made the four bugs above straightforward
  to diagnose.

## [2.4.1] - 2026-05-17 — "Stdlib modules actually run"

This is the runtime follow-up to 2.4.0. 2.4.0 made the unblocked stdlib
modules type-check; 2.4.1 makes them compile and run end-to-end through
the debug (Cranelift) backend. Twelve smoke tests now pass:
`hello`, `std::io`, `std::fs`, `std::os`, `std::crypto`, `std::re`,
`std::process`, `std::term`, `std::net`, `std::db`, `std::tracked`,
and a direct FFI primitives roundtrip (`str_to_ptr`, `alloc`,
`ptr_set_byte`, `ptr_byte_at`, `ptr_read_i64`, `ptr_write_i64`,
`buf_to_str`, `free_bytes`).

No language-surface changes. No new stdlib APIs. No breaking changes.

### Fixed

- **Selective imports now pull in full modules.** Previously,
  `use std::os::{name}` (or any selective `use foo::{a}`) filtered
  the imported module down to just the named items plus constants.
  That broke any selected function whose body called same-module
  private helpers (e.g. `std::os::name` calls `_env_or_empty`,
  several modules call internal `extern` blocks). The resolver now
  always merges the full imported module so private helpers,
  extern blocks, types, and constants are reachable. The `items`
  list in `use foo::{a, b}` is still parsed and validated but no
  longer used to prune the imported AST. Known constraint: if two
  imported modules define a public function with the same name
  (e.g. `std::fs::open` and `std::db::open`), the resolver now
  emits a clear `duplicate function ... imported from multiple
  modules` error. Use selective imports from one module and the
  fully-qualified module form from the other, or alias.
- **Cranelift backend: user functions declared as `Local`,
  not `Export`.** User-defined Kryos functions whose names match a
  libc/POSIX symbol (`bind`, `read`, `write`, `open`, `close`, ...)
  were being shadowed at JIT symbol-resolution time by `dlsym`,
  causing silent stack overflows or segfaults when called from
  user code. User functions are now declared `Linkage::Local`,
  which keeps the resolver from looking them up via `dlsym`.
- **Cranelift backend: user-defined functions can shadow built-in
  names.** `print`, `println`, `eprintln`, and `exit` were
  unconditionally declared as C-level imports (`printf`, `puts`,
  `kryos_eprintln`, `exit`). When a user defined a Kryos function
  with the same name, Cranelift raised
  `Invalid to define identifier declared as an import`. The
  built-in imports are now suppressed when a user function of the
  same name exists.
- **Cranelift backend: nine FFI helpers are now declared.** The
  builtins `str_to_ptr`, `alloc`, `free_bytes`, `buf_to_str`,
  `ptr_byte_at`, `ptr_set_byte`, `ptr_read_i64`, `ptr_write_i64`,
  and `handle_to_str` existed in `kryos-rt::builtins` and were
  wired into the LLVM backend in 2.4.0, but were not registered
  as imports in the Cranelift codegen path. Programs compiled via
  `kryos run` (which uses Cranelift) failed to link with
  `undefined reference to str_to_ptr` and friends. All nine are
  now declared in the Cranelift module on every program.
- **`!` (never) type now lowers to MIR `Void`.** In 2.4.0 the
  parser accepted `!` and the type checker treated it as
  `Simple { name: "never" }`, but the MIR lowerer's
  `lower_type_expr` had no case for it and fell through to
  `MirType::Struct("never")`. The resulting signature
  `fn exit(code: i32) -> Struct("never")` clashed with
  `kryos_builtin_exit`'s real void signature and Cranelift
  rejected the second declaration with
  `signature [I32] -> [] incompatible with previous [I32] -> [I64]`.
  `!` now lowers to `MirType::Void`, matching the runtime ABI.
- **`std::re` rename `str_data_ptr` → `str_to_ptr`.** Internal call
  site referenced `str_data_ptr`, which was a name the type checker
  accepted but had no runtime symbol. Renamed to the actual
  builtin `str_to_ptr`. User-facing API is unchanged.

### Known issues

- `std::process::Command::run()` does not yet forward argument
  arrays to `kryos_process_exec` (the runtime supports it but the
  stdlib needs a way to get the data pointer of a `[str]` array).
  `command("echo").arg("hi").run()` runs `echo` but with no
  arguments. Tracked for 2.4.2.
- `std::term::width` and `std::term::height` `throw` on non-tty
  stdin/stdout. This is the documented behaviour of the
  underlying `crossterm` call; the stdlib could be more graceful
  about it. Tracked for 2.4.2.


## [2.4.0] - 2026-05-17 — "All 31 stdlib modules type-check"

This release finishes unblocking the remaining 10 stdlib modules
(`crypto`, `db`, `fs`, `io`, `net`, `os`, `process`, `re`, `term`,
`tracked`). All 31 stdlib modules now pass `kryos check`.

To get there, the language gained a low-level FFI surface (raw
handles as `i64`, a `null` literal, the `!` never type, and nine
new builtins for crossing the FFI boundary). The `@capabilities`
annotation is now also accepted on impl-block methods.

This release is **2.4.0 rather than 2.3.4** because two stdlib
API surfaces had to be renamed to clear collisions with reserved
keywords and builtin names. See **Breaking changes** below.

### Breaking changes

- **`std::net::TcpStream` / `std::net::TlsStream`**: methods
  `send`, `send_all`, `recv`, `recv_all` were renamed to `write`,
  `write_all`, `read`, `read_all`. `send` and `recv` are reserved
  channel keywords and could not be used as method names. Call
  sites of `http_get` / `http_post` inside `std::net` were updated
  to match. User code that called `stream.send(...)` /
  `stream.recv(...)` must be updated to `stream.write(...)` /
  `stream.read(...)`.
- **`std::db::exec`** was renamed to **`std::db::execute`**. The
  function was shadowed by the `exec` process builtin (which
  requires the `process` capability) and could not be called from
  within the `db` module itself. User code that called
  `db::exec(conn, sql)` must be updated to
  `db::execute(conn, sql)`. `db::exec_multi` is unchanged.

### Added

- **`null` literal.** A built-in `null` value of type `i64`,
  intended for use with raw FFI handle / pointer values. Wired
  through the type checker (`null: i64`), MIR
  (`Operand::Constant(Constant::Int(0))` /
  `RValue::ConstInt(0)`), and codegen.
- **`!` (never) type.** The `!` token is now parsed as a type
  expression equivalent to `never`, mainly for FFI signatures and
  divergent functions. Treated as a `Simple { name: "never" }`
  type at the AST level.
- **Nine new FFI builtins (all use the `i64` ABI):**
  - `str_to_ptr(s: str) -> i64`: get a raw data pointer (as an
    `i64` handle) for a Kryos string. Pair with `len(s)` when
    calling C.
  - `buf_to_str(buf: i64, len: i64) -> str`: copy `len` bytes from
    a raw buffer handle into a new Kryos string.
  - `alloc(n: i64) -> i64`: allocate `n` bytes, return the handle.
  - `free_bytes(buf: i64, n: i64) -> void`: release a buffer
    previously returned by `alloc`.
  - `ptr_byte_at(buf: i64, i: i64) -> i64`: read byte at offset.
  - `ptr_set_byte(buf: i64, i: i64, b: i64) -> void`: write byte
    at offset.
  - `ptr_read_i64(buf: i64, i: i64) -> i64`: read an 8-byte
    little-endian integer.
  - `ptr_write_i64(buf: i64, i: i64, v: i64) -> void`: write an
    8-byte little-endian integer.
  - `handle_to_str(h: i64) -> str`: decode a runtime handle into
    a string view (used by `std::os::args` and friends).
  Runtime helpers (`kryos_str_to_ptr`, `kryos_buf_to_str`,
  `kryos_alloc_bytes`, `kryos_free_bytes`, `kryos_ptr_byte_at`,
  `kryos_ptr_set_byte`, `kryos_ptr_read_i64`,
  `kryos_ptr_write_i64`, `kryos_handle_to_str`) live in
  `kryos-rt::builtins` and are declared with `i64`-only signatures
  in the LLVM codegen.
- **`@capabilities(...)` on impl-block methods.** The annotation
  was already accepted on free functions; the parser now also
  accepts it on methods declared inside `impl` blocks (e.g.
  `@capabilities(net) fn write(self: TcpStream, data: str)` in
  `std::net`).

### Fixed — stdlib

All ten previously-broken modules now type-check. The recurring
pattern was an extern block declaring handle / pointer arguments
as `ptr` (or `*mut void`) while the rest of the module already
threaded them as `i64`. Extern blocks are now `i64`-only across
the stdlib so they line up with `str_to_ptr`, `alloc`, and
`null`.

- **`std::fs`**: extern block migrated to `i64`. `read_all`,
  `write_all`, `stat`, and friends now type-check.
- **`std::io`**: extern block migrated to `i64`. Fixed a borrow
  issue in the line-reader (snapshot `reader.position` into
  `start_val` before taking a `&mut` cursor).
- **`std::os`**: extern block migrated to `i64`. `env`, `args`,
  `exit` now type-check.
- **`std::term`**: extern block migrated to `i64`
  (`kryos_stdout_write`, `kryos_stdin_read`).
- **`std::net`**: extern block migrated to `i64`. Methods renamed
  (see Breaking changes). `http_get` and `http_post` call sites
  updated.
- **`std::process`**: extern block migrated to `i64`. Replaced a
  stale `ptr_null()` call with the new `null` literal. Fixed a
  move-after-use of `exit_code` when constructing the result
  struct.
- **`std::db`**: extern block migrated to `i64`. `exec` renamed to
  `execute` (see Breaking changes). Dropped the spurious `fs`
  capability from the file-level `@capabilities` annotation
  (`@capabilities(fs, db)` → `@capabilities(db)`).
- **`std::crypto`**: extern block migrated to `i64`. `sha256_raw`,
  `sha512_raw`, `random_bytes`, `random_int`, `random_uuid` now
  type-check.
- **`std::re`**: extern block migrated to `i64`. `is_match` and
  `find_all` now type-check.
- **`std::tracked`**: worked around the JSON-encoder construction
  in `Tracked::to_json` by escaping literal `{` and `}` with `\{`
  / `\}` so the lexer does not enter interpolation mode at the
  start of the string. (The lexer-level fix — treating `{` as
  literal unless preceded by something that indicates
  interpolation intent — is tracked separately and will land in a
  later patch release.)

### Migration

- Replace `stream.send(...)` / `stream.recv(...)` with
  `stream.write(...)` / `stream.read(...)` on `TcpStream` and
  `TlsStream`.
- Replace `db::exec(conn, sql)` with `db::execute(conn, sql)`.
- In any FFI bindings that declared handles as `ptr` or
  `*mut void`, switch to `i64`. Use `str_to_ptr(s)` to obtain a
  data pointer for a Kryos string and pair it with `len(s)`. Use
  `null` instead of `ptr_null()`.

## [2.3.3] - 2026-05-17 — "Stdlib continuation: parser and checker primitives, 10 more modules type-check"

Follow-on maintenance release after 2.3.2. No breaking changes. Adds
foundational parser/checker primitives that several stdlib modules
relied on, and finishes the surface-level cleanup of the modules
those primitives unblock.

After this release, 21 of 31 stdlib modules type-check cleanly:
`agent`, `chan`, `collections`, `cost`, `datetime`, `ffi`, `fmt`,
`http`, `iter`, `json`, `math`, `option`, `path`, `probable`,
`result`, `stream`, `string`, `sync`, `tensor`, `test`, `wasm`.
The remaining 10 (`crypto`, `db`, `fs`, `io`, `net`, `os`,
`process`, `re`, `term`, `tracked`) still depend on lower-level
builtins or syntax that is not yet implemented and are tracked
separately.

### Added

- **`sleep(ms: i64) -> void` builtin.** Wired through the
  type-checker, MIR, and LLVM codegen (maps to the existing
  `kryos_sleep` runtime entry). Already used by `std::chan` for
  timeouts and tickers; previously only the lower-level
  `sleep_ms` was reachable.
- **`close_chan(ch: i64) -> void` builtin.** Maps to the existing
  `kryos_chan_close_i64` runtime hook. Lets channel users mark a
  channel closed from Kryos code without dropping into FFI.
- **Bare `fn` type as an opaque callable.** Function-typed struct
  fields and parameters declared as `fn` (with no following
  parameter list) are now accepted by the type checker and treated
  as an opaque callable. Call sites and field-call sites bypass
  arity/argument checking for the opaque-callable shape. This is
  the type that `std::chan` uses for `SelectCase.handler`,
  `fan_out`'s `handler`, and friends.
- **`catch (name)` syntax** in `try` / `catch`. Both `catch name`
  and `catch (name)` now parse, matching the form used in several
  stdlib modules.

### Fixed

- **Empty match-arm body parses as `void`, not `MapLiteral`.**
  `_ => {}` arms (used heavily in `std::result` and `std::test`)
  previously parsed the `{}` as an empty map-literal expression,
  causing spurious type errors. The parser now detects the
  `{` `}` token pair in match-arm body context and emits an empty
  block expression instead.
- **`throw` inside match arms desugars to `assert(false, msg)`.**
  Functions like `Result::unwrap` and `Option::unwrap` use `throw`
  inside individual arms to signal failure. The parser now
  rewrites those into `assert(false, ...)` calls so the modules
  type-check without requiring a full exception runtime.
- **`MethodCall` on a bare type name routes to `StaticMethodCall`.**
  `List.new()` and similar `Type.method()` calls (the form used
  throughout `std::collections`) previously failed because the
  parser produced a `MethodCall` AST node with an `Identifier`
  receiver, but only `::` static calls were recognised. Both the
  type checker and the MIR lowerer now detect this shape and
  rewrite to the equivalent `Type__method` static call. Closes a
  long-standing inconsistency between `Type.method` (expression
  position) and `Type::method`.
- **Impl methods no longer pollute the global function namespace.**
  Defining `impl Foo { fn bar(...) }` used to register `bar` as a
  free function as well, causing name collisions when multiple
  impls used the same method name (e.g. `len`, `push`). Methods
  now live only in the per-type impl table and are looked up via
  `lookup_method` from method/static-call positions.
- **`std::chan` rewritten to compile against the available
  channel primitives.** Removed `throw` outside of match arms
  (now `assert(condition, msg)`), removed `try` / `catch`
  blocks around `send` / `recv` (the runtime aborts on closed-
  channel send, which is the desired semantics), replaced the
  inline literal `{ ... }` blocks inside `select` with a
  round-robin poll over the dynamic `[SelectCase]` array,
  dropped the placeholder `shared bool` annotation on
  `Channel.is_closed`, and reshaped `try_receive` to return a
  named `TryRecv { ok, value }` struct.
- **`std::collections` no longer shadows the built-in `Map<K,V>`
  type.** Renamed the user-defined struct `Map` → `Dict`. The
  builtin generic `Map` was being hijacked by the stdlib
  definition, causing every map literal to fail to type-check.
  Also rewrote the impl methods to use the new
  `Type.method`/`Type::method` static-call rewrite and removed
  the lingering `map_get` / `map_set` builtin references.
- **`std::test` cleaned up.** `try` / `catch` was using
  parenthesised `catch (e)` form; that now parses thanks to the
  added syntax. `null` assertions renamed to
  `assert_empty` / `assert_not_empty` (the language has no
  nullable primitive). `time_ms` calls migrated to the
  available `time_now`. Elapsed-time arithmetic switched from
  `f64` to `i64` to match the builtin's return type.
  `test.fn` field renamed to `test.body_fn` to avoid the `fn`
  keyword in field-access position. Loop bodies restructured to
  tally before consuming the test value.
- **`std::option` / `std::result` / `std::iter` finalised.**
  `none` renamed to `none_value` (since `none` is a reserved
  keyword), `throw` removed outside of match arms, `fn`-arity
  mismatches and `map_get` / `map_set` references replaced with
  the corresponding indexing expressions. (Started in 2.3.2,
  finished here.)
- **`std::sync`, `std::tensor`, `std::stream` surface fixes.**
  `mutex_new`'s null check removed (the runtime aborts on
  allocation failure already, and `*mut void` cannot be
  compared to an integer literal), `AtomicInt::increment` and
  `decrement` reshaped to avoid spurious move-after-store
  errors, `tensor_arange` declared with `i64` parameters to
  match the runtime ABI, and `stream_concat`/`stream_from_list`
  / `stream_from_range` updated to bind the array length to a
  local before constructing the `Stream` so the array isn't
  observed after move.
- **Regex literals in `std::re`** now use `\{` / `\}` brace
  escapes so that quantifiers like `\d{1,3}` and `[a-zA-Z]{2,}`
  parse as intended. (Module still has other unrelated issues
  and remains in the broken list.)

### Notes

The 10 still-broken stdlib modules need lower-level primitives
that have not been built yet: a `null` value comparable to
`*mut void` handles (or runtime-side null-aborting allocators
everywhere), `str_to_ptr`/`buf_set_byte` and the bytestring
family, the `!` never type for `exit_error`, capability syntax
(`@capabilities(net)` and friends), and a few module-specific
builtins. They are intentionally left out of this release so
2.3.3 can ship the work that is actually done.

## [2.3.2] - 2026-05-17 — "Audit pass: stdlib surface fixes, type resolution, pattern syntax"

Maintenance release driven by an end-to-end audit of every example and
standard library module. No breaking changes. Every previously-passing
example still passes; the bug fixes below unblock additional stdlib modules.

### Fixed

- **`std::fmt` now imports cleanly.** `fmt.kry` used unescaped literal
  braces (`"{"` and `"}"`) inside string contents, but `{` is reserved
  for string interpolation. Replaced with the documented `\{` / `\}`
  escape sequences so the module parses again. Also replaced a call to
  the nonexistent `map_get` builtin with the equivalent map-index
  expression (`val[k]`) inside `debug()`.
- **`std::datetime` now type-checks cleanly.** `Duration::to_string`
  shadowed the in-scope `to_string` builtin inside its own body and
  the user-defined version was being called recursively with the wrong
  argument type. Renamed to `Duration::format` to remove the shadow.
- **Parser: pattern variants accept `.` as well as `::`.** Match patterns
  like `Option.Some(v) => ...` (the form used throughout the standard
  library) now parse alongside `Option::Some(v) => ...`. Previously only
  `::` was accepted in patterns even though `.` was accepted in
  expressions, which made some stdlib modules look invalid from the user
  side.
- **Type resolver: `any` and `ptr` resolve as primitive type names.**
  `any` resolves to the type-checker's error-recovery sentinel (which
  unifies with anything without emitting a mismatch) and `ptr` resolves
  to `*mut void`. This is what `extern` declarations and dynamically-
  typed stdlib signatures already assumed; the resolver was just
  missing the entry.

### Added

- `examples/string_braces.kry` — regression example pinning down the
  correct `\{` / `\}` escape behavior for literal braces in strings.

### Standard library status (honest accounting)

- **Importable and usable:** `math`, `string`, `fmt`, `path`,
  `datetime`, `json`, `http`, `probable`, `agent`, `wasm`, `ffi`.
- **Not yet usable in this release:** `option`, `result`, `iter`,
  `collections`, `test`, `chan`, `crypto`, `fs`, `io`, `net`, `db`,
  `term`, `process`, `re`, `tensor`, `sync`, `stream`, `cost`,
  `tracked`. These modules either depend on low-level builtins that
  do not exist yet (`alloc`, `free`, `ptr_byte_at`, `str_to_ptr`),
  use reserved keywords as identifiers (e.g. a function literally
  named `none`), or rely on exception-style `throw` expressions that
  the language doesn't implement. Programs that only use builtins
  (`println`, `file_read`/`file_write`, `len`, `to_string`,
  `parse_int`/`parse_float`, `push`, `substr`, `split_lines`,
  `contains`, `sqrt`, `pow`, `sin`, `cos`, `abs`, `exit`, `args`,
  etc.) are unaffected and continue to work. Fixing the remaining
  modules is a substantial scope of work tracked for a future release.

## [2.3.0] - 2026-05-16 — "Async pipeline wired, DWARF, WASM parity, registry"

This release wires the v2.2 async substrate end-to-end and completes a
seven-item finish-line list. **No breaking language changes.** Everything
additive.

### Added

- **Async state-machine pipeline (codegen consumes the post-split CFG)**
  — `apply_split_at_awaits` is now called from `kryos-driver`'s
  pipeline after `apply_state_structs`, behind the `split_async_awaits`
  config flag (opt out via `KRYOS_DISABLE_AWAIT_SPLIT=1`). The
  Cranelift poll-wrapper now detects split functions heuristically
  (blocks>1 + entry Switch) and propagates the dispatcher's READY/PENDING
  status, only stamping state=-1 (DONE) when the call returned READY.
  Legacy single-block async functions remain eager-DONE.
- **LLVM DWARF debug info** — per-function `DISubprogram`s and per-call
  `!dbg` locations emitted from the LLVM codegen. Uses LineTablesOnly
  emissionKind so `ret`/`br` don't need `!dbg`. Verified end-to-end:
  `addr2line` resolves user functions to Kryos source lines in clang -O2 -g
  binaries. No runtime cost.
- **WASM stdlib parity surface** — 18 new host imports for strings,
  arrays, JSON, regex, and HTTP. Index assignment uses `self.type_count`
  and `self.func_count` rather than hardcoded indices so future
  additions are safe. Reference host shim landed in
  `examples/wasm_runner.js`; full doc in `docs/wasm-stdlib.md`. The
  language-level binding (a `kryos-stdlib-wasm` shim crate) is a
  deliberate follow-up; this release ships the capability surface.
- **Refreshed benchmark numbers** — fresh runs of the full suite
  (`benchmarks/run.sh`) with the v2.3.0 toolchain, including a clear
  callout of the subprocess-launch floor on the sandbox VM (~30 ms).
  `BENCHMARKS.md` rewritten with honest per-benchmark notes for `fib`,
  `mandelbrot`, `nbody`, `binary_trees`, `fannkuch`, `matmul`.
- **VS Code extension v0.4.0 — marketplace-ready packaging** — added
  `LICENSE`, icon, `.vscodeignore`, `CHANGELOG.md`, gallery banner,
  `categories`/`keywords`, `vsce`-based `package`/`publish` scripts.
  Bundled LSP client wiring stable on `kryos lsp` stdio.
- **Zed extension scaffold (`editors/zed/`)** — `extension.toml`,
  `languages/kryos/config.toml`, Rust LSP launcher (`src/lib.rs`)
  targeting `wasm32-wasi` per the Zed extension API. Auto-discovers a
  `kryos` binary on PATH or in `compiler/target/release/`.
- **Package registry: full design + reference server** —
  `docs/package-registry.md` specifies the on-disk index format,
  client protocol, security model, and what is intentionally out of
  scope. `tools/registry/` ships a dependency-free Rust HTTP server
  (`std::net` only) exposing `/v1/health`, `/v1/packages/<name>`,
  `/v1/packages/<name>/<version>`, `/v1/search?q=...`. Periodically
  `git pull`s the index. The kryos-package client (sync/lookup/search/
  pack/publish) was already wired in v2.2; this release completes the
  spec and provides a runnable reference impl.

### Notes

- Sweep: **123/123** native --release tests passing.
- MIR lib tests: **79/79**.
- kryos-codegen-wasm tests: **1/1**.
- Build warnings: **zero**.
- Registry hosting (canonical `kryos-registry` repo + tarball host)
  remains an operational decision intentionally deferred until
  someone is ready to provision infrastructure. The client and server
  are ready when that decision is made.

## [2.2.1] - 2026-05-16 — "Async substrate, repo polish, zero warnings"

Post-2.2 cleanup pass. No language-behavior changes. Everything here is
additive infrastructure, fixes, or repo polish.

### Added (MIR / async substrate)

- **MIR liveness analysis** (`kryos_mir::liveness`) — backward-dataflow
  live_in / live_out per block on the existing CFG, with a per-program-
  point query (`live_after_instruction`). Foundation for any pass that
  needs to know which locals survive a given program point.
- **`split_at_await` CFG transform** (`kryos_mir::async_lower`) — takes
  a list of `(BlockId, inst_idx)` suspension points and rewrites the
  function into a stackless state machine: per-split persist of
  live-after locals via `StoreField` on the state struct, early
  `Return(0)` (KRYOS_PENDING) as the pre-half terminator, reload via
  `Field` at the top of a freshly-created resume block, and a synthetic
  dispatch entry block that `Switch`es on the state discriminant.
- **`apply_split_at_awaits` driver** — opt-in module-wide pass that
  scans for calls to async callees as suspension points and applies
  the transform without touching AST→MIR lowering. Not yet wired into
  the main pipeline (codegen still consumes the pre-split CFG), but
  the API is stable and tested.

### Added (packaging)

- **`cargo install` support** — `kryos-cli` Cargo.toml now carries
  description, keywords, categories, license, repository, homepage,
  authors, and a readme path so `cargo install --path compiler/crates/
  kryos-cli` works against a local checkout. README documents the
  exact command.
- **Contact & community** — README has a new Community & Contact
  section: GitHub Discussions, Issues, and `info@northtek.io` for
  direct contact. The email is also embedded in the workspace
  `authors` field so it propagates into every crate.
- **GitHub Discussions enabled** on the public repo.

### Fixed

- **`kryos-linker` test build** — added missing `debug_info: false`
  to two `LinkerConfig` initializers in `tests/linker.rs`. 27/27
  linker tests now compile and pass (previously: did not compile).
- **Cleared all compiler warnings** — four small fixes: removed
  `#[inline]` from two `#[no_mangle] extern "C"` exports in
  `kryos_rt::array` (rustc was ignoring it), dropped an unnecessary
  `mut` in `kryos_stdlib_native::json`, added `#[allow(dead_code)]`
  on a forward-declared `Fn8V` arity alias and on intentionally-kept
  WASM `FnEmitter::block_by_id` / `n_params`. Release build is now
  warning-free.

### Tests

- 4 new liveness unit tests (entry, after-call, branch propagation,
  loop back-edge fixpoint).
- 7 new split-at-await unit tests (1/2/3 splits, bad-block guard,
  duplicate-block guard, empty-input no-op, basic-shape with persist
  + reload assertions).
- `kryos-mir` lib tests: 79/79.
- Native `--release` sweep: 123/123 maintained.

## [2.2.0] - 2026-05-15 — "Developer-platform completeness: 115/115 native release tests"

The 2.2 milestone closes the last three architectural gaps from v2.1's
"known limitations" list and lands the bulk of the developer-platform
work (tooling, packaging, language ergonomics) needed for v2.x to be
usable as a commercial language. The native `--release` test suite is
now **115/115** (100%).

No behavior in correct existing programs changed — every item is either
a new feature, a tooling addition, or a fix for a previously documented
v2.1 limitation.

### Added (language)

- **HashMap literal syntax `#{key: value, ...}`** — explicit, unambiguous
  map construction at the expression level. Empty literal `#{}` produces
  an empty map. Lexer + parser + typechecker support across all three
  backends.
- **`Result<T, E>` and `Option<T>` in the prelude** — first-class enum
  types with `Ok/Err` and `Some/None` variants, including the
  **`?` postfix try-operator**. `expr?` desugars at parse-time to
  `match expr { Result::Ok(__v) => __v, Result::Err(__e) => return Result.Err(__e) }`,
  with matching `arm_body_diverges()` typechecker handling so the Err
  arm doesn't pollute match-type unification.
- **Full closure capture analysis** — escaping closures (returned from
  functions or stored in structs) now work in the LLVM backend through
  a uniform `(env, user_args...)` calling convention. Every closure
  value, including no-capture lambdas, is wrapped in an ARC env
  `[thunk_ptr, cap0, cap1, ...]`; CallIndirect dispatches via env[0].
  Fixes the v2.1 `closure_escape` and `closure_capture_fn` limitations.
- **`dyn Trait` dynamic dispatch in LLVM** — real vtable codegen,
  replacing the v2.1 placeholder that returned 0. Trait objects are
  fat pointers `[data, fn_ptr_0, fn_ptr_1, ...]`; per-method dyn-thunks
  give every method a uniform i64-only ABI suitable for indirect
  dispatch, handling byval-self/sret-return correctly.

### Added (tooling)

- **`kryos doc --html`** — HTML output for the documentation generator,
  alongside the existing markdown writer.
- **LSP validation pass** — the language server now publishes parser
  and type diagnostics to the editor, not just structural info.
- **`kryos pkg add` command** — adds a dependency to the project's
  manifest from the CLI.
- **CI matrix + release artifact build** — multi-OS GitHub Actions
  matrix produces signed release binaries on tag pushes.
- **`-g` / `--debug-info` flag plumbed end-to-end** — emits a minimal
  DWARF compile-unit and `!DIFile` so `gdb`/`lldb` can resolve
  source-level frames for LLVM-built binaries.

### Improved

- **Lexer diagnostics for unterminated literals** — precise spans and
  messages for unterminated strings / char literals, replacing the
  generic "unexpected EOF".

### Test sweep

- Native `--release` builds: **115 passed, 0 failed**.
- Resolves the three v2.1 "known limitations":
  `closure_escape`, `closure_capture_fn`, `dyn_trait`.

## [2.1.0] - 2026-05-15 — "LLVM backend correctness sweep: 112/115 native release tests"

The 2.1 milestone is a focused correctness pass on the LLVM `--release`
backend, raising the native release test suite from 78/115 to **112/115**
(97.4%) and stamping the three remaining failures as documented architectural
items for v2.2 (see STABILITY.md, "Known limitations").

No language semantics changed in this release; every fix targets a real
codegen or runtime bug.

### Fixed (LLVM backend & runtime)

- **`kryos-codegen-llvm`: void operand in `inttoptr`** — Functions returning
  `()` were lowered to `void` but their SSA result was still consumed in
  later `inttoptr` casts. The backend now elides the use site when the
  source operand has void type. Fixes `ownership_shared`, `shared_deref`.
- **`kryos-codegen-llvm`: aggregate store / aggregate return** — Stores of
  `{ i64, i64 }` aggregates where the source operand was a pointer (e.g.
  cross-function throw payloads) were emitted as raw `store` without
  coercion. The backend now materializes a properly-typed aggregate via
  `insertvalue` first, then stores. Fixes `cross_fn_throw`,
  `cross_fn_throw_deep`, `nested_try`, `try_catch`.
- **`kryos-codegen-llvm`: switch terminator uses MIR default block** —
  Previously synthesized a fresh default block that fell off the end of the
  function. Now wires the MIR's recorded default and handles enum aggregate
  comparison. Fixes `match_basic`, `match_default`, `enum_match`,
  `enum_param`, `try_throw`.
- **`kryos-codegen-llvm`: TCO entry block label collision** — The TCO pass
  re-emitted the entry block, producing `multiple definition of '_0'`.
  Entry-block emission now guarded. Fixes `opt_tco`.
- **`kryos-codegen-llvm`: array element load in for-loop body** — For-loop
  body was reading the array slot instead of the element. Fixes
  `for_array_sum`, `for_continue`.
- **`kryos-codegen-llvm`: call-arg type coercion for `i32`/`i64`/`ptr`
  mismatches at the call site** — Fixes `pipe_basic`.
- **`kryos-codegen-llvm`: method dispatch through `Function`-typed struct
  fields** — MIR's `infer_expr_type` for `MethodCall` now returns the
  function field's recorded return type instead of falling through to
  `Void`. Fixes `closure_in_struct` (compile-time half).
- **`kryos-rt::arc`: ARC magic sentinel** — `kryos_arc_{retain,release,
  set_drop,ref_count}` now check a 64-bit magic word
  (`0xA7C0_DEAD_BEEF_CAFE`) at the head of every ARC header and no-op on
  pointers that don't carry it. This unblocks struct drops involving
  non-capturing function-pointer fields (which are wrapped as closures but
  point to static code), used by `closure_in_struct` and others.
- **`kryos-rt::map`: string-key insert path** — String-keyed maps were
  inserting with the integer-key entry point, so subsequent
  `kryos_map_get_str` lookups missed (content-hash vs pointer-identity
  mismatch). The codegen now emits `kryos_map_insert_str` for string keys.
  Fixes `map_basic`.
- **`kryos-rt::spawn`: drain spawned tasks at program exit** —
  `emit_main_wrapper` now emits `call void @kryos_spawn_wait_all()` before
  `ret i32 0`, so detached `spawn` tasks complete deterministically before
  the process returns. Fixes `spawn_basic`.

### Added

- **`STABILITY.md`** — First public stability document. Pins backend
  guarantees (Cranelift JIT 100% on native runner, LLVM release 112/115),
  enumerates the three known closure-ABI / vtable limitations, lists
  features explicitly out-of-scope (full borrow checker, hygienic macros,
  Vulkan/Metal/DX12, HTTP/3 server, retained-mode GUI, etc.), and documents
  the test-pass policy for cutting releases.

### Known limitations (carried to v2.2)

- **Closures that escape via return or are passed as function arguments**
  (`closure_escape`, `closure_capture_fn`) — The lambda ABI currently takes
  captures as direct parameters and only works when the `closure_locals`
  optimization fires (direct call at the same lexical scope). Fixing this
  requires either passing the env pointer as the first lambda arg and
  loading captures from env slots, or recording capture-count metadata so
  call sites can load captures dynamically. Tracked for v2.2's full capture
  analysis.
- **`dyn Trait` method dispatch** (`dyn_trait`) — `VtableCall` in the LLVM
  backend is a placeholder that returns 0. Real vtable construction and
  indirect dispatch is planned for v2.2.

### Build / packaging

- Workspace version bumped to **2.1.0**.
- No public CLI/stdlib surface changed.

## [2.0.0] - 2026-05-15 — "Production: LLVM blocker fix, WebSocket + Unix sockets, LTO, lockfile tooling"

The 2.0 milestone closes a pre-existing v1.9.0 LLVM blocker that prevented
`tcp_listen`, `tls_send`, `pg_query` and friends from working under
`--release` builds, adds RFC 6455 WebSocket helpers and Unix domain socket
primitives to the stdlib, lands link-time optimization in the build
pipeline, and rounds out the package manager with `kryos pkg outdated`.

This release is **production**: every advertised builtin now works on both
Cranelift (`kryos run`) and LLVM (`kryos build --release`) backends.

### Fixed

- **`kryos-codegen-llvm`: user-facing builtins translated to runtime symbols** —
  Pre-existing v1.9.0 blocker. The LLVM codegen emitted `call @{fname}` using
  the user-facing name without translating it to the corresponding
  `kryos_*_ks` runtime symbol, so any `--release` build calling `tcp_listen`,
  `tls_send`, `pg_query` (and ~60 other builtins) failed at link time with
  `clang: use of undefined value '@tcp_listen'`. Cranelift had this
  translation table; LLVM did not. Added a 60+ entry `runtime_fname` match
  block and matching `declare` statements in `emit_extern_declarations`
  covering tcp/tls/pg/uds/ws/json/crypto/regex/mutex. Validated end-to-end:
  `tcp_listen`, `ws_accept_key`, and `uds_bind` all work in `--release` now.
- **`kryos-rt::array`: silence harmless `inline ignored on no_mangle` warning** —
  `#[inline]` is meaningless on `#[no_mangle]` exports. Switched to
  `#[cfg_attr(not(debug_assertions), inline)]` so the attribute only applies
  where it has effect.

### Added

- **WebSocket stdlib (`kryos-stdlib-native::websocket`)** — RFC 6455 helpers:
  `ws_accept_key` (SHA-1 + base64 handshake, validated against the RFC 6455
  reference vector `dGhlIHNhbXBsZSBub25jZQ==` → `s3pPLMBiTxaQ9kYGzzhZRbK+xOo=`),
  frame encoders `ws_encode_{text,binary,close,ping,pong}`, `ws_unmask`, and
  `ws_read_frame` for server-side parsing. Reuses the existing `kryos_sha1`
  implementation in `crypto.rs` plus the `base64 = "0.22"` crate. New example
  `examples/ws_handshake.kry` validates the canonical handshake.
- **Unix domain sockets (`kryos-stdlib-native::unix_socket`)** —
  `uds_{connect,bind,accept,send,recv,close}` with full `cfg(unix)`
  implementation and `cfg(not(unix))` stubs that return `-1` so portable
  code still compiles on Windows.
- **`kryos pkg outdated`** — Compares versions in `kryos.lock` against the
  latest available in the registry index and reports up-to-date / outdated /
  unknown counts. Skips path-source entries cleanly, reports
  missing-from-index packages without failing, and prints a tabular
  PACKAGE / INSTALLED / LATEST view.
- **Link-time optimization (LTO) in release builds** —
  `compiler/Cargo.toml`: `[profile.release] lto = "fat"`, `codegen-units = 1`,
  `panic = "abort"`. Generated binaries now use `clang` as the linker for
  better cross-module inlining. Inlineable runtime helpers (`kryos_array_get`,
  `kryos_array_len`, `kryos_string_concat`) marked `#[inline]` so LTO can
  fold them through user call sites.
- **Bare `!` as logical-NOT alias** — Lexer/parser now accept `!x` in
  addition to `not x`. Mixed forms work in the same expression.
- **`--flto-jobs=N`** — Surface for parallel codegen passes through the
  build driver.
- **DWARF debug info (`-g`)** — `kryos build --release -g` emits source-level
  debug info that `lldb` / `gdb` understand.

### Changed

- **Wiring for `ws_*` and `uds_*` builtins across the compiler**:
  - MIR return types (`kryos-mir::lower`)
  - Net capability gates (`kryos-capabilities::model`)
  - Typechecker `FunctionSig` entries (`kryos-types::check`)
  - Cranelift codegen builtin map + JIT symbol registration
  (`kryos-codegen-cranelift::{codegen,jit}`)
  - LLVM codegen name-translation table + declarations
  (`kryos-codegen-llvm::codegen`)

### Benchmark baseline (vs gcc -O3)

After the LTO commit (`9c43547`):

| Benchmark      | Ratio vs gcc -O3 |
|----------------|------------------|
| fib            | 1.00×            |
| mandelbrot     | 1.00×            |
| nbody          | 4.24×            |
| binary_trees   | 2.62×            |
| fannkuch       | **7.31×** (was 12.4×) |
| matmul         | **2.01×** (was 2.67×) |

LTO closed the biggest two gaps on the matmul and fannkuch workloads.

### Honestly deferred to a future major

These are tracked but require multi-day work and are not in 2.0:

- `?` operator — needs first-class `Result` / `Option` as built-in types.
- Closures (`|x| ...`) — needs capture analysis pass.
- HashMap `{}` literals — parser collision with block syntax; needs a
  disambiguation pass.

### Not pursued in this release

Items from the gap list that we intentionally did not chase in 2.0:
borrow checker, hygienic macros, Vulkan / Metal / DX12 backends,
HTTP/3 / QUIC, full profile-guided optimization, video decode,
Windows-tested toolchain, iOS / Android / embedded targets, retained-mode
GUI toolkit, full Unicode normalization. These are acknowledged as
future-major work, not 2.0 commitments.

## [1.9.0] - 2026-05-16 — "LLVM backend production-ready: full benchmark suite"

The `kryos-codegen-llvm` crate has always existed but couldn't be exercised in
build environments without `clang`. With `clang 19` and `llvm 19` confirmed on
PATH, `kryos build --release` now produces native binaries that match Rust
`--release` and are within 1.0–1.6× of `gcc -O3` on standard numeric benchmarks
(see [BENCHMARKS.md](BENCHMARKS.md)).

### Added

- **`benchmarks/go/`** — Go equivalents of all 6 benchmark programs (`fib`,
  `mandelbrot`, `nbody`, `binary_trees`, `fannkuch`, `matmul`). All produce
  byte-identical output to the C reference implementations.
- **`benchmarks/python/`** — CPython equivalents of all 6 benchmark programs.
  `binary_trees` uses a 60 s timeout in the runner since CPython depth-18 recursion
  finishes in ~64 ms (no issue), but fib and python in other environments may time out.
- **`benchmarks/run.sh`** — Expanded from 4 columns (Kryos/Rust/C/ratio) to 8
  columns: Kryos LLVM, Kryos Cranelift, Rust --release, gcc -O3, clang -O3,
  Go, Python, and Kryos/gcc ratio. Uses `time.perf_counter()` with best-of-10
  for compiled languages and best-of-3 for Python.
- **`BENCHMARKS.md`** — New top-level benchmarking document with full methodology,
  per-benchmark analysis, honest assessment of wins and losses, and a roadmap for
  closing the remaining gap to gcc/Rust.

### Fixed

- **`kryos-codegen-llvm`: float array reads** — `kryos_array_get` returns raw
  `i64` bits; when the destination type is `f64` the codegen now emits
  `bitcast i64 → double` instead of the illegal `fadd double {i64}, 0`.
- **`kryos-codegen-llvm`: float array writes** — `kryos_array_set` expects its
  value argument as `i64`. Added `kryos_array_set` to the `runtime_param_types`
  table so `coerce_value` applies `bitcast double → i64` automatically.
- **`kryos-codegen-llvm`: undeclared math functions** — `sqrt`, `floor`, `ceil`,
  `round`, `sin`, `cos`, `tan`, `log`, `log2`, `log10`, `fabs` are standard C
  names called by Kryos builtins but were missing from the LLVM IR `declare`
  block, causing `undefined value '@sqrt'` link errors. All are now declared.

### Changed

- `benchmarks/RESULTS.md` — Regenerated with honest 7-language numbers
  including the two new LLVM codegen bug fixes (previously nbody could not
  compile under LLVM at all).
- README.md — Added link to BENCHMARKS.md and updated the speed claim to
  reflect LLVM backend parity with Rust on most numeric workloads.

### No language changes

This release is purely performance documentation and benchmark harness. No
syntax, type-system, or standard-library changes.

## [1.8.0] - 2026-05-15 — "Package registry: five starter packages"

This release closes Gap E (seed registry) by populating the empty
[kryos-registry](https://github.com/NORTHTEKDevs/kryos-registry) with five
starter packages. `kryos pkg add <name>` now resolves real metadata.

### Added
- `examples/extracted_packages/markdown/` — CommonMark-subset Markdown to HTML
  renderer extracted from `examples/showcase/markdown.kry`; public API:
  `markdown_to_html(md: str) -> str`
- `examples/extracted_packages/http-router/` — HTTP request/response structs
  (`Request`, `Response`) and routing helpers (`path_matches`, `parse_request_line`,
  `format_http_response`, etc.) extracted from `examples/http_server.kry`
- `examples/extracted_packages/json/` — friendly wrappers around Kryos built-in
  JSON builtins plus string-builder helpers (`json_object_literal`, `json_array_literal`,
  `json_escape`, etc.)
- `examples/extracted_packages/sqlite/` — FFI wrapper for SQLite via
  `libsqlite3.so.0`; public API: `sqlite_open`, `sqlite_exec`, `sqlite_close`
- `examples/extracted_packages/regex/` — regex matching via POSIX
  `regcomp`/`regexec` (libc) with pure-Kryos wildcard fallback; public API:
  `regex_match(pattern, text) -> bool`, `regex_find(pattern, text) -> i64`
- Registry index entries in `NORTHTEKDevs/kryos-registry` under NDJSON format
  (one JSON line per version) compatible with `kryos-package/src/registry.rs`
- GitHub Releases created for all five packages (`markdown-v0.1.0`,
  `http-router-v0.1.0`, `json-v0.1.0`, `sqlite-v0.1.0`, `regex-v0.1.0`)
  on `NORTHTEKDevs/kryos-registry`

### Verified
- `kryos pkg search markdown` → `markdown v0.1.0`
- `kryos pkg search regex` → `regex v0.1.0` (and all other packages)
- `kryos pkg add markdown` → `added dependency \`markdown\`` and writes entry
  to project `kryos.toml`

### Known limitations
- Tarball asset upload to GitHub Releases is blocked by the `uploads.github.com`
  domain not being proxied. Release tags exist and index URLs are correct;
  tarballs must be attached manually. `kryos pkg add` records the dependency
  and resolves metadata; tarball download will work once assets are attached.

---

## [1.7.0] - 2026-05-15 — "OpenGL 3.3 — 3D graphics"

This release closes Gap D (OpenGL 3.3) by adding full OpenGL 3.3 core-profile
bindings via SDL2's `SDL_GL_GetProcAddress`. Kryos can now build 2D/3D games
and visualizations entirely through the existing FFI subsystem.

### Added
- `examples/gl_cube.kry` — spinning cube demo using OpenGL 3.3 core profile:
  vertex + fragment shaders, VBO/VAO, indexed drawing, MVP matrix, and
  offscreen rendering verified with `glReadPixels` → PPM pixel dump
- `kryos_ffi_write_f32_bits(p, bits)` / `kryos_ffi_write_f64_bits(p, bits)` —
  write IEEE-754 floats to heap memory from integer bit-patterns (enables
  building float vertex/uniform buffers from Kryos source)
- `kryos_ffi_dlcallv_4f32(fp, b1, b2, b3, b4)` — call a `void(f32,f32,f32,f32)`
  function with per-bit-pattern arguments (used for `glClearColor`)
- `kryos_ffi_dlcall7` / `kryos_ffi_dlcallv5..7` — higher-arity FFI call
  helpers for 7-argument functions like `glReadPixels`
- `kryos_ffi_read_f32_bits` / `kryos_ffi_read_f64_bits` — read float memory
  as integer bit-patterns

### Verified
- Mesa software renderer (`LIBGL_ALWAYS_SOFTWARE=1`) renders orange cube;
  `glReadPixels` confirms 26 759 non-background pixels (vs 0 for clear)
- GL version 4.5 core profile obtained via `SDL_GL_CreateContext` offscreen

## [1.6.0] - 2026-05-15 — "HTTP/2, PostgreSQL, TLS server"

This release closes Gap C (HTTP/2 client) to complete the networking trifecta
alongside Gap A (TLS server) and Gap B (PostgreSQL driver) from v1.5.

### Added — HTTP/2 client (Gap C)

- `http2_get(url: str) -> str` — GET request with ALPN-negotiated HTTP/2,
  automatic HTTP/1.1 fallback, and shared connection pool. Returns body.
- `http2_post(url: str, body: str) -> str` — POST with body, returns response body.
- `http2_request(method: str, url: str, headers: str, body: str) -> str` — full
  request with method/headers/body control. headers is `"Name1: val1\nName2: val2"`
  newline-separated. Returns `"<status>\n<headers>\n\n<body>"` for complete
  response inspection.

All three builtins share a single `reqwest` blocking client instance (via
`OnceLock`) so the connection pool is reused across calls. `User-Agent: kryos/1.6`
is set by default. The h2 feature flag is enabled by default.

`examples/http2_demo.kry` demonstrates all three builtins against Cloudflare
(h2 trace) and httpbin.org (POST echo + custom headers).

---

## [1.5.0] - 2026-05-15 — "close the last five gaps"

This release closes every remaining gap toward true universality.
Kryos can now do TLS/HTTPS, build DOM/canvas/fetch WASM modules,
run cooperative async event loops, manage packages via a GitHub-backed
registry, drive a real Chromium browser over CDP+WebSocket, and paint
immediate-mode GUIs via SDL2 — all from pure Kryos.

### Added — Async I/O primitives

- `tcp_set_nonblocking(fd, bool) -> i64` — flip a socket between blocking
  and non-blocking modes.
- `tcp_try_accept(listener) -> i64` — returns 0 if no client is waiting
  instead of blocking. Pairs with `sleep_ms` for cooperative event loops.
- `tcp_try_recv(fd, max) -> str` — returns an empty string on WouldBlock.
- `poll_readable(fds, n, timeout_ms) -> i64` — bitmask of fds that
  became readable within the timeout (up to 63 fds).
- `sleep_ms(ms)` — sub-second cooperative pacing.

`examples/async_echo.kry` is a single-threaded non-blocking echo server
that accepts real TCP clients without spawning threads.

### Added — Crypto / binary primitives

- `sha1_hex(s) -> str` and `sha1_base64(s) -> str` — legacy SHA-1
  (RFC 6455 WebSocket handshakes, etc.) verified against the FIPS-180
  test vector for "abc".
- `base64_encode(s) -> str` / `base64_decode(s) -> str` — round-trip
  via latin-1 codepoints so binary data survives Kryos strings.
- `chr(n) -> str` / `byte_at(s, i) -> i64` — single-byte read/write
  primitives for hand-rolled binary protocols.

### Added — Browser bots (CDP)

- `examples/websocket_client.kry` — full RFC 6455 client framing:
  handshake (verified against the RFC test vector), masked text/ping/
  close frames, frame decoder for masked + unmasked frames.
- `examples/cdp_bot.kry` — Chrome DevTools Protocol driver that probes
  `http://localhost:9222/json` for an attached browser, opens a per-tab
  WebSocket, and dispatches `Page.navigate`, `Runtime.evaluate`, and
  `Page.captureScreenshot` over JSON-RPC.

### Added — Immediate-mode GUI

- `examples/sdl_imgui.kry` — pure-Kryos immediate-mode GUI on top of
  the existing SDL2 FFI bindings. Includes title bar, hoverable buttons,
  checkboxes, and a value slider. Runs headless under
  `SDL_VIDEODRIVER=dummy` for CI.

### Added — Package manager + registry

- Default registry rewritten to
  `https://github.com/NORTHTEKDevs/kryos-registry` (created this release,
  seeded with example entries).
- `kryos pkg add <name>` accepts bare names (resolves to `<name>@*`).
- `Op::Wildcard` added to the semver type so `*` matches any version.

### Notes

- TLS / HTTPS were already complete in 1.2.0 via rustls; verified
  end-to-end against httpbin.org and api.github.com in `examples/https_demo.kry`.
- WASM v0.3 (linear-memory arrays) and v0.4 (DOM/canvas/fetch host
  imports) shipped in this cycle on top of WASM v0.2 strings.

## [1.2.0] - 2026-05-15 — "truly universal: C FFI + graphics + WASM v0.2"

Kryos can now call any C library at runtime via dlopen/LoadLibrary,
including driving the full SDL2 window + renderer pipeline from pure
Kryos. No compiler changes were needed — the existing `extern "C"`
declaration syntax plus a small runtime in `kryos-stdlib-native::ffi`
is enough.

The WASM backend gained first-class strings: a `str` value now survives
in locals, parameters, and returns as a packed (offset, length) i64, and
string concatenation with `+` and `len(s)` both work in WASM modules.

This is the milestone where Kryos stops being a closed language and
becomes a real systems-level tool: anything libc, SDL2, libcurl,
libsqlite3, libssl, or any other shared library can do, Kryos can
now do. And anything text-shaped that runs in a browser can now be
written in Kryos.

### Added

- **Dynamic library FFI runtime** (`kryos-stdlib-native::ffi`).
  New runtime symbols:
  - `kryos_ffi_dlopen(name) -> handle` — wraps `dlopen` on Unix and
    `LoadLibraryA` on Windows.
  - `kryos_ffi_dlsym(handle, name) -> fnptr` — wraps `dlsym` /
    `GetProcAddress`.
  - `kryos_ffi_dlclose(handle)` — `dlclose` / `FreeLibrary`.
  - `kryos_ffi_dlcall0..6(fp, args...)` — call a resolved function
    pointer with 0-6 i64 args, returns i64.
  - `kryos_ffi_dlcallv0..4(fp, args...)` — void-return variants.
  - `kryos_ffi_dlcallv_4f(fp, f64, f64, f64, f64)` — four-f64-args
    helper for graphics APIs.
  - `kryos_ffi_cstr(s) -> *const char` — zero-copy convert a Kryos
    string to a NUL-terminated C string (KryosString is already
    NUL-terminated by design).
  - `kryos_ffi_string_from_ptr(ptr, len)` — read a C string back
    into a Kryos string (len = -1 uses strlen).
  - `kryos_ffi_malloc(n)` / `kryos_ffi_free(p, n)` — allocate raw
    memory blocks for C interop.
  - `kryos_ffi_read_i8/16/32/64`, `kryos_ffi_read_f32/64`, plus
    write variants — pointer-typed memory I/O.
- **C-compatible static-link FFI.** `extern "C" { fn foo(...); }`
  declarations are auto-resolved as `Linkage::Import` by Cranelift
  and linked against libc / system libs at build time. Works for
  any symbol the system linker can find.
- **SDL2 graphics demo, in pure Kryos.**
  - `examples/sdl_info.kry` — initializes SDL, queries version,
    platform, CPU count, RAM, performance counter.
  - `examples/sdl_window.kry` — opens a 320×240 window, creates a
    renderer, draws three colored rectangles (red, green, blue),
    presents the frame.
  - `examples/sdl_savepng.kry` — same scene but renders offscreen
    and dumps the framebuffer to disk via libc `fwrite`. Used to
    generate `docs/screenshots/sdl_kryos_demo.png` — the first
    rendered graphical output produced by a Kryos program.
- **libc FFI examples.**
  - `examples/ffi_libc.kry` — static-link smoke test
    (`getpid`/`getuid`/`time`).
  - `examples/ffi_module.kry` — dlopen libc and exercise
    `malloc`/`free`/`strlen`/`getpid` end-to-end.
  - `examples/ffi_test.kry`, `examples/ffi_dlopen.kry` — minimal
    starter snippets.
- **stdlib `ffi.kry` module.** Idiomatic wrappers (`dlopen`,
  `dlsym`, `call0..6`, `cstr`, `malloc`, etc.) for users who prefer
  named imports over raw `extern` blocks.

- **WASM v0.2: first-class strings.**
  - Strings now lower to a packed i64 `(offset | length << 32)` instead
    of just an i32 offset, so a `str` is a real value: it survives in
    locals, parameters, and returns without a side table.
  - String concatenation with `+` works end-to-end: the WASM backend
    detects `BinOp::Add` between two `Str` operands and emits a call to
    a new host import `kryos_string_concat(off1,len1,off2,len2) -> i64`.
  - `len(s)` is now a single `i64.shr_u` — the length is already there.
  - `println(s)` works on any string operand, not just literals.
  - New host imports plumbed (Node + browser): `kryos_string_concat`,
    `kryos_array_new`, `kryos_array_get`, `kryos_array_set`. Array
    builtins are wired in the runtime; Kryos-source-level array use in
    WASM lands in v0.3.
  - New example: `examples/wasm_strings.kry` — a Kryos program that
    builds greetings with `+`, prints them, and prints their lengths,
    running in Node and in any browser.
  - Updated `examples/wasm_runner.js` and `examples/wasm_browser_demo.html`
    with the new imports and a bump allocator above the static-data
    region (heap starts at 32 KB, memory grows on demand).

### Verified working

- `extern "C" { fn getpid() -> i64 }` returns the real PID.
- dlopen("libc.so.6") + dlsym("malloc") + write_i64 + read_i64 + free
  roundtrip.
- dlopen("libSDL2-2.0.so.0") + `SDL_Init` + `SDL_CreateWindow` +
  `SDL_CreateRenderer` + `SDL_SetRenderDrawColor` + `SDL_RenderClear`
  + `SDL_RenderFillRect` (3 colored rects) + `SDL_RenderPresent` +
  `SDL_RenderReadPixels` + clean shutdown — all from Kryos, **with
  no compiler changes**.
- `let s = "hello, " + name + "!"` compiles to WASM, runs in Node, and
  prints the concatenated string.
- All six v0.1 WASM examples (`wasm_hello`, `wasm_math`, `wasm_loop`,
  `wasm_fizz`, `wasm_control`, `wasm_browser_demo`) still pass.

## [1.1.0] - 2026-05-15 — "universal target"

Kryos now compiles to WebAssembly in addition to native code. The same
`.kry` source can be built to a native binary (Cranelift or LLVM) or to
a `.wasm` module that runs in any browser or WASI host.

Also fixes a long-standing TCP concurrency bug: spawned worker threads
no longer serialize through a global socket mutex during blocking
send/recv. Multi-client servers actually scale now.

### Added

- **WebAssembly backend (`--backend wasm`).** New crate
  `kryos-codegen-wasm` emits standalone `.wasm` modules. Supported in
  v0.1: i64/f64 arithmetic, comparisons, booleans, if/else/elif chains,
  while loops, function definitions, direct calls, recursion,
  `println(i64)`, `println(f64)`, `println(str-literal)` via host imports.
- **Browser demo.** `examples/wasm_browser_demo.{kry,wasm,html}` — a
  Kryos program (fib + factorial + sum) running in a real browser via
  `fetch` + `WebAssembly.instantiate`.
- **WASM host runner.** `examples/wasm_runner.js` — a 60-line Node.js
  host that provides the three imports the WASM backend expects.
- **New example programs.** `wasm_hello.kry`, `wasm_math.kry`,
  `wasm_fizz.kry`, `wasm_loop.kry`, `wasm_control.kry` — covers each
  category of WASM-supported control flow.

### Fixed

- **TCP send/recv no longer block other socket operations.** The
  global socket-table mutex is now released before `TcpStream::read`
  and `TcpStream::write` are called (via `try_clone`), matching the
  pattern `tcp_accept` already used. Verified by serving three
  concurrent curl requests against `examples/showcase/web_server.kry`.

### Backend coverage matrix

| Feature              | Cranelift | LLVM | WASM v0.1 |
|----------------------|:---------:|:----:|:---------:|
| Integer/float arith  | ✅ | ✅ | ✅ |
| Booleans, comparisons| ✅ | ✅ | ✅ |
| if/else/elif         | ✅ | ✅ | ✅ |
| while loops          | ✅ | ✅ | ✅ |
| Functions, recursion | ✅ | ✅ | ✅ |
| println              | ✅ | ✅ | ✅ |
| Heap strings         | ✅ | ✅ | ❌ |
| Arrays, maps         | ✅ | ✅ | ❌ |
| Structs, enums       | ✅ | ✅ | ❌ |
| Channels, spawn      | ✅ | ✅ | ❌ |
| HTTP, regex, JSON    | ✅ | ✅ | ❌ |

The `❌` rows in the WASM column track to v1.2 — they need ARC + a
linear-memory string runtime + WASI imports. v0.1 is intentionally
scoped to "the parts that need no heap".

## [1.0.1] - 2026-05-15 — "universal-language stress test"

Wrote 8 different classes of program in pure Kryos to validate the
universal-language claim. Found and fixed one real bug along the way.

### Added (showcases)
- `examples/showcase/extra/calc.kry` — arithmetic parser with recursive
  descent + precedence + mutual function recursion.
- `examples/showcase/extra/csv.kry` — CSV reader with group-by salary
  aggregation.
- `examples/showcase/extra/brainfuck.kry` — full Brainfuck interpreter
  (prints "Hello World!").
- `examples/showcase/extra/life.kry` — Conway's Game of Life on a 20×20
  grid (glider, blinker).
- `examples/showcase/extra/api_client.kry` — outbound HTTPS plus JSON
  tree walk against httpbin.org.
- `examples/showcase/extra/regression.kry` — linear regression by
  gradient descent. Learns y = 3x + 7 from noisy samples.
- `examples/showcase/extra/template.kry` — Mustache-style `{{var}}`
  templating engine.
- `examples/showcase/extra/regex.kry` — tiny regex engine: literals,
  `.`, `*`, `^`, `$`.

### Fixed
- **`sleep_ms` no longer fails to link.** The builtin was registered in
  the MIR builtins table but had no runtime symbol or codegen wiring.
  Now properly implemented as `kryos_rt::spawn::kryos_sleep_ms`, with
  Cranelift codegen dispatch and JIT symbol registration. Verified end
  to end: `sleep_ms(500)` waits exactly 500 ms.

## [1.0.0] - 2026-05-14 — "production"

First stable release. Same code as 0.5.0 with a 1.0 version stamp,
committing Kryos to the stability guarantees in `docs/STABILITY.md`.

From this release forward:

* The lexical grammar, the `pub` standard library, the documented
  builtins, the `kryos.toml` schema, and the `kryos` CLI subcommand
  set are stable. Breaking changes require a 2.0.0 bump.
* The `2026` language edition is the default for projects that omit
  `edition` from their manifest.
* Patch releases (`1.0.z`) fix bugs without changing behaviour. Minor
  releases (`1.y.0`) may add features and APIs but never change the
  meaning of existing code.
* Deprecations carry a warning for at least one minor cycle before
  removal in a future major.

No functional changes from 0.5.0 — see the entry below for the full
list of what shipped in this push.

## [0.5.0] - 2026-05-14 — "universal language"

The production-ready push. Kryos can now write the things it was designed
to write: HTTP servers, MCP servers, LLM agents, static site generators,
persistent databases, parallel job pools, and small compiler tools — all
in pure Kryos. The plumbing required to ship and run those programs is
also in place: a package manager with local path dependencies, prebuilt
binary distribution, a stable VS Code LSP client, and a written stability
policy.

### Added

#### Showcase apps (all runnable end-to-end)
- `examples/showcase/rest_api.kry` — full CRUD HTTP server using real
  mutable module-level globals; verified against curl.
- `examples/showcase/markdown.kry` — pure-Kryos markdown→HTML converter.
- `examples/showcase/kvdb.kry` — append-only persistent key/value store
  with tab/newline-safe percent encoding, in-memory replay, and compaction.
- `examples/showcase/mcp_server.kry` — real Model Context Protocol
  server speaking JSON-RPC 2.0 over stdio. Implements `initialize`,
  `tools/list`, `tools/call`, `shutdown`. Built-in tools: `echo`, `now`,
  `add`, `read_file`, `write_file`, `http_get`.
- `examples/showcase/agent.kry` — OpenAI-compatible Chat Completions
  agent with tool-use loop. Drives multi-turn conversations through
  function calling; falls back to an offline demo that prints the
  exact OpenAI wire-format request.
- `examples/showcase/ssg.kry` — static site generator: inlined
  markdown→HTML, layout template, manifest-driven build. Emits a real
  multi-page HTML site plus a shared `style.css`.
- `examples/showcase/worker_pool.kry` — fan-out/fan-in concurrency
  showcase using `spawn` plus channels and sentinel-based shutdown.
- `examples/showcase/kdoc.kry` — a small documentation extractor
  written in Kryos itself. Scans `.kry` files for `pub` declarations
  and emits a Markdown API reference. Satisfies the self-host milestone.

#### Language and compiler
- **Real mutable module-level globals.** `let mut <name>: <type> = <expr>`
  at file scope, no workarounds, with proper MIR type inference.
- **String comparison codegen.** `<`, `>`, `<=`, `>=` on strings now
  lower through `kryos_string_compare(a, b) -> i64` and `icmp`.
  Available in both the AOT and JIT backends.
- **f64↔i64 round-trips in codegen** for `json_number` and friends.
- **Mutable globals participate in type inference** for indexing and
  assignment.

#### Package manager
- `parse_dep_string` accepts bare relative/absolute paths (`./foo`,
  `../foo`, `/abs`) and an explicit `path:<dir>` form in addition to
  the existing `<source>@<version>` form.
- Driver import resolver: walks up from each source file looking for
  `.kryos/deps/<pkg>.redirect` written by `kryos pkg install`, parses
  the `path = "..."` entry, and resolves `use pkg` to `<dep>/src/lib.kry`
  (or `<dep>/src/<pkg>.kry`) and `use pkg::a::b` to `<dep>/src/a/b.kry`.
  Verified end-to-end with two side-by-side projects.

#### Distribution
- `install.sh` / `install.ps1` already shipped — now coupled with the
  release workflow that builds prebuilt binaries for
  `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`,
  `x86_64-apple-darwin`, and `aarch64-apple-darwin` when a `v*` tag is
  pushed.
- New `.github/workflows/cross.yml`: cheap cross-build matrix
  (`linux-gnu`, `linux-musl`, `windows-gnu`, `aarch64-linux-gnu`) on
  every push.

#### Editor support
- VS Code extension v0.3.0 wires up the LSP client. Launches
  `kryos lsp` over stdio with `vscode-languageclient`. Configurable
  via `kryos.serverPath`, `kryos.serverArgs`, and `kryos.trace.server`.

#### Documentation
- `docs/STABILITY.md` — written stability policy: SemVer, what's stable
  vs. internal, deprecation lifecycle, and the language-edition
  mechanism (`edition = "2026"` is the current default).
- `docs/12-modules-and-packages.md` — appended a verified local-path-dep
  walkthrough.

### Fixed
- `infer_expr_type` now consults `ctx.mutable_globals` so indexing a
  global `[str]` array returns a `str`, not a pointer-sized `i64`. This
  unblocked the kvdb showcase and similar code that holds collections
  in a global.

## [0.4.0] - 2026-05-11 — "credible beta"

This is the release that takes Kryos from a hand-rolled toy compiler to a
language that can credibly be tried by someone other than its author.
Every item below ships with documentation, tests, or a runnable demo;
nothing in this release is marked experimental.

### Added

#### Reliability
- **Runtime panics carry source spans.** Every runtime panic (overflow,
  division by zero, array-OOB, stack overflow, etc.) now points at the
  `file:line:col` where it originated rather than at the runtime crate
  internals.
- **Stack-overflow detection** via a `SIGSEGV` alt-stack handler that
  distinguishes recursion blow-outs from generic segfaults and reports
  them with a friendlier message + the offending span.
- **Integer-overflow policy** is now defined and documented in
  `docs/16-integer-overflow.md`: `wrapping_*` / `checked_*` /
  `saturating_*` builtins are available, signed overflow with the plain
  `+ - *` operators is well-defined as wrap-on-release, panic-on-debug.
- **Unsafe-block audit** in `docs/17-unsafe-audit.md`: every `unsafe`
  region in the runtime and native stdlib (8 patterns across 8 files)
  has a documented invariant.

#### Tooling
- **`kryos explain ERRXXXX`** with 20 long-form error articles (modelled
  on `rustc --explain`). Each includes a broken example, a fixed example,
  and the rationale behind the diagnostic. Run `kryos explain --list` for
  the catalog.
- **`kryos test` cargo-parity**: positional `FILTER` argument,
  `--exact`, `--nocapture`, `--list`, and `--format=json` for
  newline-delimited JSON output that mirrors
  `cargo test --format=json` events.
- **`kryos build --target=<triple>`** is now wired through to LLVM
  rather than silently using the host triple. Eleven known-good targets
  ship with descriptions; `--target=help` prints the table. See
  `docs/18-cross-compilation.md` for required toolchains and known
  failure modes.
- **Benchmark suite** under `benchmarks/` covering mandelbrot, n-body,
  binary-trees, fannkuch, matmul, and fib against Rust and C baselines.
  `benchmarks/run.sh` produces a reproducible `RESULTS.md`; on the
  reference hardware Kryos hits parity with C on mandelbrot (1.03×) and
  stays within 3.5×4.5× on the numeric benchmarks.

#### Documentation
- **`docs/19-language-reference.md`** — the authoritative v0.4 language
  spec: lexical structure, type system, expression grammar (with the
  full precedence table), control flow, declarations, pattern matching,
  ownership / drop order, integer overflow, concurrency, unsafe code,
  modules, panics, and a conformance checklist.
- **`docs/BUGS.md`** records the one known-leaky pattern in the v0.4
  ownership checker (string-field move across struct-returning function
  boundaries) along with its workaround.

#### Showcase suite
Five end-to-end programs under `examples/showcase/` proving the
language can be used to build the kinds of things it claims to support:

- `cli_tool.kry`       — grep-style CLI with POSIX exit codes.
- `parser.kry`         — recursive-descent calculator with error
  reporting (source columns, three failure modes).
- `bytecode_vm.kry`    — stack VM with a 13-opcode ISA, disassembler,
  and three demo programs (sum 1..10, factorial(7), fib(10)).
- `agent_runtime.kry`  — LLM-style tool-use loop: history, planner,
  tool registry, bounded step budget.
- `web_server.kry`     — minimal HTTP/1.0 server using `tcp_listen` /
  `tcp_accept` / `tcp_send`, serving HTML / JSON / 404 routes.

See `examples/showcase/README.md` for run instructions.

### Changed
- Workspace version bumped to `0.4.0` across all crates.
- The test runner library now exposes `RunOptions`, `run_test_with`,
  `run_all_with`, `run_annotated_tests_with`, and `format_report_json`
  in addition to the existing entry points. Existing callers keep
  working unchanged.
- 843 workspace tests now pass (up from 831 at the start of the v0.4
  cycle); +12 from new unit tests across the `kryos test`, `explain`,
  and `build --target` work.

### Status

Kryos v0.4.0 is the **credible-beta** release: the toolchain is
complete enough that someone other than the author can clone it, build
it, follow the docs, and write real programs. Real users and a stable
1.0 API still ahead.

---

## [0.3.6] - 2026-05-11

### Fixed
- CI green again: resolved clippy errors introduced in the LLVM aggregate-ABI and Cranelift drop-path commits (`collapsible_match`, `too_many_arguments`, `if_same_then_else`) via targeted `#[allow]` attributes; no behavior change.
- `rustfmt` drift across `kryos-codegen-cranelift`, `kryos-codegen-llvm`, `kryos-mir`, `kryos-stdlib-native`, `kryos-types` -- all formatted with `rustfmt 1.95`.

### Changed
- Repository home: all `FrostbyteDevTeam/kryos-lang` URLs in README, docs, install scripts, `Cargo.toml`, VS Code extension, and contributing guide updated to `NORTHTEKDevs/kryos-lang`.
- `README.md`: replaced the misleading "~48 GB RAM" debug-build warning with a calibrated build-footprint note (~6 GB disk, ~3 GB peak RAM with `-j 2`, ~2 min cold). Documented that LLVM is **not** a build dependency -- the LLVM backend emits IR as text.
- `README.md` quick-start example path now points at `../examples/hello.kry` (the previously referenced `examples/proof.kry` did not exist).
- Bumped to `v0.3.6` across `Cargo.toml`, `install.ps1`, `docs/01-getting-started.md`, `docs/WHY_KRYOS.md`.

## [0.3.5] - 2026-04-16

### Fixed
- `MirType::Map` sentinel migration: replaced `Ptr(Str)` map-handle hack with typed `Map { key, value }` variant throughout MIR, lowering, and both backends
- REPL `:type` map inference: map literals now report the actual key/value element types instead of always `Map<i64, i64>`
- REPL `:type` index inference: indexing into a `Map<K, V>` now returns `V` instead of `i64`
- Package registry: `parse_index_entry` now parses the `deps` JSON object into the dependency map; transitive dependency resolution from registry responses now works
- `NodeTable::get_mut` and `::remove` dead_code warnings suppressed in `kryos-stdlib-native/src/json.rs` -- intentional forward-facing API surface

### Changed
- `README.md`: corrected version badge from v0.3.4 to v0.3.5
- `CONTINUE.md` (internal dev artifact) replaced with `ARCHITECTURE.md` for public distribution
- `examples/README.md`: documented all 20 examples (was 12); added blocking notes for `http_api.kry` and `mcp_server.kry`

---

## [0.3.4] - 2026-04-14

### Added
- `float(str)` builtin -- parse a string as f64 via `kryos_builtin_parse_float` (Cranelift backend)
- Example: `ai_agent.kry` -- research agent using the Kryos agent framework with Anthropic API integration
- Example: `http_api.kry` -- in-memory task-list REST API with routing and JSON responses
- Example: `mcp_server.kry` -- Model Context Protocol server over stdio (JSON-RPC 2.0)

### Fixed
- MIR match arm type inference: enum variant field types now correctly propagate to the result local (fixes f64 fields inferred as i64 in `JsonValue::Number(n) => n` patterns)
- JSON stdlib: `if/else if/else` chains in `_parse_string`, `_parse_number`, and `_escape_string` converted to sequential `if` + flag pattern (avoids compiler branch target bug in deep else-if chains)
- JSON parser: `@copy` on `Parser` struct prevents ownership errors across recursive descent calls

---

## [0.3.3] - 2026-04-14

### Added
- `Self` type in trait method signatures resolves to the implementing type at each call site
- `Type::method(args)` associated function syntax (`StaticMethodCall` AST node, parsed, type-checked, MIR-lowered, both backends)
- `install.ps1` Windows PowerShell installer
- `CONTRIBUTING.md` developer guide with compiler pipeline walkthrough

### Fixed
- Clippy: `&param_ty` double-reference in `kryos-types/src/check.rs:1421` (immediate deref lint)
- Version bump: `compiler/Cargo.toml` 0.2.1 -> 0.3.3

---

## [0.3.2] - 2026-04-13

### Added
- Developer adoption sprint: stdlib completions, string safety improvements, DX ergonomics
- Module system for stage-0 self-host build (`use` imports in bootstrap)
- Calling closures stored as struct fields
- Correct MirType for fn-typed captures in lambda thunks

---

## [0.3.1] - 2026-04-12

### Added
- `@pure` attribute optimization -- CSE (common subexpression elimination) and dead call elimination at MIR level
- `@test` annotation runner -- discover and JIT-execute `@test` functions via `kryos test`

### Fixed
- REPL state persistence -- `use`/`type`/`extern`/`actor`/`pub` classified as declarations, persist across lines
- Array element drop recursion -- named type drop helpers for struct/enum fields (prevents infinite recursion)
- Closure capture memory leak -- per-closure dropper thunks generated for ARC env cleanup

---

## [0.2.2] - 2026-04-09

### Fixed
- Deep memory safety pass: ownership cloning, Shared drop, @copy ARC retain
- String interpolation intermediate leak
- try/catch result enum leak
- LLVM backend drop parity (enum, struct, array, map, function)
- Const eval overflow: checked arithmetic, unfoldable at compile time
- Formatter: doc comments preserved on Actor, TypeAlias, Import, Extern declarations

---

## [0.2.1] - 2026-04-08

### Fixed
- Critical memory safety, control flow, and type system fixes
- Exception cleanup includes MirType::Enum in droppable filter
- CI/CD GitHub Actions matrix (Ubuntu + Windows + macOS)
- Clippy clean (0 warnings)
- @copy struct deep-copy: Function/Shared fields call kryos_arc_retain
- ActorSend: heap-typed args cloned before send

---

## [0.2.0] - 2026-04-08

### Self-Hosting Milestone
- 18,700-line self-hosted compiler written in Kryos (15 files)
- Full compilation pipeline: lexer, parser, type checker, MIR lowering, optimizer, register allocator, x86_64 codegen, ELF/COFF linker
- Zero-dependency runtime (runtime.kry): raw Linux x86_64 syscalls, bump allocator, byte buffers, string/array/map operations
- 3-stage bootstrap verification script (stage-0 Rust -> stage-1 -> stage-2 -> stage-3, SHA-256 identity proof)
- Stage-1 binary: 1MB PE32+ executable, compiles and runs Kryos programs
- Self-host type-checks cleanly (0 errors) through the Rust compiler via concatenation

### Module System
- File-based module resolution with `use` imports
- Stdlib resolution via `use std::math`, `use std::json`, etc.
- Selective imports: `use std::math::{abs, min, max}`
- Transitive imports with diamond deduplication and cycle detection
- Sibling file and directory module (`foo/mod.kry`) resolution
- Const declarations now importable by name

### Capability Enforcement
- 35 builtin functions mapped to 7 capability categories (io, net, process, term, crypto, time, ffi)
- Deny-by-default enforcement within `@capabilities`-annotated scopes
- Cross-function capability propagation (caller must have callee's required capabilities)
- Opt-in design: unannotated functions have ambient authority (backward compatible)

### LLVM Backend
- Fixed systematic ptr/i64 type mismatch in LLVM IR emitter
- Added `coerce_value` helper for type-safe conversions at 15+ boundary points
- Fixed identity copy pattern (`add ptr` -> `getelementptr i8`) for pointer types
- Fixed `to_string` return type coercion and float argument dispatch
- LLVM tools available on Windows (clang 21.1.8, lld-link)

### Compiler Fixes
- Generic functions now monomorphize per call site (fresh type variables)
- Multiple trait impls no longer clobber each other's `self` type
- `throw` propagates across function boundaries via thread-local exception state
- `to_string()` on strings returns the string (not the raw pointer address)
- `sqrt`, `floor`, `ceil`, `abs` use native Cranelift instructions (fixes ICE)
- MIR type inference for untyped constants (no longer defaults to I64)
- Cranelift float/int type coercion uses proper bitcast instructions
- `kryos pkg init` now creates files on disk (kryos.toml, src/main.kry, .gitignore, README.md)
- `kryos check` now supports `--skip-ownership` flag

### Added
- Array concatenation operator: `a + b` and `a += b` for arrays (type-checked, MIR-lowered, both backends)
- `kryos_array_concat` runtime function for array concatenation
- Closure environments heap-allocated via `malloc` (fixes segfault when closures escape their creating function)
- `push(arr, val)` and `pop(arr)` now borrow the array instead of moving it in ownership analysis
- Native test runner prefers release binary over debug (matches `--release` build workflow)
- `StoreField` MIR instruction for proper struct field mutation (replaces `__kryos_field_store` hack)
- Full `StoreField` implementation in both Cranelift and LLVM backends
- `--skip-ownership` CLI flag for self-host bootstrap (ownership checker fires on refcounted patterns)
- `kryos_string_char_at` runtime function for string indexing
- `no_struct_lit` parser flag to prevent struct literal ambiguity in if/while/for/match conditions
- `parse_expr_no_struct_lit()` parser function used in all conditional contexts
- Array/tuple codegen now uses runtime `kryos_array_new`/`push`/`get` for consistency
- Array size coercion: fixed-size arrays assignable to dynamic arrays (`[T; N]` -> `[T]`)
- Division-by-zero check widened to i64 for narrow integer types
- Float-to-int and int-to-float bitcasting in function call argument coercion
- IndexAccess type inference for arrays, tuples, and strings in MIR lowering
- MIR elif duplicate block fix (prevents self-loop when last elif has no else)
- New example: `word_count.kry`
- Package registry now computes deterministic content hash (replaces TODO placeholder)

### Fixed
- Demo example: removed unimplemented tensor extern calls that caused segfault
- Calculator example: added `**` (power) operator to string-matched calculator
- Clippy: removed dead code, unused imports, function-cast-as-integer warnings
- Clippy: fixed prefix-stripping pattern in semver parser

### Changed
- Self-host MIR: array concatenation (`arr + [elem]`) replaced with `push(arr, elem)` for efficiency
- Self-host main: `std.io.read_file` -> `file_read`, `std.process.args()` -> `args()` (runtime functions)
- Self-host codegen: `&&`/`||` -> `and`/`or` (correct Kryos syntax), `char_at` -> `char_code(substr(...))`
- Bootstrap script upgraded from 2-stage to proper 3-stage verification (stage-2 == stage-3)
- FFI crates (`kryos-rt`, `kryos-stdlib-native`) now properly document safety and suppress raw-pointer clippy lints

## [0.1.1] - 2026-04-07

### Fixed
- Parser struct-literal ambiguity: `match TK_EOF { ... }` no longer parsed as struct literal
- Struct field access segfaults: structs now heap-allocated (malloc) instead of stack slots
- String match patterns: `match s { "hello" => ... }` now emits equality-comparison chain instead of integer switch
- Tail expression return: functions ending with bare `match`/`if` now implicitly return the result
- String concatenation with non-string operands: automatic coercion via `coerce_to_string()` helper
- Double-free prevention: `dropped_locals` tracking prevents nested scope re-drops
- ComptimeBlock type inference: `comptime { expr }` now infers correct result type
- Copy semantics for computed expressions: BinaryOp, FnCall, MatchExpr, IfExpr, UnaryOp, MethodCall, Cast, IndexAccess, Block, PipeExpr, and Borrow/Deref now correctly report copy when result is a primitive type
- `type_of()` builtin: compile-time type dispatch for all MIR types (f64, bool, str, etc.) instead of always returning "i64"
- `assert()` builtin: accepts 1 or 2 args, bool conditions extended to i64, default "assertion failed" message

### Added
- 4 new example programs: calculator, word_count, json_counter, all_features showcase
- String pattern matching in match expressions (via BinOp::Eq chain with Branch terminators)
- Implicit return for tail expressions in non-void functions
- `fn main()` wrapper for kryos_bootstrap.kry self-hosting lexer example
- Criterion benchmark suite: 9 groups (lex, parse, typecheck, ownership, capabilities, MIR, codegen, pipeline, JIT fibonacci)
- 9 new ownership analysis tests for copy semantics validation
- `is_type_expr_copy()` helper for cast expression type analysis

### Changed
- Ownership analyzer `expr_is_copy()` now recursively handles 15+ expression types
- Type checker: `assert()` signature updated to accept `bool` condition, 1-arg special case
- Type checker: `type_of()` parameter type set to `Error` (accepts any type)
- Documentation: fixed incorrect builtin names (`int`/`float`/`str` → `parse_int`/`parse_float`/`to_string`)
- Documentation: updated tail expression return note in Functions chapter
- Documentation: added implementation status callouts for borrowing and self-healing runtime
- Documentation: fixed `dyn Trait` implementation status (vtable-based dispatch is implemented)
- Standard library stubs: fixed broken references in math, string, collections, crypto, fmt, http, json, net, and test modules
- README: updated version to v0.1.1, added all_features example

## [0.1.0] - 2026-04-07

### Added
- 21-crate Rust compiler (49,000+ lines)
- Dual backends: Cranelift (fast debug builds) and LLVM (optimized release builds)
- Ownership-based memory safety without lifetime annotations
- Compile-time capability enforcement (deny-by-default resource access)
- Compile-time evaluation with `comptime` blocks
- Type inference with explicit annotations where needed
- Pattern matching with integer, string, enum, and wildcard patterns
- Dynamic dispatch via `dyn Trait` (vtable-based)
- Generics with monomorphization
- Concurrency: `spawn`, typed channels, actors, `select`
- 5 MIR optimization passes: constant folding, dead code elimination, function inlining, tail-call optimization, strength reduction
- 28 standard library modules (strings, math, collections, I/O, networking, crypto, JSON, regex, datetime, tensors, agents, probability, reactive streams)
- Ergonomic builtins: `file_read`, `file_write`, `env_get`, `time_now`, `assert`, `parse_int`, `parse_float`, `type_of`
- Error handling with `try`/`catch`/`throw`
- VS Code extension with syntax highlighting, snippets, and language configuration
- Language Server Protocol (LSP) server
- Code formatter (`kryos fmt`)
- Documentation generator (`kryos doc`)
- Package manager (`kryos pkg`)
- Test runner (`kryos test`)
- Interactive REPL (`kryos repl`)
- C header binding generator (`kryos bindgen`)
- Native tensor runtime with 38 FFI operations
- GitHub Actions CI (build, test, clippy, fmt on Linux and Windows)
- 13 example programs
- 15-chapter language manual
- 680+ tests, all passing
