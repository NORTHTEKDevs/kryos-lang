#!/usr/bin/env bash
# swa_turbofish_struct_gate.sh -- pins swa item turbofish_struct (LEDGER 2026-11-25):
# `Box<int>{ v: 1 }` parsed `<`/`>` as comparisons and failed with three
# misleading "undefined variable" errors. It must now be rejected with ONE
# diagnostic that names the form, and a plain `Box{ v: 1 }` plus ordinary
# `a < b` comparisons must still run (tests/swa/turbofish_struct_ctl.out).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0

for n in turbofish_struct_reject turbofish_struct_nested_reject turbofish_struct_ref_reject turbofish_struct_fn_reject turbofish_struct_opt_reject; do
  f="tests/swa/$n.kry"
  timeout 60 "$K" run "$f" >"$TMP/o" 2>"$TMP/e"; rc=$?
  if [ "$rc" -ne 0 ] && grep -qiE 'turbofish|generic arguments|type arguments' "$TMP/e" \
     && ! grep -qE 'undefined variable' "$TMP/e" && [ "$(grep -c 'error\[' "$TMP/e")" -eq 1 ]; then
    echo "  ok   swa $n rejected with one diagnostic naming the form"
  else
    echo "  FAIL swa $n (rc=$rc); stderr:"; head -8 "$TMP/e" | sed 's/^/    /'; fail=1
  fi
done

for c in turbofish_struct_ctl turbofish_struct_ctl2; do
f="tests/swa/$c.kry"; want="tests/swa/$c.out"
timeout 60 "$K" run "$f" >"$TMP/c.out" 2>"$TMP/c.err"; rc=$?
if [ "$rc" -eq 0 ] && diff -q "$TMP/c.out" "$want" >/dev/null; then
  echo "  ok   swa $c output matches"
else
  echo "  FAIL swa $c (rc=$rc); diff vs expected:"
  diff "$TMP/c.out" "$want" | head -8 | sed 's/^/    /'; fail=1
fi
done
[ "$fail" -eq 0 ] && echo "swa_turbofish_struct_gate: PASS" || echo "swa_turbofish_struct_gate: FAIL"
exit "$fail"
