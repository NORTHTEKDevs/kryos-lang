# 12 · Error Handling

After this chapter you will be able to choose between two real error-handling
mechanisms -- a typed `Result<T, E>` your caller's compiler forces them to
handle, or a `throw` that unwinds to the nearest `catch` -- and, more
importantly, you will know exactly which failures neither mechanism can
touch at all, because Kryos draws a hard line between an *error* (something
your code decided to report) and a *panic* (something the runtime decided
your program cannot safely continue past). Confusing the two is the most
common early mistake with this part of the language.

## Two ways to signal failure

Kryos gives you both a typed-value mechanism and an unwinding mechanism,
and leaves the choice to you:

- **`Result<T, E>`** -- a function that can fail returns `Ok(value)` or
  `Err(error)`, and the caller's `match` (or the `?` operator) is how you
  handle it. The failure is part of the function's return type, visible at
  every call site.
- **`throw` / `try` / `catch`** -- a function raises an error with `throw`,
  and it unwinds the call stack until something catches it. Nothing in the
  function's signature says it can fail; the caller finds out by wrapping
  the call in `try` or by the program crashing if nobody does.

Neither one is the "old" or "new" way -- both are live, idiomatic Kryos, and
which one to reach for is a design choice: `Result` when you want the type
signature to force every caller to acknowledge failure is possible, `throw`
when threading a `Result` through several layers of call chain is more
ceremony than the failure is worth.

## `Result<T, E>`: `Ok` and `Err`

`Result` is a two-variant enum from `std::result`. Import the pieces you
use:

```kryos
use std::result::{Result, Ok, Err}

fn divide(a: i64, b: i64) -> Result<i64, str> {
    if b == 0 {
        return Err("division by zero")
    }
    return Ok(a / b)
}

fn main() {
    match divide(10, 2) {
        Ok(v)  => println("ok: " + to_string(v)),
        Err(e) => println("err: " + e),
    }
    match divide(10, 0) {
        Ok(v)  => println("ok: " + to_string(v)),
        Err(e) => println("err: " + e),
    }
}
```

Output:

```
ok: 5
err: division by zero
```

`match` is how you handle both variants -- there is no `.unwrap()` habit to
reach for here; the two arms are the whole contract.

### Always annotate the signature

Write `Result<T, E>` with both type arguments filled in on every function
signature that returns one, never a bare `Result`. This is not a style
preference -- a bare `Result` erases its payload to `i64`, and a `str`/struct
value read back out through it renders as a raw pointer instead of the
value you put in. The "Common mistakes" section below shows this happening
for real, with real output.

## Propagating errors with `?`

Postfix `?` on a `Result<T, E>`-returning call unwraps `Ok` and returns
early with the same `Err` if the result was one -- the same shape as Rust's
`?`, and it works identically on both backends:

```kryos
use std::result::{Result, Ok, Err}

fn parse_positive(s: str) -> Result<i64, str> {
    let n: i64 = parse_int(s)
    if n <= 0 {
        return Err("not positive: " + s)
    }
    return Ok(n)
}

fn sum_two(a: str, b: str) -> Result<i64, str> {
    let x: i64 = parse_positive(a)?
    let y: i64 = parse_positive(b)?
    return Ok(x + y)
}

fn main() {
    match sum_two("3", "4") {
        Ok(v)  => println("sum: " + to_string(v)),
        Err(e) => println("error: " + e),
    }
    match sum_two("3", "-1") {
        Ok(v)  => println("sum: " + to_string(v)),
        Err(e) => println("error: " + e),
    }
}
```

Output:

```
sum: 7
error: not positive: -1
```

`sum_two("3", "-1")` never reaches its `return Ok(x + y)` line -- the second
`parse_positive(b)?` call gets `Err("not positive: -1")` back, and `?`
returns that `Err` from `sum_two` immediately, without a `match` at every
step. `?` is only valid inside a function whose own return type is a
`Result` (with a compatible `Err` type); it is the propagation tool, not a
replacement for handling the error somewhere.

## `throw` / `try` / `catch`

`throw` raises a value; the nearest enclosing `try`/`catch` unwinds to it:

```kryos
fn main() {
    try {
        throw 42
    } catch e {
        println("caught: " + e)
    }
}
```

Output:

```
caught: 42
```

### The catch variable is always a `str`

Whatever you `throw` -- a string, an integer, anything with a string
representation -- is converted to that representation at the `throw` site,
the same conversion `"{x}"` interpolation uses. `throw 42` above is caught
as the string `"42"`, not the integer `42`; `println("caught: " + e)` works
because `e` is already a `str` and `+` concatenates strings. Build
structured messages by throwing an interpolated string
(`throw "connect failed: {host}:{port}"`) rather than expecting to recover
a typed value on the other side of a `catch`.

### Uncaught exceptions

A `throw` that unwinds past `main` with no `catch` to stop it prints the
exception to stderr and exits with status **101**:

```
$ kryos run boom.kry
kryos: uncaught exception: kaboom
$ echo $?
101
```

## The line that matters: `throw` vs panic

`catch` only ever catches an explicit `throw` (including a stdlib function
that fails by throwing, like `std::json::parse`). It does **not** catch a
runtime *panic* -- and Kryos has two visibly different exit codes for the
two categories, which is a useful thing to know when a process dies and you
need to tell which kind of failure you're looking at:

| Cause | Exit code | Caught by `try`/`catch`? |
|---|---|---|
| An explicit `throw` that unwinds to a `catch` | -- (handled) | Yes |
| An explicit `throw` that reaches nobody | 101 | No -- nothing left to catch it |
| Explicit `panic("message")` | 101 | No |
| Division or modulo by zero | 98 | No |
| Array/string index out of bounds | 98 | No |
| `checked_*` arithmetic overflow | 98 | No |
| `file_read` on a missing/unreadable file | 98 | No |
| `parse_int`/`parse_float` on invalid input | 98 | No |

Watch a division-by-zero refuse to be caught, even wrapped directly in a
`try`:

```kryos
fn main() {
    let b: i64 = 0
    try {
        let x: i64 = 10 / b
        println("never")
    } catch e {
        println("caught: " + e)
    }
    println("after")
}
```

Running this prints only the panic and stops -- neither `"never"`,
`"caught: ..."`, nor `"after"` ever print:

```
kryos panic: integer division by zero
stack trace (most recent call last):
  0: main() at boom.kry:4
```

The process exits 98 before the `catch` block gets a chance to run. The
same is true of an explicit `panic("message")` call inside a `try` -- it
aborts the process exactly the same way, exit 101, `catch` never
consulted. The rule that matters in practice: `throw` is *you* deciding a
failure is recoverable and reporting it as a value; a panic -- whether
triggered implicitly by an operation like division, or explicitly via
`panic(...)` -- is the runtime (or you) deciding the program cannot safely
continue, and it never asks a `catch` block for permission to keep going.
Guard the precondition before the operation (`if b != 0 { ... }`, a bounds
check, `file_exists(path)` before `file_read`) -- there is nothing to catch
after the fact.

(`docs/07-error-handling.md` also describes a planned "self-healing
runtime" that would auto-recover from panics like these. It does not exist
in 0.9.0 -- every `@intent`/`@constraint`/`@fallback` attribute is a parsed
no-op today, and `--heal-report` is not a recognized flag. `try`/`catch`
plus an explicit guard is the entire error-recovery toolkit that actually
ships.)

## Common mistakes

**Writing a bare `Result` instead of `Result<T, E>`.** This compiles, and
silently corrupts the payload:

```kryos
use std::result::{Result, Ok, Err}

fn divide(a: i64, b: i64) -> Result {
    if b == 0 {
        return Err("division by zero")
    }
    return Ok(a / b)
}

fn main() {
    match divide(10, 0) {
        Ok(v)  => println("ok: " + to_string(v)),
        Err(e) => println("err: " + to_string(e)),
    }
}
```

Output:

```
err: 140697634537472
```

That number is a raw pointer, not the string `"division by zero"` --
`Result` with no type arguments erases its payload to `i64` (the same
erasure CLAUDE.md's gotcha #13 documents for `Option`), and printing an
`i64`-erased `str` prints the bit pattern instead of the text. The fix is
the annotated signature from the top of this chapter:
`Result<i64, str>`. The same erasure hits `std::result::to_array<T>`: call
it with the return binding annotated (`let a: [str] = to_array(r)`), not
bare (`let a = to_array(r)`), or you get the identical raw-pointer failure
-- an open gap tracked as
[LEDGER item 40c](../../../tools/loop/LEDGER.md), since `to_array`'s `T`
has nothing to bind against on the argument side and can only be resolved
from an explicit annotation.

**Wrapping a panicking builtin in `try`/`catch` and expecting it to help.**
The global `file_read` builtin panics (uncatchable, exit 98) if the file is
missing -- `try`/`catch` around it changes nothing:

```kryos
@capabilities(fs:read)
fn main() {
    try {
        let content: str = file_read("does_not_exist.txt")
        println(content)
    } catch e {
        println("caught: " + e)
    }
    println("after")
}
```

```
kryos panic: file_read: does_not_exist.txt: The system cannot find the file specified. (os error 2)
```

Neither `"caught: ..."` nor `"after"` print -- the panic ends the process
before `catch` runs. For a failure you can actually recover from, call the
throwing stdlib wrapper instead:

```kryos
use std::fs::{read_file}

@capabilities(fs:read)
fn main() {
    try {
        let content: str = read_file("does_not_exist.txt")
        println(content)
    } catch e {
        println("caught: " + e)
    }
    println("after")
}
```

```
caught: fs error: could not open file for reading: does_not_exist.txt
after
```

Same missing file, same `try`/`catch` shape -- the only thing that changed
is which function reports the failure. `read_file` throws; `file_read`
panics. Check `docs/stdlib/fs.md` before assuming a given I/O function is
the recoverable one.

## Exercises

1. Write `fn safe_divide(a: i64, b: i64) -> Result<i64, str>` and a
   `main` that calls it with `b = 0` and `b = 2`, handling both with
   `match`. Then rewrite `main` to use `?` instead by giving it its own
   `Result`-returning wrapper function.
2. Take the bare-`Result` example from "Common mistakes" and add the
   `<i64, str>` type arguments back. Run it and confirm `err:` now prints
   the real message instead of a pointer.
3. Write a function that throws a struct's field value via string
   interpolation (`throw "invalid age: {age}"`) instead of a plain string
   literal, and catch it in `main`.
4. Predict, then verify: does an array index out of bounds inside a
   `catch` block (not the `try` block) get caught by that same `catch`? Why
   or why not, given what you know about panics now?

## Summary

- `Result<T, E>` (from `std::result`, `Ok`/`Err`) is the typed mechanism;
  `throw`/`try`/`catch` is the unwinding mechanism. Both are real,
  idiomatic Kryos -- pick based on whether you want the type signature to
  force callers to handle failure.
- Always write the full `Result<T, E>` on a signature -- a bare `Result`
  erases its payload to `i64` and silently corrupts a `str`/struct value on
  read-back. The same annotation requirement applies to
  `std::result::to_array<T>`'s binding (LEDGER item 40c).
- `?` on a `Result`-returning call unwraps `Ok` or returns the `Err` early;
  it only works inside a function that itself returns a compatible
  `Result`.
- Whatever you `throw` is stringified at the throw site -- the `catch`
  variable is always a `str`, regardless of what type you threw.
- An uncaught `throw` exits 101. A runtime panic (division by zero, index
  out of bounds, `file_read` on a missing file, `parse_int` on bad input,
  or an explicit `panic("msg")`) exits 98 for the implicit cases and 101
  for `panic(...)`, and **none of them are catchable** -- `try`/`catch`
  only ever intercepts an explicit `throw`. Guard the precondition before
  the operation; there is nothing to catch afterward.

Next: [Concurrency: spawn/channels/actors](13-concurrency.md)
