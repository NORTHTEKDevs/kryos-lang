#!/usr/bin/env bash
# mem_field_of_temp_gate.sh -- regression gate for LEDGER item A17: a field read off an if / match VALUE
# (`len(match k { 0 => mk(i), _ => mk(i + 1) }.name)`) left the hidden result slot owning a struct nobody
# released, so every read leaked the struct box plus the field's heap value (72MB per 750k iterations on the
# JIT, 49MB on AOT). lower.rs now releases the slot right after a retained (str / array / map) or scalar
# field read. Checks the answer and peak RSS (1M iters) on both backends; `named` (arms are named locals the
# slot shares) and `control` (the read off a named local) are the baselines. Round 2 modes: the read in an
# if / elif / while condition and a `return`, an arm / let block that returns its own local, and nested
# `.q.name` / `.t.0` reads (see the probe header).
# Windows-only (PowerShell PeakWorkingSet64).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"
ITERS=1000000
CEIL_MB=24   # steady state ~5-8MB; pre-fix 30-110MB

want_for() {
  case "$1" in
    match_str|if_str|let_str|struct_lit|ret_str|ret_if|blk_arm|nested_q|tuple_t) echo 6888896 ;;
    cond_str|elif_str|while_str) echo 7000000 ;;
    blk_let) echo 6888890 ;;
    match_int) echo 500000000000 ;;
    match_arr) echo 2000000 ;;
    lit_arr) echo 7888896 ;;
    twice) echo 13777804 ;;
    named) echo 15000000 ;;
    *) echo 6888890 ;;
  esac
}
if ! command -v powershell >/dev/null 2>&1; then
  echo "mem-field-of-temp: SKIP (no powershell -- this gate is Windows-only)"
  exit 0
fi

win_kryos="$(cygpath -w "$KRYOS" 2>/dev/null || echo "$KRYOS")"
win_stdlib="$(cygpath -w "$KRYOS_STDLIB_DIR" 2>/dev/null || echo "$KRYOS_STDLIB_DIR")"
probe="tests/mem/field_of_temp_leak.kry"
win_probe="$(cygpath -w "$ROOT/$probe" 2>/dev/null || echo "$ROOT/$probe")"
fail=0

aot_bin="$(mktemp -u).exe"
win_aot="$(cygpath -w "$aot_bin" 2>/dev/null || echo "$aot_bin")"
if ! "$KRYOS" build --release "$ROOT/$probe" -o "$aot_bin" >/dev/null 2>&1; then
  echo "mem-field-of-temp: AOT build FAILED"; exit 1
fi

for mode in match_str if_str match_int match_arr let_str lit_arr struct_lit ret_str twice cond_str elif_str while_str ret_if blk_arm blk_let nested_q tuple_t named control; do
  WANT="$(want_for "$mode")"
  aot_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_ITERS='$ITERS'; \$env:LEAK_MODE='$mode'; \$p=Start-Process -FilePath '$win_aot' -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\mfot_${mode}_aot.txt; \$m=0; while(-not \$p.HasExited){try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; Start-Sleep -Milliseconds 20}; try{\$p.Refresh(); if(\$p.PeakWorkingSet64 -gt \$m){\$m=\$p.PeakWorkingSet64}}catch{}; \$m" 2>/dev/null | tr -d '\r')
  case "$aot_bytes" in ''|*[!0-9]*) aot_bytes="" ;; esac
  out="$(tr -d '\r' < "${TEMP:-/tmp}/mfot_${mode}_aot.txt" 2>/dev/null | tr -d '[:space:]')"
  if [ -z "$aot_bytes" ]; then
    echo "mem-field-of-temp: $mode AOT -- could not read peak RSS"; fail=1
  else
    mb=$(( aot_bytes / 1024 / 1024 ))
    echo "mem-field-of-temp: $mode AOT peak RSS ${mb}MB (ceiling ${CEIL_MB}MB) answer=$out"
    [ "$mb" -gt "$CEIL_MB" ] && { echo "mem-field-of-temp: $mode AOT FAIL -- leak present"; fail=1; }
    [ "$out" = "$WANT" ] || { echo "mem-field-of-temp: $mode AOT FAIL -- wrong answer (want $WANT)"; fail=1; }
  fi

  # JIT: poll the exec'd child by its temp name.
  jit_bytes=$(powershell -NoProfile -Command \
    "\$env:LEAK_ITERS='$ITERS'; \$env:LEAK_MODE='$mode'; \$env:KRYOS_STDLIB_DIR='$win_stdlib'; \$parent=Start-Process -FilePath '$win_kryos' -ArgumentList @('run','$win_probe') -PassThru -NoNewWindow -RedirectStandardOutput \$env:TEMP\mfot_${mode}_jit.txt; \$m=0; while(-not \$parent.HasExited){try{\$c=Get-Process -Name 'field_of_temp_leak' -ErrorAction SilentlyContinue; if(\$c){foreach(\$x in @(\$c)){try{\$x.Refresh(); if(\$x.PeakWorkingSet64 -gt \$m){\$m=\$x.PeakWorkingSet64}}catch{}}}}catch{}; Start-Sleep -Milliseconds 15}; \$m" 2>/dev/null | tr -d '\r')
  case "$jit_bytes" in ''|*[!0-9]*) jit_bytes="" ;; esac
  out="$(tr -d '\r' < "${TEMP:-/tmp}/mfot_${mode}_jit.txt" 2>/dev/null | tr -d '[:space:]')"
  if [ -z "$jit_bytes" ]; then
    echo "mem-field-of-temp: $mode JIT -- could not read peak RSS (rerun if this flakes)"; fail=1
  else
    mb=$(( jit_bytes / 1024 / 1024 ))
    echo "mem-field-of-temp: $mode JIT peak RSS ${mb}MB (ceiling ${CEIL_MB}MB) answer=$out"
    [ "$mb" -gt "$CEIL_MB" ] && { echo "mem-field-of-temp: $mode JIT FAIL -- leak present"; fail=1; }
    [ "$out" = "$WANT" ] || { echo "mem-field-of-temp: $mode JIT FAIL -- wrong answer (want $WANT)"; fail=1; }
  fi
done
rm -f "$aot_bin"

[ "$fail" -eq 0 ] && echo "mem-field-of-temp: PASS"
exit $fail
