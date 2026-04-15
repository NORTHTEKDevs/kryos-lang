# std::json

JSON parsing, serialization, and value manipulation. Built around a typed `JsonValue` enum that represents the full JSON value space.

```kryos
use std::json
```

---

## JsonValue

The central type in `std::json`. Every JSON value -- whether a number, string, object, or array -- is represented as a `JsonValue`.

```kryos
enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(str),
    Array([JsonValue]),
    Object([str], [JsonValue])
}
```

**Note on objects:** JSON objects are stored as two parallel arrays -- a `[str]` of keys and a `[JsonValue]` of values in matching order. Use `get`, `set`, and `to_object` to work with them without managing the arrays directly.

---

## Constructors

These functions build `JsonValue` instances. Use them when constructing JSON programmatically.

### json_null

`json_null() -> JsonValue`

Create a JSON null value.

**Example:**
```kryos
use std::json

let v = json_null()
println(is_null(v))   // true
println(stringify(v)) // null
```

---

### json_bool

`json_bool(v: bool) -> JsonValue`

Create a JSON boolean value.

**Example:**
```kryos
use std::json

let t = json_bool(true)
let f = json_bool(false)
println(stringify(t))   // true
println(stringify(f))   // false
```

---

### json_number

`json_number(v: f64) -> JsonValue`

Create a JSON number value. All JSON numbers are `f64` internally.

**Example:**
```kryos
use std::json

let n = json_number(3.14)
println(stringify(n))   // 3.14

let n2 = json_number(42.0)
println(stringify(n2))  // 42
```

---

### json_string

`json_string(v: str) -> JsonValue`

Create a JSON string value.

**Example:**
```kryos
use std::json

let s = json_string("hello")
println(stringify(s))   // "hello"
```

---

### json_array

`json_array(items: [JsonValue]) -> JsonValue`

Create a JSON array from a Kryos array of `JsonValue` elements.

**Example:**
```kryos
use std::json

let arr = json_array([
    json_number(1.0),
    json_number(2.0),
    json_number(3.0)
])
println(stringify(arr))   // [1,2,3]
```

---

### json_object

`json_object(keys: [str], values: [JsonValue]) -> JsonValue`

Create a JSON object from parallel key and value arrays. The arrays must be the same length.

**Example:**
```kryos
use std::json

let obj = json_object(
    ["name", "age"],
    [json_string("Alice"), json_number(30.0)]
)
println(stringify(obj))   // {"name":"Alice","age":30}
```

**Note:** Keys and values are stored as parallel arrays in the same order they are passed. Use `get` and `set` rather than indexing the arrays directly.

---

## Parsing and Serialization

### parse

`parse(input: str) -> JsonValue`

Parse a JSON string and return the corresponding `JsonValue` tree. Throws a runtime error if the input is not valid JSON.

**Example:**
```kryos
use std::json

let val = parse("{\"name\":\"Alice\",\"score\":98}")
let name = to_str(get(val, "name"))
println(name)   // Alice
```

**Edge cases:**
- Throws on malformed JSON (unmatched braces, invalid escapes, trailing commas, etc.).
- Numbers are always parsed as `f64`.

---

### stringify

`stringify(val: JsonValue) -> str`

Serialize `val` to a compact JSON string with no extra whitespace.

**Example:**
```kryos
use std::json

let obj = json_object(["x", "y"], [json_number(1.0), json_number(2.0)])
println(stringify(obj))   // {"x":1,"y":2}

let arr = json_array([json_string("a"), json_bool(true), json_null()])
println(stringify(arr))   // ["a",true,null]
```

---

### pretty_print

`pretty_print(val: JsonValue, indent: i64) -> str`

Serialize `val` to a human-readable JSON string with `indent` spaces of indentation per level.

**Example:**
```kryos
use std::json

let obj = json_object(
    ["name", "scores"],
    [
        json_string("Alice"),
        json_array([json_number(95.0), json_number(87.0)])
    ]
)
println(pretty_print(obj, 2))
// {
//   "name": "Alice",
//   "scores": [
//     95,
//     87
//   ]
// }
```

---

## Access

### get

`get(val: JsonValue, key: str) -> JsonValue`

Return the value for `key` in a JSON object. Returns `JsonValue.Null` if `key` is not found or if `val` is not an object.

**Example:**
```kryos
use std::json

let obj = parse("{\"host\":\"localhost\",\"port\":8080}")
let host = to_str(get(obj, "host"))
let port = to_int(get(obj, "port"))
println(host)   // localhost
println(port)   // 8080
```

---

### get_index

`get_index(val: JsonValue, index: i64) -> JsonValue`

Return the element at `index` in a JSON array. Returns `JsonValue.Null` if `index` is out of bounds or if `val` is not an array.

**Example:**
```kryos
use std::json

let arr = parse("[10, 20, 30]")
println(to_int(get_index(arr, 0)))   // 10
println(to_int(get_index(arr, 2)))   // 30
```

---

### set

`set(val: JsonValue, key: str, new_val: JsonValue) -> JsonValue`

Return a new JSON object with `key` set to `new_val`. If `key` already exists, it is updated. If `val` is not an object, returns `val` unchanged.

**Example:**
```kryos
use std::json

let obj = json_object(["x"], [json_number(1.0)])
let obj = set(obj, "y", json_number(2.0))
println(stringify(obj))   // {"x":1,"y":2}
```

---

## Type Checks

These return `true` if the `JsonValue` matches the named variant.

### is_null

`is_null(val: JsonValue) -> bool`

```kryos
println(is_null(json_null()))         // true
println(is_null(json_number(0.0)))    // false
```

---

### is_bool

`is_bool(val: JsonValue) -> bool`

```kryos
println(is_bool(json_bool(true)))     // true
println(is_bool(json_string("yes")))  // false
```

---

### is_number

`is_number(val: JsonValue) -> bool`

```kryos
println(is_number(json_number(42.0)))  // true
println(is_number(json_string("42")))  // false
```

---

### is_string

`is_string(val: JsonValue) -> bool`

```kryos
println(is_string(json_string("hi")))  // true
println(is_string(json_null()))        // false
```

---

### is_array

`is_array(val: JsonValue) -> bool`

```kryos
println(is_array(json_array([])))          // true
println(is_array(json_object([], [])))     // false
```

---

### is_object

`is_object(val: JsonValue) -> bool`

```kryos
println(is_object(json_object([], [])))  // true
println(is_object(json_array([])))       // false
```

---

## Type Converters

Extract native Kryos values from a `JsonValue`. Each converter throws a runtime error if the value is not the expected type.

### to_bool

`to_bool(val: JsonValue) -> bool`

Extract the `bool` from a `JsonValue.Bool`. Throws if `val` is not a boolean.

**Example:**
```kryos
use std::json

let v = parse("true")
println(to_bool(v))   // true
```

---

### to_int

`to_int(val: JsonValue) -> i64`

Extract the number from a `JsonValue.Number` and return it as `i64` (truncates the fractional part). Throws if `val` is not a number.

**Example:**
```kryos
use std::json

let v = parse("42")
println(to_int(v))   // 42
```

---

### to_float

`to_float(val: JsonValue) -> f64`

Extract the number from a `JsonValue.Number` as `f64`. Throws if `val` is not a number.

**Example:**
```kryos
use std::json

let v = parse("3.14")
println(to_float(v))   // 3.14
```

---

### to_str

`to_str(val: JsonValue) -> str`

Extract the string from a `JsonValue.String`. Throws if `val` is not a string.

**Example:**
```kryos
use std::json

let v = parse("\"hello\"")
println(to_str(v))   // hello
```

---

### to_array

`to_array(val: JsonValue) -> [JsonValue]`

Extract the element array from a `JsonValue.Array`. Throws if `val` is not an array.

**Example:**
```kryos
use std::json

let v = parse("[1, 2, 3]")
let elems = to_array(v)
println(to_int(elems[0]))   // 1
```

---

### to_object

`to_object(val: JsonValue) -> ([str], [JsonValue])`

Extract the parallel key and value arrays from a `JsonValue.Object`. Returns a tuple `([str], [JsonValue])`. Throws if `val` is not an object.

**Example:**
```kryos
use std::json

let v = parse("{\"a\":1,\"b\":2}")
let (keys, values) = to_object(v)
println(keys)               // [a, b]
println(to_int(values[0]))  // 1
```

---

## Utility

### length

`length(val: JsonValue) -> i64`

Return the number of elements in a `JsonValue.Array` or the number of keys in a `JsonValue.Object`. Returns `0` for all other variants.

**Example:**
```kryos
use std::json

let arr = parse("[1, 2, 3, 4]")
println(length(arr))   // 4

let obj = parse("{\"a\":1,\"b\":2}")
println(length(obj))   // 2

println(length(json_null()))   // 0
```

---

## Complete Example

```kryos
use std::json

// Parse incoming JSON
let raw = "{\"user\":\"bob\",\"scores\":[95,87,73],\"active\":true}"
let data = parse(raw)

// Access fields
let user   = to_str(get(data, "user"))
let active = to_bool(get(data, "active"))
let scores = to_array(get(data, "scores"))

println(user)                      // bob
println(active)                    // true
println(to_int(scores[0]))         // 95

// Build a response object
let response = json_object(
    ["ok", "user", "count"],
    [
        json_bool(true),
        json_string(user),
        json_number(3.0)
    ]
)
println(stringify(response))
// {"ok":true,"user":"bob","count":3}

// Pretty print for debugging
println(pretty_print(data, 2))
```
