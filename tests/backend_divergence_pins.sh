#!/usr/bin/env bash
# Backend-divergence pins.
#
# A handful of documented backend disagreements (Cranelift/JIT vs LLVM/AOT)
# are ACCEPTED boundaries, not bugs to be silently patched over: the launch
# decision was to document them (CLAUDE.md gotchas) rather than force an
# ABI-level fix. "Documented" is worthless if nothing checks it stays true --
# a later change could shift EITHER backend's output with no test catching
# the drift, silently invalidating the docs. This gate pins the EXACT
# current output of each backend for each documented divergence. A FAIL here
# means one of two things, and both require action before merging:
#   (a) the divergence drifted to a NEW shape -- investigate, it may be a
#       fresh bug, not the documented one; or
#   (b) the divergence was FIXED (backends now agree) -- update this pin,
#       CLAUDE.md's gotcha, and the LEDGER item together in the same commit,
#       do not just delete the pin.
# This gate intentionally does NOT require the two backends to agree (unlike
# tests/parity/run_parity.sh, which hard-gates smoke/ on byte-identical
# output) -- it asserts the specific, currently-accepted DIFFERENT values.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="compiler/target/release/kryos.exe"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0

# LEDGER item 15 / CLAUDE.md gotcha #23 (last bullet): `let a = arr[i]` on an
# array-of-structs returns a SHARED HANDLE on Cranelift/JIT (every alias of
# the same read sees a later mutation through any one of them) but an
# INDEPENDENT COPY on LLVM/AOT (only the alias mutated through sees it).
f="tests/security/attack_container_element_alias_refcount.kry"
jit_expected="last=x19999!|x19999!|x19999!|x19999!|5"
aot_expected="last=x19999!|x19999|x19999|x19999|5"

jit_out="$("$K" run "$f" 2>&1)"; jit_rc=$?
if [ "$jit_rc" -ne 0 ]; then
    echo "  FAIL: alias-refcount JIT run exited $jit_rc (expected 0):"
    printf '%s\n' "$jit_out" | tail -5 | sed 's/^/    /'
    fail=1
elif printf '%s\n' "$jit_out" | grep -qF "$jit_expected"; then
    echo "  ok   alias-refcount JIT pin holds ($jit_expected)"
else
    echo "  FAIL: alias-refcount JIT output drifted from the pinned value:"
    echo "    expected: $jit_expected"
    printf '%s\n' "$jit_out" | grep '^last=' | sed 's/^/    got:      /'
    fail=1
fi

aot_bin="$TMP/alias_refcount.exe"
if ! "$K" build --release "$f" -o "$aot_bin" >"$TMP/build.log" 2>&1; then
    echo "  FAIL: alias-refcount AOT build failed:"
    tail -5 "$TMP/build.log" | sed 's/^/    /'
    fail=1
else
    aot_out="$("$aot_bin" 2>&1)"; aot_rc=$?
    if [ "$aot_rc" -ne 0 ]; then
        echo "  FAIL: alias-refcount AOT run exited $aot_rc (expected 0):"
        printf '%s\n' "$aot_out" | tail -5 | sed 's/^/    /'
        fail=1
    elif printf '%s\n' "$aot_out" | grep -qF "$aot_expected"; then
        echo "  ok   alias-refcount AOT pin holds ($aot_expected)"
    else
        echo "  FAIL: alias-refcount AOT output drifted from the pinned value:"
        echo "    expected: $aot_expected"
        printf '%s\n' "$aot_out" | grep '^last=' | sed 's/^/    got:      /'
        fail=1
    fi
fi

[ $fail -eq 0 ] && echo "backend-divergence-pins: PASS" || echo "backend-divergence-pins: FAIL"
exit $fail
