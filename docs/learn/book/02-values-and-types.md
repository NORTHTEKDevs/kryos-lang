# 02 · Values & Types

After this chapter you will know every primitive type Kryos has, which one
you get when you don't ask for a specific width, how the compiler treats
integer overflow, how to convert between types with `as`, and how to write a
function that might not have a value to return without reaching for `null`.

## The primitive types

```kryos
fn main() {
    let n: i64 = -10
    let u: u64 = 10
    let x: f64 = 3.14
    let flag: bool = true
    let s: str = "kryos"

    println(to_string(n) + " " + to_string(u))
    println(to_string(x))
    println(to_string(flag))
    println(s)
}
```

Output:

```
-10 10
3.14
true
kryos
```

| Type | Width | Notes |
|---|---|---|
| `i8`, `i16`, `i32`, `i64` | 8/16/32/64 bits | signed, two's complement |
| `u8`, `u16`, `u32`, `u64` | 8/16/32/64 bits | unsigned |
| `f32`, `f64` | 32/64 bits | IEEE 754 |
| `bool` | -- | `true` / `false` only, no truthy coercion |
| `str` | heap | UTF-8, reference-counted (the full sharing model is [`docs/06-ownership.md`](../../06-ownership.md), a later chapter of this book) |

## Numeric defaults

Write a numeric literal with no annotation and you get the two widest types:
an integer literal defaults to `i64`, a literal with a decimal point or
exponent defaults to `f64`. `type_of` shows you the type the compiler
actually inferred:

```kryos
fn main() {
    let count = 42
    let ratio = 3.14
    println(type_of(count))
    println(type_of(ratio))
}
```

Output:

```
i64
f64
```

This matters because it decides what you get for free. Assigning between
integer types of the **same signedness** widens implicitly -- an `i32` flows
into an `i64` slot with no cast:

```kryos
fn main() {
    let small: i32 = 5
    let wide: i64 = small
    println(to_string(wide))
}
```

Output:

```
5
```

**Crossing signedness, or going from integer to float, is never implicit** --
you need `as`, covered next.

## Casting with `as`

```kryos
fn main() {
    let byte: u8 = 200
    let signed: i16 = byte as i16
    println(to_string(signed))

    let n: i64 = 5
    let f: f64 = n as f64
    println(to_string(f))
}
```

Output:

```
200
5
```

`as` also handles float-to-integer, and it is worth knowing the two rules by
heart because neither one is undefined behavior the way an out-of-range cast
would be in C:

```kryos
fn main() {
    let a: f64 = 3.9
    println(to_string(a as i64))     // truncates toward zero: 3

    let b: f64 = -3.9
    println(to_string(b as i64))     // truncates toward zero: -3

    let huge: f64 = 1.0e300
    println(to_string(huge as i64))  // out of range: saturates to i64::MAX
}
```

Output:

```
3
-3
9223372036854775807
```

Float-to-integer **truncates toward zero** (not "toward negative infinity" --
`-3.9 as i64` is `-3`, not `-4`), and an out-of-range or NaN source value
**saturates** to the target type's min/max instead of wrapping or trapping.
`bool` casts to `0`/`1` the same way; there is no cast in the other direction
because Kryos has no truthy coercion (see "Common mistakes" below).

## Integer overflow wraps

There is no compile-time or runtime overflow check on the default arithmetic
operators. `+`, `-`, and `*` wrap modulo 2^64 for `i64`, the same as C's
unsigned semantics or Rust's release-mode behavior:

```kryos
fn main() {
    let max: i64 = 9223372036854775807
    let wrapped: i64 = max + 1
    println(to_string(wrapped))
}
```

Output:

```
-9223372036854775808
```

`max + 1` doesn't panic and doesn't get clamped -- it wraps to `i64::MIN`.
If you need overflow to be visible, the runtime ships `checked_add`/
`checked_sub`/`checked_mul` (panic on overflow) and `saturating_add`/`_sub`/
`_mul` (clamp instead of wrap); see
[`docs/16-integer-overflow.md`](../../16-integer-overflow.md) for the full
builtin table and the rationale for wrapping-by-default.

## No null: `Option<T>`

Kryos has no `null`/`nil`. A value that might be absent is `Option<T>` from
`std::option`: `Some(x)` when there's a value, `None()` when there isn't.

```kryos
use std::option::{Some, None}

fn find_first_even(nums: [i64]) -> Option<i64> {
    for n in nums {
        if n % 2 == 0 {
            return Some(n)
        }
    }
    return None()
}

fn main() {
    let nums: [i64] = [1, 3, 5, 8, 9]
    match find_first_even(nums) {
        Some(n) => println("first even: " + to_string(n)),
        None()  => println("no even number found"),
    }

    let odds: [i64] = [1, 3, 5]
    match find_first_even(odds) {
        Some(n) => println("first even: " + to_string(n)),
        None()  => println("no even number found"),
    }
}
```

Output:

```
first even: 8
no even number found
```

The compiler forces you to handle both cases -- there is no way to read the
`i64` out of an `Option<i64>` without going through `match` (or a helper like
`unwrap_or` built on top of it), so "I forgot to check for absence" stops
being a category of bug. [Chapter 5](05-control-flow.md) covers `if let`/
`while let`, the shorthand for matching a single `Some`/`None` case without
writing out the full `match`.

**Always write the `<T>`.** A bare `Option` (or `Result`) on a function
signature erases the payload to a raw `i64` slot -- annotate the full generic
form (`Option<i64>`, `Option<str>`, ...) every time it appears in a signature,
never the bare type name.

## Annotations: required vs inferred

A local `let` inside a function body infers its type from the initializer --
every example above with `let count = 42` (no `: i64`) relies on this.
**Function parameters and a top-level `let` (outside any function) must be
annotated explicitly.** You've already seen why parameters need it: nothing
about `fn add(a, b)` tells the checker what `+` should mean for `a` and `b`.
[Chapter 3](03-bindings.md) covers top-level `let` in depth, including the
narrower rule for *what* a top-level initializer is allowed to call.

## Common mistakes

**No truthy coercion.** `0`, `""`, and an empty array are not `false` --
`if` requires a real `bool`:

```kryos
fn main() {
    let n: i64 = 0
    if n {          // ERROR: if requires bool, not i64
        println("truthy")
    }
}
```

```
error[E0100]: type mismatch: expected `bool`, found `i64`
 --> mistake.kry:3:5
  3 |     if n {
    |     ^^^^^^ expected type `bool`, found `i64`
  = note: Kryos does not implicitly convert between bool and other types
```

Write the comparison out: `if n != 0 { ... }`.

**Forgetting `as` across signedness or int/float.** Same-signedness widening
(`i32` into `i64`) is free, but everything else needs an explicit cast:

```kryos
fn main() {
    let n: i64 = 5
    let f: f64 = n   // ERROR: int -> float needs `as`
    println(to_string(f))
}
```

```
error[E0100]: type mismatch: expected `f64`, found `i64`
 --> mistake.kry:3:5
  3 |     let f: f64 = n
    |     ^^^^^^^^^^^^^^ expected type `f64`, found `i64`
```

Fix: `let f: f64 = n as f64`.

## Exercises

1. Write a function `average(nums: [i64]) -> f64` that sums an array of
   `i64` and divides by its length, casting where needed. Run it on
   `[1, 2, 3, 4]` and confirm you get `2.5`.
2. Predict the output of `(-1.5) as i64` and `1e400 as i64` before running
   them. Were you right about which one saturates and which one truncates?
3. Write a function `safe_div(a: i64, b: i64) -> Option<i64>` that returns
   `None()` when `b` is `0` and `Some(a / b)` otherwise. Call it once with
   `b = 0` and once with a real divisor, and `match` both results.

## Summary

- Integer literals default to `i64`, float literals to `f64` -- the two
  widest types, so you opt into a narrower one rather than opting out of it.
- Same-signedness integer widening (`i32` -> `i64`) is implicit; crossing
  signedness or converting int <-> float needs `as`.
- `as` float-to-int truncates toward zero; an out-of-range or NaN source
  saturates to the target's min/max rather than being undefined.
- Default `+`/`-`/`*` wrap modulo 2^64 on overflow, silently -- use
  `checked_*`/`saturating_*` when you need overflow to be visible.
- There is no `null`. Absence is `Option<T>` (`Some(x)` / `None()`), and a
  signature must spell out the `<T>` or the payload erases to `i64`.
- Local `let` infers its type; function params and top-level `let` must be
  annotated.

Next: [Bindings](03-bindings.md)
