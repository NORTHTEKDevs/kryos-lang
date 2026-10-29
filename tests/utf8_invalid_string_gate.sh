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

# 3. byte_at on a string holding invalid UTF-8 must read the raw byte, not
#    return -1 for EVERY index. Before the fix `byte_at(bad, 0)` on
#    "caf" + 0xC3 printed -1 (it is 99, 'c'), on BOTH backends, exit 0. Each
#    valid run still decodes to codepoints; each invalid byte is read raw, so
#    indices around a bad byte stay stable. Out of range stays -1.
#      bad  = 63 61 66 C3            -> 99 97 102 195 | -1
#      tail = E6 97 (cut 3-byte seq) -> 230 151 | -1
#      mid  = 97 A5 E6 (3 stray)     -> 151 165 230
#      mix  = 'A' C3 | C3 A9 ('e')   -> 65 195 233 | -1  (C3 C3 A9: first C3 invalid)
cat > "$TMP/ba_bad.kry" <<'KRY'
fn main() {
    let bad = substr("café", 0, 4)
    let tail = substr("日本", 0, 2)
    let mid = substr("日本", 1, 4)
    let mix = "A" + substr("é", 0, 1) + "é"
    println("{byte_at(bad, 0)} {byte_at(bad, 1)} {byte_at(bad, 2)} {byte_at(bad, 3)} {byte_at(bad, 4)}")
    println("{byte_at(tail, 0)} {byte_at(tail, 1)} {byte_at(tail, 2)}")
    println("{byte_at(mid, 0)} {byte_at(mid, 1)} {byte_at(mid, 2)}")
    println("{byte_at(mix, 0)} {byte_at(mix, 1)} {byte_at(mix, 2)} {byte_at(mix, 3)}")
    println("{byte_at(bad, -1)} {len(bad)}")
}
KRY
exp_bad="$(printf '99 97 102 195 -1\n230 151 -1\n151 165 230\n65 195 233 -1\n-1 4')"
out="$("$K" run "$TMP/ba_bad.kry" 2>&1)"; rc=$?
if [ "$rc" -eq 0 ] && [ "$out" = "$exp_bad" ]; then
    echo "  ok   byte_at reads the raw byte on invalid UTF-8 (JIT)"
else
    echo "  FAIL: byte_at on invalid UTF-8 (JIT, exit $rc), expected:"
    printf '%s\n' "$exp_bad" | sed 's/^/    exp /'; printf '%s\n' "$out" | sed 's/^/    got /'
    fail=1
fi
if "$K" build --release --backend llvm "$TMP/ba_bad.kry" -o "$TMP/ba_bad_aot" >/dev/null 2>&1; then
    out="$("$TMP/ba_bad_aot" 2>&1)"; rc=$?
    if [ "$rc" -eq 0 ] && [ "$out" = "$exp_bad" ]; then
        echo "  ok   byte_at reads the raw byte on invalid UTF-8 (AOT)"
    else
        echo "  FAIL: byte_at on invalid UTF-8 (AOT, exit $rc), expected:"
        printf '%s\n' "$exp_bad" | sed 's/^/    exp /'; printf '%s\n' "$out" | sed 's/^/    got /'
        fail=1
    fi
else
    echo "  FAIL: AOT build of byte_at invalid-UTF-8 probe failed"
    fail=1
fi

# 4. Guard the other direction: byte_at on VALID strings is unchanged
#    (CODEPOINT of the i-th character; a latin-1 chr() buffer reads back its
#    byte; out of range is -1).
cat > "$TMP/ba_good.kry" <<'KRY'
fn main() {
    let buf = chr(10) + chr(200) + chr(255) + chr(0)
    println("{byte_at("café", 0)} {byte_at("café", 3)} {byte_at("café", 4)}")
    println("{byte_at("日本", 0)} {byte_at("日本", 1)} {byte_at("日本", 2)}")
    println("{byte_at(buf, 0)} {byte_at(buf, 1)} {byte_at(buf, 2)} {byte_at(buf, 3)} {byte_at(buf, 4)}")
    println("{byte_at("", 0)} {byte_at("abc", -1)} {byte_at("abc", 2)}")
}
KRY
exp_good="$(printf '99 233 -1\n26085 26412 -1\n10 200 255 0 -1\n-1 -1 99')"
out="$("$K" run "$TMP/ba_good.kry" 2>&1)"; rc=$?
if [ "$rc" -eq 0 ] && [ "$out" = "$exp_good" ]; then
    echo "  ok   byte_at on valid strings unchanged (JIT)"
else
    echo "  FAIL: byte_at on valid strings regressed (JIT, exit $rc), expected:"
    printf '%s\n' "$exp_good" | sed 's/^/    exp /'; printf '%s\n' "$out" | sed 's/^/    got /'
    fail=1
fi
if "$K" build --release --backend llvm "$TMP/ba_good.kry" -o "$TMP/ba_good_aot" >/dev/null 2>&1; then
    out="$("$TMP/ba_good_aot" 2>&1)"; rc=$?
    if [ "$rc" -eq 0 ] && [ "$out" = "$exp_good" ]; then
        echo "  ok   byte_at on valid strings unchanged (AOT)"
    else
        echo "  FAIL: byte_at on valid strings regressed (AOT, exit $rc), expected:"
        printf '%s\n' "$exp_good" | sed 's/^/    exp /'; printf '%s\n' "$out" | sed 's/^/    got /'
        fail=1
    fi
else
    echo "  FAIL: AOT build of byte_at valid-string probe failed"
    fail=1
fi

[ "$fail" -eq 0 ] && echo "utf8-invalid-string-gate: all green"
exit "$fail"
