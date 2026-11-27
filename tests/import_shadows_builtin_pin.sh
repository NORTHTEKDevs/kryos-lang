#!/usr/bin/env bash
# import_shadows_builtin_pin.sh -- import-shadows-builtin: a function that
# shares a builtin's name (`use std::trie::{contains}`, or a user `fn contains`)
# must not capture the builtin calls INSIDE another imported module. On master
# `use std::trie::{contains}` + `use std::os::{name}` failed with
# E0100 "expected [i64], found str" at a line past the end of the user's file
# (the span belonged to std::os's own `contains(ostype, "linux")`).
# Each program must run; the user-facing `contains` must still be the user's.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos.exe}"
[ -x "$K" ] || K="$ROOT/compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
W="$(mktemp -d)"
trap 'rm -rf "$W"' EXIT
bad=0

run() { # name, expected stdout
  local out rc
  out="$(timeout 60 "$K" run --capabilities-mode=permissive "$W/$1.kry" 2>&1)"; rc=$?
  if [ "$rc" != 0 ] || [ "$out" != "$2" ]; then
    echo "FAIL $1: rc=$rc want [$2] got [$(printf '%s' "$out" | head -c 200 | tr '\n' '|')]"
    bad=1
  fi
}

cat > "$W/sel.kry" <<'K'
use std::trie::{new_trie, insert, contains}
use std::os::{name}
fn main() {
    let t = insert(new_trie(), "abc")
    println(contains(t, "abc"))
    println(contains(t, "abd"))
    println(len(name()) > 0)
}
K
run sel $'true\nfalse\ntrue'

cat > "$W/qual.kry" <<'K'
use std::trie
use std::os::{name}
fn main() {
    let t = trie::insert(trie::new_trie(), "abc")
    println(trie::contains(t, "abc"))
    println(len(name()) > 0)
}
K
run qual $'true\ntrue'

cat > "$W/local.kry" <<'K'
use std::os::{name}
fn contains(t: [i64], word: str) -> bool { return len(t) == 7 }
fn main() {
    println(len(name()) > 0)
    println(contains([1, 2, 3], "x"))
    println(contains([1, 2, 3, 4, 5, 6, 7], "x"))
}
K
run local $'true\nfalse\ntrue'

# control: no clash, the stdlib's own builtin call is untouched
cat > "$W/ctl.kry" <<'K'
use std::os::{name}
fn main() {
    println(contains("hello world", "wor"))
    println(len(name()) > 0)
}
K
run ctl $'true\ntrue'

[ "$bad" = 0 ] && echo "PASS import_shadows_builtin_pin (sel qual local ctl)"
exit "$bad"
