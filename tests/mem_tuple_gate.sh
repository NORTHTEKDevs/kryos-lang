#!/usr/bin/env bash
# mem_tuple_gate.sh -- peak-RSS gate for tuples holding heap values
# (2026-10-08): a singly-owned tuple releases its heap elements (Cranelift
# KryosArray slots, LLVM inline aggregate); destructuring and reassignment
# release the tuple they consumed. 81c61df6 at 1M: 10 of 12 legs over the
# ceiling; fixed ~4MB. Double-free direction: no_double_free.sh tuple_ownership.
# Windows-only (PowerShell PeakWorkingSet64 polling). Skips elsewhere.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
PROBE="$ROOT/tests/mem/tuple_leak.kry"
ITERS=1000000
CEIL_MB=40
MODES="literal destructure destruct_p ret_param reassign field_idx call_arg alias"

if ! command -v powershell >/dev/null 2>&1; then
  echo "mem-tuple: SKIP (no powershell -- Windows-only, like the other mem_* gates)"
  exit 0
fi

win_kryos="$(cygpath -w "$KRYOS" 2>/dev/null || echo "$KRYOS")"
win_probe="$(cygpath -w "$PROBE" 2>/dev/null || echo "$PROBE")"
win_stdlib="$(cygpath -w "$KRYOS_STDLIB_DIR" 2>/dev/null || echo "$KRYOS_STDLIB_DIR")"
aot_bin="$(mktemp -u).exe"
win_aot="$(cygpath -w "$aot_bin" 2>/dev/null || echo "$aot_bin")"
trap 'rm -f "$aot_bin"' EXIT

"$KRYOS" build --release "$PROBE" -o "$aot_bin" >/dev/null 2>&1 || { echo "mem-tuple: AOT build failed"; exit 1; }

fail=0
check() { # leg mode bytes
  local leg="$1" mode="$2" bytes="$3"
  case "$bytes" in ''|*[!0-9]*) bytes="" ;; esac
  if [ -z "$bytes" ]; then
    echo "mem-tuple: $leg $mode -- could not read peak RSS (rerun if this flakes)"
    fail=1
    return
  fi
  local mb=$(( bytes / 1024 / 1024 ))
  echo "mem-tuple: $leg $mode peak RSS ${mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
  [ "$mb" -gt "$CEIL_MB" ] && { echo "mem-tuple: $leg $mode FAIL -- leak reintroduced"; fail=1; }
}

for mode in $MODES; do
  aot_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_MODE='$mode'; \$env:LEAK_ITERS='$ITERS'; \$p=Start-Process -FilePath '$win_aot' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\metu_aot.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 20}; try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; \$m" 2>/dev/null | tr -d '\r')
  check AOT "$mode" "$aot_bytes"

  # `kryos run` execs the compiled program as a CHILD named after the probe
  # stem; poll that child, not the driver (which never shows the leak).
  jit_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_MODE='$mode'; \$env:LEAK_ITERS='$ITERS'; \$env:KRYOS_STDLIB_DIR='$win_stdlib'; \$parent=Start-Process -FilePath '$win_kryos' -ArgumentList @('run','$win_probe') -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\metu_jit.txt; \$m=0; while(-not \$parent.HasExited){try{\$c=Get-Process -Name 'tuple_leak' -ErrorAction SilentlyContinue; if(\$c){foreach(\$x in @(\$c)){try{\$x.Refresh(); if(\$x.PeakWorkingSet64 -gt \$m){\$m=\$x.PeakWorkingSet64}}catch{}}}}catch{}; Start-Sleep -Milliseconds 15}; \$m" 2>/dev/null | tr -d '\r')
  check JIT "$mode" "$jit_bytes"
done

if [ "$fail" -eq 0 ]; then
  echo "mem-tuple: PASS"
fi
exit $fail
