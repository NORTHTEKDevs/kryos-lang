#!/usr/bin/env bash
# mem_enum_nested_gate.sh -- peak-RSS gate for an enum nested DIRECTLY inside an
# enum (Option<Result<..>>, user enum holding user enum, list, expression tree)
# built fresh per iteration, matched, overwritten, passed to fns, stored in
# containers and captured by closures. Override: KRYOS_MEM_MODES="a b",
# KRYOS_MEM_ITERS=n. Double-free direction: no_double_free.sh enum_nested_ownership.
# Windows-only (PowerShell PeakWorkingSet64 polling). Skips elsewhere.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
PROBE="$ROOT/tests/mem/enum_nested_leak.kry"
ITERS="${KRYOS_MEM_ITERS:-1000000}"
CEIL_MB=40
MODES="${KRYOS_MEM_MODES:-idle user_build user_match user_call_temp user_overwrite user_copy opt_res_build opt_res_call opt_res_local opt_res_overwrite res_opt_call list_call list_overwrite tree_eval deep_call arr_store arr_push_pop map_store map_read_overwrite struct_field struct_lit closure_capture closure_local for_loop opt_array push_flat_call push_nested_local push_opt_call}"

if ! command -v powershell >/dev/null 2>&1; then
  echo "mem-enum-nested: SKIP (no powershell -- Windows-only, like the other mem_* gates)"
  exit 0
fi

win_kryos="$(cygpath -w "$KRYOS" 2>/dev/null || echo "$KRYOS")"
win_probe="$(cygpath -w "$PROBE" 2>/dev/null || echo "$PROBE")"
win_stdlib="$(cygpath -w "$KRYOS_STDLIB_DIR" 2>/dev/null || echo "$KRYOS_STDLIB_DIR")"
aot_bin="$(mktemp -u).exe"
win_aot="$(cygpath -w "$aot_bin" 2>/dev/null || echo "$aot_bin")"
trap 'rm -f "$aot_bin"' EXIT

"$KRYOS" build --release "$PROBE" -o "$aot_bin" >/dev/null 2>&1 || { echo "mem-enum-nested: AOT build failed"; exit 1; }

fail=0
check() { # leg mode bytes
  local leg="$1" mode="$2" bytes="$3"
  case "$bytes" in ''|*[!0-9]*) bytes="" ;; esac
  if [ -z "$bytes" ]; then
    echo "mem-enum-nested: $leg $mode -- could not read peak RSS (rerun if this flakes)"
    fail=1
    return
  fi
  local mb=$(( bytes / 1024 / 1024 ))
  echo "mem-enum-nested: $leg $mode peak RSS ${mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
  [ "$mb" -gt "$CEIL_MB" ] && { echo "mem-enum-nested: $leg $mode FAIL -- leak reintroduced"; fail=1; }
}

for mode in $MODES; do
  aot_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_MODE='$mode'; \$env:LEAK_ITERS='$ITERS'; \$p=Start-Process -FilePath '$win_aot' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\men_aot.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 20}; try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; \$m" 2>/dev/null | tr -d '\r')
  check AOT "$mode" "$aot_bytes"

  # `kryos run` execs the compiled program as a CHILD named after the probe
  # stem; poll that child, not the driver (which never shows the leak).
  jit_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_MODE='$mode'; \$env:LEAK_ITERS='$ITERS'; \$env:KRYOS_STDLIB_DIR='$win_stdlib'; \$parent=Start-Process -FilePath '$win_kryos' -ArgumentList @('run','$win_probe') -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\men_jit.txt; \$m=0; while(-not \$parent.HasExited){try{\$c=Get-Process -Name 'enum_nested_leak' -ErrorAction SilentlyContinue; if(\$c){foreach(\$x in @(\$c)){try{\$x.Refresh(); if(\$x.PeakWorkingSet64 -gt \$m){\$m=\$x.PeakWorkingSet64}}catch{}}}}catch{}; Start-Sleep -Milliseconds 15}; \$m" 2>/dev/null | tr -d '\r')
  check JIT "$mode" "$jit_bytes"
done

if [ "$fail" -eq 0 ]; then
  echo "mem-enum-nested: PASS"
fi
exit $fail
