#!/usr/bin/env bash
# test_bootstrap_robust.sh -- run test_bootstrap.sh N times, report
# per-module pass rate and overall best/median/worst case.
#
# Usage:  self-host/test_bootstrap_robust.sh [N]    (default N=5)
#
# Reports
#   - Per-module pass rate across N runs
#   - Mean / best / worst PASS count
#   - Modules that NEVER passed (real regressions)
#   - Modules that ALWAYS passed (stable)
#   - Modules with intermittent passes (heap-flake list)

set -u
N=${1:-5}
cd "$(dirname "$0")/.."

declare -A pass_count
modules="token lexer ast parser types mir lower optimize regalloc x86 codegen elf coff linker runtime main"

mkdir -p target/robust-logs
for m in $modules; do
    pass_count[$m]=0
done

best=0
worst=16
total=0

for run in $(seq 1 "$N"); do
    log="target/robust-logs/run-$run.log"
    bash self-host/test_bootstrap.sh > "$log" 2>&1
    pass=$(grep -E "^PASS:" "$log" | sed -E 's/.*PASS: ([0-9]+).*/\1/')
    total=$((total + pass))
    if [ "$pass" -gt "$best" ]; then best=$pass; fi
    if [ "$pass" -lt "$worst" ]; then worst=$pass; fi

    # Track per-module passes
    for m in $modules; do
        if grep -qE "^${m}\.kry\s+\|\s+OK" "$log"; then
            pass_count[$m]=$((pass_count[$m] + 1))
        fi
    done

    echo "Run $run: $pass / 16"
done

mean=$(awk "BEGIN { printf \"%.2f\", $total / $N }")
echo ""
echo "=== Summary over $N runs ==="
echo "Mean: $mean / 16   Best: $best / 16   Worst: $worst / 16"
echo ""
echo "Per-module:"
always_pass=""
never_pass=""
flaky=""
for m in $modules; do
    pc=${pass_count[$m]}
    printf "  %-12s %d/%d" "$m" "$pc" "$N"
    if [ "$pc" -eq "$N" ]; then
        echo " (stable)"
        always_pass="$always_pass $m"
    elif [ "$pc" -eq 0 ]; then
        echo " (REGRESSION)"
        never_pass="$never_pass $m"
    else
        echo " (flaky)"
        flaky="$flaky $m"
    fi
done
echo ""
[ -n "$never_pass" ] && echo "Regressions:$never_pass"
[ -n "$flaky" ] && echo "Flaky:$flaky"
[ -n "$always_pass" ] && echo "Stable:$always_pass"
