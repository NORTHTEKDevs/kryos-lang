#!/usr/bin/env bash
# no_double_free.sh -- regression gate for reference-counting double-frees that
# corrupt the heap but print correct output (so an output-only conformance test
# misses them). Each program is run with KRYOS_FREE_DIAG=1, which prints a
# `KRYOS-FREE-DIAG ... DOUBLE-FREE` line when a buffer is freed at rc 0. Any
# such line fails the gate.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0

# no_df <name> <source> : the program must run with NO double-free diagnostic.
no_df() {
  local name="$1" src="$2" f="$TMP/$1.kry"
  printf '%s' "$src" > "$f"
  local out
  out="$(KRYOS_FREE_DIAG=1 timeout 30 "$KRYOS" run "$f" 2>&1)"
  if printf '%s' "$out" | grep -q "DOUBLE-FREE"; then
    echo "  DOUBLE-FREE  $name"
    fail=$((fail+1))
  fi
}

# --- to_string(str) on an rc=1 string: the result aliases the argument, and
# both are dropped. Was a double-free of any freshly computed / returned /
# caught-exception string (a literal's rc masked it). ---
no_df tostring_fresh_str \
'fn mk() -> str { return "xx" + "yy" }
fn main() { let e = mk()  println(to_string(e)) }'

no_df tostring_inline_concat \
'fn main() { let a = "xx"  let e = a + "yy"  println(to_string(e)) }'

no_df tostring_literal \
'fn main() { let e = "boom"  println(to_string(e)) }'

no_df tostring_caught_exception \
'fn throw_it() { throw "boom" }
fn main() { try { throw_it() } catch e { println(to_string(e)) } }'

no_df tostring_loop \
'fn main() {
    let mut i = 0
    while i < 500 {
        let s = "v" + to_string(i)
        println(to_string(s))
        i = i + 1
    }
}'

# --- plain catch-variable use (was already clean; guard against regressions) ---
no_df catch_plain_println \
'fn throw_it() { throw "boom" }
fn main() { try { throw_it() } catch e { println(e) } }'

no_df catch_concat \
'fn throw_it() { throw "boom" }
fn main() { try { throw_it() } catch e { println("caught: " + e) } }'

if [ "$fail" -eq 0 ]; then
  echo "no-double-free: all programs clean (no rc-0 frees)"
else
  echo "no-double-free: $fail program(s) double-freed"
  exit 1
fi
[ "$fail" -eq 0 ]
