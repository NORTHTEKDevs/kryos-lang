#!/usr/bin/env bash
# demo/wasm/check.sh -- acceptance test for the WASM governed agent demo.
# Exit 0 = PASS. Run from the project root.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="$ROOT/compiler/target/release/kryos.exe"
NODE_RUNNER="$ROOT/tools/wasm-host/run.mjs"
WASM_OUT="$ROOT/demo/wasm/agent.wasm"

fail=0
pass() { echo "  PASS  $1"; }
fail_gate() { echo "  FAIL  $1: $2"; fail=1; }

# --- Compile to WASM ---
if "$KRYOS" build --release --backend wasm demo/wasm/main.kry -o "$WASM_OUT" >/dev/null 2>&1; then
    pass "wasm-compile"
else
    fail_gate "wasm-compile" "kryos build --backend wasm failed"
    exit 1
fi

# --- Run via Node host and assert governance properties ---
out=$(node "$NODE_RUNNER" "$WASM_OUT" 2>&1) || true

# (a) Within-budget call returns answer WITH its source
if echo "$out" | grep -q "WITHIN_BUDGET_SOURCE: mock-llm-v1" && \
   echo "$out" | grep -q "WITHIN_BUDGET_ANSWER:"; then
    pass "wasm-within-budget-answer-source"
else
    fail_gate "wasm-within-budget-answer-source" "expected WITHIN_BUDGET_SOURCE in output: $out"
fi

# (b) Over-budget call is refused with SPEND:0 (no mock LLM spend recorded)
if echo "$out" | grep -q "OVER_BUDGET_REFUSED: REFUSED:" && \
   echo "$out" | grep -q "OVER_BUDGET_SPEND: 0"; then
    pass "wasm-over-budget-refused-no-spend"
else
    fail_gate "wasm-over-budget-refused-no-spend" "expected OVER_BUDGET_REFUSED+SPEND:0 in output: $out"
fi

# (c) Overall PASS marker
if echo "$out" | grep -q "wasm: PASS"; then
    pass "wasm-pass-marker"
else
    fail_gate "wasm-pass-marker" "missing 'wasm: PASS' in output: $out"
fi

if [ "$fail" -eq 0 ]; then
    echo "demo/wasm: ALL CHECKS PASS"
    exit 0
fi
exit 1
