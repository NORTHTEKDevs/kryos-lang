#!/usr/bin/env bash
# mem_enum_array_push_gate.sh -- regression gate for LEDGER item 45 (the
# enum-array-push leak).
#
# Fixture: tests/mem/enum_array_push_leak.kry (a fresh Val.ListV([i64]) enum
# construction pushed into a scratch array, dropped every iteration). Before
# the fix, an EnumVariant construction with an Array-typed payload field
# UNCONDITIONALLY duped the field (kryos_array_dup) at the codegen level (both
# LLVM and Cranelift) with no matching MIR-level scope-end Drop for the
# now-orphaned pre-dup array literal -- kryos-mir's drop_unescaped_str_temps
# pass special-cased this exact shape for RValue::Struct (the "S { xs: [..] }"
# leak) but never got the matching RValue::EnumVariant arm, so it fell into
# the pass's generic "possible escape, don't drop" bucket instead.
#
# Measured at HEAD before the fix (this session, Windows, PowerShell
# PeakWorkingSet64 polling, both legs measuring the ACTUAL executing process
# -- see the JIT note below): AOT ~485MB, JIT ~485MB at 5,000,000 iterations
# (both backends -- NOT AOT-only, see below). Post-fix: both flat at ~4MB.
#
# CORRECTION to this item's original characterization ("AOT-only, JIT
# clean"): that was a measurement artifact, not a real backend difference.
# `kryos run` compiles via Cranelift and executes the result as a CHILD
# PROCESS (kryos-cli/src/commands/run.rs: `Command::new(bin).status()`), so
# polling PeakWorkingSet64 on the outer `kryos.exe run` process (which just
# blocks in `.status()`) reads the wrong process and shows near-flat memory
# regardless of what the child actually does. This gate polls the CHILD
# process by name for the JIT leg, which reproduces the AOT-scale growth.
# The fix lives in kryos-mir (shared MIR lowering consumed by both backends),
# so both legs are asserted here.
#
# Windows-only (PowerShell PeakWorkingSet64 polling, matching
# mem_plateau_check.sh's own fallback technique) -- this is where this
# compiler is developed. Skips with a clear message elsewhere.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
PROBE="$ROOT/tests/mem/enum_array_push_leak.kry"
ITERS=5000000
CEIL_MB=50   # steady state ~4MB both backends; pre-fix leak hits ~460MB at this iter count

if ! command -v powershell >/dev/null 2>&1; then
  echo "mem-enum-array-push: SKIP (no powershell -- this gate is Windows-only, matching mem_plateau_check.sh's fallback path)"
  exit 0
fi

win_kryos="$(cygpath -w "$KRYOS" 2>/dev/null || echo "$KRYOS")"
win_probe="$(cygpath -w "$PROBE" 2>/dev/null || echo "$PROBE")"
win_stdlib="$(cygpath -w "$KRYOS_STDLIB_DIR" 2>/dev/null || echo "$KRYOS_STDLIB_DIR")"
aot_bin="$(mktemp -u).exe"
win_aot="$(cygpath -w "$aot_bin" 2>/dev/null || echo "$aot_bin")"

"$KRYOS" build --release "$PROBE" -o "$aot_bin" >/dev/null 2>&1 || { echo "mem-enum-array-push: AOT build failed"; exit 1; }

fail=0

# --- AOT leg: measure the built binary directly. ---
aot_bytes=$(powershell -NoProfile -Command \
  "\$env:LEAK_ITERS='$ITERS'; \$p=Start-Process -FilePath '$win_aot' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\meapg_aot.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 20}; try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; \$m" 2>/dev/null | tr -d '\r')
rm -f "$aot_bin"
case "$aot_bytes" in ''|*[!0-9]*) aot_bytes="" ;; esac
if [ -z "$aot_bytes" ]; then
  echo "mem-enum-array-push: AOT leg -- could not read peak RSS (powershell probe returned nothing)"
  fail=1
else
  aot_mb=$(( aot_bytes / 1024 / 1024 ))
  echo "mem-enum-array-push: AOT peak RSS ${aot_mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
  [ "$aot_mb" -gt "$CEIL_MB" ] && { echo "mem-enum-array-push: AOT FAIL -- leak reintroduced"; fail=1; }
fi

# --- JIT leg: `kryos run` execs the Cranelift-compiled binary as a CHILD
# process and deletes it on exit, so poll the child by its predictable temp
# name ("<stem>.exe" in $env:TEMP, per kryos-cli/src/commands/run.rs) rather
# than the outer `kryos.exe run` driver, which never shows the leak (see the
# header comment's CORRECTION note).
jit_bytes=$(powershell -NoProfile -Command \
  "\$env:LEAK_ITERS='$ITERS'; \$env:KRYOS_STDLIB_DIR='$win_stdlib'; \$parent=Start-Process -FilePath '$win_kryos' -ArgumentList @('run','$win_probe') -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\meapg_jit.txt; \$m=0; while(-not \$parent.HasExited){try{\$c=Get-Process -Name 'enum_array_push_leak' -ErrorAction SilentlyContinue; if(\$c){foreach(\$x in @(\$c)){try{\$x.Refresh(); if(\$x.PeakWorkingSet64 -gt \$m){\$m=\$x.PeakWorkingSet64}}catch{}}}}catch{}; Start-Sleep -Milliseconds 15}; \$m" 2>/dev/null | tr -d '\r')
case "$jit_bytes" in ''|*[!0-9]*) jit_bytes="" ;; esac
if [ -z "$jit_bytes" ]; then
  echo "mem-enum-array-push: JIT leg -- could not read peak RSS (powershell probe returned nothing, or the child ran too briefly to sample -- rerun if this flakes)"
  fail=1
else
  jit_mb=$(( jit_bytes / 1024 / 1024 ))
  echo "mem-enum-array-push: JIT peak RSS ${jit_mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
  [ "$jit_mb" -gt "$CEIL_MB" ] && { echo "mem-enum-array-push: JIT FAIL -- leak reintroduced"; fail=1; }
fi

if [ "$fail" -eq 0 ]; then
  echo "mem-enum-array-push: PASS"
fi
exit $fail
