# std::option

A type-safe optional value. `Option` represents a value that may or may not be present, replacing `null` checks with explicit, composable operations.

```kryos
use std::option
```

---

## Type

```kryos
enum Option {
    Some(any),
    None
}
```

---

## Constructors

**The idiomatic form used elsewhere in these docs (cheatsheet.md, the language
reference) is the capitalized enum-variant form directly:** `Some(x)` /
`None()`, via `use std::option::{Some, None}`. `some`/`none_value` below are
lowercase function wrappers around those same variants -- both forms work and
produce an identical `Option`, but a new user who has already seen `Some(x)`/
`None()` elsewhere should not expect a matching lowercase `none()` to exist.

### some

`some<T>(val: T) -> Option<T>`

Create an `Option` containing `val`.

---

### none_value

`none_value() -> Option`

Create an empty `Option`. **Named `none_value`, not `none`** -- `none` is a
reserved keyword in Kryos (it backs the `Option::None`/`Result` machinery at
the language level), so `use std::option::{none}` is a clean compile error
(`E0009: reserved keyword 'none' cannot be used as a name here`).

**Example:**
```kryos
use std::option

let a = some(42)
let b = none_value()
```

---

## Querying

### is_some

`is_some<T>(opt: Option<T>) -> bool`

Return `true` if the option contains a value.

---

### is_none

`is_none<T>(opt: Option<T>) -> bool`

Return `true` if the option is empty.

**Example:**
```kryos
use std::option

let x = some("hello")
println(is_some(x))   // true
println(is_none(x))   // false
```

---

## Extracting

### unwrap

`unwrap<T>(opt: Option<T>) -> T`

Return the contained value. Throws if the option is `None`.

---

### unwrap_or

`unwrap_or<T>(opt: Option<T>, default: T) -> T`

Return the contained value, or `default` if empty.

---

### unwrap_or_else

`unwrap_or_else<T>(opt: Option<T>, f: fn() -> T) -> T`

Return the contained value, or call `f` and return its result if empty.

**Example:**
```kryos
use std::option

let x = some(10)
println(unwrap_or(x, 0))       // 10

let y = none_value()
println(unwrap_or(y, 0))       // 0
println(unwrap_or_else(y, fn() -> i64 { return 99 }))   // 99
```

---

## Transforming

### map

`map<T, U>(opt: Option<T>, f: fn(T) -> U) -> Option<U>`

If `opt` is `Some(v)`, return `Some(f(v))`. If empty, return `None`.

**Example:**
```kryos
use std::option

let doubled = map(some(5), fn(x: i64) -> i64 { return x * 2 })
println(unwrap(doubled))   // 10
```

---

### and_then

`and_then(opt: Option, f: fn(any) -> Option) -> Option`

If `opt` is `Some(v)`, return `f(v)` (which itself returns an `Option`). If empty, return `None`. Also known as flat map.

**Example:**
```kryos
use std::option

let safe_div = fn(x: i64) -> Option {
    if x == 0 { return none_value() }
    return some(100 / x)
}

println(unwrap(and_then(some(5), safe_div)))   // 20
println(is_none(and_then(some(0), safe_div)))  // true
```

---

### or_else

`or_else(opt: Option, f: fn() -> Option) -> Option`

If `opt` is `Some`, return it unchanged. If empty, call `f` and return its result.

---

### filter

`filter<T>(opt: Option<T>, pred: fn(T) -> bool) -> Option<T>`

If `opt` is `Some(v)` and `pred(v)` is `true`, return `Some(v)`. Otherwise return `None`.

**Example:**
```kryos
use std::option

let even = filter(some(4), fn(x: i64) -> bool { return x % 2 == 0 })
println(is_some(even))   // true

let odd = filter(some(3), fn(x: i64) -> bool { return x % 2 == 0 })
println(is_none(odd))    // true
```

---

## Combining

### flatten

`flatten<T>(opt: Option<Option<T>>) -> Option<T>`

If `opt` is `Some(Some(v))`, return `Some(v)`. Removes one level of nesting.

---

### zip

`zip<A, B>(a: Option<A>, b: Option<B>) -> Option<(A, B)>`

If both `a` and `b` are `Some`, return `Some((a_val, b_val))` -- a TUPLE pair (read `.0` / `.1` or destructure). Otherwise return `None`. Both payload types are preserved.

---

### replace

`replace<T>(opt: Option<T>, val: T) -> Option<T>`

Return `Some(val)` if `opt` was `Some`, preserving `None` otherwise.

---

## Conversion

### to_array

`to_array<T>(opt: Option<T>) -> [T]`

Return `[v]` if `Some(v)`, or `[]` if `None`.

---

### ok_or

`ok_or<T>(opt: Option<T>, err_msg: str) -> Result<T, str>`

Convert to a real `Result`: `Result.Ok(v)` if `Some(v)`, or `Result.Err(err_msg)` if `None`. Match on the result with `Result.Ok(v) => ...` / `Result.Err(e) => ...`.

---

## Inspection

### inspect

`inspect<T, U>(opt: Option<T>, f: fn(T) -> U) -> Option<T>`

If `opt` is `Some(v)`, call `f(v)` as a side effect and return `opt` unchanged.

---

### display

`display<T>(opt: Option<T>) -> str`

Return `"Some(v)"` or `"None"` as a string.

---

### contains

`contains<T>(opt: Option<T>, val: T) -> bool`

Return `true` if `opt` is `Some(val)` using equality comparison.

---

### map_or

`map_or<T, U>(opt: Option<T>, default: U, f: fn(T) -> U) -> U`

Apply `f` to the contained value, or return `default` if empty.

---

### map_or_else

`map_or_else<T, U>(opt: Option<T>, default_fn: fn() -> U, f: fn(T) -> U) -> U`

Apply `f` to the contained value, or call `default_fn()` if empty.

---

### and_opt

`and_opt<T, U>(a: Option<T>, b: Option<U>) -> Option<U>`

Return `b` if `a` is `Some`, otherwise return `None`.

---

### or_opt

`or_opt<T>(a: Option<T>, b: Option<T>) -> Option<T>`

Return `a` if it is `Some`, otherwise return `b`.

---

### xor

`xor<T>(a: Option<T>, b: Option<T>) -> Option<T>`

Return `Some` if exactly one of `a` or `b` is `Some`. Return `None` if both are `Some` or both are `None`.

---

## Complete Example

```kryos
use std::option

// Safe lookup in a list
let find_first = fn(items: [i64], target: i64) -> Option {
    let i = 0
    while i < len(items) {
        if items[i] == target { return some(items[i]) }
        i = i + 1
    }
    return none_value()
}

let result = find_first([10, 20, 30, 40], 30)
println(display(result))          // "Some(30)"
println(unwrap_or(result, -1))    // 30

let missing = find_first([10, 20, 30], 99)
println(display(missing))         // "None"
println(unwrap_or(missing, -1))   // -1

// Transform chain
let processed = and_then(
    filter(some(6), fn(x: i64) -> bool { return x % 2 == 0 }),
    fn(x: i64) -> Option { return some(x * x) }
)
println(unwrap(processed))   // 36

// Zip two optional values
let a = some(3)
let b = some(4)
let pair = zip(a, b)
let (x, y) = unwrap(pair)
println(to_string(x) + "," + to_string(y))   // 3,4
```
