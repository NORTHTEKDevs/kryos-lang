#!/usr/bin/env bash
# mem_struct_array_overwrite_gate.sh -- regression gate for LEDGER item A10:
# overwriting an array variable whose elements are structs / enums
# (`keep = []`, `keep = push(keep, r)` then a reset) freed only the array
# header and buffer. The reassignment release for an array local was the
# untyped `kryos_array_release_if_ne`, and `type_array_releases` (lower.rs)
# only upgraded it for str / array / map / shared / tuple elements, so every
# overwritten [Struct] / [Enum] leaked one element box plus its heap fields.
# Fixed by routing those arrays through the type-directed `DropIfNe` (the path
# an array of tuples already used; `array_overwrite_elem_releasable` in ir.rs).
# A [[Struct]] reset takes the same path (the element check recurses).
# Pre-fix, 1M iters: ~100MB on JIT and AOT, all three probes. Post-fix: ~4MB.
# Windows-only (PowerShell PeakWorkingSet64 polling, as mem_enum_overwrite_gate.sh).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
ITERS=1000000
CEIL_MB=40   # steady state ~4MB; pre-fix leak is ~100MB at this iter count

if ! command -v powershell >/dev/null 2>&1; then
  echo "mem-struct-array-overwrite: SKIP (no powershell -- this gate is Windows-only, matching mem_plateau_check.sh's fallback path)"
  exit 0
fi

win_kryos="$(cygpath -w "$KRYOS" 2>/dev/null || echo "$KRYOS")"
win_stdlib="$(cygpath -w "$KRYOS_STDLIB_DIR" 2>/dev/null || echo "$KRYOS_STDLIB_DIR")"

fail=0

run_leg() {
  # $1 = label, $2 = probe .kry path, $3 = child process name (no .exe)
  local label="$1" probe="$2" procname="$3"
  local win_probe aot_bin win_aot aot_bytes aot_mb jit_bytes jit_mb

  win_probe="$(cygpath -w "$ROOT/$probe" 2>/dev/null || echo "$ROOT/$probe")"
  aot_bin="$(mktemp -u).exe"
  win_aot="$(cygpath -w "$aot_bin" 2>/dev/null || echo "$aot_bin")"

  if ! "$KRYOS" build --release "$ROOT/$probe" -o "$aot_bin" >/dev/null 2>&1; then
    echo "mem-struct-array-overwrite: $1 -- AOT build FAILED"
    fail=1
    return
  fi

  aot_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_ITERS='$ITERS'; \$p=Start-Process -FilePath '$win_aot' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\msao_${1}_aot.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 20}; try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; \$m" 2>/dev/null | tr -d '\r')
  rm -f "$aot_bin"
  case "$aot_bytes" in ''|*[!0-9]*) aot_bytes="" ;; esac
  if [ -z "$aot_bytes" ]; then
    echo "mem-struct-array-overwrite: $1 AOT leg -- could not read peak RSS (powershell probe returned nothing)"
    fail=1
  else
    aot_mb=$(( aot_bytes / 1024 / 1024 ))
    echo "mem-struct-array-overwrite: $1 AOT peak RSS ${aot_mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
    [ "$aot_mb" -gt "$CEIL_MB" ] && { echo "mem-struct-array-overwrite: $1 AOT FAIL -- leak present"; fail=1; }
  fi

  # JIT leg: `kryos run` execs the Cranelift-compiled binary as a CHILD
  # process and deletes it on exit, so poll the child by its predictable
  # temp name rather than the outer `kryos.exe run` driver.
  jit_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_ITERS='$ITERS'; \$env:KRYOS_STDLIB_DIR='$win_stdlib'; \$parent=Start-Process -FilePath '$win_kryos' -ArgumentList @('run','$win_probe') -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\msao_${1}_jit.txt; \$m=0; while(-not \$parent.HasExited){try{\$c=Get-Process -Name '$procname' -ErrorAction SilentlyContinue; if(\$c){foreach(\$x in @(\$c)){try{\$x.Refresh(); if(\$x.PeakWorkingSet64 -gt \$m){\$m=\$x.PeakWorkingSet64}}catch{}}}}catch{}; Start-Sleep -Milliseconds 15}; \$m" 2>/dev/null | tr -d '\r')
  case "$jit_bytes" in ''|*[!0-9]*) jit_bytes="" ;; esac
  if [ -z "$jit_bytes" ]; then
    echo "mem-struct-array-overwrite: $1 JIT leg -- could not read peak RSS (powershell probe returned nothing, or the child ran too briefly to sample -- rerun if this flakes)"
    fail=1
  else
    jit_mb=$(( jit_bytes / 1024 / 1024 ))
    echo "mem-struct-array-overwrite: $1 JIT peak RSS ${jit_mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
    [ "$jit_mb" -gt "$CEIL_MB" ] && { echo "mem-struct-array-overwrite: $1 JIT FAIL -- leak present"; fail=1; }
  fi
}

run_leg struct "tests/mem/struct_array_overwrite_leak.kry" "struct_array_overwrite_leak"
run_leg enum "tests/mem/enum_array_reset_leak.kry" "enum_array_reset_leak"
run_leg nested "tests/mem/struct_array_nested_reset_leak.kry" "struct_array_nested_reset_leak"

if [ "$fail" -eq 0 ]; then
  echo "mem-struct-array-overwrite: PASS"
fi
exit $fail
