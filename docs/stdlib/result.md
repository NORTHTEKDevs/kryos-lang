# std::result

Explicit error handling without exceptions. `Result` represents either a successful value (`Ok`) or an error (`Err`), making failure paths visible in the type system.

```kryos
use std::result
```

---

## Type

```kryos
enum Result {
    Ok(any),
    Err(any)
}
```

---

## Constructors

### ok

`ok(val: any) -> Result`

Create a successful `Result` containing `val`.

---

### err

`err(msg: any) -> Result`

Create a failed `Result` with error value `msg`.

**Example:**
```kryos
use std::result

let success = ok(42)
let failure = err("file not found")
```

---

## Querying

### is_ok

`is_ok(r: Result) -> bool`

Return `true` if the result is `Ok`.

---

### is_err

`is_err(r: Result) -> bool`

Return `true` if the result is `Err`.

---

### is_ok_and

`is_ok_and(r: Result, pred: fn(any) -> bool) -> bool`

Return `true` if the result is `Ok` and the value satisfies `pred`.

---

### is_err_and

`is_err_and(r: Result, pred: fn(any) -> bool) -> bool`

Return `true` if the result is `Err` and the error satisfies `pred`.

**Example:**
```kryos
use std::result

let r = ok(10)
println(is_ok(r))                                           // true
println(is_ok_and(r, fn(x: i64) -> bool { return x > 5 })) // true
```

---

## Extracting

### unwrap

`unwrap(r: Result) -> any`

Return the contained value. Throws `"unwrap called on Err: <error>"` if the result is `Err`.

---

### unwrap_err

`unwrap_err(r: Result) -> any`

Return the contained error. Throws if the result is `Ok`.

---

### unwrap_or

`unwrap_or(r: Result, default: any) -> any`

Return the contained value, or `default` if `Err`.

---

### unwrap_or_else

`unwrap_or_else(r: Result, f: fn(any) -> any) -> any`

Return the contained value, or call `f(error)` and return its result if `Err`.

---

### expect

`expect(r: Result, msg: str) -> any`

Return the contained value. Throws with `msg` if the result is `Err`.

---

### expect_err

`expect_err(r: Result, msg: str) -> any`

Return the contained error. Throws with `msg` if the result is `Ok`.

**Example:**
```kryos
use std::result

let r = ok(42)
println(unwrap(r))          // 42
println(unwrap_or(err("oops"), 0))   // 0

let val = expect(ok("data"), "expected data")
println(val)   // "data"
```

---

## Transforming Ok

### map

`map(r: Result, f: fn(any) -> any) -> Result`

If `r` is `Ok(v)`, return `Ok(f(v))`. If `Err`, return the error unchanged.

**Example:**
```kryos
use std::result

let r = map(ok(5), fn(x: i64) -> i64 { return x * 2 })
println(unwrap(r))   // 10
```

---

### map_or

`map_or(r: Result, default: any, f: fn(any) -> any) -> any`

Apply `f` to the Ok value, or return `default` if Err.

---

### map_or_else

`map_or_else(r: Result, default_fn: fn(any) -> any, f: fn(any) -> any) -> any`

Apply `f` to the Ok value, or call `default_fn(error)` if Err.

---

### and_then

`and_then(r: Result, f: fn(any) -> Result) -> Result`

If `r` is `Ok(v)`, return `f(v)`. If `Err`, return the error unchanged. Used to chain operations that may fail.

**Example:**
```kryos
use std::result

let parse_and_double = fn(s: str) -> Result {
    // imagine parse_int returns a Result
    return and_then(ok(42), fn(n: i64) -> Result {
        return ok(n * 2)
    })
}

println(unwrap(parse_and_double("42")))   // 84
```

---

## Transforming Err

### map_err

`map_err(r: Result, f: fn(any) -> any) -> Result`

If `r` is `Err(e)`, return `Err(f(e))`. If `Ok`, return unchanged.

---

### or_else

`or_else(r: Result, f: fn(any) -> Result) -> Result`

If `r` is `Ok`, return it. If `Err(e)`, call `f(e)` and return its result.

---

## Combining

### and_res

`and_res(a: Result, b: Result) -> Result`

Return `b` if `a` is `Ok`, otherwise return `a`'s error.

---

### or_res

`or_res(a: Result, b: Result) -> Result`

Return `a` if it is `Ok`, otherwise return `b`.

---

### flatten

`flatten(r: Result) -> Result`

If `r` is `Ok(Ok(v))`, return `Ok(v)`. Removes one level of `Result` nesting.

---

## Conversion

### to_option

`to_option(r: Result) -> Option`

Convert an `Ok(v)` to `Some(v)`, or `Err` to `None`.

---

### err_to_option

`err_to_option(r: Result) -> Option`

Convert an `Err(e)` to `Some(e)`, or `Ok` to `None`.

---

### to_array

`to_array(r: Result) -> [any]`

Return `[v]` if `Ok(v)`, or `[]` if `Err`.

---

## Inspection

### inspect

`inspect(r: Result, f: fn(any)) -> Result`

If `r` is `Ok(v)`, call `f(v)` as a side effect and return `r` unchanged.

---

### inspect_err

`inspect_err(r: Result, f: fn(any)) -> Result`

If `r` is `Err(e)`, call `f(e)` as a side effect and return `r` unchanged.

---

### display

`display(r: Result) -> str`

Return `"Ok(v)"` or `"Err(e)"` as a string.

---

### contains

`contains(r: Result, val: any) -> bool`

Return `true` if `r` is `Ok(val)`.

---

### contains_err

`contains_err(r: Result, val: any) -> bool`

Return `true` if `r` is `Err(val)`.

---

## Try Pattern

### try_fn

`try_fn(f: fn() -> any) -> Result`

Call `f` and return `Ok` with its return value. If `f` throws, catch the error and return `Err` with the error message.

**Example:**
```kryos
use std::result

let r = try_fn(fn() -> i64 {
    return 100 / 0   // throws division by zero
})
println(is_err(r))   // true
```

---

### collect

`collect(results: [Result]) -> Result`

Given an array of `Result` values, return `Ok([values])` if all are `Ok`, or the first `Err` encountered.

**Example:**
```kryos
use std::result

let all_ok = collect([ok(1), ok(2), ok(3)])
println(display(all_ok))   // "Ok([1, 2, 3])"

let has_err = collect([ok(1), err("bad"), ok(3)])
println(display(has_err))  // "Err(bad)"
```

---

## Complete Example

```kryos
use std::result

// Simulate a chain of fallible operations
let read_config = fn() -> Result {
    return ok("{\"port\": 8080}")
}

let parse_port = fn(config: str) -> Result {
    // In practice: parse the JSON and extract the port
    return ok(8080)
}

let validate_port = fn(port: i64) -> Result {
    if port < 1 || port > 65535 {
        return err("port out of range: " + port)
    }
    return ok(port)
}

let result = and_then(
    and_then(read_config(), fn(cfg: str) -> Result {
        return parse_port(cfg)
    }),
    fn(port: i64) -> Result {
        return validate_port(port)
    }
)

if is_ok(result) {
    println("listening on port " + unwrap(result))   // 8080
} else {
    println("error: " + unwrap_err(result))
}

// Safe error wrapping
let risky = try_fn(fn() -> i64 {
    return 1 / 0
})
println(is_err(risky))   // true
```
