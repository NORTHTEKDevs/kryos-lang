# Keywords Reference

All reserved keywords in the Kryos language. These identifiers cannot be used as variable, function, or type names.

## Language Keywords

| Keyword | Description | Example |
|---------|-------------|---------|
| `let` | Declare a variable binding | `let x = 42` |
| `mut` | Mark a binding as mutable | `let mut count = 0` |
| `fn` | Declare a function | `fn add(a: i32, b: i32) -> i32 { a + b }` |
| `return` | Return a value from the current function | `return x + 1` |
| `if` | Conditional branch | `if x > 0 { println("positive") }` |
| `else` | Alternate branch of an `if` | `if x > 0 { ... } else { ... }` |
| `elif` | Chained conditional branch | `if x > 0 { ... } elif x == 0 { ... }` |
| `for` | Iterate over a range or collection | `for i in range(10) { println(i) }` |
| `while` | Loop while a condition is true | `while x > 0 { x -= 1 }` |
| `in` | Used in `for ... in` loops | `for item in items { ... }` |
| `break` | Exit the innermost loop | `if done { break }` |
| `continue` | Skip to the next loop iteration | `if skip { continue }` |
| `match` | Pattern matching expression | `match value { 1 => "one", _ => "other" }` |
| `struct` | Define a product type (record) | `struct Point { x: f64, y: f64 }` |
| `enum` | Define a sum type (tagged union) | `enum Color { Red, Green, Blue }` |
| `impl` | Implement methods or a trait for a type | `impl Point { fn distance(self) -> f64 { ... } }` |
| `trait` | Define a trait (interface) | `trait Printable { fn display(self) -> str }` |
| `pub` | Mark an item as publicly visible | `pub fn greet() { println("hello") }` |
| `use` | Import a module or symbol | `use math_utils` |
| `extern` | Declare an external (FFI) binding | `extern fn c_sqrt(x: f64) -> f64` |
| `as` | Type cast or import alias | `use utils::helpers as h` |
| `mod` | Declare a sub-module | `mod networking` |
| `type` | Declare a type alias | `type Coordinate = Point` |
| `actor` | Declare an actor (concurrent entity) | `actor Counter { ... }` |
| `spawn` | Spawn an actor or parallel task | `spawn my_actor()` |
| `parallel` | Reserved keyword (parallel iteration planned for future release) | `parallel for i in range(100) { ... }` |
| `quantum` | Enter a quantum computing block | `quantum { ... }` |
| `comptime` | Execute a block at compile time | `comptime { let x = 2 ** 10 }` |
| `try` | Begin an error-handling block | `try { risky_operation() }` |
| `catch` | Handle errors from a `try` block | `try { ... } catch e { println(e) }` |
| `throw` | Raise an error | `throw "invalid input"` |
| `select` | Wait on multiple channels | `select { msg from ch => ... }` |
| `send` | Send a value through a channel | `send(ch, 42)` |
| `recv` | Receive a value from a channel | `let x = recv(ch)` |
| `ask` | Send a request and await response | Reserved for future use |
| `chan` | Create a channel | `let ch = chan()` |
| `shared` | Shared reference qualifier | Reserved for future use |
| `weak` | Weak reference qualifier | Reserved for future use |
| `move` | Move ownership explicitly | Reserved for future use |

## Logical Operators (Keywords)

| Keyword | Description | Example |
|---------|-------------|---------|
| `and` | Logical AND (short-circuit) | `if a > 0 and b > 0 { ... }` |
| `or` | Logical OR (short-circuit) | `if a == 0 or b == 0 { ... }` |
| `not` | Logical NOT | `if not done { ... }` |

## Literal Keywords

| Keyword | Description | Example |
|---------|-------------|---------|
| `true` | Boolean literal true | `let active = true` |
| `false` | Boolean literal false | `let done = false` |
| `none` | The absence-of-value literal | `let result = none` |

## Built-in Type Names

These are recognized as type identifiers by the lexer. They are not strictly reserved words (you can shadow them with variables), but doing so is strongly discouraged.

| Type | Description |
|------|-------------|
| `i8`, `i16`, `i32`, `i64`, `i128` | Signed integers (8-128 bit) |
| `u8`, `u16`, `u32`, `u64`, `u128` | Unsigned integers (8-128 bit) |
| `f32`, `f64` | Floating-point numbers (32/64 bit) |
| `bool` | Boolean (`true` / `false`) |
| `str` | UTF-8 string |
| `char` | Unicode scalar value |
| `Vec` | Growable array / vector |
| `Map` | Hash map (key-value store) |
| `Set` | Hash set (unique values) |
| `Option` | Optional value (`Some(T)` or `None`) |
| `Result` | Result type (`Ok(T)` or `Err(E)`) |
| `Tensor` | Multi-dimensional numeric array (GPU-accelerable) |
| `Secret` | Capability-gated secret value |
| `Qubit` | Single quantum bit |
| `Qureg` | Quantum register (array of qubits) |
