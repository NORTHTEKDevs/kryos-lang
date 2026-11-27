#!/usr/bin/env bash
# asi_nonw0001_pin.sh -- ASI-nonW0001-tokens: a newline-led `+`, `*` or single
# `|` written at the SAME indent as the statement it silently continues must
# warn W0001 (3 warnings for the first program). An indented continuation
# (multi-line concat / bit-packing chain) and a top-level const initializer
# must stay silent. On master the first program printed `6 7 14` with no
# warning at all.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
K="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos.exe}"
[ -x "$K" ] || K="$ROOT/compiler/target/release/kryos"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
W="$(mktemp -d)"
trap 'rm -rf "$W"' EXIT

cat > "$W/trap.kry" <<'K'
fn main() {
    let a: i64 = 5
    + 1
    let b: i64 = 6
    | 1
    let c: i64 = 7
    * 2
    println(to_string(a) + " " + to_string(b) + " " + to_string(c))
}
K
cat > "$W/quiet.kry" <<'K'
const K: i64 = 1
    | 2
fn main() {
    let a: i64 = 1
        + 2
        + 3
    let b: i64 = 1
        | 2
        | 4
    let c: i64 = 2
        * 3
    let s = "x"
        + "y"
    println(to_string(a + b + c + K) + s)
}
K
bad=0
n="$(timeout 60 "$K" check "$W/trap.kry" 2>&1 | grep -c 'W0001')"
# each warning prints its code on one header line
[ "$n" = 3 ] || { echo "FAIL trap.kry: want 3 W0001, got $n"; bad=1; }
q="$(timeout 60 "$K" check "$W/quiet.kry" 2>&1 | grep -c 'W0001')"
[ "$q" = 0 ] || { echo "FAIL quiet.kry: want 0 W0001, got $q"; bad=1; }
[ "$bad" = 0 ] && echo "PASS asi_nonw0001_pin (trap=3 quiet=0)"
exit "$bad"
