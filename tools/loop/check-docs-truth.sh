#!/usr/bin/env bash
# check-docs-truth.sh -- the docs must agree with the harness, not with memory.
#
# Acceptance for graph node `docs-truth`.
#
# WHY: on 2026-08-10 the README claimed ONE documented capability residual while
# twelve were live and reproducible, and the LEDGER ranked item 10 as the
# highest-priority OPEN escape days after it had been fixed. Nobody was lying --
# nobody re-ran anything. This check makes the claim answer to the measurement:
# it derives the true count from escape_status.sh and fails if the prose
# disagrees.
set -u
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT" || exit 1

fail=0

# --- ground truth, measured now -------------------------------------------
out="$(bash tools/loop/escape_status.sh 2>&1)" || {
  echo "docs-truth: FAIL -- escape_status.sh did not run"; echo "$out" | tail -5; exit 1; }
actual="$(printf '%s\n' "$out" | sed -n 's/.*STILL ESCAPING: \([0-9]\+\).*/\1/p' | head -1)"
if [ -z "$actual" ]; then
  echo "docs-truth: FAIL -- could not parse a count out of escape_status.sh"; exit 1
fi
echo "  measured: $actual capability escape(s) still live"

# --- the README must state that number ------------------------------------
# We require the digits to appear next to the phrase, rather than trying to
# parse English. Cheap, and it fails loudly when the number moves.
if [ "$actual" -eq 0 ]; then
  if grep -qiE "known,? reproducible capability escapes" README.md; then
    echo "  FAIL: escapes are at 0 but README still advertises live escapes"; fail=1
  else
    echo "  ok   README carries no live-escape claim (count is 0)"
  fi
else
  if grep -qE "\*\*$actual known, reproducible capability escapes\*\*" README.md; then
    echo "  ok   README states $actual known escapes"
  else
    echo "  FAIL: README does not state the measured count ($actual)."
    echo "        Expected the exact phrase: **$actual known, reproducible capability escapes**"
    grep -noiE ".{0,40}known, reproducible capability escapes.{0,10}" README.md | head -3 | sed 's/^/        got: /'
    fail=1
  fi
fi

# --- the LEDGER preamble must state the same number -----------------------
if grep -qE "true count is \*\*$actual escaping" tools/loop/LEDGER.md; then
  echo "  ok   LEDGER preamble states $actual escaping"
else
  echo "  FAIL: LEDGER preamble does not state the measured count ($actual)."
  grep -noE "true count is \*\*[0-9]+ escaping" tools/loop/LEDGER.md | head -2 | sed 's/^/        got: /'
  fail=1
fi

# --- no item may sit in OPEN while its repro is actually rejected ---------
# This is the item-10 failure exactly: fixed, but still ranked top of OPEN.
while IFS= read -r line; do
  item="$(printf '%s' "$line" | sed -n 's/^### \([0-9]\+\)\..*/\1/p')"
  [ -z "$item" ] && continue
  if printf '%s\n' "$out" | grep -qE "^${item}[a-z]?[[:space:]].*fixed$"; then
    echo "  FAIL: LEDGER item $item is in OPEN but its repro is REJECTED (already fixed)"
    fail=1
  fi
done < <(sed -n '/^## OPEN — ranked/,/^## CLOSED/p' tools/loop/LEDGER.md | grep -E "^### [0-9]+\.")

[ $fail -eq 0 ] && echo "docs-truth: PASS" || echo "docs-truth: FAIL"
exit $fail
