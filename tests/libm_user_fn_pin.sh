#!/usr/bin/env bash
# Pin (finish item libm-constfold): a user function named like a C library
# function (sin/cos/sqrt/floor/exp/pow) with the libm signature must run the
# USER body on AOT. LLVM recognised the define as the libfunc and constant
# folded sin(1.0) to 0.8414... while the JIT printed the user's 42.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
f="tests/libm_user_fn_pin.kry"
want="42
43
44
45
46
47"
fail=0
jit="$(timeout 60 "$K" run "$f" 2>&1 | tr -d '\r')"
if [ "$jit" = "$want" ]; then echo "  ok   libm-named user fn JIT"; else
    echo "  FAIL: libm-named user fn JIT"; printf '%s\n' "$jit" | sed 's/^/    got: /'; fail=1; fi
if timeout 300 "$K" build --release "$f" -o "$TMP/p.exe" >"$TMP/b.log" 2>&1; then
    aot="$(timeout 60 "$TMP/p.exe" 2>&1 | tr -d '\r')"
    if [ "$aot" = "$want" ]; then echo "  ok   libm-named user fn AOT"; else
        echo "  FAIL: libm-named user fn AOT"; printf '%s\n' "$aot" | sed 's/^/    got: /'; fail=1; fi
else echo "  FAIL: AOT build"; tail -5 "$TMP/b.log"; fail=1; fi
[ "$fail" -eq 0 ] && echo "libm user fn pin: ok"
exit "$fail"
