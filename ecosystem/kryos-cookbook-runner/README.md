# kryos-cookbook-runner

**Project 30** - Executable, capability-badged documentation examples: extract fenced Kryos blocks from Markdown, compile-check each one, and annotate it with the capability badge computed by `kryos manifest --caps`.

## What it does

For every ` ```kryos ` block in a Markdown file the runner:

1. Writes the block content to a temp file.
2. Runs `kryos check` -- reports **PASS** or **FAIL** with the compiler diagnostic.
3. For passing blocks, runs `kryos manifest --caps --format pretty` and derives a capability badge from the union of all annotated function capabilities.
4. Prints a one-line summary per block.

With `--rewrite`, it inserts a `<!-- @badge: ... -->` comment immediately above each passing block in the original file.

## Usage

```bash
kryos run ecosystem/kryos-cookbook-runner/src/runner.kry <file.md>
kryos run ecosystem/kryos-cookbook-runner/src/runner.kry <file.md> --rewrite
```

**Example output:**

```
Example 1 (line 9): PASS  badge: compute
Example 2 (line 20): PASS  badge: compute
Example 3 (line 37): FAIL
  error[E0100]: type mismatch: expected `i64`, found `str`
  ...
Example 4 (line 48): PASS  badge: io
```

**Capability badges:**

| Badge | Meaning |
|-------|---------|
| `compute` | Pure computation, no ambient authority |
| `io` | Reads or writes the filesystem |
| `net` | Makes network connections |
| `io, net` | Multiple capabilities (union of all functions) |

**Exit codes:**
- `0` - all blocks compiled successfully
- `1` - one or more blocks failed (or usage error)

## Rewrite output

After `--rewrite`, each passing block gets a badge comment:

```markdown
<!-- @badge: compute -->
```kryos
@capabilities()
fn main() { println("hello") }
```
```

FAIL blocks are left unchanged; no comment is inserted.

## Build and run

```bash
# Run directly with the JIT
kryos run ecosystem/kryos-cookbook-runner/src/runner.kry docs/tutorial.md

# Rewrite mode: annotate in place
kryos run ecosystem/kryos-cookbook-runner/src/runner.kry docs/tutorial.md --rewrite

# Set a specific kryos binary
KRYOS=/path/to/kryos kryos run ecosystem/kryos-cookbook-runner/src/runner.kry docs/tutorial.md
```

## Tests

```bash
# Unit tests (pure compute) -- kryos test compatible
kryos test --path ecosystem/kryos-cookbook-runner/tests

# Integration tests (spawn kryos check + manifest -- process spawning required)
kryos run ecosystem/kryos-cookbook-runner/integration/test_runner.kry
```

**Note on `kryos test`:** The `tests/` directory contains only pure-compute tests and runs cleanly under `kryos test`. The integration tests in `integration/` invoke the kryos binary via `std::process` and must be run with `kryos run`.

## Project structure

```
ecosystem/kryos-cookbook-runner/
  kryos.toml
  src/
    extract.kry   -- markdown fence extraction (pure compute, no imports)
    badge.kry     -- manifest badge derivation (pure compute, no imports)
    runner.kry    -- CLI entry point (process + io)
  tests/
    test_extract.kry  -- 12 unit tests for extract module
    test_badge.kry    -- 16 unit tests for badge module
    fixtures/
      sample.md     -- 4-block markdown: 3 passing + 1 failing
  integration/
    test_runner.kry  -- 7 integration tests (kryos run only)
```

## Limitation

The badge reflects only functions that carry `@capabilities(...)` annotations. Unannotated functions that call IO builtins are not detected. Annotate your cookbook examples or compile with `--strict-capabilities` to ensure full coverage.

Composes with: `kryos-sandbox-runner` (pre-execution cap enforcement), `kryos-doctor-caps` (project-wide audit).
