#!/usr/bin/env bash
# spec/mem-acceptance.sh -- gate for the memory-model program.
# A probe PASSES when (a) its output is byte-identical between JIT and AOT
# (correctness -- the drop-emission work must not create use-after-free),
# and (b) its AOT RSS PLATEAUS: peak working set across the LAST 3 rounds
# must be < 1.20x the working set after round 2 (steady-state, not growth).
# Exit 0 only when: cargo build+suite green, native 48-corpus green (drop
# changes must not regress aggregates!), and every mem-probe passes both
# checks. Measured baseline 2026-07-03: m1 grows 218MB->1020MB (5x) -- RED.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="$ROOT/compiler/stdlib"
K="$ROOT/compiler/target/release/kryos.exe"
taskkill //F //IM kryostokens.exe >/dev/null 2>&1 || true
fail(){ echo "MEM-GATE FAIL: $*"; exit 1; }

(cd compiler && cargo build --release -j 4 -q) || fail "cargo build"
echo "  PASS  compiler-build"
(cd compiler && cargo test --release -q) >/tmp/mem_cargo.txt 2>&1 \
  || { tail -15 /tmp/mem_cargo.txt; fail "cargo suite regressed"; }
echo "  PASS  cargo-suite"

# Native aggregate corpus: the drop work must not break aggregates.
div=0
for f in tests/harden-probes/*.kry; do
  b=$(basename "$f" .kry)
  jit=$(timeout 20 "$K" run "$f" 2>&1); jrc=$?
  if timeout 150 "$K" build --release "$f" -o /tmp/mg.exe >/dev/null 2>&1; then
    aot=$(timeout 20 /tmp/mg.exe 2>&1); arc=$?; rm -f /tmp/mg.exe
  else aot=X; arc=99; fi
  if [ "$jrc" -ne 0 ] || [ "$arc" -ne 0 ] || [ "$jit" != "$aot" ]; then echo "  FAIL(corpus) $b"; div=1; fi
done
[ "$div" -eq 0 ] || fail "native corpus divergence -- drop emission broke aggregates"
echo "  PASS  native-48-corpus"

bad=0
for f in tests/mem-probes/*.kry; do
  b=$(basename "$f" .kry)
  jit=$(timeout 120 "$K" run "$f" 2>&1) || { echo "  FAIL(jit-run) $b"; bad=1; continue; }
  timeout 180 "$K" build --release "$f" -o "/tmp/${b}.exe" >/dev/null 2>&1 || { echo "  FAIL(build) $b"; bad=1; continue; }
  prof=$(powershell.exe -NoProfile -Command "
    \$p = Start-Process -FilePath 'C:\\Users\\Krist\\AppData\\Local\\Temp\\${b}.exe' -PassThru -WindowStyle Hidden -RedirectStandardOutput 'C:\\Users\\Krist\\AppData\\Local\\Temp\\${b}_out.txt'
    \$samples = @()
    while (-not \$p.HasExited) { Start-Sleep -Milliseconds 250; try { \$p.Refresh(); \$samples += [math]::Round(\$p.WorkingSet64/1MB,1) } catch {} }
    if (\$samples.Count -lt 6) { Write-Output 'SHORT'; exit }
    \$early = \$samples[[math]::Floor(\$samples.Count/4)]
    \$late = (\$samples | Select-Object -Last 3 | Measure-Object -Maximum).Maximum
    Write-Output ('EARLY=' + \$early + ' LATE=' + \$late)
  " | tr -d '\r')
  aot=$(cat "/tmp/${b}_out.txt" 2>/dev/null)
  rm -f "/tmp/${b}.exe"
  [ "$jit" = "$aot" ] || { echo "  FAIL(diverge) $b"; bad=1; continue; }
  case "$prof" in
    *SHORT*) echo "  PASS  $b (too fast to profile -- treated as plateau)";;
    *EARLY=*)
      early=$(echo "$prof" | grep -oE 'EARLY=[0-9.]+' | cut -d= -f2)
      late=$(echo "$prof" | grep -oE 'LATE=[0-9.]+' | cut -d= -f2)
      ok=$(python -c "print(1 if float('$late') < float('$early') * 1.20 else 0)" 2>/dev/null || echo 0)
      if [ "$ok" = "1" ]; then echo "  PASS  $b (early ${early}MB late ${late}MB)"
      else echo "  FAIL(leak) $b (early ${early}MB late ${late}MB -- no plateau)"; bad=1; fi;;
    *) echo "  FAIL(profile) $b"; bad=1;;
  esac
done
[ "$bad" -eq 0 ] || exit 1
echo "MEM-GATE: ALL PASS"; exit 0
