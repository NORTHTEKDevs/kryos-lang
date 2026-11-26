#!/usr/bin/env bash
# Pin (finish item NaN-neg0-divergence): (1) an LLVM identity copy of an f64
# must not turn -0.0 into +0.0 (1.0 / parse_float("-0.0") is -inf on AOT, was
# inf); (2) sort() of [f64] puts every NaN last whatever its sign bit (the CPU's
# 0.0/0.0 is -NaN on JIT, LLVM folds it to +NaN on AOT; bare total_cmp ordered
# them first/last respectively). Fails on a binary without both fixes.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
f="tests/nan_neg0_pin.kry"
want="-inf
-inf
-inf
[1, 2, 3, NaN, NaN]
[1, 2, NaN, NaN]"
fail=0
jit="$(timeout 60 "$K" run "$f" 2>&1 | tr -d '\r')"
if [ "$jit" = "$want" ]; then echo "  ok   nan/neg0 JIT"; else
    echo "  FAIL: nan/neg0 JIT"; printf '%s\n' "$jit" | sed 's/^/    got: /'; fail=1; fi
if timeout 300 "$K" build --release "$f" -o "$TMP/p.exe" >"$TMP/b.log" 2>&1; then
    aot="$(timeout 60 "$TMP/p.exe" 2>&1 | tr -d '\r')"
    if [ "$aot" = "$want" ]; then echo "  ok   nan/neg0 AOT"; else
        echo "  FAIL: nan/neg0 AOT"; printf '%s\n' "$aot" | sed 's/^/    got: /'; fail=1; fi
else echo "  FAIL: AOT build"; tail -5 "$TMP/b.log"; fail=1; fi
[ "$fail" -eq 0 ] && echo "nan-neg0 pin: ok"
exit "$fail"
