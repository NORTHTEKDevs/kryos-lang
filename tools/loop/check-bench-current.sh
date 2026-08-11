#!/usr/bin/env bash
# check-bench-current.sh -- published benchmark numbers must describe the
# compiler that actually ships.
#
# Acceptance for graph node `bench`.
#
# WHY: a benchmark number with no commit attached is a number from some build,
# somewhere, at some time. It cannot be reproduced or defended, and it silently
# becomes a claim about a compiler that no longer exists. BENCHMARKS.md must
# record the commit it was measured at, and that commit must be an ancestor of
# HEAD -- so the numbers are never older than a compiler change that could have
# moved them.
set -u
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT" || exit 1
BM="BENCHMARKS.md"
fail=0

[ -f "$BM" ] || { echo "bench: FAIL -- $BM does not exist"; exit 1; }

# Required provenance block: commit, machine, date.
sha="$(grep -oE "Measured at commit: *\`?[0-9a-f]{7,40}\`?" "$BM" | head -1 | grep -oE "[0-9a-f]{7,40}")"
if [ -z "$sha" ]; then
    echo "  FAIL: $BM has no 'Measured at commit: <sha>' line"
    echo "        Benchmarks without provenance are unfalsifiable."
    fail=1
else
    if git merge-base --is-ancestor "$sha" HEAD 2>/dev/null; then
        echo "  ok   measured at $sha (an ancestor of HEAD)"
    else
        echo "  FAIL: $BM cites commit $sha, which is not an ancestor of HEAD"
        echo "        Re-run the benchmarks; the compiler moved since they were taken."
        fail=1
    fi
    # The compiler must not have changed since the measurement.
    if git diff --quiet "$sha" HEAD -- compiler/crates compiler/stdlib 2>/dev/null; then
        echo "  ok   no compiler/stdlib change since the measurement"
    else
        echo "  FAIL: compiler/ or stdlib/ changed after $sha -- benchmark numbers are stale"
        git diff --stat "$sha" HEAD -- compiler/crates compiler/stdlib 2>/dev/null | tail -3 | sed 's/^/        /'
        fail=1
    fi
fi

grep -qiE "^ *- *(machine|hardware):" "$BM" || { echo "  FAIL: $BM does not record the machine it was measured on"; fail=1; }

[ $fail -eq 0 ] && echo "bench: PASS" || echo "bench: FAIL"
exit $fail
