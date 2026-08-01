#!/usr/bin/env bash
# fuzz_gate.sh -- bounded differential JIT/AOT fuzz gate for CI.
#
# Runs a fixed, deterministic seed range through tests/fuzz/run_diff.py and
# fails if any case diverges (stdout, exit code, or one backend failing to
# build/link where the other succeeds). Bounded so it stays fast in CI;
# for a real hunting session run run_diff.py directly with a much larger
# --seeds range (see tests/fuzz/README.md).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

PY="${PYTHON:-python}"
command -v "$PY" >/dev/null 2>&1 || PY=python3

SEEDS="${FUZZ_GATE_SEEDS:-1-40}"

echo "== differential fuzz gate: seeds $SEEDS =="
"$PY" tests/fuzz/run_diff.py --seeds "$SEEDS"
rc=$?
if [ $rc -eq 0 ]; then
    echo "fuzz_gate: PASS (no divergence across seeds $SEEDS)"
else
    echo "fuzz_gate: FAIL -- divergence found; replay with:"
    echo "  python tests/fuzz/gen_fuzz.py --seed <N> -o repro.kry"
    echo "  python tests/fuzz/shrink.py repro.kry -o minimal.kry"
fi
exit $rc
