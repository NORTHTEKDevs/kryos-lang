# Coming From Other Languages

Quick reference for developers transitioning to Kryos from Rust, JavaScript, or C.

---

## Coming from Rust

| Rust | Kryos | Notes |
|------|-------|-------|
| `let x = 42;` | `let x = 42` | Semicolons optional |
| `let mut x = 42;` | `let mut x = 42` | Same `mut` keyword |
| `fn add(a: i32, b: i32) -> i32 {` | `fn add(a: i32, b: i32) -> i32 {` | Nearly identical |
| `println!("{}", x);` | `println(x)` | No macro, no format string needed |
| `String::from("hi")` | `"hi"` | Strings are simple, no `&str` vs `String` |
| `vec![1, 2, 3]` | `[1, 2, 3]` | Array literal syntax |
| `v.push(42);` | `push(v, 42)` | Free function |
| `match x { ... }` | `match x { ... }` | Same keyword |
| `1..10` | `1..10` | Same range syntax |
| `1..=10` | `1..=10` | Same inclusive range |
| `enum Option<T> { Some(T), None }` | `enum Option { Some(T), None }` | Built-in, no manual definition needed |
| `impl Point { ... }` | `impl Point { ... }` | Same |
| `trait Display { ... }` | `trait Display { ... }` | Same |
| `struct Point { x: f64, y: f64 }` | `struct Point { x: f64, y: f64 }` | Same |
| `use std::io;` | `use io` | Simpler module paths |
| `pub fn ...` | `pub fn ...` | Same visibility modifier |
| `#[derive(Debug)]` | No equivalent | Kryos auto-formats all types for printing |
| `&self` / `&mut self` | `self` | No borrow syntax, ownership is implicit |
| `Box<dyn Trait>` | Just use the trait | No explicit heap allocation |
| `Result<T, E>` | `Result` type built-in | Same concept, simpler syntax |
| `?` operator | `try { ... } catch { ... }` | Explicit error handling |

---

## Coming from JavaScript

| JavaScript | Kryos | Notes |
|------------|-------|-------|
| `let x = 42` | `let x = 42` | Same, but immutable by default |
| `let x = 42` (reassignable) | `let mut x = 42` | Explicit mutability |
| `const x = 42` | `let x = 42` | `let` is already immutable |
| `function add(a, b) {` | `fn add(a, b) {` | `fn` instead of `function` |
| `(a, b) => a + b` | `fn(a, b) { return a + b }` | Explicit function syntax |
| `console.log(x)` | `println(x)` | Direct replacement |
| `typeof x` | `type_of(x)` | Function, returns lowercase type name |
| `x.length` | `len(x)` | Function, not property |
| `arr.push(42)` | `push(arr, 42)` | Free function |
| `arr.pop()` | `pop(arr)` | Free function |
| `arr.map(fn)` | `map(arr, fn)` | Free function |
| `arr.filter(fn)` | `filter(arr, fn)` | Free function |
| `arr.reduce(fn, init)` | `reduce(arr, fn, init)` | Free function |
| `str.split(",")` | `split(str, ",")` | Free function |
| `arr.join(",")` | `join(",", arr)` | Free function, delimiter first |
| `JSON.parse(s)` | `json_parse(s)` | Global function |
| `JSON.stringify(x)` | `json_stringify(x)` | Global function |
| `null` / `undefined` | `none` | Single null type |
| `true` / `false` | `true` / `false` | Same |
| `&&` / `\|\|` / `!` | `and` / `or` / `not` | Word operators |
| `===` | `==` | No loose equality, `==` is strict |
| `try { } catch (e) { }` | `try { } catch e { }` | No parentheses around error |
| `class Point { ... }` | `struct Point { ... }` | Structs, not classes |
| `import { x } from "mod"` | `use mod` | All exports imported |
| `async/await` | `spawn` | Actor-based concurrency |
| `` `hello ${name}` `` | `"hello {name}"` | Same concept, different delimiters |

---

## Coming from C

| C | Kryos | Notes |
|---|-------|-------|
| `int x = 42;` | `let x: i32 = 42` | `let` binding with optional type |
| `int x = 42;` (mutable) | `let mut x: i32 = 42` | Explicit mutability |
| `int add(int a, int b) {` | `fn add(a: i32, b: i32) -> i32 {` | Return type after `->` |
| `printf("%d\n", x);` | `println(x)` | No format strings needed |
| `scanf("%d", &x);` | `let x = int(stdin_read())` | Read + parse |
| `for (int i = 0; i < 10; i++)` | `for i in range(10) {` | Range-based iteration |
| `while (x > 0) {` | `while x > 0 {` | No parentheses required |
| `if (x > 0) {` | `if x > 0 {` | No parentheses required |
| `switch (x) { case 1: ... }` | `match x { 1 => ... }` | `match` with `=>` arms |
| `struct Point { int x; int y; };` | `struct Point { x: i32, y: i32 }` | Name-first field syntax |
| `malloc` / `free` | Automatic | No manual memory management |
| `#include <stdio.h>` | `use io` | Module imports, not text inclusion |
| `typedef int Score;` | `type Score = i32` | Type alias syntax |
| `enum { RED, GREEN, BLUE }` | `enum Color { Red, Green, Blue }` | Named enum with PascalCase variants |
| `int arr[10];` | `let arr = [0; 10]` | Dynamic arrays, no fixed size |
| `strlen(s)` | `len(s)` | Same concept |
| `strcmp(a, b) == 0` | `a == b` | Direct string comparison |
| `strcat(a, b)` | `a + b` | Concatenation with `+` |
| `NULL` | `none` | Lowercase, no pointer semantics |
| `&&` / `\|\|` / `!` | `and` / `or` / `not` | Word operators |
| `int* p = &x;` | No pointers | References are implicit |
| `#define PI 3.14` | `let PI = 3.14` | Constants are just immutable bindings |
| `/* comment */` | `// comment` | Line comments (block comments also supported) |

---

## Common Patterns

### Variable declaration

```kryos
// Immutable (default)
let name = "kryos"
let count = 42
let pi = 3.14159

// Mutable
let mut score = 0
score += 10

// With type annotation
let x: i64 = 1_000_000
let items: [str] = ["a", "b", "c"]
```

### Functions

```kryos
fn greet(name: str) -> str {
    return "Hello, " + name
}

// With default parameter
fn connect(host: str, port: i32 = 8080) {
    println("Connecting to " + host + ":" + to_string(port))
}

// Lambda / inner function
fn double(x: i64) -> i64 { return x * 2 }
let result = map([1, 2, 3], double)   // [2, 4, 6]
```

### Error handling

```kryos
try {
    let data = file_read("config.toml")
    let config = json_parse(data)
} catch e {
    println("Error: " + to_string(e))
}
```

### Struct + impl

```kryos
struct Circle {
    radius: f64,
}

impl Circle {
    fn area(self) -> f64 {
        return pi() * self.radius ** 2
    }

    fn circumference(self) -> f64 {
        return 2 * pi() * self.radius
    }
}

let c = Circle { radius: 5.0 }
println(c.area())            // 78.53981633974483
```

### Pattern matching

```kryos
enum Shape {
    Circle(f64),
    Rect(f64, f64),
    Triangle(f64, f64),
}

fn area(s: Shape) -> f64 {
    match s {
        Shape::Circle(r) => pi() * r ** 2,
        Shape::Rect(w, h) => w * h,
        Shape::Triangle(b, h) => 0.5 * b * h,
    }
}
```
