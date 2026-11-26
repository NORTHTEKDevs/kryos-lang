#!/usr/bin/env bash
# swa_payload_gate.sh -- pins the swa "undeclared payload type" closure (LEDGER
# 2026-11-25): a Result / Option whose payload type only the checker knows
# printed a raw pointer or an integer. tests/swa/payload_closure.kry must print
# tests/swa/payload_closure.out on BOTH backends; the unannotated
# `let a = to_array(r)` must stay rejected with E0110.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0
if command -v clang >/dev/null 2>&1 || [ -x "/c/Program Files/LLVM/bin/clang.exe" ]; then
  export PATH="/c/Program Files/LLVM/bin:$PATH"
fi

f="tests/swa/payload_closure.kry"; want="tests/swa/payload_closure.out"
"$K" run "$f" >"$TMP/jit.out" 2>"$TMP/jit.err"; rc=$?
if [ "$rc" -eq 0 ] && diff -q "$TMP/jit.out" "$want" >/dev/null; then
  echo "  ok   swa payload_closure JIT output matches"
else
  echo "  FAIL swa payload_closure JIT (rc=$rc); diff vs expected:"
  diff "$TMP/jit.out" "$want" | head -12 | sed 's/^/    /'
  fail=1
fi
if "$K" build "$f" --release --backend llvm -o "$TMP/pc" >"$TMP/build.log" 2>&1; then
  "$TMP/pc" >"$TMP/aot.out" 2>"$TMP/aot.err"; rc=$?
  if [ "$rc" -eq 0 ] && diff -q "$TMP/aot.out" "$want" >/dev/null; then
    echo "  ok   swa payload_closure AOT output matches"
  else
    echo "  FAIL swa payload_closure AOT (rc=$rc); diff vs expected:"
    diff "$TMP/aot.out" "$want" | head -12 | sed 's/^/    /'
    fail=1
  fi
else
  echo "  FAIL swa payload_closure AOT build:"; tail -5 "$TMP/build.log" | sed 's/^/    /'; fail=1
fi

g="tests/swa/payload_closure_reject.kry"
out="$("$K" check "$g" 2>&1)"; rc=$?
if [ "$rc" -ne 0 ] && printf '%s' "$out" | grep -q 'E0110'; then
  echo "  ok   swa unannotated to_array binding stays E0110"
else
  echo "  FAIL swa unannotated to_array binding was accepted (rc=$rc)"; fail=1
fi
[ "$fail" -eq 0 ] && echo "swa_payload_gate: PASS" || echo "swa_payload_gate: FAIL"
exit "$fail"
