#!/usr/bin/env bash
# buf_get_byte out-of-range gate (finish-line item buf_get_byte-sentinel).
# buf_get_byte(b, i) used to return -1 for i outside the buffer while arr[i]
# panics; -1 reads as data to any caller that does not test for it. It must
# now panic ("buffer index out of bounds") on BOTH backends, for a too-large
# and a negative index, while in-range reads are unchanged. A panic needs a
# nonzero-exit assertion, which a conf_*.kry cannot make, hence this script.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="compiler/target/release/kryos.exe"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0

# check_oob <name> <index expr>: reads 7 in range, then must panic on the OOB read.
check_oob() {
    local name="$1" idx="$2"
    cat > "$TMP/$name.kry" <<KRY
fn main() {
    let b: i64 = buf_new(16)
    buf_write_byte(b, 7)
    println(to_string(buf_get_byte(b, 0)))
    let i: i64 = $idx
    println(to_string(buf_get_byte(b, i)))
    println("not reached")
}
KRY
    local out rc
    out="$(timeout 60 "$K" run "$TMP/$name.kry" 2>&1)"; rc=$?
    if [ "$rc" -ne 0 ] && printf '%s' "$out" | grep -q "buffer index out of bounds" \
        && ! printf '%s' "$out" | grep -q -e '^-1$' -e 'not reached'; then
        echo "  ok   $name JIT panics (exit $rc)"
    else
        echo "  FAIL: $name JIT (exit $rc) did not panic with 'buffer index out of bounds':"
        printf '%s\n' "$out" | head -4 | sed 's/^/    /'
        fail=1
    fi
    if timeout 300 "$K" build --release --backend llvm "$TMP/$name.kry" -o "$TMP/$name.aot.exe" >/dev/null 2>&1; then
        out="$(timeout 60 "$TMP/$name.aot.exe" 2>&1)"; rc=$?
        if [ "$rc" -ne 0 ] && printf '%s' "$out" | grep -q "buffer index out of bounds" \
            && ! printf '%s' "$out" | grep -q -e '^-1$' -e 'not reached'; then
            echo "  ok   $name AOT panics (exit $rc)"
        else
            echo "  FAIL: $name AOT (exit $rc) did not panic with 'buffer index out of bounds':"
            printf '%s\n' "$out" | head -4 | sed 's/^/    /'
            fail=1
        fi
    else
        echo "  FAIL: $name AOT build failed"
        fail=1
    fi
}

check_oob oob_high 99
check_oob oob_at_len 1
check_oob oob_negative -1

# Control: in-range reads (first, last, a 0 byte, a 255 byte) are unchanged.
cat > "$TMP/ok.kry" <<'KRY'
fn main() {
    let b: i64 = buf_new(16)
    buf_write_byte(b, 7)
    buf_write_byte(b, 0)
    buf_write_byte(b, 255)
    println("{buf_get_byte(b, 0)} {buf_get_byte(b, 1)} {buf_get_byte(b, 2)}")
    buf_set_byte(b, 1, 9)
    println("{buf_get_byte(b, 1)}")
}
KRY
exp="$(printf '7 0 255\n9')"
out="$(timeout 60 "$K" run "$TMP/ok.kry" 2>&1)"; rc=$?
if [ "$rc" -eq 0 ] && [ "$out" = "$exp" ]; then
    echo "  ok   in-range reads unchanged (JIT)"
else
    echo "  FAIL: in-range reads (JIT, exit $rc):"; printf '%s\n' "$out" | sed 's/^/    /'; fail=1
fi
if timeout 300 "$K" build --release --backend llvm "$TMP/ok.kry" -o "$TMP/ok.aot.exe" >/dev/null 2>&1; then
    out="$(timeout 60 "$TMP/ok.aot.exe" 2>&1)"; rc=$?
    if [ "$rc" -eq 0 ] && [ "$out" = "$exp" ]; then
        echo "  ok   in-range reads unchanged (AOT)"
    else
        echo "  FAIL: in-range reads (AOT, exit $rc):"; printf '%s\n' "$out" | sed 's/^/    /'; fail=1
    fi
else
    echo "  FAIL: AOT build of in-range control failed"; fail=1
fi

[ "$fail" -eq 0 ] && echo "buf-get-byte-oob-gate: all green"
exit "$fail"
