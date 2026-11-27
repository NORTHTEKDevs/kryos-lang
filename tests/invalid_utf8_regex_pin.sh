#!/usr/bin/env bash
# Pin (finish item A4, regex/handle_to_str residual): every handle_to_str consumer given a string that
# is NOT valid UTF-8 (substr() splitting a multibyte char) must fail loudly, never answer for the empty
# string. Each program in tests/invalid_utf8_regex_pin/ (except control.kry) must exit non-zero with
# "requires valid UTF-8" on JIT and AOT; control.kry (valid strings) must print the right answers.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
D="tests/invalid_utf8_regex_pin"
fail=0
want_ctl="false
true
café
3
Xé
true"
for f in "$D"/*.kry; do
    n="$(basename "$f" .kry)"
    timeout 300 "$K" build --release "$f" -o "$TMP/$n.exe" >"$TMP/$n.log" 2>&1 || { echo "  FAIL: AOT build $n"; tail -3 "$TMP/$n.log"; fail=1; continue; }
    jit="$(timeout 60 "$K" run "$f" 2>&1 | tr -d '\r')"; jrc=${PIPESTATUS[0]}
    aot="$(timeout 60 "$TMP/$n.exe" 2>&1 | tr -d '\r')"; arc=${PIPESTATUS[0]}
    for m in JIT AOT; do
        if [ $m = JIT ]; then out="$jit"; rc=$jrc; else out="$aot"; rc=$arc; fi
        if [ "$n" = control ]; then
            if [ "$out" = "$want_ctl" ] && [ "$rc" -eq 0 ]; then echo "  ok   $n $m"; else
                echo "  FAIL: $n $m (rc=$rc)"; printf '%s\n' "$out" | sed 's/^/    got: /'; fail=1; fi
        else
            if [ "$rc" -ne 0 ] && printf '%s' "$out" | grep -q 'requires valid UTF-8'; then echo "  ok   $n $m"; else
                echo "  FAIL: $n $m (rc=$rc, want loud UTF-8 panic)"; printf '%s\n' "$out" | head -3 | sed 's/^/    got: /'; fail=1; fi
        fi
    done
done
[ "$fail" -eq 0 ] && echo "invalid-utf8-regex pin: ok"
exit "$fail"
