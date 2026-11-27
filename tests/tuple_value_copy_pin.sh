#!/usr/bin/env bash
# Pin (finish item A20): a tuple is a VALUE. On the Cranelift JIT a tuple is one refcounted heap
# array that every owner shared (`let mut u = t`, `let mut e = arr[i]`, `u = t`, `push(a, t)`,
# `a[i] = t`, `S { p: t }`, `m[k]`), so `u.0 = 7` also changed `t.0`; the AOT tuple is an inline
# value and was always right. A write through a tuple that is still shared (`ref_count > 1`) now
# moves the writing holder onto its own copy first (MIR `__kryos_tuple_cow*`, runtime
# `kryos_tuple_cow`). Each program's expected output is the value-semantic answer, run on BOTH
# backends.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="${KRYOS_BIN:-compiler/target/release/kryos.exe}"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0
for i in 1 2 3 4 5 6 7; do
    f="tests/tuple_value_copy_pin_$i.kry"
    want="$(tr -d '\r' < "tests/tuple_value_copy_pin_$i.out")"
    jit="$(timeout 120 "$K" run "$f" 2>&1 | tr -d '\r')"
    if [ "$jit" = "$want" ]; then echo "  ok   tuple value copy $i JIT"; else
        echo "  FAIL: tuple value copy $i JIT"; diff <(printf '%s\n' "$want") <(printf '%s\n' "$jit") | sed 's/^/    /' | head -12; fail=1; fi
    if timeout 300 "$K" build --release "$f" -o "$TMP/p$i.exe" >"$TMP/b$i.log" 2>&1; then
        aot="$(timeout 60 "$TMP/p$i.exe" 2>&1 | tr -d '\r')"
        if [ "$aot" = "$want" ]; then echo "  ok   tuple value copy $i AOT"; else
            echo "  FAIL: tuple value copy $i AOT"; diff <(printf '%s\n' "$want") <(printf '%s\n' "$aot") | sed 's/^/    /' | head -12; fail=1; fi
    else echo "  FAIL: AOT build $i"; tail -5 "$TMP/b$i.log"; fail=1; fi
done
[ "$fail" -eq 0 ] && echo "tuple value copy pin: ok"
exit "$fail"
