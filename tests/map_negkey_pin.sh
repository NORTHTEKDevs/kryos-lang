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
ok negid 'fn main() {
    let a: i64 = 4
    let m: map<i64,i64> = {-a: 1}
    println("{m[-4]} {len(m)}")
}' '1 1'
ok negexpr 'fn main() {
    let a: i64 = 4
    let m: map<i64,i64> = {-a - 1: 1, 7: 2}
    println("{m[-5]} {len(m)}")
}' '1 2'
ok parenop 'fn main() {
    let a: i64 = 4
    let m: map<i64,i64> = {(a) - 1: 7}
    println("{m[3]} {len(m)}")
}' '7 1'
ok parenop2 'fn main() {
    let a: i64 = 4
    let b: i64 = 2
    let m: map<i64,string> = {(a * b) - 1: "z"}
    println("{m[7]}")
}' 'z'
ok addkey 'fn main() {
    let a: i64 = 4
    let m: map<i64,i64> = {a + 1: 7}
    println("{m[5]}")
}' '7'
ok callkey 'fn f(x: i64) -> i64 { return x * 2 }
fn main() {
    let a: i64 = 4
    let m: map<i64,i64> = {f(a): 7}
    println("{m[8]}")
}' '7'
ok blk_stmt 'fn main() {
    let a: i64 = 4
    let v: i64 = {
        a + 1
        a * 2
    }
    println("{v}")
}' '8'
ok blk_lambda 'fn main() {
    let v: i64 = {
        let g = |y: i64| y + 1
        g(2)
    }
    println("{v}")
}' '3'
ok charkey 'fn main() {
    let m: map<char,i64> = {'"'a'"': 7}
    println("{m['"'a'"']}")
}' '7'
ok floatkey 'fn main() {
    let f: map<f64,i64> = {-1.5: 7}
    println("{f[-1.5]}")
}' '7'
ok interpkey 'fn main() {
    let a: i64 = 4
    let m8: map<string,i64> = {"k{a}": 3}
    println("{m8["k4"]}")
}' '3'
ok interpkey2 'fn main() {
    let a: i64 = 4
    let m8: map<string,i64> = {"k{a}": 3, "z": 1}
    println("{m8["k4"]} {m8["z"]}")
}' '3 1'
ok interponly 'fn main() {
    let a: i64 = 4
    let m8: map<string,i64> = {"{a}": 3}
    println("{m8["4"]}")
}' '3'
ok derefkey 'fn main() {
    let a: i64 = 4
    let p = &a
    let m: map<i64,i64> = {*p: 3}
    println("{m[4]}")
}' '3'
ok ifkey 'fn main() {
    let a: i64 = 4
    let m: map<i64,i64> = {if a > 1 { 1 } else { 2 }: 3}
    println("{m[1]}")
}' '3'
ok nlkey 'fn main() {
    let a: i64 = 4
    let m: map<i64,i64> = {a
        + 1: 7}
    println("{m[5]}")
}' '7'
ok blk_if 'fn main() {
    let a: i64 = 4
    let v: i64 = { if a > 1 { 10 } else { 20 } }
    println("{v}")
}' '10'
ok blk_deref 'fn main() {
    let a: i64 = 4
    let p = &a
    let v: i64 = { *p }
    println("{v}")
}' '4'
[ "$bad" = 0 ] && echo "PASS map_negkey_pin (neg paren negval blk_neg blk_paren negid negexpr parenop parenop2 addkey callkey blk_stmt blk_lambda charkey floatkey interpkey interpkey2 interponly derefkey ifkey nlkey blk_if blk_deref)"
exit $bad
