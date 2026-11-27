#!/usr/bin/env bash
# Pin (finish item 136): `kryos build --help` must not say a cache hit skips the
# pipeline. Analysis (incl. the capability check) always runs; the cache only
# replaces code generation after it accepts the program.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
out="$(timeout 60 "$K" build --help 2>&1 | tr -d '\r')"
fail=0
if printf '%s' "$out" | tr '\n' ' ' | tr -s ' ' | grep -q "without rerunning the pipeline"; then
    echo "  FAIL: help claims the pipeline is not rerun"; fail=1; else echo "  ok   no skip-the-pipeline claim"; fi
if printf '%s' "$out" | tr '\n' ' ' | tr -s ' ' | grep -q "capability check"; then
    echo "  ok   help states the capability check still runs"; else echo "  FAIL: help does not mention the capability check"; fail=1; fi
[ "$fail" -eq 0 ] && echo "build-help cache pin: ok"
exit "$fail"
