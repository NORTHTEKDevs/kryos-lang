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
rejected="$(printf '%s\n' "$out" | sed -n 's/.*now-rejected: \([0-9]\+\).*/\1/p' | head -1)"
if [ -z "$rejected" ]; then
  echo "docs-truth: FAIL -- could not parse a now-rejected count out of escape_status.sh"; exit 1
fi
total=$((actual + rejected))
echo "  measured: $actual capability escape(s) still live, $rejected rejected ($total repros)"

# --- the README must state those numbers ----------------------------------
# We require the digits to appear next to the phrase, rather than trying to
# parse English. Cheap, and it fails loudly when the number moves. The README
# states the count twice (the capabilities bullet near the top and the Status
# paragraph); on 2026-11-02 the bullet still said "12 of the 80" while the
# Status paragraph said 10 of 190 and this check passed, so both are pinned.
if [ "$actual" -eq 0 ]; then
  if grep -qiE "known,? reproducible capability escapes" README.md; then
    echo "  FAIL: escapes are at 0 but README still advertises live escapes"; fail=1
  else
    echo "  ok   README carries no live-escape claim (count is 0)"
  fi
else
  # Accept singular OR plural -- "1 capability escape" is correct English and
  # the doc should not be forced ungrammatical to satisfy a grep.
  if grep -qE "\*\*$actual known, reproducible capability escapes?\*\* still live, $rejected rejected \($total repros\)" README.md; then
    echo "  ok   README Status states $actual known escape(s), $rejected rejected ($total repros)"
  else
    echo "  FAIL: README Status does not state the measured counts."
    echo "        Expected: **$actual known, reproducible capability escape(s)** still live, $rejected rejected ($total repros)"
    grep -noiE ".{0,40}known, reproducible capability escapes.{0,40}" README.md | head -3 | sed 's/^/        got: /'
    fail=1
  fi
  if grep -qE "\*\*$actual of the $total known capability-escape repros under .tests/security/. still escape and $rejected are rejected\*\*" README.md; then
    echo "  ok   README capabilities bullet states $actual of $total, $rejected rejected"
  else
    echo "  FAIL: README capabilities bullet does not state the measured counts."
    echo "        Expected: **$actual of the $total known capability-escape repros under \`tests/security/\` still escape and $rejected are rejected**"
    grep -noE ".{0,10}known capability-escape repros.{0,60}" README.md | head -3 | sed 's/^/        got: /'
    fail=1
  fi
fi

# --- the LEDGER preamble must state the same numbers ----------------------
if grep -qE "true count is \*\*$actual escaping, $rejected rejected\*\*" tools/loop/LEDGER.md; then
  echo "  ok   LEDGER preamble states $actual escaping, $rejected rejected"
else
  echo "  FAIL: LEDGER preamble does not state the measured counts ($actual escaping, $rejected rejected)."
  grep -noE "true count is \*\*[0-9]+ escaping[^*]*\*\*" tools/loop/LEDGER.md | head -2 | sed 's/^/        got: /'
  fail=1
fi

# --- no item may sit in OPEN while its repro is actually rejected ---------
# This is the item-10 failure exactly: fixed, but still ranked top of OPEN.
#
# THIS CHECK WAS INERT FROM THE DAY IT WAS WRITTEN UNTIL 2026-08-16. Its `sed`
# range spelled the OPEN header with an em dash (U+2014) while the LEDGER header
# is `## OPEN - ranked` with a plain hyphen, so the range matched zero lines, the
# loop body ran zero times, and the check "passed" every run by doing nothing.
# That is why eight stale OPEN headings had to be found and removed BY HAND on
# 2026-08-13, and five more on 2026-08-15 -- by the exact check that was
# supposed to prevent it. A verification script that cannot fail is worse than
# no script: it manufactures confidence.
#
# The range matches any separator between OPEN and "ranked" (a later ASCII
# sanitisation had turned the old "either dash" class into hyphen-and-space
# only, so an em dash in the header would have disarmed it again), and the
# check FAILS when the range yields no item at all, so an inert range cannot
# pass silently.
#
# EXEMPTION, and why a naive one-character fix would have been WRONG: the escape
# corpus labels an item's regression pins with letter suffixes (`32a`, `32b`,
# and now `41a`, `41b`). The `[a-z]?` below is deliberate -- LEDGER item 32's
# repros really are named 32a/32b, and it must still be caught if it goes stale.
# But item 41 is a PRECISION-COST design note, not a defect with a repro; its
# pins 41a/41b are legitimately `fixed` while item 41 itself is legitimately
# OPEN. Matching on the bare number alone would report a spurious FAIL against a
# correctly-open item. So an OPEN heading that declares itself a deliberate
# non-defect -- "DESIGN NOTE" or "DELIBERATE", this repo's own vocabulary for
# exactly that -- is exempt. Everything else is still checked.
open_hdr_re='/^## OPEN .*ranked/,/^## CLOSED/p'
open_items=0
while IFS= read -r line; do
  item="$(printf '%s' "$line" | sed -n 's/^### \([0-9]\+\)\..*/\1/p')"
  [ -z "$item" ] && continue
  open_items=$((open_items + 1))
  case "$line" in
    *"DESIGN NOTE"*|*DELIBERATE*) continue ;;
  esac
  if printf '%s\n' "$out" | grep -qE "^${item}[a-z]?[[:space:]].*fixed$"; then
    echo "  FAIL: LEDGER item $item is in OPEN but its repro is REJECTED (already fixed)"
    fail=1
  fi
done < <(sed -n "$open_hdr_re" tools/loop/LEDGER.md | tr -d '\r' | grep -E "^### [0-9]+\.")
if [ "$open_items" -eq 0 ]; then
  echo "  FAIL: the OPEN section of tools/loop/LEDGER.md yielded no \"### N.\" item (header not found?): the stale-OPEN check would be inert"
  fail=1
else
  echo "  ok   checked $open_items OPEN item(s) against the measurement"
fi

[ $fail -eq 0 ] && echo "docs-truth: PASS" || echo "docs-truth: FAIL"
exit $fail
