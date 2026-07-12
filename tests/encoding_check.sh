#!/usr/bin/env bash
# encoding_check.sh -- source files in the encodings real editors produce
# must either compile (UTF-8 BOM, CRLF, tabs, no trailing newline) or be
# rejected with an ACTIONABLE message (UTF-16), never token soup.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
KRYOS="${KRYOS:-$ROOT/compiler/target/release/kryos}"
if [ ! -x "$KRYOS" ] && [ -x "$KRYOS.exe" ]; then KRYOS="$KRYOS.exe"; fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

python3 - "$TMP" <<'PYEOF'
import sys
tmp = sys.argv[1]
src = 'fn main() {\n    let x = 40 + 2\n    println("x=" + to_string(x))\n}\n'
open(f'{tmp}/crlf.kry', 'wb').write(src.replace('\n', '\r\n').encode())
open(f'{tmp}/bom.kry', 'wb').write(b'\xef\xbb\xbf' + src.encode())
open(f'{tmp}/bomcrlf.kry', 'wb').write(b'\xef\xbb\xbf' + src.replace('\n', '\r\n').encode())
open(f'{tmp}/notrail.kry', 'wb').write(src.rstrip('\n').encode())
open(f'{tmp}/tabs.kry', 'wb').write(src.replace('    ', '\t').encode())
open(f'{tmp}/utf16.kry', 'wb').write(src.encode('utf-16'))  # with BOM
PYEOF

fail=0
for v in crlf bom bomcrlf notrail tabs; do
    out="$("$KRYOS" run "$TMP/$v.kry" 2>&1)"
    if printf '%s' "$out" | grep -q "x=42"; then
        echo "PASS $v"
    else
        echo "FAIL $v (should compile and print x=42)"
        printf '%s\n' "$out" | head -3
        fail=1
    fi
done

out="$("$KRYOS" run "$TMP/utf16.kry" 2>&1)"
if printf '%s' "$out" | grep -qi "UTF-16"; then
    echo "PASS utf16 (clear diagnostic)"
else
    echo "FAIL utf16 (expected an actionable UTF-16 message)"
    printf '%s\n' "$out" | head -3
    fail=1
fi

[ "$fail" -eq 0 ] && echo "encoding-check: all green"
exit "$fail"
