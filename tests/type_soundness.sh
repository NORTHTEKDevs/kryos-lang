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


# --- Pass-38: global reverse/sort are array-only, in-place, void-returning ---
# (reverse(str) dereferenced a KryosString as a KryosArray -> SEGFAULT;
# capturing the void return crashed on a garbage slot.)

want_reject reverse_str '
fn main() {
    let s = "hello"
    let r = reverse(s)
    println(r)
}
'

want_reject capture_sort_void '
fn main() {
    let a: [i64] = [3, 1, 2]
    let s = sort(a)
    println(to_string(s[0]))
}
'

want_pass reverse_sort_inplace '
fn main() {
    let a: [i64] = [3, 1, 2]
    sort(a)
    reverse(a)
    println(to_string(a[0]))
}
'


# --- Pass-38: a user function shadowing a builtin must WIN (JIT + AOT-runtime) ---
# (Cranelift math fast-paths for sin/abs/sqrt/... fired before the
# user-shadow guard, so `fn sin` was silently unreachable -- the builtin
# ran instead. Now the user body wins; only an AOT constant-arg call to a
# libm-NAMED fn is still const-folded, gotcha #18.)

want_pass user_shadows_math_builtin '
fn sin(x: f64) -> f64 { return 999.0 }
fn abs(n: i64) -> i64 { return 42 }
fn main() {
    let a: f64 = parse_float("0.5")
    if sin(a) != 999.0 { println("FAIL sin")  exit(1) }
    if abs(0 - 7) != 42 { println("FAIL abs")  exit(1) }
    println("ok")
}
'


# --- Pass-39: a user `fn len` shadowing the builtin must WIN on both backends ---
# (Cranelift unconditionally declared kryos_builtin_len as an import ->
# "Invalid to define identifier declared as an import" codegen crash; LLVM
# ran the builtin instead of the user body -- a JIT/AOT divergence. The
# self-host compiler defines its own top-level `fn len`.)

want_pass user_shadows_len '
fn len(x: [i64]) -> i64 { return 999 }
fn main() {
    let a: [i64] = [1, 2, 3]
    if len(a) != 999 { println("FAIL len shadow")  exit(1) }
    println("ok")
}
'


# --- Pass-41: tuple-destructuring arity must be type-checked ---
# (over-binding passed `check` then panicked at runtime leaking the internal
# array-OOB message; under-binding silently dropped trailing elements.)

want_reject tuple_over_bind '
fn main() {
    let t: (i64, str) = (1, "a")
    let (a, b, c) = t
    println(to_string(a) + b + to_string(c))
}
'

want_reject tuple_under_bind '
fn main() {
    let t: (i64, i64, i64) = (1, 2, 3)
    let (a, b) = t
    println(to_string(a) + "," + to_string(b))
}
'

want_pass tuple_exact_bind '
fn main() {
    let t: (i64, str, bool) = (1, "a", true)
    let (a, b, c) = t
    if a != 1 { exit(1) }
    println(b)
}
'

# --- Pass 42: enum patterns against concrete non-enum scrutinees -----------
# `?` desugars to a Result match; applying it to a plain-i64-returning call
# previously produced only a stray warning, ran on the JIT, and emitted
# invalid LLVM IR on the AOT path (extractvalue on a scalar).

want_reject qmark_on_plain_i64 '
use std::result::{Result, Ok, Err}
fn parse_and_double(s: str) -> Result<i64, str> {
    let v: i64 = parse_int(s)?
    return Ok(v * 2)
}
fn main() {
    match parse_and_double("21") {
        Ok(v) => println(to_string(v)),
        Err(e) => println(e),
    }
}
'

want_reject enum_pattern_on_scalar '
enum Color { Red, Green }
fn main() {
    let x = 3
    match x {
        Color.Red => println("r"),
        _ => println("other"),
    }
}
'

want_pass qmark_on_real_result '
use std::result::{Result, Ok, Err}
fn half(n: i64) -> Result<i64, str> {
    if n % 2 != 0 { return Err("odd") }
    return Ok(n / 2)
}
fn double_half(n: i64) -> Result<i64, str> {
    let h = half(n)?
    return Ok(h * 2)
}
fn main() {
    match double_half(10) {
        Ok(v) => println(to_string(v)),
        Err(e) => println(e),
    }
}
'

# --- Pass 45: duplicate struct fields must be rejected ----------------------
# Fields are stored positionally with no uniqueness check, so which of two
# same-named fields a literal or access resolved to was implementation-defined
# (silent wrong data on the classic copy-paste typo).

want_reject dup_field_in_decl '
struct Point { x: f64, y: f64, x: f64 }
fn main() {
    let p = Point { x: 1.0, y: 2.0, x: 3.0 }
    println(to_string(p.x))
}
'

want_reject dup_field_in_literal '
struct Point { x: f64, y: f64 }
fn main() {
    let p = Point { x: 1.0, x: 2.0, y: 3.0 }
    println(to_string(p.x))
}
'

want_pass distinct_fields_still_accepted '
struct Point { x: f64, y: f64 }
fn main() {
    let p = Point { x: 1.0, y: 2.0 }
    if p.x + p.y != 3.0 { exit(1) }
    println("ok")
}
'

# The rest of the duplicate-name class, swept in one batch: enum variants
# (positional tags -> ambiguous dispatch), fn/closure/method params (last
# duplicate silently won), generic params, same-impl-block methods (died as
# an internal codegen DuplicateDefinition dump), trait methods (second
# silently shadowed the first), and pattern bindings (`let (a, a)`,
# `P(x, x)` -- last binding silently won).

want_reject dup_enum_variant '
enum E { A(i64), B, A(str) }
fn main() {
    let x = E.A(42)
    match x { A(n) => println(to_string(n)), _ => println("other") }
}
'

want_reject dup_fn_param '
fn f(a: i64, a: i64) -> i64 { return a }
fn main() { println(to_string(f(1, 2))) }
'

want_reject dup_closure_param '
fn main() {
    let f = |x: i64, x: i64| x
    println(to_string(f(1, 2)))
}
'

want_reject dup_generic_param '
fn pick<T, T>(a: T, b: T) -> T { return a }
fn main() { println(to_string(pick(1, 2))) }
'

want_reject dup_method_same_impl '
struct S { v: i64 }
impl S {
    fn get(self: S) -> i64 { return 1 }
    fn get(self: S) -> i64 { return 2 }
}
fn main() { let s = S { v: 0 }  println(to_string(s.get())) }
'

want_reject dup_trait_method '
struct S { v: i64 }
trait Sized2 {
    fn size(self: S) -> i64
    fn size(self: S) -> i64
}
impl Sized2 for S {
    fn size(self: S) -> i64 { return 1 }
}
fn main() { let s = S { v: 0 }  println(to_string(s.size())) }
'

want_reject dup_tuple_destructure_binding '
fn main() {
    let (a, a) = (1, 2)
    println(to_string(a))
}
'

want_reject dup_match_pattern_binding '
enum M { P(i64, i64), Q }
fn main() {
    let m = M.P(3, 9)
    match m {
        P(x, x) => println(to_string(x)),
        _ => println("other"),
    }
}
'

# `_` is the discard placeholder -- multiple `_` params are deliberate
# "ignore both", not duplicates (found as a false-reject in review).
want_pass underscore_params_not_duplicates '
fn f(_: i64, _: i64) -> i64 { return 7 }
fn g(_: i64, x: i64) -> i64 { return x }
fn main() {
    if f(1, 2) != 7 { exit(1) }
    if g(100, 42) != 42 { exit(1) }
    let h = |_: i64, _: i64| 9
    if h(1, 2) != 9 { exit(1) }
    println("ok")
}
'

# Bare enum variants in an OR-pattern are tag tests, not bindings -- the
# non-binding rule must resolve idents against the subject enum instead of
# rejecting the documented-legal `Red | Green` form (its own error message
# cites it). Genuine ident/payload bindings must still reject.
want_pass bare_variant_or_pattern_discriminates '
enum C { Red, Green, Blue }
fn classify(c: C) -> str {
    return match c { Red | Green => "warm", Blue => "cool" }
}
fn main() {
    if classify(C.Red) != "warm" { exit(1) }
    if classify(C.Green) != "warm" { exit(1) }
    if classify(C.Blue) != "cool" { exit(1) }
    println("ok")
}
'

want_reject or_pattern_ident_binding '
fn main() {
    let n = 3
    match n { x | y => println(to_string(x)), }
}
'

want_reject or_pattern_payload_binding '
enum V { Num(i64), Label(str) }
fn main() {
    let v = V.Num(3)
    match v { Num(x) | Label(x) => println("bad"), }
}
'

# Bare enum variants in a tuple pattern are tag TESTS, not bindings -- the
# duplicate-binding check must NOT fire on them (guards the variant-test
# exclusion; this is the Pass-35 discriminate-fix shape).
want_pass bare_variant_tuple_pattern_not_a_dup '
enum Light { Red, Yellow, Green }
fn decide(a: Light, b: Light) -> str {
    return match (a, b) {
        (Red, Red) => "both stop",
        (Green, Green) => "both go",
        _ => "mixed",
    }
}
fn main() {
    if decide(Green, Green) != "both go" { exit(1) }
    if decide(Red, Red) != "both stop" { exit(1) }
    if decide(Yellow, Red) != "mixed" { exit(1) }
    println("ok")
}
'

if [ "$fail" -eq 0 ]; then
  echo "type-soundness: all probes correct (unsound rejected, correct accepted)"
else
  echo "type-soundness: $fail probe(s) FAILED"
  exit 1
fi
