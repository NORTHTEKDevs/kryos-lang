#!/usr/bin/env bash
# mem_field_assign_temp_gate.sh -- regression gate for LEDGER item 54 (the
# struct-field-assignment intermediate-temp leak).
#
# Fixture: tests/mem/field_assign_temp_leak.kry (`s.name = "v-" + to_string(i)`
# in a loop -- `to_string(i)` is an INTERMEDIATE temp feeding the concat that
# is actually stored).
#
# `drop_unescaped_str_temps` guards its whole window with a `_ => return`
# catch-all over instruction shapes it does not model, and
# `Instruction::StoreField` was never on the allowlist -- so a field
# assignment aborted the pass for the ENTIRE statement and none of its temps
# were dropped. Same failure mode item 49 had to fix for
# `Instruction::DropIfNe`, one statement shape over.
#
# Measured on the fixture, Windows, PowerShell PeakWorkingSet64 polling:
# pre-fix AOT 32MB at 500k and 184MB at 3M iterations (~61MB/M); post-fix 4MB
# at 3M. Isolated by a 2x2 first -- the same concat into a plain local, a
# field store of a bare literal, and a field read in a loop are each flat, and
# only the combination leaks. The fix lives in shared MIR lowering, so both
# legs are asserted here.
#
# The DANGEROUS direction for this fix is a double free, not a leak: the
# stored value is MOVED into the field and must not be dropped. That half is
# covered by tests/no_double_free.sh, the self-host bootstrap, and
# compiler/self-host/test_regressions.sh (the alloc_node / in-place-push
# shapes this pass historically broke), not by this ceiling.
#
# Windows-only (PowerShell PeakWorkingSet64 polling, matching
# mem_plateau_check.sh's own fallback technique) -- this is where this
# compiler is developed. Skips with a clear message elsewhere.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
PROBE="$ROOT/tests/mem/field_assign_temp_leak.kry"
ITERS=5000000
CEIL_MB=50   # steady state ~4MB both backends; pre-fix leak reaches ~305MB at this iter count (61MB/M x 5M)

if ! command -v powershell >/dev/null 2>&1; then
  echo "mem-field-assign-temp: SKIP (no powershell -- this gate is Windows-only, matching mem_plateau_check.sh's fallback path)"
  exit 0
fi

win_kryos="$(cygpath -w "$KRYOS" 2>/dev/null || echo "$KRYOS")"
win_probe="$(cygpath -w "$PROBE" 2>/dev/null || echo "$PROBE")"
win_stdlib="$(cygpath -w "$KRYOS_STDLIB_DIR" 2>/dev/null || echo "$KRYOS_STDLIB_DIR")"
aot_bin="$(mktemp -u).exe"
win_aot="$(cygpath -w "$aot_bin" 2>/dev/null || echo "$aot_bin")"

"$KRYOS" build --release "$PROBE" -o "$aot_bin" >/dev/null 2>&1 || { echo "mem-field-assign-temp: AOT build failed"; exit 1; }

fail=0

# --- AOT leg: measure the built binary directly. ---
aot_bytes=$(powershell -NoProfile -Command \
  "\$env:LEAK_ITERS='$ITERS'; \$p=Start-Process -FilePath '$win_aot' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\mfatg_aot.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 20}; try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; \$m" 2>/dev/null | tr -d '\r')
rm -f "$aot_bin"
case "$aot_bytes" in ''|*[!0-9]*) aot_bytes="" ;; esac
if [ -z "$aot_bytes" ]; then
  echo "mem-field-assign-temp: AOT leg -- could not read peak RSS (powershell probe returned nothing)"
  fail=1
else
  aot_mb=$(( aot_bytes / 1024 / 1024 ))
  echo "mem-field-assign-temp: AOT peak RSS ${aot_mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
  [ "$aot_mb" -gt "$CEIL_MB" ] && { echo "mem-field-assign-temp: AOT FAIL -- leak reintroduced"; fail=1; }
fi

# --- JIT leg: `kryos run` execs the Cranelift-compiled binary as a CHILD
# process and deletes it on exit, so poll the child by its predictable temp
# name ("<stem>.exe" in $env:TEMP, per kryos-cli/src/commands/run.rs) rather
# than the outer `kryos.exe run` driver, which never shows the leak (see the
# header comment's CORRECTION note).
jit_bytes=$(powershell -NoProfile -Command \
  "\$env:LEAK_ITERS='$ITERS'; \$env:KRYOS_STDLIB_DIR='$win_stdlib'; \$parent=Start-Process -FilePath '$win_kryos' -ArgumentList @('run','$win_probe') -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\mfatg_jit.txt; \$m=0; while(-not \$parent.HasExited){try{\$c=Get-Process -Name 'field_assign_temp_leak' -ErrorAction SilentlyContinue; if(\$c){foreach(\$x in @(\$c)){try{\$x.Refresh(); if(\$x.PeakWorkingSet64 -gt \$m){\$m=\$x.PeakWorkingSet64}}catch{}}}}catch{}; Start-Sleep -Milliseconds 15}; \$m" 2>/dev/null | tr -d '\r')
case "$jit_bytes" in ''|*[!0-9]*) jit_bytes="" ;; esac
if [ -z "$jit_bytes" ]; then
  echo "mem-field-assign-temp: JIT leg -- could not read peak RSS (powershell probe returned nothing, or the child ran too briefly to sample -- rerun if this flakes)"
  fail=1
else
  jit_mb=$(( jit_bytes / 1024 / 1024 ))
  echo "mem-field-assign-temp: JIT peak RSS ${jit_mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
  [ "$jit_mb" -gt "$CEIL_MB" ] && { echo "mem-field-assign-temp: JIT FAIL -- leak reintroduced"; fail=1; }
fi

if [ "$fail" -eq 0 ]; then
  echo "mem-field-assign-temp: PASS"
fi
exit $fail
