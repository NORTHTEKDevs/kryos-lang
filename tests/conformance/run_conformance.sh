#!/usr/bin/env bash
# Spec-conformance suite: every conf_*.kry is a self-checking program tied
# to reference-manual chapters; each must run clean on BOTH backends
# (Cranelift JIT and LLVM AOT). Exit 0 only when everything passes.
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
KRYOS="$REPO/compiler/target/release/kryos"
[[ "${OS:-}" == "Windows_NT" || "$(uname -s)" =~ MINGW|MSYS|CYGWIN ]] && KRYOS="$KRYOS.exe"
[[ -x "$KRYOS" ]] || { echo "conformance: build compiler first"; exit 2; }

if ! command -v clang >/dev/null 2>&1; then
    for c in "/c/Program Files/LLVM/bin" "/c/Program Files (x86)/LLVM/bin"; do
        [[ -x "$c/clang.exe" || -x "$c/clang" ]] && export PATH="$c:$PATH" && break
    done
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
fail=0
n=0

# Every invocation is time-boxed. Without this, a program that DEADLOCKS (rather
# than crashing) does not make the suite fail -- it makes the suite hang, and a
# CI job blocks until the platform's step limit with no useful output. A hang is
# a failure; report it as one. 240s is ~20x the slowest passing program.
TB=240
run_tb() { timeout "$TB" "$@"; }

for f in "$REPO"/tests/conformance/conf_*.kry; do
    base="$(basename "$f" .kry)"
    n=$((n+1))
    if ! run_tb "$KRYOS" run "$f" >/dev/null 2>&1; then
        echo "  CL FAIL   $base"
        run_tb "$KRYOS" run "$f" 2>&1 | tail -3
        fail=1
        continue
    fi
    if ! run_tb "$KRYOS" build "$f" --release --backend llvm -o "$TMP/$base" >/dev/null 2>&1; then
        echo "  AOT BUILD FAIL $base"
        fail=1
        continue
    fi
    if ! run_tb "$TMP/$base" >/dev/null 2>&1; then
        rc=$?
        [[ $rc -eq 124 ]] && echo "  AOT HANG ($TB s) $base" || echo "  AOT RUN FAIL $base"
        fail=1
        continue
    fi
    echo "  OK        $base (both backends)"
done

if [[ $fail -eq 0 ]]; then
    echo "conformance: $n/$n PASS"
else
    echo "conformance: FAIL"
fi
exit $fail
