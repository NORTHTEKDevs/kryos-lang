#!/usr/bin/env bash
# Assert-shadow gate (LEDGER item 2c): `std::test::assert`'s 2-arg form used
# to be PERMANENTLY SHADOWED by the compiler's own hardcoded 2-arg `assert`
# intrinsic (kryos_builtin_assert -- prints and calls process::abort(),
# never returns), even after `use std::test::{assert}`. Both codegen
# backends dispatched any call literally named `assert`/`assert_eq`/`panic`
# straight to the intrinsic UNCONDITIONALLY, before the generic user-shadow
# check every other builtin already goes through.
#
# This gate asserts the fix in BOTH directions (the general conformance
# harness can only assert exit 0, so the "still aborts uncatchably when
# unshadowed" half needs a nonzero-exit assertion, same reason
# utf8_invalid_string_gate.sh is a standalone script and not a conf_*.kry):
#   1. `use std::test::{assert}` + try/catch around a failing assert must be
#      CATCHABLE (exit 0, the throw's message reaches the catch block).
#   2. A program that does NOT import std::test::assert must keep the TRUE
#      intrinsic's uncatchable-abort semantics exactly as before (nonzero
#      exit, no "kryos: uncaught exception:" prefix) -- proves the fix did
#      not accidentally make every program's bare `assert` catchable.
# Both checked on BOTH backends (`kryos run` and `kryos build --release`).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="compiler/target/release/kryos.exe"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0

# --- 1. shadowed: std::test::assert must be catchable ----------------------
cat > "$TMP/shadowed.kry" <<'KRY'
use std::test::{assert}
fn main() {
    try {
        assert(false, "boom")
        println("unreachable")
    } catch (e) {
        println("caught: " + e)
    }
    println("after")
}
KRY

out_jit="$("$K" run "$TMP/shadowed.kry" 2>&1)"; rc_jit=$?
if [ "$rc_jit" -eq 0 ] && grep -q "caught: assertion failed: boom" <<<"$out_jit" && grep -q "^after$" <<<"$out_jit"; then
    echo "  ok   JIT: std::test::assert is catchable (exit 0)"
else
    echo "  FAIL: JIT std::test::assert not caught (rc=$rc_jit):"; echo "$out_jit" | sed 's/^/    /'; fail=1
fi

if "$K" build --release "$TMP/shadowed.kry" -o "$TMP/shadowed_aot" >"$TMP/aot_build.log" 2>&1; then
    out_aot="$("$TMP/shadowed_aot" 2>&1)"; rc_aot=$?
    if [ "$rc_aot" -eq 0 ] && grep -q "caught: assertion failed: boom" <<<"$out_aot" && grep -q "^after$" <<<"$out_aot"; then
        echo "  ok   AOT: std::test::assert is catchable (exit 0)"
    else
        echo "  FAIL: AOT std::test::assert not caught (rc=$rc_aot):"; echo "$out_aot" | sed 's/^/    /'; fail=1
    fi
else
    echo "  FAIL: AOT build of shadowed assert failed:"; cat "$TMP/aot_build.log" | sed 's/^/    /'; fail=1
fi

# --- 2. unshadowed: the TRUE intrinsic must still abort uncatchably --------
cat > "$TMP/unshadowed.kry" <<'KRY'
fn main() {
    assert(true, "should not fire")
    println("passed-first-assert")
    assert(false, "boom")
    println("unreachable")
}
KRY

out_jit="$("$K" run "$TMP/unshadowed.kry" 2>&1)"; rc_jit=$?
if [ "$rc_jit" -ne 0 ] && grep -q "passed-first-assert" <<<"$out_jit" \
   && grep -q "assertion failed: boom" <<<"$out_jit" \
   && ! grep -q "kryos: uncaught exception:" <<<"$out_jit"; then
    echo "  ok   JIT: unshadowed assert() still aborts uncatchably (rc=$rc_jit)"
else
    echo "  FAIL: JIT unshadowed assert() regressed (rc=$rc_jit):"; echo "$out_jit" | sed 's/^/    /'; fail=1
fi

if "$K" build --release "$TMP/unshadowed.kry" -o "$TMP/unshadowed_aot" >"$TMP/aot_build2.log" 2>&1; then
    out_aot="$("$TMP/unshadowed_aot" 2>&1)"; rc_aot=$?
    if [ "$rc_aot" -ne 0 ] && grep -q "passed-first-assert" <<<"$out_aot" \
       && grep -q "assertion failed: boom" <<<"$out_aot" \
       && ! grep -q "kryos: uncaught exception:" <<<"$out_aot"; then
        echo "  ok   AOT: unshadowed assert() still aborts uncatchably (rc=$rc_aot)"
    else
        echo "  FAIL: AOT unshadowed assert() regressed (rc=$rc_aot):"; echo "$out_aot" | sed 's/^/    /'; fail=1
    fi
else
    echo "  FAIL: AOT build of unshadowed assert failed:"; cat "$TMP/aot_build2.log" | sed 's/^/    /'; fail=1
fi

if [ $fail -eq 0 ]; then
    echo "assert_shadow_gate: PASS"
else
    echo "assert_shadow_gate: FAIL"
fi
exit $fail
