#!/usr/bin/env bash
# mem_shared_field_store_gate.sh -- regression gate for the mem_attacks c06 shape: `(*h).f = v` through a `shared T`
# handle. On AOT the deref of a shared struct is an inline COPY of the payload, so the store landed in the copy
# (the write was lost); the first fix then leaked the whole-payload copy it read the old field through. Checks the
# answer (every iteration reads its own store back through the handle) and peak RSS on both backends; `control`
# (plain local struct field store) is the baseline. Windows-only (PowerShell PeakWorkingSet64).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
ITERS=1000000
WANT=1000000
CEIL_MB=24   # steady state ~3-8MB; pre-fix 40-120MB

if ! command -v powershell >/dev/null 2>&1; then
  echo "mem-shared-field-store: SKIP (no powershell -- this gate is Windows-only)"
  exit 0
fi

win_kryos="$(cygpath -w "$KRYOS" 2>/dev/null || echo "$KRYOS")"
win_stdlib="$(cygpath -w "$KRYOS_STDLIB_DIR" 2>/dev/null || echo "$KRYOS_STDLIB_DIR")"
probe="tests/mem/shared_field_store_leak.kry"
win_probe="$(cygpath -w "$ROOT/$probe" 2>/dev/null || echo "$ROOT/$probe")"
fail=0

aot_bin="$(mktemp -u).exe"
win_aot="$(cygpath -w "$aot_bin" 2>/dev/null || echo "$aot_bin")"
if ! "$KRYOS" build --release "$ROOT/$probe" -o "$aot_bin" >/dev/null 2>&1; then
  echo "mem-shared-field-store: AOT build FAILED"; exit 1
fi

for mode in name tags both copy control; do
  aot_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_ITERS='$ITERS'; \$env:LEAK_MODE='$mode'; \$p=Start-Process -FilePath '$win_aot' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\msf_${mode}_aot.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 20}; try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; \$m" 2>/dev/null | tr -d '\r')
  case "$aot_bytes" in ''|*[!0-9]*) aot_bytes="" ;; esac
  out="$(tr -d '\r' < "${TEMP:-/tmp}/msf_${mode}_aot.txt" 2>/dev/null | tr -d '[:space:]')"
  if [ -z "$aot_bytes" ]; then
    echo "mem-shared-field-store: $mode AOT -- could not read peak RSS"; fail=1
  else
    mb=$(( aot_bytes / 1024 / 1024 ))
    echo "mem-shared-field-store: $mode AOT peak RSS ${mb}MB (ceiling ${CEIL_MB}MB) answer=$out"
    [ "$mb" -gt "$CEIL_MB" ] && { echo "mem-shared-field-store: $mode AOT FAIL -- leak present"; fail=1; }
    [ "$out" = "$WANT" ] || { echo "mem-shared-field-store: $mode AOT FAIL -- wrong answer (want $WANT)"; fail=1; }
  fi

  # JIT: poll the exec'd child by its temp name.
  jit_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_ITERS='$ITERS'; \$env:LEAK_MODE='$mode'; \$env:KRYOS_STDLIB_DIR='$win_stdlib'; \$parent=Start-Process -FilePath '$win_kryos' -ArgumentList @('run','$win_probe') -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\msf_${mode}_jit.txt; \$m=0; while(-not \$parent.HasExited){try{\$c=Get-Process -Name 'shared_field_store_leak' -ErrorAction SilentlyContinue; if(\$c){foreach(\$x in @(\$c)){try{\$x.Refresh(); if(\$x.PeakWorkingSet64 -gt \$m){\$m=\$x.PeakWorkingSet64}}catch{}}}}catch{}; Start-Sleep -Milliseconds 15}; \$m" 2>/dev/null | tr -d '\r')
  case "$jit_bytes" in ''|*[!0-9]*) jit_bytes="" ;; esac
  out="$(tr -d '\r' < "${TEMP:-/tmp}/msf_${mode}_jit.txt" 2>/dev/null | tr -d '[:space:]')"
  if [ -z "$jit_bytes" ]; then
    echo "mem-shared-field-store: $mode JIT -- could not read peak RSS (rerun if this flakes)"; fail=1
  else
    mb=$(( jit_bytes / 1024 / 1024 ))
    echo "mem-shared-field-store: $mode JIT peak RSS ${mb}MB (ceiling ${CEIL_MB}MB) answer=$out"
    [ "$mb" -gt "$CEIL_MB" ] && { echo "mem-shared-field-store: $mode JIT FAIL -- leak present"; fail=1; }
    [ "$out" = "$WANT" ] || { echo "mem-shared-field-store: $mode JIT FAIL -- wrong answer (want $WANT)"; fail=1; }
  fi
done
rm -f "$aot_bin"

[ "$fail" -eq 0 ] && echo "mem-shared-field-store: PASS"
exit $fail
