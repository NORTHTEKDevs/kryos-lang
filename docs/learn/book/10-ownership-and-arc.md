# 10 · Ownership & ARC

After this chapter you will know exactly which of two things happens when a
value crosses a boundary in Kryos -- a function call, or a `let` assignment
-- and why those two boundaries behave differently on purpose. You will stop
reaching for a `.clone()` that doesn't exist, stop expecting `let b = a` to
alias the way it does in Python or JavaScript, know that the rule is the
same whether the value is an array, a struct, or a chain of five bindings
deep, and know the one place this model has a real, measured cost: passing
certain structs across a function call.

## The mechanism: box, refcount, retain, release

Kryos manages heap memory with **ARC** -- automatic reference counting.
Every heap-backed value (`str`, `[T]`, `map<K, V>`, a struct, an enum) lives
in one heap allocation, a **box**, with a count of how many bindings
currently point at it. Sharing the value bumps the count (a *retain*);
a binding going out of scope decrements it (a *release*); the box is freed
the instant the count hits zero.

This is not garbage collection. There is no background process scanning for
unreachable memory and no GC pause to wait out -- retains and releases are
just increments and decrements the compiler inserts at every point a
binding is created or dropped, and a box is freed at the exact statement
where its last owner disappears, not at some later sweep. There is also no
cycle collector: nothing periodically walks the heap looking for reference
cycles the way a tracing GC would. It is also not Rust's ownership model --
there is no destructive move, and reusing a binding after passing it is
never an error.

Primitives (`i64`, `f64`, `bool`, and the rest of the numeric family) never
go through any of this. They are `Copy`: passing or assigning one
duplicates the bits, full stop, and there is no box, no count, and nothing
to free.

## Why passing shares and assignment copies

Every language commits to a rule for what happens when a value crosses a
boundary, and most commit to *one* rule for both boundaries at once: Python
and JavaScript alias everywhere -- a function call and an assignment both
hand out a reference to the same object, so two names can always end up
pointing at one mutable thing. Rust moves everywhere by default -- both a
function call and an assignment consume the original binding unless you
explicitly borrow with `&` or duplicate with `.clone()`. Kryos's split --
share on call, copy on assignment -- can look arbitrary until you notice
what each operation is actually *for*.

A function call exists to let a piece of code act on a value right now, in
the caller's frame, and hand control back. Sharing the box is the obvious
choice for that: there is exactly one logical array or struct during the
call, and the callee working on the *same* box the caller already holds is
what you want -- the alternative would be a fresh, independent duplicate
every call, whose mutations would need to be threaded back out through a
return value just to have any effect at all, the way `reset` in Chapter 4
had to return a new array instead of mutating in place.

An assignment exists to give a *new name* its own independent lifetime.
`let backup = scores` reads as "I want a second thing, starting from a copy
of what `scores` currently holds, that I intend to evolve on its own from
here." If assignment aliased instead, every `let` in the program would be
one more binding into a single shared graph, and "does mutating `backup`
also change `scores`?" would depend on the whole assignment history of both
bindings, not on anything visible at the mutation site. Kryos's two rules
mean that question is always answerable from the *one* line that created
the binding: assignment -> independent, passing -> shared. There is no
third case to remember, and no aliasing history to trace back through --
the worked examples below make this concrete for an array, a struct, and a
chain of several bindings at once.

The trade-off is real on both sides: sharing on call is cheap for
`str`/`[T]`/`map` (a refcount bump) but not free for a struct, as the
struct-argument leak later in this chapter shows, and copying on
assignment means assigning a large array or a deeply nested struct does
real work, not just rebinds a name. Neither rule is free. The two-rule
split is Kryos betting that "predictable from the syntax alone" is worth
more than "uniformly cheap."

## Two boundaries, two rules

Watch both rules fire in the same program:

```kryos
fn double_first(nums: [i64]) {
    nums[0] = nums[0] * 2
}

fn main() {
    let mut scores: [i64] = [10, 20, 30]
    double_first(scores)
    println("after double_first: " + to_string(scores[0]))

    let mut backup: [i64] = scores
    backup[0] = 999
    println("scores[0]: " + to_string(scores[0]))
    println("backup[0]: " + to_string(backup[0]))
}
```

Output:

```
after double_first: 20
scores[0]: 20
backup[0]: 999
```

`double_first(scores)` shares the array -- `scores` and the `nums` parameter
inside `double_first` are the same box, so mutating `nums[0]` is visible on
`scores` the moment the call returns. `let backup: [i64] = scores` is a
different operation entirely: it deep-copies the array, so `backup` starts
as an independent value, and mutating `backup[0]` never touches `scores`.
Same source array, two boundaries, two outcomes -- and notice that `scores`
was read again *after* being passed to `double_first`, with nothing to
reconstruct and no clone to call first. Passing never consumes a value in
Kryos.

## Copy types don't play this game at all

| Copy (duplicated, no box) | ARC-backed (box + refcount) |
|---|---|
| `i8`..`i64`, `u8`..`u64` | `str` |
| `f32`, `f64` | `[T]` |
| `bool` | `map<K, V>` |
| tuples of the above | structs, enums |

For a `Copy` type, the "two rules" distinction above doesn't even apply --
passing and assigning both duplicate the value, so there is only ever one
independent copy per binding, on both sides of either boundary. The
exhaustive type list (including the narrow integer widths and `repr(C)`
structs) is in
[`docs/06-ownership.md`](../../06-ownership.md#copy-types), which also
covers drop order and the currently-parsed-but-not-fully-enforced `&T`/`&mut
T` reference syntax in more depth than this chapter needs.

## Structs and enums play by the same two rules

Everything above used an array. Structs (and enums) follow the *identical*
two rules -- pass shares, assign copies -- with one difference from
[Chapter 4](04-functions.md#struct-arguments-the-exception)'s
`shift_point` example: that chapter showed only the pass-shares half. Here
is the full picture, one struct that gets mutated through a parameter *and*
copied through an assignment in the same program:

```kryos
struct Account {
    balance: i64
}

fn withdraw(acc: Account, amount: i64) {
    acc.balance = acc.balance - amount
}

fn main() {
    let acc: Account = Account { balance: 100 }
    withdraw(acc, 30)
    println("after withdraw: " + to_string(acc.balance))

    let mut copy: Account = acc
    copy.balance = 0
    println("acc.balance: " + to_string(acc.balance))
    println("copy.balance: " + to_string(copy.balance))
}
```

Output:

```
after withdraw: 70
acc.balance: 70
copy.balance: 0
```

`withdraw(acc, 30)` shares `acc`'s box with the `acc` parameter inside
`withdraw`, so the mutation to `.balance` is visible the instant the call
returns -- `acc.balance` reads `70`, not `100`, with no return value doing
the work. `let copy: Account = acc` is the other boundary: it deep-copies
`Account`'s field into a fresh, independent box, so setting
`copy.balance = 0` never touches `acc` -- the final two lines read `70` and
`0`, not `0` and `0`. Same struct, same starting value, two boundaries, two
outcomes -- exactly the array example above, just with a struct instead of
`[i64]`.

`Account` is scalar-only on purpose here (a single `i64` field) -- this
isolates the share-vs-copy behavior from the struct-argument leak covered
later in this chapter, which only shows up once a struct field is itself
heap-backed.

## Multiple bindings, one box or many

The array and struct examples above each used two bindings. The rule
generalizes cleanly to more of them: every assignment produces its *own*
independent box, and a value threaded through a chain of function calls
keeps sharing the *same* box no matter how many hops it goes through.

```kryos
fn increment_first(nums: [i64]) {
    nums[0] = nums[0] + 1
}

fn main() {
    let mut original: [i64] = [1, 2, 3]
    let mut copy_a: [i64] = original
    let mut copy_b: [i64] = original

    copy_a[0] = 100

    println("original[0]: " + to_string(original[0]))
    println("copy_a[0]: " + to_string(copy_a[0]))
    println("copy_b[0]: " + to_string(copy_b[0]))

    increment_first(original)
    increment_first(original)
    println("original[0] after two increments: " + to_string(original[0]))
}
```

Output:

```
original[0]: 1
copy_a[0]: 100
copy_b[0]: 1
original[0] after two increments: 3
```

`copy_a` and `copy_b` are each assigned directly from `original`, so each
gets its *own* deep copy -- three independent boxes exist after those two
lines, not one box with three names pointing at it. Mutating `copy_a[0]`
changes nothing about `copy_b` or `original`, and `copy_b` still reads the
original value `1` even though `copy_a` was mutated right next to it.
Contrast that with `increment_first(original)` called twice in a row: both
calls pass the *same* `original` binding, so both mutations land on the
*same* box -- `original[0]` ends at `3` (started at `1`, incremented
twice), the same accumulating behavior you would get from calling a
mutating method on one object in Python. Whenever this gets confusing, the
question that resolves it is always the same one: did the value cross an
assignment (new independent box) or a function call (same box, new binding
to it) -- not how many bindings or calls were involved along the way.

## The advisory move lint

The compiler runs an ownership analysis pass that can attach an `E0300:
use of moved value` diagnostic to a value that's passed and then read
again -- but it is advisory, inconsistent about when it actually fires, and
never blocks compilation. The exact program from the worked example above,
with `scores` reused after passing it to `double_first`, produces zero
diagnostic output from `kryos check` and compiles clean. Run
`kryos explain E0300` if you want the long-form description of what the
lint is trying to flag; don't restructure working code to silence it, and
don't treat its absence or presence as a correctness signal the way you
would a Rust borrow-check error.

## The one real cost: the struct-argument leak

Everything above describes `str`, `[T]`, and `map<K, V>` params -- true
borrows, sharing a box with no ownership bookkeeping of their own. Struct
(and enum) parameters look identical from the outside -- mutating a field
through one is visible to the caller exactly like the array case above, as
[Chapter 4](04-functions.md#struct-arguments-the-exception) and the
`Account` example above both showed -- but underneath, a struct parameter
carries its own retain/drop accounting at the call boundary instead of
being a plain shared reference. Today, that accounting doesn't fully agree
with the accounting used for an ordinary local variable's lifetime: a
struct crossing a function call is tracked by a different piece of the
runtime than the piece that frees an everyday local, and the two don't
always agree on when the last owner is actually gone.

The measured consequence is a real leak, not a theoretical one: passing a
struct with heap-backed fields (a `str`, an array, or a nested struct)
across a function call leaks roughly 86MB per million calls
([`tools/loop/LEDGER.md`](../../../tools/loop/LEDGER.md), item 3 -- eight
separate fix attempts are recorded there, all ruled out, because a real fix
means unifying the two accounting paths, which is a bigger change than a
point patch this close to 1.0). A struct built only from scalar fields --
`Account` above included -- never hits this; there's no heap data for the
two paths to disagree about. In a hot loop that passes the same struct
thousands or millions of times, read the fields you need directly instead
of passing the whole struct, or keep heap-backed data out of structs that
cross call boundaries.

## Common mistakes

**Expecting assignment to alias the way passing does.** If you predicted
`backup[0] = 999` in the worked example above would also change
`scores[0]` -- because that's what happened when `double_first` mutated
`nums[0]` through its parameter -- that's the mistake worth naming
explicitly. `let backup = scores` and `double_first(scores)` both look like
"hand this array to something else," but only one of them shares the box.
The same trap applies identically to structs: `let copy = acc` and
`withdraw(acc, 30)` in the `Account` example above look just as similar to
each other, and behave just as differently. There is no syntax difference
to warn you either way; the rule is which *kind* of boundary you crossed,
not anything visible at the call site.

**Assuming a struct parameter is exactly as cheap as an array or map
parameter.** They read identically -- both let you mutate through the
parameter and both leave the caller's original valid afterward (Chapter
4's `shift_array` and `shift_point` examples produce the same *shape* of
result). But only the struct case carries the ownership-transfer accounting
above, so a struct with heap fields passed in a hot loop pays a real,
measured memory cost that an equivalent `[T]`/`map` parameter never does.
If a function is called millions of times per second, this is the one
place the "they behave the same" mental model breaks down.

## Exercises

1. Take the array worked example above and add a third binding,
   `let mut alias: [i64] = backup`. Predict whether mutating `alias[0]`
   affects `backup`, then run it and check.
2. Take the `Account` example above and add a third binding,
   `let mut copy2: Account = acc`, right after `withdraw(acc, 30)`.
   Predict what `copy2.balance` prints both before and after
   `copy.balance = 0` runs, then run it and check.
3. Read `tests/mem/struct_arg_leak.kry` in this repo. Without running the
   full soak test, identify which of its functions pass a struct with a
   heap field and which pass a scalar-only struct -- the file's own naming
   makes this discoverable.
4. In your own words, explain why Kryos deep-copies on assignment but
   shares on function call (see "Why passing shares and assignment
   copies" above). Then write a two-line program that would print a
   different result than it does today if assignment aliased instead of
   copying.

## Summary

- ARC = automatic reference counting: heap-backed values (`str`, `[T]`,
  `map<K, V>`, structs, enums) live in a refcounted box, freed
  deterministically the instant the refcount hits zero -- no GC pause, no
  cycle collector; primitives are `Copy` and never touch this machinery.
- Two different boundaries, two different rules: passing a value into a
  function call **shares** it (retain, mutation visible to the caller);
  assigning it to a new binding **deep-copies** it (independent value from
  that point on) -- true for `str`, `[T]`, `map`, structs, and enums alike,
  no matter how many bindings or calls are chained together.
- Reuse-after-pass always works -- there is no destructive move and no
  `.clone()` method, because assignment already gives you an independent
  copy when you actually want one.
- The split is deliberate, not arbitrary: a call shares because there is
  one logical value to act on right now; an assignment copies because a
  new name is supposed to get its own independent lifetime, answerable
  from the `let` line alone.
- `E0300` is an advisory lint, not a hard error -- it doesn't consistently
  fire and never blocks compilation.
- Struct parameters with heap-backed fields have a real, measured cost in a
  hot loop (~86MB/million calls, LEDGER item 3, open) that `[T]`/`map`
  parameters don't -- read fields directly or keep heap data out of structs
  crossing call boundaries.

Next: [Capabilities](11-capabilities.md)
