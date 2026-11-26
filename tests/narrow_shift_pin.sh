#!/usr/bin/env bash
# Pin (finish item M6): a shift's result has the type of the value being shifted,
# so `u8 << i64` stays u8 (1 << 9 wraps to 0) instead of promoting to i64 (512).
# Fails on a binary whose infer_expr_type ranks the shift count's width in.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
f="tests/narrow_shift_pin.kry"
want="0
0
0
14464
3705032704
-2147483648
100
8
big
small"
fail=0
jit="$(timeout 60 "$K" run "$f" 2>&1 | tr -d '\r')"
if [ "$jit" = "$want" ]; then echo "  ok   narrow shift JIT"; else
    echo "  FAIL: narrow shift JIT"; printf '%s\n' "$jit" | sed 's/^/    got: /'; fail=1; fi
if timeout 300 "$K" build --release "$f" -o "$TMP/p.exe" >"$TMP/b.log" 2>&1; then
    aot="$(timeout 60 "$TMP/p.exe" 2>&1 | tr -d '\r')"
    if [ "$aot" = "$want" ]; then echo "  ok   narrow shift AOT"; else
        echo "  FAIL: narrow shift AOT"; printf '%s\n' "$aot" | sed 's/^/    got: /'; fail=1; fi
else echo "  FAIL: AOT build"; tail -5 "$TMP/b.log"; fail=1; fi
[ "$fail" -eq 0 ] && echo "narrow-shift pin: ok"
exit "$fail"
