#!/usr/bin/env bash
# Security gate: capability attenuation must actually attenuate.
#
# Kryos is deployed as capability-attenuated infrastructure for agent tooling.
# If a program declaring NO capabilities can reach memory it was never handed,
# every attenuation guarantee above it is void. This gate asserts the boundary
# holds, in BOTH directions -- the escape is rejected AND legitimate declared
# use still works, so the fix cannot be "reject everything".
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="compiler/target/release/kryos.exe"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0

# 1. The escape must NOT compile.
if "$K" run tests/security/cap_escape_raw_memory.kry >"$TMP/esc" 2>&1; then
    echo "  FAIL: the raw-memory capability escape COMPILED AND RAN"
    grep -i recovered "$TMP/esc" | head -2 | sed 's/^/    /'
    fail=1
elif grep -q "requires \`ffi\`" "$TMP/esc"; then
    echo "  ok   raw-memory escape rejected (requires \`ffi\`)"
else
    echo "  FAIL: escape rejected, but NOT for the capability reason:"
    tail -3 "$TMP/esc" | sed 's/^/    /'
    fail=1
fi

# 2. Declared use must still work (guards against over-rejection).
cat > "$TMP/ok.kry" <<'KRY'
@capabilities(ffi)
fn main() {
  let p = alloc(32)
  ptr_write_i64(p, 0, 7)
  println(to_string(ptr_read_i64(p, 0)))
  free_bytes(p, 32)
}
KRY
if [ "$("$K" run "$TMP/ok.kry" 2>/dev/null | tail -1)" = "7" ]; then
    echo "  ok   @capabilities(ffi) still permits raw memory"
else
    echo "  FAIL: declared ffi code was rejected -- the gate over-rejects"; fail=1
fi

# 3. The stdlib must NOT cascade: a plain std::json user needs no ffi.
cat > "$TMP/nocascade.kry" <<'KRY'
use std::json::{parse, stringify}
fn main() { println(stringify(parse("{{\"a\":1}}"))) }
KRY
if "$K" check "$TMP/nocascade.kry" >"$TMP/nc" 2>&1; then
    echo "  ok   stdlib users need no ffi (no cascade)"
else
    echo "  FAIL: gating cascaded out of the stdlib:"; tail -3 "$TMP/nc" | sed 's/^/    /'; fail=1
fi

[ $fail -eq 0 ] && echo "security-gate: PASS" || echo "security-gate: FAIL"
exit $fail
