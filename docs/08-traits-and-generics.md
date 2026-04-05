# Traits and Generics

> **Implementation Status:** Traits are parsed, type-checked, and lowered through MIR. Trait methods are name-mangled as `TypeName_methodName`. `impl TraitName for Type` blocks register method implementations. Generics are parsed (with trait bounds) and compiled via monomorphization -- each unique type combination produces a specialized function. Default trait methods are supported. Dynamic dispatch (trait objects / `dyn Trait`) is not yet implemented.

Traits define shared behavior. If two types can both be printed, sorted, or serialized, a trait is how you express that contract. Generics let you write functions and structs that work across multiple types without duplicating code. Together they give you polymorphism without inheritance.

## Defining a Trait

A trait is a named collection of method signatures. Types that implement the trait must provide all the methods listed.

```
trait Printable {
    fn to_display(self: Self) -> str
}
```

The trait body contains `fn` declarations. These can have bodies (default implementations) or be left without a body to force each implementor to provide their own.

```
trait Summarizable {
    fn summary(self: Self) -> str

    fn headline(self: Self) -> str {
        return "Item: " + self.summary()
    }
}
```

Here `summary` has no body -- every type implementing `Summarizable` must define it. `headline` has a default body that implementors inherit unless they override it.

## Implementing a Trait

Use `impl TraitName for Type` to implement a trait for a specific type.

```
struct Article {
    title: str,
    body: str,
    word_count: i32
}

impl Printable for Article {
    fn to_display(self: Article) -> str {
        return self.title + " (" + to_string(self.word_count) + " words)"
    }
}

impl Summarizable for Article {
    fn summary(self: Article) -> str {
        return self.title
    }
}
```

After this, any `Article` instance has `.to_display()` and `.summary()` available as methods, plus the default `.headline()` from the trait.

```
let a = Article { title: "Kryos Traits", body: "...", word_count: 1200 }
println(a.to_display())    // Kryos Traits (1200 words)
println(a.headline())      // Item: Kryos Traits
```

## Inherent Implementations

You can also add methods to a type without a trait -- just `impl Type`:

```
impl Article {
    fn is_long(self: Article) -> bool {
        return self.word_count > 1000
    }
}
```

This adds `.is_long()` to `Article` without requiring a trait contract. Use inherent impls for behavior specific to one type. Use traits when multiple types share the same interface.

## Generic Functions

A generic function works with any type that satisfies its constraints. Generic type parameters go in angle brackets after the function name.

```
fn first<T>(items: [T]) -> T {
    return items[0]
}

let x = first([10, 20, 30])       // x is 10
let s = first(["a", "b", "c"])    // s is "a"
```

The `<T>` declares a type parameter. The compiler infers `T` from the arguments at each call site.

### Multiple Type Parameters

```
fn pair<A, B>(left: A, right: B) -> [str] {
    return [to_string(left), to_string(right)]
}
```

### Trait Bounds

Constrain a generic type to only accept types that implement specific traits. Bounds go after a colon:

```
fn print_all<T: Printable>(items: [T]) {
    for item in items {
        println(item.to_display())
    }
}
```

This says: `T` can be any type, as long as it implements `Printable`. Calling `print_all` with a type that lacks the trait is a compile error.

Multiple bounds use `+`:

```
fn process<T: Printable + Summarizable>(item: T) {
    println(item.to_display())
    println(item.summary())
}
```

## Generic Structs

Structs can also be generic:

```
struct Pair<A, B> {
    first: A,
    second: B
}

let p = Pair { first: 42, second: "hello" }
```

And generic enums:

```
enum Option<T> {
    Some(T),
    None
}
```

## Generic Traits

Traits themselves can take type parameters:

```
trait Convertible<T> {
    fn convert(self: Self) -> T
}

impl Convertible<str> for Article {
    fn convert(self: Article) -> str {
        return self.title + ": " + self.body
    }
}
```

## When to Use Traits vs Enums

Both traits and enums let you handle "one of several types." The tradeoff:

**Use an enum** when you know all the variants up front and they are closed -- no one adds new variants after you write the code. Enums work well with `match`.

```
enum Shape {
    Circle(f64),
    Rectangle(f64, f64)
}

fn area(s: Shape) -> f64 {
    match s {
        Shape.Circle(r) => 3.14159 * r * r,
        Shape.Rectangle(w, h) => w * h
    }
}
```

**Use a trait** when the set of types is open -- you want anyone to add new types that satisfy the contract. Traits work well for library interfaces.

```
trait Drawable {
    fn draw(self: Self) -> str
}

// Users can impl Drawable for their own types
// without modifying your code
```

Rule of thumb: if you can list every variant, use an enum. If the set grows over time, use a trait.

## Common Trait Patterns

### Printable

The `Printable` trait is the convention for custom display formatting:

```
trait Printable {
    fn to_display(self: Self) -> str
}

struct Color {
    r: i32,
    g: i32,
    b: i32
}

impl Printable for Color {
    fn to_display(self: Color) -> str {
        return "rgb(" + to_string(self.r) + ", " + to_string(self.g) + ", " + to_string(self.b) + ")"
    }
}
```

### Comparable

For types that have a natural ordering:

```
trait Comparable {
    fn compare(self: Self, other: Self) -> i32
}

impl Comparable for Article {
    fn compare(self: Article, other: Article) -> i32 {
        if self.word_count < other.word_count {
            return -1
        } elif self.word_count > other.word_count {
            return 1
        }
        return 0
    }
}
```

### Serializable

For types that can convert to and from a string representation:

```
trait Serializable {
    fn serialize(self: Self) -> str
}

impl Serializable for Color {
    fn serialize(self: Color) -> str {
        return to_string(self.r) + "," + to_string(self.g) + "," + to_string(self.b)
    }
}
```

## Coming from Python

If you know Python, traits replace Abstract Base Classes (`abc.ABC` and `@abstractmethod`).

| Python | Kryos |
|--------|-------|
| `class Drawable(ABC):` | `trait Drawable { }` |
| `@abstractmethod def draw(self): ...` | `fn draw(self: Self) -> str` |
| `class Circle(Drawable):` | `impl Drawable for Circle { }` |
| `isinstance(x, Drawable)` | Compile-time trait bounds |
| Duck typing | Explicit trait implementation |

The key difference: Python uses duck typing (if it has a `.draw()` method, it counts), while Kryos requires you to explicitly write `impl Drawable for Circle`. This catches errors at compile time rather than runtime. You always know exactly which types implement which traits.

## Coming from Rust

Kryos traits are directly inspired by Rust traits. The mental model is nearly identical.

| Rust | Kryos |
|------|-------|
| `trait Display { fn fmt(&self, ...) }` | `trait Printable { fn to_display(self: Self) -> str }` |
| `impl Display for Point { }` | `impl Printable for Point { }` |
| `fn print<T: Display>(x: T)` | `fn print<T: Printable>(x: T)` |
| `T: Display + Debug` | `T: Printable + Serializable` |
| `where T: Display` | Bounds on the parameter directly |

Differences from Rust:
- Kryos uses `self: Self` (or `self: TypeName`) in trait method signatures rather than `&self`
- No `dyn Trait` -- Kryos handles dispatch at the interpreter/codegen level
- No orphan rules yet -- you can implement any trait for any type in any module
- No lifetime parameters -- Kryos does not have a borrow checker
