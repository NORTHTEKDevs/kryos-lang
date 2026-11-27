#!/usr/bin/env bash
# abort() gate (finish-line item abort-undefined).
# `abort()` is documented as an ambient builtin (no capability) but the checker
# never registered it, so any program calling it died with E0102 "undefined
# variable". It must now compile, print what ran before it, terminate with a
# nonzero status and never run the code after it, on the JIT, the Cranelift AOT
# and the LLVM AOT. The capability-modelled names env_set / exec / spawn_process
# (not callable builtins) must give E0114, not "undefined variable" (E0102). A
# user-defined `fn abort` must still win over the builtin.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
K="compiler/target/release/kryos.exe"; [ -x "$K" ] || K="compiler/target/release/kryos"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0

cat > "$TMP/ab.kry" <<'KRY'
fn main() {
    println("before")
    abort()
    println("after")
}
KRY
chk() { # label out rc
    if [ "$3" -ne 0 ] && printf '%s' "$2" | grep -q '^before' \
        && ! printf '%s' "$2" | grep -q -e 'after' -e 'E0102' -e 'compilation failed'; then
        echo "  ok   $1 (exit $3)"
    else
        echo "  FAIL: $1 (exit $3):"; printf '%s\n' "$2" | head -4 | sed 's/^/    /'; fail=1
    fi
}
out="$(timeout 60 "$K" run "$TMP/ab.kry" 2>&1)"; chk "abort() JIT" "$out" $?
for be in cranelift llvm; do
    if timeout 300 "$K" build --backend "$be" "$TMP/ab.kry" -o "$TMP/ab.$be.exe" >/dev/null 2>&1; then
        out="$(timeout 60 "$TMP/ab.$be.exe" 2>&1)"; chk "abort() AOT $be" "$out" $?
    else
        echo "  FAIL: abort() AOT $be build failed"; fail=1
    fi
done

for fn in 'env_set("A", "b")' 'exec("echo hi")' 'spawn_process("echo hi")'; do
    printf 'fn main() {\n    %s\n}\n' "$fn" > "$TMP/n.kry"
    out="$(timeout 60 "$K" check --capabilities-mode=permissive "$TMP/n.kry" 2>&1)"; rc=$?
    if [ "$rc" -ne 0 ] && printf '%s' "$out" | grep -q 'E0114' && ! printf '%s' "$out" | grep -q 'E0102'; then
        echo "  ok   $fn is E0114, not E0102"
    else
        echo "  FAIL: $fn (exit $rc):"; printf '%s\n' "$out" | head -3 | sed 's/^/    /'; fail=1
    fi
done

# Control: a user-defined abort still shadows the builtin.
cat > "$TMP/sh.kry" <<'KRY'
fn abort() {
    println("user abort")
}
fn main() {
    abort()
    println("done")
}
KRY
out="$(timeout 60 "$K" run "$TMP/sh.kry" 2>&1)"; rc=$?
if [ "$rc" -eq 0 ] && [ "$out" = "$(printf 'user abort\ndone')" ]; then
    echo "  ok   user-defined abort shadows the builtin"
else
    echo "  FAIL: user-defined abort (exit $rc):"; printf '%s\n' "$out" | head -4 | sed 's/^/    /'; fail=1
fi

[ "$fail" -eq 0 ] && echo "abort-builtin-gate: all green"
exit "$fail"
