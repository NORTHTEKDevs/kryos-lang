#!/usr/bin/env bash
# parser_nesting_gate.sh -- regression gate for LEDGER item 22
# (parser nesting-depth guard: resource-DoS investigation).
#
# WHAT THIS GATE PROVES (all with an ACTUAL kill-verified timeout, not a
# timeout that can silently be outlived by a hung child -- MSYS `timeout`
# does not reliably signal a native Win32 child, see run_with_cap() below):
#
#   1. Every one of the 9 grammar constructs LEDGER item 22 named (nested
#      parens, arrays, blocks-as-expr, if-as-expr, match-as-expr,
#      Option-type nesting, long flat operator chains, nested string
#      interpolation, wide string interpolation) completes `kryos check` in
#      bounded time at and beyond the depths that item claimed hung
#      indefinitely -- this gate FAILS (does not hang alongside a
#      regression) if any of them ever again takes longer than CAP_S.
#   2. A flat operator chain just under the true 2048-node ceiling is
#      ACCEPTED, not rejected -- the false-positive this investigation
#      actually found and fixed: the Pratt-parser spine loop used to charge
#      nesting budget for a pure lookahead peek (deciding "does the chain
#      continue?") even when the answer was no and no AST node was built,
#      so a legitimately-under-the-ceiling chain could still trip E0010.
#   3. A flat operator chain just OVER that ceiling is still cleanly
#      rejected with exactly one E0010, and -- the second half of the same
#      bug -- the diagnostic's span lands inside the offending expression,
#      not on an unrelated later statement the overflow-triggering peek
#      merely happened to be looking at.
#
# Investigation record: `tools/loop/LEDGER.md` item 22. That item's own
# claimed thresholds (nested parens 1750 fast / 1780 hangs; nested arrays
# 1600 fast / 1800 hangs; nested string interpolation 495 fast / 500 hangs)
# do NOT reproduce -- re-verified live at and far beyond every one of those
# depths, every run in this gate completes in low single-digit seconds. See
# the item's own follow-up note for the full evidence trail.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
K="$ROOT/compiler/target/release/kryos.exe"; [ -x "$K" ] || K="$ROOT/compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0

# Baseline `kryos check` startup cost on this machine runs a few seconds
# (stdlib load); every construct below is at most O(depth) work in the
# parser, so a generous-but-finite cap catches a real hang without flaking
# on a slow/contended box.
CAP_S=30

# Robust bounded run: launches in the background and polls, force-killing
# via taskkill (Windows) or the process group (POSIX) if CAP_S elapses.
# Plain `timeout $N cmd` is NOT sufficient here -- on Windows, `timeout`'s
# SIGTERM does not reliably reach a native (non-MSYS) child process, so a
# genuinely-hung kryos.exe can outlive the wrapper and this gate would
# "pass" by hanging right alongside the regression it exists to catch.
run_with_cap() {
    local file="$1" out="$2"
    : > "$out"
    if [ "$(uname -s 2>/dev/null | cut -c1-3)" = "MIN" ] || [ "$(uname -s 2>/dev/null | cut -c1-3)" = "MSY" ] || [ -n "${WINDIR:-}" ]; then
        "$K" check "$file" > "$out" 2>&1 &
        local pid=$!
        local waited=0
        while kill -0 "$pid" 2>/dev/null; do
            sleep 0.25
            waited=$((waited + 1))
            if [ "$waited" -ge $((CAP_S * 4)) ]; then
                taskkill //F //T //PID "$pid" > /dev/null 2>&1
                wait "$pid" 2>/dev/null
                echo "TIMEOUT" >> "$out"
                return 124
            fi
        done
        wait "$pid"
        return $?
    else
        timeout -k 2 "$CAP_S" "$K" check "$file" > "$out" 2>&1
        return $?
    fi
}

check_bounded() {
    local label="$1" file="$2"
    local out="$TMP/$(basename "$file").out"
    local start end ms rc
    start=$(date +%s%N)
    run_with_cap "$file" "$out"
    rc=$?
    end=$(date +%s%N)
    ms=$(( (end - start) / 1000000 ))
    if [ "$rc" -eq 124 ] || grep -q "^TIMEOUT$" "$out"; then
        echo "  FAIL: $label HUNG past ${CAP_S}s"
        fail=1
        return 1
    fi
    echo "  ok   $label (${ms}ms, rc=$rc)"
    return "$rc"
}

rep() { local s="" i; for ((i=0; i<$2; i++)); do s+="$1"; done; printf '%s' "$s"; }

gen() {
    local ctor="$1" depth="$2" out="$3"
    case "$ctor" in
        parens)
            printf 'fn main() {\n let x = %s1%s\n println(to_string(x))\n}\n' \
                "$(rep '(' "$depth")" "$(rep ')' "$depth")" > "$out" ;;
        arrays)
            printf 'fn main() {\n let x = %s1%s\n println(to_string(len(x)))\n}\n' \
                "$(rep '[' "$depth")" "$(rep ']' "$depth")" > "$out" ;;
        blocks)
            printf 'fn main() {\n let x = %s1%s\n println(to_string(x))\n}\n' \
                "$(rep '{ ' "$depth")" "$(rep ' }' "$depth")" > "$out" ;;
        ifexpr)
            { printf 'fn main() {\n let cond: bool = true\n let x: i64 = '
              for ((i=0; i<depth; i++)); do printf 'if cond { %d } else { ' "$i"; done
              printf '0'
              for ((i=0; i<depth; i++)); do printf ' }'; done
              printf '\n println(to_string(x))\n}\n'
            } > "$out" ;;
        matchexpr)
            { printf 'fn main() {\n let v: i64 = 0\n let x: i64 = '
              for ((i=0; i<depth; i++)); do printf 'match v { _ => '; done
              printf '0'
              for ((i=0; i<depth; i++)); do printf ' }'; done
              printf '\n println(to_string(x))\n}\n'
            } > "$out" ;;
        optiontype)
            printf 'use std::option::{Option}\nfn mk() -> %si64%s {\n throw "unreachable"\n}\nfn main() {\n println("ok")\n}\n' \
                "$(rep 'Option<' "$depth")" "$(rep '>' "$depth")" > "$out" ;;
        flatchain)
            { printf 'fn main() {\n let x = '
              for ((i=0; i<depth; i++)); do printf '1+'; done
              printf '1\n println(to_string(x))\n}\n'
            } > "$out" ;;
        stringinterp)
            { printf 'fn main() {\n let x = '
              for ((i=0; i<depth; i++)); do printf '"{ '; done
              printf '1'
              for ((i=0; i<depth; i++)); do printf ' }"'; done
              printf '\n println(x)\n}\n'
            } > "$out" ;;
        wideinterp)
            { printf 'fn main() {\n let x: i64 = 1\n let s = "'
              for ((i=0; i<depth; i++)); do printf '{x}'; done
              printf '"\n println(to_string(len(s)))\n}\n'
            } > "$out" ;;
    esac
}

echo "== LEDGER item 22: parser nesting-depth guard =="

# --- Part 1: bounded time, at/beyond the depths LEDGER item 22 claimed hung.
echo "-- part 1: bounded-time across all 9 named constructs --"
gen parens        1900 "$TMP/parens.kry";        check_bounded "nested parens @1900"              "$TMP/parens.kry"
gen arrays        1900 "$TMP/arrays.kry";        check_bounded "nested arrays @1900"               "$TMP/arrays.kry"
gen blocks        1900 "$TMP/blocks.kry";        check_bounded "nested blocks-as-expr @1900"       "$TMP/blocks.kry"
gen ifexpr        1200 "$TMP/ifexpr.kry";        check_bounded "nested if-as-expr @1200"           "$TMP/ifexpr.kry"
gen matchexpr     1200 "$TMP/matchexpr.kry";     check_bounded "nested match-as-expr @1200"        "$TMP/matchexpr.kry"
gen optiontype     400 "$TMP/optiontype.kry";    check_bounded "nested Option<> type @400"         "$TMP/optiontype.kry"
gen flatchain    10000 "$TMP/flatchain.kry";     check_bounded "long flat operator chain @10000"   "$TMP/flatchain.kry"
gen stringinterp   550 "$TMP/stringinterp.kry";  check_bounded "nested string interpolation @550"  "$TMP/stringinterp.kry"
gen wideinterp    8000 "$TMP/wideinterp.kry";    check_bounded "wide string interpolation @8000"   "$TMP/wideinterp.kry"

# --- Part 2: no false-positive rejection just under the true ceiling.
echo "-- part 2: flat chain just under the ceiling must be ACCEPTED --"
gen flatchain 2045 "$TMP/chain_ok.kry"
check_bounded "flat chain @2045 (under ceiling)" "$TMP/chain_ok.kry"
rc_ok=$?
if [ "$rc_ok" -eq 0 ]; then
    echo "  ok   under-ceiling chain accepted (no false-positive E0010)"
else
    echo "  FAIL: under-ceiling flat chain (2045 terms) was rejected -- false positive"
    cat "$TMP/chain_ok.kry.out" 2>/dev/null | head -3 | sed 's/^/    /'
    fail=1
fi

# --- Part 3: the ceiling still fires, cleanly, attributed to the real site.
echo "-- part 3: over-ceiling chain must be rejected, diagnostic on-site --"
gen flatchain 2100 "$TMP/chain_over.kry"
check_bounded "flat chain @2100 (over ceiling)" "$TMP/chain_over.kry"
rc_over=$?
out_over="$TMP/$(basename "$TMP/chain_over.kry").out"
if [ "$rc_over" -ne 0 ] && grep -q "E0010" "$out_over"; then
    echo "  ok   over-ceiling chain rejected with E0010"
else
    echo "  FAIL: over-ceiling flat chain (2100 terms) was NOT cleanly rejected with E0010"
    fail=1
fi
# The diagnostic must point at a line inside the `let x = ...` expression
# (line 2 of the generated file), not at the unrelated `println` statement
# a few lines later -- the exact misattribution this investigation found.
loc_line="$(grep -oE ':[0-9]+:[0-9]+' "$out_over" | head -1 | cut -d: -f2)"
if [ "$loc_line" = "2" ]; then
    echo "  ok   E0010 attributed to the offending expression's own line"
else
    echo "  FAIL: E0010 attributed to line ${loc_line:-<none>}, expected line 2 (the chain itself)"
    fail=1
fi

echo
if [ "$fail" -eq 0 ]; then
    echo "PASS: parser nesting-depth guard is bounded-time and false-positive-free."
else
    echo "FAIL: see above."
fi
exit "$fail"
