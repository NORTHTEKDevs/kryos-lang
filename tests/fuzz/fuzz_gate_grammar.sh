#!/usr/bin/env bash
# fuzz_gate_grammar.sh -- bounded, deterministic CI gate for the
# combined-category grammar fuzzer (tests/fuzz/gen_grammar.py +
# run_diff_grammar.py). Complements fuzz_gate.sh (the independent-block
# template harness): this one deliberately hits generics x closures x dyn x
# spawn/actors IN COMBINATION, so it is bounded to a small seed range for CI
# (build+link across 2 backends per case dominates runtime) -- run
# run_diff_grammar.py directly with a much larger --seeds range for a real
# hunting session (see tests/fuzz/README.md).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

PY="${PYTHON:-python}"
command -v "$PY" >/dev/null 2>&1 || PY=python3

SEEDS="${FUZZ_GATE_GRAMMAR_SEEDS:-1-15}"

echo "== combined-category grammar fuzz gate: seeds $SEEDS, all scenarios =="
"$PY" tests/fuzz/run_diff_grammar.py --scenarios all --seeds "$SEEDS"
rc=$?
if [ $rc -eq 0 ]; then
    echo "fuzz_gate_grammar: PASS (no divergence, seeds $SEEDS x all scenarios)"
else
    echo "fuzz_gate_grammar: FAIL -- divergence found; replay with:"
    echo "  python tests/fuzz/gen_grammar.py --seed <N> --scenario <name> -o repro.kry"
    echo "  python tests/fuzz/shrink.py repro.kry -o minimal.kry"
fi
exit $rc
