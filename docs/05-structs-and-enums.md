# Structs and Enums

Kryos has two ways to define custom data types: structs for grouping fields together, and enums for representing a value that can be one of several variants. Together they replace classes from object-oriented languages. There is no inheritance -- you compose behavior through impl blocks and traits.

## Structs

A struct declares a named type with typed fields.

```
struct Point {
    x: i32,
    y: i32
}
```

Each field has a name and a type annotation. Fields are separated by commas.

### Struct literals

Create an instance by naming the struct and providing values for every field:

```
let p = Point { x: 2, y: 3 }
```

All fields are required. There is no default-value mechanism yet -- every field must be explicitly initialized.

### Field access

Use dot notation to read a field:

```
println(p.x)   // 2
println(p.y)   // 3
```

### Passing structs to functions

Structs are non-Copy types. When you pass a struct to a function, ownership moves into the function. See the [Ownership](06-ownership.md) chapter for details.

```
struct Point {
    x: i32,
    y: i32
}

fn manhattan(p: Point) -> i32 {
    return abs(p.x) + abs(p.y)
}

let p = Point { x: 2, y: 3 }
println(manhattan(p))  // 5
// p is moved -- using it again here would be a compile error
```

### Real-world example: 3D vectors

From the demo program:

```
struct Vector3 {
    x: f64,
    y: f64,
    z: f64
}

fn dot(a: Vector3, b: Vector3) -> f64 {
    return a.x * b.x + a.y * b.y + a.z * b.z
}

fn magnitude(v: Vector3) -> f64 {
    return sqrt(dot(v, v))
}

let v1 = Vector3 { x: 1.0, y: 2.0, z: 3.0 }
let v2 = Vector3 { x: 4.0, y: 5.0, z: 6.0 }
println("Dot product: " + to_string(dot(v1, v2)))
println("Magnitude: " + to_string(magnitude(v1)))
```

## Impl blocks

Methods are attached to a struct through an `impl` block. The first parameter of a method is conventionally named `self` and typed as the struct:

```
struct MathHelper {
    pi: f64
}

impl MathHelper {
    fn circle_area(self: MathHelper, r: f64) -> f64 {
        return self.pi * r * r
    }
}

let math = MathHelper { pi: 3.1413 }
println(math.circle_area(5.0))  // 78.53250000000001
```

The self parameter uses the convention `self: TypeName`. You access the struct's fields through `self.field_name` inside the method body.

Methods are called with dot notation on an instance: `math.circle_area(5.0)`. The instance is automatically passed as the first argument -- you do not write `MathHelper.circle_area(math, 5.0)`.

### Multiple methods

An impl block can contain any number of methods:

```
struct Rect {
    width: f64,
    height: f64
}

impl Rect {
    fn area(self: Rect) -> f64 {
        return self.width * self.height
    }

    fn perimeter(self: Rect) -> f64 {
        return 2.0 * (self.width + self.height)
    }

    fn is_square(self: Rect) -> bool {
        return self.width == self.height
    }
}
```

## Enums

An enum defines a type that can be one of several named variants.

### Simple enums

The simplest form lists variant names with no associated data:

```
enum Color {
    Red,
    Green,
    Blue
}
```

### Accessing variants

Use **dot notation** to access a variant:

```
let c = Color.Red
println(c)              // Color::Red
println(Color.Red == Color.Red)  // true
```

Note: variants are accessed with `.` (dot), not `::`. The display representation prints as `Color::Red` for clarity, but the syntax you write is `Color.Red`.

### Enums with associated data

Variants can carry data:

```
enum Shape {
    Circle(f64),
    Rect(f64, f64)
}
```

`Circle` carries one `f64` (the radius). `Rect` carries two `f64` values (width and height). Variants without data and variants with data can be mixed in the same enum.

## Pattern matching with `match`

The `match` expression tests a value against a series of patterns. Each arm has a pattern, an arrow `=>`, and a result expression:

```
let x = match 42 {
    1 => "one",
    42 => "answer",
    _ => "other",
}
println(x)  // answer
```

The underscore `_` is the wildcard pattern -- it matches anything and serves as the default case.

`match` can be used as an expression (returns a value) or as a statement:

```
match "hello" {
    "bye" => println("bye"),
    "hello" => println("greeting"),
    _ => println("unknown"),
}
```

### Matching for control flow

```
let status = match 404 {
    200 => "ok",
    404 => "not found",
    500 => "error",
    _ => "unknown",
}
println(status)  // not found
```

### Matching on enums

When matching on an enum value, use bare variant names -- not fully qualified paths:

```
enum Color {
    Red,
    Green,
    Blue,
}

let c = Color.Red
let tag = match c {
    Red => 1,
    Green => 2,
    Blue => 3,
}
println(tag)  // 1
```

The compiler knows the subject of the match is a `Color`, so it resolves `Red`, `Green`, and `Blue` against `Color`'s variant list. You don't need to write `Color.Red =>` in the arm.

The wildcard `_` works as a catch-all for enum matches too:

```
let is_red = match c {
    Red => true,
    _ => false,
}
```

Match is an expression -- it returns the value of the matched arm. All arms must produce the same type.

## Struct and enum composition

Structs and enums work well together. A common pattern is a struct that provides methods operating on enum variants:

```
enum Shape {
    Circle(f64),
    Rect(f64, f64)
}

struct MathHelper {
    pi: f64
}

impl MathHelper {
    fn circle_area(self: MathHelper, r: f64) -> f64 {
        return self.pi * r * r
    }
}

let math = MathHelper { pi: 3.1413 }
println(math.circle_area(5.0))  // 78.53250000000001
```

This is the recommended pattern for grouping related functions with shared constants or configuration -- attach them to a helper struct through an impl block instead of using global functions.

## Nested structs

Structs can contain other structs as fields:

```
struct Address {
    street: str,
    city: str
}

struct Person {
    name: str,
    home: Address
}

let p = Person {
    name: "Alice",
    home: Address { street: "123 Main", city: "Juneau" }
}
println(p.home.city)  // Juneau
```

Chain dots to access deeply nested fields.

## Coming from Rust

Kryos structs and enums are modeled after Rust, with a few simplifications:

| Rust | Kryos |
|------|-------|
| `self: &Self` or `&self` | `self: TypeName` |
| `Color::Red` | `Color.Red` |
| `impl Point { fn new(...) -> Self }` | Same, but use concrete type name instead of `Self` |
| Lifetime annotations | Not needed -- simpler ownership model |

The two things that will trip you up:

1. **Self parameter**: Write `self: Point`, not `&self` or `self: &Point`. Kryos does not have reference syntax in method signatures.
2. **Enum variant access**: Write `Color.Red`, not `Color::Red`. The `::` syntax does not exist in Kryos.

## Common mistakes

**Using `::` for enum variants**

```
// Wrong
let c = Color::Red

// Right
let c = Color.Red
```

Kryos uses `.` for everything -- field access, method calls, and enum variant access. There is no `::` operator.

**Forgetting to provide all fields**

```
struct Point {
    x: i32,
    y: i32
}

// Wrong -- missing y
let p = Point { x: 1 }

// Right
let p = Point { x: 1, y: 0 }
```

**Trying to use a struct after passing it to a function**

```
let p = Point { x: 2, y: 3 }
println(manhattan(p))
// p has been moved -- this is a compile error:
// println(manhattan(p))
```

Structs are non-Copy. Passing one to a function transfers ownership. See the [Ownership](06-ownership.md) chapter for how to work with this.
