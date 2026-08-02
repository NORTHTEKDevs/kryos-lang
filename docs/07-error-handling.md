# Error Handling

> **Implementation Status:** `try`/`catch`/`throw` is fully implemented -- parsed, lowered to Result-enum-based control flow in MIR, and compiled through both backends. Nested try/catch and throwing any value type all work. `catch` catches any `throw` (including stdlib functions that fail by throwing); it does **not** catch runtime panics such as division by zero or index-out-of-bounds (see "What `catch` catches" below). The **self-healing runtime** (automatic recovery from division by zero, index clamping, `@intent`, `@constraint`, `@fallback` attributes, and `--heal-report`) is a **roadmap feature** and is not yet implemented.

Kryos has two complementary systems for dealing with errors: explicit `try`/`catch`/`throw` for errors you expect, and a self-healing runtime that automatically recovers from common runtime faults. Understanding when to use each is key to writing robust programs.

## try / catch / throw

The basic mechanism for handling errors is `try`/`catch`. Wrap code that might fail in a `try` block, and handle the error in the `catch` block:

```
try {
    throw "custom error"
} catch e {
    println(e)  // custom error
}
```

The catch block receives the error value in the variable you name between the parentheses (here, `e`).

### Throwing errors

Use `throw` to signal an error. You can throw any value -- strings are the most common:

```
throw "something went wrong"
```

The thrown value becomes the catch variable in the nearest enclosing catch block.

### When nothing goes wrong

If the try block completes without throwing, the catch block is skipped entirely:

```
try {
    let x = 42
    println(x)  // 42
} catch e {
    println("should not print")
}
```

### Execution continues after try/catch

After the try/catch block finishes (whether through the try path or the catch path), execution continues normally:

```
try {
    throw "oops"
} catch e {
    println(e)  // oops
}
println("after try/catch")  // this runs
```

### The catch variable is a string

Whatever you `throw` is converted to its string representation at the throw
site -- the same conversion `"{x}"` interpolation uses. The catch variable
is therefore always a `str`:

```
try {
    throw 42
} catch e {
    println(e)          // 42
    println("got {e}")  // got 42
}
```

Strings pass through unchanged; ints, floats, and bools become their printed
form. Throw strings (or build one with interpolation) when you want
structured messages: `throw "connect failed: {host}:{port}"`.

### Uncaught exceptions

If a thrown value unwinds all the way out of `main` without hitting a
`catch`, the program prints the exception to stderr and exits with code
**101** on both backends:

```
$ kryos run boom.kry
kryos: uncaught exception: kaboom
$ echo $?
101
```

### Exceptions in spawned threads

A `throw` that unwinds out of a spawned thread's entry function is reported
to stderr (`kryos: uncaught exception in spawned thread: <msg>`); the thread
dies and the process keeps running — the same model as a Rust thread panic.
Catch inside the thread if you need to react to the failure.

### Nested try/catch

Try/catch blocks can nest. An inner catch can throw a new error that gets caught by an outer catch:

```
try {
    try {
        throw "inner"
    } catch e {
        println("inner: " + e)  // inner: inner
        throw "outer"
    }
} catch e {
    println("outer: " + e)     // outer: outer
}
```

This is useful for layered error handling -- handle what you can at the inner level, and escalate anything else.

### What `catch` catches (and what it does not)

`catch` catches any `throw` -- including standard-library functions that
signal failure by throwing, such as `std::json::parse` and `std::fs::read_file`:

```
try {
    let result = parse("not valid json")   // use std::json::{parse}
} catch e {
    println("Parse failed: " + e)          // caught: json: expected ...
}
```

`catch` does **not** catch runtime *panics*. Integer division by zero,
array/string index out of bounds, and builtin failures such as `file_read`,
`parse_int`, or `parse_float` on invalid input abort the process (exit 98) and
are **not** recoverable with `try`/`catch`:

```
try {
    let x = 10 / 0        // PANIC: aborts here -- the catch never runs
} catch e {
    println("never runs")
}
```

Guard those with an explicit precondition (`if b != 0 { ... }`, a bounds check,
`file_exists` before `file_read`) or a `Result`-returning wrapper. Only an
explicit `throw` (or a library that throws) unwinds to a `catch`.

## The self-healing runtime (roadmap -- NONE OF THIS EXISTS YET)

> **Not yet implemented, at all.** Everything in this section -- automatic
> division/index/type-coercion recovery, `--heal-report`, and the
> `@intent`/`@constraint`/`@fallback` attributes -- describes a **planned**
> feature, not current behavior. It is written in present tense below only
> because that is how the target design reads; treat every sentence in this
> section as "the plan is for X to work this way," never as "X works this
> way." Verified directly against this commit, not inferred: `@constraint`
> is silently a no-op (`clamp_percent(150.0)` returns `150`, not the `100`
> the example below claims), and `--heal-report` is not a recognized CLI
> flag (`kryos run --heal-report f.kry` errors with "unexpected argument").
> Division by zero and index-out-of-bounds are hard *panics* that abort the
> process today -- they are **not** caught by `try`/`catch` (only an
> explicit `throw`, or a library that throws, unwinds to a catch; see "What
> `catch` catches" above) and nothing recovers or clamps them. If your
> program needs to survive these, write the guard yourself (`if b != 0 {
> ... }`, a bounds check) -- there is no runtime safety net to lean on.

This is where Kryos is intended to diverge from every other language. The
plan: when self-healing is enabled, the runtime would automatically recover
from certain classes of errors instead of crashing.

### How it's intended to work

The self-healing engine would sit between your code and the runtime. When an
operation fails, the engine would check whether it knows how to fix the
problem, apply the fix, log what happened, and continue execution -- your
program would never see the error. **None of this exists today; every
operation below either panics (uncatchable) or, for `@intent`/`@constraint`/
`@fallback`, silently does nothing at all** (the attribute parses and is
ignored -- it neither errors nor has any runtime effect).

### Planned recovery strategies

**Division by zero** -- would substitute 0:

```
let x = 10 / 0  // TODAY: hard panic, process aborts. PLANNED: x = 0, logged as a heal action.
```

**Index out of bounds** -- would clamp to the nearest valid index:

```
let data = [10, 20, 30]
let val = data[99]  // TODAY: hard panic. PLANNED: val = 30 (clamped to last element).
```

**Type mismatch in operations** -- would coerce types:

```
let result = "5" + 10  // TODAY: this is actually a compile error (E0100) -- + requires matching types.
// PLANNED: coerces 10 to "10", result = "510"
```

**Missing key or attribute** -- would return none:

```
// PLANNED: accessing a field that does not exist on a struct would return
// none instead of crashing. TODAY: an unknown field is a compile error
// (E0107/similar); this scenario cannot even be constructed.
```

### Planned heal actions

The design classifies each planned auto-recovery as one of these actions.
None of these are emitted by the compiler or runtime today:

| Action | What it would do |
|--------|-------------|
| `RETRY` | Retry the operation (after input coercion) |
| `COERCE` | Convert types to match expectations |
| `CLAMP` | Clamp a value to a valid range |
| `FALLBACK` | Use a fallback value or function |
| `RECONSTRUCT` | Roll back to the last known good state |
| `SKIP` | Skip the failing operation safely |
| `SUBSTITUTE` | Replace with an equivalent operation |

### The planned heal report

The design calls for every self-healing action to be logged and inspectable:

```bash
kryos run --heal-report program.kry   # NOT A REAL FLAG TODAY -- errors: "unexpected argument '--heal-report' found"
```

Target report shape (illustrative of the design, not real output):

```
Self-Healing Report (2 actions):
============================================================

[1] SUBSTITUTE at binary '/'
    Error:  division by zero
    Fix:    substituted 0 for division by zero
    Result: 0

[2] CLAMP at index_access
    Error:  index 99 out of bounds (len=3)
    Fix:    clamped to 2
    Result: 30
```

## Planned intent-driven healing (attributes are no-ops today)

The design calls for `@intent`/`@constraint`/`@fallback` attributes so a
function's declared intent can drive validation/auto-correction. **These
attributes parse today but have zero runtime effect** -- they neither
validate nor correct anything; a function annotated with them behaves
identically to the same function without the annotation.

### @intent (planned)

Would describe the purpose of a function for the (nonexistent) engine to
reason about:

```
@intent("compute the absolute distance between two points")
fn distance(a: f64, b: f64) -> f64 {
    return abs(a - b)
}
```

### @constraint (planned -- currently a silent no-op)

Design intent: declare invariants on the return value that the engine would
enforce automatically. **Verified today: it does nothing.**

```
@constraint(">= 0", "<= 100")
fn clamp_percent(value: f64) -> f64 {
    return value
}

println(to_string(clamp_percent(150.0)))  // TODAY prints 150 (unclamped). PLANNED: auto-clamped to 100.
println(to_string(clamp_percent(-10.0)))  // TODAY prints -10 (unclamped). PLANNED: auto-clamped to 0.
```

Planned constraint syntax: `>= N`, `<= N`, `> N`, `not_empty`, `not_none` /
`not_null` -- none of it is parsed for meaning or enforced today.

### @fallback (planned -- currently a silent no-op)

Design intent: a backup function the engine would call if the primary one
throws.

```
@fallback(safe_divide)
fn risky_divide(a: f64, b: f64) -> f64 {
    return a / b
}

fn safe_divide(a: f64, b: f64) -> f64 {
    if b == 0.0 {
        return 0.0
    }
    return a / b
}
```

If `risky_divide` throws today, this annotation does not intercept it --
`@fallback` has no runtime effect; catch it with an ordinary `try`/`catch`
around the call site instead.

## try/catch today (self-healing does not exist to compare against)

Use `try`/`catch` for every error path you need handled -- it is the only
mechanism that actually exists. The table below describes the eventual
division of labor ONCE self-healing lands; until then, read every
"Self-healing (default)" cell as "not available -- use `try`/`catch` plus
your own explicit guard instead":

| Situation | Use |
|-----------|-----|
| You expect a specific error and want custom handling | `try`/`catch` |
| You want to transform the error into a different error | `try`/`catch` with re-throw |
| You want arithmetic and indexing to never crash | *(planned: self-healing)* -- today, guard explicitly (`if b != 0 { ... }`, a bounds check) before the operation |
| You want to enforce output constraints on functions | *(planned: `@constraint`)* -- today, check the value yourself and `throw` if it's out of range |
| You want a fallback computation | *(planned: `@fallback`)* -- today, wrap the call in `try`/`catch` and call the fallback in the `catch` block |
| You need to recover from I/O failures | `try`/`catch` |
| You are prototyping and want nothing to crash | Guard every panic-capable operation explicitly -- there is no "nothing crashes" mode today |

## Error types in the system

When an error reaches a catch block, the variable contains different things depending on the source:

| Source | Catch variable contains |
|--------|------------------------|
| `throw "message"` | The thrown value (here, a string) |
| `throw 42` | The thrown value (here, an integer) |
| Runtime error (type mismatch, etc.) | Error message as a string |
| Built-in function failure | Error message as a string |

You can throw any value -- strings, numbers, structs. The catch block receives whatever was thrown.

## Disabling self-healing

For production code or when you want strict error behavior, disable self-healing:

```bash
kryos run --no-heal program.kry
```

With healing disabled, division by zero and index-out-of-bounds produce hard errors. Use this when you prefer crashes over silent recovery -- it depends on your use case.

## Coming from Rust

| Rust | Kryos |
|------|-------|
| `Result<T, E>` | `try`/`catch` |
| `?` operator | Not needed -- `try`/`catch` handles propagation |
| `unwrap()` / `expect()` | Not needed -- values are accessed directly |
| `panic!()` | `throw` |
| `match` on `Result` | `catch` block handles the error case |

Rust's approach is more type-safe -- errors are encoded in the type system and you must handle them. Kryos trades that rigor for simplicity: `try`/`catch` is familiar, and self-healing handles the cases you forget. If you come from Rust, you will miss `Result<T, E>` for complex error hierarchies -- but you will not miss writing `.unwrap()` on every call.

## Common mistakes

**Not catching errors from I/O operations**

File reads, network calls, and JSON parsing can all fail -- but which
mechanism reports the failure depends on which function you call. The
global builtin `file_read` **panics** (uncatchable, exit 98) if the file is
missing or unreadable; wrapping it in `try`/`catch` does **not** help, the
catch block never runs (see "What `catch` catches" above). For a
recoverable failure you can actually handle, call the throwing stdlib
wrapper `std::fs::read_file` instead:

```
// Dangerous -- PANICS if the file does not exist; try/catch cannot catch this
let content = file_read("config.txt")

// Safe -- std::fs::read_file throws on failure, so try/catch works
use std::fs::{read_file}

try {
    let content = read_file("config.txt")
    // use content
} catch e {
    println("Could not read config: " + e)
    // use defaults
}
```

If you must use the raw `file_read` builtin, guard it explicitly first
(`if file_exists(path) { ... }`) -- there is no way to recover from it
failing after the call.

**Relying on self-healing for logic errors**

Self-healing is a safety net, not a substitute for correct code. If your algorithm indexes out of bounds, the clamped value is probably wrong for your calculation. Fix the algorithm:

```
// Bad -- self-healing hides the bug
let data = [1, 2, 3]
for i in range(0, 5) {      // iterates past the array
    println(data[i])         // heals by clamping, but results are wrong
}

// Good -- correct bounds
for i in range(0, len(data)) {
    println(data[i])
}
```

**Throwing inside a catch without a wrapping try**

If you throw inside a catch block and there is no outer try, the error propagates up and crashes:

```
try {
    throw "first"
} catch e {
    throw "second"  // no outer try -- this crashes
}
```

Either add a wrapping try/catch or handle the error in the catch block without rethrowing.
