#!/usr/bin/env bash
# Deterministic acceptance check for the Kryos night-shift run.
# Exit 0 == DONE. Anything non-zero == keep working.
# Every gate is machine-checkable and reproducible (no live LLM, no creds, no spend).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export KRYOS_STDLIB_DIR="$ROOT/compiler/stdlib"

# 0. Machine hygiene: kryostokens.exe leaks and wedges builds; clear stray
#    builders left by the previous iteration so the compile below is fast.
taskkill //F //IM kryostokens.exe //IM clang.exe //IM link.exe >/dev/null 2>&1 || true

fail=0
gate() { if [ "$2" -eq 0 ]; then echo "  PASS  $1"; else echo "  FAIL  $1"; fail=1; fi; }

# 1. Compiler builds (release).
( cd compiler && cargo build --release -j 4 ) >/tmp/ns_build.log 2>&1
gate "compiler-build" $?

# 2. Full test suite stays green (the regression floor — never sacrifice this).
( cd compiler && cargo test --release -j 4 ) >/tmp/ns_test.log 2>&1
gate "cargo-suite" $?

# 3. Milestone demos. Each milestone owns demo/<name>/check.sh which must exit 0.
#    A missing check.sh counts as not-done. Contracts are in spec/features.json.
for m in mutation native wasm cabi; do
  if [ -x "demo/$m/check.sh" ]; then
    bash "demo/$m/check.sh" >"/tmp/ns_$m.log" 2>&1
    gate "demo-$m" $?
  else
    echo "  TODO  demo-$m (demo/$m/check.sh missing)"
    fail=1
  fi
done

if [ "$fail" -eq 0 ]; then
  echo "ACCEPTANCE: ALL GATES PASS"
  exit 0
fi
echo "ACCEPTANCE: incomplete"
exit 1
