# std::math

Extended math functions. These supplement the core math builtins (`sin`, `cos`, `tan`, `log`, `pow`, `floor`, `ceil`, `sqrt`, `min`, `max`, `abs`) with rounding, logarithms, random numbers, and constants.

```kryos
import std::math
```

---

### round

`round(x: Float) -> Int`

Round a number to the nearest integer.

**Example:**
```kryos
print(round(3.7))   // 4
print(round(3.2))   // 3
print(round(-1.5))  // -2
```

**Edge cases:**
- Uses banker's rounding (round half to even) for .5 values: `round(0.5)` returns `0`, `round(1.5)` returns `2`.

**See also:** floor, ceil

---

### log10

`log10(x: Float) -> Float`

Base-10 logarithm.

**Example:**
```kryos
print(log10(100))    // 2.0
print(log10(1000))   // 3.0
print(log10(1))      // 0.0
```

**Edge cases:**
- Raises a domain error for non-positive values.

**See also:** log (core builtin, natural log)

---

### random

`random() -> Float`

Generate a random floating-point number in the range [0.0, 1.0).

**Example:**
```kryos
let r = random()
print(r)  // 0.7312... (varies each call)
```

```kryos
// Random integer between 1 and 6 (dice roll)
let die = floor(random() * 6) + 1
print(die)
```

```kryos
// Random element from an array
let items = ["red", "green", "blue"]
let pick = items[floor(random() * len(items))]
print(pick)
```

**Edge cases:**
- Not cryptographically secure. Use `generate_token` from `std::auth` for security-sensitive randomness.

---

### pi

`pi() -> Float`

The mathematical constant pi (3.141592653589793).

**Example:**
```kryos
let circumference = 2 * pi() * radius
let area = pi() * pow(radius, 2)
```

**See also:** e

---

### e

`e() -> Float`

Euler's number (2.718281828459045).

**Example:**
```kryos
let growth = pow(e(), rate * time)
print(growth)
```

```kryos
// Natural exponential: e^x is equivalent to pow(e(), x)
let ex = pow(e(), 1)
print(ex)  // 2.718281828459045
```

**See also:** pi, log
