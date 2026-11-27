#!/usr/bin/env bash
# Pin (finish item A4): digests / base64_encode of a string that is NOT valid
# UTF-8 (substr() splitting a multibyte char) must use the real bytes, not the
# empty string. Fails on a binary where handle_to_str collapsed them to "".
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
f="tests/invalid_utf8_digest_pin.kry"
want="4
Y2Fmww==
a9b1e7dae10adcf757827fb9796a1469fa6bd72039bb177bd3439ee2ae4411ac
445303e8e06aff19e5b37a3640371b46571626e5
RFMD6OBq/xnls3o2QDcbRlcWJuU=
2d44a9cf49c37e4b62db82a4f60a5353
cc1487deb157e51c6edcdea5ea8439a7d519401e319cb14b042f1e2f8286ffb3
false
Y2Fm6Q=="
fail=0
jit="$(timeout 60 "$K" run "$f" 2>&1 | tr -d '\r')"
if [ "$jit" = "$want" ]; then echo "  ok   invalid-utf8 digests JIT"; else
    echo "  FAIL: invalid-utf8 digests JIT"; printf '%s\n' "$jit" | sed 's/^/    got: /'; fail=1; fi
if timeout 300 "$K" build --release "$f" -o "$TMP/p.exe" >"$TMP/b.log" 2>&1; then
    aot="$(timeout 60 "$TMP/p.exe" 2>&1 | tr -d '\r')"
    if [ "$aot" = "$want" ]; then echo "  ok   invalid-utf8 digests AOT"; else
        echo "  FAIL: invalid-utf8 digests AOT"; printf '%s\n' "$aot" | sed 's/^/    got: /'; fail=1; fi
else echo "  FAIL: AOT build"; tail -5 "$TMP/b.log"; fail=1; fi
[ "$fail" -eq 0 ] && echo "invalid-utf8-digest pin: ok"
exit "$fail"
