#!/usr/bin/env bash
# mem_tuple_field_overwrite_gate.sh -- regression gate for LEDGER item A15: a tuple-typed field of an
# owned struct (a local or a mutable module-level global) is owned by the struct, and overwriting it
# (`w.t = (s, n)`, `GW.t = ...`) did not release the old tuple: every replaced tuple leaked (118MB per
# million iterations on the JIT, 50MB on AOT). lower.rs now reads the old tuple before the store and
# drops it after, the new value takes its own reference unless it is a fresh tuple/call result, and a
# named tuple stored into the field keeps its own scope-end release.
# Checks the answer and peak RSS (1M iters) on both backends; `control` (a local tuple rebuilt in the
# loop) is the baseline. Windows-only (PowerShell PeakWorkingSet64).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
ITERS=1000000
WANT=6888890
CEIL_MB=24   # steady state ~5-8MB; pre-fix ~35-120MB

if ! command -v powershell >/dev/null 2>&1; then
  echo "mem-tuple-field-overwrite: SKIP (no powershell -- this gate is Windows-only)"
  exit 0
fi

win_kryos="$(cygpath -w "$KRYOS" 2>/dev/null || echo "$KRYOS")"
win_stdlib="$(cygpath -w "$KRYOS_STDLIB_DIR" 2>/dev/null || echo "$KRYOS_STDLIB_DIR")"
probe="tests/mem/tuple_field_overwrite_leak.kry"
win_probe="$(cygpath -w "$ROOT/$probe" 2>/dev/null || echo "$ROOT/$probe")"
fail=0

aot_bin="$(mktemp -u).exe"
win_aot="$(cygpath -w "$aot_bin" 2>/dev/null || echo "$aot_bin")"
if ! "$KRYOS" build --release "$ROOT/$probe" -o "$aot_bin" >/dev/null 2>&1; then
  echo "mem-tuple-field-overwrite: AOT build FAILED"; exit 1
fi

for mode in loc_field glob_field field_named field_self field_read field_call field_global control; do
  aot_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_ITERS='$ITERS'; \$env:LEAK_MODE='$mode'; \$p=Start-Process -FilePath '$win_aot' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\mtfo_${mode}_aot.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 20}; try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; \$m" 2>/dev/null | tr -d '\r')
  case "$aot_bytes" in ''|*[!0-9]*) aot_bytes="" ;; esac
  out="$(tr -d '\r' < "${TEMP:-/tmp}/mtfo_${mode}_aot.txt" 2>/dev/null | tr -d '[:space:]')"
  if [ -z "$aot_bytes" ]; then
    echo "mem-tuple-field-overwrite: $mode AOT -- could not read peak RSS"; fail=1
  else
    mb=$(( aot_bytes / 1024 / 1024 ))
    echo "mem-tuple-field-overwrite: $mode AOT peak RSS ${mb}MB (ceiling ${CEIL_MB}MB) answer=$out"
    [ "$mb" -gt "$CEIL_MB" ] && { echo "mem-tuple-field-overwrite: $mode AOT FAIL -- leak present"; fail=1; }
    [ "$out" = "$WANT" ] || { echo "mem-tuple-field-overwrite: $mode AOT FAIL -- wrong answer (want $WANT)"; fail=1; }
  fi

  # JIT: poll the exec'd child by its temp name.
  jit_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_ITERS='$ITERS'; \$env:LEAK_MODE='$mode'; \$env:KRYOS_STDLIB_DIR='$win_stdlib'; \$parent=Start-Process -FilePath '$win_kryos' -ArgumentList @('run','$win_probe') -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\mtfo_${mode}_jit.txt; \$m=0; while(-not \$parent.HasExited){try{\$c=Get-Process -Name 'tuple_field_overwrite_leak' -ErrorAction SilentlyContinue; if(\$c){foreach(\$x in @(\$c)){try{\$x.Refresh(); if(\$x.PeakWorkingSet64 -gt \$m){\$m=\$x.PeakWorkingSet64}}catch{}}}}catch{}; Start-Sleep -Milliseconds 15}; \$m" 2>/dev/null | tr -d '\r')
  case "$jit_bytes" in ''|*[!0-9]*) jit_bytes="" ;; esac
  out="$(tr -d '\r' < "${TEMP:-/tmp}/mtfo_${mode}_jit.txt" 2>/dev/null | tr -d '[:space:]')"
  if [ -z "$jit_bytes" ]; then
    echo "mem-tuple-field-overwrite: $mode JIT -- could not read peak RSS (rerun if this flakes)"; fail=1
  else
    mb=$(( jit_bytes / 1024 / 1024 ))
    echo "mem-tuple-field-overwrite: $mode JIT peak RSS ${mb}MB (ceiling ${CEIL_MB}MB) answer=$out"
    [ "$mb" -gt "$CEIL_MB" ] && { echo "mem-tuple-field-overwrite: $mode JIT FAIL -- leak present"; fail=1; }
    [ "$out" = "$WANT" ] || { echo "mem-tuple-field-overwrite: $mode JIT FAIL -- wrong answer (want $WANT)"; fail=1; }
  fi
done
rm -f "$aot_bin"

[ "$fail" -eq 0 ] && echo "mem-tuple-field-overwrite: PASS"
exit $fail
