#!/usr/bin/env bash
# hardrule6_doc_pin.sh -- 2c-hardrule6: CLAUDE.md hard rule #6 claimed type
# annotations on a top-level `let` and on function params are required, but
# the compiler accepts and runs both unannotated. The doc must not claim it,
# and the behavior the new wording relies on must hold. KRYOS_DOC_ROOT points
# the doc grep at another checkout (master) to show the pin fails there.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DOC="${KRYOS_DOC_ROOT:-$ROOT}"
K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos.exe}"
[ -x "$K" ] || K="$ROOT/compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
W="$(mktemp -d)"
trap 'rm -rf "$W"' EXIT
bad=0
for f in CLAUDE.md docs/claude/FULL-REFERENCE.md; do
  if grep -Eq 'Type annotations on top-level `let`.*required' "$DOC/$f"; then echo "FAIL $f: stale 'annotations required' claim"; bad=1; fi
done
printf 'let count = 5\nfn add(a, b) -> i64 { return a + b }\nfn main() { println(to_string(count + add(1, 2))) }\n' > "$W/t.kry"
out="$(timeout 60 "$K" run "$W/t.kry" 2>&1)"
[ "$out" = "8" ] || { echo "FAIL t.kry: want 8, got: $out"; bad=1; }
# the doc must not overclaim: field access on an unannotated param is rejected
for f in CLAUDE.md docs/claude/FULL-REFERENCE.md; do
  if grep -Eq 'the compiler infers both' "$DOC/$f"; then echo "FAIL $f: overbroad 'compiler infers both' claim"; bad=1; fi
  grep -Fq 'field or method access needs its annotation' "$DOC/$f" || grep -Fq 'needs its annotation' "$DOC/$f" || { echo "FAIL $f: missing field-access annotation caveat"; bad=1; }
done
printf 'struct P { x: i64 }
fn getx(p) -> i64 { return p.x }
fn main() { let q = P { x: 7 }
 println(to_string(getx(q))) }
' > "$W/u.kry"
out="$(timeout 60 "$K" run "$W/u.kry" 2>&1)"
case "$out" in *E0110*) ;; *) echo "FAIL u.kry: expected E0110 (caveat in doc relies on it), got: $out"; bad=1;; esac
printf 'struct P { x: i64 }
fn getx(p: P) -> i64 { return p.x }
fn main() { let q = P { x: 7 }
 println(to_string(getx(q))) }
' > "$W/v.kry"
out="$(timeout 60 "$K" run "$W/v.kry" 2>&1)"
[ "$out" = "7" ] || { echo "FAIL v.kry: want 7, got: $out"; bad=1; }
[ "$bad" = 0 ] && echo "PASS hardrule6_doc_pin"
exit "$bad"
