#!/usr/bin/env bash
# ecosystem/kryos-embed/check.sh
#
# End-to-end integration runner for the kryos-embed SDK.
# Run from the REPO ROOT:
#   bash ecosystem/kryos-embed/check.sh
#
# What it does:
#   Stage 0 -- build.sh (caps check, DLL link, manifest)
#   Stage 1 -- hosts/python/check.sh
#   Stage 2 -- hosts/go/check.sh
#   Stage 3 -- hosts/node/check.sh  (includes WASM compile)
#
# For hosts that require runtimes not installed on this machine:
#   hosts/csharp/ -- recipe-only; marked SKIP automatically if dotnet is absent
#
# Exit 0 only when every present (non-skipped) stage passes.
# A SKIP is not a failure; it is printed clearly so the reviewer knows
# the stage was excluded rather than faked.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"

EMBED_DIR="$ROOT/ecosystem/kryos-embed"

pass=0
fail=0
skip=0

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
run_stage() {
    local label="$1"
    local script="$2"
    local timeout_secs="${3:-120}"

    echo ""
    echo "================================================================"
    echo "STAGE: $label"
    echo "================================================================"

    if [ ! -f "$script" ]; then
        echo "SKIP -- $script not found"
        skip=$((skip + 1))
        return
    fi

    local out
    local rc=0
    out=$(timeout "$timeout_secs" bash "$script" 2>&1) || rc=$?

    # Print the full output so the reviewer can audit
    echo "$out"
    echo ""

    if [ "$rc" -eq 0 ]; then
        echo "PASS  $label"
        pass=$((pass + 1))
    elif [ "$rc" -eq 124 ]; then
        echo "FAIL  $label (timeout after ${timeout_secs}s)"
        fail=$((fail + 1))
    else
        echo "FAIL  $label (exit $rc)"
        fail=$((fail + 1))
    fi
}

skip_stage() {
    local label="$1"
    local reason="$2"
    echo ""
    echo "================================================================"
    echo "STAGE: $label -- SKIP"
    echo "  $reason"
    echo "================================================================"
    skip=$((skip + 1))
}

# ---------------------------------------------------------------------------
# Stage 0: build.sh -- caps check + DLL link + manifest
# ---------------------------------------------------------------------------
run_stage "build (caps-check + DLL + manifest)" \
    "$EMBED_DIR/build.sh" \
    180

# ---------------------------------------------------------------------------
# Stage 1: Python host
# ---------------------------------------------------------------------------
if command -v python >/dev/null 2>&1 || command -v python3 >/dev/null 2>&1; then
    run_stage "python host" \
        "$EMBED_DIR/hosts/python/check.sh" \
        60
else
    skip_stage "python host" "python not found on PATH"
fi

# ---------------------------------------------------------------------------
# Stage 2: Go host
# ---------------------------------------------------------------------------
if command -v go >/dev/null 2>&1; then
    run_stage "go host" \
        "$EMBED_DIR/hosts/go/check.sh" \
        60
else
    skip_stage "go host" "go not found on PATH"
fi

# ---------------------------------------------------------------------------
# Stage 3: Node/WASM host (includes kryos WASM compile)
# ---------------------------------------------------------------------------
if command -v node >/dev/null 2>&1; then
    run_stage "node/wasm host" \
        "$EMBED_DIR/hosts/node/check.sh" \
        90
else
    skip_stage "node/wasm host" "node not found on PATH"
fi

# ---------------------------------------------------------------------------
# Stage 4: C# host -- recipe-only; skip unless dotnet is installed
# ---------------------------------------------------------------------------
if command -v dotnet >/dev/null 2>&1; then
    if [ -f "$EMBED_DIR/hosts/csharp/check.sh" ]; then
        run_stage "csharp host" \
            "$EMBED_DIR/hosts/csharp/check.sh" \
            60
    else
        skip_stage "csharp host" "check.sh not present (recipe-only host)"
    fi
else
    skip_stage "csharp host" ".NET SDK (dotnet) not installed -- recipe-only, untested on this machine"
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo "================================================================"
echo "kryos-embed check.sh summary"
echo "  PASS: $pass"
echo "  FAIL: $fail"
echo "  SKIP: $skip"
echo "================================================================"

if [ "$fail" -gt 0 ]; then
    echo "RESULT: FAIL ($fail stage(s) failed)"
    exit 1
fi

echo "RESULT: PASS (all present stages passed; $skip skipped)"
exit 0
