#!/usr/bin/env bash
# mem_pop_stmt_gate.sh -- regression gate for LEDGER item A11: `pop(xs)` as a
# STATEMENT discarded the popped element without owning it. The discarded call
# result got a Void temp (lower.rs keeps Void for builtins because a discarded
# `push` returns the receiver itself), so the element that left the array had no
# owner: every popped struct / tuple / str / array leaked its box and fields.
# Fixed in two places: lower.rs types the discarded `pop` temp with the element
# type so the statement-end temp drop owns it, and the LLVM `pop` lowering now
# takes the array's share of a struct element box through the type's drop helper
# (it only did that for tuples, so AOT leaked the box of every popped struct,
# bound or discarded). tests/mem/pop_shared_box.kry pins the answers: popping
# from a dup'd [Struct] must not free fields the other array still holds.
# Pre-fix, 1M iters: ~+90MB on JIT and AOT. Post-fix: flat.
# Windows-only (PowerShell PeakWorkingSet64 polling, as mem_struct_array_overwrite_gate.sh).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
ITERS=1000000
CEIL_MB=12   # steady state ~4MB; pre-fix AOT struct_bound ~34MB, others 60-340MB

if ! command -v powershell >/dev/null 2>&1; then
  echo "mem-pop-stmt: SKIP (no powershell -- this gate is Windows-only, matching mem_plateau_check.sh's fallback path)"
  exit 0
fi

win_kryos="$(cygpath -w "$KRYOS" 2>/dev/null || echo "$KRYOS")"
win_stdlib="$(cygpath -w "$KRYOS_STDLIB_DIR" 2>/dev/null || echo "$KRYOS_STDLIB_DIR")"

fail=0

run_leg() {
  # $1 = label, $2 = probe .kry path, $3 = child process name (no .exe)
  local label="$1" probe="tests/mem/pop_stmt_leak.kry" procname="pop_stmt_leak" mode="$2"
  local win_probe aot_bin win_aot aot_bytes aot_mb jit_bytes jit_mb

  win_probe="$(cygpath -w "$ROOT/$probe" 2>/dev/null || echo "$ROOT/$probe")"
  aot_bin="$(mktemp -u).exe"
  win_aot="$(cygpath -w "$aot_bin" 2>/dev/null || echo "$aot_bin")"

  if ! "$KRYOS" build --release "$ROOT/$probe" -o "$aot_bin" >/dev/null 2>&1; then
    echo "mem-pop-stmt: $1 -- AOT build FAILED"
    fail=1
    return
  fi

  aot_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_ITERS='$ITERS'; \$env:LEAK_MODE='$mode'; \$p=Start-Process -FilePath '$win_aot' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\mpsg_${1}_aot.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 20}; try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; \$m" 2>/dev/null | tr -d '\r')
  rm -f "$aot_bin"
  case "$aot_bytes" in ''|*[!0-9]*) aot_bytes="" ;; esac
  if [ -z "$aot_bytes" ]; then
    echo "mem-pop-stmt: $1 AOT leg -- could not read peak RSS (powershell probe returned nothing)"
    fail=1
  else
    aot_mb=$(( aot_bytes / 1024 / 1024 ))
    echo "mem-pop-stmt: $1 AOT peak RSS ${aot_mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
    [ "$aot_mb" -gt "$CEIL_MB" ] && { echo "mem-pop-stmt: $1 AOT FAIL -- leak present"; fail=1; }
  fi

  # JIT leg: `kryos run` execs the Cranelift-compiled binary as a CHILD
  # process and deletes it on exit, so poll the child by its predictable
  # temp name rather than the outer `kryos.exe run` driver.
  jit_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_ITERS='$ITERS'; \$env:LEAK_MODE='$mode'; \$env:KRYOS_STDLIB_DIR='$win_stdlib'; \$parent=Start-Process -FilePath '$win_kryos' -ArgumentList @('run','$win_probe') -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\\mpsg_${1}_jit.txt; \$m=0; while(-not \$parent.HasExited){try{\$c=Get-Process -Name '$procname' -ErrorAction SilentlyContinue; if(\$c){foreach(\$x in @(\$c)){try{\$x.Refresh(); if(\$x.PeakWorkingSet64 -gt \$m){\$m=\$x.PeakWorkingSet64}}catch{}}}}catch{}; Start-Sleep -Milliseconds 15}; \$m" 2>/dev/null | tr -d '\r')
  case "$jit_bytes" in ''|*[!0-9]*) jit_bytes="" ;; esac
  if [ -z "$jit_bytes" ]; then
    echo "mem-pop-stmt: $1 JIT leg -- could not read peak RSS (powershell probe returned nothing, or the child ran too briefly to sample -- rerun if this flakes)"
    fail=1
  else
    jit_mb=$(( jit_bytes / 1024 / 1024 ))
    echo "mem-pop-stmt: $1 JIT peak RSS ${jit_mb}MB (ceiling ${CEIL_MB}MB) at ${ITERS} iters"
    [ "$jit_mb" -gt "$CEIL_MB" ] && { echo "mem-pop-stmt: $1 JIT FAIL -- leak present"; fail=1; }
  fi
}

for m in struct struct_nested tuple str array struct_bound; do
  run_leg "$m" "$m"
done

# Answers: popping from a dup'd [Struct] on both backends.
want=$'x gamma3 3 a 1 b 3
b alpha1 beta2 gamma3
y gamma3 b 2 alpha1 beta2
empty 0 1 alpha1
q 1 t1 r2 z t2 r3 8 q2 1 t1'
aot_bin="$(mktemp -u).exe"
if "$KRYOS" build --release "$ROOT/tests/mem/pop_shared_box.kry" -o "$aot_bin" >/dev/null 2>&1; then
  got="$("$aot_bin" | tr -d '')"; rm -f "$aot_bin"
  [ "$got" = "$want" ] || { echo "mem-pop-stmt: shared_box AOT answers differ"; echo "$got"; fail=1; }
else
  echo "mem-pop-stmt: shared_box AOT build FAILED"; fail=1
fi
got="$("$KRYOS" run "$ROOT/tests/mem/pop_shared_box.kry" | tr -d '')"
[ "$got" = "$want" ] || { echo "mem-pop-stmt: shared_box JIT answers differ"; echo "$got"; fail=1; }

if [ "$fail" -eq 0 ]; then
  echo "mem-pop-stmt: PASS"
fi
exit $fail
