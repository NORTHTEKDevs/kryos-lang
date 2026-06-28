#!/usr/bin/env bash
# spec/harden-acceptance.sh
# Hardening gate for the night-shift "harden" milestone.
# Exit 0 ONLY when:
#   (1) the compiler builds,
#   (2) the full Rust suite is GREEN (catches any regression from compiler edits),
#   (3) tests/harden-probes/ has >= TARGET adversarial probes, and EVERY one runs
#       on BOTH backends (kryos run + kryos build --release) with identical stdout
#       and zero crash. JIT is the oracle; cross-backend agreement is the check.
# A probe that diverges/crashes = a real bug; FIX the compiler, never delete the probe.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="$ROOT/compiler/stdlib"
KRYOS="$ROOT/compiler/target/release/kryos.exe"
taskkill //F //IM kryostokens.exe >/dev/null 2>&1 || true
fail(){ echo "HARDEN-FAIL: $*"; exit 1; }

(cd compiler && cargo build --release -j 4 -q) || fail "cargo build"
echo "  PASS  compiler-build"

(cd compiler && cargo test --release -q) >/tmp/harden_cargo.txt 2>&1 \
  || { tail -30 /tmp/harden_cargo.txt; fail "cargo-suite regressed"; }
echo "  PASS  cargo-suite"

PROBE_DIR="$ROOT/tests/harden-probes"; mkdir -p "$PROBE_DIR"
count=$(ls "$PROBE_DIR"/*.kry 2>/dev/null | wc -l)
TARGET=24
[ "$count" -ge "$TARGET" ] || fail "need >= $TARGET adversarial probes in tests/harden-probes (have $count)"

diverged=0
for f in "$PROBE_DIR"/*.kry; do
  jit=$("$KRYOS" run "$f" 2>&1); jrc=$?
  bin="/tmp/hp_$(basename "$f" .kry).exe"
  if "$KRYOS" build --release "$f" -o "$bin" >/dev/null 2>&1; then
    aot=$("$bin" 2>&1); arc=$?; rm -f "$bin"
  else
    aot="<BUILD-FAILED>"; arc=99
  fi
  if [ "$jrc" -ne 0 ] || [ "$arc" -ne 0 ] || [ "$jit" != "$aot" ]; then
    echo "  DIVERGE $(basename "$f") jit_rc=$jrc aot_rc=$arc"; diverged=1
  fi
done
[ "$diverged" -eq 0 ] || fail "JIT/AOT divergence or crash in probe corpus"
echo "  PASS  harden-probes ($count probes agree on both backends)"
echo "HARDEN: ALL CHECKS PASS"; exit 0
