# Kryos cheatsheet

Syntax at a glance, one screenful. For deeper detail, link to the language reference at the bottom.

## Variables

```kryos
let x: i64 = 42              // immutable
let mut y: i64 = 10          // mutable
let s = "hi"                 // inferred
const PI: f64 = 3.14         // compile-time constant
```

## Types

| Kryos     | What                                          |
|-----------|-----------------------------------------------|
| `i64`     | 64-bit signed int (default)                   |
| `f64`     | 64-bit float                                  |
| `bool`    | true / false                                  |
| `str`     | UTF-8 owned string                            |
| `[T]`     | Dynamic array                                 |
| `(A, B)`  | Tuple                                         |
| `Option<T>` | `Some(x)` or `None()` (from `std::option`)   |
| `Result<T, E>` | `Ok(x)` or `Err(e)` (from `std::result`)  |

## Control flow

```kryos
if x > 0 { ... } elif x == 0 { ... } else { ... }

while i < 10 { i = i + 1 }

for item in arr { println(to_string(item)) }

loop {
    if done { break }
    if skip { continue }
}

match v {
    Some(x) => return x,
    None()  => return 0,
}
```

## Functions

```kryos
fn add(a: i64, b: i64) -> i64 {
    return a + b
}

/// Doc comment — pulled by `kryos doc`.
fn greet(name: str) -> str { return "Hello, " + name }
```

## Structs and enums

```kryos
struct Point { x: f64, y: f64 }

impl Point {
    fn distance(self: Point, other: Point) -> f64 {
        let dx = self.x - other.x
        let dy = self.y - other.y
        return sqrt(dx * dx + dy * dy)
    }
}

enum Shape {
    Circle(f64),
    Rect(f64, f64),
    Empty,
}
```

## Strings

```kryos
use std::string::{split_lines}
let n = 42
let msg = "n = " + to_string(n)             // no interpolation; use +
let parts = split_lines(file_read("x.txt"))
let has_e = contains("hello", "e")
```

## Arrays

```kryos
let arr: [i64] = [1, 2, 3]
let arr = push(arr, 4)                  // returns new array
let first = arr[0]
let n = len(arr)
for x in arr { ... }
```

## Errors

```kryos
fn divide(a: i64, b: i64) -> Result<i64, str> {
    if b == 0 { return Err("div by zero") }
    return Ok(a / b)
}

match divide(10, 2) {
    Ok(v)  => println("got " + to_string(v)),
    Err(e) => println("error: " + e),
}
```

## Imports

```kryos
use std::json::{parse, get, to_str}
use std::re::{is_match, find, replace_all}
use std::hash::{fnv1a64}
```

## Capabilities

```kryos
@pure
fn add(a: i64, b: i64) -> i64 { return a + b }   // no I/O, no net

@capabilities(io)
fn save(s: str) { file_write("out.txt", s) }

@capabilities(io, net)
fn fetch_and_log(url: str) { ... }
```

## Concurrency

```kryos
fn main() {
    let ch = chan()
    spawn { send(ch, 42) }
    let v = recv(ch)
    println(to_string(v))
}
```

## Async

<!-- docs-example: skip -->
```kryos
use std::net::{http_get}

async fn fetch(url: str) -> str {
    let resp = await http_get(url)
    return resp.body
}

@capabilities(net)
fn main() {
    let a = fetch("https://a.example")
    let b = fetch("https://b.example")
    println(await a)
    println(await b)
}
```

## Tooling

```bash
kryos run hello.kry              # JIT (Cranelift)
kryos build hello.kry            # AOT debug (Cranelift)
kryos build --release hello.kry  # AOT optimized (LLVM)
kryos build --backend wasm hello.kry
kryos check hello.kry            # type-check only, fast
kryos fmt hello.kry              # format in place
kryos test                       # run tests/
kryos explain E0300              # explain error code
kryos doc                        # generate HTML docs
kryos lsp                        # language server (stdio)
```

## More

- [Common errors](./common-errors.md)
- [Cookbook](./README.md)
- [Language reference](../19-language-reference.md)
