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

### some

`some(val: any) -> Option`

Create an `Option` containing `val`.

---

### none

`none() -> Option`

Create an empty `Option`.

**Example:**
```kryos
use std::option

let a = some(42)
let b = none()
```

---

## Querying

### is_some

`is_some(opt: Option) -> bool`

Return `true` if the option contains a value.

---

### is_none

`is_none(opt: Option) -> bool`

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

`unwrap(opt: Option) -> any`

Return the contained value. Throws if the option is `None`.

---

### unwrap_or

`unwrap_or(opt: Option, default: any) -> any`

Return the contained value, or `default` if empty.

---

### unwrap_or_else

`unwrap_or_else(opt: Option, f: fn() -> any) -> any`

Return the contained value, or call `f` and return its result if empty.

**Example:**
```kryos
use std::option

let x = some(10)
println(unwrap_or(x, 0))       // 10

let y = none()
println(unwrap_or(y, 0))       // 0
println(unwrap_or_else(y, fn() -> i64 { return 99 }))   // 99
```

---

## Transforming

### map

`map(opt: Option, f: fn(any) -> any) -> Option`

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
    if x == 0 { return none() }
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

`filter(opt: Option, pred: fn(any) -> bool) -> Option`

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

`flatten(opt: Option) -> Option`

If `opt` is `Some(Some(v))`, return `Some(v)`. Removes one level of nesting.

---

### zip

`zip(a: Option, b: Option) -> Option`

If both `a` and `b` are `Some`, return `Some([a_val, b_val])`. Otherwise return `None`.

---

### replace

`replace(opt: Option, val: any) -> Option`

Return `Some(val)` regardless of whether `opt` was `Some` or `None`.

---

## Conversion

### to_array

`to_array(opt: Option) -> [any]`

Return `[v]` if `Some(v)`, or `[]` if `None`.

---

### ok_or

`ok_or(opt: Option, err_msg: any) -> any`

Convert to a Result-like value: `{ok: true, value: v}` if `Some(v)`, or `{ok: false, error: err_msg}` if `None`.

---

## Inspection

### inspect

`inspect(opt: Option, f: fn(any)) -> Option`

If `opt` is `Some(v)`, call `f(v)` as a side effect and return `opt` unchanged.

---

### display

`display(opt: Option) -> str`

Return `"Some(v)"` or `"None"` as a string.

---

### contains

`contains(opt: Option, val: any) -> bool`

Return `true` if `opt` is `Some(val)` using equality comparison.

---

### map_or

`map_or(opt: Option, default: any, f: fn(any) -> any) -> any`

Apply `f` to the contained value, or return `default` if empty.

---

### map_or_else

`map_or_else(opt: Option, default_fn: fn() -> any, f: fn(any) -> any) -> any`

Apply `f` to the contained value, or call `default_fn()` if empty.

---

### and_opt

`and_opt(a: Option, b: Option) -> Option`

Return `b` if `a` is `Some`, otherwise return `None`.

---

### or_opt

`or_opt(a: Option, b: Option) -> Option`

Return `a` if it is `Some`, otherwise return `b`.

---

### xor

`xor(a: Option, b: Option) -> Option`

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
    return none()
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
println(display(pair))   // "Some([3, 4])"
```
