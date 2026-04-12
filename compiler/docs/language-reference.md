# Kryos Language Reference

## Variables

```kryos
let x = 42              // immutable binding
let mut y = 0           // mutable binding
y = 10                  // reassignment (only for mut)
let z: f64 = 3.14       // explicit type annotation
```

## Types

| Type | Description | Size |
|------|-------------|------|
| `i64` | Signed 64-bit integer (default integer type) | 8 bytes |
| `f64` | 64-bit floating point (default float type) | 8 bytes |
| `i8`, `i16`, `i32`, `i128` | Signed integers | 1-16 bytes |
| `u8`, `u16`, `u32`, `u64`, `u128` | Unsigned integers | 1-16 bytes |
| `f32` | 32-bit floating point | 4 bytes |
| `bool` | Boolean (`true` / `false`) | 1 byte |
| `str` | String (heap-allocated, refcounted) | pointer |
| `[T]` | Array of T (heap-allocated) | pointer |
| `void` | No value | 0 bytes |

## Functions

```kryos
fn add(a: i64, b: i64) -> i64 {
    return a + b
}

fn greet(name: str) {
    println("Hello, " + name + "!")
}

// Functions are first-class values
let f = add
println(to_string(f(3, 4)))  // 7
```

## Control Flow

```kryos
// If / elif / else
if x > 0 {
    println("positive")
} elif x == 0 {
    println("zero")
} else {
    println("negative")
}

// While loop
let mut i = 0
while i < 10 {
    i = i + 1
}

// For-in loop (over arrays)
for item in [1, 2, 3] {
    println(to_string(item))
}

// For-in with range
for i in range(0, 10) {
    println(to_string(i))
}

// Break and continue
while true {
    if done() { break }
    if skip() { continue }
}
```

## Structs

```kryos
struct Point {
    x: f64,
    y: f64
}

// Construction uses dot syntax for field access
let p = Point { x: 1.0, y: 2.0 }
println(to_string(p.x))  // 1

// Methods via impl blocks
impl Point {
    fn distance(self, other: Point) -> f64 {
        let dx = self.x - other.x
        let dy = self.y - other.y
        sqrt(dx * dx + dy * dy)
    }
}
```

### @copy Structs

By default, structs are moved on assignment. The `@copy` annotation makes a struct copyable:

```kryos
@copy
struct Vec2 {
    x: f64,
    y: f64
}

let a = Vec2 { x: 1.0, y: 2.0 }
let b = a       // deep copy — a is still valid
println(to_string(a.x))  // 1
```

## Enums

```kryos
enum Color {
    Red,
    Green,
    Blue,
}

// Enums with payloads
enum Shape {
    Circle(f64),
    Rect(f64, f64),
    Triangle(f64, f64, f64),
}

// Construction: dot syntax
let s = Shape.Circle(5.0)

// Pattern matching: double-colon syntax
match s {
    Shape::Circle(r) => println("circle r=" + to_string(r)),
    Shape::Rect(w, h) => println("rect " + to_string(w) + "x" + to_string(h)),
    Shape::Triangle(a, b, c) => println("triangle"),
}
```

## Pattern Matching

```kryos
// Match on enums
fn area(s: Shape) -> f64 {
    match s {
        Shape::Circle(r) => 3.14159 * r * r,
        Shape::Rect(w, h) => w * h,
        Shape::Triangle(a, b, c) => {
            let sp = (a + b + c) / 2.0
            sqrt(sp * (sp - a) * (sp - b) * (sp - c))
        },
    }
}

// Match on integers
match code {
    0 => println("ok"),
    1 => println("error"),
    _ => println("unknown"),
}
```

## Arrays

```kryos
let nums = [1, 2, 3, 4, 5]
let first = nums[0]
let length = len(nums)

// Mutable arrays
let mut items: [str] = []
push(items, "hello")
push(items, "world")
let last = pop(items)

// Iteration
for item in items {
    println(item)
}
```

## Strings

```kryos
let greeting = "Hello, " + "world!"
let length = len(greeting)
let sub = substr(greeting, 0, 5)       // "Hello"
let has = contains(greeting, "world")   // true
let parts = split("a,b,c", ",")        // ["a", "b", "c"]
let joined = join(parts, "-")           // "a-b-c"
let upper = to_upper("hello")          // "HELLO"
let lower = to_lower("HELLO")          // "hello"
let trimmed = trim("  hi  ")           // "hi"
let replaced = replace("foo bar", "foo", "baz")  // "baz bar"
```

## Concurrency

```kryos
// Channels for communication
let ch = chan()

// Spawn a concurrent task
spawn {
    send(ch, 42)
}

let result = recv(ch)
println(to_string(result))  // 42

// Multiple workers
let results = chan()
for i in range(0, 4) {
    spawn {
        send(results, i * i)
    }
}

let mut total = 0
for i in range(0, 4) {
    total = total + recv(results)
}
```

## Traits

```kryos
trait Describable {
    fn describe(self) -> str
}

impl Describable for Point {
    fn describe(self) -> str {
        return "(" + to_string(self.x) + ", " + to_string(self.y) + ")"
    }
}
```

## Ownership

Kryos uses an ownership system for memory safety:

- Each value has exactly one owner
- When the owner goes out of scope, the value is dropped
- Primitive types (`i64`, `f64`, `bool`, `str`) are **Copy** — assigning copies the value
- Arrays are **Copy** by handle — assigning copies the pointer (shared backing store)
- Structs are **moved** by default — assigning transfers ownership
- Structs marked `@copy` are deep-copied on assignment
- `shared` wraps a value in reference counting (ARC)

```kryos
// Copy types — both usable after assignment
let a = 42
let b = a
println(to_string(a))  // OK: i64 is Copy

// Move semantics for structs (without @copy)
struct BigData { buffer: [i64] }
let x = BigData { buffer: [1, 2, 3] }
let y = x
// println(to_string(len(x.buffer)))  // ERROR: x was moved

// Shared references
let s = shared BigData { buffer: [1, 2, 3] }
let t = s  // both s and t reference the same data
```

## Annotations

| Annotation | Target | Description |
|------------|--------|-------------|
| `@copy` | struct | Deep-copy on assignment instead of move |
| `@inline` | function | Hint to inline during optimization |
| `@pure` | function | No side effects (enables more optimizations) |
| `@test` | function | Mark as a test function |
| `@deprecated` | function | Mark as deprecated |
| `pub` | declaration | Make visible outside the module |

## Operators

| Operator | Description |
|----------|-------------|
| `+`, `-`, `*`, `/`, `%` | Arithmetic |
| `**` | Exponentiation |
| `==`, `!=`, `<`, `>`, `<=`, `>=` | Comparison |
| `and`, `or`, `not` | Logical |
| `&`, `\|`, `^`, `~` | Bitwise |
| `<<`, `>>` | Bit shift |

## Builtins

See the [README](../README.md) for the full builtin function reference.

## File I/O

```kryos
let content = file_read("input.txt")
file_write("output.txt", "Hello, file!")
```

## CLI Arguments

```kryos
fn main() {
    let argv = args()
    if len(argv) > 1 {
        println("first arg: " + argv[1])
    }
}
```

## Comments

```kryos
// Single-line comment

// Multi-line comments use multiple single-line comments
// There is no /* */ block comment syntax
```
