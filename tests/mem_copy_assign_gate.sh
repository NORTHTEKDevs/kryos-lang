#!/usr/bin/env bash
# mem_copy_assign_gate.sh -- regression gate for the copy-assign-leak item: an
# @copy struct (a literal, and each `let c = b` copy) was never freed.
#
# Fixture: tests/mem/copy_assign_leak.kry. Three legs:
#   box  JIT, KRYOS_ALLOC_STATS=1, 1000 iterations: every struct box the
#        program allocated must be freed (box live=0). Master: 5000 live.
#   AOT  peak RSS of the release binary at 2M iterations, ceiling 40MB (the
#        cloned `str` field of each @copy value leaked ~80MB per 1M on master).
#   JIT  peak RSS of the child process at 2M iterations, same ceiling.
#
# The DANGEROUS direction is a double free, not a leak: that half is pinned by
# tests/no_double_free.sh, the conformance suite and the self-host bootstrap.
#
# Windows-only for the RSS legs (PowerShell PeakWorkingSet64 polling).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
PROBE="$ROOT/tests/mem/copy_assign_leak.kry"
ITERS=2000000
CEIL_MB=40
fail=0

box_out=$(KRYOS_ALLOC_STATS=1 "$KRYOS" run "$PROBE" 2>&1)
box_line=$(printf '%s
' "$box_out" | grep '\[alloc-stats\] box' | tail -1)
if [ -z "$box_line" ]; then
  echo "mem-copy-assign: box leg FAIL -- no alloc-stats report (binary predates KRYOS_ALLOC_STATS)"; fail=1
elif printf '%s' "$box_line" | grep -q 'live=0$'; then
  echo "mem-copy-assign: box leg ok -- $box_line"
else
  echo "mem-copy-assign: box leg FAIL -- $box_line"; fail=1
fi
printf '%s
' "$box_out" | grep -q '^tot 1525280$' || { echo "mem-copy-assign: box leg FAIL -- wrong result"; fail=1; }

if ! command -v powershell >/dev/null 2>&1; then
  echo "mem-copy-assign: SKIP (no powershell -- this gate is Windows-only, matching mem_plateau_check.sh's fallback path)"
  exit 0
fi

win_kryos="$(cygpath -w "$KRYOS" 2>/dev/null || echo "$KRYOS")"
win_probe="$(cygpath -w "$PROBE" 2>/dev/null || echo "$PROBE")"
win_stdlib="$(cygpath -w "$KRYOS_STDLIB_DIR" 2>/dev/null || echo "$KRYOS_STDLIB_DIR")"
aot_bin="$(mktemp -u).exe"
win_aot="$(cygpath -w "$aot_bin" 2>/dev/null || echo "$aot_bin")"

"$KRYOS" build --release "$PROBE" -o "$aot_bin" >/dev/null 2>&1 || { echo "mem-copy-assign: AOT build failed"; exit 1; }


# --- AOT leg: measure the built binary directly. ---
aot_bytes=$(powershell -NoProfile -Command \
  "\$env:LEAK_ITERS='$ITERS'; \$p=Start-Process -FilePath '$win_aot' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\mcag_aot.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 20}; try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; \$m" 2>/dev/null | tr -d '\r')
rm -f "$aot_bin"
case "$aot_bytes" in ''|*[!0-9]*) aot_bytes="" ;; esac
if [ -z "$aot_bytes" ]; then
  echo "mem-copy-assign: AOT leg -- could not read peak RSS (powershell probe returned nothing)"
  fail=1
else
  aot_mb=$(( aot_bytes / 1024 / 1024 ))
  echo "mem-copy-assign: AOT peak RSS ${aot_mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
  [ "$aot_mb" -gt "$CEIL_MB" ] && { echo "mem-copy-assign: AOT FAIL -- leak reintroduced"; fail=1; }
fi

# --- JIT leg: `kryos run` execs the Cranelift-compiled binary as a CHILD
# process and deletes it on exit, so poll the child by its predictable temp
# name ("<stem>.exe" in $env:TEMP, per kryos-cli/src/commands/run.rs) rather
# than the outer `kryos.exe run` driver, which never shows the leak.
jit_bytes=$(powershell -NoProfile -Command \
  "\$env:LEAK_ITERS='$ITERS'; \$env:KRYOS_STDLIB_DIR='$win_stdlib'; \$parent=Start-Process -FilePath '$win_kryos' -ArgumentList @('run','$win_probe') -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\mcag_jit.txt; \$m=0; while(-not \$parent.HasExited){try{\$c=Get-Process -Name 'copy_assign_leak' -ErrorAction SilentlyContinue; if(\$c){foreach(\$x in @(\$c)){try{\$x.Refresh(); if(\$x.PeakWorkingSet64 -gt \$m){\$m=\$x.PeakWorkingSet64}}catch{}}}}catch{}; Start-Sleep -Milliseconds 15}; \$m" 2>/dev/null | tr -d '\r')
case "$jit_bytes" in ''|*[!0-9]*) jit_bytes="" ;; esac
if [ -z "$jit_bytes" ]; then
  echo "mem-copy-assign: JIT leg -- could not read peak RSS (powershell probe returned nothing, or the child ran too briefly to sample -- rerun if this flakes)"
  fail=1
else
  jit_mb=$(( jit_bytes / 1024 / 1024 ))
  echo "mem-copy-assign: JIT peak RSS ${jit_mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
  [ "$jit_mb" -gt "$CEIL_MB" ] && { echo "mem-copy-assign: JIT FAIL -- leak reintroduced"; fail=1; }
fi

if [ "$fail" -eq 0 ]; then
  echo "mem-copy-assign: PASS"
fi
exit $fail
