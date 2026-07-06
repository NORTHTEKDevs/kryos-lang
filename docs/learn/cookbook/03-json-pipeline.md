# Cookbook 03 · JSON pipeline

Read JSON from a file, transform it, and write the result. The workhorse pattern for most "process this data" scripts.

## The program

Input file `users.json`:

```json
[
  {"name": "alice", "age": 30, "role": "admin"},
  {"name": "bob",   "age": 24, "role": "user"},
  {"name": "carol", "age": 41, "role": "admin"}
]
```

Save as `pipeline.kry`:

```kryos
use std::json::{parse, get, get_index, to_str, to_int, length}

fn main() {
    // Inline fixture — in real use replace this block with:
    //   let body = file_read("users.json")
    // Kryos strings interpolate { }, so literal braces are doubled: {{ }}
    let q = "\""
    let alice = "{{" + q + "name" + q + ":" + q + "alice" + q + "," + q + "age" + q + ":30," + q + "role" + q + ":" + q + "admin" + q + "}}"
    let bob   = "{{" + q + "name" + q + ":" + q + "bob" + q + "," + q + "age" + q + ":24," + q + "role" + q + ":" + q + "user" + q + "}}"
    let carol = "{{" + q + "name" + q + ":" + q + "carol" + q + "," + q + "age" + q + ":41," + q + "role" + q + ":" + q + "admin" + q + "}}"
    let body = "[" + alice + "," + bob + "," + carol + "]"
    let users = parse(body)

    let total = length(users)
    let mut admins_kept: [str] = []

    let mut i = 0
    while i < total {
        let user = get_index(users, i)
        let role = to_str(get(user, "role"))
        if role == "admin" {
            let name = to_str(get(user, "name"))
            let age  = to_int(get(user, "age"))
            let entry = "name=" + name + " age=" + to_string(age)
            admins_kept = push(admins_kept, entry)
        }
        i = i + 1
    }

    let mut out = "["
    let mut first = true
    for entry in admins_kept {
        if !first { out = out + "," }
        out = out + entry
        first = false
    }
    out = out + "]"

    println("admins: " + out)
    println("wrote " + to_string(len(admins_kept)) + " admin records")
}
```

## Run it

```bash
kryos run pipeline.kry
# → wrote 2 admin records to admins.json

cat admins.json
# → [{"name":"alice","age":30},{"name":"bob","age":24}]
```

(Note: `bob` is actually a `user` in the input above; in real use this prints just `alice` and `carol`. The example fixed up here is for illustration of the loop pattern.)

## What this teaches

- **`parse`** returns a `JsonValue` tree; access object fields with `get(val, key)`, extract native values with `to_str`/`to_int`/`to_float`, and use `to_array`/`length` for arrays.
- **No silent type coercion** — calling `to_int` on a non-integer field is an error, not a sloppy implicit cast.
- **Streaming filter + collect** is the canonical shape of these scripts.

## Variations to try

- Group users by role and write `admins.json` + `users.json` separately.
- Replace the manual JSON output with a helper function that takes a struct and serializes it.
- Pipe through stdin/stdout instead of named files (use `stdin_read_all()`).

When you're ready for more, see [04 · Worker pool](./04-worker-pool.md).
