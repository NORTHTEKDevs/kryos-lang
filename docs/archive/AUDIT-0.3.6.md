# Kryos systems-language audit — starting from v0.3.6

This document is the working ledger from the audit that began the day v0.3.6
shipped. It records what was actually inventoried (not claimed), where the
real gaps are, and what was fixed inside the audit itself.

## Bottom line

Kryos at v0.3.6 is materially more complete than typical hobby languages.
It is **usable for non-trivial programs today** (channels, error handling,
HTTP-style routing, a self-tokenizer, structs/enums, generics, pattern
matching with exhaustiveness, ownership analysis). It is **not yet** a
full systems language in the production sense — gaps are tracked below.

## Crate inventory (compiler/, source lines, excluding tests)

| Crate                  | files | LoC (src) | LoC (tests) |
|------------------------|------:|----------:|------------:|
| kryos-ast              |     6 |       709 |           0 |
| kryos-bindgen          |     3 |     1,570 |         349 |
| kryos-capabilities     |     3 |     1,563 |       1,049 |
| kryos-cli              |    13 |     1,857 |           0 |
| kryos-codegen-cranelift|     3 |     6,387 |       3,884 |
| kryos-codegen-llvm     |     2 |     4,648 |       1,658 |
| kryos-doc              |     1 |       827 |           0 |
| kryos-driver           |     5 |     1,711 |         954 |
| kryos-errors           |     2 |       347 |           0 |
| kryos-fmt              |     2 |     1,539 |         548 |
| kryos-lexer            |     3 |       912 |         790 |
| kryos-linker           |     3 |       695 |         344 |
| kryos-lsp              |     7 |     1,249 |         942 |
| kryos-mir              |    12 |     9,962 |       2,868 |
| kryos-ownership        |     3 |     1,586 |       1,230 |
| kryos-package          |     7 |     1,843 |         456 |
| kryos-parser           |     2 |     2,512 |       1,060 |
| kryos-rt               |    17 |     5,194 |         364 |
| kryos-stdlib-native    |    18 |     3,024 |         158 |
| kryos-test-runner      |     1 |     1,066 |         243 |
| kryos-types            |     7 |     4,537 |       1,636 |

Workspace src ≈ **54k LoC**, tests ≈ **22k LoC**, **825 tests passing**.

## Standard library inventory

| Module       | Kryos impl (`compiler/stdlib/*.kry`) | Native shim (`kryos-stdlib-native/src/*.rs`) | Docs |
|--------------|:--:|:--:|:--:|
| agent        | ✅ |    | ✅ |
| chan         | ✅ |    | ✅ |
| collections  | ✅ |    | ✅ |
| core-builtins|    |    | ✅ |
| cost         | ✅ |    | ✅ |
| crypto       | ✅ | ✅ | ✅ |
| datetime     | ✅ | ✅ | ✅ |
| db           | ✅ | ✅ (sqlite) |    |
| fmt          | ✅ |    | ✅ |
| fs           | ✅ | ✅ | ✅ |
| http         | ✅ |    | ✅ |
| io           | ✅ | ✅ | ✅ |
| iter         | ✅ |    | ✅ |
| json         | ✅ | ✅ | ✅ |
| math         | ✅ | ✅ | ✅ |
| net          | ✅ | ✅ | ✅ |
| option       | ✅ |    | ✅ |
| os           | ✅ | ✅ (env, process) | ✅ |
| path         | ✅ | ✅ | ✅ |
| probable     | ✅ |    | ✅ |
| process      | ✅ | ✅ | ✅ |
| re/regex     | ✅ | ✅ | ✅ |
| result       | ✅ |    | ✅ |
| stream       | ✅ |    | ✅ |
| string       | ✅ | ✅ | ✅ |
| sync         | ✅ | ✅ (sync_prims) | ✅ |
| tensor       | ✅ |    | ✅ |
| term         | ✅ | ✅ | ✅ |
| test         | ✅ |    | ✅ |
| tls          |    | ✅ |    |
| tracked      | ✅ |    | ✅ |

`compiler/stdlib/` is 11,325 lines of pure Kryos. `kryos-stdlib-native` is
3,024 lines of Rust shims for syscalls/syscalls-like primitives.

## What runs end-to-end

Confirmed by running on real generated machine code via `kryos run`:

- `examples/all_features.kry` — 30-section feature showcase
- `examples/channels.kry` — producer/consumer over a channel
- `examples/chat_server.kry` — message routing + per-user counts
- `examples/http_server.kry` — route dispatch + error mapping to status codes
- `examples/json_counter.kry` — JSON-like parsing + error propagation
- `examples/mini_grep.kry` — file/IO error handling + match reporting
- `examples/grep.kry` — substring match across multi-line input
- `examples/kryos_bootstrap.kry` — a Kryos-in-Kryos lexer for a math expression

All 5 CI smoke targets (hello, fibonacci, calculator, word_count, shapes)
type-check and execute clean.

## Bugs found and fixed during this audit

### 1. UTF-8 double-encoding in string and char literals (FIXED)

**Symptom**: `println("hello — world")` emitted six bytes
`c3 a2 c2 80 c2 94` instead of three bytes `e2 80 94` for the em-dash.
Any non-ASCII character in a string or char literal was double-encoded.

**Root cause**: `kryos-lexer/src/lexer.rs` was reading source as `&[u8]`
and inside `scan_string` / `scan_char` doing `text.push(self.advance() as
char)`. `u8 as char` produces the Latin-1 codepoint of that byte; Rust's
`String::push` then re-encodes that codepoint back to UTF-8. The two
layers compose to double-encode every byte ≥ 0x80.

**Fix**: added `advance_utf8(&mut self) -> char` that recognizes the lead
byte's UTF-8 width (2/3/4 bytes) and decodes the full scalar value via
`std::str::from_utf8` before appending. Both string and char scanners now
use it. Malformed sequences yield U+FFFD instead of silently producing
double-encoded output. Regression tests added in `lexer_tests.rs`.

## Known gaps to address next

These are the concrete pieces between v0.3.6 and a 1.0 systems language.

### Language / type system
- [ ] Unicode identifier support (currently ASCII-only)
- [ ] Specification document beyond `docs/grammar.md` (formal type rules,
      drop order, trait resolution rules)
- [ ] Trait coherence story documented

### Stdlib portability gaps
- [ ] Audit how many `stdlib/*.kry` modules are pure types/traits vs how
      many actually depend on native shims that are missing on Windows
- [ ] `tls.rs` exists with no Kryos-side `stdlib/tls.kry` — surface it
- [ ] `sqlite.rs` exists but no docs page

### Diagnostics
- [ ] Lint suite (unused imports, dead code, shadow warnings) — partial
      coverage exists; needs an inventory
- [ ] `--explain ERRXXXX` style numbered diagnostics
- [ ] Source-level suggestions are present and excellent in many cases
      (`help: consider declaring with 'let mut'`) — extend coverage

### Memory safety
- [ ] `kryos-ownership` exists and is large; needs documented invariants
      and a fuzz harness (cargo-fuzz on parser+ownership)
- [ ] Audit every `unsafe` block (codegen, rt, stdlib-native)
- [ ] miri pass on the runtime crate

### Tooling
- [ ] Cross-compilation UX from a single host (`kryos build --target=...`)
- [ ] DWARF / debug info emission and a documented gdb/lldb story
- [ ] `kryos test` is wired (kryos-test-runner exists) — needs docs and
      flag parity with `cargo test` (--filter, --nocapture, etc.)
- [ ] LSP feature inventory (kryos-lsp is 1.2k LoC + 942 LoC tests, so
      meaningful, but unclear which of completion/hover/goto/refs/diagnostics
      are wired)
- [ ] Package manager: kryos-package is 1.8k LoC; needs a documented
      registry story

### Self-hosting
- [ ] `examples/kryos_bootstrap.kry` demonstrates a Kryos lexer; full
      self-hosting requires porting kryos-parser → MIR → codegen
- [ ] Decide on bootstrap strategy: write self-host in Kryos that
      generates Rust source, or port codegen incrementally

### Benchmarks
- [ ] No benchmarks vs Rust/C/Zig in tree. A `bench/` directory with
      mandelbrot, n-body, binary trees, regex-redux, k-nucleotide
      would set expectations honestly.

### Release engineering (carryover from v0.3.6)
- [ ] Ship a `.vsix` for the VS Code extension in the release tarball
- [ ] Address Dependabot alert on rustls-webpki (PR already open)
- [ ] Publish `kryos-cli` to crates.io once API is stable enough
