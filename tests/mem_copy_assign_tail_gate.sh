#!/usr/bin/env bash
# mem_copy_assign_tail_gate.sh -- copy-assign-leak follow-up: an @copy struct
# local read in tail / early-return position (tail if/else, else-if chain,
# match, `return` inside an if) was never freed because its scope-exit drop sat
# in a block no path reaches, and an overwritten `let mut` @copy local leaked
# the value it replaced.
#
# Fixture: tests/mem/copy_assign_tail_leak.kry. Three legs:
#   box  JIT, KRYOS_ALLOC_STATS=1, 1000 iterations: every struct box the
#        program allocated must be freed (box live=0). Master: ~17000 live.
#   AOT  peak RSS of the release binary at 500k iterations, ceiling 40MB (the
#        cloned `str` field of each @copy value leaked ~80MB per 1M on master).
#   JIT  peak RSS of the child process at 500k iterations, same ceiling.
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
PROBE="$ROOT/tests/mem/copy_assign_tail_leak.kry"
ITERS=500000
CEIL_MB=40
fail=0

box_out=$(KRYOS_ALLOC_STATS=1 "$KRYOS" run "$PROBE" 2>&1)
box_line=$(printf '%s
' "$box_out" | grep '\[alloc-stats\] box' | tail -1)
if [ -z "$box_line" ]; then
  echo "mem-copy-assign-tail: box leg FAIL -- no alloc-stats report (binary predates KRYOS_ALLOC_STATS)"; fail=1
elif printf '%s' "$box_line" | grep -q 'live=0$'; then
  echo "mem-copy-assign-tail: box leg ok -- $box_line"
else
  echo "mem-copy-assign-tail: box leg FAIL -- $box_line"; fail=1
fi
printf '%s
' "$box_out" | grep -q '^tot 2436702$' || { echo "mem-copy-assign-tail: box leg FAIL -- wrong result"; fail=1; }

if ! command -v powershell >/dev/null 2>&1; then
  echo "mem-copy-assign-tail: SKIP (no powershell -- this gate is Windows-only, matching mem_plateau_check.sh's fallback path)"
  exit 0
fi

win_kryos="$(cygpath -w "$KRYOS" 2>/dev/null || echo "$KRYOS")"
win_probe="$(cygpath -w "$PROBE" 2>/dev/null || echo "$PROBE")"
win_stdlib="$(cygpath -w "$KRYOS_STDLIB_DIR" 2>/dev/null || echo "$KRYOS_STDLIB_DIR")"
aot_bin="$(mktemp -u).exe"
win_aot="$(cygpath -w "$aot_bin" 2>/dev/null || echo "$aot_bin")"

"$KRYOS" build --release "$PROBE" -o "$aot_bin" >/dev/null 2>&1 || { echo "mem-copy-assign-tail: AOT build failed"; exit 1; }


# --- AOT leg: measure the built binary directly. ---
aot_bytes=$(powershell -NoProfile -Command \
  "\$env:LEAK_ITERS='$ITERS'; \$p=Start-Process -FilePath '$win_aot' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\mcagt_aot.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 20}; try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; \$m" 2>/dev/null | tr -d '\r')
rm -f "$aot_bin"
case "$aot_bytes" in ''|*[!0-9]*) aot_bytes="" ;; esac
if [ -z "$aot_bytes" ]; then
  echo "mem-copy-assign-tail: AOT leg -- could not read peak RSS (powershell probe returned nothing)"
  fail=1
else
  aot_mb=$(( aot_bytes / 1024 / 1024 ))
  echo "mem-copy-assign-tail: AOT peak RSS ${aot_mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
  [ "$aot_mb" -gt "$CEIL_MB" ] && { echo "mem-copy-assign-tail: AOT FAIL -- leak reintroduced"; fail=1; }
fi

# --- JIT leg: `kryos run` execs the Cranelift-compiled binary as a CHILD
# process and deletes it on exit, so poll the child by its predictable temp
# name ("<stem>.exe" in $env:TEMP, per kryos-cli/src/commands/run.rs) rather
# than the outer `kryos.exe run` driver, which never shows the leak.
jit_bytes=$(powershell -NoProfile -Command \
  "\$env:LEAK_ITERS='$ITERS'; \$env:KRYOS_STDLIB_DIR='$win_stdlib'; \$parent=Start-Process -FilePath '$win_kryos' -ArgumentList @('run','$win_probe') -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\mcagt_jit.txt; \$m=0; while(-not \$parent.HasExited){try{\$c=Get-Process -Name 'copy_assign_tail_leak' -ErrorAction SilentlyContinue; if(\$c){foreach(\$x in @(\$c)){try{\$x.Refresh(); if(\$x.PeakWorkingSet64 -gt \$m){\$m=\$x.PeakWorkingSet64}}catch{}}}}catch{}; Start-Sleep -Milliseconds 15}; \$m" 2>/dev/null | tr -d '\r')
case "$jit_bytes" in ''|*[!0-9]*) jit_bytes="" ;; esac
if [ -z "$jit_bytes" ]; then
  echo "mem-copy-assign-tail: JIT leg -- could not read peak RSS (powershell probe returned nothing, or the child ran too briefly to sample -- rerun if this flakes)"
  fail=1
else
  jit_mb=$(( jit_bytes / 1024 / 1024 ))
  echo "mem-copy-assign-tail: JIT peak RSS ${jit_mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
  [ "$jit_mb" -gt "$CEIL_MB" ] && { echo "mem-copy-assign-tail: JIT FAIL -- leak reintroduced"; fail=1; }
fi

if [ "$fail" -eq 0 ]; then
  echo "mem-copy-assign-tail: PASS"
fi
exit $fail
