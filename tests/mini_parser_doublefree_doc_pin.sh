#!/usr/bin/env bash
# mini_parser_doublefree_doc_pin.sh -- M7: docs/BUGS-mini-parser-double-free.md
# described 60/7 double-frees and an rc=139 SEGFAULT in stage1_mini_parser.kry
# as a live bug with no resolved marker. The program now runs clean, so the doc
# must say RESOLVED in its first lines, and the claim it makes (rc=0, zero
# double-frees under KRYOS_FREE_DIAG=1) must hold on the binary under test.
# DOC=<file> overrides the doc path (used to show the old doc fails).
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos.exe}"
[ -x "$K" ] || K="$ROOT/compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
DOC="${DOC:-$ROOT/docs/BUGS-mini-parser-double-free.md}"
SRC="$ROOT/compiler/self-host/stage1_mini_parser.kry"
W="$(mktemp -d)"
trap 'rm -rf "$W"' EXIT
fail=0

if [ -f "$DOC" ]; then
    if ! head -7 "$DOC" | grep -qE '\b(FIXED|RESOLVED|CLOSED|Resolved|Fixed|Closed)\b'; then
        echo "FAIL doc: no RESOLVED marker in first 7 lines of $DOC"; fail=1
    fi
fi

KRYOS_FREE_DIAG=1 timeout 300 "$K" run "$SRC" >"$W/diag.txt" 2>&1; rc=$?
if [ $rc -ne 0 ] || grep -qi 'double.\?free' "$W/diag.txt" || ! grep -q 'stage 1 mini-parser: ok' "$W/diag.txt"; then
    echo "FAIL diag: rc=$rc under KRYOS_FREE_DIAG=1"; fail=1
fi
timeout 300 "$K" run "$SRC" >"$W/norm.txt" 2>&1; rc=$?
if [ $rc -ne 0 ] || ! grep -q 'stage 1 mini-parser: ok' "$W/norm.txt"; then
    echo "FAIL normal: rc=$rc"; fail=1
fi

[ $fail -eq 0 ] && echo "PASS mini_parser_doublefree_doc_pin (doc marked resolved, 0 double-frees, rc=0)"
exit $fail
