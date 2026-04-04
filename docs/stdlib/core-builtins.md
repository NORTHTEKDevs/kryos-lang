# Core Builtins Reference

These functions are available in every `.kry` program without imports. They are registered directly by the interpreter at startup.

---

## I/O

### println

```
fn println(args: ...any)
```

Print arguments separated by spaces, followed by a newline. Each argument is formatted using Kryos value formatting rules (`true`/`false` for booleans, `none` for null, array brackets for lists).

```kryos
println("hello", "world")      // hello world
println(42)                     // 42
println(true, none)             // true none
println([1, 2, 3])              // [1, 2, 3]
```

**Edge cases:** Calling with zero arguments prints an empty line. Struct instances print as `Name { field: value }`.

**See also:** `print`, `to_string`

---

### print

```
fn print(args: ...any)
```

Print arguments separated by spaces, without a trailing newline. Useful for building output incrementally or prompts.

```kryos
print("Enter name: ")
let name = stdin_read()
```

**Edge cases:** Same formatting rules as `println`.

**See also:** `println`, `stdin_read`

---

### stdin_read

```
fn stdin_read() -> str
```

Read a single line from standard input. Blocks until the user presses Enter.

```kryos
print("Your name: ")
let name = stdin_read()
println("Hello, " + name)
```

**Edge cases:** Returns the line without the trailing newline character. Returns an empty string if stdin is closed.

**See also:** `print`, `println`

---

## Math

### abs

```
fn abs(x: number) -> number
```

Return the absolute value of a number. Preserves the input type (integer in, integer out; float in, float out).

```kryos
println(abs(-5))       // 5
println(abs(3.14))     // 3.14
println(abs(-0.5))     // 0.5
```

**Edge cases:** Raises a runtime error if `x` is not a number.

**See also:** `min`, `max`

---

### sqrt

```
fn sqrt(x: number) -> f64
```

Return the square root of `x`. The input is converted to `f64` before computation.

```kryos
println(sqrt(16))      // 4.0
println(sqrt(2))       // 1.4142135623730951
```

**Edge cases:** Raises a math domain error for negative inputs.

**See also:** `pow`, `log`

---

### sin

```
fn sin(x: number) -> f64
```

Return the sine of `x` (in radians).

```kryos
println(sin(0))           // 0.0
println(sin(pi() / 2))    // 1.0
```

**See also:** `cos`, `tan`, `pi`

---

### cos

```
fn cos(x: number) -> f64
```

Return the cosine of `x` (in radians).

```kryos
println(cos(0))        // 1.0
println(cos(pi()))     // -1.0
```

**See also:** `sin`, `tan`, `pi`

---

### tan

```
fn tan(x: number) -> f64
```

Return the tangent of `x` (in radians).

```kryos
println(tan(0))        // 0.0
```

**Edge cases:** Returns very large values near odd multiples of pi/2.

**See also:** `sin`, `cos`

---

### log

```
fn log(x: number) -> f64
```

Return the natural logarithm (base e) of `x`.

```kryos
println(log(1))        // 0.0
println(log(e()))      // 1.0
```

**Edge cases:** Raises a math domain error for `x <= 0`.

**See also:** `log10`, `e`

---

### log10

```
fn log10(x: number) -> f64
```

Return the base-10 logarithm of `x`.

```kryos
println(log10(100))    // 2.0
println(log10(1000))   // 3.0
```

**Edge cases:** Raises a math domain error for `x <= 0`.

**See also:** `log`

---

### pow

```
fn pow(x: number, y: number) -> f64
```

Return `x` raised to the power `y`. Both arguments are converted to `f64`.

```kryos
println(pow(2, 10))    // 1024.0
println(pow(9, 0.5))   // 3.0
```

**Note:** For integer exponentiation preserving type, use the `**` operator instead: `2 ** 10` returns `1024` (integer).

**See also:** `sqrt`, `**` operator

---

### floor

```
fn floor(x: number) -> i64
```

Return the largest integer less than or equal to `x`.

```kryos
println(floor(3.7))    // 3
println(floor(-2.3))   // -3
```

**See also:** `ceil`, `round`

---

### ceil

```
fn ceil(x: number) -> i64
```

Return the smallest integer greater than or equal to `x`.

```kryos
println(ceil(3.2))     // 4
println(ceil(-2.7))    // -2
```

**See also:** `floor`, `round`

---

### round

```
fn round(x: number) -> i64
```

Round `x` to the nearest integer. Uses banker's rounding (round half to even).

```kryos
println(round(3.5))    // 4
println(round(4.5))    // 4
println(round(3.7))    // 4
```

**See also:** `floor`, `ceil`

---

### min

```
fn min(a, b) -> number
```

Return the smaller of two values. Also accepts a single array argument to find the minimum element.

```kryos
println(min(3, 7))         // 3
println(min(-1, -5))       // -5
println(min([4, 2, 8]))    // 2
```

**See also:** `max`, `abs`

---

### max

```
fn max(a, b) -> number
```

Return the larger of two values. Also accepts a single array argument to find the maximum element.

```kryos
println(max(3, 7))         // 7
println(max(-1, -5))       // -1
println(max([4, 2, 8]))    // 8
```

**See also:** `min`, `abs`

---

### random

```
fn random() -> f64
```

Return a random floating-point number in the range `[0.0, 1.0)`.

```kryos
let r = random()
println(r)             // e.g. 0.7312...

// Random integer between 1 and 100
let n = floor(random() * 100) + 1
```

**See also:** `random_bytes` (in `std::crypto`)

---

### pi

```
fn pi() -> f64
```

Return the mathematical constant pi (3.141592653589793).

```kryos
let circumference = 2 * pi() * radius
println(sin(pi() / 2))    // 1.0
```

**See also:** `e`, `sin`, `cos`

---

### e

```
fn e() -> f64
```

Return Euler's number (2.718281828459045).

```kryos
println(log(e()))      // 1.0
```

**See also:** `pi`, `log`

---

## Strings

### len

```
fn len(s: str) -> i64
fn len(arr: [any]) -> i64
```

Return the length of a string (number of characters) or array (number of elements). This function is dual-purpose.

```kryos
println(len("hello"))     // 5
println(len(""))           // 0
println(len([1, 2, 3]))   // 3
```

**Edge cases:** Raises a runtime error if the argument is not a string or array.

**See also:** `char_at`, `substr`

---

### char_at

```
fn char_at(s: str, i: i64) -> str
```

Return the character at index `i` in string `s`. Zero-indexed.

```kryos
println(char_at("hello", 0))   // h
println(char_at("hello", 4))   // o
```

**Edge cases:** Raises an index error if `i` is out of bounds.

**See also:** `len`, `substr`, `char_code`

---

### char_code

```
fn char_code(c: str) -> i64
```

Return the Unicode code point of the first character in `c`.

```kryos
println(char_code("A"))   // 65
println(char_code("a"))   // 97
println(char_code("0"))   // 48
```

**See also:** `char_from`, `char_at`

---

### char_from

```
fn char_from(n: i64) -> str
```

Return the character corresponding to Unicode code point `n`.

```kryos
println(char_from(65))    // A
println(char_from(97))    // a
println(char_from(9731))  // snowman character
```

**See also:** `char_code`

---

### substr

```
fn substr(s: str, start: i64, end?: i64) -> str
```

Return a substring from index `start` up to (but not including) `end`. If `end` is omitted, returns from `start` to the end of the string.

```kryos
println(substr("hello world", 0, 5))    // hello
println(substr("hello world", 6))        // world
println(substr("abcdef", 2, 4))          // cd
```

**Edge cases:** Indices are clamped to string bounds. Negative indices are not supported.

**See also:** `len`, `char_at`, `contains`

---

### contains

```
fn contains(s: str, sub: str) -> bool
```

Return `true` if `s` contains the substring `sub`.

```kryos
println(contains("hello world", "world"))   // true
println(contains("hello", "xyz"))           // false
println(contains("", ""))                   // true
```

**See also:** `starts_with`, `ends_with`, `replace`

---

### starts_with

```
fn starts_with(s: str, prefix: str) -> bool
```

Return `true` if `s` begins with `prefix`.

```kryos
println(starts_with("hello", "he"))     // true
println(starts_with("hello", "lo"))     // false
```

**See also:** `ends_with`, `contains`

---

### ends_with

```
fn ends_with(s: str, suffix: str) -> bool
```

Return `true` if `s` ends with `suffix`.

```kryos
println(ends_with("hello.kry", ".kry"))   // true
println(ends_with("hello", "he"))         // false
```

**See also:** `starts_with`, `contains`

---

### upper

```
fn upper(s: str) -> str
```

Return a copy of `s` with all characters converted to uppercase.

```kryos
println(upper("hello"))   // HELLO
println(upper("Hello"))   // HELLO
```

**See also:** `lower`, `trim`

---

### lower

```
fn lower(s: str) -> str
```

Return a copy of `s` with all characters converted to lowercase.

```kryos
println(lower("HELLO"))   // hello
println(lower("Hello"))   // hello
```

**See also:** `upper`, `trim`

---

### trim

```
fn trim(s: str) -> str
```

Return a copy of `s` with leading and trailing whitespace removed.

```kryos
println(trim("  hello  "))    // hello
println(trim("\thello\n"))    // hello
```

**See also:** `upper`, `lower`

---

### split

```
fn split(s: str, delim: str = " ") -> [str]
```

Split `s` into an array of substrings using `delim` as the delimiter. Defaults to splitting on spaces.

```kryos
let parts = split("a,b,c", ",")
println(parts)                     // [a, b, c]

let words = split("hello world")
println(words)                     // [hello, world]
```

**See also:** `join`, `trim`

---

### join

```
fn join(delim: str, arr: [any]) -> str
```

Join the elements of `arr` into a single string, separated by `delim`. Each element is converted to a string.

```kryos
println(join(", ", ["a", "b", "c"]))    // a, b, c
println(join("-", [1, 2, 3]))           // 1-2-3
```

**Note:** The argument order is `join(delimiter, array)`, not `join(array, delimiter)`.

**See also:** `split`

---

### replace

```
fn replace(s: str, old: str, new: str) -> str
```

Replace all occurrences of `old` with `new` in `s`.

```kryos
println(replace("hello world", "world", "kryos"))   // hello kryos
println(replace("aaa", "a", "bb"))                   // bbbbbb
```

**Edge cases:** If `old` is not found, the original string is returned unchanged.

**See also:** `contains`, `split`

---

## Arrays

### push

```
fn push(arr: [any], val: any)
```

Append `val` to the end of `arr`. Mutates the array in place. Returns `none`.

```kryos
let mut items = [1, 2, 3]
push(items, 4)
println(items)             // [1, 2, 3, 4]
```

**Edge cases:** Raises a runtime error if the first argument is not an array.

**See also:** `pop`, `len`

---

### pop

```
fn pop(arr: [any]) -> any
```

Remove and return the last element of `arr`. Mutates the array in place.

```kryos
let mut items = [1, 2, 3]
let last = pop(items)
println(last)              // 3
println(items)             // [1, 2]
```

**Edge cases:** Raises a runtime error on an empty array.

**See also:** `push`, `len`

---

### range

```
fn range(end: i64) -> [i64]
fn range(start: i64, end: i64) -> [i64]
fn range(start: i64, end: i64, step: i64) -> [i64]
```

Generate an array of integers. With one argument, generates `[0, 1, ..., end-1]`. With two, generates `[start, start+1, ..., end-1]`. With three, generates with the given step.

```kryos
println(range(5))          // [0, 1, 2, 3, 4]
println(range(1, 4))       // [1, 2, 3]
println(range(0, 10, 2))   // [0, 2, 4, 6, 8]
println(range(5, 0, -1))   // [5, 4, 3, 2, 1]
```

**Edge cases:** Returns an empty array if `start >= end` (for positive step). Raises a runtime error if called with zero or more than three arguments.

**See also:** `for` loop, `len`

---

## Conversion

### to_string

```
fn to_string(x: any) -> str
```

Convert any value to its string representation using Kryos formatting rules.

```kryos
println(to_string(42))            // 42
println(to_string(true))          // true
println(to_string(none))          // none
println(to_string([1, 2, 3]))     // [1, 2, 3]
```

**See also:** `str`, `int`, `float`

---

### str

```
fn str(x: any) -> str
```

Convert a value to a string. Functionally identical to `to_string`.

```kryos
let s = str(42)
println(s + " items")     // 42 items
```

**See also:** `to_string`, `int`, `float`

---

### int

```
fn int(x: any) -> i64
```

Convert a value to an integer. Strings are parsed, floats are truncated.

```kryos
println(int("42"))         // 42
println(int(3.9))          // 3
println(int(true))         // 1
```

**Edge cases:** Raises a runtime error if the string cannot be parsed as an integer.

**See also:** `float`, `str`, `to_string`

---

### float

```
fn float(x: any) -> f64
```

Convert a value to a floating-point number.

```kryos
println(float("3.14"))    // 3.14
println(float(42))         // 42.0
```

**Edge cases:** Raises a runtime error if the string cannot be parsed as a float.

**See also:** `int`, `str`

---

### type_of

```
fn type_of(x: any) -> str
```

Return the runtime type name of a value as a string.

```kryos
println(type_of(42))           // i32
println(type_of(3.14))         // f64
println(type_of("hello"))      // str
println(type_of(true))         // bool
println(type_of([1, 2]))       // array
println(type_of(none))         // none
```

Struct instances return the struct name. Enum values return the enum name.

```kryos
struct Point { x: i32, y: i32 }
let p = Point { x: 1, y: 2 }
println(type_of(p))            // Point
```

**See also:** `to_string`

---

## Assert

### assert

```
fn assert(cond: bool, msg?: str)
```

Assert that `cond` is truthy. If the assertion fails, the program panics with the provided message (or a default `"assertion failed"`).

```kryos
assert(1 + 1 == 2)
assert(len("hello") == 5, "string length should be 5")

// This will panic:
// assert(false, "this should not happen")
```

**Edge cases:** The condition is evaluated for truthiness, not strict boolean equality. `0`, `""`, `none`, `false`, and empty arrays are falsy.

**See also:** `type_of`
