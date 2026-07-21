# The 30-Minute Tour

A complete tour of Kryos. Every major feature, with runnable examples, in the order you'd encounter them when learning.

Read top to bottom. Every snippet is a complete program you can save as `tour.kry` and run with `kryos run tour.kry`.

---

## 1 · Variables

```kryos
fn main() {
    let x = 42                // immutable, type inferred (i64)
    let mut y = 0             // mutable
    y = y + 1
    let name: str = "Kryos"   // explicit type
    println(name + " " + to_string(x))
}
```

- `let` declares a binding. Default immutable.
- `let mut` declares a mutable binding.
- Types are inferred. You can write them out for clarity or when inference can't decide.

---

## 2 · Primitives

```kryos
fn main() {
    let a: i64 = -10
    let b: u64 = 10
    let c: f64 = 3.14
    let d: bool = true
    let e: str = "string"

    println("ints: " + to_string(a) + " " + to_string(b))
    println("float: " + to_string(c))
    println("bool: " + to_string(d))
}
```

`i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `f32`, `f64`, `bool`, `str`. Integer literals default to `i64`, float literals to `f64`.

---

## 3 · Functions

```kryos
fn square(x: i64) -> i64 {
    x * x
}

fn greet(name: str) {
    println("hello, " + name)
}

fn main() {
    println(to_string(square(7)))
    greet("world")
}
```

The last expression in a function body is the return value. Use `return` for early exit.

---

## 4 · Control flow

```kryos
fn classify(n: i64) -> str {
    if n < 0          { "negative" }
    else if n == 0    { "zero" }
    else if n < 10    { "small" }
    else              { "big" }
}

fn main() {
    let mut i = 0
    while i < 5 {
        println(classify(i))
        i = i + 1
    }

    for j in 1..=5 {
        println("for: " + to_string(j))
    }
}
```

- `if`/`else if`/`else` — expressions, return values
- `while` — standard loop
- `for x in start..end` — exclusive upper bound; `start..=end` for inclusive

---

## 5 · Collections

```kryos
fn main() {
    let xs: [i64] = [1, 2, 3, 4, 5]
    let mut total = 0
    for x in xs { total = total + x }
    println("sum = " + to_string(total))

    let mut m: map<str, i64> = {}
    m["alice"] = 1
    m["bob"] = 2
    println("alice = " + to_string(m["alice"]))
}
```

Arrays are `[T]`. Maps are `map<K, V>` with literal `{}` syntax. Both are heap-allocated and ARC-managed.

---

## 6 · Structs

```kryos
struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn new(x: f64, y: f64) -> Point {
        Point { x: x, y: y }
    }

    fn distance(self, other: Point) -> f64 {
        let dx = self.x - other.x
        let dy = self.y - other.y
        sqrt(dx * dx + dy * dy)
    }
}

fn main() {
    let a = Point::new(0.0, 0.0)
    let b = Point::new(3.0, 4.0)
    println("d = " + to_string(a.distance(b)))   // → 5.0
}
```

- `struct` declares fields.
- `impl Type { ... }` blocks group methods.
- `Type::name(args)` is the associated-function (constructor) syntax.

---

## 7 · Enums and pattern matching

```kryos
enum Shape {
    Circle(f64),
    Rectangle(f64, f64),
    Point,
}

fn area(s: Shape) -> f64 {
    match s {
        Shape::Circle(r)       => 3.14159 * r * r,
        Shape::Rectangle(w, h) => w * h,
        Shape::Point           => 0.0,
    }
}

fn main() {
    let shapes = [Shape.Circle(2.0), Shape.Rectangle(3.0, 4.0), Shape.Point]
    for s in shapes {
        println("area = " + to_string(area(s)))
    }
}
```

`match` is exhaustive — the compiler errors if you miss a variant.

---

## 8 · Traits and generics

```kryos
trait Comparable {
    fn less_than(self, other: Self) -> bool
}

struct Score { value: i64 }

impl Comparable for Score {
    fn less_than(self, other: Self) -> bool {
        self.value < other.value
    }
}

fn smallest<T: Comparable>(xs: [T]) -> T {
    let mut best = xs[0]
    for x in xs {
        if x.less_than(best) { best = x }
    }
    best
}

fn main() {
    let scores = [Score { value: 30 }, Score { value: 10 }, Score { value: 20 }]
    let s = smallest(scores)
    println("min = " + to_string(s.value))
}
```

- `Self` resolves to the implementing type inside a `trait`.
- Generics use `<T>` and constraints like `<T: Comparable>`.

---

## 9 · Closures

```kryos
fn apply(f: fn(i64) -> i64, x: i64) -> i64 {
    f(x)
}

fn main() {
    let mul = 3
    let times = |x| x * mul
    println(to_string(apply(times, 10)))   // → 30
}
```

Closures capture by ARC. They're first-class values: pass them, return them, store them.

---

## 10 · Concurrency: channels + spawn

```kryos
fn main() {
    let ch = chan()

    spawn {
        for i in 1..=5 {
            send(ch, i * 10)
        }
    }

    for _ in 1..=5 {
        println("got " + to_string(recv(ch)))
    }
}
```

- `chan()` creates a channel.
- `spawn { ... }` runs a block concurrently.
- `send(ch, val)` and `recv(ch)` are blocking.

---

## 11 · Async / await

```kryos
use std::net::{http_get}

async fn fetch_size(url: str) -> i64 {
    let resp = await http_get(url)
    len(resp.body)
}

@capabilities(net)
async fn main() {
    let s1 = await fetch_size("https://example.com")
    let s2 = await fetch_size("https://example.org")
    println("sizes: " + to_string(s1) + ", " + to_string(s2))
}
```

`async fn` returns a future. `await` suspends until ready. Under the hood the compiler lowers async functions to state machines — same model as Rust and JavaScript.

---

## 12 · Capabilities

```kryos
use std::net::{http_post}

@pure
fn add(a: i64, b: i64) -> i64 {
    a + b      // compile error if this calls file_read, http_get, etc.
}

@capabilities(io)
fn read_config(path: str) -> str {
    file_read(path)
}

@capabilities(io, net)
fn upload_logs() {
    let logs = file_read("/var/log/app.log")
    http_post("https://example.com/logs", logs, "text/plain")
}
```

`@pure` means "no side effects, ever." `@capabilities(...)` declares an allowlist of effect kinds. The compiler checks both — calling `file_read` from a `@pure` function is a compile error, not a runtime surprise.

---

## 13 · Error handling

```kryos
fn parse_port(s: str) -> i64 {
    try {
        let n = parse_int(s)
        if n < 1 or n > 65535 { throw "port out of range" }
        n
    } catch e {
        println("error: " + e)
        8080
    }
}

fn main() {
    println(to_string(parse_port("80")))      // → 80
    println(to_string(parse_port("99999")))   // → 8080 (fell through catch)
}
```

`try { ... } catch e { ... }` is the recoverable-error pattern for `throw`ed errors — but note it only catches an explicit `throw`, not a runtime panic (`parse_int` on a non-numeric string panics rather than throwing, so a malformed string like `"abc"` would abort the program instead of hitting `catch`; guard the input before parsing if it might not be numeric). For truly fatal errors, `panic("message")`.

---

## 14 · Modules

<!-- docs-example: skip -->
```kryos
// in math/geometry.kry
pub fn circle_area(r: f64) -> f64 {
    3.14159 * r * r
}
```

<!-- docs-example: skip -->
```kryos
// in main.kry
use math::geometry::circle_area

fn main() {
    println(to_string(circle_area(5.0)))
}
```

- `pub` marks an item as importable.
- `use path::to::item` imports by path.
- Filesystem layout maps directly to module paths.

---

## 15 · Packages

```bash
kryos pkg init my_project       # scaffold kryos.toml + src/
kryos pkg add http_utils ^1.0   # add a dependency
kryos pkg install               # resolve + fetch
kryos build                     # build the project (not just one file)
kryos pkg publish               # produce a tarball + registry entry
```

See [12 · Modules and Packages](../12-modules-and-packages.md) and [docs/package-registry.md](../package-registry.md) for the full registry design.

---

## 16 · FFI (calling C)

```kryos
extern "C" {
    fn sqrt(x: f64) -> f64
}

@capabilities(ffi)
fn main() {
    println(to_string(sqrt(2.0)))    // → 1.414...
}
```

Foreign functions are declared in an `extern "C" { ... }` block; calling one requires the `ffi` capability. Generate bindings from C headers with `kryos bindgen /usr/include/math.h`. See [13 · FFI](../13-ffi.md) for the full story including memory ownership across the boundary — note that only calls into the `kryos_*` runtime surface are currently linked/emitted end-to-end; arbitrary third-party C library FFI (`-l` linking of a real `extern "C"` symbol) is still landing.

---

## What's next

You've seen the whole language. From here:

- Try the **[cookbook](./cookbook/01-cli-tool.md)** — runnable recipes for real tasks.
- Browse **[examples/](../../examples)** — 74 programs covering corners not in this tour.
- Read the **[manual](../README.md)** for any topic that needs more depth.

The fastest way to internalize the language is to port something you've already written in another language. Pick a small CLI tool you wrote in Python or Go and rewrite it in Kryos. It usually takes an evening and teaches you everything.
