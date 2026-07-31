#!/usr/bin/env bash
# docs_status_gate.sh -- catches the specific class of doc drift that already
# cost real time twice: docs/BUGS.md claiming a bug is "Active"/open when the
# test that reproduces it now passes cleanly (it once said "none currently
# tracked" while two tests were actually deadlocking; it later drifted the
# OTHER way, claiming those same two tests were still open for weeks after
# the fix that closed them shipped). This is NOT full auto-generation of
# doc status -- it is a mechanical, narrow check: any `tests/conformance/
# conf_*.kry` path named inside docs/BUGS.md's `## Active` section must
# currently FAIL (or hang) when run; if it passes cleanly, the doc is stale
# and this gate fails, naming exactly what to fix.
#
# Also catches stale conformance pass-count claims: docs/BUGS.md and
# README.md sometimes state "conformance is N/N" in prose. This gate checks
# that number against the LIVE file count in tests/conformance/*.kry (cheap;
# does not re-run the suite -- that's `tests/conformance/run_conformance.sh`,
# already a separate gate). A count mismatch does not by itself prove the
# suite fails at that count, but it is a reliable signal the prose was typed
# once and never updated as tests were added.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"

fail=0

echo "== docs/BUGS.md: Active-section tests must not currently pass =="

BUGS_FILE="docs/BUGS.md"
if [ ! -f "$BUGS_FILE" ]; then
    echo "  SKIP: $BUGS_FILE not found"
else
    # Isolate the "## Active" section (everything from that heading to the
    # next "## " heading or end of file).
    active_section="$(awk '/^## Active/{flag=1; next} /^## /{flag=0} flag' "$BUGS_FILE")"

    # Extract every tests/conformance/conf_*.kry path mentioned in it.
    mapfile -t active_tests < <(grep -oE 'tests/conformance/conf_[A-Za-z0-9_]+\.kry' <<<"$active_section" | sort -u)

    if [ "${#active_tests[@]}" -eq 0 ]; then
        echo "  ok: no conformance tests named as Active (nothing to cross-check)"
    else
        for t in "${active_tests[@]}"; do
            if [ ! -f "$t" ]; then
                echo "  STALE  $BUGS_FILE names $t as Active, but that file no longer exists"
                fail=1
                continue
            fi
            if timeout 60 "$KRYOS" run "$t" >/dev/null 2>&1; then
                echo "  STALE  $BUGS_FILE lists $t under Active (open bug), but it PASSES cleanly today"
                echo "         -- move it to Resolved with the fix, or re-verify the bug still reproduces"
                fail=1
            else
                echo "  ok: $t still fails/hangs, matches its Active listing"
            fi
        done
    fi
fi

echo "== conformance pass-count claims vs live test count =="

live_count=$(ls tests/conformance/*.kry 2>/dev/null | wc -l | tr -d ' ')
echo "  live tests/conformance/*.kry count: $live_count"

check_count_claims() {
    local file="$1"
    [ -f "$file" ] || return 0
    # Matches "conformance is N/N" / "conformance N/N PASS" / "conformance is N/N on both backends"
    grep -oE 'conformance[^.]{0,40}[0-9]+/[0-9]+' "$file" | grep -oE '[0-9]+/[0-9]+' | sort -u | while read -r pair; do
        local claimed="${pair%%/*}"
        if [ "$claimed" != "$live_count" ]; then
            echo "  STALE  $file claims conformance $pair, but tests/conformance/ currently has $live_count files"
        fi
    done
}

count_out=""
for f in README.md docs/BUGS.md STABILITY.md; do
    out="$(check_count_claims "$f")"
    if [ -n "$out" ]; then
        echo "$out"
        count_out="$count_out$out"
        fail=1
    fi
done
[ -z "$count_out" ] && echo "  ok: no stale conformance-count claims found in README.md / docs/BUGS.md"

if [ "$fail" -eq 0 ]; then
    echo "docs_status_gate: PASS -- docs/BUGS.md matches live test state"
else
    echo "docs_status_gate: FAIL -- see STALE lines above"
fi
exit $fail
