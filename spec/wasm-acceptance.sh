#!/usr/bin/env bash
# spec/wasm-acceptance.sh -- gate for the wasm-expansion night-shift.
# Exit 0 ONLY when: compiler builds, cargo suite green, native regression
# suite unaffected, and EVERY tests/wasm-probes/*.kry compiles with
# --backend wasm AND its node-host output matches its .expect exactly.
# p6 (structs) is the stretch probe -- it counts like the others; the gate
# passes only with all probes green.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="$ROOT/compiler/stdlib"
K="$ROOT/compiler/target/release/kryos.exe"
taskkill //F //IM kryostokens.exe >/dev/null 2>&1 || true
fail(){ echo "WASM-GATE FAIL: $*"; exit 1; }

(cd compiler && cargo build --release -j 4 -q) || fail "cargo build"
echo "  PASS  compiler-build"
(cd compiler && cargo test --release -q) >/tmp/wasm_gate_cargo.txt 2>&1 \
  || { tail -20 /tmp/wasm_gate_cargo.txt; fail "cargo suite regressed"; }
echo "  PASS  cargo-suite"

bad=0
for f in tests/wasm-probes/*.kry; do
  b=$(basename "$f" .kry); exp="tests/wasm-probes/$b.expect"
  [ -f "$exp" ] || fail "missing $exp"
  if ! timeout 90 "$K" build --release --backend wasm "$f" -o "/tmp/wg_$b.wasm" >/tmp/wg_build.txt 2>&1; then
    echo "  FAIL  $b (wasm build): $(grep -m1 error /tmp/wg_build.txt | head -c 140)"; bad=1; continue
  fi
  out=$(timeout 20 node tools/wasm-host/run.mjs "/tmp/wg_$b.wasm" 2>&1); rc=$?
  rm -f "/tmp/wg_$b.wasm"
  if [ "$rc" -ne 0 ]; then echo "  FAIL  $b (run rc=$rc)"; bad=1; continue; fi
  if [ "$out" != "$(cat "$exp")" ]; then
    echo "  FAIL  $b (output mismatch)"; echo "    got:  $(echo "$out" | tr '\n' '|')"; echo "    want: $(tr '\n' '|' < "$exp")"; bad=1; continue
  fi
  echo "  PASS  $b"
done
[ "$bad" -eq 0 ] || exit 1
echo "WASM-GATE: ALL PASS"; exit 0
