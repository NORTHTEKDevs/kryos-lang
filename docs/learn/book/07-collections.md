# 07 · Collections

After this chapter you will be able to build and search arrays, maps, and
tuples, and -- the one rule in this chapter that will cost you a real bug
if you skip it -- know exactly why `push`'s return value is not optional to
use.

## Arrays: `[T]`

An array literal infers its element type; a `let` still needs the `[T]`
annotation itself once you're past a single inline literal, same as any
other type in this book:

```kryos
fn main() {
    let primes: [i64] = [2, 3, 5, 7, 11]
    let mut total: i64 = 0
    for p in primes {
        total = total + p
    }
    println("sum: " + to_string(total))
    println("count: " + to_string(len(primes)))
    println("third: " + to_string(primes[2]))
}
```

Output:

```
sum: 28
count: 5
third: 5
```

`len` gives the element count, `arr[i]` indexes (zero-based, any integer
width per [Chapter 2](02-values-and-types.md)'s casting rules), and `for`
iterates elements directly -- [Chapter 5](05-control-flow.md) already
covered the range-vs-array split for `for`.

## `push` grows in place and returns the same handle

`push(arr, item)` is not a pure function that hands back a new array --
it grows the underlying buffer **in place** and returns that same handle.
The return value is not a convenience; it is the only way to see the
grown array, because the buffer can be reallocated to a new address during
the grow (the same reason `append` in many other languages returns the new
slice/vector). The idiom is always:

```kryos
fn main() {
    let mut nums: [i64] = [1, 2, 3]
    nums = push(nums, 4)
    nums = push(nums, 5)
    println("len: " + to_string(len(nums)))
    for n in nums {
        print(to_string(n) + " ")
    }
    println("")
}
```

Output:

```
len: 5
1 2 3 4 5
```

`nums = push(nums, ...)` -- reassign the same variable, every time. This
is exactly how you'd build an array from nothing in a loop: start from
`[]`, reassign on every `push`:

```kryos
fn main() {
    let mut squares: [i64] = []
    let mut i: i64 = 1
    while i <= 5 {
        squares = push(squares, i * i)
        i = i + 1
    }
    for s in squares {
        print(to_string(s) + " ")
    }
    println("")
}
```

Output:

```
1 4 9 16 25
```

### The aliasing hazard: a second name for the same buffer

Because `push` mutates in place, holding onto the array under its *old*
name after a `push` through a *different* name does not give you the old
value back -- both names now point at the same grown buffer:

```kryos
fn main() {
    let a: [i64] = [1, 2, 3]
    let b: [i64] = push(a, 4)
    println("len(a): " + to_string(len(a)))
    println("len(b): " + to_string(len(b)))
}
```

Output:

```
len(a): 4
len(b): 4
```

Read that again: `a` was declared to hold `[1, 2, 3]` and never reassigned,
yet `len(a)` reports `4`. `push(a, 4)` grew `a`'s own backing buffer and
handed back a second handle to that exact same buffer as `b` -- there is
only one array here, under two names, and the fourth element is visible
through both. Never branch two arrays off one `push` call expecting the
original to stay at its old length; if you need an independent snapshot,
build a fresh array instead of pushing onto a shared one. The identical
rule applies to `std::collections`' `List`/`Stack`/`Queue`/`Deque` --
their `push`/`push_front`/`push_back` share a backing buffer the same way,
so the single-name reassignment idiom (`xs = xs.push(v)`) is required
there too, not just on a raw `[T]`.

There *was* a second, narrower trap in the same family -- pushing a
freshly-constructed enum value with a heap-typed field (a `str`, array, or
nested struct payload) into an array leaked memory proportional to how many
times you did it (roughly 450MB at five million fresh-enum pushes) -- but
it's fixed now: [`tools/loop/LEDGER.md`](../../../tools/loop/LEDGER.md)
item 45 closed it on both backends 2026-08-27 (the original report's "AOT
only, Cranelift/JIT clean" characterization also turned out to be a
measurement artifact of how the leak was polled, not a real backend
difference -- both leaked at an identical rate once measured correctly).
Mentioned here only so the pattern -- building enum values with heap
payloads inside a hot push loop -- doesn't look suspect on sight if you've
read older Kryos material that still describes it as open.

## `sort`/`reverse`: in-place, and `void` -- never reassign them

`sort(arr)` and `reverse(arr)` mutate their argument in place and return
nothing. They look like `push` (an array-mutating builtin) but follow the
*opposite* calling convention -- call them as a bare statement, never
assign their result:

```kryos
fn main() {
    let nums: [i64] = [3, 1, 2]
    sort(nums)
    for n in nums {
        print(to_string(n) + " ")
    }
    println("")
}
```

Output:

```
1 2 3
```

Notice `nums` above is a plain `let`, not `let mut` -- sorting mutates
*through* the array handle, the same way [Chapter 6](06-structs-and-enums.md)
showed a struct field mutating through a plain `let`. See "Common
mistakes" below for what happens if you try to treat `sort` like `push`.

## `contains`: strings and maps, not arrays

`contains(haystack, needle)` checks substring membership on a `str` and
key membership on a `map<K, V>` (either a `str`- or an integer-keyed map):

```kryos
fn main() {
    let msg: str = "hello, kryos"
    println(to_string(contains(msg, "kryos")))
    println(to_string(contains(msg, "rust")))
}
```

Output:

```
true
false
```

It does not have an array overload. This isn't just "unsupported" in the
safe, rejected-at-compile-time sense either -- the type checker accepts
`contains(some_array, x)` without complaint (its parameter types are
deliberately lenient so the same builtin can cover both a str-keyed and an
int-keyed map), but there is no array-shaped case in the runtime behind
it, so calling it on an array crashes the process instead of returning
`true`/`false`. Don't reach for `contains` on a `[T]` at all -- for array
element membership, write the linear scan yourself; it's three lines and
doubles as a review of `for`:

```kryos
fn has(nums: [i64], target: i64) -> bool {
    for n in nums {
        if n == target {
            return true
        }
    }
    return false
}

fn main() {
    let nums: [i64] = [1, 2, 3, 4]
    println(to_string(has(nums, 3)))
    println(to_string(has(nums, 9)))
}
```

Output:

```
true
false
```

## Maps: `map<K, V>`

A map literal can start empty or pre-populated; index to read, index-assign
to write, `contains` to check a key exists without triggering a fresh entry:

```kryos
fn main() {
    let mut ages: map<str, i64> = {}
    ages["ada"] = 36
    ages["grace"] = 42
    println(to_string(ages["ada"]))
    println(to_string(contains(ages, "grace")))
    println(to_string(contains(ages, "alan")))
}
```

Output:

```
36
true
false
```

There's no `for k in map` -- a bare `for` only accepts an array or a range.
Iterate a map's keys with `keys(m)`, which returns a real `[K]` you can
`for`-loop or `sort`:

```kryos
fn main() {
    let mut ages: map<str, i64> = {}
    ages["ada"] = 36
    ages["grace"] = 42
    let names: [str] = keys(ages)
    sort(names)
    for name in names {
        println(name + ": " + to_string(ages[name]))
    }
}
```

Output:

```
ada: 36
grace: 42
```

## Tuples: `(A, B, C)`

A tuple groups a fixed number of values, possibly of different types, with
no field names. Destructure with a `let` pattern, or reach into one field
at a time with `.0`, `.1`, ...:

```kryos
fn main() {
    let point: (i64, i64, str) = (3, 4, "start")
    let (x, y, label) = point
    println(label + ": (" + to_string(x) + ", " + to_string(y) + ")")
    println("via index: " + to_string(point.0))
}
```

Output:

```
start: (3, 4)
via index: 3
```

[Chapter 5](05-control-flow.md) already covered `match`ing a tuple's shape
directly (`(0, 0) => ..`, `(x, 0) => ..`) -- that's the same tuple type
this section constructs, just consumed by `match` instead of by a `let`
pattern or `.N` access.

## Common mistakes

**Treating `sort`/`reverse` like `push` and trying to reassign them.** They
return `void`, not the array:

```kryos
fn main() {
    let mut nums: [i64] = [3, 1, 2]
    nums = sort(nums)   // ERROR: sort returns void, not [i64]
    println(to_string(len(nums)))
}
```

```
error[E0100]: type mismatch: expected `[i64]`, found `void`
 --> mistake.kry:3:5
  3 |     nums = sort(nums)
    |     ^^^^^^^^^^^^^^^^^ expected type `[i64]`, found `void`
```

Drop the assignment -- `sort(nums)` alone is the correct call, exactly the
opposite fix from a forgotten `push` reassignment.

**Forgetting to reassign `push`'s result.** Covered in depth above -- the
mechanical rule is `arr = push(arr, v)`, every time, with no exception for
"I'm about to read the old variable again anyway" (you'll read the *grown*
buffer, not the old one).

## Exercises

1. Build a `[str]` from `[]` by pushing `"a"`, `"b"`, `"c"` in a loop with
   the correct reassignment idiom, then print its length and contents.
2. Write `fn has_str(items: [str], target: str) -> bool` using the same
   linear-scan pattern as `has` above. (Do not try `contains(items,
   target)` as a shortcut -- per the warning above, an array argument to
   `contains` type-checks but crashes at runtime; the linear scan is the
   only safe way to check array membership today.)
3. Build a `map<str, i64>` of at least three word-frequency counts, use
   `keys()` + `sort()` to print them in alphabetical order.
4. Write a tuple `(str, bool, i64)` representing a task (name, done,
   priority), destructure it with `let`, and print all three fields.

## Summary

- `[T]` arrays: `len`, `arr[i]`, and `for x in arr` cover reading; building
  one goes through `push`.
- `push(arr, item)` mutates its buffer in place and returns that same
  handle -- always write `arr = push(arr, item)`; a second name taken
  before a `push` through the first sees the grown buffer too, not the old
  length.
- `sort`/`reverse` are the opposite convention: in-place, `void`, called as
  a bare statement -- reassigning their result is a type error.
- `contains` works on `str` (substring) and `map<K, V>` (key membership)
  only; array element membership is a manual loop.
- `map<K, V>` has no direct `for`-loop -- iterate `keys(m)` (a real
  `[K]`) instead.
- Tuples (`(A, B, C)`) destructure with a `let` pattern or index with
  `.0`/`.1`/...; the same tuple type `match` already patterns on directly.

Next: [Strings & text](08-strings.md)
