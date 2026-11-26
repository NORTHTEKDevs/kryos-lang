#!/usr/bin/env bash
# map_negkey_pin.sh -- M4: a map literal whose FIRST key is a negative number
# (`{-3: 1}`) or a parenthesised expression (`{(0-3): 1}`) was parsed as a block
# and failed with E0003 "unexpected token ':'". Block expressions that merely
# start with `-` or `(` must stay blocks.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos.exe}"
[ -x "$K" ] || K="$ROOT/compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
W="$(mktemp -d)"
trap 'rm -rf "$W"' EXIT
bad=0

ok() { # name, source, expected stdout
  local out rc
  printf '%s\n' "$2" > "$W/$1.kry"
  out="$(cd "$W" && timeout 60 "$K" run "$1.kry" 2>&1)"; rc=$?
  if [ "$rc" != 0 ] || [ "$out" != "$3" ]; then
    echo "FAIL $1: rc=$rc want [$3] got [$(printf '%s' "$out" | head -c 200 | tr '\n' '|')]"
    bad=1
  fi
}

ok neg 'fn main() {
    let m: map<i64,i64> = {-3: 1, 4: 2}
    println("{m[-3]} {len(m)}")
}' '1 2'
ok paren 'fn main() {
    let m: map<i64,i64> = {(0-3): 1, 4: 2}
    println("{m[0-3]} {len(m)}")
}' '1 2'
ok negval 'fn main() {
    let n: map<i64,i64> = {3: -1}
    println("{n[3]}")
}' '-1'
ok blk_neg 'fn main() {
    let v: i64 = { -3 }
    println("{v}")
}' '-3'
ok blk_paren 'fn main() {
    let v: i64 = { (1 + 2) * 3 }
    println("{v}")
}' '9'
[ "$bad" = 0 ] && echo "PASS map_negkey_pin (neg paren negval blk_neg blk_paren)"
exit $bad
