#!/usr/bin/env bash
# mem_shared_rule_gate.sh -- the ONE ownership rule for `shared` handles
# (LEDGER items 137, 138, 139): a handle slot owns exactly one reference, every
# copy retains, every overwrite and scope exit releases.
#
# Builds tests/mem/shared_rule.kry once per backend (LLVM: `build --release`,
# Cranelift: `build`), runs every LEAK_MODE with KRYOS_ALLOC_STATS=1 and a small
# fixed LEAK_ITERS, and requires, for every (mode, backend):
#   - exit code 0 and the exact result line (a use-after-free or a double free
#     prints garbage or aborts),
#   - arc live = 0 and box live = 0 on the exit-time report
#       [alloc-stats] arc allocs=N frees=M live=L
#       [alloc-stats] box allocs=N frees=M live=L
#     (every `shared` arc block, every struct/enum box released; negative =
#     an over-release).
# Each mode is a shape that an earlier attempt at items 137/139 leaked, freed
# twice or read after free (list rebuilt through a payload bind, a call result
# handed to a call, a struct holding a chain inside `try` with a throwing
# callee, a `return` out of a `for`, `s.items = push(s.items, a)`, a field read
# through a temporary handle, `return *s`, element / field overwrites, enum
# children, a `spawn` capture, a map value).
#
# A binary that cannot print the report (a compiler built without the counters)
# FAILS: it cannot prove the balance. KRYOS_FREE_DIAG makes the gate refuse to
# run: that quarantine path skips drop_fn, so the counts are not meaningful.
# Honors KRYOS_BIN and KRYOS_STDLIB_DIR like the other mem gates. Measured on
# Windows (git-bash).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"

case "${KRYOS_FREE_DIAG:-}" in
  ""|0) ;;
  *) echo "mem-shared-rule: FAIL -- KRYOS_FREE_DIAG is set; its quarantine path skips drop_fn so arc/box counts are not meaningful. Unset it."; exit 1 ;;
esac

if [ -n "${KRYOS_BIN:-}" ]; then
  KRYOS="$KRYOS_BIN"
  if [ ! -x "$KRYOS" ]; then
    echo "mem-shared-rule: FAIL -- KRYOS_BIN='$KRYOS_BIN' is not executable (no fallback)"; exit 1
  fi
else
  KRYOS="$ROOT/compiler/target/release/kryos"
  [ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
  if [ ! -x "$KRYOS" ]; then
    echo "mem-shared-rule: FAIL -- no compiler at compiler/target/release/kryos[.exe]; build it first"; exit 1
  fi
fi
PROBE="$ROOT/tests/mem/shared_rule.kry"
ITERS=3

# mode  result-line-at-ITERS=3 (checked by hand: see the shape of each mode in
# the probe; e.g. list_walk = 3 x (39 + 38 + ... + 0), enum_children = 3 x 16)
EXPECTED='
list_walk       mode list_walk sum 2340
list_return     mode list_return sum 117
pass_pass       mode pass_pass sum 48
try_temp        mode try_temp sum 41
for_return      mode for_return sum 6
field_push      mode field_push sum 30
temp_deref      mode temp_deref sum 48
two_structs     mode two_structs sum 54
deref_return    mode deref_return sum 51
array_overwrite mode array_overwrite sum 18
field_overwrite mode field_overwrite sum 6
enum_children   mode enum_children sum 48
spawn_handle    mode spawn_handle sum 3
map_value       mode map_value sum 3
'
# Backend canary: list_return builds 3 x 40 nodes; Cranelift boxes each node and
# its Option (240 boxes), LLVM keeps both inline (0), so a leg that built the
# other backend FAILS.
CANARY_MODE=list_return
CANARY_LLVM_BOX_ALLOCS=0
CANARY_CL_BOX_ALLOCS=240

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
TO=""
command -v timeout >/dev/null 2>&1 && TO="timeout 120"
fail=0

build_leg() { # leg-name build-args...
  local leg="$1"; shift
  if ! $TO "$KRYOS" build "$@" "$PROBE" -o "$TMP/probe_$leg" >"$TMP/build_$leg.log" 2>&1; then
    echo "mem-shared-rule: $leg build FAILED"; tail -5 "$TMP/build_$leg.log"
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

# Every probe mode must have a row, and every row's mode must exist in the probe.
probe_modes="$(grep -o 'mode == "[a-z_]*"' "$PROBE" | sed 's/mode == "\(.*\)"/\1/' | sort -u)"
[ -n "$probe_modes" ] || { echo "mem-shared-rule: FAIL -- found no modes in $PROBE"; fail=1; }
for m in $probe_modes; do
  printf '%s\n' "$EXPECTED" | grep -Eq "^$m[[:space:]]" || { echo "mem-shared-rule: FAIL -- probe mode '$m' has no EXPECTED row"; fail=1; }
done
for m in $(printf '%s\n' "$EXPECTED" | awk 'NF{print $1}' | sort -u); do
  printf '%s\n' "$probe_modes" | grep -qx "$m" || { echo "mem-shared-rule: FAIL -- EXPECTED row mode '$m' is not a probe mode"; fail=1; }
done

rows=0
while read -r mode result; do
  [ -z "$mode" ] && continue
  for leg in llvm cl; do
    rows=$((rows + 1))
    exe="$TMP/probe_$leg"
    out="$TMP/$mode.$leg.out"; err="$TMP/$mode.$leg.err"
    ( unset KRYOS_FREE_DIAG; KRYOS_ALLOC_STATS=1 LEAK_MODE="$mode" LEAK_ITERS=$ITERS $TO "$exe" ) >"$out" 2>"$err"
    rc=$?
    line="$(grep -E '^mode ' "$out" | head -1 | tr -d '\r')"
    arc="$(stat_of arc live "$err")"; box="$(stat_of box live "$err")"
    if [ "$rc" -ne 0 ] || [ "$line" != "$result" ]; then
      echo "mem-shared-rule: FAIL $mode/$leg -- rc=$rc result='$line' (want '$result')"
      fail=1; continue
    fi
    if [ -z "$arc" ] || [ -z "$box" ]; then
      echo "mem-shared-rule: FAIL $mode/$leg -- no [alloc-stats] report on stderr: this compiler cannot prove the balance"
      fail=1; continue
    fi
    if [ "$arc" -lt 0 ] || [ "$box" -lt 0 ]; then
      echo "mem-shared-rule: FAIL $mode/$leg NEGATIVE LIVE (arc live=$arc, box live=$box): more frees than allocs, an over-release / double free"
      fail=1; continue
    fi
    if [ "$arc" -ne 0 ] || [ "$box" -ne 0 ]; then
      echo "mem-shared-rule: FAIL $mode/$leg LEAK -- arc live=$arc, box live=$box"
      fail=1; continue
    fi
    echo "mem-shared-rule: ok   $mode/$leg balanced (arc live=0, box live=0)"
    if [ "$mode" = "$CANARY_MODE" ]; then
      allocs="$(stat_of box allocs "$err")"
      want="$CANARY_LLVM_BOX_ALLOCS"; [ "$leg" = cl ] && want="$CANARY_CL_BOX_ALLOCS"
      if [ "$allocs" != "$want" ]; then
        echo "mem-shared-rule: FAIL backend canary $leg ($CANARY_MODE box allocs=$allocs, want $want): the $leg leg did not run that backend"
        fail=1
      else
        echo "mem-shared-rule: ok   backend canary $leg ($CANARY_MODE box allocs=$allocs)"
      fi
    fi
  done
done <<EOF
$EXPECTED
EOF

expected_rows=$(( $(printf '%s\n' "$EXPECTED" | awk 'NF{n++} END{print n+0}') * 2 ))
if [ "$rows" -ne "$expected_rows" ]; then
  echo "mem-shared-rule: FAIL -- checked $rows rows, expected $expected_rows"
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  echo "mem-shared-rule: FAIL"
  exit 1
fi
echo "mem-shared-rule: PASS ($rows rows)"
