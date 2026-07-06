# Error Handling

> **Implementation Status:** `try`/`catch`/`throw` is fully implemented -- parsed, lowered to Result-enum-based control flow in MIR, and compiled through both backends. Nested try/catch, throwing any value type, and catching runtime errors all work. The **self-healing runtime** (automatic recovery from division by zero, index clamping, `@intent`, `@constraint`, `@fallback` attributes, and `--heal-report`) is a **roadmap feature** and is not yet implemented.

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

### Catching runtime errors

`catch` does not only catch explicit `throw` statements. It also catches runtime errors like type mismatches and built-in function failures:

```
try {
    let result = parse("not valid json")   // use std::json::{parse}
} catch e {
    println("Parse failed: " + e)
}
```

Any error that would normally crash the program can be caught and handled.

## The self-healing runtime (roadmap)

> **Not yet implemented.** The self-healing runtime is a planned feature. The design below describes the target behavior. Currently, division by zero, index out of bounds, and type mismatches produce hard errors caught by `try`/`catch`.

This is where Kryos will diverge from every other language. When self-healing is enabled, the runtime will automatically recover from certain classes of errors instead of crashing.

### How it works

The self-healing engine sits between your code and the runtime. When an operation fails, the engine checks whether it knows how to fix the problem. If it does, it applies the fix, logs what happened, and continues execution. Your program never sees the error.

### Recovery strategies

The engine handles these categories of errors:

**Division by zero** -- substitutes 0:

```
let x = 10 / 0  // normally a crash
// with self-healing: x = 0, logged as a heal action
```

**Index out of bounds** -- clamps to the nearest valid index:

```
let data = [10, 20, 30]
let val = data[99]  // normally a crash
// with self-healing: val = 30 (clamped to last element)
```

**Type mismatch in operations** -- coerces types:

```
let result = "5" + 10  // string + int
// with self-healing: coerces 10 to "10", result = "510"
```

**Missing key or attribute** -- returns none:

```
// accessing a field that does not exist on a struct
// with self-healing: returns none instead of crashing
```

Each recovery is deterministic -- the same problem always produces the same fix. The engine never guesses randomly.

### Heal actions

Every auto-recovery is classified as one of these actions:

| Action | What it does |
|--------|-------------|
| `RETRY` | Retry the operation (after input coercion) |
| `COERCE` | Convert types to match expectations |
| `CLAMP` | Clamp a value to a valid range |
| `FALLBACK` | Use a fallback value or function |
| `RECONSTRUCT` | Roll back to the last known good state |
| `SKIP` | Skip the failing operation safely |
| `SUBSTITUTE` | Replace with an equivalent operation |

### The heal report

Every self-healing action is logged. You can inspect what the engine did after execution:

```bash
kryos run --heal-report program.kry
```

The report shows each action taken, where it happened, what the original error was, and what fix was applied:

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

This is crucial for debugging. The program did not crash, but you should review the report to decide whether the auto-fixes match your intent.

## Intent-driven healing

For more sophisticated self-healing, you can declare what a function intends to do using attributes. The engine uses this information to validate and auto-correct results.

### @intent

Describes the purpose of a function:

```
@intent("compute the absolute distance between two points")
fn distance(a: f64, b: f64) -> f64 {
    return abs(a - b)
}
```

### @constraint

Declares invariants on the return value. The engine enforces them automatically:

```
@constraint(">= 0", "<= 100")
fn clamp_percent(value: f64) -> f64 {
    return value
}

println(clamp_percent(150.0))  // auto-clamped to 100
println(clamp_percent(-10.0))  // auto-clamped to 0
```

Supported constraint syntax: `>= N`, `<= N`, `> N`, `not_empty`, `not_none` / `not_null`.

### @fallback

Provides a backup function to call if the primary one fails:

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

If `risky_divide` throws, the engine tries `safe_divide` with the same arguments.

## try/catch vs self-healing: when to use each

The two systems serve different purposes:

| Situation | Use |
|-----------|-----|
| You expect a specific error and want custom handling | `try`/`catch` |
| You want to transform the error into a different error | `try`/`catch` with re-throw |
| You want arithmetic and indexing to never crash | Self-healing (default) |
| You want to enforce output constraints on functions | `@constraint` |
| You want a fallback computation | `@fallback` |
| You need to recover from I/O failures | `try`/`catch` |
| You are prototyping and want nothing to crash | Self-healing (default) |

A practical rule: use `try`/`catch` when the error is part of your logic (user input validation, file not found, network timeout). Let self-healing handle the unexpected edge cases (off-by-one indexes, type coercion at boundaries).

### Combining both

They work together. Self-healing runs inside try blocks too:

```
try {
    let data = [1, 2, 3]
    let x = data[10]       // self-healing clamps to data[2] = 3
    println(x)             // 3
    throw "manual error"
} catch e {
    println(e)             // manual error
}
```

Self-healing silently fixes the index error. The explicit throw is caught by the catch block. Both mechanisms operate independently.

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

File reads, network calls, and JSON parsing can all fail. Always wrap them in `try`/`catch`:

```
// Dangerous -- will crash if file does not exist
let content = file_read("config.txt")

// Safe
try {
    let content = file_read("config.txt")
    // use content
} catch e {
    println("Could not read config: " + e)
    // use defaults
}
```

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
