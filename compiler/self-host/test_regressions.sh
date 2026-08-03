#!/usr/bin/env bash
# test_regressions.sh -- value-asserted regressions for bugs found through
# self-host source that turned out to be general compiler/runtime bugs (not
# stage-1-bootstrap-specific), run directly against the NATIVE (stage-0)
# kryos.exe on both backends. Distinct from test_bootstrap.sh (stage-1
# self-compiling stage-1's own source) and test_examples.sh (stage-1
# compiling example programs) -- this script never touches stage-1.
#
# Each entry here asserts its own expected values internally (panics with a
# clear message on mismatch) and must exit 0 AND print no
# KRYOS-FREE-DIAG double-free line under KRYOS_FREE_DIAG=1, on BOTH
# `kryos run` (Cranelift/JIT) and `kryos build --release` (LLVM/AOT).
set -u
cd "$(dirname "$0")"

KRYOS=../target/release/kryos.exe
[ -x "$KRYOS" ] || KRYOS=../target/release/kryos

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail=0

check_one() {
    local name="$1" file="$2"

    local jit_out jit_rc
    jit_out=$(KRYOS_FREE_DIAG=1 "$KRYOS" run "$file" 2>&1)
    jit_rc=$?
    if [ $jit_rc -ne 0 ] || printf '%s' "$jit_out" | grep -q "DOUBLE-FREE\|kryos panic\|corrupt array"; then
        echo "  FAIL (JIT) $name  rc=$jit_rc"
        printf '%s\n' "$jit_out" | tail -6 | sed 's/^/    /'
        fail=$((fail+1))
        return
    fi

    local exe="$TMP/${name}.exe"
    local build_out
    build_out=$("$KRYOS" build --release "$file" -o "$exe" 2>&1)
    if [ ! -x "$exe" ]; then
        echo "  FAIL (AOT build) $name"
        printf '%s\n' "$build_out" | tail -6 | sed 's/^/    /'
        fail=$((fail+1))
        return
    fi
    local aot_out aot_rc
    aot_out=$(KRYOS_FREE_DIAG=1 "$exe" 2>&1)
    aot_rc=$?
    if [ $aot_rc -ne 0 ] || printf '%s' "$aot_out" | grep -q "DOUBLE-FREE\|kryos panic\|corrupt array"; then
        echo "  FAIL (AOT) $name  rc=$aot_rc"
        printf '%s\n' "$aot_out" | tail -6 | sed 's/^/    /'
        fail=$((fail+1))
        return
    fi

    echo "  PASS $name"
}

echo "Self-host-derived regressions (native kryos.exe, both backends):"
check_one "lexer_reentrant_tokenize" regression_lexer_reentrant_tokenize.kry

echo ""
if [ "$fail" -eq 0 ]; then
    echo "test_regressions: all clean"
else
    echo "test_regressions: $fail failure(s)"
fi
exit $([ "$fail" -eq 0 ] && echo 0 || echo 1)
