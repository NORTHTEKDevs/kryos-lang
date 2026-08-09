#!/usr/bin/env bash
# selfhost_wholeprogram_gate.sh -- whole-program type-check of the self-host
# compiler, under a WALL-CLOCK CEILING.
#
# WHY THIS GATE EXISTS (LEDGER item 39). `891c406` (capability-typed fn
# values) made `kryos check self-host/main.kry` go from 47 seconds to
# non-terminating: the capability-row resolver walked a cyclic substitution
# graph with a path-scoped cycle guard and no memo, so it re-expanded shared
# sub-DAGs exponentially. `test_bootstrap.sh` could no longer complete, which
# means the language's headline "it compiles itself" claim was broken on
# master for three days -- while EVERY other gate stayed green, because not
# one of them ever compiled the self-host compiler. Several sessions then
# misattributed the non-completion to this machine's Defender/CPU contention
# (it was not: the tree is on the exclusion list and Defender was idle).
#
# The bug class this catches is "a front-end change is superlinear in program
# size". A pass/fail on a SMALL input cannot see it -- only a real, large
# program under a time ceiling can. This is the cheapest such oracle: it is
# type-check only (no codegen, no linking), so it runs in well under a minute
# where the full bootstrap takes minutes.
#
# The ceiling is deliberately ~4x the healthy time, not a tight budget: this
# gate is a CLIFF detector (seconds -> forever), not a performance benchmark,
# and it must not go red from ordinary machine noise.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
[ -x "$KRYOS" ] || { echo "selfhost-wholeprogram: FAIL -- no kryos binary at $KRYOS"; exit 1; }

MAIN="$ROOT/compiler/self-host/main.kry"
[ -f "$MAIN" ] || { echo "selfhost-wholeprogram: FAIL -- missing $MAIN"; exit 1; }

CEILING="${KRYOS_SELFHOST_CHECK_CEILING:-200}"

start=$(date +%s)
# `cd` so any relative `use` paths resolve the way the bootstrap does.
( cd "$ROOT/compiler" && timeout "$CEILING" "$KRYOS" check self-host/main.kry ) > /tmp/selfhost_wholeprogram.$$ 2>&1
rc=$?
elapsed=$(( $(date +%s) - start ))

if [ "$rc" -eq 124 ]; then
  echo "selfhost-wholeprogram: FAIL -- 'kryos check self-host/main.kry' did not finish within ${CEILING}s."
  echo "  This is the LEDGER item 39 signature: a front-end change that is"
  echo "  superlinear in program size. It will also break test_bootstrap.sh."
  echo "  Do NOT write this off as machine load -- check with a profiler or a"
  echo "  call counter before blaming the environment (item 39 was blamed on"
  echo "  Defender for three days while the tree was on the exclusion list)."
  rm -f /tmp/selfhost_wholeprogram.$$
  exit 1
fi

if [ "$rc" -ne 0 ]; then
  echo "selfhost-wholeprogram: FAIL -- check exited $rc in ${elapsed}s:"
  tail -20 /tmp/selfhost_wholeprogram.$$ | sed 's/^/    /'
  rm -f /tmp/selfhost_wholeprogram.$$
  exit 1
fi

rm -f /tmp/selfhost_wholeprogram.$$
echo "selfhost-wholeprogram: PASS (${elapsed}s, ceiling ${CEILING}s)"
exit 0
