# AI-codeability benchmark — can an AI write Kryos as well as it writes mature languages?

This benchmark measures how well an AI model (Claude) writes **correct, first-attempt**
programs in Kryos vs Python, Rust, and Go. It exists to test a load-bearing claim:
*Kryos was designed so AI codes it well.* That claim is **not automatically true** — the
model has seen millions of Python/Rust/Go examples and ~zero Kryos. Language design pushes
one way; training-data familiarity pushes the other. This measures which wins, honestly.

## Method
- **12 "easy" + 8 "hard" tasks** (`tasks.json`), each defined by an **exact expected stdout**.
  Tasks are neutral and integer/string-only so they're expressible identically in all four
  languages and graded by exact string match.
- For each (task × language × 3 samples) a **fresh** Claude instance writes **one** program,
  **one shot, no compiling or iterating** (a clean pass@1 protocol).
  - **Kryos** instances first read the real `CLAUDE.md` language reference (the honest "docs
    provided" scenario — what a Kryos user actually ships).
  - **Python/Rust/Go** instances write from the model's own training knowledge (no docs).
- **Grading is deterministic** (`eval.py`): compile + run each program, compare trimmed stdout
  to the expected output. The AI never grades itself — pass numbers are executed, not reported.
- Same model (opus) for every cell, so the Kryos-vs-others comparison is internally valid.

## Result

| language | pass@1 | tasks solved |
|---|---|---|
| python | 100.0% | 20/20 |
| rust | 100.0% | 20/20 |
| go | 100.0% | 20/20 |
| **kryos (before ergonomics fixes)** | **78.3%** | **16/20** |
| **kryos (after ergonomics fixes)** | **93.3%** | **20/20** |

### What this shows (honestly)
- **Kryos is NOT yet "better" than mature languages on raw pass@1** — the mature languages
  ceiling at 100% on tasks this size. The honest headline is: **an AI writes Kryos at
  near-parity (93.3%) despite zero training data, from the docs alone.** For a language the
  model has never seen, that is a strong validation that the design is learnable-from-spec.
- **The benchmark is an engine, not a scoreboard.** Its real value is finding *where* AI trips
  on Kryos. The first run (78.3%) failed exactly 4 tasks, and the failures were precise:
  - `dedup_sort`, `histogram`: `contains(map, key)` was rejected on **int-keyed** maps (the
    type checker hard-typed the key arg to `str`) even though `CLAUDE.md` documents
    `contains(m, k)`. **Fix:** make `contains`'s key arg accept the map's key type.
  - `bracket_match`, `json_sum`: Kryos interpolates **every** string with `{ }`, so a bare `{`
    in `"([{}])"` or `"{\"a\":1}"` was parsed as interpolation. The AI wrote it as it would in
    any other language (where `{` is literal). **Fix:** `CLAUDE.md` now prominently teaches that
    all strings interpolate and literal braces need `{{`/`}}` or `\{`/`\}`.
  - Two targeted fixes (one compiler, one doc) lifted Kryos **78.3% → 93.3%** and **16/20 →
    20/20** in a single benchmark→fix cycle. That loop is repeatable.
- **Remaining stragglers (4 of 60 samples):** inconsistent brace-escaping in a few samples (the
  doc helps most but not every one-shot), and one use of `mut` as a variable name. Next lever:
  a clearer parser error for the brace case so an agentic compile-fix loop self-corrects — which
  is the more realistic way AI writes code than one-shot.

## Caveats
- One-shot pass@1 is the strict metric; it does **not** credit Kryos's compile-time safety
  (capabilities, types catching AI mistakes a Python program would run wrong). A follow-up tier
  should measure *bug-catching* (where the others "pass" but produce subtly-wrong-but-running
  code) — that's where Kryos's design is most likely to beat the others.
- Tasks are small; mature languages ceiling out. A harder tier is needed to discriminate among
  Python/Rust/Go and to stress Kryos further.
- Results are model- and date-specific (opus). Re-run to refresh.

## Reproduce
1. Generate: re-run the generation workflow (see the project history) or have an AI write one
   program per `tasks.json` entry per language into `solutions/<task>__<lang>__s<n>.<ext>`.
2. Evaluate: `KRYOS_BIN=/path/to/kryos.exe python3 eval.py` → prints the table, writes
   `results.json`. Requires `python3`, `rustc`, `go`, and a Kryos build on the machine.
`solutions/` is gitignored (regenerable AI output); `results.json` is the committed snapshot.
