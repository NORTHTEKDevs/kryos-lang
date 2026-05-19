# Kryos CLI command reference

Every `kryos` subcommand at a glance. For full options, run `kryos <command> --help`.

## Build & run

| Command | Purpose | Example |
| --- | --- | --- |
| `kryos run <file>` | JIT-compile (Cranelift) + execute | `kryos run hello.kry` |
| `kryos run <file> --time` | Run with compile/exec/total timing | `kryos run hello.kry --time` |
| `kryos build <file>` | AOT compile to a native binary (Cranelift) | `kryos build hello.kry` |
| `kryos build <file> --release` | AOT compile with LLVM optimization | `kryos build hello.kry --release` |
| `kryos build <file> --backend wasm` | AOT compile to WebAssembly | `kryos build hello.kry --backend wasm` |
| `kryos build <file> -g` | Include debug info (DWARF / PDB) | `kryos build hello.kry -g` |
| `kryos eval "<expr>"` | One-liner: wraps expression in `fn main()` | `kryos eval 'println(to_string(42))'` |

## Type-check & format

| Command | Purpose |
| --- | --- |
| `kryos check [path]` | Fast type-check without codegen |
| `kryos check [path] --watch` | Re-type-check on every save (mtime poll) |
| `kryos fmt [files]` | Format source in-place (or `--check` for CI) |
| `kryos lint [path]` | AST-driven lints (L001 large-fn, L003 magic numbers, L005 shadow, L006 todo) |
| `kryos audit [path]` | Capability inventory + extern blocks + 13 secret-pattern scan |

## Tests, benches, profiling

| Command | Purpose |
| --- | --- |
| `kryos test [filter]` | Discover + run `@test` functions; supports `--exact`, `--list`, `--format=json` |
| `kryos bench [filter]` | Run `@bench` functions; reports min/median/mean/p95/max |
| `kryos coverage` | Run tests with profile mode on; reports covered / uncovered functions |
| `kryos profile <file>` | Run with call-count profiling; top-20 hot-list at exit |
| `kryos trace <file>` | Run with depth-indented function entry/exit logging on stderr |

## Project lifecycle

| Command | Purpose | Templates |
| --- | --- | --- |
| `kryos new <name> [--template]` | Create a fresh project from a template | `cli`, `http`, `lib`, `agent` |
| `kryos pkg init [name]` | Add a `kryos.toml` to the current dir | |
| `kryos pkg add <spec>` | Add a dependency | |
| `kryos pkg remove <name>` | Remove a dependency | |
| `kryos pkg list-local [--root]` | List first-party packages discovered on disk | |
| `kryos pkg install` | Resolve + fetch all dependencies | |
| `kryos pkg search <query>` | Search the registry | |
| `kryos pkg publish` | Package and publish to the registry | |
| `kryos tree [--transitive]` | Print the dependency tree | |
| `kryos clean [--dry-run]` | Remove `target/`, root-level build artifacts, lock files | |

## Editor + docs

| Command | Purpose |
| --- | --- |
| `kryos lsp` | Start the language server on stdin/stdout (point your editor at it) |
| `kryos doc [files]` | Generate markdown or HTML docs from `///` comments |
| `kryos doc serve` | Generate HTML docs and serve them on http://127.0.0.1:8088 |
| `kryos explain <Exxx>` | rustc-style long-form explanation for any diagnostic code |
| `kryos bindgen <header.h>` | Generate Kryos `extern` declarations from a C header |
| `kryos repl` | Interactive REPL with persistent `~/.kryos_history` |

## Diagnostics

| Command | Purpose |
| --- | --- |
| `kryos doctor [--verbose]` | Diagnose toolchain installation (linker, rt-libs, env, optional tools) |
| `kryos watch <file> [--interval=N] [--run=run|check]` | Mtime-poll watch + auto-rerun on change |
| `kryos version` | Detailed version + build info |

## v3.x release notes

The full per-release detail lives in [`CHANGELOG.md`](../CHANGELOG.md). The highlights:

| Tag | Theme |
| --- | --- |
| v3.0 | Production hardening — 34/34 LLVM↔Cranelift parity, GPG-attested releases, tier-1 macOS-14 + Windows |
| v3.1 | LSP depth — document symbols, references, rename, signature help, inlay hints, code actions, semantic tokens |
| v3.2 | Stdlib breadth — datetime full UTC math + RFC3339, re find/capture/replace, base64 (new), uuid v4 (new) |
| v3.3 | Learn-Kryos — 4 cookbook recipes, common-errors reference, cheatsheet |
| v3.4 | `kryos bench` + `@bench` attribute |
| v3.5 | `kryos lint` (4 lints) + `kryos audit` (capability + extern + 13 secret patterns) |
| v3.6 | `kryos new` scaffolder with 4 templates |
| v3.7 | `kryos trace` execution tracing |
| v3.8 | Perf — 0-alloc TraceFrame, ~13.3ns/fn-call median |
| v3.9 | `kryos watch` + `kryos clean` + REPL persistent history |
| v3.10 | 5 first-party packages under `packages/` + `kryos pkg list-local` |
| v3.11 | `kryos profile` + 3 showcase examples |
| v3.12 | `kryos doctor` + `kryos tree` + LSP `codeAction` |
| v3.13 | `where`-clauses on functions + `kryos coverage` |
| v3.14 | LSP semantic tokens + `kryos run --time` |
| v3.15 | `kryos doc serve` + LSP member-access completion |
| v3.16 | `kryos check --watch` + `kryos eval` |
| v3.17 | This command reference + polish |

## Cookbook & learning path

- [docs/learn/README.md](./learn/README.md) — the 30-minute tour
- [docs/learn/cookbook/](./learn/cookbook) — 10 task-oriented recipes
- [docs/learn/common-errors.md](./learn/common-errors.md) — top-20 errors and fixes
- [docs/learn/cheatsheet.md](./learn/cheatsheet.md) — syntax at a glance
- [docs/19-language-reference.md](./19-language-reference.md) — the spec
