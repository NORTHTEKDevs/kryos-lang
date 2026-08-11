#!/usr/bin/env bash
# check-routing-complete.sh -- every open capability escape must have a MEASURED
# routing entry before anyone edits the checker again.
#
# Acceptance for graph node `escape-instrument`.
#
# WHY: on 2026-08-10 two fixes were written from reasoning about the code and
# both failed. Adding `Borrow`/`Deref` passthrough to `decompose_container_path`
# did not close item 37 (that shape never reaches the decomposer). Adding
# `TupleLiteral` to `literal_field_exists` did not close items 32/38 (blocked
# earlier than the literal resolver). Both were plausible. Both were wrong. The
# cost was two build-measure-revert cycles.
#
# So: no edit to the capability checker until the routing of each escape is
# WRITTEN DOWN from measurement -- which function the call actually reaches, and
# which line returns the fail-open answer.
set -u
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT" || exit 1
DOC="tools/loop/ESCAPE-ROUTING.md"

[ -f "$DOC" ] || { echo "escape-instrument: FAIL -- $DOC does not exist"; exit 1; }

fail=0
missing=0
# Every LIVE CAPABILITY ESCAPE still in the LEDGER's OPEN section needs an entry.
while IFS= read -r line; do
    item="$(printf '%s' "$line" | sed -n 's/^### \([0-9]\+\)\..*/\1/p')"
    [ -z "$item" ] && continue
    printf '%s' "$line" | grep -q "LIVE CAPABILITY ESCAPE" || continue
    if grep -qE "^\| *$item[a-z]? *\|" "$DOC"; then
        # An entry must name a real routing target, not a guess.
        row="$(grep -E "^\| *$item[a-z]? *\|" "$DOC" | head -1)"
        if printf '%s' "$row" | grep -qiE "TODO|unknown|\?\?"; then
            echo "  FAIL: item $item has a placeholder routing entry, not a measurement"
            fail=1
        else
            echo "  ok   item $item routing recorded"
        fi
    else
        echo "  FAIL: item $item has NO routing entry in $DOC"
        missing=$((missing+1)); fail=1
    fi
done < <(sed -n '/^## OPEN — ranked/,/^## CLOSED/p' tools/loop/LEDGER.md | grep -E "^### [0-9]+\.")

[ $missing -gt 0 ] && echo "  ($missing escape(s) unmeasured -- instrument before editing the checker)"
[ $fail -eq 0 ] && echo "escape-instrument: PASS" || echo "escape-instrument: FAIL"
exit $fail
