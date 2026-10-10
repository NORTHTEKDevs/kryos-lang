#!/usr/bin/env bash
# mem_enum_arg_gate.sh -- peak-RSS gate for enum-argument ownership
# (2026-10-08): a shareable enum argument (incl. Option<S> / Result<S, str>)
# is owned by the callee; a match arm's struct payload binding is shared and
# dropped per arm. Master at 1M (AOT): 96-433MB per mode; fixed ~4MB.
# Double-free direction: no_double_free.sh's enum_arg_ownership case.
# Windows-only (PowerShell PeakWorkingSet64 polling). Skips elsewhere.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
PROBE="$ROOT/tests/mem/enum_arg_leak.kry"
ITERS=1000000
CEIL_MB=40
MODES="enum_arg enum_temp_arg opt_arg res_arg match_bind ret struct_with_enum_arg field_overwrite"

if ! command -v powershell >/dev/null 2>&1; then
  echo "mem-enum-arg: SKIP (no powershell -- Windows-only, like the other mem_* gates)"
  exit 0
fi

win_kryos="$(cygpath -w "$KRYOS" 2>/dev/null || echo "$KRYOS")"
win_probe="$(cygpath -w "$PROBE" 2>/dev/null || echo "$PROBE")"
win_stdlib="$(cygpath -w "$KRYOS_STDLIB_DIR" 2>/dev/null || echo "$KRYOS_STDLIB_DIR")"
aot_bin="$(mktemp -u).exe"
win_aot="$(cygpath -w "$aot_bin" 2>/dev/null || echo "$aot_bin")"
trap 'rm -f "$aot_bin"' EXIT

"$KRYOS" build --release "$PROBE" -o "$aot_bin" >/dev/null 2>&1 || { echo "mem-enum-arg: AOT build failed"; exit 1; }

fail=0
check() { # leg mode bytes
  local leg="$1" mode="$2" bytes="$3"
  case "$bytes" in ''|*[!0-9]*) bytes="" ;; esac
  if [ -z "$bytes" ]; then
    echo "mem-enum-arg: $leg $mode -- could not read peak RSS (rerun if this flakes)"
    fail=1
    return
  fi
  local mb=$(( bytes / 1024 / 1024 ))
  echo "mem-enum-arg: $leg $mode peak RSS ${mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
  [ "$mb" -gt "$CEIL_MB" ] && { echo "mem-enum-arg: $leg $mode FAIL -- leak reintroduced"; fail=1; }
}

for mode in $MODES; do
  aot_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_MODE='$mode'; \$env:LEAK_ITERS='$ITERS'; \$p=Start-Process -FilePath '$win_aot' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\meag_aot.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 20}; try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; \$m" 2>/dev/null | tr -d '\r')
  check AOT "$mode" "$aot_bytes"

  # `kryos run` execs the compiled program as a CHILD named after the probe
  # stem; poll that child, not the driver (which never shows the leak).
  jit_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_MODE='$mode'; \$env:LEAK_ITERS='$ITERS'; \$env:KRYOS_STDLIB_DIR='$win_stdlib'; \$parent=Start-Process -FilePath '$win_kryos' -ArgumentList @('run','$win_probe') -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\meag_jit.txt; \$m=0; while(-not \$parent.HasExited){try{\$c=Get-Process -Name 'enum_arg_leak' -ErrorAction SilentlyContinue; if(\$c){foreach(\$x in @(\$c)){try{\$x.Refresh(); if(\$x.PeakWorkingSet64 -gt \$m){\$m=\$x.PeakWorkingSet64}}catch{}}}}catch{}; Start-Sleep -Milliseconds 15}; \$m" 2>/dev/null | tr -d '\r')
  check JIT "$mode" "$jit_bytes"
done

if [ "$fail" -eq 0 ]; then
  echo "mem-enum-arg: PASS"
fi
exit $fail
