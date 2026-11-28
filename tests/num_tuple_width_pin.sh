#!/usr/bin/env bash
# Pin (finish item A5): a tuple of integers moved into a tuple slot of the same
# shape at other integer widths -- `(i32, i32)` into `(i64, i64)` through a
# return, annotated let, call argument, struct field, enum payload, map / array
# element, push, closure call, if / match join, nested tuple, or an array of
# tuples -- must be converted element by element (an unsigned element
# zero-extends). On master the AOT build failed ("defined with type { i32, i32 }
# but expected { i64, i64 }") or printed `0 0` / wrong bytes, and the JIT
# sign-extended a u8/u32 element (200 read back as -56). array_copy.kry: an array of
# such tuples moved into an array of the same tuple at other widths (call result
# as argument, annotated let, return, struct / enum field, store, closure arg)
# is a converted copy.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0
for name in scalar array array_copy a5_c7 a5_c7b; do
    f="tests/num_tuple_width/$name.kry"
    want="$(tr -d '\r' < "tests/num_tuple_width/$name.out")"
    jit="$(timeout 120 "$K" run "$f" 2>&1 | tr -d '\r')"
    if [ "$jit" = "$want" ]; then echo "  ok   $name JIT"; else
        echo "  FAIL: $name JIT"; printf '%s\n' "$jit" | sed 's/^/    got: /' | head -20; fail=1; fi
    if timeout 300 "$K" build --release "$f" -o "$TMP/$name.exe" >"$TMP/$name.log" 2>&1; then
        aot="$(timeout 120 "$TMP/$name.exe" 2>&1 | tr -d '\r')"
        if [ "$aot" = "$want" ]; then echo "  ok   $name AOT"; else
            echo "  FAIL: $name AOT"; printf '%s\n' "$aot" | sed 's/^/    got: /' | head -20; fail=1; fi
    else echo "  FAIL: $name AOT build"; head -4 "$TMP/$name.log"; fail=1; fi
done
[ "$fail" -eq 0 ] && echo "num-tuple-width pin: ok" || echo "num-tuple-width pin: FAIL"
exit "$fail"
