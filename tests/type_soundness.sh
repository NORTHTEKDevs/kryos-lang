#!/usr/bin/env bash
# type_soundness.sh -- regression gate for type-system soundness holes that
# previously type-checked clean and then segfaulted (or died at link time)
# at runtime. Encodes audit backlog #13/#19 (Result/Option payload bridge),
# #18/#34 (dyn Trait coercion without an impl), and #89 (unchecked generic
# trait bounds). If a refactor reopens one of these, this gate turns red.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
export KRYOS_STDLIB_DIR="${KRYOS_STDLIB_DIR:-$ROOT/compiler/stdlib}"
KRYOS="${KRYOS_BIN:-$ROOT/compiler/target/release/kryos}"
[ -x "$KRYOS" ] || KRYOS="$ROOT/compiler/target/release/kryos.exe"

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
fail=0

# want_reject <name> <source> : unsound program MUST fail `kryos check`.
want_reject() {
  local name="$1" src="$2" f="$TMP/$1.kry"
  printf '%s' "$src" > "$f"
  if timeout 30 "$KRYOS" check "$f" >/dev/null 2>&1; then
    echo "  HOLE  $name -- unsound program passed kryos check"
    fail=$((fail+1))
  fi
}

# want_pass <name> <source> : correct program MUST be accepted AND run clean.
want_pass() {
  local name="$1" src="$2" f="$TMP/$1.kry"
  printf '%s' "$src" > "$f"
  if ! timeout 60 "$KRYOS" run "$f" >/dev/null 2>&1; then
    echo "  BREAK $name -- correct program rejected or crashed"
    fail=$((fail+1))
  fi
}

# --- #13/#19: Result/Option annotated payloads must be checked -------------

want_reject result_err_payload '
use std::result::{Result, Ok, Err}
fn get(fail: bool) -> Result<i64, str> {
    if fail { return Err(42) }
    return Ok(1)
}
fn main() {
    match get(true) {
        Ok(v) => println("ok " + to_string(v)),
        Err(e) => println("err " + e),
    }
}
'

want_reject result_ok_payload '
use std::result::{Result, Ok, Err}
fn get() -> Result<str, str> {
    return Ok(7)
}
fn main() {
    match get() {
        Ok(v) => println("ok " + v),
        Err(e) => println("err " + e),
    }
}
'

want_reject option_some_payload '
use std::option::{Option, Some, None}
fn get(x: bool) -> Option<str> {
    if x { return Some(999) }
    return None()
}
fn main() {
    match get(true) {
        Some(v) => println("got: " + v),
        None() => println("none"),
    }
}
'

want_pass result_correct '
use std::result::{Result, Ok, Err}
fn get(fail: bool) -> Result<i64, str> {
    if fail { return Err("bad input") }
    return Ok(1)
}
fn main() {
    match get(true) {
        Ok(v) => println("ok " + to_string(v)),
        Err(e) => println("err " + e),
    }
}
'

want_pass option_correct '
use std::option::{Option, Some, None}
fn get(x: bool) -> Option<str> {
    if x { return Some("hello") }
    return None()
}
fn main() {
    match get(true) {
        Some(v) => println("got: " + v),
        None() => println("none"),
    }
}
'

# --- #18/#34: dyn Trait coercion requires an impl ---------------------------

want_reject dyn_without_impl '
trait Speaker {
    fn speak(self) -> str
}
struct Rock { weight: i64 }
fn make_noise(s: dyn Speaker) -> str {
    return s.speak()
}
fn main() {
    let r = Rock { weight: 5 }
    println(make_noise(r))
}
'

# --- #89: generic trait bounds enforced at the call site --------------------

want_reject bound_without_impl '
trait Speaker {
    fn speak(self) -> str
}
struct Rock { weight: i64 }
fn noisy<T: Speaker>(x: T) -> str {
    return x.speak()
}
fn main() {
    let r = Rock { weight: 5 }
    println(noisy(r))
}
'

want_pass dyn_and_bound_with_impl '
trait Speaker {
    fn speak(self) -> str
}
struct Dog { name: str }
impl Speaker for Dog {
    fn speak(self) -> str { return "woof" }
}
fn make_noise(s: dyn Speaker) -> str {
    return s.speak()
}
fn noisy<T: Speaker>(x: T) -> str {
    return x.speak()
}
fn main() {
    let d = Dog { name: "rex" }
    println(make_noise(d))
    let d2 = Dog { name: "max" }
    println(noisy(d2))
}
'

if [ "$fail" -eq 0 ]; then
  echo "type-soundness: all probes correct (unsound rejected, correct accepted)"
else
  echo "type-soundness: $fail probe(s) FAILED"
  exit 1
fi
