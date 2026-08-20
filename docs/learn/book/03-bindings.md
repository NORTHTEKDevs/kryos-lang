# 03 · Bindings

After this chapter you will know when a binding can be reassigned and when it
can't, how shadowing lets you reuse a name for a transformed value without
fighting the type checker, why a handful of top-level `let`s behave
differently from every `let` inside a function, and the single most common
way a Kryos program silently does the wrong thing without printing a single
diagnostic.

## `let` is immutable, `let mut` is mutable

```kryos
fn main() {
    let mut total: i64 = 0
    total = total + 1
    total = total + 1
    println(to_string(total))
}
```

Output:

```
2
```

A plain `let` cannot be reassigned. Try it without `mut`:

```kryos
fn main() {
    let total: i64 = 0
    total = total + 1   // ERROR: total is immutable
    println(to_string(total))
}
```

```
error[E0302]: assignment to immutable variable `total`
 --> mistake.kry:3:5
  3 |     total = total + 1
    |     ^^^^^^^^^^^^^^^^^ help: consider declaring with `let mut`
```

The fix is always the same: add `mut` at the declaration site, not at the
point of reassignment. This makes every mutable binding in a function
grep-able from its `let` line -- you never have to scan forward to discover
that a variable changes later.

## Scope

A binding is visible from its `let` to the end of the enclosing block.
A nested `{ }` block introduces a fresh scope; a `let` inside it shadows an
outer name for the block's duration and the outer binding reappears once
the block ends:

```kryos
fn main() {
    let x: i64 = 1
    println("outer: " + to_string(x))
    {
        let x: i64 = 2
        println("inner: " + to_string(x))
    }
    println("outer again: " + to_string(x))
}
```

Output:

```
outer: 1
inner: 2
outer again: 1
```

The inner `let x` is a completely separate binding -- it doesn't mutate the
outer `x`, and the outer one is untouched once the block closes. `if`,
`while`, `for`, and function bodies all introduce their own scope the same
way.

## Shadowing

Declaring a new `let` with a name already in scope doesn't reassign it --
it introduces a second, independent binding that happens to share a name,
and it can have a **different type** than the one it replaces:

```kryos
fn main() {
    let count: str = "3"
    println("input: " + count)
    let count: i64 = parse_int(count)
    println("parsed: " + to_string(count + 1))
}
```

Output:

```
input: 3
parsed: 4
```

This is not the same thing as `let mut` -- the second `count` is a brand new
`i64` binding, not the original `str` binding mutated in place. Shadowing is
the idiomatic way to carry a value through a series of transformations
(parse, validate, normalize) without inventing a new name at each step or
declaring the original `mut` just to change its type, which you couldn't do
anyway -- `let mut` still requires every reassignment to keep the same type.

## Top-level `let`

A `let` outside any function -- a module-level constant -- works the same
way as a local one, with one restriction: **its initializer can only call
things that need no capability.** There is no enclosing function to hang
`@capabilities(...)` on, so anything gated (`env_get`, `file_read`, a
user-defined function that transitively needs one) is rejected at the
top level, while a pure computation or an ungated builtin like `args()`
is fine:

```kryos
let PI: f64 = 3.14159
let mut cli_args: [str] = args()

fn main() {
    println(to_string(PI))
    println(to_string(len(cli_args)))
}
```

Output:

```
3.14159
1
```

`args()` needs no capability, so it's allowed here. `env_get` does -- see
"Common mistakes" below for what happens when you reach for it at the top
level anyway. When an initializer needs anything beyond a pure computation
or an ungated builtin, do it in `main()` instead: declare `let mut` for the
eventual value, or just move the whole computation inside `main` and skip
the top-level binding entirely.

## The line-continuation trap

Kryos has no semicolons -- a newline ends a statement, *unless* the next
line opens with a token the parser can still attach to the previous
expression. A binary operator, `(`, or `[` at the start of a line does
exactly that, and the parser has no line-number awareness to stop it: it
just keeps consuming tokens as long as the grammar allows. This is the
single most common way a Kryos program compiles clean and produces the
wrong answer.

```kryos
fn main() {
    let a: i64 = 5
    -1
    println(to_string(a))
}
```

Output:

```
4
```

Read that again: the program never printed `-1`, and `a` is `4`, not `5`.
The parser saw `let a: i64 = 5` and, instead of stopping at the newline,
kept going onto `-1` -- there is no unary-minus statement here, only
`let a: i64 = 5 - 1`. The standalone `-1` you meant to write as its own line
doesn't exist as far as the compiler is concerned; it was silently absorbed
into the line above.

`[` does the same thing to an array:

```kryos
fn main() {
    let arr: [i64] = [10, 20, 30]
    let x = arr
    [0]
    println(to_string(x))
}
```

Output:

```
10
```

`let x = arr` followed by a line starting with `[0]` parses as
`let x = arr[0]` -- indexing, not two statements. `x` is `10`, an `i64`, not
the array. `(` merges the same way, turning a function call on the line
above into a continued call expression with whatever is inside the new
parentheses as an extra argument list.

Kryos has no diagnostic for any of this -- `kryos check` is clean on both
examples above, because both parse into a completely valid program; it's
just not the program you wrote. The rule to keep is mechanical, not
about remembering every case: **never start a new statement line with a
binary operator, unary `-`, `(`, or `[`.** If a value needs a leading
`-`, bind it to a name first (`let neg = -1`) instead of leaving it as the
first token on its own line.

## Common mistakes

**Forgetting `mut` before a loop accumulator.** The classic case is a running
total or a string built up across iterations:

```kryos
fn main() {
    let mut sum: i64 = 0
    let nums: [i64] = [1, 2, 3, 4]
    for n in nums {
        sum = sum + n   // needs `let mut sum` above -- confirm it's there
    }
    println(to_string(sum))
}
```

If you see `E0302: assignment to immutable variable`, the fix is always at
the `let`, not at the assignment.

**Reaching for a gated builtin in a top-level initializer.** `env_get` needs
`process`, and a top-level binding has no function to declare it on:

```kryos
let home: str = env_get("HOME")   // ERROR: no enclosing function for @capabilities

fn main() {
    println(home)
}
```

```
error[E0505]: builtin `env_get` requires `process` capability
 --> mistake.kry:1:17
  1 | let home: str = env_get("HOME")
    |                 ^^^^^^^^^^^^^^^ requires `process`
  = note: add `@capabilities(process)` to the enclosing function or actor
```

Move it into `main()`: `let home: str = env_get("HOME")` as the first line of
a `@capabilities(process) fn main()` works, because now there's a function to
carry the annotation.

## Exercises

1. Write a loop that builds a running product of `[1, 2, 3, 4, 5]` (the
   factorial of 5) using a `let mut` accumulator. Confirm you get `120`.
2. Take the shadowing example above and add a third `let count` that
   converts back to `str` with `to_string`. Print all three stages.
3. Deliberately write a `let` statement followed by a line starting with
   `(` and predict what it merges into before running `kryos check` to
   check your prediction.
4. Move the `env_get` example from "Common mistakes" into `main()` with the
   right `@capabilities` annotation and confirm it compiles.

## Summary

- `let` is immutable; `let mut` allows reassignment, and `mut` always goes
  at the declaration, never the assignment.
- A block introduces its own scope; a `let` inside it shadows an outer name
  only for that block's duration.
- Shadowing (`let count: str = ..` then later `let count: i64 = ..`)
  introduces a new binding, possibly with a different type -- it is not
  mutation.
- A top-level `let`'s initializer can only call things that need no
  capability, since there's no enclosing function to declare one on; move
  anything gated into `main()`.
- Never start a statement line with a binary operator, unary `-`, `(`, or
  `[` -- the parser has no line-number awareness and will silently merge it
  into the previous line with no diagnostic.

Next: [Functions](04-functions.md)
