# json

Friendly wrappers around the Kryos built-in JSON functions, plus string-builder helpers for assembling JSON manually.

## Installation

```
kryos pkg add json
```

## Usage

```kryos
import "json"

fn main() {
    // Node-based API (uses builtins)
    let node = json_decode("{\"name\":\"Alice\",\"age\":30}")
    let name = json_as_str(json_field(node, "name"))
    let age  = json_as_int(json_field(node, "age"))
    println(name + " is " + to_string(age))

    // String-builder API (no nodes)
    let keys = ["x", "y"]
    let vals = [json_number_literal(10), json_number_literal(20)]
    let obj = json_object_literal(keys, vals)
    println(obj)  // -> {"x":10,"y":20}
}
```

## API

### Node-based (wrapping builtins)

- `fn json_decode(s: str) -> i64` — parse JSON string into a node handle
- `fn json_encode(node: i64) -> str` — serialize node to JSON string
- `fn json_obj(keys: [str], vals: [i64]) -> i64` — create object node
- `fn json_arr(items: [i64]) -> i64` — create array node
- `fn json_str(s: str) -> i64` — create string node
- `fn json_num(n: f64) -> i64` — create number node
- `fn json_bool_val(b: bool) -> i64` — create boolean node
- `fn json_null_val() -> i64` — create null node
- `fn json_field(node: i64, key: str) -> i64` — get object field
- `fn json_index(node: i64, i: i64) -> i64` — get array element
- `fn json_as_str(node: i64) -> str` — extract string value
- `fn json_as_int(node: i64) -> i64` — extract integer value
- `fn json_as_float(node: i64) -> f64` — extract float value
- `fn json_node_type(node: i64) -> str` — get node type string
- `fn json_node_len(node: i64) -> i64` — get array/object length
- `fn json_is_null_node(node: i64) -> bool` — check for null

### String-builder helpers

- `fn json_escape(s: str) -> str` — escape a string for JSON
- `fn json_string_literal(s: str) -> str` — produce `"..."` with escaping
- `fn json_number_literal(n: i64) -> str` — format integer
- `fn json_bool_literal(b: bool) -> str` — `"true"` or `"false"`
- `fn json_null_literal() -> str` — `"null"`
- `fn json_array_literal(items: [str]) -> str` — assemble `[...]`
- `fn json_object_literal(keys: [str], values: [str]) -> str` — assemble `{...}`

## License

MIT — see https://github.com/NORTHTEKDevs/kryos-registry
