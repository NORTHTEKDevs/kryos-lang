# std::json

JSON parsing, serialization, and object access utilities.

All functions in this module are available after `use std::json`. Kryos maps and arrays are JSON-compatible, so round-tripping through `json_parse` and `json_stringify` preserves structure.

---

### json_parse

```
json_parse(s: str) -> any
```

Parse a JSON string into a Kryos value. JSON objects become maps, JSON arrays become arrays, and JSON primitives become their Kryos equivalents (string, number, bool, `none`).

**Example:**

```kryos
let data = json_parse("{\"name\": \"kryos\", \"version\": 1}")
println(json_get(data, "name"))  // "kryos"
```

```kryos
let items = json_parse("[1, 2, 3]")
println(to_string(len(items)))  // 3
```

```kryos
// Parse a file's JSON content
let config = json_parse(file_read("config.json"))
let port = json_get(config, "port")
```

**Edge cases:**

- Throws a runtime error if the string is not valid JSON. The error message includes the parse position.
- JSON `null` becomes Kryos `none`.
- JSON numbers become integers or floats depending on whether they have a decimal point.

**See also:** `json_stringify`, `json_get`

---

### json_stringify

```
json_stringify(value: any) -> str
```

Convert a Kryos value to a JSON string. Maps become JSON objects, arrays become JSON arrays, and primitives convert directly.

**Example:**

```kryos
let s = json_stringify({"name": "kryos", "version": 1})
println(s)  // {"name": "kryos", "version": 1}
```

```kryos
let s = json_stringify([1, 2, 3])
println(s)  // [1, 2, 3]
```

```kryos
// Write structured data to a file
let data = {"users": [{"name": "alice"}, {"name": "bob"}]}
file_write("data.json", json_stringify(data))
```

**Edge cases:**

- Throws a runtime error if the value contains types that are not JSON-serializable (e.g., functions, custom objects without a serialization path).
- Kryos `none` becomes JSON `null`.
- Does not produce pretty-printed output. The result is compact (no extra whitespace).

**See also:** `json_parse`

---

### json_get

```
json_get(obj: map, key: str) -> any
```

Get a value from a map by key. Returns `none` if the key does not exist.

**Example:**

```kryos
let data = json_parse("{\"name\": \"kryos\", \"version\": 1}")
let name = json_get(data, "name")
println(name)  // "kryos"

let missing = json_get(data, "author")
println(to_string(missing))  // none
```

```kryos
// Nested access
let config = json_parse(file_read("config.json"))
let db = json_get(config, "database")
let host = json_get(db, "host")
```

**Edge cases:**

- Returns `none` (not an error) if the key does not exist.
- Throws a runtime error if the first argument is not a map.
- The key is converted to a string before lookup.

**See also:** `json_has`, `json_parse`

---

### json_has

```
json_has(obj: map, key: str) -> bool
```

Check whether a key exists in a map.

**Example:**

```kryos
let data = json_parse("{\"name\": \"kryos\"}")

if json_has(data, "name") {
    println("Has name: " + json_get(data, "name"))
}

if !json_has(data, "version") {
    println("No version field")
}
```

**Edge cases:**

- Returns `false` for keys that are not present, even if the map is empty.
- Throws a runtime error if the first argument is not a map.
- A key that exists with a value of `none` still returns `true`.

**See also:** `json_get`

---

## Common Patterns

### Round-trip serialization

```kryos
let original = {"key": "value", "count": 42}
let serialized = json_stringify(original)
let restored = json_parse(serialized)
println(json_get(restored, "key"))  // "value"
```

### Building a response payload

```kryos
let payload = json_stringify({
    "status": "ok",
    "data": [1, 2, 3],
    "metadata": {"count": 3}
})
respond(200, payload)
```

### Safe key access

```kryos
fn get_or_default(obj, key, default_val) {
    if json_has(obj, key) {
        return json_get(obj, key)
    }
    return default_val
}

let config = json_parse(file_read("config.json"))
let port = get_or_default(config, "port", 3000)
```
