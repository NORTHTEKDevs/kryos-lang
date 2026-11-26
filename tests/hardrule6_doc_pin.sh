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
[ "$bad" = 0 ] && echo "PASS hardrule6_doc_pin"
exit "$bad"
