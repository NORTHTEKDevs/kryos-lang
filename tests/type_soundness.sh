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

# --- actor request-response: no synchronous reply channel exists in the ----
# runtime (each actor runs on its own OS thread, kryos_actor_send just
# enqueues into a mailbox), so a handler declaring a non-void return must be
# rejected at the CALL site rather than silently threading back 0 -- or
# crashing for f64 (Cranelift verifier "entered unreachable code" / LLVM
# `add double 0, 0`). The handler BODY still type-checks against its declared
# return (declaring the actor alone must NOT fail); only calling it does.

want_reject actor_nonvoid_handler_call_rejected '
actor Calc {
    memory: i64
    fn add(x: i64) -> i64 {
        memory = memory + x
        return memory
    }
}
fn main() {
    let c = Calc()
    let r = c.add(5)
    println(to_string(r))
}
'

want_reject actor_nonvoid_handler_bare_call_rejected '
actor Calc {
    memory: f64
    fn add(x: f64) -> f64 {
        memory = memory + x
        return memory
    }
}
fn main() {
    let c = Calc()
    c.add(5.0)
    println("done")
}
'

want_pass actor_nonvoid_handler_body_typechecks_undeclared '
actor Calc {
    memory: i64
    fn add(x: i64) -> i64 {
        memory = memory + x
        return memory
    }
}
fn main() {
    let c = Calc()
    println("actor with non-void handler declared but not called: ok")
}
'

want_pass actor_void_handler_still_works '
actor Calc {
    memory: i64
    fn add(x: i64) {
        memory = memory + x
    }
}
fn main() {
    let c = Calc()
    c.add(5)
    println("void actor handler: ok")
}
'


# --- Pass-36/37: dyn Trait in a CONTAINER position must be rejected --------
# ([dyn T] / Option<dyn T> / (i64, dyn T) / map<K, dyn T> type-checked clean
# and then SEGFAULTED or hung at runtime on both backends; single dyn
# positions stay supported).

want_reject dyn_in_array '
trait DShape { fn area(self) -> i64 }
struct DSq { s: i64 }
impl DShape for DSq { fn area(self: DSq) -> i64 { return self.s * self.s } }
fn main() {
    let arr: [dyn DShape] = []
    println(len(arr))
}
'

want_reject dyn_in_option '
use std::option::{Option, Some, None}
trait DShape2 { fn area(self) -> i64 }
struct DSq2 { s: i64 }
impl DShape2 for DSq2 { fn area(self: DSq2) -> i64 { return self.s * self.s } }
fn main() {
    let o: Option<dyn DShape2> = Some(DSq2 { s: 3 })
    match o {
        Some(x) => println(x.area()),
        None() => println(-1),
    }
}
'

want_reject dyn_in_tuple '
trait DShape3 { fn area(self) -> i64 }
struct DSq3 { s: i64 }
impl DShape3 for DSq3 { fn area(self: DSq3) -> i64 { return self.s * self.s } }
fn main() {
    let t: (i64, dyn DShape3) = (1, DSq3 { s: 3 })
    println(t.1.area())
}
'

want_reject dyn_in_map '
trait DShape4 { fn area(self) -> i64 }
fn main() {
    let m: map<str, dyn DShape4> = {}
    println(len(m))
}
'

want_pass dyn_single_positions '
trait DShape5 { fn area(self) -> i64 }
struct DSq5 { s: i64 }
impl DShape5 for DSq5 { fn area(self: DSq5) -> i64 { return self.s * self.s } }
fn describe(s: dyn DShape5) -> i64 { return s.area() }
fn main() {
    let boxed: dyn DShape5 = DSq5 { s: 4 }
    println(to_string(describe(boxed) + describe(DSq5 { s: 2 })))
}
'


# --- Pass-37: duplicate top-level function names must be rejected ----------
# (kryos check passed them silently; codegen then died with a raw internal
# "Duplicate definition of identifier" dump.)

want_reject dup_local_fns '
fn helper(x: i64) -> i64 { return x }
fn helper(x: i64, y: i64) -> i64 { return x + y }
fn main() { println(to_string(helper(1))) }
'

want_reject dup_import_vs_local '
use std::math::{clamp}
fn clamp(x: f64, lo: f64, hi: f64) -> f64 { return x }
fn main() { println(to_string(clamp(15.0, 0.0, 10.0))) }
'

if [ "$fail" -eq 0 ]; then
  echo "type-soundness: all probes correct (unsound rejected, correct accepted)"
else
  echo "type-soundness: $fail probe(s) FAILED"
  exit 1
fi
