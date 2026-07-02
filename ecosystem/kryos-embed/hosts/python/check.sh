#!/usr/bin/env bash
# check.sh -- runs kryos_agent.py + demo_crm.py and asserts decisive output lines.
# Exit 0 only if ALL assertions pass.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PYTHON="${PYTHON:-python}"

pass=0
fail=0

run_and_check() {
    local label="$1"
    local script="$2"
    shift 2
    local patterns=("$@")

    echo ""
    echo "=== $label ==="
    local out
    out=$("$PYTHON" "$SCRIPT_DIR/$script" 2>&1)
    echo "$out"

    local ok=1
    for pat in "${patterns[@]}"; do
        if echo "$out" | grep -qF "$pat"; then
            echo "  [ASSERT PASS] found: $pat"
        else
            echo "  [ASSERT FAIL] missing: $pat"
            ok=0
        fi
    done

    if [ "$ok" -eq 1 ]; then
        pass=$((pass + 1))
    else
        fail=$((fail + 1))
    fi
}

# ---- kryos_agent.py (capability gate proof + basic ask) ----
run_and_check \
    "kryos_agent.py" \
    "kryos_agent.py" \
    "PROOF: doctored manifest with 'net:tcp' was REFUSED before DLL load -- PASS" \
    "answered=1" \
    "source="

# ---- demo_crm.py (CRM demo: within-budget, over-budget, cap-gate) ----
run_and_check \
    "demo_crm.py" \
    "demo_crm.py" \
    "answered=1" \
    "source=" \
    "PASS: answered=1, source present, spend=3" \
    "answered=0" \
    "spend_cents: 0" \
    "PASS: answered=0 (refusal), spend_cents=0 (no charge on refusal)" \
    "CapabilityViolation raised" \
    "PASS: DLL was NOT loaded when allowed_caps=[] (ffi excluded)"

# ---- Summary ----
echo ""
echo "========================================"
echo "check.sh results: $pass passed, $fail failed"
echo "========================================"

if [ "$fail" -gt 0 ]; then
    echo "FAIL"
    exit 1
fi

echo "ALL ASSERTIONS PASSED"
exit 0
