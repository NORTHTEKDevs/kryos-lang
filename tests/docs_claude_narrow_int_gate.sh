#!/usr/bin/env bash
# Pin: CLAUDE.md must not claim narrow-int literals are unchecked (compiler emits E0111).
root="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
if grep -q 'literals are not range-checked at compile time' "$root/CLAUDE.md"; then
  echo "FAIL docs_claude_narrow_int_gate: stale range-check claim in CLAUDE.md"; exit 1
fi
grep -q 'E0111' "$root/CLAUDE.md" || { echo "FAIL docs_claude_narrow_int_gate: E0111 not documented"; exit 1; }
echo "PASS docs_claude_narrow_int_gate"
