#!/usr/bin/env bash
# determinism.sh MODULE [N] -- compile self-host/MODULE.kry N times with the
# CURRENT target/bootstrap/kryos-stage1.exe and report distinct md5 count.
# Honors whatever KRYOS_* env is exported by the caller (NO_ASLR, LEAK_ON_ZERO,
# SKIP_TYPES). Does NOT rebuild stage-1. Pure measurement.
set -u
cd "$(dirname "$0")/.."
mod="${1:?usage: determinism.sh MODULE [N]}"
N="${2:-5}"
src="self-host/${mod}.kry"
STAGE1=target/bootstrap/kryos-stage1.exe
[ ! -x "$STAGE1" ] && STAGE1=target/bootstrap/kryos-stage1
hashes=""
fails=0
for i in $(seq 1 "$N"); do
  out="/tmp/det_${mod}_$i.obj"
  rm -f "$out"
  KRYOS_SKIP_TYPES=1 "$STAGE1" obj "$src" -o "$out" >/dev/null 2>&1
  rc=$?
  if [ $rc -ne 0 ]; then fails=$((fails+1)); hashes="$hashes RC$rc"; continue; fi
  h=$(md5sum "$out" 2>/dev/null | cut -c1-12)
  hashes="$hashes $h"
done
distinct=$(echo $hashes | tr ' ' '\n' | sort -u | grep -c .)
echo "module=$mod N=$N distinct=$distinct fails=$fails  NO_ASLR=${KRYOS_NO_ASLR:-0} LEAK_ON_ZERO=${KRYOS_LEAK_ON_ZERO:-0}"
echo "  hashes:$hashes"
