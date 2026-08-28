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
  # Match EVERY double-free emission format the runtime has, not one:
  #   "KRYOS-FREE-DIAG ... DOUBLE-FREE ..."          (the rc-0 diag this gate was built for)
  #   "kryos_free: double free of 0x... ; ignored"   (the alloc.rs poison-guard, found
  #                                                   slipping past BOTH gates on 2026-08-17 --
  #                                                   the minilisp demo printed three of them
  #                                                   while every gate stayed green)
  if printf '%s' "$out" | grep -qiE "DOUBLE-FREE|double free"; then
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

# --- array/tuple LITERAL with a bare-identifier heap element: the element box
# was shared with the source local, so container teardown + the source's own
# scope drop both freed it. Retain the element so both drops balance. ---
no_df array_literal_nested \
'fn main() {
    let inner1: [str] = ["a1", "a2"]
    let inner2: [str] = ["b1"]
    let outer: [[str]] = [inner1, inner2]
    println(outer[0][0])
    println(inner1[0])
}'

no_df tuple_literal_arrays \
'fn main() {
    let a: [str] = ["x", "y"]
    let b: [str] = ["z"]
    let t = (a, b)
    println(t.0[0])
    println(a[1])
}'

no_df array_literal_str_idents \
'fn main() {
    let s1 = "hello" + "!"
    let s2 = "world" + "!"
    let arr = [s1, s2]
    println(arr[0])
    println(s1)
}'

# --- StringBuilder.build() called twice was a use-after-free of the freed
# buffer (segfault). build() is now idempotent (2nd call returns ""). ---
no_df stringbuilder_double_build \
'use std::string::{string_builder}
fn main() {
    let sb = string_builder()
    let sb2 = sb.append("one").append("-").append("two")
    println(sb2.build())
    println("[" + sb2.build() + "]")
}'

# --- a non-@copy struct with a fn field (not cleanly deep-clonable), copied
# 2+ times from the SAME source, aliased the shared value; each alias dropped
# its heap/fn fields -> arc release-after-free (exit 127). Now only the source
# owns/drops. ---
no_df struct_fnfield_two_copies \
'fn mk() -> str { return "x" }
struct Handler { route: str, fn_: fn() -> str }
fn main() {
    let h1 = Handler { route: "/a", fn_: mk }
    let h2 = h1
    let h3 = h1
    println(h1.route + h2.route + h3.route + h1.fn_())
}'

no_df struct_fnfield_closure_copies \
'struct Handler { route: str, fn_: fn() -> str }
fn main() {
    let base = "resp-"
    let h1 = Handler { route: "/a", fn_: || base + "a" }
    let h2 = h1
    let h3 = h1
    println(h1.fn_())
    println(h2.fn_())
    println(h3.fn_())
}'

# --- plain catch-variable use (was already clean; guard against regressions) ---
no_df catch_plain_println \
'fn throw_it() { throw "boom" }
fn main() { try { throw_it() } catch e { println(e) } }'

no_df catch_concat \
'fn throw_it() { throw "boom" }
fn main() { try { throw_it() } catch e { println("caught: " + e) } }'

# --- match arm binding the WHOLE heap subject to a name (`match s { v if .. }`).
# The binding aliased the subject handle with no retain and was then dropped at
# scope exit alongside the subject's real owner -- a double-free of any heap
# subject (str/array/map/struct). Guarded arms and a bare binding arm both
# route through the sequential path that created the binding. ---
no_df match_bind_str_subject \
'fn main() {
    let key = "k" + "1"
    let r = match key { v if len(v) > 0 => "hit", "k1" => "one", _ => "rest" }
    println(r + key)
}'

no_df match_bind_str_subject_loop \
'fn main() {
    let mut t = 0
    let mut i = 0
    while i < 40 {
        let key = "k" + to_string(i % 7)
        let r = match key { v if i % 2 == 0 => v + "-even", "k1" => "one", _ => "rest" }
        t = t + len(r)
        i = i + 1
    }
    println(to_string(t))
}'

no_df match_bind_array_subject \
'fn main() {
    let mut a = [1, 2]
    a = push(a, 3)
    let n = match a { v if len(v) > 0 => len(v), _ => 0 }
    println(to_string(n + len(a)))
}'

no_df match_bind_struct_subject \
'struct Rec { name: str, n: i64 }
fn main() {
    let r = Rec { name: "n" + "ame", n: 1 }
    let s = match r { v if v.n > 0 => v.name, _ => "none" }
    println(s + r.name)
}'

# --- a bare MUTABLE GLOBAL identifier returned directly (`return G`) aliased
# the global's own copy with no extra reference. Harmless as long as the
# global is never reassigned again, but the moment a LATER call resets it
# (`G = []`), the reassignment's guarded release frees the SAME box the
# earlier caller's return value still holds -- a real double-free (this is
# the root cause the self-host lexer's LEX_TOKENS worked around for years:
# tools/loop/LEDGER.md, "parse_nested_binop_corrupts_next"). ---
no_df global_return_alias \
'let mut G: [i64] = []
fn reset_and_build(seed: i64) -> [i64] {
    G = []
    let mut i = 0
    while i < seed {
        G = push(G, i)
        i = i + 1
    }
    return G
}
fn main() {
    let a = reset_and_build(3)
    let b = reset_and_build(5)
    println(to_string(len(a)) + "," + to_string(len(b)))
}'

# --- enum-typed FIELD of a struct rebuilt across an array-push loop, both
# via a fresh struct LITERAL copying a field off a shared array-element alias
# (`Task { priority: t.priority, .. }`) and via a bare re-push of the whole
# aliased struct (`push(dest, t)`). Cranelift had NO retain/deep-copy arm for
# a Struct/Enum-typed field in either RValue::Struct's non-@copy field-init
# match OR emit_struct_deep_copy_inner's (__kryos_struct_index_clone's) field
# match -- the enum box ended up shared between the source and the rebuilt
# array with zero compensating owner, freed once by each side's later drop.
# LEDGER "enum-in-struct array rebuild" wave. AOT was never affected (LLVM
# materializes an enum field as an inline aggregate, not a heap alias). ---
no_df enum_field_struct_rebuild \
'enum Priority { Low  Medium  High }
fn priority_label(p: Priority) -> str {
    match p { Priority::Low => "LOW"  Priority::Medium => "MED"  Priority::High => "HIGH" }
}
struct Task { id: i64  title: str  done: bool  priority: Priority }
fn main() {
    let tasks = [
        Task { id: 1, title: "a", done: false, priority: Priority::High },
        Task { id: 2, title: "b", done: false, priority: Priority::Low },
        Task { id: 3, title: "c", done: false, priority: Priority::Medium },
    ]
    let mut new_tasks: [Task] = []
    let mut i = 0
    while i < len(tasks) {
        let t = tasks[i]
        if t.id == 2 {
            new_tasks = push(new_tasks, Task { id: t.id, title: t.title, done: true, priority: t.priority })
        } else {
            new_tasks = push(new_tasks, t)
        }
        i = i + 1
    }
    let mut j = 0
    while j < len(new_tasks) {
        println(priority_label(new_tasks[j].priority))
        j = j + 1
    }
}'

# --- adjacent shape: TWO enum-typed fields on the same struct, both copied
# through the same rebuild-literal pattern. ---
no_df enum_field_struct_rebuild_two_fields \
'enum Priority { Low  Medium  High }
enum Status { Open  Closed }
fn priority_label(p: Priority) -> str {
    match p { Priority::Low => "LOW"  Priority::Medium => "MED"  Priority::High => "HIGH" }
}
fn status_label(s: Status) -> str {
    match s { Status::Open => "OPEN"  Status::Closed => "CLOSED" }
}
struct Task { id: i64  priority: Priority  status: Status }
fn main() {
    let tasks = [
        Task { id: 1, priority: Priority::High, status: Status::Open },
        Task { id: 2, priority: Priority::Low, status: Status::Closed },
    ]
    let mut new_tasks: [Task] = []
    let mut i = 0
    while i < len(tasks) {
        let t = tasks[i]
        new_tasks = push(new_tasks, Task { id: t.id, priority: t.priority, status: t.status })
        i = i + 1
    }
    let mut j = 0
    while j < len(new_tasks) {
        println(priority_label(new_tasks[j].priority) + " " + status_label(new_tasks[j].status))
        j = j + 1
    }
}'

# --- adjacent shape: a STRUCT field (not just Enum) holding an enum, nested
# two levels deep inside an array-rebuild via plain push (the whole-struct
# alias path, __kryos_struct_index_clone). ---
no_df nested_struct_enum_field_rebuild \
'enum Priority { Low  Medium  High }
struct Inner { priority: Priority }
struct Outer { inner: Inner }
fn priority_label(p: Priority) -> str {
    match p { Priority::Low => "LOW"  Priority::Medium => "MED"  Priority::High => "HIGH" }
}
fn main() {
    let outers = [
        Outer { inner: Inner { priority: Priority::High } },
        Outer { inner: Inner { priority: Priority::Low } },
    ]
    let mut rebuilt: [Outer] = []
    let mut i = 0
    while i < len(outers) {
        let o = outers[i]
        rebuilt = push(rebuilt, o)
        i = i + 1
    }
    let mut j = 0
    while j < len(rebuilt) {
        println(priority_label(rebuilt[j].inner.priority))
        j = j + 1
    }
}'

# --- adjacent shape: same rebuild, then the rebuilt array is read from
# inside a spawned thread -- the enum-field retain must hold across the
# spawn boundary too, not just the synchronous teardown path. ---
no_df enum_field_struct_rebuild_under_spawn \
'enum Priority { Low  Medium  High }
struct Task { id: i64  priority: Priority }
fn priority_label(p: Priority) -> str {
    match p { Priority::Low => "LOW"  Priority::Medium => "MED"  Priority::High => "HIGH" }
}
fn main() {
    let tasks = [
        Task { id: 1, priority: Priority::High },
        Task { id: 2, priority: Priority::Low },
    ]
    let mut new_tasks: [Task] = []
    let mut i = 0
    while i < len(tasks) {
        let t = tasks[i]
        new_tasks = push(new_tasks, Task { id: t.id, priority: t.priority })
        i = i + 1
    }
    spawn {
        let mut j = 0
        while j < len(new_tasks) {
            println(priority_label(new_tasks[j].priority))
            j = j + 1
        }
    }
    sleep(200)
}'

if [ "$fail" -eq 0 ]; then
  echo "no-double-free: all programs clean (no rc-0 frees)"
else
  echo "no-double-free: $fail program(s) double-freed"
  exit 1
fi
[ "$fail" -eq 0 ]
