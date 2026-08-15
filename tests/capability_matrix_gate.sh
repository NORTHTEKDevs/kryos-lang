#!/usr/bin/env bash
# capability_matrix_gate.sh -- bounded EXHAUSTIVE capability-laundering search.
#
# WHY: all 17 shapes pinned by `tools/loop/escape_status.sh` were found BY HAND.
# That corpus's weakness is precisely "the shape nobody thought of", and no
# amount of re-running it green addresses that. This enumerates the laundering
# space combinatorially -- SOURCE x CONTAINER x TRANSPORT -- so the claim
# becomes "no counterexample in a systematically enumerated space of size N"
# instead of "nobody has thought of another one". It is still not a proof; it
# is a bounded-model result, and N is printed so the bound is never implicit.
#
# TWO AXES, REPORTED SEPARATELY, because they fail in opposite directions and
# collapsing them hides the interesting half:
#
#   SOUNDNESS (pass/fail) -- every ATTACK routes a privileged closure into a
#     `deny!(fs:read)` block. Every one must be REJECTED. A single acceptance
#     is a live escape and fails this gate.
#
#   PRECISION (measured, reported, NOT pass/fail) -- every attack has a twin
#     CONTROL that is identical except the closure is pure. Ideally all compile.
#     They do not, and that is a KNOWN, DELIBERATE consequence of the
#     fail-closed stance: reading a fn-bearing container yields
#     `CapRow::Unknown`, which erases to ALL, so passing even a PURE closure
#     through some shapes demands `@capabilities(all)`. `tools/loop/STAGE2-PLAN.md`
#     predicted this cost; this gate is the first thing to MEASURE it.
#
# Precision is deliberately not a failure condition. Making it one would create
# pressure to loosen enforcement to make a number go green -- the exact
# incentive that produced the fail-OPEN default this design replaced. It is
# printed so the usability cost is visible and can be argued about honestly.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1
K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos.exe}"
[ -x "$K" ] || K="$ROOT/compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"

GEN="$ROOT/tests/gen_capability_matrix.py"
[ -f "$GEN" ] || { echo "capability-matrix: SKIP (generator missing)"; exit 0; }
command -v python3 >/dev/null 2>&1 || { echo "capability-matrix: SKIP (no python3)"; exit 0; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

shapes="$(python3 "$GEN" "$TMP/mx")"
echo "  enumerated shapes: $shapes  (SOURCE x CONTAINER x TRANSPORT)"

escaped=0; rejected=0; escaped_names=""
ctl_ok=0; ctl_rej=0; ctl_names=""

for f in "$TMP"/mx/*__attack.kry; do
    b="$(basename "$f" __attack.kry)"
    # Both enforcement modes, exactly like escape_status.sh does.
    if timeout 90 "$K" check "$f" >/dev/null 2>&1; then
        escaped=$((escaped + 1)); escaped_names="$escaped_names $b"
        echo "  ESCAPE      $b -- accepted in default-inferred mode"
        continue
    fi
    if timeout 90 "$K" check --strict-capabilities "$f" >/dev/null 2>&1; then
        escaped=$((escaped + 1)); escaped_names="$escaped_names $b(strict)"
        echo "  ESCAPE      $b -- accepted under --strict-capabilities"
        continue
    fi
    rejected=$((rejected + 1))

    c="$TMP/mx/${b}__control.kry"
    if timeout 90 "$K" check "$c" >/dev/null 2>&1; then
        ctl_ok=$((ctl_ok + 1))
    else
        ctl_rej=$((ctl_rej + 1)); ctl_names="$ctl_names $b"
    fi
done

total=$((escaped + rejected))
echo
echo "  SOUNDNESS  attacks rejected: $rejected/$total"
echo "  PRECISION  pure-closure controls accepted: $ctl_ok/$total"
if [ "$ctl_rej" -ne 0 ]; then
    echo "             over-rejected (fail-closed Unknown->ALL cost, not a failure):"
    for n in $ctl_names; do echo "               $n"; done
fi
echo
if [ "$escaped" -eq 0 ]; then
    echo "capability-matrix: PASS -- 0 escapes across $total enumerated shapes, both modes"
    exit 0
fi
echo "capability-matrix: FAIL -- $escaped ESCAPE(S) --$escaped_names"
exit 1
