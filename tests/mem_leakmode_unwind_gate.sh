#!/usr/bin/env bash
# mem_leakmode_unwind_gate.sh -- peak-RSS + double-free gate for exception
# unwinding and closure-env / struct-field release (LEDGER "leakmode" entry,
# 2026-11-27). Measured on the Cranelift AOT (`kryos build`) and on the
# `kryos run` path (the compiled program runs as a CHILD named after the probe).
#   unwind modes        a throw (direct, in a try body, or from a callee caught in
#                       the SAME function) must release the frame's named locals,
#                       for-loop iterables and a freshly built message
#   outer_throw/succ/   a local the MIR already released must not be released a
#   block               second time by the unwind cleanup of a LATER throwing call
#                       (KRYOS_FREE_DIAG reports it)
#   clo_* / s_reset     a closure env / struct field holding a tuple at any depth
# Probe: tests/mem/leakmode_unwind.kry. Honors KRYOS_BIN. Windows-only
# (PowerShell PeakWorkingSet64 polling); skips elsewhere, like the other mem_* gates.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
PROBE="$ROOT/tests/mem/leakmode_unwind.kry"
ITERS=300000
CEIL_MB=12
LEAK_MODES="callee_in_try throw_in_try struct_tuple loop_in_try nested throw_in_catch dyn_throw direct_catch outer_throw clo_nta clo_nta3 clo_tuple_struct clo_arr_tuple_struct clo_tuple_nested clo_deep clo_struct_rows s_reset"
DIAG_MODES="outer_throw callee_in_try throw_in_catch"

if ! command -v powershell >/dev/null 2>&1; then
  echo "mem-leakmode-unwind: SKIP (no powershell -- Windows-only, like the other mem_* gates)"
  exit 0
fi

win_kryos="$(cygpath -w "$KRYOS" 2>/dev/null || echo "$KRYOS")"
win_probe="$(cygpath -w "$PROBE" 2>/dev/null || echo "$PROBE")"
win_stdlib="$(cygpath -w "$KRYOS_STDLIB_DIR" 2>/dev/null || echo "$KRYOS_STDLIB_DIR")"
aot_bin="$(mktemp -u).exe"
win_aot="$(cygpath -w "$aot_bin" 2>/dev/null || echo "$aot_bin")"
trap 'rm -f "$aot_bin"' EXIT

"$KRYOS" build "$PROBE" -o "$aot_bin" >/dev/null 2>&1 || { echo "mem-leakmode-unwind: Cranelift AOT build failed"; exit 1; }

fail=0
check() { # leg mode bytes
  local leg="$1" mode="$2" bytes="$3"
  case "$bytes" in ''|*[!0-9]*) bytes="" ;; esac
  if [ -z "$bytes" ]; then
    echo "mem-leakmode-unwind: $leg $mode -- could not read peak RSS (rerun if this flakes)"
    fail=1
    return
  fi
  local mb=$(( bytes / 1024 / 1024 ))
  echo "mem-leakmode-unwind: $leg $mode peak RSS ${mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
  [ "$mb" -gt "$CEIL_MB" ] && { echo "mem-leakmode-unwind: $leg $mode FAIL -- leak"; fail=1; }
}

for mode in $LEAK_MODES; do
  aot_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_MODE='$mode'; \$env:LEAK_ITERS='$ITERS'; \$p=Start-Process -FilePath '$win_aot' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\mlu_aot.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 20}; try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; \$m" 2>/dev/null | tr -d '\r')
  check CRANELIFT-AOT "$mode" "$aot_bytes"

  # `kryos run` execs the compiled program as a CHILD named after the probe
  # stem; poll that child, not the driver (which never shows the leak).
  jit_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_MODE='$mode'; \$env:LEAK_ITERS='$ITERS'; \$env:KRYOS_STDLIB_DIR='$win_stdlib'; \$parent=Start-Process -FilePath '$win_kryos' -ArgumentList @('run','$win_probe') -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\mlu_jit.txt; \$m=0; while(-not \$parent.HasExited){try{\$c=Get-Process -Name 'leakmode_unwind' -ErrorAction SilentlyContinue; if(\$c){foreach(\$x in @(\$c)){try{\$x.Refresh(); if(\$x.PeakWorkingSet64 -gt \$m){\$m=\$x.PeakWorkingSet64}}catch{}}}}catch{}; Start-Sleep -Milliseconds 15}; \$m" 2>/dev/null | tr -d '\r')
  check JIT "$mode" "$jit_bytes"
done

# Double-free leg: no KRYOS_FREE_DIAG report on the shapes that unwind through
# a frame whose locals were already released.
for mode in $DIAG_MODES; do
  n=$(LEAK_MODE="$mode" LEAK_ITERS=3000 KRYOS_FREE_DIAG=1 "$aot_bin" 2>&1 >/dev/null | grep -c "KRYOS-FREE-DIAG")
  echo "mem-leakmode-unwind: CRANELIFT-AOT $mode free-diag reports: $n"
  [ "$n" -ne 0 ] && { echo "mem-leakmode-unwind: $mode FAIL -- double free"; fail=1; }
done

if [ "$fail" -eq 0 ]; then
  echo "mem-leakmode-unwind: PASS"
else
  exit 1
fi
