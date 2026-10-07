#!/usr/bin/env bash
# ownership_census.sh -- run the whole corpus on a BASE and a CANDIDATE kryos
# binary, both backends, and list every program whose behaviour differs.
#
# Why this exists (LEDGER items 3 + 51, 2026-10-06): ten ownership attempts
# each fixed one aliasing shape, then discovered the next one only after the
# previous fix made drops "real". A census lists every shape in one sweep.
# Run it on a candidate BEFORE theorizing about the next shape.
#
# CAND runs under KRYOS_FREE_DIAG=1 (quarantine: freed blocks are never
# reused, every over-release is reported). Per program x backend it records:
# exit codes, stdout identical to BASE or not, and the double-free count.
# A use-after-free shows as a stdout/rc DIFF (quarantine poisons the block),
# not as a double-free count -- read both columns.
#
# Usage: tools/loop/ownership_census.sh <base-kryos> <cand-kryos> <out-dir>
# Output: <out-dir>/summary.tsv, plus per-run .out/.err under <out-dir>/w.
# Anomalies: grep -v -P '\tsame\tdf=0$' summary.tsv | grep -v 'base_rc=99'
# (base_rc=99 cand_rc=99 = the program does not build on either binary).
#
# A few examples write fixture files into the current directory -- run this
# from a throwaway directory, not the repo root.
set -u
BASE="$1"; CAND="$2"; OUT="$3"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
mkdir -p "$OUT/w"
SUM="$OUT/summary.tsv"; : > "$SUM"

# Network / interactive programs are skipped: they differ run to run.
NET=$(grep -l -E "http|tcp_|tls_|listen|stdin|read_line|https|serve" \
  "$ROOT"/examples/*.kry "$ROOT"/examples/showcase/*.kry)
CORPUS=$(ls "$ROOT"/tests/conformance/*.kry "$ROOT"/tests/mem/*.kry \
  "$ROOT"/compiler/self-host/regression_*.kry \
  "$ROOT"/examples/*.kry "$ROOT"/examples/showcase/*.kry | grep -vxF -f <(echo "$NET"))

run_one() { # bin backend file outprefix diag
  local bin="$1" be="$2" f="$3" o="$4" diag="$5"
  if [ "$be" = jit ]; then
    KRYOS_FREE_DIAG=$diag KRYOS_FREE_DIAG_STACK=$diag KRYOS_FREE_DIAG_MAX=50 \
      timeout 25 "$bin" run --capabilities-mode=permissive "$f" </dev/null >"$o.out" 2>"$o.err"
  else
    timeout 120 "$bin" build --release --capabilities-mode=permissive "$f" -o "$o.exe" >"$o.bout" 2>&1 \
      || { echo BUILD_FAIL >"$o.out"; return 99; }
    KRYOS_FREE_DIAG=$diag KRYOS_FREE_DIAG_STACK=$diag KRYOS_FREE_DIAG_MAX=50 \
      timeout 25 "$o.exe" </dev/null >"$o.out" 2>"$o.err"
  fi
}

for f in $CORPUS; do
  n=$(echo "${f#$ROOT/}" | tr '/\\' '__')
  for be in jit aot; do
    run_one "$BASE" $be "$f" "$OUT/w/$n.$be.base" 0; brc=$?
    run_one "$CAND" $be "$f" "$OUT/w/$n.$be.cand" 1; crc=$?
    df=$(grep -ciE "double.free|DOUBLE-FREE" "$OUT/w/$n.$be.cand.err" 2>/dev/null)
    if cmp -s "$OUT/w/$n.$be.base.out" "$OUT/w/$n.$be.cand.out"; then m=same; else m=DIFF; fi
    printf '%s\t%s\tbase_rc=%s\tcand_rc=%s\t%s\tdf=%s\n' "$n" $be $brc $crc $m "${df:-0}" >> "$SUM"
    rm -f "$OUT/w/$n.$be".*.exe "$OUT/w/$n.$be".*.pdb
  done
done
echo DONE >> "$SUM"
grep -v -P '\tsame\tdf=0$' "$SUM" | grep -v 'base_rc=99	cand_rc=99' | grep -v '^DONE$' || echo "ownership-census: no anomalies"
