#!/usr/bin/env bash
# Pin (finish item A4, ed25519 residual): ed25519_sign/verify message of a string that is NOT valid
# UTF-8 (substr() splitting a multibyte char) must use the real bytes, not the
# empty string. Fails on a binary where handle_to_str collapsed them to "".
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
f="tests/invalid_utf8_ed25519_pin.kry"
want="false
false
true
1
0
0
0
0
1"
fail=0
jit="$(timeout 60 "$K" run "$f" 2>&1 | tr -d '\r')"
if [ "$jit" = "$want" ]; then echo "  ok   invalid-utf8 ed25519 JIT"; else
    echo "  FAIL: invalid-utf8 ed25519 JIT"; printf '%s\n' "$jit" | sed 's/^/    got: /'; fail=1; fi
if timeout 300 "$K" build --release "$f" -o "$TMP/p.exe" >"$TMP/b.log" 2>&1; then
    aot="$(timeout 60 "$TMP/p.exe" 2>&1 | tr -d '\r')"
    if [ "$aot" = "$want" ]; then echo "  ok   invalid-utf8 ed25519 AOT"; else
        echo "  FAIL: invalid-utf8 ed25519 AOT"; printf '%s\n' "$aot" | sed 's/^/    got: /'; fail=1; fi
else echo "  FAIL: AOT build"; tail -5 "$TMP/b.log"; fail=1; fi
[ "$fail" -eq 0 ] && echo "invalid-utf8-ed25519 pin: ok"
exit "$fail"
