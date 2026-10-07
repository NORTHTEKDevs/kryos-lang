#!/usr/bin/env bash
# mem_struct_field_read_gate.sh -- regression gate for LEDGER items 3 + 51
# (2026-10-06): the unbalanced struct FIELD-READ retain.
#
# Fixture: tests/mem/struct_field_read_leak.kry -- construct a struct with an
# array field and a map field, read both with len(), drop it, in a loop.
#
# Both backends retain an array/map field read (LLVM kryos_array_retain_opt /
# kryos_map_retain_opt, Cranelift kryos_array_retain / kryos_map_retain), but
# drop_unescaped_str_temps only ever dropped STR field-read temps. The retain
# was never balanced, so the struct's own drop saw rc > 1 and freed neither the
# container nor its elements -- with no call involved at all. This is the leak
# items 3 and 51 had been attributing to the call boundary and to field
# overwrite respectively.
#
# Measured (Windows, PeakWorkingSet64): pre-fix AOT 586MB at 1M and 1748MB at
# 3M iterations; post-fix 4.5MB / 4.0MB. Ceiling 50MB at 2M is ~20x below the
# pre-fix value and ~10x above steady state.
#
# The DANGEROUS direction is a double free, not a leak. That half is pinned by
# tests/no_double_free.sh (the stage1_mini_parser / lexer-reentrant cases this
# change initially broke on the JIT), the self-host bootstrap, and
# compiler/self-host/test_regressions.sh.
#
# Windows-only (PowerShell PeakWorkingSet64 polling, matching
# mem_plateau_check.sh's own fallback technique). Skips elsewhere.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
PROBE="$ROOT/tests/mem/struct_field_read_leak.kry"
ITERS=2000000
CEIL_MB=50   # steady state ~4-5MB; pre-fix AOT reached 586MB at 1M (map + array field reads)

if ! command -v powershell >/dev/null 2>&1; then
  echo "mem-struct-field-read: SKIP (no powershell -- this gate is Windows-only, matching mem_plateau_check.sh's fallback path)"
  exit 0
fi

win_kryos="$(cygpath -w "$KRYOS" 2>/dev/null || echo "$KRYOS")"
win_probe="$(cygpath -w "$PROBE" 2>/dev/null || echo "$PROBE")"
win_stdlib="$(cygpath -w "$KRYOS_STDLIB_DIR" 2>/dev/null || echo "$KRYOS_STDLIB_DIR")"
aot_bin="$(mktemp -u).exe"
win_aot="$(cygpath -w "$aot_bin" 2>/dev/null || echo "$aot_bin")"

"$KRYOS" build --release "$PROBE" -o "$aot_bin" >/dev/null 2>&1 || { echo "mem-struct-field-read: AOT build failed"; exit 1; }

fail=0

# --- AOT leg: measure the built binary directly. ---
aot_bytes=$(powershell -NoProfile -Command \
  "\$env:LEAK_ITERS='$ITERS'; \$p=Start-Process -FilePath '$win_aot' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\msfrg_aot.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 20}; try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; \$m" 2>/dev/null | tr -d '\r')
rm -f "$aot_bin"
case "$aot_bytes" in ''|*[!0-9]*) aot_bytes="" ;; esac
if [ -z "$aot_bytes" ]; then
  echo "mem-struct-field-read: AOT leg -- could not read peak RSS (powershell probe returned nothing)"
  fail=1
else
  aot_mb=$(( aot_bytes / 1024 / 1024 ))
  echo "mem-struct-field-read: AOT peak RSS ${aot_mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
  [ "$aot_mb" -gt "$CEIL_MB" ] && { echo "mem-struct-field-read: AOT FAIL -- leak reintroduced"; fail=1; }
fi

# --- JIT leg: `kryos run` execs the Cranelift-compiled binary as a CHILD
# process and deletes it on exit, so poll the child by its predictable temp
# name ("<stem>.exe" in $env:TEMP, per kryos-cli/src/commands/run.rs) rather
# than the outer `kryos.exe run` driver, which never shows the leak.
jit_bytes=$(powershell -NoProfile -Command \
  "\$env:LEAK_ITERS='$ITERS'; \$env:KRYOS_STDLIB_DIR='$win_stdlib'; \$parent=Start-Process -FilePath '$win_kryos' -ArgumentList @('run','$win_probe') -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\msfrg_jit.txt; \$m=0; while(-not \$parent.HasExited){try{\$c=Get-Process -Name 'struct_field_read_leak' -ErrorAction SilentlyContinue; if(\$c){foreach(\$x in @(\$c)){try{\$x.Refresh(); if(\$x.PeakWorkingSet64 -gt \$m){\$m=\$x.PeakWorkingSet64}}catch{}}}}catch{}; Start-Sleep -Milliseconds 15}; \$m" 2>/dev/null | tr -d '\r')
case "$jit_bytes" in ''|*[!0-9]*) jit_bytes="" ;; esac
if [ -z "$jit_bytes" ]; then
  echo "mem-struct-field-read: JIT leg -- could not read peak RSS (powershell probe returned nothing, or the child ran too briefly to sample -- rerun if this flakes)"
  fail=1
else
  jit_mb=$(( jit_bytes / 1024 / 1024 ))
  echo "mem-struct-field-read: JIT peak RSS ${jit_mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
  [ "$jit_mb" -gt "$CEIL_MB" ] && { echo "mem-struct-field-read: JIT FAIL -- leak reintroduced"; fail=1; }
fi

if [ "$fail" -eq 0 ]; then
  echo "mem-struct-field-read: PASS"
fi
exit $fail
