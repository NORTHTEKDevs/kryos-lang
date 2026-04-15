# std::math

Extended math functions and constants. These complement the core builtins (`abs`, `sqrt`, `sin`, `cos`, `tan`, `log`, `log10`, `pow`, `floor`, `ceil`, `round`, `min`, `max`, `random`, `pi`, `e`) with a full-precision floating-point library.

All functions in `std::math` operate on `f64`. Integer-specific functions (`gcd`, `lcm`, `factorial`) take and return `i64`.

```kryos
use std::math
```

---

## Constants

These are module-level `let` bindings, not functions. Access them directly by name after importing the module.

| Constant  | Value                     | Description                   |
|-----------|---------------------------|-------------------------------|
| `PI`      | `3.141592653589793`        | Pi                            |
| `E`       | `2.718281828459045`        | Euler's number                |
| `TAU`     | `6.283185307179586`        | 2*pi (full circle in radians) |
| `INF`     | `+Infinity`               | Positive infinity             |
| `NEG_INF` | `-Infinity`               | Negative infinity             |
| `NAN`     | `NaN`                     | Not-a-number                  |

**Example:**
```kryos
use std::math

println(PI)        // 3.141592653589793
println(TAU)       // 6.283185307179586
println(E)         // 2.718281828459045
println(INF)       // inf
println(NEG_INF)   // -inf
println(NAN)       // nan
```

**Note:** `PI` and `E` are constants in `std::math`. The core builtins `pi()` and `e()` (functions) are separate and available without an import.

---

## Arithmetic

### abs

`abs(x: f64) -> f64`

Return the absolute value of `x`.

**Example:**
```kryos
use std::math

println(abs(-3.14))   // 3.14
println(abs(2.0))     // 2.0
```

**Note:** This shadows the core builtin `abs`. The core builtin is polymorphic (works on integers too); `std::math.abs` is `f64`-only.

---

### min

`min(a: f64, b: f64) -> f64`

Return the smaller of two `f64` values.

**Example:**
```kryos
use std::math

println(min(3.0, 7.5))    // 3.0
println(min(-1.0, -5.0))  // -5.0
```

**Note:** This shadows the core builtin `min`. The core builtin also accepts a single array argument; `std::math.min` takes exactly two `f64` arguments.

---

### max

`max(a: f64, b: f64) -> f64`

Return the larger of two `f64` values.

**Example:**
```kryos
use std::math

println(max(3.0, 7.5))    // 7.5
println(max(-1.0, -5.0))  // -1.0
```

---

### clamp

`clamp(x: f64, lo: f64, hi: f64) -> f64`

Clamp `x` to the range `[lo, hi]`. Returns `lo` if `x < lo`, `hi` if `x > hi`, otherwise `x`.

**Example:**
```kryos
use std::math

println(clamp(5.0, 0.0, 10.0))   // 5.0
println(clamp(-3.0, 0.0, 10.0))  // 0.0
println(clamp(15.0, 0.0, 10.0))  // 10.0
```

---

### sign

`sign(x: f64) -> f64`

Return `1.0` if `x > 0`, `-1.0` if `x < 0`, or `0.0` if `x == 0`.

**Example:**
```kryos
use std::math

println(sign(3.14))   // 1.0
println(sign(-2.0))   // -1.0
println(sign(0.0))    // 0.0
```

---

## Rounding

`std::math` rounding functions return `f64`, unlike the core builtins (`floor`, `ceil`, `round`) which return `i64`. Use `std::math` rounding when you need to stay in floating-point arithmetic.

### floor

`floor(x: f64) -> f64`

Return the largest `f64` integer less than or equal to `x`.

**Example:**
```kryos
use std::math

println(floor(3.7))    // 3.0
println(floor(-2.3))   // -3.0
```

**Note:** Returns `f64`. The core builtin `floor` returns `i64`.

---

### ceil

`ceil(x: f64) -> f64`

Return the smallest `f64` integer greater than or equal to `x`.

**Example:**
```kryos
use std::math

println(ceil(3.2))    // 4.0
println(ceil(-2.7))   // -2.0
```

**Note:** Returns `f64`. The core builtin `ceil` returns `i64`.

---

### round

`round(x: f64) -> f64`

Round `x` to the nearest integer, with ties going away from zero (0.5 rounds to 1.0, -0.5 rounds to -1.0).

**Example:**
```kryos
use std::math

println(round(3.5))    // 4.0
println(round(4.5))    // 5.0
println(round(-0.5))   // -1.0
println(round(3.7))    // 4.0
```

**Note:** Returns `f64`. Uses half-away-from-zero rounding. The core builtin `round` returns `i64` and uses banker's rounding (round half to even).

---

## Powers and Roots

### sqrt

`sqrt(x: f64) -> f64`

Return the square root of `x`.

**Example:**
```kryos
use std::math

println(sqrt(16.0))   // 4.0
println(sqrt(2.0))    // 1.4142135623730951
```

**Edge cases:**
- Returns `NAN` for negative inputs. Does not throw.

---

### cbrt

`cbrt(x: f64) -> f64`

Return the cube root of `x`.

**Example:**
```kryos
use std::math

println(cbrt(27.0))    // 3.0
println(cbrt(-8.0))    // -2.0
println(cbrt(2.0))     // 1.2599210498948732
```

---

### pow

`pow(base: f64, exponent: f64) -> f64`

Return `base` raised to the power `exponent`.

**Example:**
```kryos
use std::math

println(pow(2.0, 10.0))   // 1024.0
println(pow(9.0, 0.5))    // 3.0
println(pow(2.0, -1.0))   // 0.5
```

**Note:** For integer exponentiation preserving type, use the `**` operator: `2 ** 10` returns `1024` (integer).

---

### exp

`exp(x: f64) -> f64`

Return e raised to the power `x` (e^x).

**Example:**
```kryos
use std::math

println(exp(0.0))   // 1.0
println(exp(1.0))   // 2.718281828459045
println(exp(2.0))   // 7.38905609893065
```

---

## Logarithms

### ln

`ln(x: f64) -> f64`

Return the natural logarithm (base e) of `x`.

**Example:**
```kryos
use std::math

println(ln(1.0))   // 0.0
println(ln(E))     // 1.0
println(ln(10.0))  // 2.302585092994046
```

**Edge cases:**
- Returns `NEG_INF` for `x == 0`.
- Returns `NAN` for `x < 0`. Does not throw.

---

### log2

`log2(x: f64) -> f64`

Return the base-2 logarithm of `x`.

**Example:**
```kryos
use std::math

println(log2(8.0))     // 3.0
println(log2(1024.0))  // 10.0
```

**Edge cases:** Returns `NEG_INF` for `x == 0`, `NAN` for `x < 0`.

---

### log10

`log10(x: f64) -> f64`

Return the base-10 logarithm of `x`.

**Example:**
```kryos
use std::math

println(log10(100.0))   // 2.0
println(log10(1000.0))  // 3.0
```

**Edge cases:** Returns `NEG_INF` for `x == 0`, `NAN` for `x < 0`.

**Note:** This shadows the core builtin `log10`. The behavior is identical for valid inputs.

---

## Trigonometry

All trigonometric functions use radians. Use `degrees` and `radians` to convert.

### sin

`sin(x: f64) -> f64`

Return the sine of `x`.

**Example:**
```kryos
use std::math

println(sin(0.0))       // 0.0
println(sin(PI / 2.0))  // 1.0
```

---

### cos

`cos(x: f64) -> f64`

Return the cosine of `x`.

**Example:**
```kryos
use std::math

println(cos(0.0))   // 1.0
println(cos(PI))    // -1.0
```

---

### tan

`tan(x: f64) -> f64`

Return the tangent of `x`.

**Example:**
```kryos
use std::math

println(tan(0.0))       // 0.0
println(tan(PI / 4.0))  // 1.0
```

**Edge cases:** Returns very large values near odd multiples of pi/2.

---

### asin

`asin(x: f64) -> f64`

Return the arcsine of `x` in radians. Domain: `[-1.0, 1.0]`.

**Example:**
```kryos
use std::math

println(asin(1.0))   // 1.5707963267948966 (pi/2)
println(asin(0.0))   // 0.0
```

**Edge cases:** Returns `NAN` for `|x| > 1`.

---

### acos

`acos(x: f64) -> f64`

Return the arccosine of `x` in radians. Domain: `[-1.0, 1.0]`.

**Example:**
```kryos
use std::math

println(acos(1.0))   // 0.0
println(acos(0.0))   // 1.5707963267948966 (pi/2)
```

**Edge cases:** Returns `NAN` for `|x| > 1`.

---

### atan

`atan(x: f64) -> f64`

Return the arctangent of `x` in radians. Range: `(-pi/2, pi/2)`.

**Example:**
```kryos
use std::math

println(atan(1.0))   // 0.7853981633974483 (pi/4)
println(atan(0.0))   // 0.0
```

---

### atan2

`atan2(y: f64, x: f64) -> f64`

Return the angle in radians between the positive x-axis and the point `(x, y)`. Range: `(-pi, pi]`.

**Example:**
```kryos
use std::math

println(atan2(1.0, 1.0))    // 0.7853981633974483 (pi/4)
println(atan2(0.0, -1.0))   // 3.141592653589793 (pi)
println(atan2(-1.0, 0.0))   // -1.5707963267948966 (-pi/2)
```

**Note:** Argument order is `atan2(y, x)` -- y first, x second.

---

## Hyperbolic

### sinh

`sinh(x: f64) -> f64`

Return the hyperbolic sine of `x`.

**Example:**
```kryos
use std::math

println(sinh(0.0))   // 0.0
println(sinh(1.0))   // 1.1752011936438014
```

---

### cosh

`cosh(x: f64) -> f64`

Return the hyperbolic cosine of `x`.

**Example:**
```kryos
use std::math

println(cosh(0.0))   // 1.0
println(cosh(1.0))   // 1.5430806348152417
```

---

### tanh

`tanh(x: f64) -> f64`

Return the hyperbolic tangent of `x`. Range: `(-1.0, 1.0)`.

**Example:**
```kryos
use std::math

println(tanh(0.0))   // 0.0
println(tanh(1.0))   // 0.7615941559557649
```

---

## Angle Conversion

### degrees

`degrees(radians: f64) -> f64`

Convert radians to degrees.

**Example:**
```kryos
use std::math

println(degrees(PI))        // 180.0
println(degrees(PI / 2.0))  // 90.0
println(degrees(TAU))       // 360.0
```

---

### radians

`radians(degrees: f64) -> f64`

Convert degrees to radians.

**Example:**
```kryos
use std::math

println(radians(180.0))   // 3.141592653589793
println(radians(90.0))    // 1.5707963267948966
println(radians(360.0))   // 6.283185307179586
```

---

## Interpolation

### lerp

`lerp(a: f64, b: f64, t: f64) -> f64`

Linearly interpolate between `a` and `b` by factor `t`. Returns `a` when `t == 0.0`, `b` when `t == 1.0`.

**Example:**
```kryos
use std::math

println(lerp(0.0, 10.0, 0.5))   // 5.0
println(lerp(0.0, 10.0, 0.0))   // 0.0
println(lerp(0.0, 10.0, 1.0))   // 10.0
println(lerp(2.0, 6.0, 0.25))   // 3.0
```

**Edge cases:** `t` is not clamped -- values outside `[0.0, 1.0]` extrapolate beyond `a` and `b`.

---

## Special Value Checks

### is_nan

`is_nan(x: f64) -> bool`

Return `true` if `x` is NaN.

**Example:**
```kryos
use std::math

println(is_nan(NAN))         // true
println(is_nan(0.0 / 0.0))   // true
println(is_nan(1.0))          // false
```

**Note:** NaN is not equal to itself -- `NAN == NAN` is `false`. Use `is_nan` to check.

---

### is_inf

`is_inf(x: f64) -> bool`

Return `true` if `x` is positive or negative infinity.

**Example:**
```kryos
use std::math

println(is_inf(INF))       // true
println(is_inf(NEG_INF))   // true
println(is_inf(1.0))        // false
```

---

## Integer Math

### gcd

`gcd(a: i64, b: i64) -> i64`

Return the greatest common divisor of `a` and `b`.

**Example:**
```kryos
use std::math

println(gcd(12, 8))    // 4
println(gcd(100, 75))  // 25
println(gcd(7, 13))    // 1
```

---

### lcm

`lcm(a: i64, b: i64) -> i64`

Return the least common multiple of `a` and `b`.

**Example:**
```kryos
use std::math

println(lcm(4, 6))    // 12
println(lcm(3, 5))    // 15
println(lcm(12, 8))   // 24
```

---

### factorial

`factorial(n: i64) -> i64`

Return `n!` (n factorial).

**Example:**
```kryos
use std::math

println(factorial(0))   // 1
println(factorial(5))   // 120
println(factorial(10))  // 3628800
```

**Edge cases:**
- Throws a runtime error for negative `n`.
- Large values of `n` will overflow `i64`.

---

## Core Builtin Differences

Several `std::math` functions shadow core builtins. Key differences:

| Function  | Core builtin return type       | `std::math` return type        | Other differences                    |
|-----------|-------------------------------|-------------------------------|--------------------------------------|
| `abs`     | `number` (polymorphic)         | `f64`                          | Core accepts integers                |
| `min`     | `number` (polymorphic)         | `f64`                          | Core accepts array argument          |
| `max`     | `number` (polymorphic)         | `f64`                          | Core accepts array argument          |
| `floor`   | `i64`                          | `f64`                          |                                      |
| `ceil`    | `i64`                          | `f64`                          |                                      |
| `round`   | `i64` (banker's rounding)      | `f64` (half-away-from-zero)    | Different rounding mode and type     |
| `sqrt`    | `f64` (throws on negative)     | `f64` (returns `NAN`)          | Error handling differs               |
| `log10`   | `f64`                          | `f64`                          | Identical behavior                   |
| `sin`     | `f64`                          | `f64`                          | Identical behavior                   |
| `cos`     | `f64`                          | `f64`                          | Identical behavior                   |
| `tan`     | `f64`                          | `f64`                          | Identical behavior                   |
| `pow`     | `f64`                          | `f64`                          | Identical behavior                   |

Functions **not** in `std::math` (core builtins only): `random()`, `pi()`, `e()`.
