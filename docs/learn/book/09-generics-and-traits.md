# 09 · Generics & Traits

After this chapter you will be able to write a function or struct that
works across multiple types without duplicating code, define a `trait` to
name a shared contract, constrain a generic parameter to only types that
satisfy one, and know exactly where dynamic dispatch (`dyn Trait`) works
and where it currently doesn't.

This chapter summarizes [`docs/08-traits-and-generics.md`](../../08-traits-and-generics.md);
read that version for generic traits (`trait Convertible<T>`), the
when-to-use-a-trait-vs-an-enum tradeoff, and default trait method bodies in
more depth than this chapter covers.

## Generic functions

A generic function takes a type parameter in angle brackets and lets the
compiler infer the concrete type at each call site:

```kryos
fn first<T>(items: [T]) -> T {
    return items[0]
}

fn main() {
    let nums: [i64] = [10, 20, 30]
    let words: [str] = ["a", "b", "c"]
    println(to_string(first(nums)))
    println(first(words))
}
```

Output:

```
10
a
```

`first<T>` compiles once per concrete `T` it's actually called with
(monomorphization, the same strategy Rust and C++ templates use) -- calling
it with `[i64]` and `[str]` in the same program produces two independent
specializations, each with the real element type, not an erased one.

## Generic structs and `impl<T>`

A struct can carry its own type parameter, and an `impl<T>` block attaches
methods that work for any `T` the struct was constructed with:

```kryos
struct Wrapper<T> {
    value: T,
}

impl<T> Wrapper<T> {
    fn get(self: Wrapper<T>) -> T {
        return self.value
    }
}

fn main() {
    let a: Wrapper<i64> = Wrapper { value: 42 }
    let b: Wrapper<str> = Wrapper { value: "hello" }
    println(to_string(a.get()))
    println(b.get())
}
```

Output:

```
42
hello
```

`Wrapper<i64>` and `Wrapper<str>` are two different monomorphized types
under the hood, but you write `impl<T> Wrapper<T>` once and both get a
correctly-typed `get()` -- `a.get()` really does return an `i64`, `b.get()`
really does return a `str`, with no manual annotation needed at either call
site.

## Traits: naming a shared contract

A `trait` lists method signatures that any implementing type must provide.
Define one, then `impl TraitName for Type` per type that satisfies it:

```kryos
trait HasArea {
    fn area(self: Self) -> f64
}

struct Circle {
    r: f64,
}

struct Square {
    side: f64,
}

impl HasArea for Circle {
    fn area(self: Circle) -> f64 {
        return 3.14159 * self.r * self.r
    }
}

impl HasArea for Square {
    fn area(self: Square) -> f64 {
        return self.side * self.side
    }
}

fn main() {
    let c: Circle = Circle { r: 2.0 }
    let s: Square = Square { side: 3.0 }
    println(to_string(c.area()))
    println(to_string(s.area()))
}
```

Output:

```
12.56636
9
```

`self: Self` in the trait body means "whatever concrete type implements
this" -- each `impl` block substitutes its own type (`Circle`, `Square`)
for `Self` in that type's implementation.

### Trait bounds constrain a generic parameter

Combine the two features: `<T: HasArea>` restricts `T` to types that
implement `HasArea`, so the function body can call `.area()` on any value
of type `T`:

```kryos
trait HasArea {
    fn area(self: Self) -> f64
}

struct Circle {
    r: f64,
}

impl HasArea for Circle {
    fn area(self: Circle) -> f64 {
        return 3.14159 * self.r * self.r
    }
}

fn total_area<T: HasArea>(shapes: [T]) -> f64 {
    let mut sum: f64 = 0.0
    for s in shapes {
        sum = sum + s.area()
    }
    return sum
}

fn main() {
    let circles: [Circle] = [Circle { r: 1.0 }, Circle { r: 2.0 }]
    println(to_string(total_area(circles)))
}
```

Output:

```
15.70795
```

Without the `: HasArea` bound, `s.area()` inside `total_area` would be a
compile error -- the checker only knows what a *bare* `T` can do (nothing)
until a bound tells it otherwise. See "Common mistakes" below for exactly
that error.

## `dyn Trait`: dynamic dispatch, and where it stops working

Every example above resolves which `area()` to call at *compile* time
(monomorphization picks the concrete type per instantiation). `dyn Trait`
is the other mode -- one value, parameter, or return type that can hold
*any* type implementing the trait, resolved at runtime through a vtable.
A single `dyn Trait` value works cleanly as a parameter:

```kryos
trait HasArea {
    fn area(self: Self) -> f64
}

struct Circle {
    r: f64,
}

struct Square {
    side: f64,
}

impl HasArea for Circle {
    fn area(self: Circle) -> f64 {
        return 3.14159 * self.r * self.r
    }
}

impl HasArea for Square {
    fn area(self: Square) -> f64 {
        return self.side * self.side
    }
}

fn print_area(shape: dyn HasArea) {
    println(to_string(shape.area()))
}

fn main() {
    print_area(Circle { r: 2.0 })
    print_area(Square { side: 3.0 })
}
```

Output:

```
12.56636
9
```

`print_area` takes one `dyn HasArea` parameter and correctly dispatches to
`Circle`'s or `Square`'s `area()` depending on what's actually passed --
this is the genuine runtime-polymorphism case a trait buys you over an
enum's closed variant set.

**The limitation:** `dyn Trait` cannot be stored *inside* a container --
an array, an `Option`, a tuple, or a map value:

```kryos
trait HasArea {
    fn area(self: Self) -> f64
}

struct Circle {
    r: f64,
}

impl HasArea for Circle {
    fn area(self: Circle) -> f64 {
        return 3.14159 * self.r * self.r
    }
}

fn main() {
    let shapes: [dyn HasArea] = [Circle { r: 2.0 }]   // ERROR: dyn Trait can't live in a container
    println(to_string(len(shapes)))
}
```

```
error[E0110]: `dyn HasArea` cannot be stored in an array yet -- trait objects in containers are unimplemented; use an enum with one variant per concrete type and `match`
 --> mistake.kry:16:18
  16 |     let shapes: [dyn HasArea] = [Circle { r: 2.0 }]
     |                  ^^^^^^^^^^^ here
```

The error names its own workaround: an enum with one tuple variant per
concrete type, `match`ed instead of dynamically dispatched. This is exactly
[Chapter 6](06-structs-and-enums.md)'s enum-wrapping pattern, applied here
to recover a "list of heterogeneous shapes" that a `[dyn HasArea]` can't
give you today:

```kryos
trait HasArea {
    fn area(self: Self) -> f64
}

struct Circle {
    r: f64,
}

struct Square {
    side: f64,
}

impl HasArea for Circle {
    fn area(self: Circle) -> f64 {
        return 3.14159 * self.r * self.r
    }
}

impl HasArea for Square {
    fn area(self: Square) -> f64 {
        return self.side * self.side
    }
}

enum Shape {
    ACircle(Circle),
    ASquare(Square),
}

fn area(s: Shape) -> f64 {
    match s {
        Shape.ACircle(c) => c.area(),
        Shape.ASquare(sq) => sq.area(),
    }
}

fn main() {
    let shapes: [Shape] = [Shape.ACircle(Circle { r: 2.0 }), Shape.ASquare(Square { side: 3.0 })]
    for s in shapes {
        println(to_string(area(s)))
    }
}
```

Output:

```
12.56636
9
```

Each variant still wraps a real struct that implements `HasArea`, so the
trait's method (`c.area()`) is callable inside each `match` arm -- you get
the trait's behavior and a heterogeneous container, just dispatched by
`match` at each use site instead of through a vtable. A **single**
`dyn Trait` value, parameter, field, return type, or `let` binding works
and dispatches correctly right now; it's specifically the container case
that isn't implemented yet.

## Common mistakes

**Calling a trait method on a bare, unbounded generic parameter.** `T`
alone gives the checker nothing to work with:

```kryos
trait HasArea {
    fn area(self: Self) -> f64
}

fn print_area<T>(shape: T) {
    println(to_string(shape.area()))   // ERROR: T has no bound, so no method is known
}

fn main() {
    println("unreachable")
}
```

```
error[E0107]: no method `area` found for type `?T5`
 --> mistake.kry:6:23
  6 |     println(to_string(shape.area()))
    |                       ^^^^^^^^^^^^ here
```

Add the bound: `fn print_area<T: HasArea>(shape: T)`.

**Trying to put `dyn Trait` in a container.** Covered in depth above --
the fix is always the same enum-and-`match` shape, not a smaller tweak to
the container declaration.

## Exercises

1. Define a `trait Describable` with one method `describe(self: Self) -> str`,
   implement it for two different structs, and write a generic function
   `announce<T: Describable>(items: [T])` that prints each one's
   description.
2. Take the `dyn HasArea` single-parameter example and add a third shape
   type (e.g. `Triangle`) with its own `impl HasArea`. Confirm
   `print_area` dispatches to it correctly with no changes to
   `print_area` itself.
3. Reproduce the `[dyn HasArea]` container error yourself, then rewrite it
   using the enum-and-`match` workaround with your `Triangle` type
   included as a third variant.

## Summary

- A generic function (`fn first<T>(items: [T]) -> T`) monomorphizes per
  concrete type at each call site -- no runtime erasure, the real type is
  known inside the specialized body.
- `impl<T> StructName<T> { }` attaches methods to a generic struct; each
  instantiation (`Wrapper<i64>`, `Wrapper<str>`) gets correctly-typed
  methods with no manual annotation.
- `trait Name { fn method(self: Self) -> T }` defines a contract;
  `impl Name for Type { }` satisfies it per concrete type.
- `<T: TraitName>` bounds a generic parameter to types implementing that
  trait -- required before the body can call the trait's methods on a bare
  `T`.
- A single `dyn Trait` value, parameter, field, or return works and
  dispatches at runtime; storing `dyn Trait` **inside a container** (array,
  `Option`, tuple, map value) is a clean `E0110` today -- use an enum with
  one variant per concrete type and `match` instead.

Next: [Ownership & ARC](10-ownership-and-arc.md)
