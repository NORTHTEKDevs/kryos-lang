#!/usr/bin/env bash
# Pin (finish item A20, verifier residuals): `let mut e = arr[0]`, `let c = s.p`, `let mut v = m[k]`,
# `u = t` / `u = arr[i]` / `u = s.p` on a tuple shared the heap handle on the JIT, so a write through
# one name showed through the other. A tuple is a value type.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
f="tests/tuple_elem_value_copy_pin.kry"
want="arr 1 e=77
field s.p=1 c=55
reassign t=2 u=9
late arr2=50 e2=1
late field s2.p=60 c2=1
late local a=5 b=1
asg-idx arr=2 u=9 b
asg-field s=1 u=8 x
nested a=99 n=2 n
map m=5 mv=6
total 134556
for 298 1000
for 299 1000
w 3 m3 v 4 m3"
fail=0
jit="$(timeout 60 "$K" run "$f" 2>&1 | tr -d '\r')"
if [ "$jit" = "$want" ]; then echo "  ok   tuple elem value copy JIT"; else
    echo "  FAIL: tuple elem value copy JIT"; printf '%s\n' "$jit" | sed 's/^/    got: /'; fail=1; fi
if timeout 300 "$K" build --release "$f" -o "$TMP/p.exe" >"$TMP/b.log" 2>&1; then
    aot="$(timeout 60 "$TMP/p.exe" 2>&1 | tr -d '\r')"
    if [ "$aot" = "$want" ]; then echo "  ok   tuple elem value copy AOT"; else
        echo "  FAIL: tuple elem value copy AOT"; printf '%s\n' "$aot" | sed 's/^/    got: /'; fail=1; fi
else echo "  FAIL: AOT build"; tail -5 "$TMP/b.log"; fail=1; fi
[ "$fail" -eq 0 ] && echo "tuple elem value copy pin: ok"
exit "$fail"
