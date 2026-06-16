# kryos-notebook-caps

**Project 40** - A capability-badged literate-execution notebook for Kryos.

A notebook is an ordinary markdown file with fenced ` ```kryos ` cells. This
tool runs each cell in order with state threaded between them (the repl
accumulation model), and renders an output notebook where **every cell carries
its compiler-verified capability badge**.

## Why Kryos

Kryos will not out-compete Jupyter on the data-science ecosystem. The narrow,
defensible win is *legibility of risk*: a Jupyter cell can do anything, and a
reader has no idea which cells are dangerous before running them. A Kryos
notebook is self-describing - each cell renders the static capability surface
of its code, so a shared notebook tells you, per cell, which ones touch
`io` / `net` / `process`. A `--pure-only` mode then runs only the
compute-badged cells and skips the rest, so you can execute the safe subset of
an untrusted notebook without its side-effecting cells.

The badge is not a heuristic. It comes from the compiler: each cell is checked
with `kryos check --strict-capabilities` (deny-by-default), which reports every
capability-gated builtin the cell's code reaches. No annotations required - the
inference is the compiler's.

## How it works

For a notebook with cells `c0, c1, ..., cn`:

1. **Badge** (one pass). All cell bodies are assembled into a single `main()`
   (so a `let` in `c0` is visible in `c1`), and `kryos check
   --strict-capabilities` is run once. Each `E0505` diagnostic names a required
   capability and a source line; the line is mapped back to its owning cell.
   A cell with no gated calls is `[compute]` (pure).

2. **Execute** (accumulate-and-rerun). For cell `i`, the accumulated program of
   executed cells `0..=i` is run with `kryos run`, and cell `i`'s own stdout is
   recovered by stripping the previous run's stdout as a prefix. State threads
   because the whole program is recompiled and rerun each step.

3. **Render**. Prose is emitted verbatim; each cell is annotated with its badge,
   its source, and its captured output (or a skipped/error note).

`--pure-only` excludes non-`[compute]` cells from execution entirely and marks
them skipped - their side effects never run, while later pure cells still see
the threaded state of earlier pure cells.

## Usage

```bash
# render to stdout
kryos run ecosystem/kryos-notebook-caps/src/notebook.kry -- <notebook.md>

# only run the safe (compute) cells; skip io/net/process cells
kryos run ecosystem/kryos-notebook-caps/src/notebook.kry -- --pure-only <notebook.md>

# write the rendered notebook to a file
kryos run ecosystem/kryos-notebook-caps/src/notebook.kry -- -o out.md <notebook.md>

# build a standalone binary (program args need no `--` separator then)
kryos build --release ecosystem/kryos-notebook-caps/src/notebook.kry -o kryos-notebook-caps
./kryos-notebook-caps --pure-only <notebook.md>
```

> Note the `--` separator when invoking via `kryos run`: it stops `kryos run`
> from interpreting `--pure-only` / `-o` as its own flags and forwards them to
> the program. A built binary does not need it.

The tool shells out to the `kryos` toolchain. It resolves the binary from
`$KRYOS`, falling back to `kryos` on `PATH`.

## Notebook format

````markdown
# My analysis

Some prose.

```kryos
let total: i64 = 2 + 40
println("total = " + to_string(total))
```

```kryos
file_write("report.txt", "total is " + to_string(total))
```
````

Cells are statement-level (like repl input): bodies share one `main()` scope,
and `use ...` lines are hoisted and de-duplicated above it. Fenced blocks with
any other tag (e.g. ` ```bash `) are preserved as prose, never executed.

## Tests

```bash
# Pure unit tests (cell parsing, program assembly, diagnostic parsing,
# rendering) -- kryos test compatible, all compute, no subprocesses.
kryos test --path ecosystem/kryos-notebook-caps/tests

# End-to-end integration (real `kryos check` + `kryos run` subprocesses).
# Must run via `kryos run` from the project root, not `kryos test`: Command.run
# needs the ptr_read_i64 builtin the test JIT does not expose.
kryos run ecosystem/kryos-notebook-caps/integration/test_notebook.kry
```

The integration test asserts the spec's done criteria against
`integration/fixtures/demo.md`: a pure cell badges `[compute]` and an io cell
badges `[io]`; under `--pure-only` exactly the two compute cells run and the io
cell is skipped; and threaded state survives (`n` defined in cell 0 is visible
in cell 2 as `n * 2 == 42`, even with the io cell skipped).

## Demo

```bash
kryos run ecosystem/kryos-notebook-caps/demo_notebook.kry
```

Renders the same notebook in full and in `--pure-only` mode side by side, so you
can see the io cell's file write happen in one and not the other.

## Project structure

```
ecosystem/kryos-notebook-caps/
  kryos.toml
  src/
    cells.kry       -- markdown -> ordered prose/code segments (PURE)
    program.kry     -- assemble accumulated main() + per-cell line ranges (PURE)
    badge.kry       -- parse strict-capabilities diagnostics -> per-cell badges (PURE)
    render.kry      -- render the annotated output notebook (PURE)
    engine.kry      -- execution engine: badge + run each cell (process, io)
    notebook.kry    -- CLI entry point (main)
  tests/            -- kryos test --path tests (all pure compute)
    test_cells.kry  test_program.kry  test_badge.kry  test_render.kry
  integration/
    test_notebook.kry  -- run via: kryos run integration/test_notebook.kry
    fixtures/demo.md   -- the pure + io + threaded-state fixture
  demo_notebook.kry -- interactive full vs pure-only demo
```

The execution logic lives in `engine.kry` (importable) so the integration tests
can drive it without colliding with `notebook.kry`'s `main()`.

## Limitations and risks (honest)

- **Quadratic execution.** This is the accumulate-and-recompile-per-cell model.
  Badging is a single check, but execution reruns the growing accumulated
  program once per executed cell, so wall time is `O(n^2)` in cell count and
  every io/net cell re-runs its side effects on each later cell's pass. Fine for
  human-sized notebooks; incremental recompute is deliberately out of scope.
  This is a DX/credibility piece for existing Kryos users, not a pull on Python
  data scientists.
- **Determinism assumption.** Per-cell stdout is recovered by prefix-diffing
  successive accumulated runs, which assumes deterministic, append-only output
  (no clock/random reordering between runs).
- **Statement-level cells.** Cells are repl-style statements sharing a `main()`
  scope, not top-level `fn`/`struct` declarations (`use` lines excepted). A
  pure cell that depends on a `let` defined in a skipped io cell will fail to
  compile under `--pure-only` (surfaced as that cell's error).
- **AOT note.** The engine returns results as parallel `[str]` arrays
  (`NotebookRun`) and materialises one `RenderCell` at a time, rather than
  building an array of result structs in the execution loop. This sidesteps an
  LLVM-backend issue where computed (substr-derived) strings stored into struct
  fields of an array element were dropped. Both `kryos run` (JIT) and `kryos
  build --release` (AOT) produce identical, correct output.

## Out of scope (MVP)

Rich output (plots/tables), a live kernel/protocol, out-of-order re-execution,
data-science libraries, and incremental recompute.

Composes with: `kryos-sandbox-runner` (project 26, the pure-execution sandbox),
`kryos-cookbook-runner` (project 30), and `kryos repl`.
