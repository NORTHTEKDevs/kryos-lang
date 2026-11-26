#!/usr/bin/env bash
# Pin (finish item A20): `let mut u = t` on a tuple shared the handle, so
# `u.0 = 7` also changed `t.0` on JIT and AOT. A tuple is a value type.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
f="tests/tuple_value_copy_pin.kry"
want="src-mut t.0=5 u.0=1
w=1a x=7z
nest n=1 m=9
param w=1 b=101
loop 1 1
loop 2 1
loop 3 1
both 3 2"
fail=0
jit="$(timeout 60 "$K" run "$f" 2>&1 | tr -d '\r')"
if [ "$jit" = "$want" ]; then echo "  ok   tuple value copy JIT"; else
    echo "  FAIL: tuple value copy JIT"; printf '%s\n' "$jit" | sed 's/^/    got: /'; fail=1; fi
if timeout 300 "$K" build --release "$f" -o "$TMP/p.exe" >"$TMP/b.log" 2>&1; then
    aot="$(timeout 60 "$TMP/p.exe" 2>&1 | tr -d '\r')"
    if [ "$aot" = "$want" ]; then echo "  ok   tuple value copy AOT"; else
        echo "  FAIL: tuple value copy AOT"; printf '%s\n' "$aot" | sed 's/^/    got: /'; fail=1; fi
else echo "  FAIL: AOT build"; tail -5 "$TMP/b.log"; fail=1; fi
[ "$fail" -eq 0 ] && echo "tuple value copy pin: ok"
exit "$fail"
