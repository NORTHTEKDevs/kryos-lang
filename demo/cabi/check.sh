#!/usr/bin/env bash
# demo/cabi/check.sh -- acceptance test for the C-ABI governed agent demo.
# Exit 0 = PASS. Run from the project root.
#
# Recipe:
#   1. Emit LLVM IR from agent_lib.kry
#   2. Patch IR: dllexport agent_query_c (Kryos emits all fns as `internal`)
#   3. Link into agent_lib.dll with zig cc + kryos runtime libs
#   4. Build harness.exe (C host) with zig cc
#   5. Run harness.exe, assert governance properties hold across the ABI boundary
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="$ROOT/compiler/target/release/kryos.exe"
KRYOS_RT="$ROOT/compiler/target/release/kryos_rt.lib"
KRYOS_STD="$ROOT/compiler/target/release/kryos_stdlib_native.lib"
TMP_DIR="${TEMP:-/tmp}"
AGENT_LL="$TMP_DIR/kryos_cabi_agent_$$.ll"
AGENT_LL_EXP="$TMP_DIR/kryos_cabi_agent_exp_$$.ll"
AGENT_DLL="$TMP_DIR/kryos_cabi_agent_$$.dll"
HARNESS_EXE="$TMP_DIR/kryos_cabi_harness_$$.exe"

fail=0
pass() { echo "  PASS  $1"; }
fail_gate() { echo "  FAIL  $1: $2"; fail=1; }
cleanup() { rm -f "$AGENT_LL" "$AGENT_LL_EXP" "$AGENT_DLL" "$HARNESS_EXE"; }
trap cleanup EXIT

# --- Step 1: emit LLVM IR ---
if "$KRYOS" build --release --emit-llvm demo/cabi/agent_lib.kry -o "$AGENT_LL" >/dev/null 2>&1; then
    pass "cabi-emit-llvm"
else
    fail_gate "cabi-emit-llvm" "kryos build --emit-llvm failed"
    exit 1
fi

# --- Step 2: patch IR to export agent_query_c ---
sed 's/define internal i64 @agent_query_c/define dllexport i64 @agent_query_c/' \
    "$AGENT_LL" > "$AGENT_LL_EXP"
if grep -q "define dllexport i64 @agent_query_c" "$AGENT_LL_EXP"; then
    pass "cabi-patch-ir"
else
    fail_gate "cabi-patch-ir" "agent_query_c not found/patched in IR"
    exit 1
fi

# --- Step 3: link DLL ---
if zig cc -target x86_64-windows-msvc -shared -o "$AGENT_DLL" "$AGENT_LL_EXP" \
        "$KRYOS_RT" "$KRYOS_STD" \
        -lntdll -luserenv -lws2_32 -ldbghelp -luser32 -lbcrypt -ladvapi32 \
        -Wno-override-module >/dev/null 2>&1; then
    pass "cabi-link-dll"
else
    fail_gate "cabi-link-dll" "zig cc DLL link failed"
    exit 1
fi

# --- Step 4: build C harness ---
if zig cc -target x86_64-windows-msvc -o "$HARNESS_EXE" demo/cabi/harness.c \
        -Wno-unknown-warning-option >/dev/null 2>&1; then
    pass "cabi-build-harness"
else
    fail_gate "cabi-build-harness" "zig cc harness build failed"
    exit 1
fi

# --- Step 5: run and assert governance properties ---
out=$("$HARNESS_EXE" "$AGENT_DLL" 2>&1) || true

# (a) Within-budget call: answer + source present, return code 1
if echo "$out" | grep -q "WITHIN_BUDGET_ANSWER:" && \
   echo "$out" | grep -q "WITHIN_BUDGET_SOURCE: mock-llm-v1" && \
   echo "$out" | grep -q "C: within-budget result=1"; then
    pass "cabi-within-budget-answer-source"
else
    fail_gate "cabi-within-budget-answer-source" "expected WITHIN_BUDGET_ANSWER+SOURCE+result=1 in: $out"
fi

# (b) Over-budget call: refused before spend, return code 0
if echo "$out" | grep -q "OVER_BUDGET_REFUSED: REFUSED:" && \
   echo "$out" | grep -q "OVER_BUDGET_SPEND: 0" && \
   echo "$out" | grep -q "C: over-budget result=0"; then
    pass "cabi-over-budget-refused-no-spend"
else
    fail_gate "cabi-over-budget-refused-no-spend" "expected OVER_BUDGET_REFUSED+SPEND:0+result=0 in: $out"
fi

# (c) Overall PASS marker
if echo "$out" | grep -q "cabi: PASS"; then
    pass "cabi-pass-marker"
else
    fail_gate "cabi-pass-marker" "missing 'cabi: PASS' in output: $out"
fi

if [ "$fail" -eq 0 ]; then
    echo "demo/cabi: ALL CHECKS PASS"
    exit 0
fi
exit 1
