# 06 · Structs & Enums

After this chapter you will be able to define your own record types with
`struct`, attach behavior to them with `impl`, define a closed set of
alternatives with `enum`, and destructure an enum's data back out with
`match` -- the same triad (struct for "has these fields," enum for "is one
of these shapes," `impl` for "does these things") that replaces
inheritance-based class hierarchies in Kryos.

This chapter summarizes [`docs/05-structs-and-enums.md`](../../05-structs-and-enums.md);
read that version for the exhaustive grammar (nested-struct field chains,
the full `impl` block syntax) once this chapter's mental model is solid.

## Defining a struct

A `struct` names a record type and its fields, each with a required type
annotation:

```kryos
struct Rectangle {
    width: f64,
    height: f64,
}
```

Construct one with a struct literal -- every field must be given a value,
in any order, by name:

```kryos
struct Rectangle {
    width: f64,
    height: f64,
}

fn main() {
    let r: Rectangle = Rectangle { width: 3.0, height: 4.0 }
    println("width: " + to_string(r.width))
    println("height: " + to_string(r.height))
}
```

Output:

```
width: 3
height: 4
```

There is no default-value mechanism -- leaving a field out is a compile
error, not a zero-initialized field:

```kryos
struct Rectangle {
    width: f64,
    height: f64,
}

fn main() {
    let r: Rectangle = Rectangle { width: 3.0 }   // ERROR: missing field `height`
    println(to_string(r.width))
}
```

```
error[E0100]: missing field `height` in `Rectangle` literal -- every field must be initialized (Kryos has no default field values)
 --> mistake.kry:6:20
  6 |     let r: Rectangle = Rectangle { width: 3.0 }
    |                        ^^^^^^^^^^^^^^^^^^^^^^^^ here
```

A field mutates through a `let` binding the same way an array element
does -- `r.width = 5.0` is legal even though `r` itself was declared with
plain `let`, not `let mut`. What's locked is the *whole binding*
(`r = Rectangle { ... }` again would need `let mut r`), not the fields
inside it. [Chapter 4](04-functions.md) covers this borrow/mutate-through
distinction in depth for the parameter case; it applies identically to a
local struct variable.

## `impl`: attaching methods

Methods live in a separate `impl` block, not inside the struct body. The
first parameter is always named `self`, typed explicitly as the struct
(there is no bare `self` shorthand and no `&self` reference syntax):

```kryos
struct Rectangle {
    width: f64,
    height: f64,
}

impl Rectangle {
    fn area(self: Rectangle) -> f64 {
        return self.width * self.height
    }

    fn scaled(self: Rectangle, factor: f64) -> Rectangle {
        return Rectangle { width: self.width * factor, height: self.height * factor }
    }
}

fn main() {
    let r: Rectangle = Rectangle { width: 3.0, height: 4.0 }
    println("area: " + to_string(r.area()))
    let bigger: Rectangle = r.scaled(2.0)
    println("bigger area: " + to_string(bigger.area()))
}
```

Output:

```
area: 12
bigger area: 48
```

Call a method with dot notation (`r.area()`) -- `self` is filled in from
the receiver automatically, you never pass `r` a second time. `scaled`
follows the same immutable-value pattern [Chapter 4](04-functions.md)
established for functions: instead of mutating `r` in place, it builds and
returns a new `Rectangle`, leaving the original untouched.

## Enums: a closed set of shapes

An `enum` lists a fixed set of named variants. A variant with no data is
just a bare name; a variant that carries data is a **tuple variant** --
positional fields in parentheses, the same shape as a tuple:

```kryos
enum Event {
    Login(str),
    Logout(str),
    Error(str, i64),
}
```

`Login` and `Logout` each carry one `str` (the username); `Error` carries
two fields, a message and a code. There is no named-field variant syntax
(`Error { msg: str, code: i64 }`) -- Kryos deliberately keeps variants
positional-only, so the grammar and every `match` pattern that destructures
one stay uniform between structs (named fields, matched by `.field`) and
enums (positional payloads, matched by position). If you want a variant's
payload to have named fields, wrap them in a struct and give the variant a
single tuple slot for it:

```kryos
struct ErrorInfo {
    msg: str,
    code: i64,
}

enum Event {
    Login(str),
    Error(ErrorInfo),
}

fn main() {
    let e: Event = Event.Error(ErrorInfo { msg: "timeout", code: 504 })
    match e {
        Event.Login(user) => println(user + " logged in"),
        Event.Error(info) => println("error " + to_string(info.code) + ": " + info.msg),
    }
}
```

Output:

```
error 504: timeout
```

Trying the struct-style shorthand directly on the enum gets a clean,
specific error rather than a confusing parse failure:

```kryos
enum Shape {
    Circle { r: f64 },   // ERROR: struct-style variants aren't supported
    Square(f64),
}

fn main() {
    println("unreachable")
}
```

```
error[E0009]: struct-style enum variants (`Variant { field: Type }`) are not supported; use a tuple variant like `Variant(Type, ...)`
 --> mistake.kry:2:12
  2 |     Circle { r: f64 },
    |            ^ here
```

## Matching an enum's data back out

`match` destructures a tuple variant's payload into named bindings, one per
position -- the same tuple-pattern mechanics [Chapter 5](05-control-flow.md)
covered for plain tuples, applied to an enum's payload instead:

```kryos
enum Event {
    Login(str),
    Logout(str),
    Error(str, i64),
}

fn describe(e: Event) -> str {
    match e {
        Event.Login(user) => user + " logged in",
        Event.Logout(user) => user + " logged out",
        Event.Error(msg, code) => "error " + to_string(code) + ": " + msg,
    }
}

fn main() {
    let events: [Event] = [Event.Login("ada"), Event.Error("timeout", 504), Event.Logout("ada")]
    for e in events {
        println(describe(e))
    }
}
```

Output:

```
ada logged in
error 504: timeout
ada logged out
```

`Event.Error(msg, code)` binds `msg` and `code` from the two positional
fields, in order -- there's no way to bind them by name, since tuple
variants don't have names to bind. This `match` is exhaustive: leaving out
`Event.Logout` would be a compile error (see [Chapter 5](05-control-flow.md)
for the exhaustiveness rule and the `_` wildcard escape hatch).

## Common mistakes

**A bare variant name resolves to whichever enum declared it first --
silently, with no error, if more than one imported enum shares a variant
name.** Match patterns always know their subject's type and resolve
correctly (see "Matching an enum's data back out" above), but a bare
variant used as a plain *expression* has no such context:

```kryos
enum TrafficLight {
    Red,
    Yellow,
    Green,
}

enum Wine {
    Red,
    White,
    Rose,
}

fn main() {
    let c = Red   // means TrafficLight.Red -- Wine was declared second
    println(to_string(c == TrafficLight.Red))
}
```

Output:

```
true
```

If you meant `Wine.Red`, this compiles clean and gives you `TrafficLight.Red`
instead -- `TrafficLight` merely happens to be the enum declared (or
imported) first. There is no diagnostic, because as far as the checker is
concerned `Red` unambiguously resolved to *something* real. The fix is to
never leave a shared variant name bare once a second enum defines it --
qualify both sides:

```kryos
enum TrafficLight {
    Red,
    Yellow,
    Green,
}

enum Wine {
    Red,
    White,
    Rose,
}

fn main() {
    let light: TrafficLight = TrafficLight.Red
    let glass: Wine = Wine.Red
    println(to_string(light == TrafficLight.Red))
    println(to_string(glass == Wine.Red))
}
```

Output:

```
true
true
```

**Forgetting a field in a struct literal.** Covered above -- every field is
required, and the error names the exact one you left out
(`E0100: missing field ...`).

## Exercises

1. Define a `struct Circle { r: f64 }` with an `impl` block containing an
   `area` method (`3.14159 * r * r`). Construct one with `r: 2.0` and print
   its area.
2. Define an `enum Shape` with tuple variants `Circle(f64)` and
   `Rect(f64, f64)`, write a function that `match`es a `Shape` and returns
   its area, and call it on one of each variant.
3. Declare two enums that both have a `Pending` variant. Write a bare
   `let status = Pending` and predict which enum it resolves to *before*
   running `kryos check` -- then confirm by comparing against both
   qualified forms.

## Summary

- `struct` fields need type annotations and every field is required at
  construction -- there's no default-value mechanism.
- Mutating through a field (`r.width = 5.0`) works on a plain `let`
  binding; reassigning the whole variable needs `let mut`.
- `impl Type { }` attaches methods; the first parameter is always
  `self: TypeName`, called with dot notation (`r.area()`).
- Enum variants are tuple-style only (`Error(str, i64)`), never
  struct-style (`Error { msg: str }`) -- wrap named data in a struct and
  give the variant one tuple slot if you want field names.
- `match` destructures a tuple variant's payload into named bindings by
  position, and is exhaustive over an enum's full variant set.
- A bare, unqualified nullary variant (`Red`, not `TrafficLight.Red`) that
  exists in more than one enum silently resolves to whichever enum
  declared it first, with no diagnostic -- qualify it once a second enum
  shares the name.

Next: [Collections](07-collections.md)
