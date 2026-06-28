#!/usr/bin/env bash
# demo/native/check.sh -- acceptance test for the native governed agent demo.
# Exit 0 = PASS. Run from the project root (night-acceptance.sh sets ROOT + KRYOS_STDLIB_DIR).
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="$ROOT/compiler/target/release/kryos.exe"

fail=0
pass() { echo "  PASS  $1"; }
fail_gate() { echo "  FAIL  $1: $2"; fail=1; }

# (a)+(b) JIT backend: kryos run
out_jit=$("$KRYOS" run demo/native/main.kry 2>&1) || true
if echo "$out_jit" | grep -q "WITHIN_BUDGET_SOURCE: mock-llm-v1" && \
   echo "$out_jit" | grep -q "OVER_BUDGET_REFUSED: REFUSED:" && \
   echo "$out_jit" | grep -q "OVER_BUDGET_SPEND: 0" && \
   echo "$out_jit" | grep -q "native: PASS"; then
    pass "native-jit"
else
    fail_gate "native-jit" "unexpected output: $out_jit"
fi

# (a)+(b) AOT backend: kryos build --release
aot_bin="/tmp/kryos_native_aot_$$.exe"
if "$KRYOS" build --release demo/native/main.kry -o "$aot_bin" >/dev/null 2>&1; then
    out_aot=$("$aot_bin" 2>&1) || true
    if echo "$out_aot" | grep -q "WITHIN_BUDGET_SOURCE: mock-llm-v1" && \
       echo "$out_aot" | grep -q "OVER_BUDGET_REFUSED: REFUSED:" && \
       echo "$out_aot" | grep -q "OVER_BUDGET_SPEND: 0" && \
       echo "$out_aot" | grep -q "native: PASS"; then
        pass "native-aot"
    else
        fail_gate "native-aot" "unexpected output: $out_aot"
    fi
    rm -f "$aot_bin"
else
    fail_gate "native-aot" "build failed"
fi

# (c) Negative control: missing capability must fail E0507 under --strict-capabilities
neg_out=$("$KRYOS" check --strict-capabilities demo/native/neg_cap.kry 2>&1 || true)
neg_exit=0
"$KRYOS" check --strict-capabilities demo/native/neg_cap.kry >/dev/null 2>&1 && neg_exit=0 || neg_exit=$?
if [ "$neg_exit" -ne 0 ] && echo "$neg_out" | grep -q "E0507"; then
    pass "native-neg-cap-E0507"
else
    fail_gate "native-neg-cap-E0507" "expected E0507 compile failure (exit=$neg_exit, out=$neg_out)"
fi

if [ "$fail" -eq 0 ]; then
    echo "demo/native: ALL CHECKS PASS"
    exit 0
fi
exit 1
