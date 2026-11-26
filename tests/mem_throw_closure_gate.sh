#!/usr/bin/env bash
# mem_throw_closure_gate.sh -- peak-RSS gate for two Cranelift-only leaks
# (LEDGER "leakmode" entry, 2026-11-26), both measured on the JIT (`kryos run`)
# and on the Cranelift AOT (`kryos build`, no --release):
#   throw_locals / throw_loop  a function that `throw`s directly leaked every
#                              named local live at the throw
#   clo_nested_tuple(3)        a closure env capturing [[(str, i64)]] leaked
#                              the tuples inside the rows
# Also (2026-11-26, second round): a `throw` inside a try body (try_throw*,
# try_rethrow) and a closure env / struct field holding nested tuples
# (clo_tuple_*, clo_struct_rows, clo_flat_*) or calling a captured closure
# inline (clo_chain).
# Probe: tests/mem/throw_closure_leak.kry. On the unfixed compiler every leg is
# well over the ceiling (60-200MB at 300k); fixed they sit at the ~5MB floor.
# Honors KRYOS_BIN. Windows-only (PowerShell PeakWorkingSet64 polling); skips
# elsewhere, like the other mem_* gates.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
PROBE="$ROOT/tests/mem/throw_closure_leak.kry"
ITERS=300000
CEIL_MB=25
MODES="throw_locals throw_loop clo_nested_tuple clo_nested_tuple3 try_throw try_throw_loop try_rethrow clo_tuple_arr clo_tuple_tuple clo_struct_rows clo_flat_arr clo_flat_tuple clo_chain"

if ! command -v powershell >/dev/null 2>&1; then
  echo "mem-throw-closure: SKIP (no powershell -- Windows-only, like the other mem_* gates)"
  exit 0
fi

win_kryos="$(cygpath -w "$KRYOS" 2>/dev/null || echo "$KRYOS")"
win_probe="$(cygpath -w "$PROBE" 2>/dev/null || echo "$PROBE")"
win_stdlib="$(cygpath -w "$KRYOS_STDLIB_DIR" 2>/dev/null || echo "$KRYOS_STDLIB_DIR")"
aot_bin="$(mktemp -u).exe"
win_aot="$(cygpath -w "$aot_bin" 2>/dev/null || echo "$aot_bin")"
trap 'rm -f "$aot_bin"' EXIT

"$KRYOS" build "$PROBE" -o "$aot_bin" >/dev/null 2>&1 || { echo "mem-throw-closure: Cranelift AOT build failed"; exit 1; }

fail=0
check() { # leg mode bytes
  local leg="$1" mode="$2" bytes="$3"
  case "$bytes" in ''|*[!0-9]*) bytes="" ;; esac
  if [ -z "$bytes" ]; then
    echo "mem-throw-closure: $leg $mode -- could not read peak RSS (rerun if this flakes)"
    fail=1
    return
  fi
  local mb=$(( bytes / 1024 / 1024 ))
  echo "mem-throw-closure: $leg $mode peak RSS ${mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
  [ "$mb" -gt "$CEIL_MB" ] && { echo "mem-throw-closure: $leg $mode FAIL -- leak"; fail=1; }
}

for mode in $MODES; do
  aot_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_MODE='$mode'; \$env:LEAK_ITERS='$ITERS'; \$p=Start-Process -FilePath '$win_aot' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\metc_aot.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 20}; try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; \$m" 2>/dev/null | tr -d '\r')
  check CRANELIFT-AOT "$mode" "$aot_bytes"

  # `kryos run` execs the compiled program as a CHILD named after the probe
  # stem; poll that child, not the driver (which never shows the leak).
  jit_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_MODE='$mode'; \$env:LEAK_ITERS='$ITERS'; \$env:KRYOS_STDLIB_DIR='$win_stdlib'; \$parent=Start-Process -FilePath '$win_kryos' -ArgumentList @('run','$win_probe') -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\metc_jit.txt; \$m=0; while(-not \$parent.HasExited){try{\$c=Get-Process -Name 'throw_closure_leak' -ErrorAction SilentlyContinue; if(\$c){foreach(\$x in @(\$c)){try{\$x.Refresh(); if(\$x.PeakWorkingSet64 -gt \$m){\$m=\$x.PeakWorkingSet64}}catch{}}}}catch{}; Start-Sleep -Milliseconds 15}; \$m" 2>/dev/null | tr -d '\r')
  check JIT "$mode" "$jit_bytes"
done

if [ "$fail" -eq 0 ]; then
  echo "mem-throw-closure: PASS"
else
  exit 1
fi
