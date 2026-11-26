#!/usr/bin/env bash
# swa_tuple_widen_gate.sh -- pins swa item a5 (LEDGER 2026-11-25): a tuple of narrow ints into a wider tuple slot
# (return, let annotation, array literal; call argument, struct field, assignment,
# element store, push, closure call, nested tuple) kept its own layout: AOT read zeros or
# garbage, JIT sign-extended a u8 element.
# tests/swa/tuple_int_widen.kry and tuple_int_widen_slots.kry must print their .out
# on BOTH backends.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0
if command -v clang >/dev/null 2>&1 || [ -x "/c/Program Files/LLVM/bin/clang.exe" ]; then
  export PATH="/c/Program Files/LLVM/bin:$PATH"
fi

for f in tests/swa/tuple_int_widen.kry tests/swa/tuple_int_widen_slots.kry; do
  n="${f##*/}"; n="${n%.kry}"; want="${f%.kry}.out"
  "$K" run "$f" >"$TMP/jit.out" 2>"$TMP/jit.err"; rc=$?
  if [ "$rc" -eq 0 ] && diff -q "$TMP/jit.out" "$want" >/dev/null; then
    echo "  ok   swa $n JIT output matches"
  else
    echo "  FAIL swa $n JIT (rc=$rc); diff vs expected:"
    diff "$TMP/jit.out" "$want" | head -12 | sed 's/^/    /'
    fail=1
  fi
  if "$K" build "$f" --release --backend llvm -o "$TMP/dg" >"$TMP/build.log" 2>&1; then
    "$TMP/dg" >"$TMP/aot.out" 2>"$TMP/aot.err"; rc=$?
    if [ "$rc" -eq 0 ] && diff -q "$TMP/aot.out" "$want" >/dev/null; then
      echo "  ok   swa $n AOT output matches"
    else
      echo "  FAIL swa $n AOT (rc=$rc); diff vs expected:"
      diff "$TMP/aot.out" "$want" | head -12 | sed 's/^/    /'
      fail=1
    fi
  else
    echo "  FAIL swa $n AOT build:"; tail -5 "$TMP/build.log" | sed 's/^/    /'; fail=1
  fi
done
[ "$fail" -eq 0 ] && echo "swa_tuple_widen_gate: PASS" || echo "swa_tuple_widen_gate: FAIL"
exit "$fail"
