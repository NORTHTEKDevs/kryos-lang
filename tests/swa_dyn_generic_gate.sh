#!/usr/bin/env bash
# swa_dyn_generic_gate.sh -- pins swa item a8 (LEDGER 2026-11-25): a `dyn Trait`
# built from a monomorphized generic struct dispatched to the shared erased
# method copy, so a str / f64 field printed as a raw pointer / float bits.
# tests/swa/dyn_generic_instance.kry must print tests/swa/dyn_generic_instance.out
# on BOTH backends.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0
if command -v clang >/dev/null 2>&1 || [ -x "/c/Program Files/LLVM/bin/clang.exe" ]; then
  export PATH="/c/Program Files/LLVM/bin:$PATH"
fi

f="tests/swa/dyn_generic_instance.kry"; want="tests/swa/dyn_generic_instance.out"
"$K" run "$f" >"$TMP/jit.out" 2>"$TMP/jit.err"; rc=$?
if [ "$rc" -eq 0 ] && diff -q "$TMP/jit.out" "$want" >/dev/null; then
  echo "  ok   swa dyn_generic_instance JIT output matches"
else
  echo "  FAIL swa dyn_generic_instance JIT (rc=$rc); diff vs expected:"
  diff "$TMP/jit.out" "$want" | head -12 | sed 's/^/    /'
  fail=1
fi
if "$K" build "$f" --release --backend llvm -o "$TMP/dg" >"$TMP/build.log" 2>&1; then
  "$TMP/dg" >"$TMP/aot.out" 2>"$TMP/aot.err"; rc=$?
  if [ "$rc" -eq 0 ] && diff -q "$TMP/aot.out" "$want" >/dev/null; then
    echo "  ok   swa dyn_generic_instance AOT output matches"
  else
    echo "  FAIL swa dyn_generic_instance AOT (rc=$rc); diff vs expected:"
    diff "$TMP/aot.out" "$want" | head -12 | sed 's/^/    /'
    fail=1
  fi
else
  echo "  FAIL swa dyn_generic_instance AOT build:"; tail -5 "$TMP/build.log" | sed 's/^/    /'; fail=1
fi
[ "$fail" -eq 0 ] && echo "swa_dyn_generic_gate: PASS" || echo "swa_dyn_generic_gate: FAIL"
exit "$fail"
