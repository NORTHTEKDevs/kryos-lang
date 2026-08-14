#!/usr/bin/env bash
# stdlib_compile_gate.sh -- every shipped stdlib module must actually compile.
#
# WHY: LEDGER item 29 found that `compiler/stdlib/smtp.kry` and
# `compiler/stdlib/term.kry` had NEVER been compiled by anything -- not by a
# conformance test, not by an example, not by CI. They shipped in the stdlib
# listing and in `docs/stdlib/*.md` while nothing ever type-checked them. Two
# modules is the count that was noticed; the real defect is that there was no
# mechanism to notice, so any future module could ship the same way.
#
# This imports each module ON ITS OWN (one probe file per module -- never two
# at once, because imports share ONE flat namespace and two modules exporting
# the same symbol collide by design, which is a documented language rule and
# not what this gate is testing) and requires `kryos check` to accept it.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1
K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos.exe}"
[ -x "$K" ] || K="$ROOT/compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

total=0; fail=0; failed_names=""
for f in "$ROOT"/compiler/stdlib/*.kry; do
    m="$(basename "$f" .kry)"
    total=$((total + 1))
    cat > "$TMP/use_$m.kry" <<EOF
use std::$m::*
fn main() { println("ok") }
EOF
    if ! out="$(timeout 120 "$K" check "$TMP/use_$m.kry" 2>&1)"; then
        fail=$((fail + 1)); failed_names="$failed_names $m"
        echo "  FAIL  std::$m does not compile"
        printf '%s\n' "$out" | head -4 | sed 's/^/          /'
    fi
done

echo
if [ "$fail" -eq 0 ]; then
    echo "stdlib-compile: $total/$total modules compile"
    exit 0
fi
echo "stdlib-compile: $fail/$total modules FAILED to compile --$failed_names"
exit 1
