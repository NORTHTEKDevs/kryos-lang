#!/usr/bin/env bash
# verdict_diff.sh -- did a checker change move any VERDICT over the corpus?
#
# A fail-closed capability fix is only as good as the legitimate programs it
# does NOT start rejecting. This runs `kryos check` over every program in the
# corpus under a BASELINE binary (the master build, copied aside BEFORE the
# change) and a CANDIDATE binary (the build under test), in the default mode
# and under --strict-capabilities, and prints every program whose verdict moved.
#
# A verdict is the pair (exit code, first error code). It is deliberately
# coarse: a program that was rejected for E0507 and is now rejected for E0110
# is still rejected, but the line is printed because the REASON moved.
#
# Every changed verdict must then be classified by hand -- a script cannot:
#   (a) a targeted escape that is now rejected          -> the fix working
#   (b) a program that escaped and was never in the list -> a NEW finding; prove
#       it by running it and showing the leak, and record it in the LEDGER
#   (c) a legitimate program that is now rejected        -> an OVER-REJECTION;
#       the fix is not done until there are none
#
# Usage:
#   tools/loop/verdict_diff.sh <baseline-kryos> <candidate-kryos> [outdir]
#
# Environment:
#   CORPUS_DIRS   space-separated directories to scan (default: see below)
#   JOBS          parallel checks (default 8)
#   KEEP_LISTS    when set, keep the per-binary TSVs in <outdir>
#
# Both binaries run from the repository root of THIS checkout with the SAME
# KRYOS_STDLIB_DIR, so only the binary differs. Exit status: 0 when no verdict
# moved, 1 when at least one did (it is a report, not a gate), 2 on misuse.
set -u
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT" || exit 2

BASE="${1:-}"; CAND="${2:-}"; OUT="${3:-}"
if [ -z "$BASE" ] || [ -z "$CAND" ]; then
    sed -n '2,32p' "$0"; exit 2
fi
[ -x "$BASE" ] || { echo "verdict_diff: baseline binary not executable: $BASE" >&2; exit 2; }
[ -x "$CAND" ] || { echo "verdict_diff: candidate binary not executable: $CAND" >&2; exit 2; }
[ -n "$OUT" ] || OUT="$(mktemp -d)"
mkdir -p "$OUT"
JOBS="${JOBS:-8}"
CORPUS_DIRS="${CORPUS_DIRS:-tests examples ecosystem compiler/self-host compiler/stdlib compiler/examples compiler/benchmarks compiler/tests packages benchmarks demo}"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
export NO_COLOR=1

# shellcheck disable=SC2086
find $CORPUS_DIRS -name '*.kry' -type f 2>/dev/null | LC_ALL=C sort > "$OUT/corpus.txt"
N="$(wc -l < "$OUT/corpus.txt" | tr -d ' ')"
echo "verdict_diff: corpus $N programs, 2 modes, 2 binaries, $JOBS jobs"
echo "  baseline : $BASE"
echo "  candidate: $CAND"
ls -l --time-style=long-iso "$BASE" "$CAND" 2>/dev/null | awk '{print "    " $6 " " $7 "  " $8}'

one() {
    # $1 binary  $2 mode ("" or flag)  $3 file
    local out rc code
    out="$(timeout 120 "$1" check $2 "$3" 2>&1)"; rc=$?
    code="$(printf '%s' "$out" | grep -oE 'E[0-9]{4}' | head -1)"
    printf '%s\t%s\t%s\t%s\n' "$3" "${2:-default}" "$rc" "${code:--}"
}
export -f one

run_all() {
    local bin="$1" tsv="$2"
    : > "$tsv.tmp"
    for mode in "" "--strict-capabilities"; do
        # shellcheck disable=SC2016
        xargs -a "$OUT/corpus.txt" -d '\n' -P "$JOBS" -I{} bash -c 'one "$0" "$1" "$2"' "$bin" "$mode" {} >> "$tsv.tmp"
    done
    LC_ALL=C sort "$tsv.tmp" > "$tsv"; rm -f "$tsv.tmp"
}

run_all "$BASE" "$OUT/baseline.tsv"
run_all "$CAND" "$OUT/candidate.tsv"

# join on (file, mode)
TAB="$(printf '\t')"
LC_ALL=C join -t "$TAB" -j 1 \
    <(awk -F'\t' '{print $1 "|" $2 "\t" $3 "\t" $4}' "$OUT/baseline.tsv" | LC_ALL=C sort -t "$TAB" -k1,1) \
    <(awk -F'\t' '{print $1 "|" $2 "\t" $3 "\t" $4}' "$OUT/candidate.tsv" | LC_ALL=C sort -t "$TAB" -k1,1) \
    > "$OUT/joined.tsv"

TOTAL="$(wc -l < "$OUT/joined.tsv" | tr -d ' ')"
awk -F'\t' '$2 != $4 || $3 != $5 {print}' "$OUT/joined.tsv" > "$OUT/changed.tsv"
CHANGED="$(wc -l < "$OUT/changed.tsv" | tr -d ' ')"

# newly rejected: baseline rc 0 -> candidate rc != 0
awk -F'\t' '$2 == 0 && $4 != 0' "$OUT/changed.tsv" > "$OUT/newly_rejected.tsv"
# newly accepted: baseline rc != 0 -> candidate rc 0
awk -F'\t' '$2 != 0 && $4 == 0' "$OUT/changed.tsv" > "$OUT/newly_accepted.tsv"
# reason moved only: rejected both times, different first code
awk -F'\t' '$2 != 0 && $4 != 0' "$OUT/changed.tsv" > "$OUT/reason_moved.tsv"

fmt() { awk -F'\t' '{split($1, a, "|"); printf "  %-78s %-22s rc=%s %s -> rc=%s %s\n", a[1], a[2], $2, $3, $4, $5}' "$1"; }

echo
echo "verdicts compared : $TOTAL"
echo "verdicts changed  : $CHANGED"
echo "  newly REJECTED  : $(wc -l < "$OUT/newly_rejected.tsv" | tr -d ' ')"
echo "  newly ACCEPTED  : $(wc -l < "$OUT/newly_accepted.tsv" | tr -d ' ')"
echo "  reason moved    : $(wc -l < "$OUT/reason_moved.tsv" | tr -d ' ')"
for kind in newly_rejected newly_accepted reason_moved; do
    if [ -s "$OUT/$kind.tsv" ]; then
        echo; echo "== $kind =="; fmt "$OUT/$kind.tsv"
    fi
done
echo
echo "verdict_diff: lists kept in $OUT"
[ "$CHANGED" -eq 0 ]
