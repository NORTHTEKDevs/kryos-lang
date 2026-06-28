#!/usr/bin/env bash
# demo/mutation/check.sh
# Verifies in-place mutation of struct fields inside collections on both backends.
set -euo pipefail
REPO="$(cd "$(dirname "$0")/../.." && pwd)"
KRYOS="$REPO/compiler/target/release/kryos.exe"
STDLIB="$REPO/compiler/stdlib"
DEMO="$REPO/demo/mutation/demo.kry"

fail() { echo "FAIL: $*" >&2; exit 1; }

EXPECTED_LINES=(
    "STACK[0].score=99"
    "STACK[1].id=42"
    "STACK[1].score=20"
    "STACK[2].score=77"
    "HEAP[0].id=88"
    "HEAP[0].score=55"
    "HEAP[1].id=20"
    "HEAP[1].score=66"
    "MAP[alpha].id=1"
    "MAP[alpha].score=77"
    "MAP[beta].id=99"
    "MAP[beta].score=20"
    "mutation: PASS"
)

check_output() {
    local label="$1"
    local output="$2"
    for line in "${EXPECTED_LINES[@]}"; do
        if ! echo "$output" | grep -qF "$line"; then
            fail "$label: missing expected line: $line"
        fi
    done
    echo "  PASS  $label"
}

# JIT backend
JIT_OUT=$(KRYOS_STDLIB_DIR="$STDLIB" "$KRYOS" run "$DEMO" 2>&1) \
    || fail "kryos run exited non-zero"
check_output "jit" "$JIT_OUT"

# AOT backend
AOT_BIN="$REPO/demo/mutation/mutation_demo_check.exe"
KRYOS_STDLIB_DIR="$STDLIB" "$KRYOS" build --release "$DEMO" -o "$AOT_BIN" 2>&1 \
    || fail "kryos build --release failed"
AOT_OUT=$("$AOT_BIN" 2>&1) || fail "AOT binary exited non-zero"
check_output "aot" "$AOT_OUT"
rm -f "$AOT_BIN"

echo "PASS  demo-mutation"
