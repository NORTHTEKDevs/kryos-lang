#!/usr/bin/env bash
# mem_shadow_builtin_gate.sh -- regression gate for LEDGER item A13: a free function
# that shadows a builtin name (read_file, index_of, split) was still classed as the
# builtin by the MIR ownership rules, so a struct temp passed to it leaked its box and
# fields on every call. lower.rs now drops the name from builtin_fn_names when the
# program defines it. Pre-fix, 1M iters: ~+70MB per shadowed mode on JIT and AOT.
# Checks the answer (sum == iters) and peak RSS on both backends; the control mode
# (non-shadowing name) is the baseline. Windows-only (PowerShell PeakWorkingSet64).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
ITERS=1000000
CEIL_MB=24   # steady state ~5-8MB; pre-fix ~75MB

if ! command -v powershell >/dev/null 2>&1; then
  echo "mem-shadow-builtin: SKIP (no powershell -- this gate is Windows-only)"
  exit 0
fi

win_kryos="$(cygpath -w "$KRYOS" 2>/dev/null || echo "$KRYOS")"
win_stdlib="$(cygpath -w "$KRYOS_STDLIB_DIR" 2>/dev/null || echo "$KRYOS_STDLIB_DIR")"
probe="tests/mem/shadow_builtin_leak.kry"
win_probe="$(cygpath -w "$ROOT/$probe" 2>/dev/null || echo "$ROOT/$probe")"
fail=0

aot_bin="$(mktemp -u).exe"
win_aot="$(cygpath -w "$aot_bin" 2>/dev/null || echo "$aot_bin")"
if ! "$KRYOS" build --release "$ROOT/$probe" -o "$aot_bin" >/dev/null 2>&1; then
  echo "mem-shadow-builtin: AOT build FAILED"; exit 1
fi

for mode in read_file index_of split control; do
  aot_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_ITERS='$ITERS'; \$env:LEAK_MODE='$mode'; \$p=Start-Process -FilePath '$win_aot' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\msbg_${mode}_aot.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 20}; try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; \$m" 2>/dev/null | tr -d '\r')
  case "$aot_bytes" in ''|*[!0-9]*) aot_bytes="" ;; esac
  out="$(tr -d '\r' < "${TEMP:-/tmp}/msbg_${mode}_aot.txt" 2>/dev/null | tr -d '[:space:]')"
  if [ -z "$aot_bytes" ]; then
    echo "mem-shadow-builtin: $mode AOT -- could not read peak RSS"; fail=1
  else
    mb=$(( aot_bytes / 1024 / 1024 ))
    echo "mem-shadow-builtin: $mode AOT peak RSS ${mb}MB (ceiling ${CEIL_MB}MB) answer=$out"
    [ "$mb" -gt "$CEIL_MB" ] && { echo "mem-shadow-builtin: $mode AOT FAIL -- leak present"; fail=1; }
    [ "$out" = "$ITERS" ] || { echo "mem-shadow-builtin: $mode AOT FAIL -- wrong answer (want $ITERS)"; fail=1; }
  fi

  # JIT: poll the exec'd child by its temp name.
  jit_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_ITERS='$ITERS'; \$env:LEAK_MODE='$mode'; \$env:KRYOS_STDLIB_DIR='$win_stdlib'; \$parent=Start-Process -FilePath '$win_kryos' -ArgumentList @('run','$win_probe') -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\msbg_${mode}_jit.txt; \$m=0; while(-not \$parent.HasExited){try{\$c=Get-Process -Name 'shadow_builtin_leak' -ErrorAction SilentlyContinue; if(\$c){foreach(\$x in @(\$c)){try{\$x.Refresh(); if(\$x.PeakWorkingSet64 -gt \$m){\$m=\$x.PeakWorkingSet64}}catch{}}}}catch{}; Start-Sleep -Milliseconds 15}; \$m" 2>/dev/null | tr -d '\r')
  case "$jit_bytes" in ''|*[!0-9]*) jit_bytes="" ;; esac
  out="$(tr -d '\r' < "${TEMP:-/tmp}/msbg_${mode}_jit.txt" 2>/dev/null | tr -d '[:space:]')"
  if [ -z "$jit_bytes" ]; then
    echo "mem-shadow-builtin: $mode JIT -- could not read peak RSS (rerun if this flakes)"; fail=1
  else
    mb=$(( jit_bytes / 1024 / 1024 ))
    echo "mem-shadow-builtin: $mode JIT peak RSS ${mb}MB (ceiling ${CEIL_MB}MB) answer=$out"
    [ "$mb" -gt "$CEIL_MB" ] && { echo "mem-shadow-builtin: $mode JIT FAIL -- leak present"; fail=1; }
    [ "$out" = "$ITERS" ] || { echo "mem-shadow-builtin: $mode JIT FAIL -- wrong answer (want $ITERS)"; fail=1; }
  fi
done
rm -f "$aot_bin"

[ "$fail" -eq 0 ] && echo "mem-shadow-builtin: PASS"
exit $fail
