# 04 · Functions

After this chapter you will know how to declare a function, the two ways to
return a value from one, and -- the part that actually surprises people
coming from Rust or C++ -- exactly what happens to an argument after you pass
it: whether the function you called can see mutations you make to it
afterward, whether it can mutate what you passed in, and whether you can
keep using your own copy once the call returns.

## Declaring a function

```kryos
fn add(a: i64, b: i64) -> i64 {
    return a + b
}

fn square(n: i64) -> i64 {
    n * n
}

fn main() {
    println(to_string(add(2, 3)))
    println(to_string(square(6)))
}
```

Output:

```
5
36
```

`fn` name, parameters in parens with a type on each one, `->` and a return
type if the function produces a value, then a body in braces. Every
parameter needs an explicit annotation -- there's nothing in `fn add(a, b)`
alone to tell the checker what `+` should mean, and Chapter 2 covers why
this is one of the two places (alongside top-level `let`) Kryos never
infers.

## Two ways to return a value

`add` above uses an explicit `return`. `square` returns its body's last
expression instead -- no `return` keyword, and no trailing content after
`n * n`. Both are real, both ran in the example above. Prefer explicit
`return` for anything longer than a one-liner: it's unambiguous about which
expression is the function's result, and it lets you exit early from the
middle of a function body, which a tail expression can't do.

## Borrow semantics: what a function does to what you pass it

This is the part worth reading slowly, because it's the one place Kryos's
model genuinely differs from what a Rust or C++ background trains you to
expect.

**A parameter is a borrow: the caller keeps ownership, the callee just gets
to read and mutate through it.** Passing a `str`, `[T]`, or `map<K, V>`
shares the same underlying heap data with the callee -- no copy -- and the
caller's binding stays completely valid to keep using after the call
returns. Mutating *through* the parameter (an array element, a map entry)
is visible to the caller, because there's only one array behind both names.
Reassigning the *whole* parameter binding, on the other hand, is rejected --
a parameter name itself is never mutable, regardless of what it points at:

```kryos
fn shift_array(nums: [i64]) {
    nums[0] = nums[0] + 100
}

fn main() {
    let nums: [i64] = [1, 2, 3]
    shift_array(nums)
    println("array: " + to_string(nums[0]))
    println("still usable, len " + to_string(len(nums)))
}
```

Output:

```
array: 101
still usable, len 3
```

`shift_array` mutated index `0` in place, and the caller sees it -- `nums`
in `main` and `nums` inside `shift_array` are the same array. And `nums` in
`main` is still a perfectly good array afterward; there is no `.clone()` to
call and no destructive move to work around, because passing a value never
consumed it in the first place.

Trying to replace the whole parameter, instead of mutating through it, is a
compile error:

```kryos
fn reset(nums: [i64]) {
    nums = [0, 0, 0]   // ERROR: nums itself is never mutable
}

fn main() {
    let nums: [i64] = [1, 2, 3]
    reset(nums)
}
```

```
error[E0302]: assignment to immutable variable `nums`
 --> mistake.kry:2:5
  2 |     nums = [0, 0, 0]
    |     ^^^^^^^^^^^^^^^^ help: consider declaring with `let mut`
```

If you actually want to produce a new value and hand it back, shadow the
parameter with a local `let mut` first, then return it -- don't fight the
checker into making the parameter itself reassignable:

```kryos
fn reset(nums: [i64]) -> [i64] {
    let mut local = nums
    local = [0, 0, 0]
    return local
}

fn main() {
    let nums: [i64] = [1, 2, 3]
    let cleared: [i64] = reset(nums)
    println("original: " + to_string(nums[0]))
    println("cleared: " + to_string(cleared[0]))
}
```

Output:

```
original: 1
cleared: 0
```

`nums` in `main` never changed -- `reset` built an independent value and
returned it, which is a completely different thing from mutating the
argument in place.

### Struct arguments: the exception

Struct (and enum) parameters *look* identical from the outside -- mutating a
field through a struct parameter is visible to the caller exactly like the
array case above:

```kryos
struct Point { x: i64, y: i64 }

fn shift_point(p: Point) {
    p.x = p.x + 100
}

fn main() {
    let pt: Point = Point { x: 1, y: 2 }
    shift_point(pt)
    println("x: " + to_string(pt.x))
}
```

Output:

```
x: 101
```

Same visible-mutation behavior, same "the whole binding `p` is still
immutable" rule, same reuse-after-pass safety. Where structs differ is
underneath, not in anything you can observe from this kind of example: a
`str`/`[T]`/`map` parameter is a **true borrow** with no ownership
bookkeeping at the call boundary, but a struct parameter is handled with
**ownership-transfer semantics** -- the callee's copy carries its own
retain/drop accounting. This has a real, measured cost: passing a struct
that has heap-backed fields (a `str`, array, or nested struct field) across
a function call leaks roughly 86MB per million calls -- an open design note
in this compiler, not something you did wrong
([`tools/loop/LEDGER.md`](../../../tools/loop/LEDGER.md), item 3). A struct
made only of scalar fields (`i64`, `f64`, `bool`) doesn't hit this, since
there's no heap data to mismanage. In a hot loop that passes the same
struct thousands or millions of times, read the fields you need directly
instead of passing the whole struct, or keep heap-backed data out of
structs that cross call boundaries.

### Scalars are different again: real copies

Primitives (`i64`, `f64`, `bool`) are not heap handles at all -- passing one
copies the value, full stop, and a mutation inside the callee is invisible
to the caller no matter what you do to it:

```kryos
fn try_double(n: i64) {
    let mut local = n
    local = local * 2
    println("inside: " + to_string(local))
}

fn main() {
    let n: i64 = 21
    try_double(n)
    println("outside: " + to_string(n))
}
```

Output:

```
inside: 42
outside: 21
```

This is the baseline every other type deviates from in the direction of
*more* sharing, not less: scalars copy, `str`/`[T]`/`map` share a heap
handle without owning it, and structs share a heap handle while also owning
a copy of the bookkeeping around it. Chapter 10 (Ownership and ARC) is where
this whole family of rules gets its formal treatment; for now, the
behavioral rule -- mutate through it and the caller sees it, reassign the
whole binding and the checker stops you, reuse the original after passing
it and it just works -- covers everything you'll hit before then.

## Common mistakes

**Assuming a passed value needs cloning to keep using it.** There is no
`.clone()` method and no "moved value" hard error in Kryos. A value passed
to a function stays valid in the caller:

```kryos
fn consume(x: str) {
    println(x)
}

fn main() {
    let s: str = "hello"
    consume(s)
    println(s)   // fine -- s was never moved
}
```

```
hello
hello
```

Coming from Rust, reaching for `.clone()` here or restructuring code to
avoid a "use after move" is solving a problem Kryos doesn't have.

**Trying to reassign a parameter instead of mutating through it or
returning a new value.** Covered in detail above -- the fix is either mutate
a field/element (`p.x = ..`, `arr[i] = ..`), or shadow with a local
`let mut` and return the result.

## Exercises

1. Write `fn double_all(nums: [i64])` that mutates every element of its
   array argument in place (no return value). Call it and confirm the
   caller's array changed.
2. Write `fn scaled(nums: [i64], factor: i64) -> [i64]` that does *not*
   mutate its argument -- it builds and returns a new array instead. Call
   it and confirm the original array is untouched.
3. Write a struct `Counter { value: i64 }` and a function that takes one by
   parameter and increments its field. Confirm the caller's struct changed,
   the same way the `Point` example did.

## Summary

- Every parameter needs an explicit type annotation; a function's return
  type follows `->`.
- A function body's last expression is returned automatically if there's no
  trailing `return` -- prefer explicit `return` for anything nontrivial.
- `str`/`[T]`/`map` parameters are true borrows: mutating through the
  parameter (an element, an entry) is visible to the caller, reassigning
  the whole parameter binding is a compile error, and the original binding
  stays valid to reuse after the call returns.
- Struct/enum parameters behave identically from the outside but carry
  ownership-transfer semantics underneath -- the cost is a real, measured
  memory leak (~86MB/million calls) when a struct's fields are heap-backed,
  an open design note (LEDGER item 3), not a bug in your code.
- Scalars (`i64`, `f64`, `bool`) are plain copies -- a mutation inside the
  callee is never visible to the caller.

Next: [Control flow](05-control-flow.md)
