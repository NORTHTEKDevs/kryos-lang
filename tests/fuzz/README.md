# Differential JIT/AOT fuzzing harness

Generates random-but-valid Kryos programs and diffs `kryos run` (Cranelift
JIT) against `kryos build --release` (LLVM AOT). Any difference in stdout,
exit code, or build/link success is a bug: both backends compile the same
MIR, so if they disagree, one of them is silently wrong.

## Files

- `gen_fuzz.py` -- the generator. `(seed, blocks)` deterministically produces
  one `.kry` program: a fixed prelude (a scalar struct, a heap-field struct,
  a generic struct, an enum, one throwing helper) plus `blocks` randomly
  chosen "block" functions, one per category below. Each block returns a
  deterministic `i64`; `main` prints one tagged line per block, XORs the
  value into a running checksum, and prints the checksum last.
- `run_diff.py` -- the driver. Generates a case (or a seed range), runs both
  backends, diffs stdout + exit code, reports divergences.
- `shrink.py` -- delta-debugging (ddmin) reducer. Takes a `.kry` file that
  diverges and deletes lines while the divergence (same kind: build failure,
  exit-code mismatch, or stdout mismatch) persists, checking `kryos check`
  after every candidate deletion so the result stays syntactically valid.
- `fuzz_gate.sh` -- bounded CI gate (default seeds `1-40`, ~40s). Wired into
  the loop driver's tier-1 gates are NOT required to include this (it is
  intentionally separate so a slow fuzz sweep never blocks the fast gate
  ladder) -- run it explicitly, or set `FUZZ_GATE_SEEDS` for a different
  range.
- `repros/` -- minimal repros for confirmed divergences, one `.kry` file
  each, with the seed and root cause noted in a header comment.

## Categories covered

Integer arithmetic/casts, float arithmetic/casts, string ops
(concat/interpolate/substr/contains/compare), arrays (push/sort/index/for),
maps (insert/lookup/contains), scalar structs + methods, heap-field structs
passed across call boundaries, enums + match (including or-patterns), direct
closures (mutation, currying), `std::iter` HOF closures (map/filter/fold),
generics (a generic struct instantiated at `i64` and `str`), control flow
(while/loop+break/for range/if-elif-else), and try/throw (caught, value
derived from the message).

Deliberately excluded: constructs CLAUDE.md already documents as a known,
understood cross-backend divergence (NaN sign bits, parsed `-0.0`, a closure
escaping through a container then reading a mutated heap capture, a generic
function returning an `f64`-typed closure, `dyn Trait` in a container,
`i128`/`u128`, concurrency/`spawn`). Generating those would just re-report
a cataloged limitation, not find something new -- the whole point of this
harness is NEW divergences.

## Usage

```bash
# One case, verbose:
python tests/fuzz/run_diff.py --seed 12345 -v

# A sweep:
python tests/fuzz/run_diff.py --seeds 1-500

# Keep the generated sources (not just on divergence):
python tests/fuzz/run_diff.py --seeds 1-20 --keep-dir /tmp/fuzzkeep

# Bounded CI gate:
bash tests/fuzz/fuzz_gate.sh
FUZZ_GATE_SEEDS=1-200 bash tests/fuzz/fuzz_gate.sh
```

## Replay a specific case

Every case is fully determined by `(seed, blocks)`. `run_diff.py --seeds`
uses `gen_fuzz.py`'s default block count (`12 + seed % 19`) unless
`--blocks` is passed explicitly.

```bash
python tests/fuzz/gen_fuzz.py --seed 4821 -o repro.kry
compiler/target/release/kryos.exe run repro.kry
compiler/target/release/kryos.exe build --release repro.kry -o repro.exe && ./repro.exe
```

## Shrinking a found divergence

```bash
python tests/fuzz/shrink.py repro.kry -o minimal.kry
```

`shrink.py` also accepts `--seed`/`--blocks` directly (generates first, then
shrinks) instead of a pre-generated file. It has been validated against a
known, still-open divergence (`parse_float("-0.0")`'s sign, CLAUDE.md gotcha
#18) as a self-test: given a 10-line program with that divergence buried
among unrelated noise, it reduces to the exact 4-line minimal repro in well
under a second.

## Adding a category

Add a `tmpl_<name>(rng) -> str` function to `gen_fuzz.py` returning a
function BODY (no wrapping `fn`/`{}`) that ends in `return <i64 expr>`, and
add `("<name>", tmpl_<name>)` to the `TEMPLATES` list. Keep new templates
deterministic (no clock/env/random reads, no concurrency) and avoid
constructs already on the "deliberately excluded" list above unless you are
specifically trying to re-confirm one is still fixed.

---

## Combined-category grammar fuzzer (`gen_grammar.py`)

`gen_fuzz.py` above is deliberately BLOCK-INDEPENDENT (each category is its
own `fn` with no shared state, no spawn, no dyn, shallow generics) -- great
for cheap shrinking, but it structurally cannot find a bug that needs
CROSS-CATEGORY interaction (a generic struct holding a closure holding an
array holding a struct, dyn dispatch feeding a closure feeding a generic
struct, spawn workers feeding an enum payload, ...). `gen_grammar.py` +
`run_diff_grammar.py` fill that gap.

### Files

- `gen_grammar.py` -- 9 scenario builders (`generic_multi_type`,
  `closure_curry_escape`, `dyn_trait_story`, `spawn_channels`,
  `actor_story`, `enum_option_result_tuple`, `mega_combo`,
  `generic_fn_multi_inst`, `narrow_cast_boundaries`), each a CONNECTED
  data-flow story (not independent blocks) built on top of `ExprGen`, a real
  recursive expression grammar (random operator/operand/depth choice at
  every node, including nested boundary casts and nested `{ if .. } else
  { .. }`-valued blocks). `--scenario all` shuffles and concatenates every
  scenario into one program. HONEST SCOPE: the expression layer is a real
  grammar; the surrounding statement/declaration scaffolding is scenario-
  based, not a fully free statement grammar -- see the module docstring for
  why that tradeoff was made deliberately.
- `run_diff_grammar.py` -- same stdout+exit-code diff contract as
  `run_diff.py`, plus a third `both-fail` bucket: when BOTH backends reject
  a program identically (not a stdout divergence, since there's nothing to
  diff), it's reported separately rather than silently discarded -- this is
  exactly the shape that found and closed a real bug this round (see
  LEDGER, "capability provenance checker false-rejects a closure call
  inside a bare block").
- `fuzz_gate_grammar.sh` -- bounded CI gate (default seeds `1-15`, all 9
  scenarios + `all`, ~2.5 min). `FUZZ_GATE_GRAMMAR_SEEDS` overrides the
  range.

### Usage

```bash
python tests/fuzz/run_diff_grammar.py --scenarios all --seeds 1-200
python tests/fuzz/run_diff_grammar.py --scenario mega_combo --seed 12345 -v
python tests/fuzz/run_diff_grammar.py --scenarios all --seeds 1-50 --keep-dir out/ --report-both-fail
bash tests/fuzz/fuzz_gate_grammar.sh
FUZZ_GATE_GRAMMAR_SEEDS=1-100 bash tests/fuzz/fuzz_gate_grammar.sh
```

Replay a specific case:

```bash
python tests/fuzz/gen_grammar.py --seed 4821 --scenario mega_combo -o repro.kry
compiler/target/release/kryos.exe run repro.kry
compiler/target/release/kryos.exe build --release repro.kry -o repro.exe && ./repro.exe
```

Shrinking works exactly the same way as for `gen_fuzz.py` -- `shrink.py`
operates on any materialized `.kry` file via `kryos check` and the same
JIT/AOT diff, independent of which generator produced it:

```bash
python tests/fuzz/shrink.py repro.kry -o minimal.kry
```

### One real bug found and fixed while building this (not a JIT/AOT
divergence -- both backends rejected identically, so `run_diff_grammar.py`'s
own stdout diff never fires; found by hand from the `both-fail` bucket,
exactly like the `Box_` trailing-underscore bug the block-template harness
found the same way)

The capability provenance checker false-rejected a zero-capability closure
call when the closure was defined and called inside a bare `{ }` scoping
block or a `let x = { .. }` block-tail-value, forcing `@capabilities(all)`
on ordinary code. See `tools/loop/LEDGER.md`'s CLOSED table for the full
root cause and fix (`kryos-capabilities/src/checker.rs`), and
`tests/conformance/conf_closure_block_scope_caps.kry` for the regression. A
third, narrower, deeper instance (calling the chained return of a generic
passthrough accessor method) was found and deliberately left open with a
documented workaround -- see LEDGER item 20.

### Result (this wave, 2026-08-04)

**1,600 cases (seeds 1-160, 9 scenarios + the shuffled `all`-combo = 10
variants/seed) post-fix: 0 divergences, 0 both-fail, 0.00% divergence
rate.** Run as bounded, fully-captured batches (`python -u`, unbuffered) so
every run's result is a real, verified summary line -- an initial unbounded
seeds-16-300 attempt was killed by session limits before its buffered
output flushed and is correctly NOT counted (an unflushed/killed run is not
evidence). Also re-ran the existing `gen_fuzz.py`/`run_diff.py` template
harness at seeds 1-300 as a regression check on the shared
`kryos-capabilities` checker change this wave made: 300/300 match, 0
divergences. Also ran `tools/diff-fuzz/memsafety_fuzz.py`
(`KRYOS_FREE_DIAG`) for 400 cases: 0 with double-free.

Honest caveat, same as `gen_fuzz.py`'s own: this is a positive signal for
the shapes these 9 scenarios happen to hit, not proof the combined surface
is divergence-free in general -- run a much larger `--seeds` range (in
bounded, unbuffered batches, not one giant unbounded call) for a real
hunting session.
