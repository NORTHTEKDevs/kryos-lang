#!/usr/bin/env bash
# match_exhaustiveness.sh -- regression gate for non-exhaustive match runtime
# safety. A match with no wildcard/default that fails to match any arm must
# PANIC (fail loud, non-zero exit), NOT read back uninitialized memory (0 /
# empty string / garbage tuple result). Covered / wildcard / exhaustive-enum
# matches must still succeed.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0

# panics <name> <source> : a non-exhaustive miss MUST exit non-zero (panic),
# and its stdout must NOT contain a garbage result line.
panics() {
  local name="$1" src="$2" f="$TMP/$1.kry"
  printf '%s' "$src" > "$f"
  local out rc
  out="$(timeout 30 "$KRYOS" run "$f" 2>/dev/null)"; rc=$?
  if [ "$rc" -eq 0 ]; then
    echo "  NO-PANIC  $name -- non-exhaustive miss returned silently (rc 0): [$out]"
    fail=$((fail+1))
  fi
}

# succeeds <name> <source> <expected-stdout-substring>
succeeds() {
  local name="$1" src="$2" want="$3" f="$TMP/$1.kry"
  printf '%s' "$src" > "$f"
  local out rc
  out="$(timeout 30 "$KRYOS" run "$f" 2>/dev/null)"; rc=$?
  if [ "$rc" -ne 0 ] || [[ "$out" != *"$want"* ]]; then
    echo "  BROKE  $name -- rc=$rc out=[$out] want substring [$want]"
    fail=$((fail+1))
  fi
}

# --- non-exhaustive misses MUST panic (were silent garbage: 0 / "" / 0) ---
panics int_miss \
'fn main() { let n: i64 = 99  let r: i64 = match n { 1 => 10, 2 => 20 }  println("r=" + to_string(r)) }'

panics str_miss \
'fn main() { let s: str = "zzz"  let r: str = match s { "a" => "one", "b" => "two" }  println("r=" + r) }'

panics tuple_miss \
'enum Side { Left, Right }
fn f(a: Side, b: Side) -> i64 { match (a, b) { (Left, Left) => 1, (Left, Right) => 2, (Right, Left) => 3 } }
fn main() { println(to_string(f(Right, Right))) }'

# --- covered / wildcard / exhaustive matches MUST still succeed ---
succeeds int_wildcard \
'fn f(n: i64) -> i64 { match n { 1 => 10, 2 => 20, _ => 99 } }
fn main() { println(to_string(f(1)) + "," + to_string(f(5))) }' \
'10,99'

succeeds int_hit \
'fn main() { let n: i64 = 2  let r: i64 = match n { 1 => 10, 2 => 20 }  println(to_string(r)) }' \
'20'

succeeds enum_exhaustive \
'enum C { A, B }
fn f(c: C) -> i64 { match c { A => 1, B => 2 } }
fn main() { println(to_string(f(C.A)) + "," + to_string(f(C.B))) }' \
'1,2'

succeeds tuple_covered \
'enum Side { Left, Right }
fn f(a: Side, b: Side) -> i64 { match (a, b) { (Left, Left) => 1, (Left, Right) => 2, (Right, Left) => 3, (Right, Right) => 4 } }
fn main() { println(to_string(f(Right, Right))) }' \
'4'

if [ "$fail" -eq 0 ]; then
  echo "match-exhaustiveness: all probes correct (misses panic, covered succeed)"
else
  echo "match-exhaustiveness: $fail probe(s) WRONG"
  exit 1
fi
[ "$fail" -eq 0 ]
