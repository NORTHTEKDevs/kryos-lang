#!/usr/bin/env bash
# mem_json_handle_gate.sh -- ownership of `json_*` node handles (LEDGER item A12).
# A node is an arc block in the runtime and its handle is a plain i64; the
# ownership pass (kryos-mir shared_own.rs, json_handle_locals) releases every
# local it can prove holds a node handle with no escaping integer copy.
#
# Builds tests/mem/json_handles.kry once per backend (LLVM `build --release`,
# Cranelift `build`), runs every LEAK_MODE with KRYOS_ALLOC_STATS=1 at a small
# fixed LEAK_ITERS and requires, for every (mode, backend):
#   - exit code 0 and the exact result line (a use-after-free prints garbage or
#     aborts; the lines are the master compiler's, which never freed a node),
#   - TRACKED modes: arc live = 0 (every node released),
#   - G modes: tracked, and the release exe's peak memory must not grow with the
#     iteration count (psutil), so a leak is caught even by a compiler that
#     cannot print the [alloc-stats] report,
#   - ESCAPE modes (a handle stored in a struct field or an array, or returned
#     by a helper that passes its parameter through): live >= 0 only. They leak
#     by design today; an over-release (negative live) still fails.
# A compiler that cannot print the [alloc-stats] report FAILS (it cannot prove
# the balance). Honors KRYOS_BIN and KRYOS_STDLIB_DIR like the other mem gates.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"

case "${KRYOS_FREE_DIAG:-}" in
  ""|0) ;;
  *) echo "mem-json-handle: FAIL -- KRYOS_FREE_DIAG is set; its quarantine path skips drop_fn so arc counts are not meaningful. Unset it."; exit 1 ;;
esac

if [ -n "${KRYOS_BIN:-}" ]; then
  KRYOS="$KRYOS_BIN"
  [ -x "$KRYOS" ] || { echo "mem-json-handle: FAIL -- KRYOS_BIN='$KRYOS_BIN' is not executable (no fallback)"; exit 1; }
else
  KRYOS="$ROOT/compiler/target/release/kryos"
  [ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
  [ -x "$KRYOS" ] || { echo "mem-json-handle: FAIL -- no compiler at compiler/target/release/kryos[.exe]; build it first"; exit 1; }
fi
PROBE="$ROOT/tests/mem/json_handles.kry"
ITERS=3

# kind(T = tracked, G = tracked + peak-memory growth check, E = escape)  mode  result-line-at-ITERS=3
EXPECTED='
T unused         mode unused sum 3
T read_chain     mode read_chain sum 96
T copy           mode copy sum 24
T child_outlives mode child_outlives sum 24
T error_handle   mode error_handle sum 9
T reassign       mode reassign sum 9
T cond_def       mode cond_def sum 5
T shared_child   mode shared_child sum 96
T build          mode build sum 51
T escape_fn      mode escape_fn sum 12
T escape_ret     mode escape_ret sum 3
G build_inline   mode build_inline sum 69
G helper_ret     mode helper_ret sum 111
G helper_borrow  mode helper_borrow sum 27
G child_via_fn   mode child_via_fn sum 12
E escape_field   mode escape_field sum 18
E escape_array   mode escape_array sum 18
E pass_through   mode pass_through sum 24
'

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
TO=""
command -v timeout >/dev/null 2>&1 && TO="timeout 120"
fail=0

build_leg() {
  local leg="$1"; shift
  if ! $TO "$KRYOS" build "$@" "$PROBE" -o "$TMP/probe_$leg" >"$TMP/build_$leg.log" 2>&1; then
    echo "mem-json-handle: $leg build FAILED"; tail -5 "$TMP/build_$leg.log"
    return 1
  fi
}
build_leg llvm --release || exit 1
build_leg cl || exit 1

stat_of() {
  sed -n "s/^\[alloc-stats\] $1\(.*[ ]\)$2=\(-\{0,1\}[0-9][0-9]*\)\(\$\|[ ].*\)/\2/p" "$3" | tr -d '\r' | head -1
}

# Peak working-set growth (MB) of the release exe between 20000 and 80000
# iterations: a per-iteration leak shows up here on ANY compiler, including one
# that cannot print the [alloc-stats] report. Prints "skip" without psutil.
growth_mb() {
  local exe="$1" mode="$2"
  command -v python3 >/dev/null 2>&1 || { echo skip; return; }
  python3 -c '
import os, subprocess, sys, time
try:
    import psutil
except ImportError:
    print("skip"); sys.exit(0)
exe, mode = sys.argv[1], sys.argv[2]
def peak(iters):
    env = dict(os.environ, LEAK_MODE=mode, LEAK_ITERS=str(iters))
    env.pop("KRYOS_ALLOC_STATS", None)
    p = subprocess.Popen([exe], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    pp = psutil.Process(p.pid)
    best = 0
    t0 = time.time()
    while p.poll() is None:
        if time.time() - t0 > 100:
            p.kill(); raise SystemExit("ERR timeout")
        try:
            best = max(best, pp.memory_info().peak_wset)
        except Exception:
            pass
        time.sleep(0.002)
    if p.returncode != 0:
        raise SystemExit("ERR rc=%d" % p.returncode)
    return best / 1048576.0
try:
    a = peak(20000); b = peak(80000)
    print("%.1f" % (b - a))
except SystemExit as e:
    print(e)
' "$exe" "$mode" 2>&1
}

probe_modes="$(grep -o 'mode == "[a-z_]*"' "$PROBE" | sed 's/mode == "\(.*\)"/\1/' | sort -u)"
[ -n "$probe_modes" ] || { echo "mem-json-handle: FAIL -- found no modes in $PROBE"; fail=1; }
for m in $probe_modes; do
  printf '%s\n' "$EXPECTED" | awk 'NF{print $2}' | grep -qx "$m" || { echo "mem-json-handle: FAIL -- probe mode '$m' has no EXPECTED row"; fail=1; }
done
for m in $(printf '%s\n' "$EXPECTED" | awk 'NF{print $2}' | sort -u); do
  printf '%s\n' "$probe_modes" | grep -qx "$m" || { echo "mem-json-handle: FAIL -- EXPECTED row mode '$m' is not a probe mode"; fail=1; }
done

rows=0
while read -r kind mode result; do
  [ -z "$mode" ] && continue
  for leg in llvm cl; do
    rows=$((rows + 1))
    exe="$TMP/probe_$leg"
    out="$TMP/$mode.$leg.out"; err="$TMP/$mode.$leg.err"
    ( unset KRYOS_FREE_DIAG; KRYOS_ALLOC_STATS=1 LEAK_MODE="$mode" LEAK_ITERS=$ITERS $TO "$exe" ) >"$out" 2>"$err"
    rc=$?
    line="$(grep -E '^mode ' "$out" | head -1 | tr -d '\r')"
    arc="$(stat_of arc live "$err")"
    if [ "$rc" -ne 0 ] || [ "$line" != "$result" ]; then
      echo "mem-json-handle: FAIL $mode/$leg -- rc=$rc result='$line' (want '$result')"
      fail=1; continue
    fi
    gfail=0
    if [ "$kind" = G ] && [ "$leg" = llvm ]; then
      grow="$(growth_mb "$exe" "$mode")"
      case "$grow" in
        skip) echo "mem-json-handle: note $mode/$leg growth check skipped (python3 + psutil not available)" ;;
        ERR*) echo "mem-json-handle: FAIL $mode/$leg growth check could not run: $grow"; fail=1; gfail=1 ;;
        *)
          if awk -v g="$grow" 'BEGIN{exit !(g > 10.0)}'; then
            echo "mem-json-handle: FAIL $mode/$leg LEAK -- peak memory grew ${grow}MB from 20000 to 80000 iterations (limit 10MB)"
            fail=1; gfail=1
          else
            echo "mem-json-handle: ok   $mode/$leg growth ${grow}MB over 60000 iterations"
          fi ;;
      esac
    fi
    if [ -z "$arc" ]; then
      echo "mem-json-handle: FAIL $mode/$leg -- no [alloc-stats] report on stderr: this compiler cannot prove the balance"
      fail=1; continue
    fi
    if [ "$arc" -lt 0 ]; then
      echo "mem-json-handle: FAIL $mode/$leg NEGATIVE LIVE (arc live=$arc): more frees than allocs, an over-release"
      fail=1; continue
    fi
    if [ "$kind" != E ] && [ "$arc" -ne 0 ]; then
      echo "mem-json-handle: FAIL $mode/$leg LEAK -- arc live=$arc"
      fail=1; continue
    fi
    [ "$gfail" -eq 0 ] || continue
    echo "mem-json-handle: ok   $mode/$leg ($kind, arc live=$arc)"
  done
done <<EOF
$EXPECTED
EOF

[ "$rows" -gt 0 ] || { echo "mem-json-handle: FAIL -- no rows ran"; fail=1; }
if [ "$fail" -eq 0 ]; then echo "mem-json-handle: PASS ($rows rows)"; exit 0; fi
echo "mem-json-handle: FAIL"; exit 1
