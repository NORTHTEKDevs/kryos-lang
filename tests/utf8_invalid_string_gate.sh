#!/usr/bin/env bash
# UTF-8 invalid-string gate: a Kryos string that ends up holding invalid
# UTF-8 bytes (reachable purely from ordinary code -- substr() is
# byte-indexed and nothing stops a caller from landing mid-codepoint) must
# fail LOUDLY on a text-semantic operation, never silently.
#
# Before this fix: `contains`, `trim`, `to_upper`, `to_lower`, and `replace`
# (all Rust-builtin-backed) converted the raw bytes to &str via
# `str::from_utf8(..).unwrap_or("")` -- ANY invalid byte in the string made
# the WHOLE operation act as if the string were empty. `trim`/`to_upper`/
# `to_lower`/`replace` therefore silently DISCARDED the entire original
# content (returned "") and `contains` silently returned `false` even for a
# needle that is a genuine byte-prefix of the haystack -- no panic, no
# diagnostic, just wrong data. This gate asserts the fix (a loud panic
# naming the real cause) in BOTH directions: the invalid-input case must
# fail LOUDLY, and ordinary valid multibyte input must keep working exactly
# as before (the fix must not turn a valid string operation into a panic).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="compiler/target/release/kryos.exe"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0

# 1. Invalid UTF-8 (produced by an ordinary substr() that splits a multibyte
#    codepoint) must PANIC LOUDLY on each affected builtin, not silently
#    return "" / false.
for op in "trim(bad)" "to_upper(bad)" "to_lower(bad)" "replace(bad, \"x\", \"y\")" "contains(bad, \"caf\")"; do
    cat > "$TMP/bad.kry" <<KRY
fn main() {
    let s = "café"
    let bad = substr(s, 0, 4)
    println(to_string($op))
}
KRY
    out="$("$K" run "$TMP/bad.kry" 2>&1)"; rc=$?
    if [ "$rc" -eq 0 ]; then
        echo "  FAIL: $op on invalid UTF-8 exited 0 (should panic loudly) -- got: $out"
        fail=1
    elif printf '%s' "$out" | grep -qi "UTF-8" && printf '%s' "$out" | grep -qi "invalid"; then
        echo "  ok   $op on invalid UTF-8 panics with a clear diagnostic (exit $rc)"
    else
        echo "  FAIL: $op on invalid UTF-8 failed (exit $rc) but NOT with a UTF-8 diagnosis:"
        printf '%s\n' "$out" | tail -3 | sed 's/^/    /'
        fail=1
    fi
done

# 2. Guard against over-rejection: the SAME operations on ordinary VALID
#    multibyte input must still work and exit 0 (the fix must not make every
#    non-ASCII string panic).
cat > "$TMP/good.kry" <<'KRY'
fn main() {
    println(trim("  café  "))
    println(to_upper("café"))
    println(to_lower("CAFÉ"))
    println(replace("café menu", "menu", "list"))
    println(to_string(contains("café menu", "café")))
}
KRY
out="$("$K" run "$TMP/good.kry" 2>&1)"; rc=$?
expected="$(printf 'café\nCAFÉ\ncafé\ncafé list\ntrue')"
if [ "$rc" -eq 0 ] && [ "$out" = "$expected" ]; then
    echo "  ok   valid multibyte input still works on all five builtins (no over-rejection)"
else
    echo "  FAIL: valid multibyte input regressed (exit $rc):"
    printf '%s\n' "$out" | sed 's/^/    /'
    fail=1
fi

[ "$fail" -eq 0 ] && echo "utf8-invalid-string-gate: all green"
exit "$fail"
