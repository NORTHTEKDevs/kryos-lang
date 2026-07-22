#!/usr/bin/env bash
# Regression: module imports must be case-EXACT (portability across Linux/CI),
# and `kryos audit` must render sub-capabilities glued (fs:write, not "fs : write").
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
K="$ROOT/compiler/target/release/kryos.exe"
[ -x "$K" ] || K="$ROOT/compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="$ROOT/compiler/stdlib"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
fail=0

# 1. Exact-case import compiles.
cat > "$TMP/ok.kry" <<'EOF'
use std::string::{split_lines}
fn main() { println(to_string(len(split_lines("a\nb")))) }
EOF
if ! "$K" run "$TMP/ok.kry" >/dev/null 2>&1; then
    echo "FAIL: exact-case import std::string rejected"; fail=1
fi

# 2. Wrong-case import is rejected (would break on a case-sensitive FS).
cat > "$TMP/bad.kry" <<'EOF'
use std::String::{split_lines}
fn main() { println(to_string(len(split_lines("a\nb")))) }
EOF
out="$("$K" run "$TMP/bad.kry" 2>&1)"
if ! echo "$out" | grep -qi "case-insensitive"; then
    echo "FAIL: wrong-case import std::String was NOT rejected (portability bug)"; fail=1
fi

# 3. audit renders sub-capabilities glued, not space-padded.
mkdir -p "$TMP/proj"
cat > "$TMP/proj/a.kry" <<'EOF'
@capabilities(fs:write, net:http)
fn main() { file_write("x.txt", "hi") }
EOF
aud="$("$K" audit "$TMP/proj" 2>&1)"
if echo "$aud" | grep -q "fs : write"; then
    echo "FAIL: audit still renders 'fs : write' (colon padding)"; fail=1
fi
if ! echo "$aud" | grep -q "fs:write"; then
    echo "FAIL: audit does not render glued 'fs:write'"; fail=1
fi

if [ "$fail" -eq 0 ]; then
    echo "module-case + audit-rendering gate: PASS"
fi
exit $fail
