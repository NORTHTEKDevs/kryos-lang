#!/usr/bin/env bash
# mem_balance_gate.sh -- arc/box BALANCE gate (1.3.1 memory-fix release).
#
# Builds tests/mem/shared_leak.kry once per backend (LLVM: `build --release`,
# Cranelift: `build`), runs every LEAK_MODE with KRYOS_ALLOC_STATS=1 and a
# small fixed LEAK_ITERS, and reads the exit-time counts the runtime prints:
#
#   [alloc-stats] arc allocs=N frees=M live=L
#   [alloc-stats] box allocs=N frees=M live=L
#
# WHAT THIS PROVES, AND WHAT IT DOES NOT. "balanced" means every ARC block
# (`shared T`, closure environments) and every struct/enum BOX
# (`kryos_calloc`) the program allocated was released. It does NOT mean "no
# leak": strings, arrays, maps, raw alloc() and Range are NOT counted. The
# RSS-based gates (tests/mem_*_gate.sh) remain the proof for those.
#
# The state of every (mode, backend) is pinned EXACTLY in EXPECTED below
# (arc live, box live, and the program's result line, all deterministic at
# ITERS=3). Any difference FAILS:
#   - a live count < 0 on ANY row   -> FAIL (over-release / double free)
#   - a balanced row now leaking    -> FAIL (regression)
#   - a leaking row now balanced    -> FAIL (a fix landed: flip the entry)
#   - a leaking row with other live -> FAIL (partial fix, new leak, or a
#                                      growing leak: edit the table)
# so the gate is green on today's tree and ratchets as the ownership fixes
# land (LEDGER items 137, 138, 139).
#
# Guards against a blind gate:
#   - backend legs are proven: the table pins counts that differ by backend
#     (e.g. tree_drop box live 0 on LLVM vs 18420 on Cranelift) and a canary
#     checks plain_embedded box allocs (0 on LLVM, 3 on Cranelift), so a leg
#     that built the other backend FAILS;
#   - the number of rows checked must equal EXPECTED_ROWS, and every
#     `mode == "..."` branch of the probe must have a row on both legs;
#   - KRYOS_BIN, when set, must be executable (no silent fallback);
#   - with KRYOS_ALLOC_STATS unset (or falsy) there must be no report;
#   - KRYOS_FREE_DIAG set makes the gate refuse to run: that quarantine path
#     skips drop_fn, so the counts are not meaningful there.
# A hard fault (stack overflow, abort) prints no report; the gate requires
# rc=0 for every run.
#
# The table was measured on Windows (git-bash) only. Honors KRYOS_BIN and
# KRYOS_STDLIB_DIR like the other gates.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"

case "${KRYOS_FREE_DIAG:-}" in
  ""|0) ;;
  *) echo "mem-balance: FAIL -- KRYOS_FREE_DIAG is set; its quarantine path skips drop_fn so arc/box counts are not meaningful. Unset it."; exit 1 ;;
esac

if [ -n "${KRYOS_BIN:-}" ]; then
  KRYOS="$KRYOS_BIN"
  if [ ! -x "$KRYOS" ]; then
    echo "mem-balance: FAIL -- KRYOS_BIN='$KRYOS_BIN' is not executable (no fallback)"; exit 1
  fi
else
  KRYOS="$ROOT/compiler/target/release/kryos"
  [ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
  if [ ! -x "$KRYOS" ]; then
    echo "mem-balance: FAIL -- no compiler at compiler/target/release/kryos[.exe]; build it first"; exit 1
  fi
fi
PROBE="$ROOT/tests/mem/shared_leak.kry"
ITERS=3
EXPECTED_ROWS=16

# ---------------------------------------------------------------------------
# EXPECTED-STATE TABLE.
#   mode  backend  arc_live  box_live  result-line-at-ITERS=3
# Measured 2026-11-03 on master ef9a66b1 + the counter (kryos 1.3.0).
# Open defects (the non-zero rows):
#   tree_*, list_drop  item 137 (a `shared` payload is never freed; on the
#                      Cranelift backend the struct boxes behind it leak too)
#   shared_scalar      item 137 (a bare `shared <int>` arc block is never
#                      released to zero; invisible to RSS)
#   match_field        item 139 (match on an Option field of a by-value param)
# ---------------------------------------------------------------------------
EXPECTED='
tree_drop      llvm 6138  0     mode tree_drop sum 3
tree_drop      cl   6138  18420 mode tree_drop sum 3
tree_let_call  llvm 6138  0     mode tree_let_call sum 6141
tree_let_call  cl   6138  18423 mode tree_let_call sum 6141
tree_embedded  llvm 6138  0     mode tree_embedded sum 6141
tree_embedded  cl   6138  18423 mode tree_embedded sum 6141
list_drop      llvm 6000  0     mode list_drop sum 3
list_drop      cl   6000  12003 mode list_drop sum 3
match_field    llvm 0     3     mode match_field sum 39
match_field    cl   0     9     mode match_field sum 39
shared_scalar  llvm 3     0     mode shared_scalar sum 3
shared_scalar  cl   3     0     mode shared_scalar sum 3
plain_embedded llvm 0     0     mode plain_embedded sum 93
plain_embedded cl   0     0     mode plain_embedded sum 93
array_tree     llvm 0     0     mode array_tree sum 6141
array_tree     cl   0     0     mode array_tree sum 6141
'
# Backend canary: plain_embedded box ALLOCS differ by backend (LLVM keeps the
# struct inline, Cranelift boxes it).
CANARY_LLVM_BOX_ALLOCS=0
CANARY_CL_BOX_ALLOCS=3
# Work canary: a balanced row is only evidence if the mode really allocates.
# array_tree builds 3 depth-10 trees of boxed nodes, so its box ALLOCS are
# pinned too; a probe branch gutted to allocate nothing fails here.
WORK_LLVM_BOX_ALLOCS=6138
WORK_CL_BOX_ALLOCS=6141

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
TO=""
command -v timeout >/dev/null 2>&1 && TO="timeout 120"
fail=0

build_leg() { # leg-name build-args...
  local leg="$1"; shift
  if ! $TO "$KRYOS" build "$@" "$PROBE" -o "$TMP/probe_$leg" >"$TMP/build_$leg.log" 2>&1; then
    echo "mem-balance: $leg build FAILED"; tail -5 "$TMP/build_$leg.log"
    return 1
  fi
}
build_leg llvm --release || exit 1
build_leg cl || exit 1

# stat_of <kind> <field> <stderr-file> -> value of `<field>=` on the
# `[alloc-stats] <kind>` line (empty when absent)
stat_of() {
  sed -n "s/^\[alloc-stats\] $1\(.*[ ]\)$2=\(-\{0,1\}[0-9][0-9]*\)\(\$\|[ ].*\)/\2/p" "$3" | tr -d '\r' | head -1
}

# With the variable unset, empty or falsy there must be no report, on BOTH
# legs. With an explicit truthy value (any case) there must be one.
for leg in llvm cl; do
  for v in __UNSET__ "" 0 false FALSE off no; do
    if [ "$v" = __UNSET__ ]; then
      ( unset KRYOS_ALLOC_STATS; LEAK_MODE=plain_embedded LEAK_ITERS=$ITERS $TO "$TMP/probe_$leg" ) >"$TMP/off.out" 2>"$TMP/off.err"
    else
      ( export KRYOS_ALLOC_STATS="$v"; LEAK_MODE=plain_embedded LEAK_ITERS=$ITERS $TO "$TMP/probe_$leg" ) >"$TMP/off.out" 2>"$TMP/off.err"
    fi
    rc=$?
    if [ "$rc" -ne 0 ] || grep -q "alloc-stats" "$TMP/off.out" "$TMP/off.err"; then
      echo "mem-balance: FAIL $leg -- KRYOS_ALLOC_STATS='${v/__UNSET__/<unset>}' must print no report (rc=$rc)"
      fail=1
    fi
  done
  for v in 1 TRUE Yes on; do
    ( export KRYOS_ALLOC_STATS="$v"; LEAK_MODE=plain_embedded LEAK_ITERS=$ITERS $TO "$TMP/probe_$leg" ) >"$TMP/on.out" 2>"$TMP/on.err"
    if ! grep -q '^\[alloc-stats\] arc ' "$TMP/on.err"; then
      echo "mem-balance: FAIL $leg -- KRYOS_ALLOC_STATS='$v' must print the report"
      fail=1
    fi
  done
done

# Every probe mode must have a row on both legs, and every row's mode must
# exist in the probe.
probe_modes="$(grep -o 'mode == "[a-z_]*"' "$PROBE" | sed 's/mode == "\(.*\)"/\1/' | sort -u)"
[ -n "$probe_modes" ] || { echo "mem-balance: FAIL -- found no modes in $PROBE"; fail=1; }
for m in $probe_modes; do
  for leg in llvm cl; do
    if ! printf '%s\n' "$EXPECTED" | grep -Eq "^$m[[:space:]]+$leg[[:space:]]"; then
      echo "mem-balance: FAIL -- probe mode '$m' has no EXPECTED row for leg '$leg'"
      fail=1
    fi
  done
done
for m in $(printf '%s\n' "$EXPECTED" | awk 'NF{print $1}' | sort -u); do
  printf '%s\n' "$probe_modes" | grep -qx "$m" || { echo "mem-balance: FAIL -- EXPECTED row mode '$m' is not a probe mode"; fail=1; }
done

rows=0
while read -r mode leg want_arc want_box result; do
  [ -z "$mode" ] && continue
  rows=$((rows + 1))
  exe="$TMP/probe_$leg"
  out="$TMP/$mode.$leg.out"; err="$TMP/$mode.$leg.err"
  ( unset KRYOS_FREE_DIAG; KRYOS_ALLOC_STATS=1 LEAK_MODE="$mode" LEAK_ITERS=$ITERS $TO "$exe" ) >"$out" 2>"$err"
  rc=$?
  line="$(grep -E '^mode ' "$out" | head -1 | tr -d '\r')"
  arc="$(stat_of arc live "$err")"; box="$(stat_of box live "$err")"
  if [ "$rc" -ne 0 ] || [ "$line" != "$result" ]; then
    echo "mem-balance: FAIL $mode/$leg -- rc=$rc result='$line' (want '$result')"
    fail=1; continue
  fi
  if [ -z "$arc" ] || [ -z "$box" ]; then
    echo "mem-balance: FAIL $mode/$leg -- missing an [alloc-stats] line on stderr (arc='$arc' box='$box')"
    fail=1; continue
  fi
  if [ "$arc" -lt 0 ] || [ "$box" -lt 0 ]; then
    echo "mem-balance: FAIL $mode/$leg NEGATIVE LIVE (arc live=$arc, box live=$box): more frees than allocs, an over-release / double free"
    fail=1; continue
  fi
  if [ "$arc" -eq "$want_arc" ] && [ "$box" -eq "$want_box" ]; then
    if [ "$want_arc" -eq 0 ] && [ "$want_box" -eq 0 ]; then st=balanced; else st="leaking as pinned"; fi
    echo "mem-balance: ok   $mode/$leg $st (arc live=$arc, box live=$box)"
  elif [ "$want_arc" -eq 0 ] && [ "$want_box" -eq 0 ]; then
    echo "mem-balance: FAIL $mode/$leg REGRESSION -- expected balanced, now arc live=$arc, box live=$box"
    fail=1
  elif [ "$arc" -eq 0 ] && [ "$box" -eq 0 ]; then
    echo "mem-balance: FAIL $mode/$leg FIXED -- expected arc live=$want_arc box live=$want_box, now balanced: set this EXPECTED row to 0 0 in tests/mem_balance_gate.sh in the same change"
    fail=1
  else
    echo "mem-balance: FAIL $mode/$leg CHANGED -- expected arc live=$want_arc box live=$want_box, got arc live=$arc box live=$box (partial fix, new leak or growing leak): update the EXPECTED row in the same change"
    fail=1
  fi
done <<EOF
$EXPECTED
EOF

if [ "$rows" -ne "$EXPECTED_ROWS" ]; then
  echo "mem-balance: FAIL -- checked $rows rows, expected $EXPECTED_ROWS (edit EXPECTED_ROWS together with the table)"
  fail=1
fi

# Backend canary.
for pair in "llvm:$CANARY_LLVM_BOX_ALLOCS" "cl:$CANARY_CL_BOX_ALLOCS"; do
  leg="${pair%%:*}"; want="${pair##*:}"
  KRYOS_ALLOC_STATS=1 LEAK_MODE=plain_embedded LEAK_ITERS=$ITERS $TO "$TMP/probe_$leg" >/dev/null 2>"$TMP/canary_$leg.err"
  got="$(stat_of box allocs "$TMP/canary_$leg.err")"
  if [ "$got" != "$want" ]; then
    echo "mem-balance: FAIL backend canary: leg '$leg' plain_embedded box allocs='$got', want $want -- this leg is not the backend it claims to be"
    fail=1
  else
    echo "mem-balance: ok   backend canary $leg (plain_embedded box allocs=$got)"
  fi
done

# Work canary.
for pair in "llvm:$WORK_LLVM_BOX_ALLOCS" "cl:$WORK_CL_BOX_ALLOCS"; do
  leg="${pair%%:*}"; want="${pair##*:}"
  KRYOS_ALLOC_STATS=1 LEAK_MODE=array_tree LEAK_ITERS=$ITERS $TO "$TMP/probe_$leg" >/dev/null 2>"$TMP/work_$leg.err"
  got="$(stat_of box allocs "$TMP/work_$leg.err")"
  if [ "$got" != "$want" ]; then
    echo "mem-balance: FAIL work canary: leg '$leg' array_tree box allocs='$got', want $want -- the balanced control no longer allocates what it should"
    fail=1
  else
    echo "mem-balance: ok   work canary $leg (array_tree box allocs=$got)"
  fi
done

if [ "$fail" -eq 0 ]; then
  echo "mem-balance: PASS ($rows rows)"
else
  echo "mem-balance: FAIL"
  exit 1
fi
