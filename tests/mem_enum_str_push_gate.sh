#!/usr/bin/env bash
# mem_enum_str_push_gate.sh -- regression gate for LEDGER item 52 (the
# enum-STR-payload construction leak).
#
# Fixture: tests/mem/enum_str_push_leak.kry (a fresh Val.StrV(str) enum
# construction pushed into a scratch array, dropped every iteration) -- item
# 45's own push-and-drop shape with a `str` payload substituted for the
# `[i64]` one.
#
# Item 45 gave kryos-mir's drop_unescaped_str_temps an
# RValue::EnumVariant-with-ARRAY-field arm. It never got the matching STR-field
# arm, and the gap is not symmetric with the struct path: RValue::Struct MOVES
# a str handle in with a bare insertvalue (so the source temp must NOT be
# dropped there, and that arm is correctly Array-only), while
# RValue::EnumVariant CLONES it -- `kryos_string_clone` in Cranelift's payload-
# store match and `call ptr @kryos_string_clone` in LLVM's, both read end to
# end. The enum owns the clone, the source temp still owns the ORIGINAL, and
# nothing dropped it: one leaked buffer per construction, whether the enum was
# later pushed, overwritten, or just dropped. Unrelated to container-slot
# overwrite (item 49) -- no overwrite is involved in this shape at all.
#
# Measured on the fixture, Windows, PowerShell PeakWorkingSet64 polling:
# pre-fix AOT 43MB at 500k and 234MB at 3M iterations (~78MB/M, growing with
# the iteration count, which is what makes it a leak rather than a fixed
# allocation); post-fix AOT 4MB at 500k and 3MB at 3M, JIT 4MB at 3M. The fix
# lives in shared MIR lowering, not a backend-specific path, so both legs are
# asserted here.
#
# JIT-leg measurement note (inherited from the item-45 gate, and the reason
# that item was once mischaracterized as AOT-only): `kryos run` compiles via
# Cranelift and executes the result as a CHILD PROCESS
# (kryos-cli/src/commands/run.rs: `Command::new(bin).status()`), so polling
# PeakWorkingSet64 on the outer `kryos.exe run` process -- which just blocks in
# `.status()` -- reads the wrong process and shows near-flat memory regardless
# of what the child actually does. This gate polls the CHILD by name.
#
# Windows-only (PowerShell PeakWorkingSet64 polling, matching
# mem_plateau_check.sh's own fallback technique) -- this is where this
# compiler is developed. Skips with a clear message elsewhere.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
PROBE="$ROOT/tests/mem/enum_str_push_leak.kry"
ITERS=5000000
CEIL_MB=50   # steady state ~3-4MB both backends; pre-fix leak reaches ~390MB at this iter count (78MB/M x 5M)

if ! command -v powershell >/dev/null 2>&1; then
  echo "mem-enum-str-push: SKIP (no powershell -- this gate is Windows-only, matching mem_plateau_check.sh's fallback path)"
  exit 0
fi

win_kryos="$(cygpath -w "$KRYOS" 2>/dev/null || echo "$KRYOS")"
win_probe="$(cygpath -w "$PROBE" 2>/dev/null || echo "$PROBE")"
win_stdlib="$(cygpath -w "$KRYOS_STDLIB_DIR" 2>/dev/null || echo "$KRYOS_STDLIB_DIR")"
aot_bin="$(mktemp -u).exe"
win_aot="$(cygpath -w "$aot_bin" 2>/dev/null || echo "$aot_bin")"

"$KRYOS" build --release "$PROBE" -o "$aot_bin" >/dev/null 2>&1 || { echo "mem-enum-str-push: AOT build failed"; exit 1; }

fail=0

# --- AOT leg: measure the built binary directly. ---
aot_bytes=$(powershell -NoProfile -Command \
  "\$env:LEAK_ITERS='$ITERS'; \$p=Start-Process -FilePath '$win_aot' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\mespg_aot.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 20}; try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; \$m" 2>/dev/null | tr -d '\r')
rm -f "$aot_bin"
case "$aot_bytes" in ''|*[!0-9]*) aot_bytes="" ;; esac
if [ -z "$aot_bytes" ]; then
  echo "mem-enum-str-push: AOT leg -- could not read peak RSS (powershell probe returned nothing)"
  fail=1
else
  aot_mb=$(( aot_bytes / 1024 / 1024 ))
  echo "mem-enum-str-push: AOT peak RSS ${aot_mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
  [ "$aot_mb" -gt "$CEIL_MB" ] && { echo "mem-enum-str-push: AOT FAIL -- leak reintroduced"; fail=1; }
fi

# --- JIT leg: `kryos run` execs the Cranelift-compiled binary as a CHILD
# process and deletes it on exit, so poll the child by its predictable temp
# name ("<stem>.exe" in $env:TEMP, per kryos-cli/src/commands/run.rs) rather
# than the outer `kryos.exe run` driver, which never shows the leak (see the
# header comment's CORRECTION note).
jit_bytes=$(powershell -NoProfile -Command \
  "\$env:LEAK_ITERS='$ITERS'; \$env:KRYOS_STDLIB_DIR='$win_stdlib'; \$parent=Start-Process -FilePath '$win_kryos' -ArgumentList @('run','$win_probe') -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\mespg_jit.txt; \$m=0; while(-not \$parent.HasExited){try{\$c=Get-Process -Name 'enum_str_push_leak' -ErrorAction SilentlyContinue; if(\$c){foreach(\$x in @(\$c)){try{\$x.Refresh(); if(\$x.PeakWorkingSet64 -gt \$m){\$m=\$x.PeakWorkingSet64}}catch{}}}}catch{}; Start-Sleep -Milliseconds 15}; \$m" 2>/dev/null | tr -d '\r')
case "$jit_bytes" in ''|*[!0-9]*) jit_bytes="" ;; esac
if [ -z "$jit_bytes" ]; then
  echo "mem-enum-str-push: JIT leg -- could not read peak RSS (powershell probe returned nothing, or the child ran too briefly to sample -- rerun if this flakes)"
  fail=1
else
  jit_mb=$(( jit_bytes / 1024 / 1024 ))
  echo "mem-enum-str-push: JIT peak RSS ${jit_mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
  [ "$jit_mb" -gt "$CEIL_MB" ] && { echo "mem-enum-str-push: JIT FAIL -- leak reintroduced"; fail=1; }
fi

if [ "$fail" -eq 0 ]; then
  echo "mem-enum-str-push: PASS"
fi
exit $fail
