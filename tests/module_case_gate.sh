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

# 4. A locally-declared type whose name COLLIDES with a real stdlib module
#    (e.g. `enum set { .. }` vs `std::set`) must still resolve its own
#    static-method calls (`set::make(..)`) -- the qualified-call-origin
#    validator used to treat ANY lowercase name matching a stdlib module
#    file stem as a module qualifier, rejecting a perfectly ordinary
#    program with a false "not imported" / wrong-origin error even though
#    it never imports std::set at all.
cat > "$TMP/collide.kry" <<'EOF'
enum set {
    Full(i64),
    Empty,
}

impl set {
    fn make(v: i64) -> set {
        return set::Full(v)
    }
}

fn main() {
    let s = set::make(5)
    match s {
        Full(v) => println(to_string(v)),
        Empty => println("empty"),
    }
}
EOF
out="$("$K" run "$TMP/collide.kry" 2>&1)"
if [ "$out" != "5" ]; then
    echo "FAIL: local type 'set' colliding with std::set misresolved its own static call: $out"; fail=1
fi

# 5. Regression guard: a GENUINE qualified-call misbinding must still be
#    caught after fix #4 -- the local-type carve-out must not swallow real
#    cross-module wrong-origin/not-imported errors.
cat > "$TMP/wrong_origin.kry" <<'EOF'
use std::csv::{parse}

fn main() {
    let x = json::parse("{}")
    println("done")
}
EOF
out="$("$K" check "$TMP/wrong_origin.kry" 2>&1)"
if ! echo "$out" | grep -q "E0201"; then
    echo "FAIL: genuine wrong-origin qualified call ('json::parse' bound via csv import) no longer caught"; fail=1
fi

cat > "$TMP/not_imported.kry" <<'EOF'
fn main() {
    let x = csv::parse("{}")
    println("done")
}
EOF
out="$("$K" check "$TMP/not_imported.kry" 2>&1)"
if ! echo "$out" | grep -q "E0202"; then
    echo "FAIL: genuine not-imported qualified call ('csv::parse' with no import) no longer caught"; fail=1
fi

if [ "$fail" -eq 0 ]; then
    echo "module-case + audit-rendering + qualified-call gate: PASS"
fi
exit $fail
