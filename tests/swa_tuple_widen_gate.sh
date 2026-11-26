#!/usr/bin/env bash
# swa_tuple_widen_gate.sh -- pins swa item a5 (LEDGER 2026-11-25): a tuple of narrow ints into a wider tuple slot
# (return, let annotation, array literal) kept its own layout: AOT read zeros or
# garbage, JIT sign-extended a u8 element.
# tests/swa/tuple_int_widen.kry must print tests/swa/tuple_int_widen.out
# on BOTH backends.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0
if command -v clang >/dev/null 2>&1 || [ -x "/c/Program Files/LLVM/bin/clang.exe" ]; then
  export PATH="/c/Program Files/LLVM/bin:$PATH"
fi

f="tests/swa/tuple_int_widen.kry"; want="tests/swa/tuple_int_widen.out"
"$K" run "$f" >"$TMP/jit.out" 2>"$TMP/jit.err"; rc=$?
if [ "$rc" -eq 0 ] && diff -q "$TMP/jit.out" "$want" >/dev/null; then
  echo "  ok   swa tuple_int_widen JIT output matches"
else
  echo "  FAIL swa tuple_int_widen JIT (rc=$rc); diff vs expected:"
  diff "$TMP/jit.out" "$want" | head -12 | sed 's/^/    /'
  fail=1
fi
if "$K" build "$f" --release --backend llvm -o "$TMP/dg" >"$TMP/build.log" 2>&1; then
  "$TMP/dg" >"$TMP/aot.out" 2>"$TMP/aot.err"; rc=$?
  if [ "$rc" -eq 0 ] && diff -q "$TMP/aot.out" "$want" >/dev/null; then
    echo "  ok   swa tuple_int_widen AOT output matches"
  else
    echo "  FAIL swa tuple_int_widen AOT (rc=$rc); diff vs expected:"
    diff "$TMP/aot.out" "$want" | head -12 | sed 's/^/    /'
    fail=1
  fi
else
  echo "  FAIL swa tuple_int_widen AOT build:"; tail -5 "$TMP/build.log" | sed 's/^/    /'; fail=1
fi
[ "$fail" -eq 0 ] && echo "swa_tuple_widen_gate: PASS" || echo "swa_tuple_widen_gate: FAIL"
exit "$fail"
