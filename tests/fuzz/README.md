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
