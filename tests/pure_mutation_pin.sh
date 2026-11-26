#!/usr/bin/env bash
# Pin (finish item M2): `@pure` must not license deleting/merging calls to a
# function that mutates a parameter or module state. JIT and release AOT must agree.
# Fails on a binary whose optimize/pure.rs trusts the annotation unverified.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
f="tests/pure_mutation_pin.kry"
want="len=2 counter=1
counter=21
ys=3
zs=2 counter=22
log=2
sum=12"
fail=0
jit="$(timeout 60 "$K" run "$f" 2>&1 | tr -d '\r')"
if [ "$jit" = "$want" ]; then echo "  ok   pure mutation JIT"; else
    echo "  FAIL: pure mutation JIT"; printf '%s\n' "$jit" | sed 's/^/    got: /'; fail=1; fi
if timeout 300 "$K" build --release "$f" -o "$TMP/p.exe" >"$TMP/b.log" 2>&1; then
    aot="$(timeout 60 "$TMP/p.exe" 2>&1 | tr -d '\r')"
    if [ "$aot" = "$want" ]; then echo "  ok   pure mutation AOT"; else
        echo "  FAIL: pure mutation AOT"; printf '%s\n' "$aot" | sed 's/^/    got: /'; fail=1; fi
else echo "  FAIL: AOT build"; tail -5 "$TMP/b.log"; fail=1; fi
[ "$fail" -eq 0 ] && echo "pure-mutation pin: ok"
exit "$fail"
