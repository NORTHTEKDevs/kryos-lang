#!/usr/bin/env bash
# parser_nesting_gate.sh -- regression gate for LEDGER item 22
# (parser nesting-depth guard: resource-DoS investigation), extended to
# also cover LEDGER item 14: a flat run of stray `;` at statement
# position used to recurse through parse_statement with NO nesting/depth
# guard at all -- the one recursive-descent entry point item 22's
# original sweep missed -- stack-overflowing `kryos check` natively,
# uncatchably, at ~500k semicolons (see the "semicolons" construct below).
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
#   4. (2026-10-15/31) An `elif let` / `elif` chain in expression position
#      is parsed in a loop and charged to the same 2048 AST-depth budget as a
#      flat operator chain: ~1500 clauses are accepted and run, a chain past
#      the budget is rejected with E0010, and a 50k-clause bomb yields one
#      E0010 in a few hundred bytes under check, run AND build.
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
# Honor KRYOS_BIN like security_gate.sh, so a caller comparing two binaries
# (candidate vs master) is not silently measuring the same one twice.
K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
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
        semicolons)
            { printf 'fn main() {\n'
              for ((i=0; i<depth; i++)); do printf ';'; done
              printf '\n println("done")\n}\n'
            } > "$out" ;;
        wideinterp)
            { printf 'fn main() {\n let x: i64 = 1\n let s = "'
              for ((i=0; i<depth; i++)); do printf '{x}'; done
              printf '"\n println(to_string(len(s)))\n}\n'
            } > "$out" ;;
        # An `elif let` chain. Recursion runs parse_if_rest -> parse_if_let ->
        # parse_if_let_else -> parse_if_rest, and each body's parse_block
        # charged AND RELEASED its own depth, so the chain itself accumulated
        # nothing and overflowed the native stack (rc 253, no diagnostic) at
        # ~50k clauses. tests/security/attack_parser_elif_let_chain_stack_overflow.kry
        eliflet)
            { printf 'fn main() {\n let v: i64 = 0\n if let 1 = v { println("a") }\n'
              for ((i=0; i<depth; i++)); do printf ' elif let %d = v { println("b") }\n' $((i+2)); done
              printf ' else { println("z") }\n}\n'
            } > "$out" ;;
        # The same chain in EXPRESSION position (a `let` initializer), which
        # recurses through parse_if_expr / parse_if_expr_after_if.
        # tests/security/attack_expr_if_elif_chain_stack_overflow.kry
        elifexpr)
            { printf 'fn main() {\n let a: bool = false\n let x: i64 = if a { 0 }\n'
              for ((i=0; i<depth; i++)); do printf ' elif a { %d }\n' "$i"; done
              printf ' else { -1 }\n println(to_string(x))\n}\n'
            } > "$out" ;;
        # The same expression chain as a CALL ARGUMENT (889 KB at 50k). The
        # abandoned tail used to parse as struct-literal fields, each emitting
        # its own unsuppressed error: 5.9 MB of diagnostics under `run`.
        elifcall)
            { printf 'fn main() {
 let a: bool = false
 println(to_string(if a { 0 }
'
              for ((i=0; i<depth; i++)); do printf ' elif a { %d }
' "$i"; done
              printf ' else { -1 }))
}
'
            } > "$out" ;;
        # Nested interpolation deep enough to overflow the LEXER, which runs
        # before the parser's guard can see a token: scan_string recurses into
        # scan_token on the nested literal
        # (tests/security/resource_dos_lexer_nested_interp_stack_overflow.kry,
        # 3 MB, rc 253 with no output).
        deepinterp)
            { printf 'fn main() {\n let s: str = "'
              for ((i=0; i<depth; i++)); do printf '{ "'; done
              printf 'x'
              for ((i=0; i<depth; i++)); do printf '" }'; done
              printf '"\n println(s)\n}\n'
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
gen semicolons    6000 "$TMP/semicolons.kry";    check_bounded "flat semicolon-recovery chain @6000 (LEDGER item 14)" "$TMP/semicolons.kry"

# --- Part 1b: the three unguarded recursions (2026-10-15). Each one
# stack-overflowed `kryos check` natively (rc 253) with NO diagnostic at the
# depths below; each must now be bounded AND rejected with E0010, and a
# legitimate shallow chain of the same shape must still be ACCEPTED.
echo "-- part 1b: elif-let / elif-expr / deep-interpolation recursion --"
for shape in eliflet elifexpr deepinterp; do
    case $shape in
        eliflet)    deep=20000; shallow=20;  label="elif-let chain" ;;
        elifexpr)   deep=20000; shallow=20;  label="elif chain in expression position" ;;
        # The LEXER overflow needs a quarter-million levels, far past where a
        # generated file is practical in bash and past the parser's own cap,
        # so this leg runs the committed 3 MB probe (rc 253 with no output
        # before the lexer gained its own depth counter) and generates only
        # the shallow control.
        deepinterp) deep=committed; shallow=8; label="nested interpolation" ;;
    esac
    if [ "$deep" = committed ]; then
        src=tests/security/resource_dos_lexer_nested_interp_stack_overflow.kry
        cp "$src" "$TMP/$shape.kry"
    else
        gen "$shape" "$deep" "$TMP/$shape.kry"
    fi
    check_bounded "$label @$deep" "$TMP/$shape.kry"
    rc_deep=$?
    out_deep="$TMP/$shape.kry.out"
    if [ "$rc_deep" -ne 0 ] && grep -q "E0010" "$out_deep"; then
        echo "  ok   $label @$deep rejected with E0010 (no native overflow)"
    else
        echo "  FAIL: $label @$deep was not cleanly rejected with E0010 (rc=$rc_deep)"
        head -3 "$out_deep" | cut -c1-160 | sed 's/^/    /'
        fail=1
    fi
    # The output must stay bounded too: one error on a multi-megabyte line
    # used to render the whole line plus a caret padded to its column.
    sz=$(wc -c < "$out_deep")
    if [ "$sz" -le 262144 ]; then
        echo "  ok   $label @$deep diagnostic output bounded (${sz}B)"
    else
        echo "  FAIL: $label @$deep printed ${sz}B of diagnostics"
        fail=1
    fi
    gen "$shape" "$shallow" "$TMP/${shape}_ok.kry"
    check_bounded "$label @$shallow (legitimate)" "$TMP/${shape}_ok.kry"
    if [ $? -eq 0 ]; then
        echo "  ok   $label @$shallow accepted (no false positive)"
    else
        echo "  FAIL: $label @$shallow rejected -- the guard over-rejects"
        head -3 "$TMP/${shape}_ok.kry.out" | cut -c1-160 | sed 's/^/    /'
        fail=1
    fi
done

# --- Part 1b-2: the chain limit is DOCUMENTED, not accidental. An
# `elif` / `elif let` chain is parsed in a loop and each clause costs one unit
# of the same 2048 AST-depth budget a flat operator chain pays, so a legitimate
# chain well past the old 250-clause recursion cap must be ACCEPTED and RUN
# correctly, and one past the budget must be rejected with E0010.
echo "-- part 1b-2: long legitimate chains (limit ~2048 clauses) --"
for shape in eliflet elifexpr; do
    gen "$shape" 1500 "$TMP/${shape}_1500.kry"
    check_bounded "$shape chain @1500 (legitimate)" "$TMP/${shape}_1500.kry"
    if [ $? -eq 0 ]; then
        echo "  ok   $shape chain @1500 accepted"
    else
        echo "  FAIL: $shape chain @1500 rejected -- over-rejects a valid flat chain"
        head -3 "$TMP/${shape}_1500.kry.out" | cut -c1-160 | sed 's/^/    /'
        fail=1
    fi
    gen "$shape" 2100 "$TMP/${shape}_2100.kry"
    check_bounded "$shape chain @2100 (over the budget)" "$TMP/${shape}_2100.kry"
    if [ $? -ne 0 ] && grep -q "E0010" "$TMP/${shape}_2100.kry.out"; then
        echo "  ok   $shape chain @2100 rejected with E0010"
    else
        echo "  FAIL: $shape chain @2100 was not rejected with E0010"
        fail=1
    fi
done
# RUN it, not just check it: the observable is the value the chain selects.
gen elifexpr 1500 "$TMP/elifexpr_run.kry"
got=$(timeout 120 "$K" run "$TMP/elifexpr_run.kry" 2>&1 | tr -d '')
if [ "$got" = "-1" ]; then
    echo "  ok   elif-expr chain @1500 runs and selects the else value (-1)"
else
    echo "  FAIL: elif-expr chain @1500 printed '${got:0:80}' (want -1)"
    fail=1
fi
gen eliflet 1500 "$TMP/eliflet_run.kry"
got=$(timeout 120 "$K" run "$TMP/eliflet_run.kry" 2>&1 | tr -d '')
if [ "$got" = "z" ]; then
    echo "  ok   elif-let chain @1500 runs and takes the else arm (z)"
else
    echo "  FAIL: elif-let chain @1500 printed '${got:0:80}' (want z)"
    fail=1
fi
# The abandoned chain must yield ONE E0010 and a small output on EVERY path,
# not only `check`: `run` and `build` printed 5.9 MB for the call-argument bomb.
gen elifcall 50000 "$TMP/elifcall.kry"
for mode in check run build; do
    case $mode in
        check) cmd=(check) ;;
        run)   cmd=(run) ;;
        build) cmd=(build -o "$TMP/elifcall.exe") ;;
    esac
    timeout 120 "$K" "${cmd[@]}" "$TMP/elifcall.kry" > "$TMP/elifcall.$mode.out" 2>&1
    rc=$?
    sz=$(wc -c < "$TMP/elifcall.$mode.out")
    if [ "$rc" -ne 0 ] && [ "$rc" -ne 124 ] && grep -q "E0010" "$TMP/elifcall.$mode.out" && [ "$sz" -le 4096 ]; then
        echo "  ok   50k-clause call-argument bomb under '$mode': E0010, ${sz}B"
    else
        echo "  FAIL: 50k-clause call-argument bomb under '$mode': rc=$rc, ${sz}B (want E0010 in <= 4096B)"
        fail=1
    fi
done

# --- Part 1c: the error path itself must be bounded (2026-10-15). These two
# need no deep nesting, so they isolate the rendering/printing fix from the
# recursion guards above. On master the long line renders in full (4 MB for a
# 2 MB line) and all 500 diagnostics print.
echo "-- part 1c: diagnostic output is bounded (long line, many errors) --"
{ printf 'fn main() {\n let x: i64 = "'; head -c 2000000 /dev/zero | tr '\0' 'a'
  printf '"\n println(to_string(x))\n}\n'; } > "$TMP/longline.kry"
check_bounded "single 2 MB line with one type error" "$TMP/longline.kry"
sz=$(wc -c < "$TMP/longline.kry.out")
if grep -q "E0100" "$TMP/longline.kry.out" && [ "$sz" -le 4096 ]; then
    echo "  ok   long-line diagnostic is a bounded window (${sz}B for a 2000000B line)"
else
    echo "  FAIL: long-line diagnostic printed ${sz}B (want the E0100 error in <= 4096B)"
    fail=1
fi
{ printf 'fn main() {\n'; for ((i=0; i<500; i++)); do printf ' let a%d: i64 = "s"\n' "$i"; done; printf '}\n'; } > "$TMP/manyerr.kry"
check_bounded "500 distinct type errors" "$TMP/manyerr.kry"
shown=$(grep -c "E0100" "$TMP/manyerr.kry.out")
if [ "$shown" -le 100 ] && grep -q "400 more diagnostics not shown" "$TMP/manyerr.kry.out" \
   && grep -q "500 errors" "$TMP/manyerr.kry.out"; then
    echo "  ok   500 errors print ${shown} diagnostics, '400 more' footer, count still 500"
else
    echo "  FAIL: 500 errors printed ${shown} diagnostics (want <= 100, a '400 more' footer, and '500 errors' in the summary)"
    fail=1
fi

# The same cap holds for `run` and `build`, not only `check`.
for mode in run build; do
    case $mode in
        run)   cmd=(run) ;;
        build) cmd=(build -o "$TMP/manyerr.exe") ;;
    esac
    timeout 120 "$K" "${cmd[@]}" "$TMP/manyerr.kry" > "$TMP/manyerr.$mode.out" 2>&1
    shown=$(grep -c "E0100" "$TMP/manyerr.$mode.out")
    if [ "$shown" -le 100 ] && grep -q "400 more diagnostics not shown" "$TMP/manyerr.$mode.out"; then
        echo "  ok   '$mode' prints ${shown} of 500 diagnostics with a '400 more' footer"
    else
        echo "  FAIL: '$mode' printed ${shown} diagnostics for 500 errors (want <= 100 and a '400 more' footer)"
        fail=1
    fi
done

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
    echo "PASS: parser nesting-depth guard is bounded-time; chains up to the 2048-level budget are accepted."
else
    echo "FAIL: see above."
fi
exit "$fail"
