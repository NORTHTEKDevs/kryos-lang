#!/usr/bin/env bash
# Gate for the `kryos test` runner itself (cert Pass 43). Historical failure
# modes, each of which regressed silently because nothing exercised the
# runner end-to-end:
#   - a test file importing stdlib modules found "no @test functions"
#     (discovery compile failed on a false cross-module name collision)
#   - @capabilities tests crashed the in-process JIT with
#     "can't resolve symbol str_to_ptr" / "kryos_db_open" (the JIT's
#     symbol table had drifted behind the AOT link path)
#   - failing tests must exit 1, passing ones 0
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="$ROOT/compiler/stdlib"
K="$ROOT/compiler/target/release/kryos.exe"
FIX="$ROOT/tests/harness-fixtures"
fail=0

expect() { # name file want_rc want_grep
  local name="$1" file="$2" want_rc="$3" want_grep="$4"
  local out rc
  out=$(timeout 120 "$K" test "$FIX/$file" 2>&1); rc=$?
  if [ "$rc" -ne "$want_rc" ]; then
    echo "  FAIL $name -- rc=$rc want=$want_rc"; echo "$out" | head -5 | sed 's/^/       /'; fail=1; return
  fi
  if ! echo "$out" | grep -q "$want_grep"; then
    echo "  FAIL $name -- missing '$want_grep'"; echo "$out" | head -5 | sed 's/^/       /'; fail=1; return
  fi
  echo "  PASS $name"
}

expect basic          test_math.kry   0 "2 passed, 0 failed"
expect failing_exit1  test_fail.kry   1 "1 passed, 1 failed"
expect stdlib_imports test_stdlib.kry 0 "2 passed, 0 failed"
expect capabilities   test_caps.kry   0 "1 passed, 0 failed"

# Selective+bare imports with overlapping module-internal names must coexist
# (std::string's internal `split` vs std::re's exported `split`).
out=$(timeout 120 "$K" run "$FIX/dual_import.kry" 2>&1); rc=$?
if [ "$rc" -eq 0 ] && echo "$out" | grep -q "anchored=1"; then
  echo "  PASS dual_import"
else
  echo "  FAIL dual_import rc=$rc"; echo "$out" | head -4 | sed 's/^/       /'; fail=1
fi

[ "$fail" -eq 0 ] && echo "harness-check: all green" || echo "harness-check: FAILURES"
exit "$fail"
