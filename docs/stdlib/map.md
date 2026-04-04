# std::map

Dictionary/map data structure for key-value storage. Keys are always coerced to strings. All mutating operations return a new map (immutable style) except where noted.

```kryos
import std::map
```

---

### map_new

`map_new() -> Map`

Create an empty map.

**Example:**
```kryos
let m = map_new()
print(m)  // {}
```

**See also:** map_from

---

### map_from

`map_from(key1: Any, val1: Any, key2: Any, val2: Any, ...) -> Map`

Create a map from alternating key-value pairs. Requires an even number of arguments.

**Example:**
```kryos
let user = map_from("name", "Alice", "age", 30, "active", true)
print(map_get(user, "name"))  // Alice
```

**Edge cases:**
- Raises a runtime error if an odd number of arguments is passed.
- Keys are coerced to strings.

**See also:** map_new

---

### map_set

`map_set(m: Map, key: Any, value: Any) -> Map`

Return a new map with the key set to value. Does not mutate the original.

**Example:**
```kryos
let m = map_new()
let m2 = map_set(m, "color", "blue")
print(map_get(m2, "color"))  // blue
print(map_has(m, "color"))   // false (original unchanged)
```

**Edge cases:**
- Raises if the first argument is not a map.
- The key is coerced to a string.

**See also:** map_get, map_delete

---

### map_get

`map_get(m: Map, key: Any) -> Any`
`map_get(m: Map, key: Any, default: Any) -> Any`

Get a value by key. Returns `nil` if the key does not exist, unless a default is provided.

**Example:**
```kryos
let m = map_from("host", "localhost", "port", 8080)
print(map_get(m, "host"))           // localhost
print(map_get(m, "missing"))        // nil
print(map_get(m, "missing", 3000))  // 3000
```

**Edge cases:**
- Raises if the first argument is not a map.
- Accepts 2 or 3 arguments. Raises on any other arity.

**See also:** map_has, map_set

---

### map_has

`map_has(m: Map, key: Any) -> Bool`

Check whether a key exists in the map.

**Example:**
```kryos
let m = map_from("x", 1)
print(map_has(m, "x"))  // true
print(map_has(m, "y"))  // false
```

**See also:** map_get, map_keys

---

### map_keys

`map_keys(m: Map) -> Array`

Return all keys as an array of strings.

**Example:**
```kryos
let m = map_from("a", 1, "b", 2, "c", 3)
let keys = map_keys(m)
print(keys)  // ["a", "b", "c"]
```

**See also:** map_values

---

### map_values

`map_values(m: Map) -> Array`

Return all values as an array.

**Example:**
```kryos
let m = map_from("a", 1, "b", 2)
let vals = map_values(m)
print(vals)  // [1, 2]
```

**See also:** map_keys

---

### map_remove

`map_remove(m: Map, key: Any) -> Map`

Return a new map with the given key removed. Does not mutate the original. No-op if the key does not exist.

**Example:**
```kryos
let m = map_from("x", 1, "y", 2)
let m2 = map_remove(m, "x")
print(map_has(m2, "x"))  // false
print(map_has(m, "x"))   // true (original unchanged)
```

**See also:** map_set

---

### map_merge

`map_merge(a: Map, b: Map) -> Map`

Merge two maps. Keys in `b` overwrite keys in `a`. Returns a new map.

**Example:**
```kryos
let defaults = map_from("port", 3000, "host", "localhost")
let overrides = map_from("port", 8080)
let cfg = map_merge(defaults, overrides)
print(map_get(cfg, "port"))  // 8080
print(map_get(cfg, "host"))  // localhost
```

**Edge cases:**
- Raises if either argument is not a map.

**See also:** map_set
