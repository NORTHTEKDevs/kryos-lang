# 05 · Control Flow

After this chapter you will be able to write every branching and looping
construct in Kryos, match a value against literal, tuple, or multi-pattern
shapes instead of a chain of `if`/`elif`, unwrap a single `Option` case
inline with `if let`, and use the fact that almost everything in Kryos --
`if`, `match`, even a bare `{ }` block -- is itself an expression that
produces a value.

## `if` / `elif` / `else`

```kryos
fn main() {
    let x: i64 = 5
    if x > 10 {
        println("big")
    } elif x > 0 {
        println("small")
    } else {
        println("non-positive")
    }
}
```

Output:

```
small
```

[Chapter 1](01-hello.md) already covers the one thing worth remembering
here: Kryos spells this `elif`, and while `else if` also parses (the grammar
accepts both), every example in this book, the standard library, and the
compiler's own source uses `elif` -- write what the rest of the codebase
writes. Braces are always required; there's no single-statement shortcut
and no parentheses required around the condition.

## `while`

```kryos
fn main() {
    let mut i: i64 = 0
    while i < 5 {
        i = i + 1
        if i % 2 == 0 {
            continue
        }
        println("odd: " + to_string(i))
    }
}
```

Output:

```
odd: 1
odd: 3
odd: 5
```

`continue` skips to the next check of the `while` condition; `break` exits
the loop immediately. Both apply to the innermost enclosing loop only --
there's no labeled break for jumping out of a nested loop from the inside.

## `for ... in`

`for` iterates a range or a collection. `0..5` is a half-open range
(excludes `5`); `0..=5` includes it:

```kryos
fn main() {
    for i in 0..5 {
        print(to_string(i) + " ")
    }
    println("")
    for i in 0..=5 {
        print(to_string(i) + " ")
    }
    println("")
}
```

Output:

```
0 1 2 3 4 
0 1 2 3 4 5 
```

The same `for` iterates any array directly, which is the common case once
you have real data instead of a counter:

```kryos
fn main() {
    let names: [str] = ["Ada", "Grace", "Alan"]
    for name in names {
        println("hello, " + name)
    }
}
```

Output:

```
hello, Ada
hello, Grace
hello, Alan
```

## `loop` and `break`

`loop { }` is an unconditional loop -- there's no separate `do`/`repeat`
keyword, and no condition to get wrong. Exit with `break`:

```kryos
fn main() {
    let mut i: i64 = 0
    loop {
        if i >= 3 {
            break
        }
        println("i = " + to_string(i))
        i = i + 1
    }
}
```

Output:

```
i = 0
i = 1
i = 2
```

`loop` desugars to `while true` -- reach for it when the exit condition
belongs in the middle of the loop body rather than cleanly up front, which
is exactly the shape `while true { if done() { break } }` forces you to
write anyway.

## `match`

`match` compares one value against a series of patterns and runs the first
arm that matches. Patterns can be literals, tuples, or several literals
joined with `|`:

```kryos
fn describe(point: (i64, i64)) -> str {
    match point {
        (0, 0) => "origin",
        (x, 0) => "on x-axis at " + to_string(x),
        (0, y) => "on y-axis at " + to_string(y),
        (x, y) => "(" + to_string(x) + ", " + to_string(y) + ")",
    }
}

fn size_label(n: i64) -> str {
    match n {
        1 | 2 | 3 => "small",
        4 | 5 | 6 => "medium",
        _ => "large",
    }
}

fn main() {
    println(describe((0, 0)))
    println(describe((5, 0)))
    println(describe((3, 4)))
    println(size_label(2))
    println(size_label(9))
}
```

Output:

```
origin
on x-axis at 5
(3, 4)
small
large
```

A tuple pattern like `(x, 0)` binds `x` to whatever the first element is
while requiring the second to be exactly `0` -- patterns can mix a literal
requirement in one position with a binding in another, and arms are tried
top to bottom, so put the more specific patterns first. `1 | 2 | 3` is an
or-pattern: match any of several literals with one arm. Or-pattern
alternatives must all be non-binding (bare literals or bare enum variant
names) -- see "Common mistakes" for what happens if you try to bind a
variable inside one.

A guard adds an arbitrary boolean condition to a pattern with `if`:

```kryos
fn classify(n: i64) -> str {
    match n {
        0 => "zero",
        n if n < 0 => "negative",
        n if n < 10 => "single-digit",
        _ => "big",
    }
}

fn main() {
    println(classify(0))
    println(classify(-5))
    println(classify(7))
    println(classify(42))
}
```

Output:

```
zero
negative
single-digit
big
```

`match` on a `bool` or an enum is **exhaustive** -- the compiler rejects a
`match` that doesn't cover every case, unless you add a `_` wildcard arm.
This is what makes `match` on `Option`'s `Some`/`None` a reliable substitute
for a null check: you cannot forget the empty case and have it compile.

## `if let` / `while let`

Matching a single `Some`/`None` case doesn't need a full `match` -- `if let`
runs its block only when the pattern matches, with the bound name in scope
for that block:

```kryos
use std::option::{Some, None}

fn first_positive(nums: [i64]) -> Option<i64> {
    for n in nums {
        if n > 0 {
            return Some(n)
        }
    }
    return None()
}

fn main() {
    let nums: [i64] = [-3, -1, 4, 7]
    if let Some(n) = first_positive(nums) {
        println("found: " + to_string(n))
    } else {
        println("none found")
    }
}
```

Output:

```
found: 4
```

`while let` repeats its block for as long as the pattern keeps matching,
stopping the first time it doesn't -- useful for draining a source that
signals "done" with `None()`:

```kryos
use std::option::{Some, None}

fn next_value(state: i64) -> Option<i64> {
    if state <= 0 {
        return None()
    }
    return Some(state)
}

fn main() {
    let mut n: i64 = 3
    while let Some(v) = next_value(n) {
        println("got " + to_string(v))
        n = n - 1
    }
    println("done")
}
```

Output:

```
got 3
got 2
got 1
done
```

Both desugar to an ordinary `match` under the hood -- `if let PAT = expr { A } else { B }`
is `match expr { PAT => A, _ => B }`, and `while let` is the same match
wrapped in a `loop` that `break`s on the non-matching arm.

## Blocks are expressions

`if`, `match`, and a bare `{ }` all produce a value -- every branch of an
`if`/`match` used this way must produce the *same* type, and that shared
type is what you can bind with `let`:

```kryos
fn main() {
    let n: i64 = 7

    let label: str = if n % 2 == 0 {
        "even"
    } else {
        "odd"
    }
    println(label)

    let described: str = match n {
        0 => "zero",
        n if n % 2 == 0 => "even",
        _ => "odd",
    }
    println(described)

    let sum: i64 = {
        let a: i64 = 3
        let b: i64 = 4
        a + b
    }
    println(to_string(sum))
}
```

Output:

```
odd
odd
7
```

The last one is worth pausing on: `{ }` alone, with no `if`/`match`
attached, is already a valid expression -- its value is its last statement's
value (no trailing `return` needed, same tail-expression rule Chapter 4
covers for function bodies), and everything declared inside it (`a`, `b`)
goes out of scope the moment the block ends. This is also where `comptime`
blocks live: `comptime { 6 * 7 }` is the same block-as-value shape,
evaluated at compile time instead of runtime. `comptime` is
**expression-only** -- it has to appear where a value is expected (`let
answer: i64 = comptime { 6 * 7 }`), not as a standalone statement, and
nothing inside one may have a side effect (a `println` inside a `comptime`
block is rejected rather than silently doing nothing).

## Common mistakes

**Binding a variable inside an or-pattern.** Every alternative in `a | b`
has to be non-binding, because the compiler can't reconcile a name that
might come from two different alternatives:

```kryos
fn describe(v: i64) -> str {
    match v {
        x | 0 => to_string(x),   // ERROR: x doesn't bind on the `0` alternative
        _ => "other",
    }
}

fn main() {
    println(describe(5))
}
```

```
error[E0110]: or-pattern alternatives must be non-binding: use literals
(`1 | 2`) or bare enum variants (`Red | Green`); a pattern that binds a
variable is not allowed here because alternatives may bind different
names or types
 --> mistake.kry:3:9
  3 |         x | 0 => to_string(x),
    |         ^ here
```

Split it into two arms instead: a guard (`n if n == 0 or n == x`) or two
separate `=>` cases.

**Forgetting the wildcard on a non-exhaustive match.** `bool` and enum
matches must cover every case:

```kryos
fn label(flag: bool) -> str {
    match flag {
        true => "yes",   // ERROR: missing `false`
    }
}

fn main() {
    println(label(true))
}
```

```
error[E0112]: non-exhaustive match: missing `false`
 --> mistake.kry:2:5
  2 |     match flag {
    |     ^^^^^^^^^^^^ add the missing case(s) or a wildcard `_`
```

Add the missing arm, or a trailing `_ => ..` if you genuinely mean "every
other case."

## Exercises

1. Write a `for` loop over `0..=10` that prints only the multiples of 3,
   using `continue` to skip everything else.
2. Write a function that takes a three-element tuple `(i64, i64, i64)` and
   `match`es it to classify how many of the three elements are `0` (zero,
   one, two, or three), using tuple patterns.
3. Write a `while let` loop that counts down from `5` to `1` using the
   `next_value`-style `Option`-returning helper from this chapter, printing
   each value.
4. Rewrite the `label` common mistake above with the missing `false` arm
   added, and confirm `kryos check` passes.

## Summary

- `elif`, never `else if` in idiomatic code (though both parse); braces are
  always required, condition parentheses are optional.
- `while`/`for`/`loop` cover conditional, range-or-collection, and
  unconditional iteration; `break`/`continue` apply to the innermost loop.
- `match` patterns include literals, tuples, or-patterns (`1 | 2 | 3`,
  alternatives must be non-binding), and guards (`n if n < 10`); `bool` and
  enum matches must be exhaustive.
- `if let`/`while let` match a single pattern inline without a full
  `match`, and both desugar to one.
- `if`, `match`, and a bare `{ }` are all expressions -- their value can be
  bound directly with `let`, and `comptime { }` is the same shape evaluated
  at compile time, expression-only, no side effects allowed inside.

Next: [Structs & enums](06-structs-and-enums.md)
