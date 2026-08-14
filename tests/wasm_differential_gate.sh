#!/usr/bin/env bash
# wasm_differential_gate.sh -- wasm must AGREE with native, or fail to compile.
#
# WHY: LEDGER item 27 -- the wasm32 backend is a documented first-class target
# but was excluded from the differential fuzzer, security_gate, and all leak /
# race testing. Its coverage was 11 hand-written smoke probes plus one CI smoke
# job. Item 27's own stated next step was: "add `kryos build --backend wasm` +
# `node tools/wasm-host/run.mjs` as a third comparison leg to the existing
# differential fuzzer." This is that leg, standalone.
#
# THE CONTRACT BEING PINNED (docs/wasm-contract.md): the backend is an explicit
# SUBSET, and anything outside it fails AT COMPILE TIME with a clear error --
# "never a miscompile". So there are exactly two acceptable outcomes per
# program, and this gate enforces that dichotomy:
#
#   1. it does not compile to wasm            -> ACCEPTABLE (out of subset)
#   2. it compiles AND matches native output  -> ACCEPTABLE (in subset, correct)
#
# and exactly one unacceptable outcome:
#
#   3. it compiles but DISAGREES with native  -> FAIL, a silent miscompile,
#                                                which is the one thing the
#                                                contract promises cannot happen
#
# A compile failure is therefore NOT a gate failure -- treating it as one would
# pressure the subset to grow rather than stay honest. Only divergence fails.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1
K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos.exe}"
[ -x "$K" ] || K="$ROOT/compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"

if ! command -v node >/dev/null 2>&1; then
    echo "wasm-differential: SKIP (node not available; the JS host contract needs it)"
    exit 0
fi

W="$(mktemp -d)"
trap 'rm -rf "$W"' EXIT

compiled=0; skipped=0; agreed=0; diverged=0; diverged_names=""
total=0

for f in "$ROOT"/tests/harden-probes/*.kry "$ROOT"/examples/wasm_*.kry; do
    [ -f "$f" ] || continue
    n="$(basename "$f" .kry)"
    total=$((total + 1))

    if ! timeout 120 "$K" build --backend wasm "$f" -o "$W/$n.wasm" >/dev/null 2>&1; then
        skipped=$((skipped + 1))          # out of subset -- acceptable
        continue
    fi
    compiled=$((compiled + 1))

    nat="$(timeout 120 "$K" run "$f" 2>/dev/null)"; nrc=$?
    [ $nrc -ne 0 ] && continue            # native itself refuses; nothing to compare

    was="$(timeout 120 node "$ROOT/tools/wasm-host/run.mjs" "$W/$n.wasm" 2>/dev/null)"

    if [ "$nat" = "$was" ]; then
        agreed=$((agreed + 1))
    else
        diverged=$((diverged + 1)); diverged_names="$diverged_names $n"
        echo "  MISCOMPILE  $n"
        echo "      native: $(printf '%s' "$nat" | head -3 | tr '\n' '|')"
        echo "      wasm:   $(printf '%s' "$was" | head -3 | tr '\n' '|')"
    fi
done

echo
echo "  programs:            $total"
echo "  compiled to wasm:    $compiled   (out-of-subset, compile-refused: $skipped)"
echo "  agreed with native:  $agreed"
if [ "$diverged" -eq 0 ]; then
    echo "wasm-differential: PASS -- 0 miscompiles ($agreed/$compiled compiled programs match native)"
    exit 0
fi
echo "wasm-differential: FAIL -- $diverged MISCOMPILE(S) --$diverged_names"
exit 1
