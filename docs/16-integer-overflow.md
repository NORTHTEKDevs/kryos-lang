# Integer Overflow Policy

This document defines Kryos' semantics for integer overflow and the
overflow-aware arithmetic builtins.

## Default behavior

All Kryos integer types — `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64` —
**wrap on overflow using two's-complement semantics**, both in debug and
release builds. This matches C, Rust release, Go, and Java.

```kryos
let imax: i64 = 9223372036854775807   // INT64_MAX
let x = imax + 1                       // = -9223372036854775808 (INT64_MIN)

let imin: i64 = -9223372036854775808   // INT64_MIN
let y = imin - 1                       // = 9223372036854775807  (INT64_MAX)

let m: i64 = 9223372036854775807
let z = m * 2                          // = -2
```

Overflow is never undefined behavior in Kryos. It is always a defined
2's-complement wrap. This makes Kryos a memory- *and* arithmetic-safe
language: a program cannot enter undefined-behavior land just because
some intermediate computation overflows.

## Why wrap by default?

- **No silent UB.** Some languages (C, C++ with signed overflow) say
  overflow is undefined and the compiler is free to assume it can't
  happen, which leads to surprising miscompilations. Kryos refuses to
  do that.
- **Performance.** Trap-on-overflow checks at every arithmetic op cost
  ~10–20% in numeric kernels in current benchmarks. Wrap is free.
- **Predictability.** "Always wrap" is the easiest rule to teach and the
  easiest to reason about when the program does need overflow (hashing,
  PRNGs, bit twiddling, fixed-point math).
- **Migration.** Most existing code from C, Rust, Go, and Java already
  expects wrap-on-overflow under release builds.

## Explicit overflow-aware operations

When the default isn't what you want, use the explicit overflow
builtins. All operate on `i64`:

| Name                | Behavior on overflow                              |
| ------------------- | ------------------------------------------------- |
| `wrapping_add(a,b)` | Wrap (same as `a + b`).                            |
| `wrapping_sub(a,b)` | Wrap (same as `a - b`).                            |
| `wrapping_mul(a,b)` | Wrap (same as `a * b`).                            |
| `checked_add(a,b)`  | **Panic** with `kryos panic: integer overflow…`. |
| `checked_sub(a,b)`  | **Panic** with `kryos panic: integer overflow…`. |
| `checked_mul(a,b)`  | **Panic** with `kryos panic: integer overflow…`. |
| `saturating_add`    | Clamp to `INT64_MIN..=INT64_MAX`.                  |
| `saturating_sub`    | Clamp to `INT64_MIN..=INT64_MAX`.                  |
| `saturating_mul`    | Clamp to `INT64_MIN..=INT64_MAX`.                  |

Examples:

```kryos
let imax: i64 = 9223372036854775807

// Default: wrap
let a = imax + 1                   // -9223372036854775808

// Explicit wrap (same value, but documents intent)
let b = wrapping_add(imax, 1)      // -9223372036854775808

// Saturate to INT64_MAX
let c = saturating_add(imax, 1)    // 9223372036854775807

// Panic with stack trace
let d = checked_add(imax, 1)
// kryos panic: integer overflow in checked_add
// stack trace (most recent call last):
//   0: main() at file.kry:N
```

## Division and remainder

Integer division and remainder still trap when the divisor is zero —
this is a real bug, not an arithmetic edge case, and we panic with a
stack trace:

```kryos
let x = 10 / 0
// kryos panic: integer division by zero
// stack trace (most recent call last):
//   0: main() at file.kry:N
```

Integer division/remainder of `INT64_MIN / -1` currently wraps to
`INT64_MIN` (no overflow trap). Use `checked_div` (planned) if you need
the panic.

## Float overflow

IEEE-754 floats (`f32`, `f64`) follow IEEE rules: overflow produces
`+inf` / `-inf`, divide-by-zero produces `inf` / `nan`. No panic.

## Roadmap

- `checked_div`, `wrapping_div`, `saturating_div` (and `_rem`).
- Generic-over-width variants so the same name works for `i32`, `i16`, etc.
- Compile-time-detectable overflow on constant expressions becomes an
  error rather than a silent wrap.
