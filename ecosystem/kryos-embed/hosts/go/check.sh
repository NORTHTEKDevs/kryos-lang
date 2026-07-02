#!/usr/bin/env bash
# check.sh -- run the Go CRM-assistant demo and assert decisive output lines
# Run from the REPO ROOT:
#   bash ecosystem/kryos-embed/hosts/go/check.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "[check] Running Go kryos-embed host demo"
OUTPUT=$(cd "$SCRIPT_DIR" && go run . 2>&1)
echo "$OUTPUT"
echo ""

# Assert decisive lines are present in output
assert_contains() {
    local label="$1"
    local needle="$2"
    if echo "$OUTPUT" | grep -qF "$needle"; then
        echo "ASSERT PASS: $label"
    else
        echo "ASSERT FAIL: $label"
        echo "  expected to find: $needle"
        exit 1
    fi
}

assert_contains "doctored manifest refused"  "PASS: doctored manifest refused before DLL load"
assert_contains "DLL loaded"                 "PASS: DLL loaded"
assert_contains "within-budget answered=1"   "answered=1"
assert_contains "within-budget spend=3"      "spend_cents=3"
assert_contains "within-budget PASS"         "PASS: answered=1, source present, spend_cents=3"
assert_contains "over-budget answered=0"     "answered=0"
assert_contains "over-budget spend=0"        "spend_cents=0"
assert_contains "over-budget PASS"           "PASS: answered=0, spend_cents=0, reason present"
assert_contains "all assertions"             "ALL ASSERTIONS PASSED"

echo ""
echo "check.sh: ALL ASSERTIONS PASSED"
