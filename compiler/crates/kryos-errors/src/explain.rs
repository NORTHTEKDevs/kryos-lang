//! Long-form explanations for diagnostic error codes.
//!
//! Each entry is a paragraph-length article suitable for `kryos explain ERRXXXX`,
//! modeled after `rustc --explain`. Explanations describe:
//!   - What the error means.
//!   - A short broken example.
//!   - A short fixed example.
//!   - Why the rule exists.
//!
//! When you add or rename an error code in `codes.rs`, add or update its entry
//! here too. The `explain()` function returns `None` for codes without a
//! long-form article — those still print the short diagnostic in compiler
//! output, they just lack a `--explain` page.

/// Look up the long-form explanation for an error code. Code lookup is
/// case-insensitive; the leading `E` is required.
pub fn explain(code: &str) -> Option<&'static str> {
    let normalized = code.trim().to_ascii_uppercase();
    match normalized.as_str() {
        "E0001" => Some(E0001),
        "E0002" => Some(E0002),
        "E0003" => Some(E0003),
        "E0004" => Some(E0004),
        "E0009" => Some(E0009),
        "E0010" => Some(E0010),
        "E0200" => Some(E0200),
        "E0201" => Some(E0201),
        "E0202" => Some(E0202),
        "E0203" => Some(E0203),
        "E0204" => Some(E0204),
        "E0205" => Some(E0205),
        "E0110" => Some(E0110),
        "E0111" => Some(E0111),
        "E0112" => Some(E0112),
        "E0113" => Some(E0113),
        "E0100" => Some(E0100),
        "E0101" => Some(E0101),
        "E0102" => Some(E0102),
        "E0103" => Some(E0103),
        "E0104" => Some(E0104),
        "E0105" => Some(E0105),
        "E0106" => Some(E0106),
        "E0107" => Some(E0107),
        "E0108" => Some(E0108),
        "E0109" => Some(E0109),
        "E0300" => Some(E0300),
        "E0301" => Some(E0301),
        "E0302" => Some(E0302),
        "E0303" => Some(E0303),
        "W0300" => Some(W0300),
        "E0400" => Some(E0400),
        "E0401" => Some(E0401),
        "E0500" => Some(E0500),
        "E0501" => Some(E0501),
        "E0502" => Some(E0502),
        "E0503" => Some(E0503),
        "E0504" => Some(E0504),
        "E0505" => Some(E0505),
        "E0506" => Some(E0506),
        "E0507" => Some(E0507),
        "E0508" => Some(E0508),
        "W0400" => Some(W0400),
        "W0500" => Some(W0500),
        _ => None,
    }
}

/// Iterate over every (code, summary) pair that has a long-form explanation.
/// Used by `kryos explain --list` to print an index.
pub fn list() -> Vec<(&'static str, &'static str)> {
    vec![
        ("E0001", "unexpected token"),
        ("E0002", "expected identifier"),
        ("E0003", "expected expression"),
        ("E0004", "expected type"),
        ("E0009", "syntax error"),
        ("E0010", "program nesting too deep"),
        ("E0200", "module path could not be resolved"),
        ("E0201", "qualified call resolves to a different module than named"),
        ("E0202", "qualified call names a symbol that was never imported"),
        ("E0203", "import of a private/internal module member"),
        ("E0204", "module has no export by that name"),
        ("E0205", "duplicate name imported from multiple modules"),
        ("E0110", "type error"),
        ("E0111", "integer literal out of range for declared type"),
        ("E0112", "non-exhaustive match"),
        ("E0113", "generic monomorphization resource limit exceeded"),
        ("E0100", "type mismatch"),
        ("E0101", "unknown type"),
        ("E0102", "undefined variable"),
        ("E0103", "unknown struct"),
        ("E0104", "wrong number of arguments"),
        ("E0105", "unknown trait"),
        ("E0106", "no such field"),
        ("E0107", "no such method"),
        ("E0108", "missing fields in struct literal"),
        ("E0109", "Self used outside of impl/trait"),
        ("E0300", "use of moved value"),
        ("E0301", "use of uninitialized value"),
        ("E0302", "assignment to immutable variable"),
        ("E0303", "use of partially moved value"),
        ("W0300", "conditional move"),
        ("E0400", "integer overflow in checked operation"),
        ("E0401", "stack overflow (recursion too deep)"),
        ("E0500", "unsafe operation outside `unsafe` context"),
        ("E0501", "capability import violation"),
        ("E0502", "missing required capability"),
        ("E0503", "capability attenuation violation"),
        ("E0504", "capability escalation"),
        ("E0505", "builtin capability violation"),
        ("E0506", "FFI capability violation"),
        ("E0507", "capability propagation violation"),
        ("E0508", "unsupported extern declaration shape"),
        ("W0400", "tracked value discarded"),
        ("W0500", "unrecognized capability name in deny!()"),
    ]
}

// ----- E0001 ----------------------------------------------------------------

const E0001: &str = r#"E0001: unexpected token

The parser encountered a token it didn't expect at the current position.

Erroneous code example:

    fn main() {
        let x = 1 +
    }

Here the parser expected an expression after `+` but found `}` instead.

Fixed:

    fn main() {
        let x = 1 + 2
    }

Common causes:
  - Missing operand after a binary operator.
  - Stray punctuation (semicolons are not used in Kryos — the lexer will
    emit a hint when it sees one).
  - `else if` instead of `elif`.
  - Mixing C-style `;` line terminators with Kryos line breaks.

Kryos has no semicolons. Statements end at line breaks (or at `}`); break
long expressions across lines using parentheses or trailing operators.
"#;

// ----- E0002 ----------------------------------------------------------------

const E0002: &str = r#"E0002: expected identifier

An identifier was required at this position but a non-identifier token was
found. Identifiers must start with a letter or `_` and may contain letters,
digits, and `_`.

Erroneous code example:

    fn 1main() { }      // function name can't start with a digit

Fixed:

    fn main1() { }

Reserved keywords (`fn`, `let`, `if`, `match`, `chan`, `trait`, `impl`,
`unsafe`, ...) cannot be used as identifiers. If you need a similar name,
add a suffix or use snake_case.
"#;

// ----- E0003 ----------------------------------------------------------------

const E0003: &str = r#"E0003: expected expression

The parser was looking for an expression — a value, function call, literal,
operator chain, block, or `if`/`match` — and found something else.

Erroneous code example:

    let x = let y = 1

`let` is a statement, not an expression; you can't assign one to another
binding.

Fixed:

    let y = 1
    let x = y

This error often comes up when:
  - A function returns a value but its body is empty.
  - A match arm has no body (`Foo.Bar =>`).
  - A trailing operator left dangling (`1 + ` at end of line).
"#;

// ----- E0004 ----------------------------------------------------------------

const E0004: &str = r#"E0004: expected type

A type was required (after `:`, after `->`, in a generic argument list,
etc.) but the parser found a non-type token.

Erroneous code example:

    fn add(a: , b: i64) -> i64 { return a + b }

Fixed:

    fn add(a: i64, b: i64) -> i64 { return a + b }

Built-in types include: `i8`, `i16`, `i32`, `i64`, `u8`..`u64`, `f32`,
`f64`, `bool`, `str`, `chan`, plus arrays (`[T]`), maps (`map<K, V>`),
options (`Option<T>`), and results (`Result<T, E>`). User-defined types
must be in scope (imported or defined in the same module).
"#;

// ----- E0009 ----------------------------------------------------------------

const E0009: &str = r#"E0009: syntax error

A general syntax error the parser could not place in a more specific
category (E0001 unexpected token, E0002 expected identifier, E0003 expected
expression, E0004 expected type). The diagnostic message describes what was
expected at that position.

Common causes:
  - A misplaced or missing keyword (`fn`, `let`, `else`, ...).
  - A modifier (`pub`, `async`) with nothing valid following it.
  - A construct used where the grammar does not allow it.

Read the message and the underlined span, then compare against the grammar
in docs/19-language-reference.md.
"#;

// ----- E0010 ----------------------------------------------------------------

const E0010: &str = r#"E0010: program nesting too deep

The program exceeds the compiler's nesting budget: 256 levels of syntactic
nesting (parentheses, blocks, nested types or patterns), or 2048 nodes on a
single expression path (which a long flat chain like `a + b + c + ...` or
`x.f().g().h()...` also builds, one level per link).

These limits exist so that no input, however deeply nested, can crash the
compiler with a native stack overflow instead of a diagnostic. No
hand-written program approaches them; generated code occasionally does.

How to fix:
  - Split a long expression chain into intermediate `let` bindings.
  - Flatten deeply nested conditionals into early returns or `match`.
  - Build long string/array content with a loop instead of one giant
    expression.
"#;

// ----- E0200 ----------------------------------------------------------------

const E0200: &str = r#"E0200: module path could not be resolved

A `use std::<module>::{...}` (or a project-local module import) named a
module that either doesn't exist on disk, or whose file itself failed to
parse.

Erroneous code example:

    use std::strnig::{trim}   // typo: "strnig"

Fixed:

    use std::string::{trim}

Check the module name against `docs/19-language-reference.md`'s stdlib
list, or the project's own directory layout for a local module. If the
module DOES exist, the underlying diagnostic (bundled with this one) points
at the actual parse failure inside that module's file.
"#;

// ----- E0201 ----------------------------------------------------------------

const E0201: &str = r#"E0201: qualified call resolves to a different module than named

Kryos has one flat function namespace and no import aliasing. Writing
`Mod::name(...)` is sugar for the flat name `name` -- it is only valid when
`name` was actually imported FROM `Mod`. If two modules export a
same-named function and you imported the OTHER one, the qualifier is
misleading: the call still runs the imported function, just not the one
the qualifier suggests.

Erroneous code example:

    use std::csv::{parse}
    // ... later ...
    let v = json::parse(text)   // `parse` in scope came from std::csv, not std::json

Fixed:

    use std::json::{parse}
    let v = json::parse(text)

Import the name from the module you intend to call, and drop the
conflicting import -- only one module's version of a shared name can be in
scope at a time.
"#;

// ----- E0202 ----------------------------------------------------------------

const E0202: &str = r#"E0202: qualified call names a symbol that was never imported

`Mod::name(...)` requires `name` to already be in scope via a `use`
statement -- the `Mod::` qualifier is sugar for the flat name, not an
alternate way to reach an un-imported symbol.

Erroneous code example:

    use std::json::{stringify}
    // ... later ...
    let v = json::parse(text)   // `parse` was never imported

Fixed:

    use std::json::{parse, stringify}
    let v = json::parse(text)

Add the name to the module's `use` list.
"#;

// ----- E0203 ----------------------------------------------------------------

const E0203: &str = r#"E0203: import of a private/internal module member

Only public (non-underscore-prefixed) functions, types, constants, and enum
variants can be imported. A name starting with `_`, or an internal `extern`
primitive a stdlib module wraps, is not public API.

Erroneous code example:

    use std::os::{_env_or_empty}

Fixed: call the module's public wrapper instead (check the module's
documented exports), or use the underlying global builtin directly if one
exists (e.g. `env_get(...)`).
"#;

// ----- E0204 ----------------------------------------------------------------

const E0204: &str = r#"E0204: module has no export by that name

A `use std::<module>::{name}` named a symbol the module doesn't define at
all -- not a visibility problem (see E0203), the name simply doesn't exist
in that module.

Erroneous code example:

    use std::string::{capitalize_words}   // not a real std::string function

Fixed: check the exact name against the module's real exports (`kryos doc`
on the stdlib, or `docs/19-language-reference.md`), or write a small
wrapper yourself around the documented builtins -- don't guess a plausible
stdlib name and assume it exists.
"#;

// ----- E0205 ----------------------------------------------------------------

const E0205: &str = r#"E0205: duplicate name imported from multiple modules

Kryos imports share one flat namespace with no aliasing (`use m::{parse as
p}` is a parse error), so two `use` statements that both bring in a symbol
with the same name collide -- the compiler can't tell which one a later
bare call means.

Erroneous code example:

    use std::csv::{parse}
    use std::json::{parse}

Fixed: import only the module you actually need `parse` from, and reach
the other one through a qualified call (`csv::parse(...)`) if you genuinely
need both -- or better, import disjoint names selectively so only one
`parse` is ever in scope.
"#;

// ----- E0100 ----------------------------------------------------------------

const E0100: &str = r#"E0100: type mismatch

An expression's actual type does not match the type required by its
context. Kryos is strictly typed: there are no implicit conversions
between numeric types, between integers and booleans, or between strings
and numbers.

Erroneous code example:

    fn double(n: i64) -> i64 { return n * 2 }

    fn main() {
        let x: f64 = 1.5
        print(to_string(double(x)))   // f64 passed where i64 required
    }

Fixed:

    let x: i64 = 1
    print(to_string(double(x)))

Conversions are explicit. Use:
  - `i64_to_f64(n)`, `f64_to_i64(x)` for numeric widening/truncation.
  - `to_string(n)` for converting any numeric to a `str`.
  - `parse_int(s)`, `parse_float(s)` for parsing.

Type inference will infer the type of a `let` binding from its initializer
only if no explicit annotation is given.
"#;

// ----- E0101 ----------------------------------------------------------------

const E0101: &str = r#"E0101: unknown type

The name used as a type does not refer to any visible type. Either:
  - It is misspelled.
  - The defining module has not been imported.
  - It is defined later in the same file (but Kryos sees declarations in
    source order; forward references between modules are fine, within a
    module forward refs to types are also fine, but typos still trip this).

Erroneous code example:

    fn area(r: Float) -> Float { return 3.14 * r * r }

`Float` does not exist; the built-in type is `f64` (or `f32`).

Fixed:

    fn area(r: f64) -> f64 { return 3.14 * r * r }

User-defined types must be defined with `struct`, `enum`, or `trait`, and
must be either in the current module or `use`-imported.
"#;

// ----- E0102 ----------------------------------------------------------------

const E0102: &str = r#"E0102: undefined variable

The identifier was used as a value but no binding with that name is in
scope. Causes:
  - Typo.
  - Variable defined inside an inner block (and the use is outside that
    block).
  - Function defined after its use in a position where forward references
    are not yet supported.

Erroneous code example:

    fn main() {
        if true {
            let x = 1
        }
        print(to_string(x))  // x is out of scope here
    }

Fixed:

    fn main() {
        let x = 1
        if true {
            // ...
        }
        print(to_string(x))
    }

Each `{ ... }` block introduces a new scope. Bindings cannot escape their
defining block; if you need a value to outlive the block, declare it before
the block and assign inside.
"#;

// ----- E0103 ----------------------------------------------------------------

const E0103: &str = r#"E0103: unknown struct

A struct literal or pattern names a struct that has not been declared or
imported.

Erroneous code example:

    let p = Point { x: 1, y: 2 }

If `Point` isn't defined or imported in this module, this fails.

Fixed:

    struct Point { x: i64, y: i64 }

    let p = Point { x: 1, y: 2 }

Or, if it lives in another module:

    use math::geometry::{Point}

    let p = Point { x: 1, y: 2 }
"#;

// ----- E0104 ----------------------------------------------------------------

const E0104: &str = r#"E0104: wrong number of arguments

The call passes a different number of arguments than the function declares.
Kryos does not support default arguments, variadic functions (yet), or
optional parameters via overloading.

Erroneous code example:

    fn add(a: i64, b: i64) -> i64 { return a + b }

    let n = add(1)        // missing second argument
    let m = add(1, 2, 3)  // one too many

Fixed:

    let n = add(1, 2)

If you want optional values, take `Option<T>`:

    fn greet(name: str, greeting: Option<str>) -> str {
        match greeting {
            Option::Some(g) => g + " " + name,
            Option::None    => "Hello " + name,
        }
    }
"#;

// ----- E0105 ----------------------------------------------------------------

const E0105: &str = r#"E0105: unknown trait

A trait name in an `impl ... for ...`, a trait bound, or a `dyn Trait`
type does not refer to any visible trait.

Erroneous code example:

    impl Printable for Point { fn show(self) { } }

If `Printable` isn't defined or imported, this fails.

Fixed: define the trait, or import it.

    trait Printable { fn show(self) }

    impl Printable for Point { fn show(self) { } }
"#;

// ----- E0106 ----------------------------------------------------------------

const E0106: &str = r#"E0106: no such field

The field accessed on a struct value does not exist on that struct's type.

Erroneous code example:

    struct Point { x: i64, y: i64 }

    let p = Point { x: 1, y: 2 }
    print(to_string(p.z))     // no field `z`

Fixed: use an existing field, or add `z` to the struct definition.

Field names are case-sensitive. Public/private visibility for fields is
controlled at the module level — fields are accessible inside the
defining module and any module that imports the containing struct.
"#;

// ----- E0107 ----------------------------------------------------------------

const E0107: &str = r#"E0107: no such method

The method called does not exist for the receiver's type. Either:
  - Method name is misspelled.
  - The `impl` block defining it is not imported.
  - The method is on a different trait than is in scope.

Erroneous code example:

    struct Vec3 { x: f64, y: f64, z: f64 }

    impl Vec3 { fn length(self) -> f64 { return 0.0 } }

    fn main() {
        let v = Vec3 { x: 1.0, y: 2.0, z: 3.0 }
        let n = v.norm()     // no method `norm`
    }

Fixed:

    let n = v.length()

If the method is provided by a trait, `use` the trait into scope:

    use math::vector::{LengthOps}     // brings the `.length()` method into scope
"#;

// ----- E0108 ----------------------------------------------------------------

const E0108: &str = r#"E0108: missing fields in struct literal

A struct literal must initialize every field. Kryos has no field-default
syntax (yet) — you must provide an explicit value for each.

Erroneous code example:

    struct Point { x: i64, y: i64 }

    let p = Point { x: 1 }     // missing y

Fixed:

    let p = Point { x: 1, y: 0 }

If your struct has many fields with sensible defaults, define a `new`
constructor:

    impl Point {
        fn new(x: i64) -> Point { return Point { x: x, y: 0 } }
    }

    let p = Point::new(1)
"#;

// ----- E0109 ----------------------------------------------------------------

const E0109: &str = r#"E0109: `Self` used outside of impl/trait

`Self` (the type) and `self` (the receiver) only have meaning inside an
`impl` block or a `trait` definition. Using them at top-level or inside a
free function is invalid.

Erroneous code example:

    fn make() -> Self { return Self { } }    // no enclosing impl/trait

Fixed: put the function inside an `impl`:

    struct Point { x: i64, y: i64 }

    impl Point {
        fn make() -> Self { return Self { x: 0, y: 0 } }
    }

Inside an impl, `Self` means the type being implemented; `self` is the
receiver if the method takes one.
"#;

// ----- E0110 ----------------------------------------------------------------

const E0110: &str = r#"E0110: type error

A general type-checking error that does not fall under a more specific code
(E0100 mismatch, E0101 unknown type, E0102 undefined variable, E0103-E0109,
capability E05xx, ownership E03xx). The diagnostic message describes the
specific problem — for example a generic type given the wrong number of
arguments, a field/variant access on the wrong type, or an operation applied
to an incompatible type.

Read the message and the underlined span; the fix is usually to correct the
type annotation, the number of generic arguments, or the operation.
"#;

const E0111: &str = r#"E0111: integer literal out of range for declared type

An integer literal does not fit the narrow integer type it is annotated or
assigned with, for example:

    let small: u8 = 999        // u8 holds 0..=255
    let temp: i8 = 200         // i8 holds -128..=127

Before this check existed the literal was silently truncated (999 became
231), which is data corruption at the source level.

Fixes:
  - use a wider type:        let small: u16 = 999
  - or, if truncation is genuinely intended, say so with an explicit cast:
                             let small: u8 = (999 as u8)

Explicit `as` casts keep their documented truncation semantics; only
implicit literal coercion is range-checked.
"#;

const E0112: &str = r#"E0112: non-exhaustive match

A `match` does not cover every possible value of the matched type. Kryos
requires enum matches to handle every variant (or add a wildcard `_`):

    enum Color { Red, Green, Blue }
    match c {
        Red => ...,
        Green => ...,      // missing `Blue` -- E0112
    }

Fixes:
  - add an arm for each missing variant, or
  - add a catch-all wildcard:   _ => ...

Exhaustiveness makes adding a new enum variant a compile error at every
match that must handle it, rather than a silent fall-through at runtime.
"#;

const E0113: &str = r#"E0113: generic monomorphization resource limit exceeded

The compiler bounds how much work generic monomorphization can be made to
do, on three axes: recursive instantiation DEPTH, total distinct
instantiation COUNT, and the structural SIZE of any one concrete type
argument. This error fires when a program crosses one of those bounds.

This is a compile-time denial-of-service guard, not an ordinary type error:
without it, either of the two shapes below makes the compiler exhaust
memory or hang indefinitely on a tiny source file, with no diagnostic.

1. A generic that PAIRS or otherwise duplicates its own type parameter in
   its return type, called in a chain:

       fn dup<T>(x: T) -> (T, T) {
           return (x, x)
       }
       let d1 = dup(x)
       let d2 = dup(d1)   // T = (T0, T0)
       let d3 = dup(d2)   // T = ((T0,T0),(T0,T0))  -- doubles every level

   Each level's concrete type is TWICE the size of the previous one, so a
   linear chain of calls produces an exponentially large type.

2. A self-recursive generic function whose recursive call instantiates a
   genuinely NEW, larger concrete type at every level:

       fn f<T>(x: T) {
           f(grow(x))   // grow(x) has a DIFFERENT, larger type than x
       }

   Unlike ordinary self-recursion at a single fixed type (which the
   compiler caches and does not re-monomorphize), each level here is an
   uncached, fresh instantiation, so the recursion is unbounded.

Fixes:
  - stop composing the generic's own result back into itself across a long
    chain (shape 1) -- if you need a fixed-size aggregate, build it with a
    concrete (non-doubling) type instead of `(T, T)`;
  - bound the recursion in shape 2 to a fixed type (e.g. recurse on the
    SAME `T`, converting/growing the VALUE without growing the TYPE), or
    convert the recursion into an explicit loop over a homogeneous
    collection.

The limits themselves (instantiation depth, total instantiation count, and
per-type node-count) are set far above what any legitimate generic-heavy
program needs -- if you believe you have hit one without either of the
above patterns, the diagnostic names the offending generic and prints the
instantiation chain that led to the limit; that chain is the place to look
for an unintended type-growing or count-growing loop.
"#;

// ----- E0300 ----------------------------------------------------------------

const E0300: &str = r#"E0300: use of moved value

A value owned by another binding has been moved away, and the original
binding is no longer usable.

Erroneous code example:

    struct Big { data: [i64] }

    fn take(b: Big) { /* ... */ }

    fn main() {
        let b = Big { data: [1, 2, 3] }
        take(b)
        print(to_string(len(b.data)))     // b was moved into `take`
    }

Fixed: either don't use `b` after the move, or pass by reference:

    fn inspect(b: &Big) -> i64 { return len(b.data) }

    fn main() {
        let b = Big { data: [1, 2, 3] }
        let n = inspect(&b)
        print(to_string(n))
    }

Kryos ownership rules: when a value with destructable state is passed by
value (no `&`), ownership moves to the callee, which is responsible for
dropping it. Types that implement `Copy` (all primitives, small `repr(C)`
structs of primitives) are duplicated instead of moved.
"#;

// ----- E0301 ----------------------------------------------------------------

const E0301: &str = r#"E0301: use of uninitialized value

A variable was read before it was assigned. Kryos requires every read to
be preceded by a definite assignment on every control-flow path leading
to that read.

Erroneous code example:

    fn main() {
        let x: i64
        if some_condition() {
            x = 1
        }
        print(to_string(x))     // x is uninitialized if branch not taken
    }

Fixed: initialize on both paths, or at declaration:

    fn main() {
        let x: i64 = 0
        if some_condition() {
            x = 1
        }
        print(to_string(x))
    }

Or use an `Option<T>` if "no value" is meaningful:

    let x: Option<i64> = if some_condition() { Option::Some(1) } else { Option::None }
"#;

// ----- E0302 ----------------------------------------------------------------

const E0302: &str = r#"E0302: assignment to immutable variable

`let` introduces an immutable binding by default. Reassigning requires
`let mut`.

Erroneous code example:

    fn main() {
        let x = 1
        x = 2        // cannot reassign immutable `x`
    }

Fixed:

    fn main() {
        let mut x = 1
        x = 2
    }

Immutability by default is a discipline tool: it lets you read a function
top-to-bottom knowing which bindings can change.

Note that immutability of the *binding* is independent of mutability of
the *value*: passing an `&Big` makes the value read-only through that
reference even if the original owner declared `let mut`.
"#;

// ----- E0303 ----------------------------------------------------------------

const E0303: &str = r#"E0303: use of partially moved value

A struct had one of its fields moved out, and then the whole value (or the
moved field) was used again. Once a field is moved, the parent value is
only partially valid: the moved field can no longer be read.

Erroneous code example:

    struct Pair { a: Big, b: Big }

    fn take(x: Big) { /* ... */ }

    fn main() {
        let p = Pair { a: Big {}, b: Big {} }
        take(p.a)               // moves `p.a` out of `p`
        print(to_string(len(p.a.data)))   // error: `p.a` was moved
    }

Fixed: don't read the moved field afterwards, or move the whole value, or
pass the field by reference:

    fn inspect(x: &Big) -> i64 { return len(x.data) }

    fn main() {
        let p = Pair { a: Big {}, b: Big {} }
        let n = inspect(&p.a)   // borrow instead of move
        print(to_string(n))
    }
"#;

// ----- W0300 ----------------------------------------------------------------

const W0300: &str = r#"W0300: conditional move

A value is moved on one branch of an `if`/`match` but not on the other, so
whether the value is still usable after the branch depends on which path
ran. Kryos conservatively warns because the binding's state is no longer
statically obvious.

Example that triggers the warning:

    fn main() {
        let b = Big {}
        if some_condition() {
            take(b)             // moved here...
        }
        // ...but not on the else path; `b`'s state is now branch-dependent
    }

Fixes:
  - Move the value on every path (or none), so its state is unambiguous.
  - Restructure so the move happens after the branch.
  - Use `shared` if the value genuinely needs branch-dependent lifetime.

This is a warning, not an error: the program still compiles. It flags code
that is easy to misread and that will become an E0300 if you later add a
use after the branch.
"#;

// ----- E0400 ----------------------------------------------------------------

const E0400: &str = r#"E0400: integer overflow in checked operation

A `checked_add`, `checked_sub`, or `checked_mul` operation overflowed the
i64 range, triggering a runtime panic.

Erroneous run-time example:

    let big: i64 = 9_000_000_000_000_000_000
    let _ = checked_add(big, big)   // i64 max is ~9.22e18; this overflows

This panics with:

    kryos panic: integer overflow in checked_add

Choose the right arithmetic builtin for your situation:
  - `wrapping_add(a, b)`     — silently wraps modulo 2^64. Fastest.
  - `checked_add(a, b)`      — panics on overflow. Use when overflow is a bug.
  - `saturating_add(a, b)`   — clamps to i64::MIN or i64::MAX on overflow.

Default `+`, `-`, `*` use wrapping arithmetic (matching Rust's release
behavior and most peer languages). See `docs/16-integer-overflow.md`.
"#;

// ----- E0401 ----------------------------------------------------------------

const E0401: &str = r#"E0401: stack overflow (recursion too deep)

A Kryos program ran out of stack space, typically by recursing without a
base case or with too high a recursion depth for the default 8 MB stack.

Erroneous example:

    fn recurse(n: i64) -> i64 { return recurse(n + 1) }

    fn main() { let _ = recurse(0) }

This will print:

    kryos panic: stack overflow (possible infinite recursion)

and exit with status 134.

Fixes:
  - Add a base case: `if n > 1000 { return n }`.
  - Convert recursion to iteration (`for` / `while`).
  - For deep but bounded recursion, raise the thread stack via the
    runtime spawn API: `spawn_with_stack(16 * 1024 * 1024, my_fn)`.

Kryos installs a SIGSEGV handler at program start to detect stack
overflow; without it, raw SIGSEGV would print nothing and exit silently.
"#;

// ----- E0500 ----------------------------------------------------------------

const E0500: &str = r#"E0500: unsafe operation outside `unsafe` context

An operation that requires the `unsafe` keyword was used in a safe
context. This covers raw pointer dereferences, calls to `unsafe` functions
(including most FFI), and reads of `mut static` data.

Erroneous code example:

    extern "C" fn raw_write(ptr: *mut u8, val: u8)

    fn main() {
        let p: *mut u8 = ...
        raw_write(p, 0)        // calling an extern unsafe fn
    }

Fixed:

    fn main() {
        let p: *mut u8 = ...
        unsafe { raw_write(p, 0) }
    }

The `unsafe` block is a *promise to the compiler* that you've personally
verified the invariants of the operation (see `docs/17-unsafe-audit.md`
for the runtime's catalog of patterns). It does not turn off any checks
the compiler already does — it just enables operations that the compiler
cannot verify on its own.
"#;

// ----- E0501..E0507: capability system ---------------------------------------
//
// Every function carries a capability set inferred from the builtins it calls
// (io, net, process, ...). These errors fire when the declared/granted set is
// violated. See docs/10-capabilities.md.

const E0501: &str = r#"E0501: capability import violation

An `import`/`use` brings in a function whose capability requirements exceed
what the importing scope is allowed to grant. Importing a capability you may
not hold is rejected at the import site rather than at the call site.

Fix: grant the capability on the enclosing function (e.g. `@capabilities(io)`),
or import a more attenuated API that does not require it.
"#;

const E0502: &str = r#"E0502: missing required capability

A function calls an operation that needs a capability (e.g. `file_write`
needs `io`, `spawn` needs `process`) that the function's inferred or declared
capability set does not include.

Fix: add the capability to the function/entry-point capability annotation, or
stop calling the privileged operation on this path.
"#;

const E0503: &str = r#"E0503: capability attenuation violation

Attenuation may only narrow a capability set, never widen it. A child scope
tried to claim a capability broader than the one it was handed.

Fix: request only a subset of the parent's capabilities.
"#;

const E0504: &str = r#"E0504: capability escalation

Code attempted to acquire a capability it was not granted — a privilege
escalation. Capabilities flow downward through the call graph and cannot be
conjured locally.

Fix: thread the capability in from a caller that legitimately holds it.
"#;

const E0505: &str = r#"E0505: builtin capability violation

A builtin was called in a context lacking the capability that builtin
requires. Builtins like `file_read`/`file_write` (io), `spawn` (process),
and network operations (net) are capability-gated.

Fix: grant the required capability, or avoid the builtin on this path.
"#;

const E0506: &str = r#"E0506: FFI capability violation

A foreign (FFI / `extern`) call requires the `ffi` capability (FFI is
unverifiable, so it is treated as maximally privileged). The current scope
does not hold it.

Fix: grant `ffi` on the entry point, and prefer wrapping FFI behind a small,
audited, capability-attenuated module.
"#;

const E0507: &str = r#"E0507: capability propagation violation

A capability required by a callee was not propagated through an intermediate
function's signature. Capabilities must be visible along the whole call chain,
not silently swallowed.

Fix: declare the capability on the intermediate function so it propagates to
its callers.
"#;

const E0508: &str = r#"E0508: unsupported extern declaration shape

An `extern` block declared a function this compiler cannot safely emit a
real call for. Two distinct shapes are rejected:

1. A non-`kryos_*` name (a genuine C-library / system-call symbol, e.g.
   `getpid`, `abs`, `puts`). Arbitrary C FFI emission is not implemented:
   the extern's parameter/symbol info is not threaded to codegen, so such a
   declaration either fails to link, fails with a confusing type-mismatch,
   or -- worst case -- silently "succeeds" only because the name happens to
   collide with an unrelated Kryos builtin (`sqrt`, `abs`, `cos`, ...),
   which is calling the BUILTIN, not your declared foreign function. See
   docs/13-ffi.md for the verified current state.

2. A `kryos_*`-prefixed name with a `str`/array/map/struct/tuple/enum/fn
   -typed parameter or return, hand-declared outside the compiler's small
   set of verified-safe wrappers. The real runtime symbols behind these
   names expect RAW pointer/length pairs (as `std::os`'s `_env_or_empty`
   demonstrates: `kryos_env_get(key_ptr: i64, key_len: i64, ...)`), not a
   Kryos `str` handle -- hand-declaring the same name with a `str`-typed
   signature bypasses that marshalling and reads/writes through the wrong
   pointer shape, which segfaults at runtime.

Fix: for (1), call the documented builtin/`std::` wrapper instead of
hand-declaring a foreign symbol -- there is currently no supported way to
link an arbitrary C function. For (2), call the safe builtin/stdlib
function (e.g. `env_get`, `to_upper`) instead of hand-declaring the raw
`kryos_*` symbol yourself; if you are implementing a NEW raw-pointer
wrapper, declare it with the raw i64/i32/f64/ptr signature the native
symbol actually uses (see `compiler/stdlib/os.kry` for the pattern), not
with `str`/array/map types directly.
"#;


// ----- W0400 ----------------------------------------------------------------
const W0400: &str = r#"W0400: tracked value is discarded

A `Tracked<T>` value carries provenance/lineage metadata. Discarding one
(calling a tracked-returning function without using its result) silently
loses that lineage, which defeats the point of tracking it.

Erroneous code:

    use std::tracked::{tracked_source}

    fn main() {
        tracked_source("sensor-a", 42)   // result discarded -> W0400
    }

Fix: use the value, or drop it explicitly with a recorded reason:

    use std::tracked::{tracked_source, tracked_discard}

    fn main() {
        let t = tracked_source("sensor-a", 42)
        tracked_discard(t, "calibration sample, not persisted")
    }

This is a warning, not an error: the program still compiles and runs.
"#;

// ----- W0500 ----------------------------------------------------------------
const W0500: &str = r#"W0500: unrecognized capability name in deny!()

A `deny!(...)` block names a capability the compiler does not recognize.
The deny still applies to nothing, so the block gives no protection --
this usually means a typo.

Erroneous code:

    fn main() {
        deny!(newtork) {           // typo: not a real capability -> W0500
            fetch_data()
        }
    }

Fix: use a recognized capability name (`net`, `fs`, `fs:read`, `fs:write`,
`db`, `crypto`, `ffi`, `env`, `process`, ...):

    fn main() {
        deny!(net) {
            fetch_data()           // now actually rejected if it uses net
        }
    }

This is a warning so existing programs keep compiling, but a misspelled
deny provides NO sandboxing -- treat it as an error in security-sensitive
code.
"#;
