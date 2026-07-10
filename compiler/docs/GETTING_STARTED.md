# Getting Started with Kryos

Welcome to Kryos! This guide walks you through building your first Kryos program.

## Prerequisites

Before starting, ensure you have:
- Rust toolchain installed (https://rustup.rs/)
- Git installed
- A terminal or command prompt

## Building the Compiler

Clone the repository and navigate to the compiler directory:

```bash
cd kryos-lang/compiler
cargo build --release -j 4
```

The compiled `cargo` binary will be available as `cargo run --release`. You can now run Kryos programs.

## Hello World

Create a file called `hello.kry`:

```kryos
fn main() {
    let name = "Kryos"
    let greeting = "Hello, " + name + "!"
    println(greeting)
}
```

Run it with:

```bash
cargo run --release -j 4 -- run hello.kry
```

Expected output:
```
Hello, Kryos!
```

## Variables and Types

Kryos has two forms of variable binding:

```kryos
fn main() {
    // Immutable binding
    let x = 42
    
    // Mutable binding
    let mut counter = 0
    counter = counter + 1
    
    // With explicit type annotation
    let pi: f64 = 3.14159
    
    // Arithmetic
    let sum = 10 + 5
    let product = 6 * 7
    let quotient = 100 / 4
    
    println("sum = " + to_string(sum))
    println("product = " + to_string(product))
    println("quotient = " + to_string(quotient))
}
```

Run with:
```bash
cargo run --release -j 4 -- run variables.kry
```

Expected output:
```
sum = 15
product = 42
quotient = 25
```

## Functions

Functions are declared with `fn`, parameter types, and a return type:

```kryos
fn add(a: i64, b: i64) -> i64 {
    return a + b
}

fn multiply(x: i64, y: i64) -> i64 {
    x * y
}

fn greet(name: str) {
    println("Hello, " + name)
}

fn main() {
    let result1 = add(5, 3)
    let result2 = multiply(4, 7)
    
    println("5 + 3 = " + to_string(result1))
    println("4 * 7 = " + to_string(result2))
    
    greet("World")
}
```

Run with:
```bash
cargo run --release -j 4 -- run functions.kry
```

Expected output:
```
5 + 3 = 8
4 * 7 = 28
Hello, World
```

## Structs

Structs group related data together:

```kryos
struct Person {
    name: str,
    age: i64,
}

fn main() {
    let person = Person {
        name: "Alice",
        age: 30,
    }
    
    println("Name: " + person.name)
    println("Age: " + to_string(person.age))
}
```

Run with:
```bash
cargo run --release -j 4 -- run structs.kry
```

Expected output:
```
Name: Alice
Age: 30
```

## Enums and Pattern Matching

Enums allow you to define types with multiple variants, and pattern matching lets you handle each case:

```kryos
enum Shape {
    Circle(f64),
    Rectangle(f64, f64),
}

fn compute_area(shape: Shape) -> f64 {
    match shape {
        Shape::Circle(r) => 3.14159 * r * r,
        Shape::Rectangle(w, h) => w * h,
    }
}

fn main() {
    let circle = Shape.Circle(5.0)
    let rect = Shape.Rectangle(4.0, 6.0)
    
    let area1 = compute_area(circle)
    let area2 = compute_area(rect)
    
    println("Circle area: " + to_string(area1))
    println("Rectangle area: " + to_string(area2))
}
```

Run with:
```bash
cargo run --release -j 4 -- run shapes.kry
```

Expected output:
```
Circle area: 78.54975
Rectangle area: 24
```

## Control Flow

Kryos supports `if`, `elif`, `else`, `while`, and `for` loops:

```kryos
fn classify(x: i64) {
    if x > 0 {
        println("positive")
    } elif x == 0 {
        println("zero")
    } else {
        println("negative")
    }
}

fn main() {
    // If-elif-else
    classify(10)
    classify(0)
    classify(-5)
    
    println("")
    
    // While loop
    let mut i = 0
    while i < 3 {
        println("While: " + to_string(i))
        i = i + 1
    }
    
    println("")
    
    // For loop with range
    for i in 0..3 {
        println("For: " + to_string(i))
    }
}
```

Run with:
```bash
cargo run --release -j 4 -- run control_flow.kry
```

Expected output:
```
positive
zero
negative

While: 0
While: 1
While: 2

For: 0
For: 1
For: 2
```

## Next Steps

- See the [Language Reference](language-reference.md) for complete syntax documentation
- Check out the [Examples](../../examples/README.md) directory for more complex programs
- Study the existing examples: `calculator.kry` and `word_count.kry`

Happy coding with Kryos!
