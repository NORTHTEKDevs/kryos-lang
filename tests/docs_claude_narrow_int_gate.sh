#!/usr/bin/env bash
# Pin: docs must describe narrow-int literal range checks as partial (E0111 in let/assign/arg only).
root="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
book="$root/docs/learn/book/20-idioms-and-pitfalls.md"
for f in "$root/CLAUDE.md" "$book"; do
  if grep -qiE "literals (are not|aren't) range-checked at compile time|literals ARE range-checked at compile time" "$f"; then
    echo "FAIL docs_claude_narrow_int_gate: blanket range-check claim in $f"; exit 1
  fi
done
grep -q 'E0111' "$root/CLAUDE.md" || { echo "FAIL docs_claude_narrow_int_gate: E0111 not documented"; exit 1; }
grep -q 'NOT checked and truncate silently' "$root/CLAUDE.md" || { echo "FAIL docs_claude_narrow_int_gate: unchecked positions not listed in CLAUDE.md"; exit 1; }
grep -q 'only partly range-checked' "$book" || { echo "FAIL docs_claude_narrow_int_gate: book pitfall not updated"; exit 1; }
echo "PASS docs_claude_narrow_int_gate"
